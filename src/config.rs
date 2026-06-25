//! Runtime configuration and per-call settings.
//!
//! The original JS exposed these as mutable module-level globals with
//! validating getters/setters. In Rust we use an owned `Config` struct on the
//! client instead of `static mut`, which is both safer and avoids cross-call
//! state bleed.

use crate::group::Period;

/// Tunable client configuration (the JS `*_UPDATE_*` globals and URLs).
#[derive(Debug, Clone)]
pub struct Config {
    pub api_url: String,
    pub update_interval: i64,
    pub prices_update_chunk: usize,
    pub prices_update_chunk_delay: u64,
    pub prices_update_retry_count: u32,
    pub prices_update_retry_delay: u64,

    pub intraday_update_chunk_delay: u64,
    pub intraday_update_chunk_max_wait: u64,
    pub intraday_update_retry_count: u32,
    pub intraday_update_retry_delay: u64,
    pub intraday_update_servers: Vec<i32>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            api_url: crate::request::DEFAULT_API_URL.to_string(),
            update_interval: 1,
            prices_update_chunk: 50,
            prices_update_chunk_delay: 300,
            prices_update_retry_count: 3,
            prices_update_retry_delay: 1000,

            intraday_update_chunk_delay: 100,
            intraday_update_chunk_max_wait: 60_000,
            intraday_update_retry_count: 3,
            intraday_update_retry_delay: 1000,
            intraday_update_servers: vec![-1, 0],
        }
    }
}

/// Per-call settings for `get_prices` (JS `defaultSettings`).
#[derive(Debug, Clone)]
pub struct PriceSettings {
    pub columns: Vec<usize>,
    pub adjust_prices: u8,
    pub get_adjust_info: bool,
    pub get_adjust_info_only: bool,
    pub days_without_trade: bool,
    pub start_date: String,
    pub merge_similar_symbols: bool,
    pub cache: bool,
    pub csv: bool,
    pub csv_headers: bool,
    pub csv_delimiter: String,
    /// Resampling timeframe for the returned rows. Daily (default) keeps the
    /// rows as-is; Weekly/Monthly aggregate them using the Jalali calendar.
    pub period: Period,
}

impl Default for PriceSettings {
    fn default() -> Self {
        PriceSettings {
            columns: vec![0, 2, 3, 4, 5, 6, 7, 8, 9],
            adjust_prices: 0,
            get_adjust_info: false,
            get_adjust_info_only: false,
            days_without_trade: false,
            start_date: "20010321".to_string(),
            merge_similar_symbols: true,
            cache: true,
            csv: false,
            csv_headers: true,
            csv_delimiter: ",".to_string(),
            period: Period::Daily,
        }
    }
}

/// Per-call settings for `get_intraday` (JS `itdDefaultSettings`).
#[derive(Debug, Clone)]
pub struct IntradaySettings {
    pub start_date: String,
    pub end_date: String,
    pub cache: bool,
    pub gzip: bool,
    pub re_update_no_trades: bool,
    pub update_only: bool,
    pub chunk_delay: u64,
    pub chunk_max_wait: u64,
    pub retry_count: u32,
    pub retry_delay: u64,
    pub servers: Vec<i32>,
}

impl Default for IntradaySettings {
    fn default() -> Self {
        IntradaySettings {
            start_date: "20010321".to_string(),
            end_date: String::new(),
            cache: true,
            gzip: true,
            re_update_no_trades: false,
            update_only: false,
            chunk_delay: 100,
            chunk_max_wait: 60_000,
            retry_count: 3,
            retry_delay: 1000,
            servers: vec![-1, 0],
        }
    }
}

pub const SYMBOL_RENAME_STRING: &str = "-ق";
pub const TRADING_SESSION_END_HOUR: u8 = 16;
