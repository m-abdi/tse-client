//! Fetch daily price history for one or more symbols.
//!
//! Run with:
//! ```shell
//! cargo run --example get_prices
//! ```

use tse_client::{Client, Period, PriceSettings};

#[tokio::main]
async fn main() -> tse_client::Result<()> {
    let client = Client::new()?;

    let symbols = vec!["فملی".to_string()];

    // Weekly timeframe (Jalali Saturday–Friday). Use Period::Daily (the
    // default) for daily rows or Period::Monthly for Jalali months.
    let settings = PriceSettings {
        period: Period::Weekly,
        ..PriceSettings::default()
    };
    let res = client.get_prices(&symbols, &settings).await?;

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
