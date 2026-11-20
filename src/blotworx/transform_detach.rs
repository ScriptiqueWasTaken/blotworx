//! REL353dc4..353edc: synchronization before typed parent assignment.
//! This routine does not itself clear the Parent property.
use anyhow::Result;
pub trait Backend {
    fn parent(&mut self) -> Result<u32>;
    fn component_3764(&mut self, parent: u32) -> Result<Option<usize>>;
    fn own_entity(&mut self) -> Result<Option<usize>>;
    fn detach_312cc8(&mut self, component: usize, entity: Option<usize>) -> Result<()>;
    fn synchronize_356104(&mut self) -> Result<()>;
    fn cached_11c(&mut self) -> Result<Option<usize>>;
    fn synchronize_356160(&mut self, record: usize) -> Result<()>;
    fn store_byte(&mut self, offset: usize, value: u8) -> Result<()>;
    fn update_354010(&mut self, entity: Option<usize>) -> Result<()>;
    fn reset_transform_e8(&mut self) -> Result<()>; //native_transform_record::reset
    fn update_3542e0(&mut self, flag: u32) -> Result<()>;
    fn parent_transform(&mut self, parent: u32) -> Result<usize>;
    fn entity_handle(&mut self, entity: usize) -> Result<u32>;
    fn remove_child(&mut self, parent: usize, child: u32) -> Result<()>; //Children::remove
}
pub fn detach(backend: &mut impl Backend) -> Result<()> {
    if backend.parent()? == 0 {
        return Ok(());
    }
    let parent = backend.parent()?;
    if let Some(component) = backend.component_3764(parent)? {
        let entity = backend.own_entity()?;
        backend.detach_312cc8(component, entity)?;
    }
    backend.synchronize_356104()?;
    if let Some(record) = backend.cached_11c()? {
        backend.synchronize_356160(record)?;
    }
    for offset in [0x126, 0x127, 0x128] {
        backend.store_byte(offset, 0)?;
    }
    backend.update_354010(None)?;
    backend.reset_transform_e8()?;
    backend.update_3542e0(1)?;
    let parent = backend.parent()?;
    let parent = backend.parent_transform(parent)?;
    let entity = backend.own_entity()?;
    let child = match entity {
        Some(e) => backend.entity_handle(e)?,
        None => 0,
    };
    backend.remove_child(parent, child)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::blotworx::{
        native_transform_record::{self, Constants},
        transform_child_list::Children,
    };
    struct State {
        parent: u32,
        record: [u8; 0x140],
        children: Children,
        calls: Vec<String>,
        fail: bool,
    }
    impl Backend for State {
        fn parent(&mut self) -> Result<u32> {
            self.calls.push("parent".into());
            Ok(self.parent)
        }
        fn component_3764(&mut self, p: u32) -> Result<Option<usize>> {
            assert_eq!(p, 1);
            self.calls.push("component".into());
            Ok(Some(3))
        }
        fn own_entity(&mut self) -> Result<Option<usize>> {
            self.calls.push("entity".into());
            Ok(Some(7))
        }
        fn detach_312cc8(&mut self, c: usize, e: Option<usize>) -> Result<()> {
            assert_eq!((c, e), (3, Some(7)));
            self.calls.push("component detach".into());
            Ok(())
        }
        fn synchronize_356104(&mut self) -> Result<()> {
            self.calls.push("sync".into());
            Ok(())
        }
        fn cached_11c(&mut self) -> Result<Option<usize>> {
            self.calls.push("cached".into());
            Ok(Some(4))
        }
        fn synchronize_356160(&mut self, r: usize) -> Result<()> {
            assert_eq!(r, 4);
            self.calls.push("cached sync".into());
            Ok(())
        }
        fn store_byte(&mut self, o: usize, v: u8) -> Result<()> {
            self.calls.push(format!("byte {o:x}"));
            self.record[o] = v;
            Ok(())
        }
        fn update_354010(&mut self, e: Option<usize>) -> Result<()> {
            assert_eq!(e, None);
            self.calls.push("update null".into());
            Ok(())
        }
        fn reset_transform_e8(&mut self) -> Result<()> {
            self.calls.push("reset".into());
            native_transform_record::reset(
                &mut self.record[0xe8..],
                Constants {
                    matrix_one: 0x3f800000,
                    matrix_zero: 0,
                    translation: [0; 3],
                    scale: 0x3f800000,
                },
            )
        }
        fn update_3542e0(&mut self, f: u32) -> Result<()> {
            assert_eq!(f, 1);
            self.calls.push("update".into());
            if self.fail {
                anyhow::bail!("Physical synchronization unavailable")
            };
            self.parent = 2;
            Ok(())
        }
        fn parent_transform(&mut self, p: u32) -> Result<usize> {
            assert_eq!(p, 2);
            self.calls.push("parent transform".into());
            Ok(9)
        }
        fn entity_handle(&mut self, e: usize) -> Result<u32> {
            assert_eq!(e, 7);
            self.calls.push("handle".into());
            Ok(70)
        }
        fn remove_child(&mut self, p: usize, c: u32) -> Result<()> {
            assert_eq!(p, 9);
            self.calls.push("remove".into());
            self.children.remove(c);
            Ok(())
        }
    }
    #[test]
    fn detach_order_actual_reset_child_removal_and_partial_failure() -> Result<()> {
        let mut s = State {
            parent: 1,
            record: [0xa5; 0x140],
            children: Children::default(),
            calls: vec![],
            fail: false,
        };
        for h in [70, 80, 90] {
            s.children
                .register(h, |_, _| anyhow::bail!("Unexpected lookup"))?;
        }
        detach(&mut s)?;
        assert_eq!(
            s.calls,
            [
                "parent",
                "parent",
                "component",
                "entity",
                "component detach",
                "sync",
                "cached",
                "cached sync",
                "byte 126",
                "byte 127",
                "byte 128",
                "update null",
                "reset",
                "update",
                "parent",
                "parent transform",
                "entity",
                "handle",
                "remove"
            ]
        );
        assert_eq!(s.children.entries(), [90, 80]);
        assert_eq!(&s.record[0x126..0x129], [0; 3]);
        assert_eq!(&s.record[0x118..0x11c], &0x3f800000u32.to_be_bytes());
        assert!(s.record[0x11c..0x126].iter().all(|b| *b == 0xa5));
        s.parent = 1;
        s.fail = true;
        s.calls.clear();
        assert!(detach(&mut s).is_err());
        assert_eq!(s.parent, 1);
        assert_eq!(s.calls.last().unwrap(), "update");
        assert!(!s.calls.contains(&"remove".into()));
        s.parent = 0;
        s.calls.clear();
        detach(&mut s)?;
        assert_eq!(s.calls, ["parent"]);
        Ok(())
    }
}
