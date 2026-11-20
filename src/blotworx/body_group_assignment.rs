//! Entity group assignment297ddc and restoration298088. Virtual368 returns
//! the current body; virtual37c performs required native filter publication.
use anyhow::Result;
pub trait Backend {
    fn component(&mut self, entity: Option<usize>) -> Result<Option<usize>>;
    fn body_virtual_368(&mut self, component: usize, flag: u32) -> Result<Option<usize>>;
    fn userdata_242f6c(&mut self, component: usize) -> Result<Option<usize>>;
    fn userdata_242fb0(&mut self, body: usize) -> Result<Option<usize>>;
    fn input_word(&mut self, index: usize) -> Result<u32>;
    fn store_word(&mut self, record: usize, offset: usize, value: u32) -> Result<()>;
    fn read_word(&mut self, record: usize, offset: usize) -> Result<u32>;
    fn publish_virtual_37c(
        &mut self,
        component: usize,
        group: u32,
        value: u32,
        third: u32,
    ) -> Result<()>;
}
pub fn assign(entity: Option<usize>, backend: &mut impl Backend) -> Result<bool> {
    let Some(component) = backend.component(entity)? else {
        return Ok(false);
    };
    if backend.body_virtual_368(component, 0)?.is_none() {
        return Ok(false);
    }
    if let Some(record) = backend.userdata_242f6c(component)? {
        let group = backend.input_word(0)?;
        backend.store_word(record, 0xc, group)?;
        let value = backend.input_word(1)?;
        backend.store_word(record, 0x10, value)?;
    }
    let group = backend.input_word(0)?;
    let value = backend.input_word(1)?;
    let third = backend.input_word(2)?;
    backend.publish_virtual_37c(component, group, value, third)?;
    Ok(true)
}
pub fn restore(entity: Option<usize>, backend: &mut impl Backend) -> Result<()> {
    let Some(component) = backend.component(entity)? else {
        return Ok(());
    };
    if backend.body_virtual_368(component, 0)?.is_none() {
        return Ok(());
    }
    // Native calls virtual368 again; component callbacks may replace the body.
    let body = backend.body_virtual_368(component, 0)?;
    let body = body
        .ok_or_else(|| anyhow::anyhow!("Body disappeared before required userdata lookup242fb0"))?;
    let (group, value) = if let Some(record) = backend.userdata_242fb0(body)? {
        (
            backend.read_word(record, 0xc)?,
            backend.read_word(record, 0x10)?,
        )
    } else {
        (0, 0)
    };
    backend.publish_virtual_37c(component, group, value, 0)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        inputs: [u32; 3],
        words: [u32; 2],
        body_reads: u32,
        calls: Vec<String>,
        fail: bool,
    }
    impl Backend for State {
        fn component(&mut self, _: Option<usize>) -> Result<Option<usize>> {
            self.calls.push("component".into());
            Ok(Some(1))
        }
        fn body_virtual_368(&mut self, _: usize, f: u32) -> Result<Option<usize>> {
            assert_eq!(f, 0);
            self.body_reads += 1;
            self.calls.push("body".into());
            Ok(Some(self.body_reads as usize))
        }
        fn userdata_242f6c(&mut self, _: usize) -> Result<Option<usize>> {
            self.calls.push("component data".into());
            Ok(Some(4))
        }
        fn userdata_242fb0(&mut self, b: usize) -> Result<Option<usize>> {
            assert_eq!(b, 2);
            self.calls.push("body data".into());
            Ok(Some(4))
        }
        fn input_word(&mut self, i: usize) -> Result<u32> {
            self.calls.push(format!("input {i}"));
            Ok(self.inputs[i])
        }
        fn store_word(&mut self, _: usize, o: usize, v: u32) -> Result<()> {
            self.calls.push(format!("store {o:x}"));
            self.words[(o - 0xc) / 4] = v;
            self.inputs[0] = 8;
            Ok(())
        }
        fn read_word(&mut self, _: usize, o: usize) -> Result<u32> {
            self.calls.push(format!("read {o:x}"));
            Ok(self.words[(o - 0xc) / 4])
        }
        fn publish_virtual_37c(&mut self, _: usize, g: u32, v: u32, t: u32) -> Result<()> {
            self.calls.push(format!("publish {g} {v} {t}"));
            if self.fail {
                anyhow::bail!("Filter backend unavailable")
            };
            Ok(())
        }
    }
    #[test]
    fn group_userdata_publication_and_restore_fresh_body() -> Result<()> {
        let mut s = State {
            inputs: [7, 4, 99],
            words: [0; 2],
            body_reads: 0,
            calls: vec![],
            fail: false,
        };
        assert!(assign(Some(1), &mut s)?);
        assert_eq!(s.words, [7, 4]);
        assert_eq!(
            s.calls,
            [
                "component",
                "body",
                "component data",
                "input 0",
                "store c",
                "input 1",
                "store 10",
                "input 0",
                "input 1",
                "input 2",
                "publish 8 4 99"
            ]
        );
        s.calls.clear();
        s.body_reads = 0;
        restore(Some(1), &mut s)?;
        assert_eq!(
            s.calls,
            [
                "component",
                "body",
                "body",
                "body data",
                "read c",
                "read 10",
                "publish 7 4 0"
            ]
        );
        s.fail = true;
        s.inputs = [11, 6, 0];
        s.calls.clear();
        assert!(assign(Some(1), &mut s).is_err());
        assert_eq!(s.words, [11, 6]);
        Ok(())
    }
}
