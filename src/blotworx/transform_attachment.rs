//! Attachment entry354a14..354a5c. Recognized modes continue into skeletal
//! branches; out-of-range modes copy parent pose296f98 then force cache scale.
use anyhow::Result;
pub trait Backend {
    fn store_translation_override_126(&mut self, value: u8) -> Result<()>;
    fn mode_353ae8(&mut self) -> Result<i32>;
    /// Must publish in296f98 getter/write order; aggregate snapshot replacement
    /// would erase native partial writes if a later getter fails.
    fn copy_parent_pose_296f98(&mut self, parent: u32) -> Result<()>;
    fn one_4cd748(&mut self) -> Result<f32>;
    fn store_cache_scale_118(&mut self, value: f32) -> Result<()>;
    fn skeletal_354a5c(&mut self, parent: u32, mode: i32) -> Result<()>;
}
pub fn refresh(parent: u32, backend: &mut impl Backend) -> Result<()> {
    backend.store_translation_override_126(1)?;
    let mode = backend.mode_353ae8()?;
    if !(0..8).contains(&mode) {
        backend.copy_parent_pose_296f98(parent)?;
        let scale = backend.one_4cd748()?;
        backend.store_cache_scale_118(scale)
    } else {
        backend.skeletal_354a5c(parent, mode)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        mode: i32,
        flag: u8,
        scale: f32,
        calls: Vec<String>,
    }
    impl Backend for State {
        fn store_translation_override_126(&mut self, v: u8) -> Result<()> {
            self.calls.push("override".into());
            self.flag = v;
            Ok(())
        }
        fn mode_353ae8(&mut self) -> Result<i32> {
            self.calls.push("mode".into());
            Ok(self.mode)
        }
        fn copy_parent_pose_296f98(&mut self, p: u32) -> Result<()> {
            assert_eq!(p, 70);
            self.calls.push("parent pose".into());
            self.scale = 9.;
            Ok(())
        }
        fn one_4cd748(&mut self) -> Result<f32> {
            self.calls.push("one".into());
            Ok(1.)
        }
        fn store_cache_scale_118(&mut self, v: f32) -> Result<()> {
            self.calls.push("scale".into());
            self.scale = v;
            Ok(())
        }
        fn skeletal_354a5c(&mut self, p: u32, m: i32) -> Result<()> {
            assert_eq!((p, m), (70, self.mode));
            self.calls.push("skeletal".into());
            anyhow::bail!("Skeletal branch unavailable")
        }
    }
    #[test]
    fn mode_range_controls_parent_copy_and_retains_override_on_failure() -> Result<()> {
        for mode in [i32::MIN, -1, 8, i32::MAX] {
            let mut s = State {
                mode,
                flag: 0,
                scale: 99.,
                calls: vec![],
            };
            refresh(70, &mut s)?;
            assert_eq!((s.flag, s.scale), (1, 1.));
            assert_eq!(s.calls, ["override", "mode", "parent pose", "one", "scale"]);
        }
        for mode in 0..8 {
            let mut s = State {
                mode,
                flag: 0,
                scale: 99.,
                calls: vec![],
            };
            assert!(refresh(70, &mut s).is_err());
            assert_eq!((s.flag, s.scale), (1, 99.));
            assert_eq!(s.calls, ["override", "mode", "skeletal"]);
        }
        Ok(())
    }
}
