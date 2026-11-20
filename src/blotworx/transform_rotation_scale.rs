//! Inner rotation351d28, outer351cdc and scale351b28.
use super::{native_matrix::Matrix, transform_dirty};
use anyhow::Result;
pub trait Backend: transform_dirty::Backend {
    fn rotation(&mut self) -> Result<Matrix>;
    fn store_rotation(&mut self, value: Matrix) -> Result<()>;
    fn scale(&mut self) -> Result<f32>;
    fn store_scale(&mut self, value: f32) -> Result<()>;
    fn flags_20(&mut self) -> Result<u8>;
    fn local_rotation_354638(&mut self, flag: u32) -> Result<()>;
}
pub fn rotation_inner(value: Matrix, backend: &mut impl Backend) -> Result<bool> {
    if backend.rotation()? == value {
        return Ok(false);
    }
    backend.store_rotation(value)?;
    transform_dirty::publish(backend)?;
    Ok(true)
}
pub fn rotation(value: Matrix, backend: &mut impl Backend) -> Result<bool> {
    if !rotation_inner(value, backend)? {
        return Ok(false);
    }
    if backend.flags_20()? & 8 == 0 {
        backend.local_rotation_354638(0)?;
    }
    Ok(true)
}
pub fn scale(value: f32, backend: &mut impl Backend) -> Result<bool> {
    if backend.scale()? == value {
        return Ok(false);
    }
    backend.store_scale(value)?;
    transform_dirty::publish(backend)?;
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        r: Matrix,
        s: f32,
        flags: u8,
        calls: Vec<String>,
    }
    impl transform_dirty::Backend for State {
        fn mark_children(&mut self) -> Result<()> {
            self.calls.push("children".into());
            Ok(())
        }
        fn related(&mut self, o: usize) -> Result<Option<usize>> {
            self.calls.push(format!("related {o:x}"));
            Ok(Some(o))
        }
        fn mark(&mut self, _: usize, o: usize, v: u8) -> Result<()> {
            assert_eq!(v, 1);
            self.calls.push(format!("mark {o:x}"));
            Ok(())
        }
    }
    impl Backend for State {
        fn rotation(&mut self) -> Result<Matrix> {
            self.calls.push("rotation".into());
            Ok(self.r)
        }
        fn store_rotation(&mut self, v: Matrix) -> Result<()> {
            self.calls.push("store rotation".into());
            self.r = v;
            Ok(())
        }
        fn scale(&mut self) -> Result<f32> {
            self.calls.push("scale".into());
            Ok(self.s)
        }
        fn store_scale(&mut self, v: f32) -> Result<()> {
            self.calls.push("store scale".into());
            self.s = v;
            Ok(())
        }
        fn flags_20(&mut self) -> Result<u8> {
            self.calls.push("flags".into());
            Ok(self.flags)
        }
        fn local_rotation_354638(&mut self, f: u32) -> Result<()> {
            assert_eq!(f, 0);
            self.calls.push("local".into());
            Ok(())
        }
    }
    #[test]
    fn rotation_scale_inner_outer_and_numeric_equality() -> Result<()> {
        let mut s = State {
            r: [0.; 9],
            s: 0.,
            flags: 0,
            calls: vec![],
        };
        assert!(rotation([1.; 9], &mut s)?);
        assert_eq!(
            s.calls,
            [
                "rotation",
                "store rotation",
                "children",
                "related 134",
                "mark 56",
                "related 138",
                "mark 3e",
                "flags",
                "local"
            ]
        );
        s.calls.clear();
        s.flags = 8;
        assert!(rotation([2.; 9], &mut s)?);
        assert_eq!(s.calls.last().unwrap(), "flags");
        s.calls.clear();
        assert!(rotation_inner([3.; 9], &mut s)?);
        assert!(!s.calls.contains(&"flags".into()));
        s.calls.clear();
        assert!(!scale(-0., &mut s)?);
        assert_eq!(s.s.to_bits(), 0);
        assert_eq!(s.calls, ["scale"]);
        s.calls.clear();
        assert!(scale(f32::NAN, &mut s)?);
        assert_eq!(
            s.calls,
            [
                "scale",
                "store scale",
                "children",
                "related 134",
                "mark 56",
                "related 138",
                "mark 3e"
            ]
        );
        Ok(())
    }
}
