//! Filter extraction297d28 and pairing297e9c. A newly assigned source group
//! intentionally does not replace the earlier captured zero used for child.
use super::body_group_assignment;
use anyhow::Result;
pub trait Backend: body_group_assignment::Backend {
    fn filter_info_2c(&mut self, body: usize) -> Result<u32>;
    fn reserve_shared_group(&mut self) -> Result<u32>;
    fn assign_source(
        &mut self,
        entity: Option<usize>,
        group: u32,
        value: u32,
        third_holder: usize,
    ) -> Result<bool>;
}
pub fn extract(entity: Option<usize>, backend: &mut impl Backend) -> Result<Option<[u32; 3]>> {
    let Some(component) = backend.component(entity)? else {
        return Ok(None);
    };
    if backend.body_virtual_368(component, 0)?.is_none() {
        return Ok(None);
    }
    let body = backend
        .body_virtual_368(component, 0)?
        .ok_or_else(|| anyhow::anyhow!("Body disappeared before filter extraction297d28"))?;
    let filter = backend.filter_info_2c(body)?;
    Ok(Some([
        filter >> 17,
        (filter >> 5) & 31,
        (filter >> 10) & 31,
    ]))
}
pub fn pair(
    source: Option<usize>,
    child: Option<usize>,
    value: u32,
    backend: &mut impl Backend,
) -> Result<()> {
    let Some([group, sub_a, _sub_b]) = extract(source, backend)? else {
        return Ok(());
    };
    let Some(component) = backend.component(child)? else {
        return Ok(());
    };
    if backend.body_virtual_368(component, 0)?.is_none() {
        return Ok(());
    }
    if group == 0 {
        let allocated = backend.reserve_shared_group()?;
        // Return boolean from297ddc is ignored by native caller.
        backend.assign_source(source, allocated, 6, 0x4e63b4)?;
    }
    backend.publish_virtual_37c(component, group, value, sub_a)
}
#[cfg(test)]
mod tests {
    use super::super::{
        body_group_assignment::Backend as Assignment, physics_filter::GroupCounter,
    };
    use super::*;
    struct State {
        filter: u32,
        groups: GroupCounter,
        calls: Vec<String>,
    }
    impl Assignment for State {
        fn component(&mut self, e: Option<usize>) -> Result<Option<usize>> {
            self.calls.push(format!("component {e:?}"));
            Ok(e)
        }
        fn body_virtual_368(&mut self, c: usize, f: u32) -> Result<Option<usize>> {
            assert_eq!(f, 0);
            self.calls.push(format!("body {c}"));
            Ok(Some(c + 10))
        }
        fn userdata_242f6c(&mut self, _: usize) -> Result<Option<usize>> {
            anyhow::bail!("Unused")
        }
        fn userdata_242fb0(&mut self, _: usize) -> Result<Option<usize>> {
            anyhow::bail!("Unused")
        }
        fn input_word(&mut self, _: usize) -> Result<u32> {
            anyhow::bail!("Unused")
        }
        fn store_word(&mut self, _: usize, _: usize, _: u32) -> Result<()> {
            anyhow::bail!("Unused")
        }
        fn read_word(&mut self, _: usize, _: usize) -> Result<u32> {
            anyhow::bail!("Unused")
        }
        fn publish_virtual_37c(&mut self, c: usize, g: u32, v: u32, t: u32) -> Result<()> {
            self.calls.push(format!("publish {c} {g} {v} {t}"));
            Ok(())
        }
    }
    impl Backend for State {
        fn filter_info_2c(&mut self, b: usize) -> Result<u32> {
            assert_eq!(b, 11);
            self.calls.push("filter".into());
            Ok(self.filter)
        }
        fn reserve_shared_group(&mut self) -> Result<u32> {
            self.calls.push("reserve".into());
            Ok(self.groups.reserve())
        }
        fn assign_source(&mut self, e: Option<usize>, g: u32, v: u32, h: usize) -> Result<bool> {
            assert_eq!((e, g, v, h), (Some(1), 1, 6, 0x4e63b4));
            self.calls.push("assign source".into());
            self.filter = 1 << 17;
            Ok(false)
        }
    }
    #[test]
    fn captured_zero_group_survives_source_assignment_and_subfields_extract() -> Result<()> {
        let mut s = State {
            filter: 3 << 5 | 7 << 10,
            groups: GroupCounter { value: 0 },
            calls: vec![],
        };
        pair(Some(1), Some(2), 4, &mut s)?;
        assert_eq!(
            s.calls,
            [
                "component Some(1)",
                "body 1",
                "body 1",
                "filter",
                "component Some(2)",
                "body 2",
                "reserve",
                "assign source",
                "publish 2 0 4 3"
            ]
        );
        s.filter = 0xffff0000 | 31 << 5 | 17 << 10;
        s.calls.clear();
        assert_eq!(extract(Some(1), &mut s)?, Some([0x7fff, 31, 17]));
        s.calls.clear();
        pair(Some(1), Some(2), 9, &mut s)?;
        assert_eq!(s.calls.last().unwrap(), "publish 2 32767 9 31");
        assert_eq!(s.groups.value, 1);
        s.calls.clear();
        pair(None, Some(2), 4, &mut s)?;
        assert_eq!(s.calls, ["component None"]);
        Ok(())
    }
}
