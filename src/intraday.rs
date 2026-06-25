//! Intraday data fetching, ported from the JS `getIntraday`, `extractAndStore`
//! and `itdUpdateManager`.
//!
//! As with prices, the JS kept a module-level mutable `stored` map that was
//! never cleared between calls. Here the working map is owned per call.

use std::collections::{HashMap, HashSet};

use crate::client::Client;
use crate::config::IntradaySettings;
use crate::error::{Error, Result, ResultError};
use crate::request::intraday_url;
use crate::storage::ItdValue;
use crate::util::clean_fa;

/// Per-symbol intraday payload: a list of `(deven, payload)` pairs, where
/// `payload` is the decompressed text unless `gzip` was requested.
pub type SymbolIntraday = Vec<(String, Option<ItdValue>)>;

/// Result of `get_intraday`.
#[derive(Debug, Default)]
pub struct IntradayResult {
    /// For each requested symbol (in order), its intraday payload (if present).
    pub data: Vec<Option<SymbolIntraday>>,
    pub error: Option<ResultError>,
}

/// Parse a `var XData=[ ... ];` block out of the page text into JSON rows.
fn parse_raw(separator: &str, text: &str) -> Result<serde_json::Value> {
    let after = text
        .split(separator)
        .nth(1)
        .ok_or_else(|| Error::InvalidData(format!("missing block: {separator}")))?;
    let inner = after.split("];").next().unwrap_or("");
    let json = format!("[{}]", inner.replace('\'', "\""));
    Ok(serde_json::from_str(&json)?)
}

fn cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Port of JS `extractAndStore` for a single (deven, text) pair: returns the
/// assembled, newline-joined record (uncompressed).
fn extract_record(text: &str) -> Result<String> {
    if text == "N/A" {
        return Ok("N/A".to_string());
    }
    let closing = parse_raw("var ClosingPriceData=[", text)?;
    let best_limit = parse_raw("var BestLimitData=[", text)?;
    let intra_trade = parse_raw("var IntraTradeData=[", text)?;
    let client_type = parse_raw("var ClientTypeData=[", text)?;
    let inst_state = parse_raw("var InstrumentStateData=[", text)?;
    let static_tresh = parse_raw("var StaticTreshholdData=[", text)?;
    let inst_simple = parse_raw("var InstSimpleData=[", text)?;
    let share_holder = parse_raw("var ShareHolderData=[", text)?;

    let row_cols = |row: &serde_json::Value, idxs: &[usize]| -> String {
        idxs.iter()
            .map(|&i| row.get(i).map(cell).unwrap_or_default())
            .collect::<Vec<_>>()
            .join(",")
    };

    // price
    let price = arr(&closing)
        .iter()
        .map(|r| row_cols(r, &[12, 2, 3, 4, 6, 7, 8, 9, 10, 11]))
        .collect::<Vec<_>>()
        .join("\n");

    // order
    let order = arr(&best_limit)
        .iter()
        .map(|r| row_cols(r, &[0, 1, 2, 3, 4, 5, 6, 7]))
        .collect::<Vec<_>>()
        .join("\n");

    // trade (convert HH:MM:SS to int, sort by time)
    let mut trade_rows: Vec<Vec<String>> = arr(&intra_trade)
        .iter()
        .map(|r| {
            let time_str = r.get(1).map(cell).unwrap_or_default();
            let parts: Vec<&str> = time_str.split(':').collect();
            let h: i64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            let m: i64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let s: i64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let timeint = (h * 10000 + m * 100 + s).to_string();
            // coli = [1,0,2,3,4] with row[1] replaced by timeint
            vec![
                timeint,
                r.get(0).map(cell).unwrap_or_default(),
                r.get(2).map(cell).unwrap_or_default(),
                r.get(3).map(cell).unwrap_or_default(),
                r.get(4).map(cell).unwrap_or_default(),
            ]
        })
        .collect();
    trade_rows.sort_by_key(|r| r[0].parse::<i64>().unwrap_or(0));
    let trade = trade_rows
        .iter()
        .map(|r| r.join(","))
        .collect::<Vec<_>>()
        .join("\n");

    // client
    let ct = arr(&client_type);
    let client = [
        4, 0, 12, 16, 8, 6, 2, 14, 18, 10, 5, 1, 13, 17, 9, 7, 3, 15, 19, 11, 20,
    ]
    .iter()
    .map(|&i| ct.get(i).map(cell).unwrap_or_default())
    .collect::<Vec<_>>()
    .join(",");

    // misc
    let a = arr(&inst_state);
    let b = arr(&static_tresh);
    let state = a
        .first()
        .and_then(|r| r.get(2))
        .map(cell)
        .unwrap_or_default();
    let (daymin, daymax) = if b.len() > 1 {
        (
            b[1].get(2).map(cell).unwrap_or_default(),
            b[1].get(1).map(cell).unwrap_or_default(),
        )
    } else {
        (String::new(), String::new())
    };
    let is = arr(&inst_simple);
    let flow = is.get(4).map(cell).unwrap_or_default();
    let basevol = is.get(9).map(cell).unwrap_or_default();
    let misc = [basevol, flow, daymin, daymax, state].join(",");

    // shareholder
    let shareholder = arr(&share_holder)
        .iter()
        .filter(|r| r.get(4).map(|v| !cell(v).is_empty()).unwrap_or(false))
        .map(|r| {
            let arrow = match cell(&r[4]).as_str() {
                "ArrowUp" => "+",
                "ArrowDown" => "-",
                _ => "",
            }
            .to_string();
            let name = clean_fa(&cell(r.get(5).unwrap_or(&serde_json::Value::Null)));
            // coli = [2,3,4,0,5] where col4 replaced by arrow, col5 cleaned
            [
                r.get(2).map(cell).unwrap_or_default(),
                r.get(3).map(cell).unwrap_or_default(),
                arrow,
                r.get(0).map(cell).unwrap_or_default(),
                name,
            ]
            .join(",")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut file = vec![price, order, trade, client, misc];
    if !shareholder.is_empty() {
        file.push(shareholder);
    }
    Ok(file.join("\n\n"))
}

fn arr(v: &serde_json::Value) -> Vec<serde_json::Value> {
    v.as_array().cloned().unwrap_or_default()
}

impl Client {
    /// Port of JS `getIntraday`.
    pub async fn get_intraday(
        &self,
        symbols: &[String],
        settings: &IntradaySettings,
    ) -> Result<IntradayResult> {
        let mut result = IntradayResult::default();
        if symbols.is_empty() {
            return Ok(result);
        }

        if let Some(err) = self.update_instruments().await? {
            result.error = Some(err);
            return Ok(result);
        }

        let instruments = self.get_instruments().await?;
        // index by symbol
        let by_symbol: HashMap<String, &crate::models::Instrument> = instruments
            .values()
            .map(|i| (i.symbol.clone(), i))
            .collect();

        let selection: Vec<Option<crate::models::Instrument>> = symbols
            .iter()
            .map(|s| by_symbol.get(s).map(|i| (*i).clone()))
            .collect();
        let not_founds: Vec<String> = symbols
            .iter()
            .zip(&selection)
            .filter(|(_, s)| s.is_none())
            .map(|(s, _)| s.clone())
            .collect();
        if !not_founds.is_empty() {
            result.error = Some(ResultError::IncorrectSymbol {
                symbols: not_founds,
            });
            return Ok(result);
        }

        let selins: HashSet<String> = selection
            .iter()
            .flatten()
            .map(|i| i.ins_code.clone())
            .collect();

        // We rely on cached inscode_devens; price update path is reused via
        // get_prices' machinery only when needed. For brevity and to keep the
        // intraday device list correct, read it from cache.
        let stored_raw = self.storage().get_item_async("tse.inscode_devens", false)?;
        let mut stored_inscode_devens: HashMap<String, Vec<i64>> = HashMap::new();
        if !stored_raw.is_empty() {
            for line in stored_raw.split('\n') {
                let mut it = line.split(';');
                if let (Some(i), Some(d)) = (it.next(), it.next()) {
                    let devens = d.split(',').filter_map(|x| x.parse::<i64>().ok()).collect();
                    stored_inscode_devens.insert(i.to_string(), devens);
                }
            }
        }

        let start_date: i64 = settings.start_date.parse().unwrap_or(0);
        let end_date: i64 = settings.end_date.parse().unwrap_or(0);

        let in_range = |i: i64| -> bool {
            if end_date != 0 {
                i >= start_date && i <= end_date
            } else {
                i >= start_date
            }
        };

        let asked_inscode_devens: Vec<(String, Vec<i64>)> = selins
            .iter()
            .map(|inscode| {
                let devens = stored_inscode_devens
                    .get(inscode)
                    .map(|all| all.iter().copied().filter(|d| in_range(*d)).collect())
                    .unwrap_or_default();
                (inscode.clone(), devens)
            })
            .collect();

        // load stored intraday data (owned, no global leak)
        let mut stored = self.storage().itd_get_items(
            &selins,
            if settings.update_only {
                settings.re_update_no_trades
            } else {
                true
            },
        )?;

        // determine what to update
        let mut to_update: Vec<(String, Vec<i64>)> = Vec::new();
        for (inscode, devens) in &asked_inscode_devens {
            if devens.is_empty() {
                continue;
            }
            match stored.get(inscode) {
                None => to_update.push((inscode.clone(), devens.clone())),
                Some(have) => {
                    let need: Vec<i64> = if settings.re_update_no_trades {
                        devens
                            .iter()
                            .copied()
                            .filter(|d| {
                                match have.get(&d.to_string()).and_then(|v| v.to_text()) {
                                    None => true,
                                    Some(text) => {
                                        // trade is the 3rd block (index 2)
                                        text.split("\n\n")
                                            .nth(2)
                                            .map(|t| t.is_empty())
                                            .unwrap_or(true)
                                    }
                                }
                            })
                            .collect()
                    } else {
                        devens
                            .iter()
                            .copied()
                            .filter(|d| !have.contains_key(&d.to_string()))
                            .collect()
                    };
                    if !need.is_empty() {
                        to_update.push((inscode.clone(), need));
                    }
                }
            }
        }

        let mut servers = settings.servers.clone();
        if servers.is_empty() || servers.iter().any(|&s| s < 0 && s != -1) {
            servers = self.config().intraday_update_servers.clone();
        }

        if !to_update.is_empty() {
            let (succs, fails) = self
                .run_intraday_update(&mut stored, &to_update, settings, &servers)
                .await?;

            if !fails.is_empty() {
                let k: HashMap<String, String> = selection
                    .iter()
                    .flatten()
                    .map(|i| (i.ins_code.clone(), i.symbol.clone()))
                    .collect();
                let group = |pairs: &[(String, String)]| -> HashMap<String, Vec<String>> {
                    let mut o: HashMap<String, Vec<String>> = HashMap::new();
                    for (i, d) in pairs {
                        let sym = k.get(i).cloned().unwrap_or_else(|| i.clone());
                        o.entry(sym).or_default().push(d.clone());
                    }
                    o
                };
                result.error = Some(ResultError::IncompleteIntradayUpdate {
                    fails: group(&fails),
                    succs: group(&succs),
                });
            }
        }

        if !settings.update_only {
            result.data = asked_inscode_devens
                .iter()
                .map(|(inscode, devens)| {
                    stored.get(inscode).map(|instr| {
                        devens
                            .iter()
                            .map(|deven| {
                                let key = deven.to_string();
                                let val = instr.get(&key).cloned();
                                if settings.gzip {
                                    (key, val)
                                } else {
                                    let plain = val.and_then(|v| match &v {
                                        ItdValue::Present => None,
                                        ItdValue::Plain(_) => Some(v.clone()),
                                        ItdValue::Gzipped(_) => v.to_text().map(ItdValue::Plain),
                                    });
                                    (key, plain)
                                }
                            })
                            .collect()
                    })
                })
                .collect();
        }

        Ok(result)
    }

    /// Port of JS `itdUpdateManager`, as bounded async/await with retries.
    /// Returns (succs, fails) as (inscode, deven) string pairs.
    async fn run_intraday_update(
        &self,
        stored: &mut HashMap<String, HashMap<String, ItdValue>>,
        to_update: &[(String, Vec<i64>)],
        settings: &IntradaySettings,
        servers: &[i32],
    ) -> Result<(Vec<(String, String)>, Vec<(String, String)>)> {
        let chunk_delay = settings.chunk_delay;
        let retry_count = settings.retry_count;
        let retry_delay = settings.retry_delay;
        let should_cache = settings.cache;

        // flatten to (server_idx, inscode, deven)
        let mut chunks: Vec<(usize, String, String)> = Vec::new();
        for (inscode, devens) in to_update {
            for d in devens {
                chunks.push((0usize, inscode.clone(), d.to_string()));
            }
        }

        let mut succs: Vec<(String, String)> = Vec::new();
        let mut fails: HashSet<(String, String)> = HashSet::new();
        // track newly extracted instruments to persist into instruments.intraday
        let mut extracted_ins: HashMap<String, String> = HashMap::new();
        // last deven per inscode for the InstSimple extraction step
        let mut inslastdeven: HashMap<String, String> = HashMap::new();
        for (inscode, devens) in to_update {
            if let Some(last) = devens.last() {
                inslastdeven.insert(inscode.clone(), last.to_string());
            }
        }

        let mut retries = 0u32;
        loop {
            let mut retry_chunks: Vec<(usize, String, String)> = Vec::new();

            for (i, (srv_idx, inscode, deven)) in chunks.iter().enumerate() {
                if i > 0 && chunk_delay > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(chunk_delay)).await;
                }
                let server = servers[*srv_idx % servers.len()];
                let url = intraday_url(server, inscode, deven);
                let outcome = self.fetch_intraday(&url, settings.chunk_max_wait).await;

                match outcome {
                    Some(text) => {
                        fails.remove(&(inscode.clone(), deven.clone()));
                        succs.push((inscode.clone(), deven.clone()));

                        // extract InstSimple for instrument list when needed
                        let is_last = inslastdeven
                            .get(inscode)
                            .map(|s| s == deven)
                            .unwrap_or(false);
                        if is_last && text != "N/A" {
                            if let Some(after) = text.split("var InstSimpleData=").nth(1) {
                                let block = after.split(';').next().unwrap_or("");
                                let json = block.replace('\'', "\"");
                                if let Ok(serde_json::Value::Array(row)) =
                                    serde_json::from_str::<serde_json::Value>(&json)
                                {
                                    let mut parts = vec![inscode.clone()];
                                    parts.extend(row.iter().map(cell));
                                    extracted_ins.insert(inscode.clone(), parts.join(","));
                                }
                            }
                        }

                        // extract and store this record
                        match extract_record(&text) {
                            Ok(record) => {
                                let entry = stored.entry(inscode.clone()).or_default();
                                entry.insert(deven.clone(), ItdValue::Plain(record));
                                if should_cache {
                                    let single: HashMap<String, ItdValue> = entry
                                        .iter()
                                        .filter(|(_, v)| !matches!(v, ItdValue::Present))
                                        .map(|(k, v)| (k.clone(), v.clone()))
                                        .collect();
                                    self.storage().itd_set_item(inscode, &single)?;
                                }
                            }
                            Err(_) => {
                                // treat parse failure as a soft failure to retry
                                fails.insert((inscode.clone(), deven.clone()));
                                retry_chunks.push((
                                    next_server(servers, *srv_idx),
                                    inscode.clone(),
                                    deven.clone(),
                                ));
                            }
                        }
                    }
                    None => {
                        fails.insert((inscode.clone(), deven.clone()));
                        retry_chunks.push((
                            next_server(servers, *srv_idx),
                            inscode.clone(),
                            deven.clone(),
                        ));
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
            for (_, inscode, deven) in &retry_chunks {
                fails.remove(&(inscode.clone(), deven.clone()));
            }
            chunks = retry_chunks;
        }

        // persist instrument list for intraday (JS does this in poll())
        if !extracted_ins.is_empty() {
            let mut current = self.get_intraday_instruments_raw()?;
            current.extend(extracted_ins);
            let joined = current.values().cloned().collect::<Vec<_>>().join("\n");
            self.storage()
                .set_item("tse.instruments.intraday", &joined)?;
        }

        Ok((succs, fails.into_iter().collect()))
    }

    fn get_intraday_instruments_raw(&self) -> Result<HashMap<String, String>> {
        let rows = self.storage().get_item("tse.instruments.intraday")?;
        let mut map = HashMap::new();
        if rows.is_empty() {
            return Ok(map);
        }
        for row in rows.split('\n').filter(|r| !r.is_empty()) {
            if let Some(code) = row.split(',').next() {
                map.insert(code.to_string(), row.to_string());
            }
        }
        Ok(map)
    }

    /// Fetch one intraday page; returns `Some(text)` on a usable page,
    /// `Some("N/A")` when the server reports no data, or `None` on failure.
    async fn fetch_intraday(&self, url: &str, max_wait: u64) -> Option<String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(max_wait.max(1)))
            .build()
            .ok()?;
        let res = client.get(url).send().await.ok()?;
        if res.status().as_u16() != 200 {
            return None;
        }
        let text = res.text().await.ok()?;
        if text.contains(
            "Object moved to <a href=\"/GeneralError.aspx?aspxerrorpath=/Loader.aspx\">here</a>",
        ) {
            return Some("N/A".to_string());
        }
        // verify the page is up to date
        let after = text.split("var StaticTreshholdData").nth(1)?;
        let up_to_date = [
            "ClosingPrice",
            "IntraDayPrice",
            "IntraTrade",
            "ClientType",
            "BestLimit",
        ]
        .iter()
        .any(|name| {
            after
                .split(&format!("var {name}Data=["))
                .nth(1)
                .and_then(|s| s.split("];").next())
                .map(|s| !s.is_empty())
                .unwrap_or(false)
        });
        if up_to_date { Some(text) } else { None }
    }
}

fn next_server(servers: &[i32], current_idx: usize) -> usize {
    (current_idx + 1) % servers.len()
}
