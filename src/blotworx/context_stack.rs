//! Retail generic state transition1498b0, used by input manager1f80a0.
//! Concrete state enter/exit/query effects are mandatory, not no-op defaults.
use anyhow::{ensure, Context, Result};
#[derive(Clone, Debug, serde::Serialize)]
pub struct State {
    pub current: i32,
    pub previous: i32,
    pub stack: Vec<i32>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            current: -1,
            previous: -1,
            stack: vec![],
        }
    }
}
#[derive(Default, Debug)]
pub struct Transition {
    pub phase: i32,
}
pub trait Backend {
    fn lookup(&mut self, key: i32) -> Result<usize>;
    fn suspend_previous(&mut self, state: usize, previous: i32) -> Result<bool>;
    fn resume_notification(&mut self, state: usize) -> Result<bool>;
    fn enter(
        &mut self,
        state: usize,
        manager: &mut State,
        other: i32,
        context: &mut Transition,
    ) -> Result<()>;
    fn exit(
        &mut self,
        state: usize,
        manager: &mut State,
        other: i32,
        context: &mut Transition,
    ) -> Result<()>;
    fn poll(&mut self, state: usize, manager: &mut State, context: &mut Transition) -> Result<i32>;
    fn tick(&mut self, state: usize, manager: &mut State, context: &mut Transition) -> Result<()>;
}
impl State {
    fn push(&mut self, key: i32) -> Result<()> {
        ensure!(
            self.stack.len() < usize::from(u16::MAX),
            "Native context count cannot fit u16"
        );
        self.stack.try_reserve(1)?;
        self.stack.push(key);
        Ok(())
    }
    pub fn switch(
        &mut self,
        request: i32,
        context: &mut Transition,
        b: &mut impl Backend,
    ) -> Result<()> {
        if self.current == -1 {
            return Ok(());
        }
        let old = b.lookup(self.current)?;
        self.transition_from(old, request, context, b).map(|_| ())
    }
    ///149564: poll cached current state, transition, then update the cached
    ///selected state. Callbacks may change current; do not reselect afterward.
    pub fn update(&mut self, context: &mut Transition, b: &mut impl Backend) -> Result<()> {
        if self.current == -1 {
            return Ok(());
        }
        context.phase = 0;
        let old = b.lookup(self.current)?;
        let request = b.poll(old, self, context)?;
        let selected = self.transition_from(old, request, context, b)?;
        b.tick(selected, self, context)
    }
    fn transition_from(
        &mut self,
        old: usize,
        request: i32,
        context: &mut Transition,
        b: &mut impl Backend,
    ) -> Result<usize> {
        if request == -1 {
            return Ok(old);
        }
        if request == -2 {
            self.stack.pop();
            let top = *self
                .stack
                .last()
                .context("Native context return has no remaining stack entry")?;
            //149938 uses the same last entry for previous when depth >1.
            //Preserve this behavior rather than substituting the entry below it.
            self.previous = if self.stack.len() > 1 { top } else { -1 };
            self.current = top;
            let next = b.lookup(self.current)?;
            context.phase = 2;
            let current = self.current;
            b.exit(old, self, current, context)?;
            if b.resume_notification(next)? {
                let previous = self.previous;
                b.enter(next, self, previous, context)?;
            }
            context.phase = 0;
            return Ok(next);
        }
        self.previous = self.current;
        let next = b.lookup(request)?;
        self.current = request;
        if request as u32 & 0xa000 != 0 {
            context.phase = 2;
            let mut index = self.stack.len();
            while index > 0 {
                index -= 1;
                let key = *self
                    .stack
                    .get(index)
                    .context("Context callback invalidated a pending exit slot")?;
                let state = b.lookup(key)?;
                b.exit(state, self, request, context)?;
            }
            self.stack.clear();
            self.push(request)?;
            context.phase = 1;
            let previous = self.previous;
            b.enter(next, self, previous, context)?;
            context.phase = 0;
            return Ok(next);
        }
        if !b.suspend_previous(next, self.previous)? {
            self.stack.pop();
            context.phase = 2;
            let current = self.current;
            b.exit(old, self, current, context)?;
        } else if b.resume_notification(old)? {
            context.phase = 1;
            let current = self.current;
            b.exit(old, self, current, context)?;
        }
        self.push(request)?;
        context.phase = 1;
        let previous = self.previous;
        b.enter(next, self, previous, context)?;
        context.phase = 0;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        calls: Vec<Value>,
        counts: std::collections::BTreeMap<String, usize>,
    }
    impl Host<'_> {
        fn event(&mut self, op: &str, mut fields: Value) -> Result<()> {
            fields
                .as_object_mut()
                .unwrap()
                .insert("op".into(), op.into());
            self.calls.push(fields);
            let count = self.counts.entry(op.into()).or_default();
            *count += 1;
            ensure!(
                self.case["fail"].as_str() != Some(op)
                    && self.case["fail"].as_str() != Some(&format!("{op}:{count}")),
                "Supplied context service failure"
            );
            Ok(())
        }
        fn effect(
            &mut self,
            op: &str,
            state: usize,
            s: &mut State,
            other: i32,
            c: &mut Transition,
        ) -> Result<()> {
            self.event(
                op,
                json!({"state":state,"other":other,"manager":snapshot(s,c)}),
            )?;
            if self.case["mutation"]["on"] == op
                && self.counts[op]
                    == self.case["mutation"]["occurrence"].as_u64().unwrap_or(1) as usize
            {
                let m = &self.case["mutation"];
                if let Some(value) = m["current"].as_i64() {
                    s.current = value as i32;
                }
                if let Some(value) = m["previous"].as_i64() {
                    s.previous = value as i32;
                }
            }
            Ok(())
        }
    }
    impl Backend for Host<'_> {
        fn lookup(&mut self, key: i32) -> Result<usize> {
            self.event("lookup", json!({"key":key}))?;
            usize::try_from(key).context("Unknown supplied state")
        }
        fn suspend_previous(&mut self, state: usize, previous: i32) -> Result<bool> {
            self.event("suspend", json!({"state":state,"previous":previous}))?;
            Ok(self.case["suspend"].as_bool().unwrap())
        }
        fn resume_notification(&mut self, state: usize) -> Result<bool> {
            self.event("resume", json!({"state":state}))?;
            Ok(self.case["resume"].as_bool().unwrap())
        }
        fn enter(
            &mut self,
            state: usize,
            s: &mut State,
            other: i32,
            c: &mut Transition,
        ) -> Result<()> {
            self.effect("enter", state, s, other, c)
        }
        fn exit(
            &mut self,
            state: usize,
            s: &mut State,
            other: i32,
            c: &mut Transition,
        ) -> Result<()> {
            self.effect("exit", state, s, other, c)
        }
        fn poll(&mut self, state: usize, s: &mut State, c: &mut Transition) -> Result<i32> {
            self.event("poll", json!({"state":state,"manager":snapshot(s,c)}))?;
            Ok(self.case["request"]
                .as_i64()
                .context("Missing poll request")? as i32)
        }
        fn tick(&mut self, state: usize, s: &mut State, c: &mut Transition) -> Result<()> {
            self.event("tick", json!({"state":state,"manager":snapshot(s,c)}))
        }
    }
    fn snapshot(s: &State, c: &Transition) -> Value {
        json!({"current":s.current,"previous":s.previous,"stack":s.stack,"phase":c.phase})
    }
    #[test]
    fn transitions_match_original_state_callback_order_and_failure_prefixes() -> Result<()> {
        let reference: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/context_stack.json"
        ))?;
        for case in reference["cases"].as_array().unwrap() {
            let i = &case["initial"];
            let mut s = State {
                current: i["current"].as_i64().unwrap() as i32,
                previous: i["previous"].as_i64().unwrap() as i32,
                stack: serde_json::from_value(i["stack"].clone())?,
            };
            let mut c = Transition {
                phase: i["phase"].as_i64().unwrap() as i32,
            };
            let mut h = Host {
                case,
                calls: vec![],
                counts: Default::default(),
            };
            let result = if case["mode"] == "update" {
                s.update(&mut c, &mut h)
            } else {
                s.switch(case["request"].as_i64().unwrap() as i32, &mut c, &mut h)
            };
            assert_eq!(result.is_err(), !case["stopped"].is_null(), "{case}");
            assert_eq!(Value::Array(h.calls), case["calls"], "{case}");
            assert_eq!(snapshot(&s, &c), case["output"], "{case}");
        }
        Ok(())
    }
    #[test]
    fn invalid_return_keeps_native_pop_prefix_and_does_not_invent_a_context() -> Result<()> {
        let case = json!({});
        let mut h = Host {
            case: &case,
            calls: vec![],
            counts: Default::default(),
        };
        let mut s = State {
            current: 2,
            previous: -1,
            stack: vec![2],
        };
        let mut c = Transition::default();
        assert!(s.switch(-2, &mut c, &mut h).is_err());
        assert!(s.stack.is_empty());
        assert_eq!(s.current, 2);
        assert_eq!(c.phase, 0);
        assert_eq!(h.calls, [json!({"op":"lookup","key":2})]);
        Ok(())
    }
}
