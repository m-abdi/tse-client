//! Weekly / monthly price grouping based on the Jalali (Solar Hijri) calendar.
//!
//! Daily [`ClosingPrice`] rows are aggregated into weekly or monthly OHLCV
//! bars using Jalali calendar boundaries:
//!
//! - **Weekly** groups run from Saturday (شنبه) through Friday (جمعه), the
//!   conventional Iranian trading week.
//! - **Monthly** groups follow the Jalali month (e.g. Farvardin, Ordibehesht).
//!
//! Aggregation rules per group (matching common OHLCV resampling):
//!
//! - `price_first` (open)      = first row's open
//! - `price_max` (high)        = max high across the group
//! - `price_min` (low)         = min low across the group
//! - `pclosing` (close)        = last row's close
//! - `pdr_cot_val` (last)      = last row's last trade price
//! - `price_yesterday`         = first row's yesterday price
//! - `qtot_tran5j` (volume)    = sum of volume
//! - `ztot_tran` (count)       = sum of trade count
//! - `qtot_cap` (value)        = sum of value
//! - `deven`                   = the last (most recent) trading day in the group
//! - `ins_code`                = carried over from the rows
//!
//! Rows are assumed to be sorted ascending by `deven`; the result is sorted the
//! same way. Rows whose dates can't be converted to the Jalali calendar are
//! skipped.

use std::str::FromStr;

use rust_decimal::Decimal;

use crate::models::ClosingPrice;
use crate::util::{shamsi_month_key, shamsi_week_key};

/// The grouping period for [`group`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    /// No grouping; daily rows are returned unchanged.
    #[default]
    Daily,
    /// Group into Jalali weeks (Saturday–Friday).
    Weekly,
    /// Group into Jalali (Solar Hijri) calendar months.
    Monthly,
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

/// Sum a numeric string field, preserving an integer-style textual form.
fn sum_field<'a, I>(values: I) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    let total: Decimal = values.into_iter().map(dec).sum();
    total.normalize().to_string()
}

/// Aggregate the rows of a single group into one [`ClosingPrice`] bar.
///
/// `rows` must be non-empty and ordered ascending by `deven`.
fn aggregate(rows: &[ClosingPrice]) -> ClosingPrice {
    let first = &rows[0];
    let last = &rows[rows.len() - 1];

    // High = max of highs, Low = min of lows (compared as decimals).
    let mut high = dec(&first.price_max);
    let mut low = dec(&first.price_min);
    for r in &rows[1..] {
        let h = dec(&r.price_max);
        if h > high {
            high = h;
        }
        let l = dec(&r.price_min);
        if l < low {
            low = l;
        }
    }

    ClosingPrice {
        ins_code: first.ins_code.clone(),
        deven: last.deven.clone(),
        pclosing: last.pclosing.clone(),
        pdr_cot_val: last.pdr_cot_val.clone(),
        ztot_tran: sum_field(rows.iter().map(|r| r.ztot_tran.as_str())),
        qtot_tran5j: sum_field(rows.iter().map(|r| r.qtot_tran5j.as_str())),
        qtot_cap: sum_field(rows.iter().map(|r| r.qtot_cap.as_str())),
        price_min: low.normalize().to_string(),
        price_max: high.normalize().to_string(),
        price_yesterday: first.price_yesterday.clone(),
        price_first: first.price_first.clone(),
    }
}

/// Group daily [`ClosingPrice`] rows into Jalali weekly or monthly OHLCV bars.
///
/// Rows should be sorted ascending by `deven`. For [`Period::Daily`] the input
/// is returned unchanged (cloned). Rows whose `deven` can't be converted to the
/// Jalali calendar are dropped from weekly/monthly output.
///
/// # Examples
///
/// ```
/// use tse_client::{group, ClosingPrice, Period};
///
/// let daily = vec![/* ClosingPrice rows sorted by date */];
/// let weekly = group(&daily, Period::Weekly);
/// let monthly = group(&daily, Period::Monthly);
/// let _ = (weekly, monthly);
/// ```
pub fn group(prices: &[ClosingPrice], period: Period) -> Vec<ClosingPrice> {
    if period == Period::Daily || prices.is_empty() {
        return prices.to_vec();
    }

    let key_of = |p: &ClosingPrice| -> Option<String> {
        match period {
            Period::Weekly => shamsi_week_key(&p.deven),
            Period::Monthly => shamsi_month_key(&p.deven),
            Period::Daily => unreachable!(),
        }
    };

    let mut out: Vec<ClosingPrice> = Vec::new();
    let mut current_key: Option<String> = None;
    let mut bucket: Vec<ClosingPrice> = Vec::new();

    for p in prices {
        let Some(key) = key_of(p) else {
            // Unconvertible date: skip the row rather than mis-group it.
            continue;
        };

        match &current_key {
            Some(k) if *k == key => bucket.push(p.clone()),
            Some(_) => {
                out.push(aggregate(&bucket));
                bucket.clear();
                bucket.push(p.clone());
                current_key = Some(key);
            }
            None => {
                current_key = Some(key);
                bucket.push(p.clone());
            }
        }
    }

    if !bucket.is_empty() {
        out.push(aggregate(&bucket));
    }

    out
}
