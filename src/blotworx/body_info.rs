//! Rigid-body cinfo defaults, DOL802b7c84..802b7da0. Padding is untouched.
use anyhow::{ensure, Result};
#[derive(Clone, Copy, Debug)]
pub struct Constants(pub [u32; 9]);
impl Constants {
    /// Original DOL806793d0..806793f4. Owners may supply current runtime words.
    pub fn retail() -> Self {
        Self([
            0, 0, 0x3f800000, 0, 0x3d4ccccd, 0x3f000000, 0x3ecccccd, 0x43480000, 0xbf800000,
        ])
    }
}
pub fn initialize(body: &mut [u8], constants: Constants) -> Result<()> {
    ensure!(
        body.len() >= 0xbc,
        "Truncated rigid-body cinfo default storage"
    );
    let c = constants.0;
    let word = |storage: &mut [u8], offset: usize, value: u32| {
        storage[offset..offset + 4].copy_from_slice(&value.to_be_bytes())
    };
    for offset in (0x10..0x90).step_by(8) {
        word(body, offset, c[0]);
        word(body, offset + 4, c[1]);
    }
    // Quaternion and inertia basis are explicitly overwritten after pair copies.
    for (offset, value) in [
        (0x20, c[3]),
        (0x24, c[3]),
        (0x28, c[3]),
        (0x2c, c[2]),
        (0x50, c[2]),
        (0x64, c[2]),
        (0x78, c[2]),
        (0x90, c[2]),
        (0x94, c[3]),
        (0x98, c[4]),
        (0x9c, c[2]),
        (0xa0, c[5]),
        (0xa4, c[6]),
        (0xa8, c[7]),
        (0xac, c[7]),
        (0xb0, c[8]),
    ] {
        word(body, offset, value);
    }
    body[0xb4..0xb8].copy_from_slice(&[1, 1, 2, 0xff]);
    body[0xc] = 1;
    body[0xe..0x10].copy_from_slice(&0xffffu16.to_be_bytes());
    body[..0xc].fill(0);
    body[0xb8..0xbc].fill(0);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_cinfo_defaults_match_original_instruction_reference_and_preserve_padding() -> Result<()>
    {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/body_info_defaults.json"
        ))?;
        assert_eq!(
            reference["source_sha256"],
            "994779fa2e276983efdb50f02c406568f285a82558cb5d54bbdf74651a98a920"
        );
        let constants = Constants(std::array::from_fn(|i| {
            reference["constants"][i].as_u64().unwrap() as u32
        }));
        assert_eq!(constants.0, Constants::retail().0);
        let mut body = [0xa5; 0xc0];
        initialize(&mut body, constants)?;
        let expected = reference["expected"].as_str().unwrap();
        let decoded = expected
            .as_bytes()
            .chunks_exact(2)
            .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(body.as_slice(), decoded);
        assert_eq!(body[0xd], 0xa5);
        assert_eq!(body[0xbc..], [0xa5; 4]);
        let mut short = [0x55; 0xbb];
        assert!(initialize(&mut short, constants).is_err());
        assert_eq!(short, [0x55; 0xbb]);
        // Character transfer retains gravity, angular speed and other defaults.
        let mut character = [0u8; 0x70];
        character[0x30..0x34].copy_from_slice(&75f32.to_be_bytes());
        super::super::character_body_info::apply(&character, &mut body, 0)?;
        assert_eq!(&body[0x90..0x94], &75f32.to_be_bytes());
        assert_eq!(&body[0x9c..0xa0], &1f32.to_be_bytes());
        assert_eq!(&body[0xac..0xb0], &200f32.to_be_bytes());
        assert_eq!((body[0xb4], body[0xb7]), (7, 8));
        Ok(())
    }
}
