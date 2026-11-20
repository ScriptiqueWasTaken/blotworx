//! Attachment temporary pose setup, REL 0x354f50..0x354f90.
use super::{actor_bone_query::Pose, native_transform_record::Transform};
use anyhow::Result;
pub trait Backend {
    fn default_transform(&mut self) -> Result<Transform>;
    fn attachment_scale(&mut self) -> Result<f32>;
    fn store_scale_override_128(&mut self, value: u8) -> Result<()>;
}
pub fn prepare(bone: Pose, backend: &mut impl Backend) -> Result<Transform> {
    let mut transform = backend.default_transform()?;
    transform[9..12].copy_from_slice(&bone[..3]);
    let scale = backend.attachment_scale()?;
    // REL4cd8c0=34000000. It is a strict positive threshold, not abs(scale).
    if scale > f32::EPSILON {
        backend.store_scale_override_128(1)?;
        transform[12] = bone[8] * scale;
    }
    Ok(transform)
}
/// REL23b920: DOL8014bdd4 produces padded columns; REL23b884 transposes
/// their XYZ words into the NiMatrix3. Translation/scale remain unchanged.
pub fn rotation(transform: &mut Transform, quaternion: [f32; 4]) {
    let mut padded = [0.; 16];
    super::native_hk_transform::set_rotation(&mut padded, quaternion);
    for row in 0..3 {
        for col in 0..3 {
            transform[row * 3 + col] = padded[col * 4 + row];
        }
    }
}
pub trait ModeBackend {
    fn store_override(&mut self, offset: usize, value: u8) -> Result<()>;
    /// REL354fd0..355360 requires a second bone query or NiNode fallback,
    /// followed by the source directional basis calculation.
    fn directional(&mut self, bone: Pose, transform: &mut Transform) -> Result<()>;
}
pub trait OffsetBackend {
    fn offset(&mut self) -> Result<[f32; 3]>;
    fn identity_rotation(&mut self) -> Result<[f32; 9]>;
}
/// REL35561c..35572c, after the special mode6 cached-record composition.
/// Mode7 has a separate branch and must not enter this offset routine.
pub fn offset_work(
    mode: i32,
    prepared: Transform,
    rotate_offset: bool,
    backend: &mut impl OffsetBackend,
) -> Result<Transform> {
    anyhow::ensure!(
        (0..7).contains(&mode),
        "Attachment mode requires separate work preparation"
    );
    let mut work = prepared;
    let offset = backend.offset()?;
    let offset = if rotate_offset {
        super::native_matrix::transform(&work[..9].try_into().unwrap(), offset)
    } else {
        offset
    };
    if mode == 5 {
        work[..9].copy_from_slice(&backend.identity_rotation()?);
    }
    for i in 0..3 {
        work[9 + i] += offset[i];
    }
    Ok(work)
}
pub struct OwnedOffset<'a, 's> {
    pub storage: &'a super::transform_storage::Storage<'s>,
    pub identity: [f32; 9],
}
impl OffsetBackend for OwnedOffset<'_, '_> {
    fn offset(&mut self) -> Result<[f32; 3]> {
        Ok(self.storage.attachment_offset())
    }
    fn identity_rotation(&mut self) -> Result<[f32; 9]> {
        Ok(self.identity)
    }
}
/// Only the rotation switch at354f90..355388. The returned flag is r30,
/// consumed by later parent composition; it is not a completion result.
pub fn mode_rotation(
    mode: i32,
    bone: Pose,
    transform: &mut Transform,
    backend: &mut impl ModeBackend,
) -> Result<bool> {
    let mut compose_rotation = false;
    match mode {
        0 | 5 => {
            backend.store_override(0x127, 1)?;
            rotation(transform, bone[4..8].try_into().unwrap());
            compose_rotation = true;
        }
        4 => {
            rotation(transform, bone[4..8].try_into().unwrap());
            compose_rotation = true;
        }
        1 => backend.store_override(0x127, 1)?,
        2 => {
            backend.store_override(0x127, 1)?;
            backend.directional(bone, transform)?;
        }
        6 => {
            backend.store_override(0x127, 1)?;
            backend.store_override(0x126, 1)?;
            rotation(transform, bone[4..8].try_into().unwrap());
        }
        3 | 7 => {}
        _ => anyhow::bail!("Attachment mode outside skeletal rotation switch"),
    }
    Ok(compose_rotation)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_pose_offset_rotates_before_mode5_reset_without_scale() -> Result<()> {
        let prepared = [-1., 0., 0., 0., -1., 0., 0., 0., 1., 10., 20., 30., 7.];
        let identity = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let mut bytes = [0xa5; 0x140];
        for (i, value) in [1f32, 2., 3.].into_iter().enumerate() {
            bytes[0xcc + i * 4..0xd0 + i * 4].copy_from_slice(&value.to_be_bytes());
        }
        let storage = super::super::transform_storage::Storage::new(&mut bytes)?;
        let mut backend = OwnedOffset {
            storage: &storage,
            identity,
        };
        let out = offset_work(5, prepared, true, &mut backend)?;
        assert_eq!(&out[..9], &identity);
        assert_eq!(&out[9..], &[9., 18., 33., 7.]);
        let out = offset_work(0, prepared, true, &mut backend)?;
        assert_eq!(&out[..9], &prepared[..9]);
        assert_eq!(&out[9..], &[9., 18., 33., 7.]);
        let out = offset_work(1, prepared, false, &mut backend)?;
        assert_eq!(&out[9..], &[11., 22., 33., 7.]);
        assert!(offset_work(7, prepared, false, &mut backend).is_err());
        assert_eq!(prepared[12], 7.);
        Ok(())
    }
    #[test]
    fn attachment_pose_rotation_modes_keep_distinct_flags() -> Result<()> {
        struct Flags(Vec<usize>);
        impl ModeBackend for Flags {
            fn store_override(&mut self, o: usize, v: u8) -> Result<()> {
                assert_eq!(v, 1);
                self.0.push(o);
                Ok(())
            }
            fn directional(&mut self, _: Pose, _: &mut Transform) -> Result<()> {
                anyhow::bail!("Directional basis unavailable")
            }
        }
        let bone = [2., 4., 6., 0., 0., 0., 1., 0., 1., 1., 1., 1.];
        for (mode, flags, rotates, composes) in [
            (0, vec![0x127], true, true),
            (1, vec![0x127], false, false),
            (3, vec![], false, false),
            (4, vec![], true, true),
            (5, vec![0x127], true, true),
            (6, vec![0x127, 0x126], true, false),
            (7, vec![], false, false),
        ] {
            let mut backend = Flags(vec![]);
            let mut transform = [99.; 13];
            assert_eq!(
                mode_rotation(mode, bone, &mut transform, &mut backend)?,
                composes
            );
            assert_eq!(backend.0, flags);
            if rotates {
                assert_eq!(&transform[..9], &[-1., 0., 0., 0., -1., 0., 0., 0., 1.]);
            } else {
                assert_eq!(&transform[..9], &[99.; 9]);
            }
            assert_eq!(&transform[9..], &[99.; 4]);
        }
        let mut backend = Flags(vec![]);
        let mut transform = [99.; 13];
        assert!(mode_rotation(2, bone, &mut transform, &mut backend).is_err());
        assert_eq!(backend.0, [0x127]);
        assert_eq!(transform, [99.; 13]);
        Ok(())
    }
    struct State {
        scale: f32,
        flag: u8,
        calls: Vec<&'static str>,
    }
    impl Backend for State {
        fn default_transform(&mut self) -> Result<Transform> {
            self.calls.push("default");
            Ok([99.; 13])
        }
        fn attachment_scale(&mut self) -> Result<f32> {
            self.calls.push("scale");
            Ok(self.scale)
        }
        fn store_scale_override_128(&mut self, v: u8) -> Result<()> {
            self.calls.push("override");
            self.flag = v;
            Ok(())
        }
    }
    #[test]
    fn attachment_pose_preserves_strict_scale_gate_and_rotation_layout() -> Result<()> {
        let bone = [2., 4., 6., 999., 0., 0., 1., 0., 7., 8., 9., 10.];
        for scale in [-1., 0., f32::EPSILON, f32::NAN] {
            let mut state = State {
                scale,
                flag: 9,
                calls: vec![],
            };
            let out = prepare(bone, &mut state)?;
            assert_eq!(&out[9..12], &[2., 4., 6.]);
            assert_eq!(out[12], 99.);
            assert_eq!(state.flag, 9);
            assert_eq!(state.calls, ["default", "scale"]);
        }
        let mut state = State {
            scale: 2.,
            flag: 0,
            calls: vec![],
        };
        let mut out = prepare(bone, &mut state)?;
        assert_eq!(out[12], 14.);
        assert_eq!(state.flag, 1);
        assert_eq!(state.calls, ["default", "scale", "override"]);
        rotation(&mut out, bone[4..8].try_into().unwrap());
        assert_eq!(&out[..9], &[-1., 0., 0., 0., -1., 0., 0., 0., 1.]);
        assert_eq!(&out[9..], &[2., 4., 6., 14.]);
        Ok(())
    }
}
