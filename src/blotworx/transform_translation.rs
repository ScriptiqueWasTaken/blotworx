//! Transform translation publication, REL351bc4 /351c10. Actual child traversal,
//! related records and transform update remain mandatory backend operations.
use anyhow::Result;
pub trait Backend: super::transform_dirty::Backend {
    fn translation(&mut self) -> Result<[f32; 3]>;
    fn store(&mut self, value: [f32; 3]) -> Result<()>;
    fn flags_20(&mut self) -> Result<u8>;
    fn update(&mut self, flag: u32) -> Result<()>; //354828
}
/// Numeric equality preserves native signed-zero suppression; unordered NaNs
/// count as changed. Flags/related pointers are read after earlier callbacks.
pub fn set(value: [f32; 3], backend: &mut impl Backend) -> Result<bool> {
    if !set_inner(value, backend)? {
        return Ok(false);
    }
    if backend.flags_20()? & 8 == 0 {
        backend.update(0)?;
    }
    Ok(true)
}
/// REL351c10: world reconstruction uses this nonrecursive entry. The outer
/// REL351bc4 alone checks flags20 and converts world position back to local.
pub fn set_inner(value: [f32; 3], backend: &mut impl Backend) -> Result<bool> {
    if backend.translation()? == value {
        return Ok(false);
    }
    backend.store(value)?;
    super::transform_dirty::publish(backend)?;
    Ok(true)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Trace {
        value: [f32; 3],
        flags: u8,
        calls: Vec<String>,
        fail: bool,
    }
    impl Backend for Trace {
        fn translation(&mut self) -> Result<[f32; 3]> {
            self.calls.push("read".into());
            Ok(self.value)
        }
        fn store(&mut self, v: [f32; 3]) -> Result<()> {
            self.calls.push("store".into());
            self.value = v;
            Ok(())
        }
        fn flags_20(&mut self) -> Result<u8> {
            self.calls.push("flags".into());
            Ok(self.flags)
        }
        fn update(&mut self, f: u32) -> Result<()> {
            assert_eq!(f, 0);
            self.calls.push("update".into());
            Ok(())
        }
    }
    impl super::super::transform_dirty::Backend for Trace {
        fn mark_children(&mut self) -> Result<()> {
            self.calls.push("children".into());
            if self.fail {
                anyhow::bail!("Child traversal unavailable")
            };
            Ok(())
        }
        fn related(&mut self, o: usize) -> Result<Option<usize>> {
            self.calls.push(format!("related {o:x}"));
            Ok(Some(o))
        }
        fn mark(&mut self, r: usize, o: usize, v: u8) -> Result<()> {
            assert_eq!(v, 1);
            self.calls.push(format!("mark {r:x} {o:x}"));
            Ok(())
        }
    }
    #[test]
    fn native_translation_order_equality_lock_flag_and_partial_failure() -> Result<()> {
        let mut t = Trace {
            value: [0.; 3],
            flags: 0,
            calls: vec![],
            fail: false,
        };
        assert!(!set([-0., 0., 0.], &mut t)?);
        assert_eq!(t.value[0].to_bits(), 0);
        assert_eq!(t.calls, ["read"]);
        t.calls.clear();
        assert!(set([1., 2., 3.], &mut t)?);
        assert_eq!(
            t.calls,
            [
                "read",
                "store",
                "children",
                "related 134",
                "mark 134 56",
                "related 138",
                "mark 138 3e",
                "flags",
                "update"
            ]
        );
        t.calls.clear();
        t.flags = 8;
        assert!(set([2., 3., 4.], &mut t)?);
        assert_eq!(t.calls.last().unwrap(), "flags");
        t.calls.clear();
        t.fail = true;
        assert!(set([5., 6., 7.], &mut t).is_err());
        assert_eq!(t.value, [5., 6., 7.]);
        assert_eq!(t.calls, ["read", "store", "children"]);
        t.fail = false;
        t.value = [f32::NAN, 0., 0.];
        t.calls.clear();
        assert!(set([f32::NAN, 0., 0.], &mut t)?);
        assert!(t.calls.contains(&"store".into()));
        Ok(())
    }
    #[test]
    fn inner_world_publication_does_not_reenter_local_conversion() -> Result<()> {
        let mut t = Trace {
            value: [0.; 3],
            flags: 0,
            calls: vec![],
            fail: false,
        };
        assert!(set_inner([1., 2., 3.], &mut t)?);
        assert_eq!(
            t.calls,
            [
                "read",
                "store",
                "children",
                "related 134",
                "mark 134 56",
                "related 138",
                "mark 138 3e"
            ]
        );
        t.calls.clear();
        assert!(!set_inner([1., 2., 3.], &mut t)?);
        assert_eq!(t.calls, ["read"]);
        Ok(())
    }
}
