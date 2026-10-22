//! Decimal text to fixed-point ticks.
//!
//! [`parse_ticks`] accepts only a plain base-10 literal at scale `10^8`.
//! [`parse_ticks_exact`] accepts that literal and a scientific exponent when
//! the value is exact at the given scale. Neither function rounds, and
//! neither uses a binary float.

use parangsae_core::SCALE;

use crate::error::ScaleError;

/// Decimal places in one collector tick. `10^TICK_PLACES` is [`SCALE`].
pub const TICK_PLACES: u32 = 8;

const _: () = assert!(10i128.pow(TICK_PLACES) == SCALE);

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

/// Parse a base-10 literal into ticks at `10^{-scale}`.
///
/// `scale` is the number of decimal places. Funding rates and open interest
/// use [`TICK_PLACES`], so one tick is `1e-8`. The text is
/// `[-]digits[.digits]` with an optional `[eE][+-]digits` exponent. Checked
/// `i128` arithmetic converts it. A nonzero digit past `scale` is
/// [`ScaleError::Scale`]. A magnitude that does not fit in `i64` ticks is
/// [`ScaleError::Overflow`]. There is no rounding and no binary float.
pub fn parse_ticks_exact(raw: &str, scale: u32) -> Result<i64, ScaleError> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(ScaleError::Empty);
    }
    let (neg, unsigned) = split_sign(text)?;
    let (mantissa, exponent) = split_exponent(unsigned)?;
    let (int_part, frac_part) = split_mantissa(mantissa)?;
    if !is_ascii_digits(int_part) || !is_ascii_digits(frac_part) {
        return Err(ScaleError::Charset);
    }
    if is_all_zero(int_part, frac_part) {
        return Ok(0);
    }
    let exp = match exponent {
        Exponent::Finite(value) => value,
        Exponent::HugePositive => return Err(ScaleError::Overflow),
        Exponent::HugeNegative => return Err(ScaleError::Scale),
    };
    let frac_len = i128::try_from(frac_part.len()).map_err(|_| ScaleError::Overflow)?;
    let shift = i128::from(exp)
        .checked_sub(frac_len)
        .and_then(|power| power.checked_add(i128::from(scale)))
        .ok_or(ScaleError::Overflow)?;
    let mut digits = String::with_capacity(int_part.len() + frac_part.len());
    digits.push_str(int_part);
    digits.push_str(frac_part);
    let magnitude = magnitude_at_shift(&digits, shift)?;
    let signed = if neg {
        magnitude.checked_neg().ok_or(ScaleError::Overflow)?
    } else {
        magnitude
    };
    i64::try_from(signed).map_err(|_| ScaleError::Overflow)
}

enum Exponent {
    Finite(i64),
    HugePositive,
    HugeNegative,
}

fn split_sign(text: &str) -> Result<(bool, &str), ScaleError> {
    if let Some(rest) = text.strip_prefix('-') {
        if rest.is_empty() {
            return Err(ScaleError::Charset);
        }
        Ok((true, rest))
    } else if text.starts_with('+') {
        Err(ScaleError::Charset)
    } else {
        Ok((false, text))
    }
}

fn split_exponent(text: &str) -> Result<(&str, Exponent), ScaleError> {
    let mut marker = None;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'e' || byte == b'E' {
            if marker.is_some() {
                return Err(ScaleError::Charset);
            }
            marker = Some(index);
        }
    }
    let Some(index) = marker else {
        return Ok((text, Exponent::Finite(0)));
    };
    if index == 0 {
        return Err(ScaleError::Charset);
    }
    Ok((&text[..index], parse_exponent(&text[index + 1..])?))
}

fn parse_exponent(text: &str) -> Result<Exponent, ScaleError> {
    if text.is_empty() {
        return Err(ScaleError::Charset);
    }
    let (neg, digits) = if let Some(rest) = text.strip_prefix('+') {
        (false, rest)
    } else if let Some(rest) = text.strip_prefix('-') {
        (true, rest)
    } else {
        (false, text)
    };
    if digits.is_empty() || !is_ascii_digits(digits) {
        return Err(ScaleError::Charset);
    }
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        return Ok(Exponent::Finite(0));
    }
    if trimmed.len() > 18 {
        return Ok(if neg {
            Exponent::HugeNegative
        } else {
            Exponent::HugePositive
        });
    }
    let magnitude: i64 = trimmed.parse().map_err(|_| ScaleError::Overflow)?;
    Ok(Exponent::Finite(if neg { -magnitude } else { magnitude }))
}

fn split_mantissa(mantissa: &str) -> Result<(&str, &str), ScaleError> {
    if mantissa.is_empty() || mantissa.starts_with('+') || mantissa.starts_with('-') {
        return Err(ScaleError::Charset);
    }
    match mantissa.split_once('.') {
        Some((int_part, frac_part)) => {
            if int_part.is_empty() || frac_part.is_empty() || frac_part.contains('.') {
                return Err(ScaleError::Charset);
            }
            Ok((int_part, frac_part))
        }
        None => Ok((mantissa, "")),
    }
}

fn is_ascii_digits(text: &str) -> bool {
    text.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_all_zero(int_part: &str, frac_part: &str) -> bool {
    int_part.bytes().all(|byte| byte == b'0') && frac_part.bytes().all(|byte| byte == b'0')
}

fn magnitude_at_shift(digits: &str, shift: i128) -> Result<i128, ScaleError> {
    if shift >= 0 {
        if shift > 38 {
            return Err(ScaleError::Overflow);
        }
        let mut value = parse_digit_prefix(digits, digits.len())?;
        let times = u32::try_from(shift).map_err(|_| ScaleError::Overflow)?;
        for _ in 0..times {
            value = value.checked_mul(10).ok_or(ScaleError::Overflow)?;
        }
        return Ok(value);
    }
    let drop = usize::try_from(-shift).map_err(|_| ScaleError::Scale)?;
    if drop >= digits.len() {
        return Err(ScaleError::Scale);
    }
    let keep = digits.len() - drop;
    if digits.bytes().skip(keep).any(|byte| byte != b'0') {
        return Err(ScaleError::Scale);
    }
    parse_digit_prefix(digits, keep)
}

fn parse_digit_prefix(digits: &str, keep: usize) -> Result<i128, ScaleError> {
    let mut value: i128 = 0;
    for byte in digits.bytes().take(keep) {
        let digit = i128::from(byte - b'0');
        value = value
            .checked_mul(10)
            .and_then(|next| next.checked_add(digit))
            .ok_or(ScaleError::Overflow)?;
    }
    Ok(value)
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
    use proptest::prelude::*;
    use proptest::test_runner::TestRunner;

    use super::*;

    #[test]
    fn real_archive_literals() -> Result<(), String> {
        assert_eq!(
            parse_ticks("81903.30").map_err(|err| err.to_string())?,
            8_190_330_000_000
        );
        assert_eq!(
            parse_ticks("-0.00042342").map_err(|err| err.to_string())?,
            -42_342
        );
        assert_eq!(
            parse_ticks("0.00008482").map_err(|err| err.to_string())?,
            8_482
        );
        assert_eq!(
            parse_ticks("-5.00").map_err(|err| err.to_string())?,
            -500_000_000
        );
        assert_eq!(
            parse_ticks("-0.20").map_err(|err| err.to_string())?,
            -20_000_000
        );
        assert_eq!(
            parse_ticks("7743705995.7837090000000000").map_err(|err| err.to_string())?,
            774_370_599_578_370_900
        );
        assert_eq!(parse_ticks("1.000000009"), Err(ScaleError::Scale));
        assert_eq!(parse_ticks("1e-3"), Err(ScaleError::Charset));
        assert_eq!(parse_ticks(""), Err(ScaleError::Empty));
        Ok(())
    }

    #[test]
    fn format_round_trip_drops_only_trailing_zeros() -> Result<(), String> {
        let ticks = parse_ticks("81903.3000").map_err(|err| err.to_string())?;
        assert_eq!(
            parse_ticks(&format_ticks(ticks)).map_err(|err| err.to_string())?,
            ticks
        );
        assert_eq!(format_ticks(ticks), "81903.3");
        Ok(())
    }

    #[test]
    fn real_archive_exponent_rows() -> Result<(), ScaleError> {
        let scale = TICK_PLACES;
        assert_eq!(parse_ticks_exact("8.4E-7", scale)?, 84);
        assert_eq!(parse_ticks_exact("-3.5E-7", scale)?, -35);
        assert_eq!(parse_ticks_exact("-1E-8", scale)?, -1);
        assert_eq!(parse_ticks_exact("-1.0E-7", scale)?, -10);
        assert_eq!(parse_ticks_exact("6E-8", scale)?, 6);
        assert_eq!(parse_ticks_exact("0E-16", scale)?, 0);
        assert_eq!(parse_ticks_exact("0E-8", scale)?, 0);
        assert_eq!(parse_ticks_exact("0E400", scale)?, 0);
        assert_eq!(parse_ticks_exact("-0E-16", scale)?, 0);
        Ok(())
    }

    #[test]
    fn plain_decimals_match_the_strict_parser() -> Result<(), ScaleError> {
        for text in [
            "81903.30",
            "-0.00042342",
            "0.00008482",
            "-5.00",
            "-0.20",
            "7743705995.7837090000000000",
            "1.000000009",
            "0",
            "000.1000",
            "",
        ] {
            assert_eq!(
                parse_ticks_exact(text, TICK_PLACES),
                parse_ticks(text),
                "{text}"
            );
        }
        assert_eq!(parse_ticks("1e-3"), Err(ScaleError::Charset));
        assert_eq!(parse_ticks_exact("1e-3", TICK_PLACES)?, 100_000);
        assert_eq!(
            parse_ticks_exact(&format_ticks(i64::MAX), TICK_PLACES)?,
            i64::MAX
        );
        assert_eq!(
            parse_ticks_exact(&format_ticks(i64::MIN), TICK_PLACES)?,
            i64::MIN
        );
        Ok(())
    }

    #[test]
    fn rounding_exponent_is_scale() -> Result<(), ScaleError> {
        for text in [
            "1E-9",
            "-1E-9",
            "1.5E-8",
            "8.45E-7",
            "11E-9",
            "1E-400",
            "-1E-400",
            "2E-9",
            "1.000000009",
        ] {
            assert_eq!(
                parse_ticks_exact(text, TICK_PLACES),
                Err(ScaleError::Scale),
                "{text}"
            );
        }
        Ok(())
    }

    #[test]
    fn overflow_exponent_is_overflow() -> Result<(), ScaleError> {
        for text in [
            "1E400",
            "-1E400",
            "1E+400",
            "1e400",
            "1E11",
            "9223372036854775808",
            "-9223372036854775809",
            "92233720368.54775808",
        ] {
            assert_eq!(
                parse_ticks_exact(text, TICK_PLACES),
                Err(ScaleError::Overflow),
                "{text}"
            );
        }
        Ok(())
    }

    #[test]
    fn malformed_exponent_is_charset() -> Result<(), ScaleError> {
        for text in [
            "1E", "1e", "1E+", "1E-", "E-7", "-E-7", "8.4E-", "8.4E--7", "8.4E+-7", "8.4E-+7",
            ".4E-7", "4.E-7", "8.4E-7.1", "+8.4E-7", "8.4 E-7", "8.4E -7", "8.4E-7e1", "8.4eE-7",
            "-", "+", "++1", ".", "1E2.0",
        ] {
            assert_eq!(
                parse_ticks_exact(text, TICK_PLACES),
                Err(ScaleError::Charset),
                "{text}"
            );
        }
        assert_eq!(
            parse_ticks_exact("   ", TICK_PLACES),
            Err(ScaleError::Empty)
        );
        Ok(())
    }

    #[test]
    fn scale_argument_is_the_decimal_places() -> Result<(), ScaleError> {
        assert_eq!(parse_ticks_exact("1E-9", 9)?, 1);
        assert_eq!(parse_ticks_exact("1E-9", 8), Err(ScaleError::Scale));
        assert_eq!(parse_ticks_exact("1.5", 1)?, 15);
        assert_eq!(parse_ticks_exact("1.5", 0), Err(ScaleError::Scale));
        assert_eq!(parse_ticks_exact("10E-9", TICK_PLACES)?, 1);
        Ok(())
    }

    #[test]
    fn equivalent_exponent_spells_quoted_archive_rows() -> Result<(), String> {
        let style = ExponentStyle {
            e_upper: true,
            sign_plus: false,
            exp_pad: 0,
            frac_extra_zeros: 0,
            int_lead_zeros: 0,
            omit_zero_exponent: false,
        };
        let cases = [
            (84, -7, "8.4E-7"),
            (-35, -7, "-3.5E-7"),
            (-1, -8, "-1E-8"),
            (-10, -7, "-1.0E-7"),
            (6, -8, "6E-8"),
            (0, -16, "0E-16"),
        ];
        for (ticks, exponent, expected) in cases {
            let form = equivalent_exponent(ticks, TICK_PLACES, exponent, style);
            if form != expected {
                return Err(format!(
                    "{ticks} at exponent {exponent} spelled {form}, expected {expected}"
                ));
            }
        }
        Ok(())
    }

    #[test]
    fn plain_and_every_exponent_form_are_the_same_tick() -> Result<(), String> {
        let mut config = ProptestConfig::with_cases(256);
        config.failure_persistence = None;
        let mut runner = TestRunner::new(config);
        runner
            .run(&exponent_sample(), |sample| {
                let plain = format_ticks(sample.ticks);
                prop_assert_eq!(parse_ticks(&plain), Ok(sample.ticks));
                prop_assert_eq!(parse_ticks_exact(&plain, TICK_PLACES), Ok(sample.ticks));
                let sci =
                    equivalent_exponent(sample.ticks, TICK_PLACES, sample.exponent, sample.style);
                prop_assert_eq!(
                    parse_ticks_exact(&sci, TICK_PLACES),
                    Ok(sample.ticks),
                    "form {}",
                    sci
                );
                if !sci.bytes().any(|byte| byte == b'e' || byte == b'E') {
                    prop_assert_eq!(parse_ticks(&sci), Ok(sample.ticks), "plain form {}", sci);
                }
                if sample.exponent != 0 {
                    prop_assert!(
                        sci.contains('e') || sci.contains('E'),
                        "exponent form omitted the marker: {}",
                        sci
                    );
                }
                Ok(())
            })
            .map_err(|err| err.to_string())
    }

    #[derive(Clone, Copy, Debug)]
    struct ExponentStyle {
        e_upper: bool,
        sign_plus: bool,
        exp_pad: u8,
        frac_extra_zeros: u8,
        int_lead_zeros: u8,
        omit_zero_exponent: bool,
    }

    #[derive(Clone, Debug)]
    struct ExponentSample {
        ticks: i64,
        exponent: i32,
        style: ExponentStyle,
    }

    fn exponent_sample() -> impl Strategy<Value = ExponentSample> {
        (
            any::<i64>(),
            -48i32..=48,
            any::<bool>(),
            any::<bool>(),
            0u8..=3,
            0u8..=6,
            0u8..=3,
            any::<bool>(),
        )
            .prop_map(
                |(
                    ticks,
                    exponent,
                    e_upper,
                    sign_plus,
                    exp_pad,
                    frac_extra_zeros,
                    int_lead_zeros,
                    omit_zero_exponent,
                )| ExponentSample {
                    ticks,
                    exponent,
                    style: ExponentStyle {
                        e_upper,
                        sign_plus,
                        exp_pad,
                        frac_extra_zeros,
                        int_lead_zeros,
                        omit_zero_exponent,
                    },
                },
            )
    }

    fn equivalent_exponent(ticks: i64, scale: u32, exponent: i32, style: ExponentStyle) -> String {
        let scale_i = i32::try_from(scale).expect("scale fits in i32");
        let neg = ticks < 0;
        let digits = ticks.unsigned_abs().to_string();
        let body = if ticks == 0 {
            zero_coefficient(style)
        } else {
            shifted_coefficient(&digits, scale_i, exponent, style)
        };
        let mut out = String::new();
        if neg {
            out.push('-');
        }
        out.push_str(&body);
        out.push_str(&exponent_suffix(exponent, style));
        out
    }

    fn zero_coefficient(style: ExponentStyle) -> String {
        let mut body = "0".repeat(usize::from(style.int_lead_zeros) + 1);
        if style.frac_extra_zeros > 0 {
            body.push('.');
            body.push_str(&"0".repeat(usize::from(style.frac_extra_zeros)));
        }
        body
    }

    fn shifted_coefficient(
        digits: &str,
        scale: i32,
        exponent: i32,
        style: ExponentStyle,
    ) -> String {
        let shift = -scale - exponent;
        if shift >= 0 {
            let mut int_digits = digits.to_owned();
            int_digits.push_str(&"0".repeat(shift as usize));
            return with_fraction(&int_digits, "", style);
        }
        let places = (-shift) as usize;
        if digits.len() > places {
            let split = digits.len() - places;
            with_fraction(&digits[..split], &digits[split..], style)
        } else {
            let mut frac = "0".repeat(places - digits.len());
            frac.push_str(digits);
            with_fraction("0", &frac, style)
        }
    }

    fn with_fraction(int_digits: &str, frac_digits: &str, style: ExponentStyle) -> String {
        let mut body = "0".repeat(usize::from(style.int_lead_zeros));
        body.push_str(int_digits);
        if frac_digits.is_empty() && style.frac_extra_zeros == 0 {
            return body;
        }
        body.push('.');
        body.push_str(frac_digits);
        body.push_str(&"0".repeat(usize::from(style.frac_extra_zeros)));
        body
    }

    fn exponent_suffix(exponent: i32, style: ExponentStyle) -> String {
        if style.omit_zero_exponent && exponent == 0 {
            return String::new();
        }
        let marker = if style.e_upper { 'E' } else { 'e' };
        let sign = if exponent < 0 {
            "-"
        } else if style.sign_plus {
            "+"
        } else {
            ""
        };
        let digits = format!(
            "{}{}",
            "0".repeat(usize::from(style.exp_pad)),
            exponent.unsigned_abs()
        );
        format!("{marker}{sign}{digits}")
    }
}
