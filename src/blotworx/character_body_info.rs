//! Character->rigid-body construction-info transfer, DOL802f56cc..802f573c.
//! Call after the real rigid-body cinfo constructor. Opaque32-bit shape tokens
//! must be resolved by the owner backend; no Wii pointer is dereferenced here.
use anyhow::{ensure, Result};
/// Remaining constructor values, DOL802f57b4..802f582c. This is prepared
/// metadata, not body property installation, listener registration or simulation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeValues {
    pub property_key: u32,
    pub property_bits: u64,
    pub up: [u32; 4],
    pub field_30: u32,
    pub slope_cosine: u32,
    pub field_38: u32,
    pub field_3c: u32,
    pub field_40: u32,
    pub velocity: [u32; 4],
    pub field_60: u32,
}
pub trait RuntimeBackend {
    fn body(&mut self) -> Result<usize>;
    /// DOL802e0efc addProperty: preserve the first existing key/value.
    fn property(&mut self, body: usize, key: u32, bits: u64) -> Result<()>;
    fn up(&mut self, words: [u32; 4]) -> Result<()>;
    fn scalar(&mut self, offset: usize, word: u32) -> Result<()>;
    fn velocity(&mut self, words: [u32; 4]) -> Result<()>;
    /// Register this wrapper's actual entity-listener interface on the body.
    fn register_body_listener(&mut self, body: usize) -> Result<()>;
    fn clear_pointer_14(&mut self) -> Result<()>;
}
/// Ordered DOL802f57b4..802f5844 publication. The body is reread before
/// registration; backend/cosine errors preserve earlier effects and stop.
pub fn initialize_runtime(
    character: &[u8],
    velocity_pair: [u32; 2],
    backend: &mut impl RuntimeBackend,
) -> Result<()> {
    ensure!(
        character.len() >= 0x6c,
        "Truncated character runtime construction info"
    );
    let word = |offset| u32::from_be_bytes(character[offset..offset + 4].try_into().unwrap());
    let body = backend.body()?;
    ensure!(body != 0, "Null character body for property installation");
    backend.property(body, 0x1130, (word(0x68) as i32 as i64) as u64)?;
    backend.up(std::array::from_fn(|i| word(0x40 + i * 4)))?;
    let cosine = super::native_angle::sin_cos_medium(f32::from_bits(word(0x50)) as f64)?.1 as f32;
    // Preserve native store order, rather than sorting offsets numerically.
    for (offset, value) in [
        (0x34, cosine.to_bits()),
        (0x3c, word(0x60)),
        (0x40, word(0x64)),
        (0x38, word(0x5c)),
        (0x30, word(0x58)),
    ] {
        backend.scalar(offset, value)?;
    }
    backend.velocity([
        velocity_pair[0],
        velocity_pair[1],
        velocity_pair[0],
        velocity_pair[1],
    ])?;
    backend.scalar(0x60, word(0x54))?;
    let body = backend.body()?;
    ensure!(body != 0, "Null character body for listener registration");
    backend.register_body_listener(body)?;
    backend.clear_pointer_14()
}
pub fn runtime_values(character: &[u8], velocity_pair: [u32; 2]) -> Result<RuntimeValues> {
    ensure!(
        character.len() >= 0x6c,
        "Truncated character runtime construction info"
    );
    let word = |offset| u32::from_be_bytes(character[offset..offset + 4].try_into().unwrap());
    let angle = f32::from_bits(word(0x50)) as f64;
    let cosine = super::native_angle::sin_cos_medium(angle)?.1 as f32;
    Ok(RuntimeValues {
        property_key: 0x1130,
        property_bits: (word(0x68) as i32 as i64) as u64,
        up: std::array::from_fn(|i| word(0x40 + i * 4)),
        field_30: word(0x58),
        slope_cosine: cosine.to_bits(),
        field_38: word(0x5c),
        field_3c: word(0x60),
        field_40: word(0x64),
        velocity: [
            velocity_pair[0],
            velocity_pair[1],
            velocity_pair[0],
            velocity_pair[1],
        ],
        field_60: word(0x54),
    })
}
pub fn apply(character: &[u8], body: &mut [u8], restitution_word: u32) -> Result<()> {
    ensure!(
        character.len() >= 0x40,
        "Truncated character body construction info"
    );
    ensure!(
        body.len() >= 0xb8,
        "Truncated initialized rigid-body construction info"
    );
    for (source, destination, length) in [
        (0, 0, 8),
        (0x10, 0x10, 0x20),
        (0x30, 0x90, 4),
        (0x34, 0xa0, 4),
        (0x38, 0xa8, 4),
        (0x3c, 0xb0, 4),
    ] {
        body[destination..destination + length]
            .copy_from_slice(&character[source..source + length]);
    }
    body[0xa4..0xa8].copy_from_slice(&restitution_word.to_be_bytes());
    body[0xb4] = 7;
    body[0xb7] = 8;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_publication_preserves_order_fresh_body_and_partial_failures() -> Result<()> {
        struct Trace {
            body: usize,
            calls: Vec<String>,
            fail: bool,
        }
        impl RuntimeBackend for Trace {
            fn body(&mut self) -> Result<usize> {
                self.calls.push("body".into());
                Ok(self.body)
            }
            fn property(&mut self, b: usize, k: u32, v: u64) -> Result<()> {
                assert_eq!((b, k, v), (7, 0x1130, 0xffffffff80000000));
                self.calls.push("property".into());
                Ok(())
            }
            fn up(&mut self, w: [u32; 4]) -> Result<()> {
                assert_eq!(w, [0; 4]);
                self.calls.push("up".into());
                self.body = 9;
                Ok(())
            }
            fn scalar(&mut self, o: usize, w: u32) -> Result<()> {
                if o == 0x34 {
                    assert_eq!(w, 0x3f800000)
                };
                self.calls.push(format!("scalar {o:x}"));
                if self.fail {
                    anyhow::bail!("Scalar backend failed")
                };
                Ok(())
            }
            fn velocity(&mut self, w: [u32; 4]) -> Result<()> {
                assert_eq!(w, [0; 4]);
                self.calls.push("velocity".into());
                Ok(())
            }
            fn register_body_listener(&mut self, b: usize) -> Result<()> {
                assert_eq!(b, 9);
                self.calls.push("register".into());
                Ok(())
            }
            fn clear_pointer_14(&mut self) -> Result<()> {
                self.calls.push("clear14".into());
                Ok(())
            }
        }
        let mut source = [0; 0x70];
        source[0x68..0x6c].copy_from_slice(&0x80000000u32.to_be_bytes());
        let mut trace = Trace {
            body: 7,
            calls: vec![],
            fail: false,
        };
        initialize_runtime(&source, [0; 2], &mut trace)?;
        assert_eq!(
            trace.calls,
            [
                "body",
                "property",
                "up",
                "scalar 34",
                "scalar 3c",
                "scalar 40",
                "scalar 38",
                "scalar 30",
                "velocity",
                "scalar 60",
                "body",
                "register",
                "clear14"
            ]
        );
        let mut trace = Trace {
            body: 7,
            calls: vec![],
            fail: true,
        };
        assert!(initialize_runtime(&source, [0; 2], &mut trace).is_err());
        assert_eq!(trace.calls, ["body", "property", "up", "scalar 34"]);
        source[0x50..0x54].copy_from_slice(&1e20f32.to_be_bytes());
        let mut trace = Trace {
            body: 7,
            calls: vec![],
            fail: false,
        };
        assert!(initialize_runtime(&source, [0; 2], &mut trace).is_err());
        assert_eq!(trace.calls, ["body", "property", "up"]);
        assert_eq!(trace.body, 9);
        let mut trace = Trace {
            body: 7,
            calls: vec![],
            fail: false,
        };
        assert!(initialize_runtime(&source[..0x6b], [0; 2], &mut trace).is_err());
        assert!(trace.calls.is_empty());
        Ok(())
    }
    #[test]
    fn character_runtime_values_keep_native_scalar_mapping_and_signed_property() -> Result<()> {
        let mut source = [0u8; 0x70];
        for (offset, value) in [
            (0x40, 0),
            (0x44, 0x3f800000),
            (0x48, 0),
            (0x4c, 0x80000000),
            (0x50, 0),
            (0x54, 0x3f000000),
            (0x58, 0x11111111),
            (0x5c, 0x22222222),
            (0x60, 0x33333333),
            (0x64, 0x44444444),
        ] {
            source[offset..offset + 4].copy_from_slice(&u32::to_be_bytes(value));
        }
        for (value, expected_property) in [
            (0, 0u64),
            (1, 1),
            (0x7fffffff, 0x7fffffff),
            (0x80000000, 0xffffffff80000000),
            (0xffffffff, 0xffffffffffffffff),
        ] {
            source[0x68..0x6c].copy_from_slice(&u32::to_be_bytes(value));
            let state = runtime_values(&source, [0, 0x80000000])?;
            assert_eq!(state.property_key, 0x1130);
            assert_eq!(state.property_bits, expected_property);
            assert_eq!(state.up, [0, 0x3f800000, 0, 0x80000000]);
            assert_eq!(state.slope_cosine, 0x3f800000);
            assert_eq!(
                (
                    state.field_30,
                    state.field_38,
                    state.field_3c,
                    state.field_40,
                    state.field_60
                ),
                (0x11111111, 0x22222222, 0x33333333, 0x44444444, 0x3f000000)
            );
            assert_eq!(state.velocity, [0, 0x80000000, 0, 0x80000000]);
        }
        assert!(runtime_values(&source[..0x6b], [0; 2]).is_err());
        source[0x50..0x54].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(runtime_values(&source, [0; 2]).is_err());
        source[0x50..0x54].copy_from_slice(&1e20f32.to_be_bytes());
        assert!(runtime_values(&source, [0; 2]).is_err());
        Ok(())
    }
    #[test]
    fn character_cinfo_transfer_keeps_constructor_fields_and_explicit_motion_quality() -> Result<()>
    {
        let mut source = [0u8; 0x70];
        for (i, word) in source.chunks_exact_mut(4).enumerate() {
            word.copy_from_slice(&(0x12340000u32 + i as u32).to_be_bytes());
        }
        source[0x14..0x18].copy_from_slice(&0x80000000u32.to_be_bytes());
        source[0x24..0x28].copy_from_slice(&0x7fc12345u32.to_be_bytes());
        let mut body = [0xa5; 0xc0];
        apply(&source, &mut body, 0)?;
        let mut expected = [0xa5; 0xc0];
        expected[..8].copy_from_slice(&source[..8]);
        expected[0x10..0x30].copy_from_slice(&source[0x10..0x30]);
        expected[0x90..0x94].copy_from_slice(&source[0x30..0x34]);
        expected[0xa0..0xa4].copy_from_slice(&source[0x34..0x38]);
        expected[0xa4..0xa8].fill(0);
        expected[0xa8..0xac].copy_from_slice(&source[0x38..0x3c]);
        expected[0xb0..0xb4].copy_from_slice(&source[0x3c..0x40]);
        expected[0xb4] = 7;
        expected[0xb7] = 8;
        assert_eq!(body, expected);
        assert_eq!(body[0xb5..0xb7], [0xa5; 2]);
        assert_eq!(
            super::super::body_quality::constructor_quality(body[0xb7], body[0xb4], body[0xb4]),
            8
        );
        let prior = body;
        assert!(apply(&source[..0x3f], &mut body, 0).is_err());
        assert_eq!(body, prior);
        let mut truncated = [0x55; 0xb7];
        assert!(apply(&source, &mut truncated, 0).is_err());
        assert_eq!(truncated, [0x55; 0xb7]);
        apply(&source, &mut body, 0x3e800000)?;
        assert_eq!(&body[0xa4..0xa8], &0x3e800000u32.to_be_bytes());
        Ok(())
    }
}
