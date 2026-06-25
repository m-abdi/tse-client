//! Fetch the full instrument list (keyed by `InsCode`).
//!
//! Run with:
//! ```shell
//! cargo run --example get_instruments
//! ```

use tse_client::Client;

#[tokio::main]
async fn main() -> tse_client::Result<()> {
    let client = Client::new()?;

    let instruments = client.get_instruments().await?;
    println!("fetched {} instruments", instruments.len());

    for ins in instruments.values().take(5) {
        println!("{} ({})", ins.symbol, ins.ins_code);
    }

    Ok(())
}
