//! File-based cache storage, replacing the Node `fs`/`zlib` branch of the
//! original JS `storage` object.
//!
//! The original JS keeps this synchronous under the hood (the `*Async`
//! variants merely wrap synchronous `fs` calls in a Promise), so this port is
//! plain synchronous `std::fs` + `flate2`. Call sites that run inside the async
//! download pipeline can offload these via `tokio::task::spawn_blocking` if a
//! particular write is large enough to matter.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::error::Result;

/// Synchronous, file-backed cache.
#[derive(Debug, Clone)]
pub struct Storage {
    datadir: PathBuf,
}

fn gzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data)?;
    enc.finish()
}

fn gunzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut dec = GzDecoder::new(data);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    Ok(out)
}

impl Storage {
    /// Create a `Storage` resolving the cache dir exactly like the JS:
    /// honor `~/.tse` tracker file if present, else default to `~/tse-cache`.
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let default_dir = home.join("tse-cache");
        let tracker = home.join(".tse");

        let datadir = if tracker.exists() {
            let recorded = fs::read_to_string(&tracker).unwrap_or_default();
            let p = PathBuf::from(recorded.trim());
            if p.is_dir() { p } else { default_dir.clone() }
        } else {
            if !default_dir.exists() {
                fs::create_dir_all(&default_dir)?;
            }
            fs::write(&tracker, default_dir.to_string_lossy().as_bytes())?;
            default_dir.clone()
        };

        if !datadir.exists() {
            fs::create_dir_all(&datadir)?;
        }
        Ok(Storage { datadir })
    }

    /// Create a `Storage` rooted at an explicit directory (useful for tests).
    pub fn with_dir<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let datadir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&datadir)?;
        Ok(Storage { datadir })
    }

    /// The current cache directory (JS `CACHE_DIR`).
    pub fn cache_dir(&self) -> &Path {
        &self.datadir
    }

    fn resolve(&self, key: &str) -> PathBuf {
        let key = key.strip_prefix("tse.").unwrap_or(key);
        if let Some(rest) = key.strip_prefix("prices.") {
            self.datadir.join("prices").join(format!("{rest}.csv"))
        } else {
            self.datadir.join(format!("{key}.csv"))
        }
    }

    /// Read a value, creating an empty file if missing (JS `getItem`).
    pub fn get_item(&self, key: &str) -> Result<String> {
        let file = self.resolve(key);
        if let Some(parent) = file.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        if !file.exists() {
            fs::write(&file, b"")?;
            return Ok(String::new());
        }
        Ok(fs::read_to_string(&file)?)
    }

    /// Write a value (JS `setItem`).
    pub fn set_item(&self, key: &str, value: &str) -> Result<()> {
        let file = self.resolve(key);
        if let Some(parent) = file.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(&file, value.as_bytes())?;
        Ok(())
    }

    fn resolve_zipped(&self, key: &str, zip: bool) -> PathBuf {
        let mut p = self.resolve(key);
        if zip {
            let name = format!("{}.gz", p.file_name().unwrap().to_string_lossy());
            p.set_file_name(name);
        }
        p
    }

    /// Async-equivalent read (JS `getItemAsync`). Optionally gunzips.
    pub fn get_item_async(&self, key: &str, zip: bool) -> Result<String> {
        let file = self.resolve_zipped(key, zip);
        if let Some(parent) = file.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        if !file.exists() {
            if !zip {
                fs::write(&file, b"")?;
            }
            return Ok(String::new());
        }
        let raw = fs::read(&file)?;
        if zip {
            Ok(String::from_utf8_lossy(&gunzip(&raw)?).into_owned())
        } else {
            Ok(String::from_utf8_lossy(&raw).into_owned())
        }
    }

    /// Async-equivalent write (JS `setItemAsync`). Optionally gzips.
    pub fn set_item_async(&self, key: &str, value: &str, zip: bool) -> Result<()> {
        let file = self.resolve_zipped(key, zip);
        if let Some(parent) = file.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        if zip {
            fs::write(&file, gzip(value.as_bytes())?)?;
        } else {
            fs::write(&file, value.as_bytes())?;
        }
        Ok(())
    }

    /// Load the cached price CSVs for the selected instruments (JS `getItems`).
    /// Only the requested inscodes are read into `result`.
    pub fn get_items(
        &self,
        selins: &HashSet<String>,
        result: &mut HashMap<String, String>,
    ) -> Result<()> {
        let dir = self.datadir.join("prices");
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
            return Ok(());
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let key = name.strip_suffix(".csv").unwrap_or(&name).to_string();
            if !selins.contains(&key) {
                continue;
            }
            result.insert(key, fs::read_to_string(entry.path())?);
        }
        Ok(())
    }

    /// Intraday: read stored devens for selected instruments (JS `itd.getItems`).
    /// When `full` is true, the gzipped bytes are returned; otherwise each
    /// deven maps to an empty marker (presence only).
    pub fn itd_get_items(
        &self,
        selins: &HashSet<String>,
        full: bool,
    ) -> Result<HashMap<String, HashMap<String, ItdValue>>> {
        let dir = self.datadir.join("intraday");
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
            return Ok(HashMap::new());
        }
        let mut result: HashMap<String, HashMap<String, ItdValue>> = HashMap::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if !entry.path().is_dir() {
                continue;
            }
            let inscode = entry.file_name().to_string_lossy().into_owned();
            if !selins.contains(&inscode) {
                continue;
            }
            let mut files: HashMap<String, ItdValue> = HashMap::new();
            for f in fs::read_dir(entry.path())? {
                let f = f?;
                let fname = f.file_name().to_string_lossy().into_owned();
                let (deven, zipped) = match fname.strip_suffix(".gz") {
                    Some(stem) => (stem.to_string(), true),
                    None => (fname.clone(), false),
                };
                if full {
                    let raw = fs::read(f.path())?;
                    let val = if zipped {
                        ItdValue::Gzipped(raw)
                    } else {
                        ItdValue::Plain(String::from_utf8_lossy(&raw).into_owned())
                    };
                    files.insert(deven, val);
                } else {
                    files.insert(deven, ItdValue::Present);
                }
            }
            result.insert(inscode, files);
        }
        Ok(result)
    }

    /// Intraday: write one instrument's deven map (JS `itd.setItem`).
    /// Values equal to "N/A" are stored uncompressed; everything else gzipped.
    pub fn itd_set_item(&self, key: &str, obj: &HashMap<String, ItdValue>) -> Result<()> {
        let key = key.strip_prefix("tse.").unwrap_or(key);
        let dir = self.datadir.join("intraday").join(key);
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        for (deven, val) in obj {
            match val {
                ItdValue::Plain(s) if s == "N/A" => {
                    fs::write(dir.join(deven), s.as_bytes())?;
                }
                ItdValue::Plain(s) => {
                    fs::write(dir.join(format!("{deven}.gz")), gzip(s.as_bytes())?)?;
                }
                ItdValue::Gzipped(bytes) => {
                    fs::write(dir.join(format!("{deven}.gz")), bytes)?;
                }
                ItdValue::Present => {}
            }
        }
        Ok(())
    }
}

/// A stored intraday value: either a presence marker, raw gzipped bytes, or
/// plain text (e.g. "N/A").
#[derive(Debug, Clone)]
pub enum ItdValue {
    Present,
    Plain(String),
    Gzipped(Vec<u8>),
}

impl ItdValue {
    /// Decompress to text when needed (JS `unzip`).
    pub fn to_text(&self) -> Option<String> {
        match self {
            ItdValue::Plain(s) => Some(s.clone()),
            ItdValue::Gzipped(b) => gunzip(b)
                .ok()
                .map(|v| String::from_utf8_lossy(&v).into_owned()),
            ItdValue::Present => None,
        }
    }
}
