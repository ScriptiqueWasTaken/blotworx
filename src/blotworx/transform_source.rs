//! REL354010..3541cc. Component names remain unverified; address labels
//! prevent assigning invented semantics to the native collision relationships.
use anyhow::Result;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Body3b850,
    Phantom7abe0,
    Special4da10,
    Other1bd00,
}
pub trait Backend {
    fn own_entity(&mut self) -> Result<Option<usize>>;
    fn component(&mut self, entity: Option<usize>, kind: Kind) -> Result<Option<usize>>;
    fn body_virtual_370(&mut self, body: usize, source: Option<usize>) -> Result<()>;
    fn phantom_378630(&mut self, phantom: usize, source: Option<usize>) -> Result<()>;
    fn disable_special(&mut self, component: usize) -> Result<()>; //virtual274, property2d0c0+4, false, flag0
    fn reserve_shared_group(&mut self) -> Result<u32>; //physics_filter::GroupCounter::reserve
    fn assign_297ddc(
        &mut self,
        entity: usize,
        group: u32,
        value: u32,
        name_offset: usize,
    ) -> Result<()>;
    fn pair_297e9c(&mut self, source: usize, own: Option<usize>, value: u32) -> Result<()>;
    fn remove_298088(&mut self, own: Option<usize>) -> Result<()>;
}
pub fn update(source: Option<usize>, backend: &mut impl Backend) -> Result<()> {
    let own = backend.own_entity()?;
    let body = backend.component(own, Kind::Body3b850)?;
    let phantom = backend.component(own, Kind::Phantom7abe0)?;
    if let Some(body) = body {
        backend.body_virtual_370(body, source)?;
    } else if let Some(phantom) = phantom {
        backend.phantom_378630(phantom, source)?;
    }
    if backend.component(source, Kind::Special4da10)?.is_some() {
        return Ok(());
    }
    let source_body = backend.component(source, Kind::Body3b850)?;
    let other = backend.component(own, Kind::Other1bd00)?;
    if let Some(source) = source {
        if source_body.is_none() {
            return Ok(());
        }
        let name = if other.is_some() {
            0x4f9f8c
        } else {
            let Some(special) = backend.component(own, Kind::Special4da10)? else {
                return Ok(());
            };
            backend.disable_special(special)?;
            0x4f9f90
        };
        let group = backend.reserve_shared_group()?;
        backend.assign_297ddc(source, group, 4, name)?;
        backend.pair_297e9c(source, own, 4)?;
    } else if other.is_some() || backend.component(own, Kind::Special4da10)?.is_some() {
        backend.remove_298088(own)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::blotworx::physics_filter::GroupCounter;
    struct State {
        body: bool,
        other: bool,
        special: bool,
        source_special: bool,
        groups: GroupCounter,
        calls: Vec<String>,
        fail: bool,
    }
    impl Backend for State {
        fn own_entity(&mut self) -> Result<Option<usize>> {
            self.calls.push("own".into());
            Ok(Some(1))
        }
        fn component(&mut self, e: Option<usize>, k: Kind) -> Result<Option<usize>> {
            self.calls.push(format!("query {e:?} {k:?}"));
            let present = match (e, k) {
                (Some(1), Kind::Body3b850) => self.body,
                (Some(1), Kind::Phantom7abe0) => true,
                (Some(1), Kind::Other1bd00) => self.other,
                (Some(1), Kind::Special4da10) => self.special,
                (Some(2), Kind::Special4da10) => self.source_special,
                (Some(2), Kind::Body3b850) => true,
                _ => false,
            };
            Ok(present.then_some(10))
        }
        fn body_virtual_370(&mut self, _: usize, _: Option<usize>) -> Result<()> {
            self.calls.push("body".into());
            Ok(())
        }
        fn phantom_378630(&mut self, _: usize, _: Option<usize>) -> Result<()> {
            self.calls.push("phantom".into());
            Ok(())
        }
        fn disable_special(&mut self, _: usize) -> Result<()> {
            self.calls.push("disable".into());
            Ok(())
        }
        fn reserve_shared_group(&mut self) -> Result<u32> {
            self.calls.push("reserve".into());
            Ok(self.groups.reserve())
        }
        fn assign_297ddc(&mut self, e: usize, g: u32, v: u32, n: usize) -> Result<()> {
            assert_eq!((e, g, v), (2, 1, 4));
            self.calls.push(format!("assign {n:x}"));
            if self.fail {
                anyhow::bail!("Missing group assignment backend")
            };
            Ok(())
        }
        fn pair_297e9c(&mut self, s: usize, o: Option<usize>, v: u32) -> Result<()> {
            assert_eq!((s, o, v), (2, Some(1), 4));
            self.calls.push("pair".into());
            Ok(())
        }
        fn remove_298088(&mut self, o: Option<usize>) -> Result<()> {
            assert_eq!(o, Some(1));
            self.calls.push("remove".into());
            Ok(())
        }
    }
    #[test]
    fn component_precedence_group_reservation_and_null_removal() -> Result<()> {
        let mut s = State {
            body: true,
            other: true,
            special: false,
            source_special: false,
            groups: GroupCounter { value: 0 },
            calls: vec![],
            fail: false,
        };
        update(Some(2), &mut s)?;
        assert_eq!(
            s.calls,
            [
                "own",
                "query Some(1) Body3b850",
                "query Some(1) Phantom7abe0",
                "body",
                "query Some(2) Special4da10",
                "query Some(2) Body3b850",
                "query Some(1) Other1bd00",
                "reserve",
                "assign 4f9f8c",
                "pair"
            ]
        );
        s.calls.clear();
        update(None, &mut s)?;
        assert_eq!(s.calls.last().unwrap(), "remove");
        assert_eq!(s.groups.value, 1);
        s.calls.clear();
        s.body = false;
        s.other = false;
        s.special = true;
        s.fail = true;
        s.groups.value = 0;
        assert!(update(Some(2), &mut s).is_err());
        assert!(s.calls.contains(&"phantom".into()));
        assert_eq!(
            &s.calls[s.calls.len() - 3..],
            ["disable", "reserve", "assign 4f9f90"]
        );
        assert_eq!(s.groups.value, 1);
        s.calls.clear();
        s.source_special = true;
        update(Some(2), &mut s)?;
        assert_eq!(s.calls.last().unwrap(), "query Some(2) Special4da10");
        Ok(())
    }
}
