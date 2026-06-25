use std::collections::HashMap;

use tse_client::adjust::adjust;
use tse_client::group::{Period, group};
use tse_client::models::{ClosingPrice, Share};
use tse_client::util::{
    clean_fa, day_diff, greg_to_shamsi, shamsi_month_key, shamsi_to_greg, shamsi_week_key,
};

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

/// Full OHLCV row constructor for grouping tests.
/// Order: ins_code,deven,close,last,count,vol,value,low,high,yesterday,first
#[allow(clippy::too_many_arguments)]
fn ohlcv(
    deven: &str,
    open: &str,
    high: &str,
    low: &str,
    close: &str,
    vol: &str,
    yesterday: &str,
) -> ClosingPrice {
    ClosingPrice::parse(&format!(
        "1,{deven},{close},{close},10,{vol},1000,{low},{high},{yesterday},{open}"
    ))
    .unwrap()
}

#[test]
fn shamsi_week_key_groups_saturday_to_friday() {
    // 1402/03/06 = Saturday 2023-05-27 .. 1402/03/12 = Friday 2023-06-02.
    // All days in that Jalali week must share the same week key.
    let sat = shamsi_week_key("20230527").unwrap();
    let wed = shamsi_week_key("20230531").unwrap();
    let fri = shamsi_week_key("20230602").unwrap();
    assert_eq!(sat, wed);
    assert_eq!(sat, fri);
    // The next Saturday starts a new week.
    let next_sat = shamsi_week_key("20230603").unwrap();
    assert_ne!(sat, next_sat);
    // The key is the Saturday that starts the week.
    assert_eq!(sat, "20230527");
}

#[test]
fn shamsi_month_key_matches_jalali_month() {
    // 2023-04-21 falls in Jalali 1402/02 (Ordibehesht).
    let key = shamsi_month_key("20230421").unwrap();
    assert_eq!(&key, &greg_to_shamsi("20230421")[0..6]);
}

#[test]
fn group_daily_returns_input_unchanged() {
    let daily = vec![
        ohlcv("20230527", "10", "12", "9", "11", "100", "10"),
        ohlcv("20230528", "11", "13", "10", "12", "200", "11"),
    ];
    let out = group(&daily, Period::Daily);
    assert_eq!(out, daily);
}

#[test]
fn group_weekly_aggregates_ohlcv() {
    // Three days in one Jalali week (Sat 2023-05-27 .. Mon 2023-05-29),
    // then one day in the next week (Sat 2023-06-03).
    let daily = vec![
        ohlcv("20230527", "10", "15", "9", "11", "100", "8"),
        ohlcv("20230528", "11", "20", "7", "12", "200", "11"),
        ohlcv("20230529", "12", "14", "10", "13", "300", "12"),
        ohlcv("20230603", "20", "22", "18", "21", "50", "13"),
    ];
    let out = group(&daily, Period::Weekly);
    assert_eq!(out.len(), 2);

    let w1 = &out[0];
    assert_eq!(w1.price_first, "10"); // first open
    assert_eq!(w1.price_max, "20"); // max high
    assert_eq!(w1.price_min, "7"); // min low
    assert_eq!(w1.pclosing, "13"); // last close
    assert_eq!(w1.price_yesterday, "8"); // first yesterday
    assert_eq!(w1.qtot_tran5j, "600"); // summed volume
    assert_eq!(w1.deven, "20230529"); // last trading day in the week

    let w2 = &out[1];
    assert_eq!(w2.price_first, "20");
    assert_eq!(w2.pclosing, "21");
    assert_eq!(w2.qtot_tran5j, "50");
    assert_eq!(w2.deven, "20230603");
}

#[test]
fn group_monthly_aggregates_ohlcv() {
    // 2023-04-21 -> Jalali 1402/02, 2023-05-22 -> Jalali 1402/03 (different months).
    let daily = vec![
        ohlcv("20230421", "10", "15", "9", "11", "100", "8"),
        ohlcv("20230422", "11", "18", "10", "12", "150", "11"),
        ohlcv("20230522", "20", "25", "19", "24", "300", "12"),
    ];
    let out = group(&daily, Period::Monthly);
    assert_eq!(out.len(), 2);

    assert_eq!(out[0].price_max, "18");
    assert_eq!(out[0].qtot_tran5j, "250");
    assert_eq!(out[0].deven, "20230422");

    assert_eq!(out[1].pclosing, "24");
    assert_eq!(out[1].qtot_tran5j, "300");
    assert_eq!(out[1].deven, "20230522");
}
