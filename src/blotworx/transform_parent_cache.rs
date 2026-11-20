//! Parent cache refresh354930 dispatch and unattached35599c branch.
//! Skeletal/attachment continuation354a14 remains a mandatory backend operation.
use super::{
    native_matrix::Matrix,
    native_transform_record::{self, Transform},
};
use anyhow::Result;
pub trait Backend {
    fn parent(&mut self) -> Result<u32>;
    fn cached_record_11c(&mut self) -> Result<usize>;
    fn store_byte(&mut self, offset: usize, value: u8) -> Result<()>;
    fn record_word(&mut self, record: usize, offset: usize) -> Result<u32>;
    fn attached_354a14(&mut self, record: usize, parent: u32) -> Result<()>;
    fn parent_transform(&mut self, parent: u32) -> Result<usize>;
    fn one_4cd748(&mut self) -> Result<f32>;
    fn parent_position(&mut self, transform: usize) -> Result<[f32; 3]>;
    fn parent_rotation(&mut self, transform: usize) -> Result<Matrix>;
    fn default_transform_804c026c(&mut self) -> Result<Transform>;
    fn mode_353ae8(&mut self) -> Result<i32>;
    fn identity_80767b64(&mut self) -> Result<Matrix>;
    fn store_cached_parent_e8(&mut self, value: Transform) -> Result<()>;
}
pub fn refresh(backend: &mut impl Backend) -> Result<()> {
    let parent = backend.parent()?;
    let record = backend.cached_record_11c()?;
    backend.store_byte(0x126, 0)?;
    backend.store_byte(0x127, 0)?;
    if backend.record_word(record, 0x38)? != u32::MAX || backend.record_word(record, 0x34)? != 0 {
        return backend.attached_354a14(record, parent);
    }
    let transform = backend.parent_transform(parent)?;
    let one = backend.one_4cd748()?;
    let position = backend.parent_position(transform)?;
    let mut rotation = backend.parent_rotation(transform)?;
    let default = backend.default_transform_804c026c()?;
    if backend.mode_353ae8()? == 7 {
        rotation = backend.identity_80767b64()?;
    }
    let parent: Transform = std::array::from_fn(|i| {
        if i < 9 {
            rotation[i]
        } else if i < 12 {
            position[i - 9]
        } else {
            one
        }
    });
    let local_rotation: Matrix = default[..9].try_into().unwrap();
    let (position, rotation) = native_transform_record::world_pose(
        &parent,
        [default[9], default[10], default[11]],
        &local_rotation,
    );
    let one = backend.one_4cd748()?; // Fresh final scale load355c60.
    let value = std::array::from_fn(|i| {
        if i < 9 {
            rotation[i]
        } else if i < 12 {
            position[i - 9]
        } else {
            one
        }
    });
    backend.store_cached_parent_e8(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        calls: Vec<String>,
        index: u32,
        object: u32,
        mode: i32,
        cache: Transform,
    }
    impl Backend for State {
        fn parent(&mut self) -> Result<u32> {
            self.calls.push("parent".into());
            Ok(70)
        }
        fn cached_record_11c(&mut self) -> Result<usize> {
            self.calls.push("record".into());
            Ok(100)
        }
        fn store_byte(&mut self, o: usize, v: u8) -> Result<()> {
            assert_eq!(v, 0);
            self.calls.push(format!("clear {o:x}"));
            Ok(())
        }
        fn record_word(&mut self, r: usize, o: usize) -> Result<u32> {
            assert_eq!(r, 100);
            self.calls.push(format!("word {o:x}"));
            Ok(if o == 0x38 { self.index } else { self.object })
        }
        fn attached_354a14(&mut self, _: usize, _: u32) -> Result<()> {
            self.calls.push("attached".into());
            anyhow::bail!("Attachment branch unavailable")
        }
        fn parent_transform(&mut self, p: u32) -> Result<usize> {
            assert_eq!(p, 70);
            self.calls.push("transform".into());
            Ok(700)
        }
        fn one_4cd748(&mut self) -> Result<f32> {
            self.calls.push("one".into());
            Ok(1.)
        }
        fn parent_position(&mut self, _: usize) -> Result<[f32; 3]> {
            self.calls.push("position".into());
            Ok([2., 4., 6.])
        }
        fn parent_rotation(&mut self, _: usize) -> Result<Matrix> {
            self.calls.push("rotation".into());
            Ok([0., 1., 0., -1., 0., 0., 0., 0., 1.])
        }
        fn default_transform_804c026c(&mut self) -> Result<Transform> {
            self.calls.push("default".into());
            Ok([1., 0., 0., 0., 1., 0., 0., 0., 1., 1., 2., 3., 1.])
        }
        fn mode_353ae8(&mut self) -> Result<i32> {
            self.calls.push("mode".into());
            Ok(self.mode)
        }
        fn identity_80767b64(&mut self) -> Result<Matrix> {
            self.calls.push("identity".into());
            Ok([1., 0., 0., 0., 1., 0., 0., 0., 1.])
        }
        fn store_cached_parent_e8(&mut self, v: Transform) -> Result<()> {
            self.calls.push("store".into());
            self.cache = v;
            Ok(())
        }
    }
    #[test]
    fn unattached_cache_mode7_and_attached_gate_preserve_native_order() -> Result<()> {
        let mut s = State {
            calls: vec![],
            index: u32::MAX,
            object: 0,
            mode: 0,
            cache: [99.; 13],
        };
        refresh(&mut s)?;
        assert_eq!(&s.cache[9..], [4., 3., 9., 1.]);
        assert_eq!(
            s.calls,
            [
                "parent",
                "record",
                "clear 126",
                "clear 127",
                "word 38",
                "word 34",
                "transform",
                "one",
                "position",
                "rotation",
                "default",
                "mode",
                "one",
                "store"
            ]
        );
        s.calls.clear();
        s.mode = 7;
        refresh(&mut s)?;
        assert_eq!(&s.cache[9..], [3., 6., 9., 1.]);
        assert!(s.calls.contains(&"identity".into()));
        s.calls.clear();
        s.index = 0;
        let before = s.cache;
        assert!(refresh(&mut s).is_err());
        assert_eq!(s.cache, before);
        assert_eq!(
            s.calls,
            [
                "parent",
                "record",
                "clear 126",
                "clear 127",
                "word 38",
                "attached"
            ]
        );
        Ok(())
    }
}
