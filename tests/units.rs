use std::collections::HashMap;

use tse_client::adjust::adjust;
use tse_client::models::{ClosingPrice, Share};
use tse_client::util::{clean_fa, day_diff, greg_to_shamsi, shamsi_to_greg};

#[test]
fn clean_fa_normalizes_arabic_letters() {
    // Arabic kaf (ك) and yeh (ي) -> Persian forms
    assert_eq!(clean_fa("\u{0643}\u{064A}"), "\u{06A9}\u{06CC}");
    // zero-width chars stripped/collapsed
    assert_eq!(clean_fa("a\u{200B}b"), "ab");
}

#[test]
fn date_conversions_round_trip() {
    // 2016-01-02 (Gregorian) == 1394-10-12 (Jalali) per ptime docs region
    let sh = greg_to_shamsi("20160102");
    assert_eq!(sh.len(), 8);
    let back = shamsi_to_greg(&sh);
    assert_eq!(back, "20160102");
}

#[test]
fn day_diff_basic() {
    assert_eq!(day_diff("20200101", "20200101"), 0);
    assert_eq!(day_diff("20200101", "20200111"), 10);
}

fn cp(deven: &str, close: &str, yesterday: &str) -> ClosingPrice {
    // ins_code,deven,close,last,count,vol,value,low,high,yesterday,first
    ClosingPrice::parse(&format!(
        "1,{deven},{close},{close},10,100,1000,{close},{close},{yesterday},{close}"
    ))
    .unwrap()
}

#[test]
fn adjust_passthrough_when_cond_zero() {
    let prices = vec![cp("20200101", "100", "100"), cp("20200102", "110", "100")];
    let res = adjust(0, &prices, &HashMap::new(), false, false);
    // cond 0 => prices returned unchanged
    assert_eq!(res.prices.unwrap().len(), 2);
}

#[test]
fn adjust_capital_increase_applies_coefficient() {
    // cond=2 bypasses the GPL-ratio guard and uses share counts.
    // A price gap on 20200102 with a matching share record (old=100, new=200)
    // scales older rows by old/new = 0.5.
    let prices = vec![
        cp("20200101", "100", "100"),
        cp("20200102", "120", "90"), // PriceYesterday(90) != prev PClosing(100)
    ];
    let mut shares: HashMap<String, Share> = HashMap::new();
    // share keyed by next.DEven == "20200102"
    shares.insert(
        "20200102".to_string(),
        Share::parse("1,1,20200102,200,100").unwrap(), // new=200, old=100
    );
    let res = adjust(2, &prices, &shares, false, false);
    let out = res.prices.unwrap();
    assert_eq!(out.len(), 2);
    // newest row pushed verbatim
    assert_eq!(out[1].pclosing, "120");
    // older row scaled by old/new = 100/200 = 0.5 -> 50.00
    assert_eq!(out[0].pclosing, "50.00");
}
