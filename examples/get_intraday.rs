//! Fetch intraday data for one or more symbols.
//!
//! Run with:
//! ```shell
//! cargo run --example get_intraday
//! ```

use tse_client::{Client, IntradaySettings};

#[tokio::main]
async fn main() -> tse_client::Result<()> {
    let client = Client::new()?;

    let symbols = vec!["فملی".to_string()];
    let res = client
        .get_intraday(&symbols, &IntradaySettings::default())
        .await?;

    if let Some(err) = &res.error {
        eprintln!("request error: {err:?}");
        return Ok(());
    }

    for (symbol, payload) in symbols.iter().zip(res.data.iter()) {
        match payload {
            Some(records) => println!("{symbol}: {} day(s) of intraday data", records.len()),
            None => println!("{symbol}: no data"),
        }
    }

    Ok(())
}
