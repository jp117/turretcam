//! Number formatting identical to the Lantek PSTSTR05 output in the shop archive.
//!
//! * at most `dec` decimals, trailing zeros removed, halves rounded up (6.53125 -> 6.5313)
//! * whole numbers keep a trailing dot: `96.` `0.` `270.`
//! * counts are bare integers: `K6`

/// Format a value the way the post does (`96.`, `58.1268`, `0.`).
pub fn num(v: f64, dec: usize) -> String {
    let r = round_half_up(v, dec);
    let mut s = format!("{:.*}", dec, r);
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
        s.remove(0);
    }
    while s.ends_with('0') {
        s.pop();
    }
    s
}

/// Decimal half-up rounding on the shortest round-trip representation of `v`
/// (same result as Python's `Decimal(repr(v)).quantize(..., ROUND_HALF_UP)`).
fn round_half_up(v: f64, dec: usize) -> f64 {
    let txt = format!("{}", v.abs());
    let Some(dot) = txt.find('.') else { return v };
    let frac = &txt[dot + 1..];
    if frac.len() <= dec {
        return v;
    }
    let int_part = &txt[..dot];
    let digits = format!("{}{}", int_part, &frac[..dec]);
    let mut n: i128 = digits.parse().unwrap_or(0);
    if frac.as_bytes()[dec] >= b'5' {
        n += 1;
    }
    let r = n as f64 / 10f64.powi(dec as i32);
    if v < 0.0 {
        -r
    } else {
        r
    }
}

/// Header comment numbers: no trailing dot, truncated to 3 dp (Lantek truncates: 0.0315 -> 0.031).
pub fn trunc3(v: f64) -> String {
    let t = ((v * 1000.0) + 1e-6).floor() / 1000.0;
    strip(format!("{:.3}", t))
}

/// Plain number for comments (clamp positions): up to 4 dp, no trailing dot.
pub fn plain(v: f64) -> String {
    let s = num(v, 4);
    let s = s.trim_end_matches('.').to_string();
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s
    }
}

fn strip(mut s: String) -> String {
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats() {
        assert_eq!(num(96.0, 4), "96.");
        assert_eq!(num(0.0, 4), "0.");
        assert_eq!(num(-0.00001, 4), "0.");
        assert_eq!(num(6.53125, 4), "6.5313");
        assert_eq!(num(2.4047, 4), "2.4047");
        assert_eq!(num(-0.8125, 4), "-0.8125");
        assert_eq!(num(89.903654, 5), "89.90365");
        assert_eq!(num(97.00787, 3), "97.008");
        assert_eq!(num(1e-7, 4), "0.");
        assert_eq!(trunc3(0.0315), "0.031");
        assert_eq!(trunc3(45.8125), "45.812");
        assert_eq!(trunc3(96.0), "96");
        assert_eq!(plain(12.0), "12");
    }
}
