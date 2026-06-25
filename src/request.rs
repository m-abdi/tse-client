//! HTTP request layer, replacing the JS `rq` object built on `node-fetch`.

use std::time::Duration;

use crate::error::{Error, Result};

pub const DEFAULT_API_URL: &str = "http://service.tsetmc.com/tsev2/data/TseClient2.aspx";

/// Thin client wrapping `reqwest` for the TseClient2 endpoint.
#[derive(Debug, Clone)]
pub struct Requester {
    client: reqwest::Client,
    api_url: String,
}

impl Requester {
    pub fn new(api_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("failed to build reqwest client");
        Requester { client, api_url }
    }

    async fn make_request(&self, params: &[(&str, &str)]) -> Result<String> {
        let res = self.client.get(&self.api_url).query(params).send().await?;
        let status = res.status();
        if status.as_u16() == 200 {
            Ok(res.text().await?)
        } else {
            Err(Error::InvalidResponse(format!(
                "{} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("")
            )))
        }
    }

    pub async fn instrument(&self, deven: &str) -> Result<String> {
        self.make_request(&[("t", "Instrument"), ("a", deven)])
            .await
    }

    pub async fn instrument_and_share(&self, deven: &str, last_id: i64) -> Result<String> {
        let last_id = last_id.to_string();
        self.make_request(&[("t", "InstrumentAndShare"), ("a", deven), ("a2", &last_id)])
            .await
    }

    pub async fn last_possible_deven(&self) -> Result<String> {
        self.make_request(&[("t", "LastPossibleDeven")]).await
    }

    pub async fn closing_prices(&self, ins_codes: &str) -> Result<String> {
        self.make_request(&[("t", "ClosingPrices"), ("a", ins_codes)])
            .await
    }
}

/// Build an intraday URL, mirroring the JS `INTRADAY_URL` closure.
pub fn intraday_url(server: i32, inscode: &str, deven: &str) -> String {
    let host = if server > 0 {
        format!("cdn{server}.")
    } else if server < 0 {
        String::new()
    } else {
        "cdn.".to_string()
    };
    format!("http://{host}tsetmc.com/Loader.aspx?ParTree=15131P&i={inscode}&d={deven}")
}
