//! World pose reconstruction355d7c. The flag argument isn't consumed by the
//! native routine; world setters here are the inner, nonrecursive variants.
use super::{
    native_matrix::Matrix,
    native_transform_record::{self, Transform},
};
use anyhow::Result;
pub trait Backend {
    fn byte(&mut self, offset: usize) -> Result<u8>;
    fn local_position(&mut self) -> Result<[f32; 3]>;
    fn local_rotation(&mut self) -> Result<Matrix>;
    fn zero_4cd728(&mut self) -> Result<f32>;
    fn identity_80767b64(&mut self) -> Result<Matrix>;
    fn one_4cd748(&mut self) -> Result<f32>;
    fn cached_parent_e8(&mut self) -> Result<Transform>;
    fn set_world_position_351c10(&mut self, value: [f32; 3]) -> Result<()>;
    fn set_world_rotation_351d28(&mut self, value: Matrix) -> Result<()>;
    fn set_world_scale_351b28(&mut self, value: f32) -> Result<()>;
}
pub fn update(backend: &mut impl Backend) -> Result<()> {
    let position = if backend.byte(0x126)? != 0 {
        [backend.zero_4cd728()?; 3]
    } else {
        backend.local_position()?
    };
    let rotation = if backend.byte(0x127)? != 0 {
        backend.identity_80767b64()?
    } else {
        backend.local_rotation()?
    };
    let one = backend.one_4cd748()?;
    let parent = backend.cached_parent_e8()?;
    let scale = parent[12] * one;
    let (position, rotation) = native_transform_record::world_pose(&parent, position, &rotation);
    backend.set_world_position_351c10(position)?;
    backend.set_world_rotation_351d28(rotation)?;
    if backend.byte(0x128)? != 0 {
        backend.set_world_scale_351b28(scale)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    const ID: Matrix = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
    struct State {
        flags: [u8; 3],
        position: [f32; 3],
        rotation: Matrix,
        scale: f32,
        calls: Vec<String>,
    }
    impl Backend for State {
        fn byte(&mut self, o: usize) -> Result<u8> {
            self.calls.push(format!("flag {o:x}"));
            Ok(self.flags[o - 0x126])
        }
        fn local_position(&mut self) -> Result<[f32; 3]> {
            self.calls.push("local position".into());
            Ok([-5., 4., 6.])
        }
        fn local_rotation(&mut self) -> Result<Matrix> {
            self.calls.push("local rotation".into());
            Ok(ID)
        }
        fn zero_4cd728(&mut self) -> Result<f32> {
            self.calls.push("zero".into());
            Ok(0.)
        }
        fn identity_80767b64(&mut self) -> Result<Matrix> {
            self.calls.push("identity".into());
            Ok(ID)
        }
        fn one_4cd748(&mut self) -> Result<f32> {
            self.calls.push("one".into());
            Ok(1.)
        }
        fn cached_parent_e8(&mut self) -> Result<Transform> {
            self.calls.push("parent".into());
            Ok([0., 1., 0., -1., 0., 0., 0., 0., 1., 2., 4., 6., 2.])
        }
        fn set_world_position_351c10(&mut self, v: [f32; 3]) -> Result<()> {
            self.calls.push("position".into());
            self.position = v;
            Ok(())
        }
        fn set_world_rotation_351d28(&mut self, v: Matrix) -> Result<()> {
            self.calls.push("rotation".into());
            self.rotation = v;
            self.flags[2] = 1;
            Ok(())
        }
        fn set_world_scale_351b28(&mut self, v: f32) -> Result<()> {
            self.calls.push("scale".into());
            self.scale = v;
            Ok(())
        }
    }
    #[test]
    fn world_reconstruction_overrides_and_fresh_scale_gate() -> Result<()> {
        let mut s = State {
            flags: [0; 3],
            position: [0.; 3],
            rotation: [0.; 9],
            scale: 99.,
            calls: vec![],
        };
        update(&mut s)?;
        assert_eq!(s.position, [10., 14., 18.]);
        assert_eq!(s.rotation, [0., 1., 0., -1., 0., 0., 0., 0., 1.]);
        assert_eq!(s.scale, 2.);
        assert_eq!(
            s.calls,
            [
                "flag 126",
                "local position",
                "flag 127",
                "local rotation",
                "one",
                "parent",
                "position",
                "rotation",
                "flag 128",
                "scale"
            ]
        );
        s.flags = [1; 3];
        s.calls.clear();
        update(&mut s)?;
        assert_eq!(s.position, [2., 4., 6.]);
        assert!(!s.calls.contains(&"local position".into()));
        assert!(!s.calls.contains(&"local rotation".into()));
        Ok(())
    }
}
