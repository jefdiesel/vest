//! Ready-to-show amount strings, computed in the module so QML only displays them.
//!
//! Amounts stay exact u128 integers on the wire (`"claimable": "94138000"`) and are also
//! returned formatted with the token's decimals (`"claimable_display": "0.094138 LEZ"`).
//! The token is LEZ (9 decimals) by default; the `decimals` setting overrides it.

use std::sync::atomic::{AtomicU8, Ordering};

static DECIMALS: AtomicU8 = AtomicU8::new(9);
const SYMBOL: &str = "LEZ";

pub fn set_decimals(value: u8) {
    DECIMALS.store(value.min(18), Ordering::Relaxed)
}

pub fn decimals() -> u8 {
    DECIMALS.load(Ordering::Relaxed)
}

fn group(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// "94138000" (9 decimals) -> "0.094138 LEZ"; "1000000000" -> "1 LEZ".
pub fn amount(value: u128) -> String {
    let decimals = decimals() as usize;
    if decimals == 0 {
        return format!("{} {SYMBOL}", group(&value.to_string()));
    }
    let base = 10u128.pow(decimals as u32);
    let whole = value / base;
    let fraction = value % base;
    let mut out = group(&whole.to_string());
    if fraction > 0 {
        let mut digits = fraction.to_string();
        while digits.len() < decimals {
            digits.insert(0, '0');
        }
        let trimmed = digits.trim_end_matches('0');
        out.push('.');
        out.push_str(trimmed);
    }
    out.push(' ');
    out.push_str(SYMBOL);
    out
}
