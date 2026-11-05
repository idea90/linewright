pub mod blocks;
pub mod curves;
pub mod defaults;
pub mod dimension;
pub mod drafting;
pub mod draw;
pub mod edit;
pub mod groups;
pub mod inquiry;
pub mod layer_tools;
pub mod layout;
pub mod measuring;
pub mod modify;
pub mod parse;
pub mod prompts;
pub mod qdim;
pub mod registry;
pub mod reshape;
pub mod shapes;

#[cfg(test)]
mod tests;

/// A number the way Python's `{:g}` shows it: up to six significant digits, without trailing zeros.
pub fn format_general(value: f64) -> String {
    format_significant(value, 6)
}

/// A number with up to `digits` significant digits and no trailing zeros, like Python's `{:.<digits>g}`.
pub fn format_significant(value: f64, digits: i32) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let rounded: f64 = format!("{value:.*e}", (digits - 1) as usize).parse().unwrap_or(value);
    let exponent = rounded.abs().log10().floor() as i32;
    if exponent < -4 || exponent >= digits {
        let text = format!("{value:.*e}", (digits - 1) as usize);
        let (mantissa, power) = text.split_once('e').unwrap_or((&text, "0"));
        let mantissa = if mantissa.contains('.') { mantissa.trim_end_matches('0').trim_end_matches('.') } else { mantissa };
        let power: i32 = power.parse().unwrap_or(0);
        return format!("{mantissa}e{}{:02}", if power < 0 { '-' } else { '+' }, power.abs());
    }
    let decimals = (digits - 1 - exponent).max(0) as usize;
    let text = format!("{value:.decimals$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text }
}

#[cfg(test)]
mod format_tests {
    use super::format_general;

    #[test]
    fn like_python_g() {
        assert_eq!(format_general(0.0), "0");
        assert_eq!(format_general(2.5), "2.5");
        assert_eq!(format_general(10.0), "10");
        assert_eq!(format_general(1.0 / 3.0), "0.333333");
        assert_eq!(format_general(1234567.0), "1.23457e+06");
        assert_eq!(format_general(0.0001), "0.0001");
        assert_eq!(format_general(999999.5), "1e+06");
        assert_eq!(super::format_significant(50.0, 4), "50");
        assert_eq!(super::format_significant(1.0 / 0.03, 4), "33.33");
    }
}
