//! Decimal text to scale-`10^8` ticks. This is the only decimal parser.

use parangsae_core::SCALE;

use crate::error::ScaleError;

/// Parse a plain base-10 literal into ticks.
///
/// More than 8 fractional digits fail unless the extra digits are zero.
/// There is no exponent and no binary float.
pub fn parse_ticks(raw: &str) -> Result<i64, ScaleError> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(ScaleError::Empty);
    }
    let (neg, digits) = if let Some(rest) = text.strip_prefix('-') {
        if rest.is_empty() {
            return Err(ScaleError::Charset);
        }
        (true, rest)
    } else {
        (false, text)
    };
    if digits.starts_with('+') || digits.starts_with('-') {
        return Err(ScaleError::Charset);
    }
    let (int_part, frac_part) = match digits.split_once('.') {
        Some((int_part, frac_part)) => {
            if int_part.is_empty() || frac_part.is_empty() || frac_part.contains('.') {
                return Err(ScaleError::Charset);
            }
            (int_part, frac_part)
        }
        None => (digits, ""),
    };
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(ScaleError::Charset);
    }
    let frac_bytes = frac_part.as_bytes();
    if frac_bytes.len() > 8 && frac_bytes[8..].iter().any(|byte| *byte != b'0') {
        return Err(ScaleError::Scale);
    }
    let mut frac_pad = [b'0'; 8];
    let take = frac_bytes.len().min(8);
    frac_pad[..take].copy_from_slice(&frac_bytes[..take]);
    let mut frac_val: i128 = 0;
    for byte in frac_pad {
        frac_val = frac_val * 10 + i128::from(byte - b'0');
    }
    let int_trim = int_part.trim_start_matches('0');
    let int_trim = if int_trim.is_empty() { "0" } else { int_trim };
    let int_val: i128 = int_trim.parse().map_err(|_| ScaleError::Overflow)?;
    let mut scaled = int_val
        .checked_mul(SCALE)
        .ok_or(ScaleError::Overflow)?
        .checked_add(frac_val)
        .ok_or(ScaleError::Overflow)?;
    if neg {
        scaled = scaled.checked_neg().ok_or(ScaleError::Overflow)?;
    }
    i64::try_from(scaled).map_err(|_| ScaleError::Overflow)
}

pub fn format_ticks(ticks: i64) -> String {
    let neg = ticks < 0;
    let magnitude = ticks.unsigned_abs();
    let scale = 100_000_000u64;
    let int_part = magnitude / scale;
    let frac = magnitude % scale;
    let mut out = if neg {
        format!("-{int_part}")
    } else {
        format!("{int_part}")
    };
    if frac != 0 {
        let mut frac_text = format!("{frac:08}");
        while frac_text.ends_with('0') {
            frac_text.pop();
        }
        out.push('.');
        out.push_str(&frac_text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_archive_literals() {
        assert_eq!(parse_ticks("81903.30").unwrap(), 8_190_330_000_000);
        assert_eq!(parse_ticks("-0.00042342").unwrap(), -42_342);
        assert_eq!(parse_ticks("0.00008482").unwrap(), 8_482);
        assert_eq!(parse_ticks("-5.00").unwrap(), -500_000_000);
        assert_eq!(parse_ticks("-0.20").unwrap(), -20_000_000);
        assert_eq!(
            parse_ticks("7743705995.7837090000000000").unwrap(),
            774_370_599_578_370_900
        );
        assert_eq!(parse_ticks("1.000000009"), Err(ScaleError::Scale));
        assert_eq!(parse_ticks("1e-3"), Err(ScaleError::Charset));
        assert_eq!(parse_ticks(""), Err(ScaleError::Empty));
    }

    #[test]
    fn format_round_trip_drops_only_trailing_zeros() {
        let ticks = parse_ticks("81903.3000").unwrap();
        assert_eq!(parse_ticks(&format_ticks(ticks)).unwrap(), ticks);
        assert_eq!(format_ticks(ticks), "81903.3");
    }
}
