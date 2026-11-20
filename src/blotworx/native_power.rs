//! DOL800249f0 ->800213f0, for nonnegative single-precision field inputs.
//! The ordinary core is fdlibm. The original's half-power branch calls the
//! single-precision800071f0 helper BEFORE the usual1**y shortcut: host pow or
//! IEEE sqrt alone does not preserve that behavior. FPSCR/errno and broader
//! double/negative-base API compatibility are not claimed by this adapter.
use anyhow::{ensure, Result};

pub(super) fn field_double(base: f32, exponent: f32) -> Result<f64> {
    ensure!(
        base.is_finite() && base >= 0. && exponent.is_finite(),
        "Native field power requires finite nonnegative float inputs"
    );
    Ok(if exponent == 0.5 {
        super::cursor::native_sqrt(base) as f64
    } else {
        libm::pow(base as f64, exponent as f64)
    })
}
///804fbdb8 explicitly narrows the native double result with frsp.
pub(super) fn field(base: f32, exponent: f32) -> Result<f32> {
    Ok(field_double(base, exponent)? as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_power_matches_original_instruction_results() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/native_power.json"
        ))?;
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 326);
        for row in cases {
            let x = row[0].as_u64().unwrap();
            let y = row[1].as_u64().unwrap();
            let expected = row[2].as_u64().unwrap();
            let base = f64::from_bits(x) as f32;
            let exponent = f64::from_bits(y) as f32;
            assert_eq!((base as f64).to_bits(), x);
            assert_eq!((exponent as f64).to_bits(), y);
            assert_eq!(
                field_double(base, exponent)?.to_bits(),
                expected,
                "{base}^{exponent}"
            );
            assert_eq!(
                field(base, exponent)?.to_bits(),
                (f64::from_bits(expected) as f32).to_bits()
            );
        }
        Ok(())
    }
    #[test]
    fn unsupported_numeric_domains_are_errors_instead_of_host_fallbacks() {
        assert!(field(-1., 2.).is_err());
        assert!(field(f32::NAN, 1.).is_err());
        assert!(field(1., f32::INFINITY).is_err());
        assert!(field(f32::INFINITY, 1.).is_err());
        assert_eq!(field(1., 0.5).unwrap().to_bits(), 0x3f7fffff);
    }
}
