//! Integration tests exercising the public API surface.
//!
//! The compile-only tests guarantee the public API stays stable. The tests
//! marked `#[ignore]` hit the live TSETMC network and are skipped by default;
//! run them explicitly with:
//!
//! ```shell
//! cargo test --test public_api -- --ignored
//! ```

use tse_client::{Client, Config, IntradaySettings, PriceSettings};

/// The public types should be constructible exactly as documented. This runs
/// in normal `cargo test` (no network) and breaks the build if the public API
/// changes incompatibly.
#[test]
fn public_api_constructs() {
    let _ = Config::default();
    let _ = PriceSettings::default();
    let _ = IntradaySettings::default();
    // `Client::new()` only resolves the cache dir; no network involved.
    let _client = Client::new().expect("client construction should succeed");
}

#[tokio::test]
#[ignore = "hits the live TSETMC network"]
async fn get_instruments_live() {
    let client = Client::new().unwrap();
    let instruments = client.get_instruments().await.unwrap();
    assert!(!instruments.is_empty(), "expected at least one instrument");
}

#[tokio::test]
#[ignore = "hits the live TSETMC network"]
async fn get_prices_live() {
    let client = Client::new().unwrap();
    let symbols = vec!["فملی".to_string()];
    let res = client
        .get_prices(&symbols, &PriceSettings::default())
        .await
        .unwrap();
    assert!(res.error.is_none(), "unexpected error: {:?}", res.error);
    assert_eq!(res.data.len(), symbols.len());
}

#[tokio::test]
#[ignore = "hits the live TSETMC network"]
async fn get_intraday_live() {
    let client = Client::new().unwrap();
    let symbols = vec!["فملی".to_string()];
    let res = client
        .get_intraday(&symbols, &IntradaySettings::default())
        .await
        .unwrap();
    assert_eq!(res.data.len(), symbols.len());
}
