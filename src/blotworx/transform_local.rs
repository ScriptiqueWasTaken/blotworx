//! World-to-local publication3542e0, rotation-only354638 and local setters
//! 351e10/351ef0. Local writes don't perform world setter child invalidation.
use super::{
    native_matrix::Matrix,
    native_transform_record::{self, Transform},
};
use anyhow::Result;
pub trait Backend {
    fn world_position(&mut self) -> Result<[f32; 3]>;
    fn world_rotation(&mut self) -> Result<Matrix>;
    fn cached_parent_e8(&mut self) -> Result<Transform>;
    fn byte(&mut self, offset: usize) -> Result<u8>;
    fn zero_4cd728(&mut self) -> Result<f32>;
    fn identity_80767b64(&mut self) -> Result<Matrix>;
    fn set_local_position_351e10(&mut self, value: [f32; 3]) -> Result<()>;
    fn set_local_rotation_351ef0(&mut self, value: Matrix) -> Result<()>;
}
pub fn pose(backend: &mut impl Backend) -> Result<()> {
    let position = backend.world_position()?;
    let rotation = backend.world_rotation()?;
    let parent = backend.cached_parent_e8()?;
    let (mut position, mut rotation) =
        native_transform_record::local_pose(&parent, position, &rotation);
    if backend.byte(0x126)? != 0 {
        position = [backend.zero_4cd728()?; 3];
    }
    backend.set_local_position_351e10(position)?;
    // Must reread after the position setter, which may alter component state.
    if backend.byte(0x127)? != 0 {
        rotation = backend.identity_80767b64()?;
    }
    backend.set_local_rotation_351ef0(rotation)
}
pub fn rotation(backend: &mut impl Backend) -> Result<()> {
    let value = if backend.byte(0x127)? != 0 {
        backend.identity_80767b64()?
    } else {
        let world = backend.world_rotation()?;
        let parent = backend.cached_parent_e8()?;
        let parent: Matrix = parent[..9].try_into().unwrap();
        super::native_matrix::local_rotation(&parent, &world)
    };
    backend.set_local_rotation_351ef0(value)
}
/// REL354828 position-only conversion; override avoids reading world/cache.
pub fn position(backend: &mut impl Backend) -> Result<()> {
    let value = if backend.byte(0x126)? != 0 {
        [backend.zero_4cd728()?; 3]
    } else {
        let position = backend.world_position()?;
        let parent = backend.cached_parent_e8()?;
        let inverse = native_transform_record::inverse(&parent);
        let matrix: Matrix = inverse[..9].try_into().unwrap();
        let rotated = super::native_matrix::transform(&matrix, position);
        std::array::from_fn(|i| rotated[i] * inverse[12] + inverse[9 + i])
    };
    backend.set_local_position_351e10(value)
}
/// Numeric comparisons retain signed-zero suppression and NaN changed state.
/// Typed property wrappers still own actual publication in production.
pub fn set_position(current: &mut [f32; 3], value: [f32; 3]) -> bool {
    if *current == value {
        false
    } else {
        *current = value;
        true
    }
}
pub fn set_rotation(current: &mut Matrix, value: Matrix) -> bool {
    if *current == value {
        false
    } else {
        *current = value;
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const ID: Matrix = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
    struct State {
        position: [f32; 3],
        rotation: Matrix,
        flags: [u8; 2],
        calls: Vec<String>,
    }
    impl Backend for State {
        fn world_position(&mut self) -> Result<[f32; 3]> {
            self.calls.push("world position".into());
            Ok([10., 14., 18.])
        }
        fn world_rotation(&mut self) -> Result<Matrix> {
            self.calls.push("world rotation".into());
            Ok([0., 1., 0., -1., 0., 0., 0., 0., 1.])
        }
        fn cached_parent_e8(&mut self) -> Result<Transform> {
            self.calls.push("parent".into());
            Ok([0., 1., 0., -1., 0., 0., 0., 0., 1., 2., 4., 6., 2.])
        }
        fn byte(&mut self, o: usize) -> Result<u8> {
            self.calls.push(format!("flag {o:x}"));
            Ok(self.flags[o - 0x126])
        }
        fn zero_4cd728(&mut self) -> Result<f32> {
            self.calls.push("zero".into());
            Ok(0.)
        }
        fn identity_80767b64(&mut self) -> Result<Matrix> {
            self.calls.push("identity".into());
            Ok(ID)
        }
        fn set_local_position_351e10(&mut self, v: [f32; 3]) -> Result<()> {
            self.calls.push("position".into());
            set_position(&mut self.position, v);
            self.flags[1] = 1;
            Ok(())
        }
        fn set_local_rotation_351ef0(&mut self, v: Matrix) -> Result<()> {
            self.calls.push("rotation".into());
            set_rotation(&mut self.rotation, v);
            Ok(())
        }
    }
    #[test]
    fn pose_override_order_rotation_shortcut_and_numeric_setters() -> Result<()> {
        let mut s = State {
            position: [99.; 3],
            rotation: [99.; 9],
            flags: [0; 2],
            calls: vec![],
        };
        pose(&mut s)?;
        assert_eq!(s.position, [-5., 4., 6.]);
        assert_eq!(s.rotation, ID);
        assert_eq!(
            s.calls,
            [
                "world position",
                "world rotation",
                "parent",
                "flag 126",
                "position",
                "flag 127",
                "identity",
                "rotation"
            ]
        );
        s.calls.clear();
        s.flags[0] = 1;
        pose(&mut s)?;
        assert_eq!(s.position, [0.; 3]);
        assert!(s.calls.contains(&"zero".into()));
        s.calls.clear();
        rotation(&mut s)?;
        assert_eq!(s.calls, ["flag 127", "identity", "rotation"]);
        s.flags[1] = 0;
        s.calls.clear();
        rotation(&mut s)?;
        assert_eq!(
            s.calls,
            ["flag 127", "world rotation", "parent", "rotation"]
        );
        let mut v = [0.; 3];
        assert!(!set_position(&mut v, [-0., 0., 0.]));
        assert_eq!(v[0].to_bits(), 0);
        assert!(set_position(&mut v, [f32::NAN, 0., 0.]));
        assert!(set_position(&mut v, [f32::NAN, 0., 0.]));
        let mut r = ID;
        r[1] = -0.;
        assert!(!set_rotation(&mut r, ID));
        assert_eq!(r[1].to_bits(), 0x80000000);
        Ok(())
    }
}
