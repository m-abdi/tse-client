# tse-client (Rust)

A Rust library-crate port of the [`tse-client`](https://www.npmjs.com/package/tse-client)
npm package: a client for fetching stock data from the Tehran Stock Exchange
(TSETMC).

This port covers the **library** surface only (no CLI, no WASM/browser target).

## Stack

| Concern | Crate |
|---|---|
| Async runtime | `tokio` |
| HTTP | `reqwest` (rustls) |
| Exact decimal math (price adjustment) | `rust_decimal` |
| Jalali ↔ Gregorian dates | `ptime` + `time` |
| gzip cache | `flate2` |
| (de)serialization | `serde` / `serde_json` |
| Errors | `thiserror` |
| Cache dir resolution | `dirs` |

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

Configuration that used to be mutable globals (`PRICES_UPDATE_CHUNK`,
`INTRADAY_URL`, etc.) now lives on `Client::config_mut()` via the `Config`
struct.

## Notable differences from the JS version (bug / leak fixes)

The original JS kept several **module-level mutable maps** that were *never
cleared between calls*:

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
