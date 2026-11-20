//! REL2b2400..2b2468. Forward/reverse insertion consume the actual counter
//! holder; don't snapshot one key before callbacks or roll back partial writes.
use anyhow::Result;
pub trait Backend {
    fn insert_forward_2c1790(&mut self, entity: u32) -> Result<()>;
    fn insert_reverse_2c1c14(&mut self, entity: u32) -> Result<()>;
    fn counter_40(&mut self) -> Result<u32>;
    fn store_counter_40(&mut self, value: u32) -> Result<()>;
}
pub fn register(entity: u32, backend: &mut impl Backend) -> Result<()> {
    backend.insert_forward_2c1790(entity)?;
    backend.insert_reverse_2c1c14(entity)?;
    let next = backend.counter_40()?.wrapping_add(1);
    backend.store_counter_40(next)?;
    if next == 0 {
        backend.store_counter_40(1)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        counter: u32,
        forward: Vec<(u32, u32)>,
        reverse: Vec<(u32, u32)>,
        calls: Vec<String>,
        fail: bool,
    }
    impl Backend for State {
        fn insert_forward_2c1790(&mut self, e: u32) -> Result<()> {
            self.calls.push("forward".into());
            self.forward.push((self.counter, e));
            Ok(())
        }
        fn insert_reverse_2c1c14(&mut self, e: u32) -> Result<()> {
            self.calls.push("reverse".into());
            if self.fail {
                anyhow::bail!("Reverse insertion unavailable")
            };
            self.reverse.push((e, self.counter));
            Ok(())
        }
        fn counter_40(&mut self) -> Result<u32> {
            self.calls.push("counter".into());
            Ok(self.counter)
        }
        fn store_counter_40(&mut self, v: u32) -> Result<()> {
            self.calls.push(format!("store {v}"));
            self.counter = v;
            Ok(())
        }
    }
    #[test]
    fn paired_publication_precedes_counter_advance_and_wrap_skips_zero() -> Result<()> {
        let mut s = State {
            counter: 1,
            forward: vec![],
            reverse: vec![],
            calls: vec![],
            fail: false,
        };
        register(70, &mut s)?;
        assert_eq!(s.calls, ["forward", "reverse", "counter", "store 2"]);
        assert_eq!(s.forward, [(1, 70)]);
        assert_eq!(s.reverse, [(70, 1)]);
        s.calls.clear();
        s.counter = u32::MAX;
        register(80, &mut s)?;
        assert_eq!(
            s.calls,
            ["forward", "reverse", "counter", "store 0", "store 1"]
        );
        assert_eq!(s.forward.last(), Some(&(u32::MAX, 80)));
        s.calls.clear();
        s.fail = true;
        assert!(register(90, &mut s).is_err());
        assert_eq!(s.calls, ["forward", "reverse"]);
        assert_eq!(s.counter, 1);
        assert_eq!(s.forward.last(), Some(&(1, 90)));
        assert_eq!(s.reverse.len(), 2);
        Ok(())
    }
}
