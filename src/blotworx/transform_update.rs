//! Dirty transform update352010. Cache refresh354930 is mandatory; marking
//! initialized and clearing dirty only happen after successful reconstruction.
use anyhow::Result;
pub trait Backend {
    fn byte(&mut self, offset: usize) -> Result<u8>;
    fn refresh_parent_354930(&mut self, flag: u32) -> Result<()>;
    fn rebuild_world_355d7c(&mut self, flag: u32) -> Result<()>;
    fn store_byte(&mut self, offset: usize, value: u8) -> Result<()>;
}
/// Host bool indicates that update ran, not a claim about native returnR3.
pub fn update(backend: &mut impl Backend) -> Result<bool> {
    if backend.byte(0x124)? == 0 {
        return Ok(false);
    }
    let flag = u32::from(backend.byte(0x125)? == 0);
    backend.refresh_parent_354930(flag)?;
    let flag = u32::from(backend.byte(0x125)? == 0);
    backend.rebuild_world_355d7c(flag)?;
    backend.store_byte(0x125, 1)?;
    backend.store_byte(0x124, 0)?;
    Ok(true)
}
pub trait Cache {
    fn refresh(
        &mut self,
        storage: &mut super::transform_storage::Storage<'_>,
        flag: u32,
    ) -> Result<()>;
}
pub struct Owned<'a, 's, H: super::transform_runtime::Hooks, C: Cache> {
    pub runtime: &'a mut super::transform_runtime::Runtime<'s, H>,
    pub cache: C,
}
impl<H: super::transform_runtime::Hooks, C: Cache> Backend for Owned<'_, '_, H, C> {
    fn byte(&mut self, o: usize) -> Result<u8> {
        self.runtime.storage.byte(o)
    }
    fn refresh_parent_354930(&mut self, f: u32) -> Result<()> {
        self.cache.refresh(&mut self.runtime.storage, f)
    }
    fn rebuild_world_355d7c(&mut self, _: u32) -> Result<()> {
        super::transform_world::update(self.runtime)
    }
    fn store_byte(&mut self, o: usize, v: u8) -> Result<()> {
        self.runtime.storage.store_byte(o, v)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        dirty: u8,
        initialized: u8,
        calls: Vec<String>,
        fail: bool,
    }
    impl Backend for State {
        fn byte(&mut self, o: usize) -> Result<u8> {
            self.calls.push(format!("read {o:x}"));
            Ok(if o == 0x124 {
                self.dirty
            } else {
                self.initialized
            })
        }
        fn refresh_parent_354930(&mut self, f: u32) -> Result<()> {
            self.calls.push(format!("refresh {f}"));
            self.initialized = 3;
            Ok(())
        }
        fn rebuild_world_355d7c(&mut self, f: u32) -> Result<()> {
            self.calls.push(format!("world {f}"));
            if self.fail {
                anyhow::bail!("World reconstruction unavailable")
            };
            Ok(())
        }
        fn store_byte(&mut self, o: usize, v: u8) -> Result<()> {
            self.calls.push(format!("store {o:x} {v}"));
            if o == 0x124 {
                self.dirty = v
            } else {
                self.initialized = v
            };
            Ok(())
        }
    }
    #[test]
    fn dirty_gate_fresh_initialized_flag_and_failure_retains_dirty() -> Result<()> {
        let mut s = State {
            dirty: 1,
            initialized: 0,
            calls: vec![],
            fail: false,
        };
        assert!(update(&mut s)?);
        assert_eq!(
            s.calls,
            [
                "read 124",
                "read 125",
                "refresh 1",
                "read 125",
                "world 0",
                "store 125 1",
                "store 124 0"
            ]
        );
        assert_eq!((s.dirty, s.initialized), (0, 1));
        s.calls.clear();
        assert!(!update(&mut s)?);
        assert_eq!(s.calls, ["read 124"]);
        s.calls.clear();
        s.dirty = 1;
        s.initialized = 2;
        s.fail = true;
        assert!(update(&mut s).is_err());
        assert_eq!((s.dirty, s.initialized), (1, 3));
        assert_eq!(
            s.calls,
            ["read 124", "read 125", "refresh 0", "read 125", "world 0"]
        );
        Ok(())
    }
}
