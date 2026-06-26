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
    /// Column indices to include in the output.
    ///
    /// Maps to `columnList` indices. Default: `[0, 2, 3, 4, 5, 6, 7, 8, 9]`
    pub columns: Vec<usize>,

    /// Price adjustment mode applied to returned data.
    ///
    /// - `0`: No adjustment (`بدون تعدیل`)
    /// - `1`: Capital increase + dividends (`افزایش سرمایه + سود نقدی`)
    /// - `2`: Capital increase only (`افزایش سرمایه`)
    pub adjust_prices: u8,

    /// If `true`, an `adjust_info` payload is appended to each `ClosingPrice`
    /// containing all events needed to manually re-apply price adjustment.
    ///
    /// Default: `false`
    pub get_adjust_info: bool,

    /// If `true`, returns only the `adjust_info` payload without any
    /// `ClosingPrice` data. Implies `get_adjust_info`.
    ///
    /// Default: `false`
    pub get_adjust_info_only: bool,

    /// If `true`, rows with zero trades are included in the output.
    ///
    /// Default: `false`
    pub days_without_trade: bool,

    /// Start date for price data in `YYYYMMDD` format (e.g., `"20230101"`).
    /// Only prices with dates greater than this value will be included.
    ///
    /// Min: `"20010321"`. Default: `"20010321"`
    pub start_date: String,

    /// End date for price data in `YYYYMMDD` format (e.g., `"20231231"`).
    /// Only prices with dates less than or equal to this value will be included.
    /// If empty, no end date filtering is applied.
    ///
    /// `start_date` should be less than or equal to `end_date` when both are specified.
    pub end_date: String,

    /// If `true`, data for similar renamed symbols is merged into a single series.
    ///
    /// Default: `true`
    pub merge_similar_symbols: bool,

    /// If `true`, downloaded data is persisted to the local cache directory.
    ///
    /// Default: `true`
    pub cache: bool,

    /// If `true`, each symbol's result is returned as a CSV string instead
    /// of a structured `ClosingPrice` object.
    ///
    /// Default: `false`
    pub csv: bool,

    /// If `true`, a header row is prepended to each CSV result.
    /// Has no effect when `csv` is `false`.
    ///
    /// Default: `true`
    pub csv_headers: bool,

    /// Cell delimiter character used when generating CSV results.
    /// Has no effect when `csv` is `false`.
    ///
    /// Default: `","`
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
            end_date: String::new(),
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
