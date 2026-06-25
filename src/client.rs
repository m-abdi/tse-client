//! The main client: instrument/price updates, merging, and `get_prices`.
//!
//! Memory-safety note vs. the original JS:
//! the JS kept `storedPrices`, `lastdevens` and `stored` as *module-level*
//! mutable maps that were never cleared between calls, so a long-lived process
//! accumulated every instrument it ever fetched (an unbounded leak). Here all
//! of that working state is owned locally per call (`PricesState`) and dropped
//! when the call returns.

use std::collections::{HashMap, HashSet};

use regex::Regex;

use crate::adjust::{AdjustInfo, adjust};
use crate::config::{Config, PriceSettings, SYMBOL_RENAME_STRING, TRADING_SESSION_END_HOUR};
use crate::error::{Result, ResultError};
use crate::group::group;
use crate::models::{ClosingPrice, Column, Instrument, Share};
use crate::request::Requester;
use crate::storage::Storage;
use crate::util::{date_to_str, greg_to_shamsi, should_update, today};

const MERGED_SYMBOL_CONTENT: &str = "merged";

/// Per-`get_prices` working state — replaces the JS module globals
/// `storedPrices` and `lastdevens`, scoped to a single call so it cannot leak.
#[derive(Default)]
struct PricesState {
    stored_prices: HashMap<String, String>,
    lastdevens: HashMap<String, String>,
}

/// The library client. Holds configuration, the HTTP requester and the cache.
#[derive(Clone)]
pub struct Client {
    config: Config,
    storage: Storage,
    requester: Requester,
}

/// A single output cell value (string or number), used in the structured
/// (non-CSV) result.
#[derive(Debug, Clone)]
pub enum Cell {
    Text(String),
    Number(f64),
}

/// Structured per-instrument price output.
#[derive(Debug, Clone, Default)]
pub struct InstrumentPrices {
    /// header -> column of cells
    pub columns: HashMap<String, Vec<Cell>>,
    pub adjust_info: Option<AdjustInfo>,
    /// Set when this symbol was merged into another (JS `MERGED_SYMBOL_CONTENT`).
    pub merged: bool,
}

/// The result of `get_prices`.
#[derive(Debug, Default)]
pub struct PricesResult {
    pub data: Vec<Option<InstrumentPrices>>,
    pub csv: Vec<Option<String>>,
    pub error: Option<ResultError>,
}

impl Client {
    pub fn new() -> Result<Self> {
        let config = Config::default();
        Ok(Client {
            requester: Requester::new(config.api_url.clone()),
            storage: Storage::new()?,
            config,
        })
    }

    pub fn with_parts(config: Config, storage: Storage) -> Self {
        let requester = Requester::new(config.api_url.clone());
        Client {
            config,
            storage,
            requester,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    // ----- instruments -----------------------------------------------------

    fn parse_instruments_by_symbol(&self) -> Result<HashMap<String, Instrument>> {
        let rows = self.storage.get_item("tse.instruments")?;
        let mut map = HashMap::new();
        if rows.is_empty() {
            return Ok(map);
        }
        for row in rows.split('\n') {
            if row.is_empty() {
                continue;
            }
            let ins = Instrument::parse(row)?;
            map.insert(ins.symbol.clone(), ins);
        }
        Ok(map)
    }

    fn parse_shares_struct(&self) -> Result<Vec<Share>> {
        let rows = self.storage.get_item("tse.shares")?;
        let mut out = Vec::new();
        if rows.is_empty() {
            return Ok(out);
        }
        for row in rows.split('\n') {
            if row.is_empty() {
                continue;
            }
            out.push(Share::parse(row)?);
        }
        Ok(out)
    }

    fn parse_shares_raw(&self) -> Result<Vec<String>> {
        let rows = self.storage.get_item("tse.shares")?;
        if rows.is_empty() {
            return Ok(vec![]);
        }
        Ok(rows.split('\n').map(|s| s.to_string()).collect())
    }

    /// Port of JS `getLastPossibleDevens`. Returns `(NO, ID)` or a domain error.
    async fn get_last_possible_devens(
        &self,
    ) -> Result<std::result::Result<(String, String), ResultError>> {
        let mut no = String::new();
        let mut id = String::new();

        let stored = self.storage.get_item("tse.lastPossibleDevens")?;
        if !stored.is_empty() {
            let mut it = stored.split(',');
            no = it.next().unwrap_or("").to_string();
            id = it.next().unwrap_or("").to_string();
        }

        let today_str = date_to_str(today());
        let last_update = self.storage.get_item("tse.lastLPDUpdate")?;
        let today_num: i64 = today_str.parse().unwrap_or(0);
        let last_update_num: i64 = last_update.parse().unwrap_or(0);
        if today_num <= last_update_num {
            return Ok(Ok((no, id)));
        }

        if stored.is_empty() || should_update(&today_str, &no) || should_update(&today_str, &id) {
            match self.requester.last_possible_deven().await {
                Ok(res) => {
                    let re = Regex::new(r"^\d{8};\d{8}$").unwrap();
                    if !re.is_match(res.trim()) {
                        return Ok(Err(ResultError::Request {
                            title: "Invalid server response: LastPossibleDeven".into(),
                            detail: String::new(),
                        }));
                    }
                    let parts: Vec<&str> = res.trim().split(';').collect();
                    self.storage
                        .set_item("tse.lastPossibleDevens", &parts.join(","))?;
                    self.storage.set_item("tse.lastLPDUpdate", &today_str)?;
                    no = parts[0].to_string();
                    id = parts[1].to_string();
                }
                Err(e) => {
                    return Ok(Err(ResultError::Request {
                        title: "Failed request: LastPossibleDeven".into(),
                        detail: e.to_string(),
                    }));
                }
            }
        }

        Ok(Ok((no, id)))
    }

    /// Port of JS `updateInstruments`. Returns an optional domain error.
    pub async fn update_instruments(&self) -> Result<Option<ResultError>> {
        let last_update: i64 = self
            .storage
            .get_item("tse.lastInstrumentUpdate")?
            .trim()
            .parse()
            .unwrap_or(0);
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        let today_deven: i64 = date_to_str(now.date()).parse().unwrap_or(0);
        if last_update != 0
            && (today_deven <= last_update || now.hour() <= TRADING_SESSION_END_HOUR)
        {
            return Ok(None);
        }

        let mut current_shares: Option<Vec<String>> = None;
        let last_id: i64 = if last_update == 0 {
            0
        } else {
            let cs = self.parse_shares_raw()?;
            let max = cs
                .iter()
                .filter_map(|i| i.split(',').next())
                .filter_map(|s| s.parse::<i64>().ok())
                .max()
                .unwrap_or(0);
            current_shares = Some(cs);
            max
        };

        let res = match self
            .requester
            .instrument_and_share(&today_deven.to_string(), last_id)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Ok(Some(ResultError::Request {
                    title: "Failed request: InstrumentAndShare".into(),
                    detail: e.to_string(),
                }));
            }
        };
        let shares_part = res.split('@').nth(1).unwrap_or("").to_string();

        let instruments_raw = match self.requester.instrument("0").await {
            Ok(r) => r,
            Err(e) => {
                return Ok(Some(ResultError::Request {
                    title: "Failed request: Instrument".into(),
                    detail: e.to_string(),
                }));
            }
        };

        let mut wrote_instruments = false;
        if !instruments_raw.is_empty() && instruments_raw != "*" {
            let instruments = Self::dedupe_and_rename(&instruments_raw);
            self.storage.set_item("tse.instruments", &instruments)?;
            wrote_instruments = true;
        }

        let mut wrote_shares = false;
        if !shares_part.is_empty() {
            let shares_str = if let Some(cs) = current_shares.filter(|c| !c.is_empty()) {
                let mut all = cs;
                all.extend(shares_part.split(';').map(|s| s.to_string()));
                all.join("\n")
            } else {
                shares_part.replace(';', "\n")
            };
            self.storage.set_item("tse.shares", &shares_str)?;
            wrote_shares = true;
        }

        if wrote_instruments || wrote_shares {
            self.storage
                .set_item("tse.lastInstrumentUpdate", &today_deven.to_string())?;
        }

        Ok(None)
    }

    /// Port of the duplicate-symbol renaming block inside `updateInstruments`.
    fn dedupe_and_rename(instruments_raw: &str) -> String {
        let mut rows: Vec<Vec<String>> = instruments_raw
            .split(';')
            .map(|i| i.split(',').map(|s| s.to_string()).collect())
            .collect();

        // cleaned symbols for duplicate detection
        let cleaned: Vec<String> = rows
            .iter()
            .map(|r| crate::util::clean_fa(&r[5]).trim().to_string())
            .collect();

        // unique symbols that appear more than once
        let mut seen = HashSet::new();
        let mut dup_syms: Vec<String> = Vec::new();
        let mut counts: HashMap<&String, usize> = HashMap::new();
        for s in &cleaned {
            *counts.entry(s).or_insert(0) += 1;
        }
        for s in &cleaned {
            if counts[s] > 1 && seen.insert(s.clone()) {
                dup_syms.push(s.clone());
            }
        }

        let code_idx: HashMap<String, usize> = rows
            .iter()
            .enumerate()
            .map(|(j, r)| (r[0].clone(), j))
            .collect();

        for dsym in dup_syms {
            // indices of rows whose cleaned symbol equals dsym
            let mut group: Vec<usize> = cleaned
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == dsym)
                .map(|(i, _)| i)
                .collect();
            // sort by DEven (col 8) descending
            group.sort_by(|&a, &b| {
                let da: i64 = rows[a][8].parse().unwrap_or(0);
                let db: i64 = rows[b][8].parse().unwrap_or(0);
                db.cmp(&da)
            });

            for (j, &ridx) in group.iter().enumerate() {
                let orig = rows[ridx][5].clone();
                if j > 0 {
                    let postfix = format!("{}{}", SYMBOL_RENAME_STRING, j + 1);
                    // push original symbol as the 19th field
                    if rows[ridx].len() == 18 {
                        rows[ridx].push(orig.clone());
                    } else {
                        rows[ridx][18] = orig.clone();
                    }
                    rows[ridx][5] = format!("{}{}", orig.trim(), postfix);
                } else {
                    rows[ridx][5] = orig;
                }
            }
        }

        let _ = code_idx; // parity with JS (kept for clarity)

        rows.iter()
            .map(|r| r.join(","))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ----- prices update ---------------------------------------------------

    /// Port of JS `updatePrices`, operating on owned `PricesState`.
    async fn update_prices(
        &self,
        state: &mut PricesState,
        selection: &[Option<Instrument>],
        should_cache: bool,
    ) -> Result<std::result::Result<(Vec<String>, Vec<String>), ResultError>> {
        // load lastdevens from cache
        let raw = self.storage.get_item("tse.inscode_lastdeven")?;
        let mut inscodes: HashSet<String> = HashSet::new();
        if !raw.is_empty() {
            for line in raw.split('\n') {
                let mut it = line.split(',');
                if let (Some(k), Some(v)) = (it.next(), it.next()) {
                    state.lastdevens.insert(k.to_string(), v.to_string());
                    inscodes.insert(k.to_string());
                }
            }
        }

        let last_possible = match self.get_last_possible_devens().await? {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        };
        let (lpd_no, lpd_id) = last_possible;
        let first_possible_deven = "20010321";

        let mut to_update: Vec<(String, String, i32)> = Vec::new();
        for instrument in selection.iter().flatten() {
            let inscode = &instrument.ins_code;
            let market = &instrument.ymar_nsc;
            let is_not_normal = if market == "NO" { 0 } else { 1 };

            if !inscodes.contains(inscode) {
                to_update.push((
                    inscode.clone(),
                    first_possible_deven.to_string(),
                    is_not_normal,
                ));
            } else {
                let lastdeven = state.lastdevens.get(inscode).cloned().unwrap_or_default();
                let last_possible_deven = if market != "NO" { &lpd_id } else { &lpd_no };
                if lastdeven.is_empty() {
                    continue; // expired symbol
                }
                if should_update(&lastdeven, last_possible_deven) {
                    to_update.push((inscode.clone(), lastdeven, is_not_normal));
                }
            }
        }

        // load any not-yet-loaded stored prices
        let selins: HashSet<String> = selection
            .iter()
            .flatten()
            .map(|i| i.ins_code.clone())
            .collect();
        let stored_has: HashSet<String> = state.stored_prices.keys().cloned().collect();
        if stored_has.is_empty() || selins.iter().any(|i| !stored_has.contains(i)) {
            self.storage.get_items(&selins, &mut state.stored_prices)?;
        }

        if to_update.is_empty() {
            return Ok(Ok((vec![], vec![])));
        }

        let (succs, fails) = self
            .run_prices_update(state, &to_update, should_cache, &lpd_no)
            .await?;

        if !succs.is_empty() && should_cache {
            let str = state
                .lastdevens
                .iter()
                .map(|(k, v)| format!("{k},{v}"))
                .collect::<Vec<_>>()
                .join("\n");
            self.storage.set_item("tse.inscode_lastdeven", &str)?;
        }

        Ok(Ok((succs, fails)))
    }

    /// Port of the JS `pricesUpdateManager`, expressed as straightforward
    /// async/await with bounded concurrency and retries instead of the
    /// setTimeout/poll state machine.
    async fn run_prices_update(
        &self,
        state: &mut PricesState,
        to_update: &[(String, String, i32)],
        should_cache: bool,
        last_possible_deven: &str,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let chunk_size = self.config.prices_update_chunk;
        let chunk_delay = self.config.prices_update_chunk_delay;
        let retry_count = self.config.prices_update_retry_count;
        let retry_delay = self.config.prices_update_retry_delay;

        let mut succs: Vec<String> = Vec::new();
        let mut fails: HashSet<String> = HashSet::new();

        // current set of chunks to process
        let mut chunks: Vec<Vec<(String, String, i32)>> =
            to_update.chunks(chunk_size).map(|c| c.to_vec()).collect();

        let mut retries = 0u32;
        loop {
            let mut retry_chunks: Vec<Vec<(String, String, i32)>> = Vec::new();

            for (idx, chunk) in chunks.iter().enumerate() {
                if idx > 0 && chunk_delay > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(chunk_delay)).await;
                }
                let ins_codes = chunk
                    .iter()
                    .map(|(c, d, m)| format!("{c},{d},{m}"))
                    .collect::<Vec<_>>()
                    .join(";");

                match self.requester.closing_prices(&ins_codes).await {
                    Ok(resp) if is_valid_prices_response(&resp) => {
                        let parts: Vec<&str> = resp.split('@').collect();
                        for (i, item) in chunk.iter().enumerate() {
                            let inscode = &item.0;
                            let newdata = parts.get(i).copied().unwrap_or("").replace(';', "\n");
                            succs.push(inscode.clone());
                            fails.remove(inscode);

                            if !newdata.is_empty() {
                                let data = match state.stored_prices.get(inscode) {
                                    Some(old) => format!("{old}\n{newdata}"),
                                    None => newdata.clone(),
                                };
                                // last deven = second field of last row
                                if let Some(last_row) = newdata.split('\n').next_back() {
                                    if let Some(dv) = last_row.split(',').nth(1) {
                                        state.lastdevens.insert(inscode.clone(), dv.to_string());
                                    }
                                }
                                if should_cache {
                                    self.storage.set_item_async(
                                        &format!("tse.prices.{inscode}"),
                                        &data,
                                        false,
                                    )?;
                                }
                                state.stored_prices.insert(inscode.clone(), data);
                            } else {
                                state
                                    .lastdevens
                                    .insert(inscode.clone(), last_possible_deven.to_string());
                            }
                        }
                    }
                    _ => {
                        for item in chunk {
                            fails.insert(item.0.clone());
                        }
                        retry_chunks.push(chunk.clone());
                    }
                }
            }

            if retry_chunks.is_empty() || retries >= retry_count {
                break;
            }
            retries += 1;
            if retry_delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(retry_delay)).await;
            }
            // a chunk that is being retried is no longer a hard fail yet
            for ch in &retry_chunks {
                for item in ch {
                    fails.remove(&item.0);
                }
            }
            chunks = retry_chunks;
        }

        Ok((succs, fails.into_iter().collect()))
    }

    // ----- public: get_prices ---------------------------------------------

    /// Port of JS `getPrices`.
    pub async fn get_prices(
        &self,
        symbols: &[String],
        settings: &PriceSettings,
    ) -> Result<PricesResult> {
        let mut result = PricesResult::default();
        if symbols.is_empty() {
            return Ok(result);
        }

        if let Some(err) = self.update_instruments().await? {
            result.error = Some(err);
            return Ok(result);
        }

        let instruments = self.parse_instruments_by_symbol()?;
        // selection preserving order & not-found tracking
        let mut selection: Vec<Option<Instrument>> = symbols
            .iter()
            .map(|s| instruments.get(s).cloned())
            .collect();
        let not_founds: Vec<String> = symbols
            .iter()
            .zip(&selection)
            .filter(|(_, sel)| sel.is_none())
            .map(|(s, _)| s.clone())
            .collect();
        if !not_founds.is_empty() {
            result.error = Some(ResultError::IncorrectSymbol {
                symbols: not_founds,
            });
            return Ok(result);
        }

        // ----- merge similar symbols -----
        let merge = settings.merge_similar_symbols;
        let mut merges: HashMap<String, Vec<MergeItem>> = HashMap::new();
        let mut extras_index: isize = -1;

        if merge {
            let re = Regex::new(&format!("{}(\\d+)", regex::escape(SYMBOL_RENAME_STRING))).unwrap();
            let roots: HashSet<String> = instruments
                .values()
                .filter_map(|i| i.symbol_original.clone())
                .collect();
            for r in &roots {
                merges.insert(r.clone(), Vec::new());
            }
            for i in instruments.values() {
                let renamed_or_root = i
                    .symbol_original
                    .clone()
                    .unwrap_or_else(|| i.symbol.clone());
                if let Some(v) = merges.get_mut(&renamed_or_root) {
                    let order = if let Some(_orig) = &i.symbol_original {
                        re.captures(&i.symbol)
                            .and_then(|c| c.get(1))
                            .and_then(|m| m.as_str().parse::<i64>().ok())
                            .unwrap_or(1)
                    } else {
                        1
                    };
                    v.push(MergeItem {
                        sym: i.symbol.clone(),
                        code: i.ins_code.clone(),
                        order,
                    });
                }
            }
            for v in merges.values_mut() {
                v.sort_by_key(|m| m.order);
            }

            let selsyms: HashSet<String> = selection
                .iter()
                .flatten()
                .map(|i| i.symbol.clone())
                .collect();
            let mut extras: Vec<Instrument> = Vec::new();
            for ins in selection.iter().flatten() {
                if let Some(items) = merges.get(&ins.symbol) {
                    for leaf in items.iter().skip(1) {
                        if !selsyms.contains(&leaf.sym) {
                            if let Some(li) = instruments.get(&leaf.sym) {
                                extras.push(li.clone());
                            }
                        }
                    }
                }
            }
            if !extras.is_empty() {
                extras_index = selection.len() as isize;
                selection.extend(extras.into_iter().map(Some));
            }
        }

        // ----- update prices (owned state, no leak) -----
        let mut state = PricesState::default();
        let update_result = self
            .update_prices(&mut state, &selection, settings.cache)
            .await?;
        let (succs, fails) = match update_result {
            Ok(v) => v,
            Err(e) => {
                result.error = Some(e);
                return Ok(result);
            }
        };

        if !fails.is_empty() {
            let syms: HashMap<String, String> = selection
                .iter()
                .flatten()
                .map(|i| (i.ins_code.clone(), i.symbol.clone()))
                .collect();
            let fail_set: HashSet<String> = fails.iter().cloned().collect();
            result.error = Some(ResultError::IncompletePriceUpdate {
                fails: fails.iter().filter_map(|k| syms.get(k).cloned()).collect(),
                succs: succs.iter().filter_map(|k| syms.get(k).cloned()).collect(),
            });
            for sel in selection.iter_mut() {
                if let Some(ins) = sel {
                    if fail_set.contains(&ins.ins_code) {
                        *sel = None;
                    }
                }
            }
        }

        if merge && extras_index > -1 {
            selection.truncate(extras_index as usize);
        }

        // resolve columns
        let columns: Vec<Column> = settings
            .columns
            .iter()
            .map(|&i| Column::new(i, None))
            .collect::<Result<_>>()?;

        let all_shares = self.parse_shares_struct()?;

        // ----- merged price assembly -----
        let mut stored_prices_merged: HashMap<String, String> = HashMap::new();
        if merge {
            self.build_merged_prices(&state, &merges, &mut stored_prices_merged)?;
        }

        // ----- build output -----
        let textcols: HashSet<&str> = ["companycode", "namelatin", "symbol", "name"]
            .into_iter()
            .collect();

        if settings.csv {
            let headers = if settings.csv_headers {
                columns
                    .iter()
                    .map(|c| c.header.clone())
                    .collect::<Vec<_>>()
                    .join(",")
                    + "\n"
            } else {
                String::new()
            };
            for instrument in &selection {
                let Some(instrument) = instrument else {
                    result.csv.push(None);
                    continue;
                };
                let insdata = self.get_instrument_data(
                    instrument,
                    &state,
                    &merges,
                    &stored_prices_merged,
                    &all_shares,
                    settings,
                )?;
                let Some(insdata) = insdata else {
                    result.csv.push(Some(headers.clone()));
                    continue;
                };
                if insdata.merged {
                    result.csv.push(Some(MERGED_SYMBOL_CONTENT.to_string()));
                    continue;
                }
                if settings.get_adjust_info_only {
                    result.csv.push(Some(String::new()));
                    continue;
                }
                let body = insdata
                    .prices
                    .iter()
                    .map(|p| {
                        columns
                            .iter()
                            .map(|c| get_cell(c.name, instrument, p))
                            .collect::<Vec<_>>()
                            .join(&settings.csv_delimiter)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                result.csv.push(Some(format!("{headers}{body}")));
            }
        } else {
            for instrument in &selection {
                let Some(instrument) = instrument else {
                    result.data.push(None);
                    continue;
                };
                let mut out = InstrumentPrices {
                    columns: columns
                        .iter()
                        .map(|c| (c.header.clone(), Vec::new()))
                        .collect(),
                    ..Default::default()
                };
                let insdata = self.get_instrument_data(
                    instrument,
                    &state,
                    &merges,
                    &stored_prices_merged,
                    &all_shares,
                    settings,
                )?;
                let Some(insdata) = insdata else {
                    result.data.push(Some(out));
                    continue;
                };
                if insdata.merged {
                    out.merged = true;
                    result.data.push(Some(out));
                    continue;
                }
                out.adjust_info = insdata.adjust_info.clone();
                if settings.get_adjust_info_only {
                    result.data.push(Some(out));
                    continue;
                }
                for p in &insdata.prices {
                    for c in &columns {
                        let cell = get_cell(c.name, instrument, p);
                        let col = out.columns.get_mut(&c.header).unwrap();
                        if textcols.contains(c.name) {
                            col.push(Cell::Text(cell));
                        } else {
                            col.push(Cell::Number(cell.parse::<f64>().unwrap_or(f64::NAN)));
                        }
                    }
                }
                result.data.push(Some(out));
            }
        }

        Ok(result)
    }

    fn build_merged_prices(
        &self,
        state: &PricesState,
        merges: &HashMap<String, Vec<MergeItem>>,
        out: &mut HashMap<String, String>,
    ) -> Result<()> {
        for items in merges.values() {
            // collect rows by code
            let mut day_bounds: Vec<(i64, i64)> = Vec::new();
            let codes: Vec<String> = items.iter().rev().map(|i| i.code.clone()).collect();
            let latest_code = match codes.last() {
                Some(c) => c.clone(),
                None => continue,
            };
            for it in items.iter().rev() {
                if let Some(data) = state.stored_prices.get(&it.code).filter(|d| !d.is_empty()) {
                    let rows: Vec<&str> = data.split('\n').collect();
                    let first = rows
                        .first()
                        .and_then(|r| r.split(',').nth(1))
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(0);
                    let last = rows
                        .last()
                        .and_then(|r| r.split(',').nth(1))
                        .and_then(|s| s.parse::<i64>().ok())
                        .unwrap_or(0);
                    day_bounds.push((first, last));
                } else {
                    day_bounds.push((0, 0));
                }
            }

            // flatten bounds and detect overlap
            let flat: Vec<i64> = day_bounds.iter().flat_map(|(a, b)| [*a, *b]).collect();
            let overlap = flat
                .iter()
                .enumerate()
                .any(|(i, v)| i > 0 && *v < flat[i - 1]);

            if overlap {
                let fixed = self.merge_overlapping(state, &codes)?;
                out.insert(latest_code, fixed);
            } else {
                let merged = codes
                    .iter()
                    .filter_map(|c| state.stored_prices.get(c))
                    .filter(|d| !d.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                out.insert(latest_code, merged);
            }
        }
        Ok(())
    }

    /// Port of the overlap-resolution block of `getPrices` (the conflict
    /// reconciliation using trade counts and InsCode distance).
    fn merge_overlapping(&self, state: &PricesState, codes: &[String]) -> Result<String> {
        let mut a: Vec<ClosingPrice> = Vec::new();
        for code in codes {
            if let Some(data) = state.stored_prices.get(code).filter(|d| !d.is_empty()) {
                for row in data.split('\n') {
                    a.push(ClosingPrice::parse(row)?);
                }
            }
        }
        if a.is_empty() {
            return Ok(String::new());
        }

        let cp_eq = |x: &ClosingPrice, y: &ClosingPrice| x == y;
        let sum = |xs: &[i64]| xs.iter().sum::<i64>();
        let ins = |s: &str| s.parse::<i64>().unwrap_or(0);

        let mut m: HashMap<String, ClosingPrice> = HashMap::new();
        m.insert(a[0].deven.clone(), a[0].clone());

        let len = a.len();
        if len >= 2 {
            for i in 1..len - 1 {
                let prev = a[i - 1].clone();
                let curr = a[i].clone();
                let day = curr.deven.clone();
                let mut select = curr.clone();
                if let Some(existing) = m.get(&day).cloned() {
                    select = resolve_conflict(&existing, &curr, &prev, ins, sum);
                }
                m.insert(day, select);

                let has_adj =
                    curr.ins_code == prev.ins_code && curr.price_yesterday != prev.pclosing;
                if has_adj {
                    let yday = prev.deven.clone();
                    let mut select = prev.clone();
                    if let Some(existing) = m.get(&yday).cloned() {
                        if cp_eq(&existing, &prev) {
                            continue;
                        }
                        if existing.pclosing != prev.pclosing {
                            let trades = sum(&[ins(&existing.ztot_tran), ins(&prev.ztot_tran)]);
                            let mut row = prev.clone();
                            row.ztot_tran = trades.to_string();
                            select = row;
                        } else {
                            select = resolve_conflict(&existing, &prev, &curr, ins, sum);
                        }
                    }
                    m.insert(yday, select);
                }
            }
        }

        let mut fixed_rows: Vec<ClosingPrice> = m.into_values().collect();
        fixed_rows.sort_by_key(|r| r.deven.parse::<i64>().unwrap_or(0));
        Ok(fixed_rows
            .iter()
            .map(|r| r.to_csv_row())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn get_instrument_data(
        &self,
        instrument: &Instrument,
        state: &PricesState,
        merges: &HashMap<String, Vec<MergeItem>>,
        merged: &HashMap<String, String>,
        all_shares: &[Share],
        settings: &PriceSettings,
    ) -> Result<Option<InstrumentDataOut>> {
        let inscode = &instrument.ins_code;
        let sym = &instrument.symbol;

        let (prices_raw, inscodes): (Option<String>, HashSet<String>) =
            if instrument.symbol_original.is_some() {
                if settings.merge_similar_symbols {
                    return Ok(Some(InstrumentDataOut {
                        prices: vec![],
                        adjust_info: None,
                        merged: true,
                    }));
                }
                (
                    state.stored_prices.get(inscode).cloned(),
                    HashSet::from([inscode.clone()]),
                )
            } else {
                let is_root = merges.contains_key(sym);
                if is_root {
                    let codes: HashSet<String> = merges
                        .get(sym)
                        .map(|v| v.iter().map(|m| m.code.clone()).collect())
                        .unwrap_or_default();
                    (merged.get(inscode).cloned(), codes)
                } else {
                    (
                        state.stored_prices.get(inscode).cloned(),
                        HashSet::from([inscode.clone()]),
                    )
                }
            };

        let Some(prices_raw) = prices_raw.filter(|p| !p.is_empty()) else {
            return Ok(None);
        };

        let mut prices: Vec<ClosingPrice> = prices_raw
            .split('\n')
            .map(ClosingPrice::parse)
            .collect::<Result<_>>()?;

        let mut adjust_info: Option<AdjustInfo> = None;
        let should_get_info = settings.get_adjust_info || settings.get_adjust_info_only;
        if settings.adjust_prices > 0 || should_get_info {
            let related: HashMap<String, Share> = all_shares
                .iter()
                .filter(|s| inscodes.contains(&s.ins_code))
                .map(|s| (s.deven.clone(), s.clone()))
                .collect();
            let res = adjust(
                settings.adjust_prices,
                &prices,
                &related,
                settings.get_adjust_info,
                settings.get_adjust_info_only,
            );
            if settings.adjust_prices > 0 {
                prices = res.prices.unwrap_or_default();
            }
            if should_get_info {
                adjust_info = res.info;
            }
        }

        if !settings.days_without_trade {
            prices.retain(|p| p.ztot_tran.parse::<i64>().unwrap_or(0) > 0);
        }
        let start: i64 = settings.start_date.parse().unwrap_or(0);
        prices.retain(|p| p.deven.parse::<i64>().unwrap_or(0) > start);

        // Resample into weekly/monthly Jalali bars when requested. Daily is a
        // no-op, so this is free for the default path. Done last so grouping
        // sees adjusted, filtered, date-bounded rows.
        let prices = group(&prices, settings.period);

        Ok(Some(InstrumentDataOut {
            prices,
            adjust_info,
            merged: false,
        }))
    }
}

struct InstrumentDataOut {
    prices: Vec<ClosingPrice>,
    adjust_info: Option<AdjustInfo>,
    merged: bool,
}

#[derive(Debug, Clone)]
struct MergeItem {
    sym: String,
    code: String,
    order: i64,
}

// Two stable passes mirror the JS exactly; collapsing them would change the
// tie-breaking order, so the line-by-line form is intentional.
fn resolve_conflict(
    existing: &ClosingPrice,
    candidate: &ClosingPrice,
    reference: &ClosingPrice,
    ins: impl Fn(&str) -> i64,
    sum: impl Fn(&[i64]) -> i64,
) -> ClosingPrice {
    // pick higher trade count, then lower InsCode distance to `reference`
    let ref_ins = ins(&reference.ins_code);
    let candidates = [existing, candidate];
    let mut ranked: Vec<(&ClosingPrice, i64, i64)> = candidates
        .iter()
        .map(|c| (*c, ins(&c.ztot_tran), (ins(&c.ins_code) - ref_ins).abs()))
        .collect();
    // Two stable passes, exactly mirroring the JS:
    //   .sort((a,b)=>a[2]-b[2]).sort((a,b)=>b[1]-a[1])
    // i.e. order by trade-count desc, ties broken by smaller InsCode distance.
    ranked.sort_by_key(|a| a.2);
    ranked.sort_by_key(|a| std::cmp::Reverse(a.1));
    let chosen = ranked[0].0;
    let trades = sum(&[ins(&existing.ztot_tran), ins(&candidate.ztot_tran)]);
    let mut row = chosen.clone();
    row.ztot_tran = trades.to_string();
    row
}

/// Port of JS `getCell`.
fn get_cell(name: &str, instrument: &Instrument, cp: &ClosingPrice) -> String {
    match name {
        "date" => cp.deven.clone(),
        "dateshamsi" => greg_to_shamsi(&cp.deven),
        "open" => cp.price_first.clone(),
        "high" => cp.price_max.clone(),
        "low" => cp.price_min.clone(),
        "last" => cp.pdr_cot_val.clone(),
        "close" => cp.pclosing.clone(),
        "vol" => cp.qtot_tran5j.clone(),
        "count" => cp.ztot_tran.clone(),
        "value" => cp.qtot_cap.clone(),
        "yesterday" => cp.price_yesterday.clone(),
        "symbol" => instrument.symbol.clone(),
        "name" => instrument.name.clone(),
        "namelatin" => instrument.latin_name.clone(),
        "companycode" => instrument.company_code.clone(),
        _ => String::new(),
    }
}

/// JS regex `/^[\d.,;@-]+$/` test (or empty string).
fn is_valid_prices_response(resp: &str) -> bool {
    if resp.is_empty() {
        return true;
    }
    resp.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ';' | '@' | '-'))
}
