//! # tse-client
//!
//! A Rust port of the `tse-client` npm package: a client for fetching stock
//! data from the Tehran Stock Exchange (TSETMC).
//!
//! The public surface mirrors the original JS API:
//! - [`Client::get_prices`] ↔ `getPrices`
//! - [`Client::get_instruments`] ↔ `getInstruments`
//! - [`Client::get_intraday`] ↔ `getIntraday`
//! - [`Client::get_intraday_instruments`] ↔ `getIntradayInstruments`
//!
//! Unlike the JS version, all per-call working state is owned locally and
//! dropped when a call returns, so there is no cross-call accumulation.
//!
//! # Example
//!
//! ```no_run
//! use tse_client::{Client, IntradaySettings, PriceSettings};
//!
//! # async fn run() -> tse_client::Result<()> {
//! let client = Client::new()?;
//!
//! // getPrices
//! let res = client
//!     .get_prices(&["فملی".to_string()], &PriceSettings::default())
//!     .await?;
//! println!("{:#?}", res.data);
//!
//! // getInstruments
//! let instruments = client.get_instruments().await?;
//! println!("{} instruments", instruments.len());
//!
//! // getIntraday
//! let itd = client
//!     .get_intraday(&["فملی".to_string()], &IntradaySettings::default())
//!     .await?;
//! println!("{:#?}", itd.error);
//! # Ok(())
//! # }
//! ```

pub mod adjust;
pub mod client;
pub mod config;
pub mod error;
pub mod group;
pub mod intraday;
pub mod models;
pub mod request;
pub mod storage;
pub mod util;

pub use client::{Cell, Client, InstrumentPrices, PricesResult};
pub use config::{Config, IntradaySettings, PriceSettings};
pub use error::{Error, Result, ResultError};
pub use group::{Period, group};
pub use models::{
    COLS, COLS_FA, ClosingPrice, Column, Instrument, InstrumentItd, Share, itd_group_cols,
};

use std::collections::HashMap;

impl Client {
    /// Port of JS `getInstruments`. Returns instruments keyed by `InsCode`.
    pub async fn get_instruments(&self) -> Result<HashMap<String, Instrument>> {
        let last_update = self.storage().get_item("tse.lastInstrumentUpdate")?;
        if let Some(err) = self.update_instruments().await? {
            if last_update.is_empty() {
                return Err(Error::FailedRequest {
                    title: format!("{err:?}"),
                    detail: String::new(),
                });
            }
        }
        let rows = self.storage().get_item("tse.instruments")?;
        let mut map = HashMap::new();
        if rows.is_empty() {
            return Ok(map);
        }
        for row in rows.split('\n').filter(|r| !r.is_empty()) {
            let ins = Instrument::parse(row)?;
            map.insert(ins.ins_code.clone(), ins);
        }
        Ok(map)
    }

    /// Port of JS `getIntradayInstruments`. Returns intraday instruments keyed
    /// by `InsCode`.
    pub async fn get_intraday_instruments(&self) -> Result<HashMap<String, InstrumentItd>> {
        let rows = self.storage().get_item("tse.instruments.intraday")?;
        let mut map = HashMap::new();
        if rows.is_empty() {
            return Ok(map);
        }
        for row in rows.split('\n').filter(|r| !r.is_empty()) {
            let ins = InstrumentItd::parse(row)?;
            map.insert(ins.ins_code.clone(), ins);
        }
        Ok(map)
    }
}
