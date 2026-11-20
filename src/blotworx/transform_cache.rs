//! Detach cleanup REL356104/356160. Havok and Gamebryo references use
//! different headers; neither may be replaced with a generic drop/no-op.
use anyhow::Result;
pub trait HavokBackend {
    fn pointer(&mut self, offset: usize) -> Result<Option<usize>>;
    fn release(&mut self, object: usize) -> Result<()>; //hk_reference::release
    fn clear(&mut self, offset: usize) -> Result<()>;
}
pub fn release_havok(backend: &mut impl HavokBackend) -> Result<()> {
    for offset in [0x12c, 0x130] {
        if let Some(object) = backend.pointer(offset)? {
            backend.release(object)?;
            backend.clear(offset)?;
        }
    }
    Ok(())
}
pub trait RecordBackend {
    fn pointer_34(&mut self) -> Result<Option<usize>>;
    fn enter_8059e8e0(&mut self) -> Result<u32>;
    fn references(&mut self, object: usize) -> Result<u32>;
    fn store_references(&mut self, object: usize, value: u32) -> Result<()>;
    fn leave_8059e920(&mut self, flag: u32) -> Result<()>;
    fn destroy_virtual_0c(&mut self, object: usize) -> Result<()>;
    fn clear_pointer_34(&mut self) -> Result<()>;
    fn store_word_38(&mut self, value: u32) -> Result<()>;
    fn reset_transform(&mut self) -> Result<()>; //native_transform_record::reset
}
pub fn reset_record(backend: &mut impl RecordBackend) -> Result<()> {
    if let Some(object) = backend.pointer_34()? {
        let token = backend.enter_8059e8e0()?;
        let count = backend.references(object)?.wrapping_sub(1);
        backend.store_references(object, count)?;
        backend.leave_8059e920(u32::from(token != 0))?;
        if count == 0 {
            backend.destroy_virtual_0c(object)?;
        }
        backend.clear_pointer_34()?;
    }
    backend.store_word_38(u32::MAX)?;
    backend.reset_transform()
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        pointers: [Option<usize>; 2],
        object: Option<usize>,
        refs: u32,
        word: u32,
        calls: Vec<String>,
        fail: bool,
    }
    impl HavokBackend for State {
        fn pointer(&mut self, o: usize) -> Result<Option<usize>> {
            self.calls.push(format!("pointer {o:x}"));
            Ok(self.pointers[(o - 0x12c) / 4])
        }
        fn release(&mut self, o: usize) -> Result<()> {
            self.calls.push(format!("release {o}"));
            if self.fail {
                anyhow::bail!("Destructor unavailable")
            };
            self.pointers[1] = Some(3);
            Ok(())
        }
        fn clear(&mut self, o: usize) -> Result<()> {
            self.calls.push(format!("clear {o:x}"));
            self.pointers[(o - 0x12c) / 4] = None;
            Ok(())
        }
    }
    impl RecordBackend for State {
        fn pointer_34(&mut self) -> Result<Option<usize>> {
            self.calls.push("pointer34".into());
            Ok(self.object)
        }
        fn enter_8059e8e0(&mut self) -> Result<u32> {
            self.calls.push("enter".into());
            Ok(0x80000000)
        }
        fn references(&mut self, _: usize) -> Result<u32> {
            self.calls.push("refs".into());
            Ok(self.refs)
        }
        fn store_references(&mut self, _: usize, v: u32) -> Result<()> {
            self.calls.push(format!("store {v}"));
            self.refs = v;
            Ok(())
        }
        fn leave_8059e920(&mut self, f: u32) -> Result<()> {
            assert_eq!(f, 1);
            self.calls.push("leave".into());
            Ok(())
        }
        fn destroy_virtual_0c(&mut self, _: usize) -> Result<()> {
            self.calls.push("destroy".into());
            if self.fail {
                anyhow::bail!("Destructor unavailable")
            };
            Ok(())
        }
        fn clear_pointer_34(&mut self) -> Result<()> {
            self.calls.push("clear34".into());
            self.object = None;
            Ok(())
        }
        fn store_word_38(&mut self, v: u32) -> Result<()> {
            self.calls.push("word38".into());
            self.word = v;
            Ok(())
        }
        fn reset_transform(&mut self) -> Result<()> {
            self.calls.push("reset".into());
            Ok(())
        }
    }
    #[test]
    fn reference_cleanup_order_fresh_reads_and_partial_failure() -> Result<()> {
        let mut s = State {
            pointers: [Some(1), Some(2)],
            object: Some(4),
            refs: 1,
            word: 0,
            calls: vec![],
            fail: false,
        };
        release_havok(&mut s)?;
        assert_eq!(
            s.calls,
            [
                "pointer 12c",
                "release 1",
                "clear 12c",
                "pointer 130",
                "release 3",
                "clear 130"
            ]
        );
        s.calls.clear();
        reset_record(&mut s)?;
        assert_eq!(
            s.calls,
            [
                "pointer34",
                "enter",
                "refs",
                "store 0",
                "leave",
                "destroy",
                "clear34",
                "word38",
                "reset"
            ]
        );
        assert_eq!(s.word, u32::MAX);
        s.calls.clear();
        s.object = Some(4);
        s.refs = 1;
        s.fail = true;
        s.word = 123;
        assert!(reset_record(&mut s).is_err());
        assert_eq!(s.refs, 0);
        assert_eq!(s.object, Some(4));
        assert_eq!(s.word, 123);
        assert_eq!(s.calls.last().unwrap(), "destroy");
        s.fail = false;
        s.refs = 0;
        s.calls.clear();
        reset_record(&mut s)?;
        assert_eq!(s.refs, u32::MAX);
        assert!(!s.calls.contains(&"destroy".into()));
        Ok(())
    }
}
