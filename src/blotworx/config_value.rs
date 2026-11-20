//! Retail config bool conversion: case-insensitive TRUE/FALSE, otherwise
//! strtod -> f32 -> fctiwz -> nonzero (REL14adf4,26a7e0). Not Lua truthiness.
pub fn boolean(value: Option<&str>) -> Result<bool, &'static str> {
    let Some(value) = value else { return Ok(false) };
    let value = value.split('\0').next().unwrap_or("");
    if value.eq_ignore_ascii_case("true") { return Ok(true) }
    if value.eq_ignore_ascii_case("false") { return Ok(false) }
    let value = value.trim_start_matches(|c: char| c.is_ascii_whitespace());
    if value.starts_with('(') {
        return Err("Tuple configuration bool needs native retained integer-state validation");
    }
    let number=decimal_float(value)?;
    if number < i32::MIN as f32 || number >= 2147483648. {
        return Err("Configuration bool exceeds verified finite fctiwz range");
    }
    Ok(number.trunc() as i32 != 0)
}
/// Verified finite decimal part of DOL8001b528 followed by frsp. Preserve
/// numeric prefixes and incomplete exponents; unsupported hex/nonfinite cases
/// are diagnosed rather than assigned Rust parsing/saturation semantics.
pub(super) fn decimal_float(value:&str)->Result<f32,&'static str> {
    let value=value.split('\0').next().unwrap_or("").trim_start_matches(|c:char|c.is_ascii_whitespace());
    // C strtod accepts a numeric prefix and returns zero if no number starts.
    // Hex floats and special nonfinite/out-of-range fctiwz are not admitted.
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    if unsigned.get(..3).is_some_and(|s| s.eq_ignore_ascii_case("nan") || s.eq_ignore_ascii_case("inf")) {
        return Err("Nonfinite decimal conversion is not connected");
    }
    if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
        return Err("Hex decimal-input conversion is not connected");
    }
    let bytes = value.as_bytes();
    let mut end = usize::from(bytes.first().is_some_and(|v| matches!(v, b'+' | b'-')));
    let mut digits = 0;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) { end += 1; digits += 1; }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) { end += 1; digits += 1; }
    }
    if digits == 0 { return Ok(0.) }
    if bytes.get(end).is_some_and(|v| matches!(v, b'e' | b'E')) {
        let exponent = end;
        end += 1;
        if bytes.get(end).is_some_and(|v| matches!(v, b'+' | b'-')) { end += 1; }
        let start = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) { end += 1; }
        if start == end { end = exponent; }
    }
    let number = value[..end].parse::<f64>().map_err(|_| "Invalid config numeric prefix")? as f32;
    if !number.is_finite() {return Err("Nonfinite decimal float is not admitted");}
    Ok(number)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_and_string_booleans() {
        for value in [None, Some("false"), Some("FaLsE"), Some("")] { assert!(!boolean(value).unwrap()); }
        for value in ["TRUE", "true", "TrUe", "TRUE\0ignored"] { assert!(boolean(Some(value)).unwrap()); }
    }
    #[test]
    fn numeric_boolean_uses_truncated_single_not_float_truthiness() {
        for value in ["0", "0.999", "-0.5", "junk", "1e-3", ".5", "1e-", " +.1suffix"] {
            // Incomplete exponent falls back to the preceding numeric prefix.
            let expected = value == "1e-";
            assert_eq!(boolean(Some(value)).unwrap(), expected, "{value}");
        }
        for value in ["1", "-1", "2.9suffix", "  +1e1", "0.999999999"] { assert!(boolean(Some(value)).unwrap(), "{value}"); }
    }
    #[test]
    fn unverified_conversions_are_explicit() {
        for value in ["0x1p2", "(0,1,2)", "1e100", "2147483648", "NaN", "-inf"] { assert!(boolean(Some(value)).is_err()); }
    }
}
