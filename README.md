# tse-client (Rust)

<p align="center">
<img src="assets/tsetmc.png" alt="TSETMC" width="150">
</p>
A Rust library crate and port of the [`tse-client`](https://www.npmjs.com/package/tse-client) npm package, providing a client for accessing data from the Tehran Stock Exchange Technology Management Company (TSETMC). The crate supports fetching OHLC (Open/High/Low/Close) candlestick data, intraday trading data, market indices, and the complete list of tradable instruments across all major Iranian capital markets, including the Tehran Stock Exchange (TSE / بورس تهران), Iran Fara Bourse (IFB / فرابورس ایران), and the Base Market (بازار پایه). Supported indices include the Total Index (شاخص کل) and IFB Total Index (شاخص کل فرابورس).


> [!NOTE]
> This crate must be called on a server located **inside Iran**. TSE data
> servers are not accessible from outside the country.

## Features

- 📈 Fetch OHLC candlestick data for any TSE instrument
- 📋 Get the full list of TSE-listed instruments
- 🕯️ Supports Daily, Weekly, and Monthly timeframes with automatic resampling
- ⚡ Fetch intraday trading data
- 📅 Jalali (Solar Hijri) calendar support for correct weekly/monthly grouping
- 🔧 Adjusted prices support
- 🦀 High-performance Rust implementation and port of `tse-client`

## Public API

Mirrors the JS module:

```rust
use tse_client::{Client, PriceSettings, IntradaySettings};

#[tokio::main]
async fn main() -> tse_client::Result<()> {
    let client = Client::new()?;

    // getPrices
    let res = client
        .get_prices(&["فملی".to_string()], &PriceSettings::default())
        .await?;
    println!("{:#?}", res.data);

    // getInstruments
    let instruments = client.get_instruments().await?;
    println!("{} instruments", instruments.len());

    // getIntraday
    let itd = client
        .get_intraday(&["فملی".to_string()], &IntradaySettings::default())
        .await?;
    println!("{:#?}", itd.error);

    Ok(())
}
```

### Weekly / monthly timeframe (Jalali calendar)

`get_prices` can resample daily data into weekly or monthly OHLCV bars using
Jalali (Solar Hijri) calendar boundaries. Weeks run Saturday–Friday (the Iranian
trading week); months follow the Jalali calendar. Set the `period` field on
`PriceSettings`:

```rust
use tse_client::{Client, Period, PriceSettings};

# async fn run() -> tse_client::Result<()> {
let client = Client::new()?;
let settings = PriceSettings {
    period: Period::Weekly, // or Period::Monthly / Period::Daily (default)
    ..PriceSettings::default()
};
let res = client.get_prices(&["فملی".to_string()], &settings).await?;
# Ok(())
# }
```

You can also group already-parsed `ClosingPrice` rows directly with the free
function:

```rust
use tse_client::{group, ClosingPrice, Period};

// `daily` is a slice of ClosingPrice rows sorted ascending by date.
let daily: Vec<ClosingPrice> = /* ... */ vec![];

let weekly = group(&daily, Period::Weekly);
let monthly = group(&daily, Period::Monthly);
let unchanged = group(&daily, Period::Daily); // returns the input as-is
```

Aggregation per group: open = first open, close = last close, high = max high,
low = min low, and volume/value/count are summed. See
[`examples/group_prices.rs`](examples/group_prices.rs) for a runnable demo.

Configuration that used to be mutable globals (`PRICES_UPDATE_CHUNK`,
`INTRADAY_URL`, etc.) now lives on `Client::config_mut()` via the `Config`
struct.

## Notable differences from the JS version (bug / leak fixes)

The original JS kept several **module-level mutable maps** that were _never
cleared between calls_:

- `storedPrices` — accumulated every instrument's full price history ever
  fetched, for the lifetime of the process.
- `lastdevens` — accumulated last-update markers.
- `stored` (intraday) — accumulated every intraday record ever fetched.

In a short-lived CLI run this is harmless, but in a long-running process (a
server importing the module) it is an unbounded memory leak: memory grew with
every distinct symbol requested and was never reclaimed.

In this port, all of that working state is **owned locally per call**
(`PricesState` in `client.rs`, and the `stored` map in `intraday.rs`) and is
dropped when the call returns. No cross-call accumulation, no leak.

Other cleanups:

- Mutable URL/config globals replaced by an owned `Config` (no `static mut`).
- The `setTimeout`/`poll` callback state machines (`pricesUpdateManager`,
  `itdUpdateManager`) are expressed as plain bounded `async`/`await` loops with
  explicit retry rounds, removing the timer/`Map<id, timeout>` bookkeeping.
- Price adjustment uses `rust_decimal` with `MidpointNearestEven` rounding to
  match `decimal.js`'s `ROUND_HALF_EVEN`.

## Build & test

```shell
cargo build
cargo test
cargo clippy --all-targets
```
