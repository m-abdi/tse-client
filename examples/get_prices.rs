//! Fetch daily price history for one or more symbols.
//!
//! Run with:
//! ```shell
//! cargo run --example get_prices
//! ```

use tse_client::{Client, PriceSettings};

#[tokio::main]
async fn main() -> tse_client::Result<()> {
    let client = Client::new()?;

    let symbols = vec!["فملی".to_string()];
    let res = client
        .get_prices(&symbols, &PriceSettings::default())
        .await?;

    if let Some(err) = &res.error {
        eprintln!("request error: {err:?}");
        return Ok(());
    }

    for (symbol, instrument) in symbols.iter().zip(res.data.iter()) {
        match instrument {
            Some(prices) => println!("{symbol}: {} columns", prices.columns.len()),
            None => println!("{symbol}: not found"),
        }
    }

    Ok(())
}
