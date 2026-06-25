//! Error types for the crate.

use thiserror::Error;

/// Top-level error type returned by the library.
#[derive(Error, Debug)]
pub enum Error {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid server response: {0}")]
    InvalidResponse(String),

    #[error("failed request: {title}: {detail}")]
    FailedRequest { title: String, detail: String },

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("invalid url: {0}")]
    InvalidUrl(String),
}

/// Domain errors that mirror the JS `result.error` object shapes.
/// These are *not* fatal `Result::Err` values; they are returned inside
/// the successful result so callers can inspect partial outcomes.
#[derive(Debug, Clone)]
pub enum ResultError {
    /// code 1 — a server/request failure occurred.
    Request { title: String, detail: String },
    /// code 2 — one or more requested symbols were not found.
    IncorrectSymbol { symbols: Vec<String> },
    /// code 3 — prices for some symbols failed to update.
    IncompletePriceUpdate {
        fails: Vec<String>,
        succs: Vec<String>,
    },
    /// code 4 — intraday data for some symbols failed to update.
    IncompleteIntradayUpdate {
        fails: std::collections::HashMap<String, Vec<String>>,
        succs: std::collections::HashMap<String, Vec<String>>,
    },
}

impl ResultError {
    /// The numeric error code matching the original JS API.
    pub fn code(&self) -> u8 {
        match self {
            ResultError::Request { .. } => 1,
            ResultError::IncorrectSymbol { .. } => 2,
            ResultError::IncompletePriceUpdate { .. } => 3,
            ResultError::IncompleteIntradayUpdate { .. } => 4,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
