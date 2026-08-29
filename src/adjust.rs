//! Price-adjustment logic, ported from the JS `adjust()` using `rust_decimal`
//! for exact decimal arithmetic with banker's rounding (ROUND_HALF_EVEN).

use std::collections::HashMap;
use std::str::FromStr;

use rust_decimal::{Decimal, RoundingStrategy};

use crate::models::{ClosingPrice, Share};

/// A single price-adjustment event (capital increase or dividend).
#[derive(Debug, Clone)]
pub struct AdjustEvent {
    pub kind: String, // "capital increase" | "dividend"
    pub increase_pct: Option<String>,
    pub old_shares: Option<String>,
    pub new_shares: Option<String>,
    pub dividend: Option<String>,
    pub price_before_event: String,
    pub price_after_event: String,
    pub date: String,
}

/// Adjustment info collected when `get_adjust_info(_only)` is set.
#[derive(Debug, Clone, Default)]
pub struct AdjustInfo {
    pub events: Vec<AdjustEvent>,
    pub valid_gpl_ratio: Option<bool>,
}

/// Result of `adjust`.
#[derive(Debug, Clone)]
pub struct AdjustResult {
    pub prices: Option<Vec<ClosingPrice>>,
    pub info: Option<AdjustInfo>,
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap_or(Decimal::ZERO)
}

/// Multiply, and if it overflows `Decimal`'s 96-bit mantissa, shave scale off
/// whichever operand carries more of it and retry. This makes no assumption
/// about the magnitude or precision of the input data — it only gives up as
/// much precision as is actually necessary to make the specific multiplication
/// fit, so it's safe whether prices are single digits or six figures, and
/// whether `coef` has accumulated 2 digits of scale or 25.
///
/// Falls back to `0` only if trimming scale all the way to 0 still overflows,
/// which means the *integer* magnitude of the product itself exceeds
/// `Decimal::MAX` (~7.9e28) — a case no amount of rounding can fix.
fn safe_mul(a: Decimal, b: Decimal) -> Decimal {
    if let Some(v) = a.checked_mul(b) {
        return v;
    }
    let (mut a, mut b) = (a, b);
    // Trim scale one digit at a time from whichever side has more, retrying
    // after each step, until it fits or there's no scale left to give up.
    while a.scale() > 0 || b.scale() > 0 {
        if a.scale() >= b.scale() {
            a = a.round_dp_with_strategy(
                a.scale().saturating_sub(1),
                RoundingStrategy::MidpointNearestEven,
            );
        } else {
            b = b.round_dp_with_strategy(
                b.scale().saturating_sub(1),
                RoundingStrategy::MidpointNearestEven,
            );
        }
        if let Some(v) = a.checked_mul(b) {
            return v;
        }
    }
    Decimal::ZERO
}

/// `a * b / c`, using the same overflow-safe multiply, plus a checked divide.
/// Division by zero or a genuinely unrecoverable overflow leaves `coef`
/// unchanged rather than panicking or poisoning it with a garbage value.
fn safe_mul_div(a: Decimal, b: Decimal, c: Decimal) -> Decimal {
    if c.is_zero() {
        return a;
    }
    let ab = safe_mul(a, b);
    ab.checked_div(c).unwrap_or(a)
}

fn round_fixed(v: Decimal, places: u32) -> String {
    // Matches `.toDecimalPlaces(n).toFixed(n)` — half-even then fixed format.
    let r = v.round_dp_with_strategy(places, RoundingStrategy::MidpointNearestEven);
    format!("{:.*}", places as usize, r)
}

fn round_int(v: Decimal) -> String {
    let r = v.round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven);
    // `.toDecimalPlaces(0).toString()` — integer string, no trailing dot.
    r.normalize().to_string()
}

/// Port of the JS `adjust(cond, closingPrices, shares, getInfo, getInfoOnly)`.
///
/// `shares` maps a DEven string to the corresponding `Share` record.
pub fn adjust(
    cond: u8,
    closing_prices: &[ClosingPrice],
    shares: &HashMap<String, Share>,
    get_info: bool,
    get_info_only: bool,
) -> AdjustResult {
    let cp = closing_prices;
    let len = cp.len();
    let should_get_info = get_info || get_info_only;

    let mut info = AdjustInfo::default();

    // Default result mirrors the JS early/default `res`.
    let default_prices = if get_info_only {
        None
    } else {
        Some(cp.to_vec())
    };
    let mut result = AdjustResult {
        prices: default_prices.clone(),
        info: if should_get_info {
            Some(info.clone())
        } else {
            None
        },
    };

    if !((cond == 1 || cond == 2 || should_get_info) && len > 1) {
        return result;
    }

    let mut gaps = Decimal::ZERO;
    let mut coef = Decimal::ONE;
    let mut adjusted: Vec<ClosingPrice> = Vec::with_capacity(len);
    adjusted.push(cp[len - 1].clone());

    if cond == 1 || should_get_info {
        for i in (0..=len - 2).rev() {
            let curr = &cp[i];
            let next = &cp[i + 1];
            if dec(&curr.pclosing) != dec(&next.price_yesterday) && curr.ins_code == next.ins_code {
                gaps += Decimal::ONE;
            }
        }
    }

    let gaps_to_lifespan_ratio = gaps / Decimal::from(len as u64);
    let has_valid_ratio = gaps_to_lifespan_ratio < dec("0.08");
    info.valid_gpl_ratio = Some(has_valid_ratio);

    if !((cond == 1 && has_valid_ratio) || cond == 2 || should_get_info) {
        // Recompute info-only result so valid_gpl_ratio is surfaced.
        result.info = if should_get_info { Some(info) } else { None };
        return result;
    }

    for i in (0..=len - 2).rev() {
        let curr = &cp[i];
        let next = &cp[i + 1];
        let prices_dont_match =
            dec(&curr.pclosing) != dec(&next.price_yesterday) && curr.ins_code == next.ins_code;
        let target_share = shares.get(&next.deven);

        if should_get_info && prices_dont_match && (has_valid_ratio || target_share.is_some()) {
            let price_before_event = curr.pclosing.clone();
            let price_after_event = next.price_yesterday.clone();
            let date = curr.deven.clone();

            let event = if let Some(ts) = target_share {
                let old_shares = Decimal::from(ts.number_of_share_old);
                let new_shares = Decimal::from(ts.number_of_share_new);
                let increase_pct = if old_shares.is_zero() {
                    Decimal::ZERO
                } else {
                    (new_shares - old_shares) / old_shares
                };
                AdjustEvent {
                    kind: "capital increase".to_string(),
                    increase_pct: Some(increase_pct.normalize().to_string()),
                    old_shares: Some(ts.number_of_share_old.to_string()),
                    new_shares: Some(ts.number_of_share_new.to_string()),
                    dividend: None,
                    price_before_event,
                    price_after_event,
                    date,
                }
            } else {
                let dividend = dec(&price_before_event) - dec(&price_after_event);
                AdjustEvent {
                    kind: "dividend".to_string(),
                    increase_pct: None,
                    old_shares: None,
                    new_shares: None,
                    dividend: Some(dividend.normalize().to_string()),
                    price_before_event,
                    price_after_event,
                    date,
                }
            };
            info.events.push(event);
        }

        if get_info_only {
            continue;
        }

        if cond == 1 && prices_dont_match {
            coef = safe_mul_div(coef, dec(&next.price_yesterday), dec(&curr.pclosing));
        } else if cond == 2 && prices_dont_match {
            if let Some(ts) = target_share {
                let old_shares = Decimal::from(ts.number_of_share_old);
                let new_shares = Decimal::from(ts.number_of_share_new);
                coef = safe_mul_div(coef, old_shares, new_shares);
            }
        }
        // No fixed scale clamp here: `safe_mul`/`safe_mul_div` above and below
        // already trim only as much scale as each specific multiplication
        // needs to avoid overflow, whatever `coef`'s accumulated precision or
        // the price magnitudes happen to be — so this adapts to any data
        // shape instead of relying on a guessed-at limit.
        let close = round_fixed(safe_mul(coef, dec(&curr.pclosing)), 2);
        let last = round_fixed(safe_mul(coef, dec(&curr.pdr_cot_val)), 2);
        let low = round_int(safe_mul(coef, dec(&curr.price_min)));
        let high = round_int(safe_mul(coef, dec(&curr.price_max)));
        let yday = round_int(safe_mul(coef, dec(&curr.price_yesterday)));
        let first = round_fixed(safe_mul(coef, dec(&curr.price_first)), 2);

        adjusted.push(ClosingPrice {
            ins_code: curr.ins_code.clone(),
            deven: curr.deven.clone(),
            pclosing: close,
            pdr_cot_val: last,
            ztot_tran: curr.ztot_tran.clone(),
            qtot_tran5j: curr.qtot_tran5j.clone(),
            qtot_cap: curr.qtot_cap.clone(),
            price_min: low,
            price_max: high,
            price_yesterday: yday,
            price_first: first,
        });
    }

    adjusted.reverse();

    AdjustResult {
        prices: if get_info_only { None } else { Some(adjusted) },
        info: if should_get_info { Some(info) } else { None },
    }
}
