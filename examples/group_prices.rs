//! Group daily prices into weekly / monthly bars using the Jalali calendar.
//!
//! Run with: `cargo run --example group_prices`

use tse_client::models::ClosingPrice;
use tse_client::util::greg_to_shamsi;
use tse_client::{Period, group};

/// Build a daily row: ins_code,deven,close,last,count,vol,value,low,high,yesterday,first
fn row(deven: &str, open: &str, high: &str, low: &str, close: &str, vol: &str) -> ClosingPrice {
    ClosingPrice::parse(&format!(
        "1,{deven},{close},{close},10,{vol},1000,{low},{high},{close},{open}"
    ))
    .unwrap()
}

fn print_rows(title: &str, rows: &[ClosingPrice]) {
    println!("\n== {title} ==");
    println!(
        "{:<10} {:<10} open  high  low   close  volume",
        "date", "jalali"
    );
    for r in rows {
        println!(
            "{:<10} {:<10} {:<5} {:<5} {:<5} {:<6} {}",
            r.deven,
            greg_to_shamsi(&r.deven),
            r.price_first,
            r.price_max,
            r.price_min,
            r.pclosing,
            r.qtot_tran5j
        );
    }
}

fn main() {
    // A few weeks of (fabricated) daily data spanning two Jalali months.
    let daily = vec![
        row("20230421", "10", "15", "9", "11", "100"),
        row("20230422", "11", "18", "10", "12", "150"),
        row("20230423", "12", "16", "11", "14", "120"),
        row("20230520", "14", "19", "13", "18", "200"),
        row("20230521", "18", "21", "17", "20", "220"),
        row("20230522", "20", "25", "19", "24", "300"),
    ];

    print_rows("Daily", &daily);
    print_rows("Weekly (Jalali, Sat–Fri)", &group(&daily, Period::Weekly));
    print_rows("Monthly (Jalali)", &group(&daily, Period::Monthly));
}
