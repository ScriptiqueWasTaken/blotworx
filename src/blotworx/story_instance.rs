//! Apprentice instance2839d4/283a34/283d48/283ecc. Mandatory effects belong to
//! their real owners; this is not a substitute conversation/presentation host.
use anyhow::{ensure, Context, Result};
#[derive(Debug, Clone, serde::Serialize)]
pub struct Instance {
    pub stack: Vec<usize>, //0/4: root accessor uses FIRST entry, not last
    pub owner: Option<usize>,
    pub context: Option<usize>, //8/c
    pub sequence: Option<usize>,
    pub callback: Option<usize>, //10/14; host identities
    pub event: i32,
    pub jump: i32,
    pub active: bool,
    pub phase: i32,       //18/1c/20/24
    pub flags: [bool; 7], //28..2e; names need the concrete presentation owner
}
impl Default for Instance {
    fn default() -> Self {
        Self {
            stack: vec![],
            owner: None,
            context: None,
            sequence: None,
            callback: None,
            event: 0,
            jump: -1,
            active: false,
            phase: 0,
            flags: [false, false, true, false, false, false, false],
        }
    }
}
pub trait Backend {
    fn presentation18(&mut self) -> Result<bool>;
    fn presentation20(&mut self) -> Result<bool>;
    fn reset_events(&mut self, sequence: usize) -> Result<()>;
    fn started(&mut self, instance: &mut Instance, owner: Option<usize>) -> Result<()>;
    fn sequence_flags(&mut self, sequence: usize) -> Result<u16>;
    fn restrict(&mut self, instance: &mut Instance) -> Result<()>;
    fn presentation1c(&mut self, instance: &mut Instance, value: bool) -> Result<()>;
    fn presentation24(&mut self, instance: &mut Instance, value: bool) -> Result<()>;
    fn played(&mut self, instance: &mut Instance, sequence: usize) -> Result<()>;
    fn presentation_busy(&mut self) -> Result<bool>;
    fn event_count(&mut self, sequence: usize) -> Result<i32>;
    fn event_at(&mut self, sequence: usize, index: i32) -> Result<usize>;
    fn event_kind(&mut self, event: usize) -> Result<u16>;
    fn update_event(&mut self, instance: &mut Instance, event: usize, delta: f32) -> Result<()>;
    fn event_waits(&mut self, event: usize) -> Result<bool>;
    fn event_ready(&mut self, event: usize) -> Result<bool>;
    fn pop_parent(&mut self, instance: &mut Instance) -> Result<()>;
    fn begin_end(&mut self, instance: &mut Instance, immediate: bool) -> Result<()>;
    fn complete(&mut self, instance: &mut Instance, aborted: bool) -> Result<()>;
}
impl Instance {
    pub fn root(&self) -> Result<usize> {
        self.stack
            .first()
            .copied()
            .or(self.sequence)
            .context("Story instance has no root resource")
    }
    pub fn reset(&mut self, b: &mut impl Backend) -> Result<()> {
        self.phase = 0;
        self.event = 0;
        self.jump = -1;
        self.active = false;
        self.callback = None;
        self.sequence = None;
        self.owner = None;
        self.stack.clear();
        self.flags[0] = !b.presentation18()?;
        self.flags[1] = b.presentation20()?;
        self.flags[2] = true;
        self.flags[3] = false;
        self.context = None;
        self.flags[4..].fill(false);
        Ok(())
    }
    pub fn activate(
        &mut self,
        owner: Option<usize>,
        sequence: Option<usize>,
        callback: Option<usize>,
        b: &mut impl Backend,
    ) -> Result<()> {
        self.reset(b)?;
        let Some(sequence) = sequence else {
            return Ok(());
        };
        self.sequence = Some(sequence);
        b.reset_events(sequence)?;
        b.started(self, owner)?;
        if b.sequence_flags(self.root()?)? & 1 != 0 {
            b.restrict(self)?;
        }
        if b.sequence_flags(self.root()?)? & 0x20 != 0 {
            b.presentation1c(self, false)?;
        }
        b.presentation24(self, true)?;
        let root = self.root()?;
        b.played(self, root)?;
        self.callback = callback;
        self.owner = owner;
        self.context = owner;
        self.active = true;
        Ok(())
    }
    pub fn update(&mut self, delta: f32, b: &mut impl Backend) -> Result<()> {
        ensure!(delta.is_finite(), "Nonfinite story update delta");
        if self.sequence.is_none() {
            return Ok(());
        }
        match self.phase {
            0 => {
                if !b.presentation_busy()? {
                    self.phase = 1;
                }
            }
            1 => {
                loop {
                    let sequence = self
                        .sequence
                        .context("Active story resource vanished before event lookup")?;
                    if self.event >= b.event_count(sequence)? {
                        break;
                    }
                    let event = b.event_at(sequence, self.event)?;
                    if self.flags[5] && b.event_kind(event)? == 5 {
                        self.flags[5] = false;
                        self.event = self.event.wrapping_add(1);
                        continue;
                    }
                    b.update_event(self, event, delta)?;
                    if !self.active || self.phase != 1 || self.sequence != Some(sequence) {
                        break;
                    }
                    if b.event_waits(event)? && !b.event_ready(event)? {
                        break;
                    }
                    if self.jump >= 0 {
                        self.event = self.jump;
                        self.jump = -1;
                    } else {
                        self.event = self.event.wrapping_add(1);
                    }
                }
                if self.active {
                    let count = b.event_count(
                        self.sequence
                            .context("Active story resource vanished at completion")?,
                    )?;
                    if self.event == count {
                        if !self.stack.is_empty() {
                            b.pop_parent(self)?;
                        } else {
                            b.begin_end(self, false)?;
                        }
                    }
                }
            }
            2 => {
                if !b.presentation_busy()? {
                    b.complete(self, false)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct Pool {
    pub slots: Vec<Instance>,
}
impl Pool {
    ///281264 scans the whole pool and retains the LAST inactive instance.
    pub fn acquire(&mut self) -> Result<usize> {
        if let Some(i) = self.slots.iter().rposition(|s| !s.active) {
            return Ok(i);
        }
        ensure!(
            self.slots.len() < u16::MAX as usize,
            "Story pool count cannot fit the native u16 owner"
        );
        self.slots.try_reserve(1)?;
        self.slots.push(Instance::default());
        Ok(self.slots.len() - 1)
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
        fn record(&mut self, op: &str, args: Value) -> Result<()> {
            let mut args = args.as_object().unwrap().clone();
            args.insert("op".into(), json!(op));
            self.calls.push(Value::Object(args));
            let n = self.counts.entry(op.into()).or_default();
            *n += 1;
            ensure!(
                self.case["fail"].as_str() != Some(op)
                    && self.case["fail"].as_str() != Some(&format!("{op}:{n}")),
                "Supplied boundary failure"
            );
            Ok(())
        }
    }
    impl Backend for Host<'_> {
        fn presentation18(&mut self) -> Result<bool> {
            self.record("p18", json!({}))?;
            self.case["p18"].as_bool().context("Missing p18")
        }
        fn presentation20(&mut self) -> Result<bool> {
            self.record("p20", json!({}))?;
            self.case["p20"].as_bool().context("Missing p20")
        }
        fn reset_events(&mut self, sequence: usize) -> Result<()> {
            self.record("reset", json!({"sequence":sequence}))
        }
        fn started(&mut self, s: &mut Instance, owner: Option<usize>) -> Result<()> {
            self.record("start",json!({"owner":owner,"observed_owner":s.owner,"active":s.active,"sequence":s.sequence}))
        }
        fn sequence_flags(&mut self, sequence: usize) -> Result<u16> {
            self.record("flags", json!({"sequence":sequence}))?;
            Ok(self.case["flags"].as_u64().context("Missing flags")? as u16)
        }
        fn restrict(&mut self, _: &mut Instance) -> Result<()> {
            self.record("restrict", json!({}))
        }
        fn presentation1c(&mut self, _: &mut Instance, v: bool) -> Result<()> {
            self.record("p1c", json!({"value":v}))
        }
        fn presentation24(&mut self, _: &mut Instance, v: bool) -> Result<()> {
            self.record("p24", json!({"value":v}))
        }
        fn played(&mut self, _: &mut Instance, sequence: usize) -> Result<()> {
            self.record("played", json!({"sequence":sequence}))
        }
        fn presentation_busy(&mut self) -> Result<bool> {
            self.record("busy", json!({}))?;
            self.case["busy"].as_bool().context("Missing busy")
        }
        fn event_count(&mut self, sequence: usize) -> Result<i32> {
            self.record("count", json!({"sequence":sequence}))?;
            Ok(self.case["values"]
                .as_array()
                .context("Missing events")?
                .len()
                .try_into()?)
        }
        fn event_at(&mut self, sequence: usize, index: i32) -> Result<usize> {
            self.record("at", json!({"sequence":sequence,"index":index}))?;
            let i = usize::try_from(index)?;
            ensure!(
                i < self.case["values"].as_array().unwrap().len(),
                "Missing event"
            );
            Ok(i)
        }
        fn event_kind(&mut self, event: usize) -> Result<u16> {
            self.record("kind", json!({"index":event}))?;
            Ok(self.case["values"][event][0].as_u64().unwrap() as u16)
        }
        fn update_event(&mut self, s: &mut Instance, event: usize, delta: f32) -> Result<()> {
            assert_eq!(delta.to_bits(), (1f32 / 30.).to_bits());
            self.record("update", json!({"index":event}))?;
            if let Some(m) = self.case["mutation"].as_object() {
                if event == m.get("at").and_then(|v| v.as_u64()).unwrap_or(0) as usize {
                    if let Some(v) = m.get("phase") {
                        s.phase = v.as_i64().unwrap() as i32;
                    }
                    if let Some(v) = m.get("event") {
                        s.event = v.as_i64().unwrap() as i32;
                    }
                    if let Some(v) = m.get("jump") {
                        s.jump = v.as_i64().unwrap() as i32;
                    }
                    if let Some(v) = m.get("active") {
                        s.active = v.as_bool().unwrap();
                    }
                    if let Some(v) = m.get("sequence") {
                        s.sequence = Some(v.as_u64().unwrap() as usize);
                    }
                }
            }
            Ok(())
        }
        fn event_waits(&mut self, event: usize) -> Result<bool> {
            self.record("waits", json!({"index":event}))?;
            Ok(self.case["values"][event][1].as_bool().unwrap())
        }
        fn event_ready(&mut self, event: usize) -> Result<bool> {
            self.record("ready", json!({"index":event}))?;
            Ok(self.case["values"][event][2].as_bool().unwrap())
        }
        fn pop_parent(&mut self, s: &mut Instance) -> Result<()> {
            self.record("pop", json!({}))?;
            s.stack.pop().context("No parent")?;
            s.sequence = Some(2);
            s.event = 0;
            Ok(())
        }
        fn begin_end(&mut self, s: &mut Instance, immediate: bool) -> Result<()> {
            assert!(!immediate);
            self.record("end", json!({}))?;
            s.phase = 2;
            Ok(())
        }
        fn complete(&mut self, s: &mut Instance, aborted: bool) -> Result<()> {
            assert!(!aborted);
            self.record("complete", json!({}))?;
            s.sequence = None;
            s.active = false;
            Ok(())
        }
    }
    fn snapshot(s: &Instance) -> Value {
        json!({"sequence":s.sequence,"event":s.event,"jump":s.jump,"active":s.active,
        "phase":s.phase,"depth":s.stack.len(),"skip":s.flags[5]})
    }
    #[test]
    fn instance_update_matches_original_calls_mutations_and_failure_prefixes() -> Result<()> {
        let data: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/story_instance.json"
        ))?;
        for case in data["cases"].as_array().unwrap() {
            let i = &case["initial"];
            let mut s = Instance {
                sequence: i["sequence"].as_u64().map(|v| v as usize),
                event: i["event"].as_i64().unwrap() as i32,
                jump: i["jump"].as_i64().unwrap() as i32,
                active: i["active"].as_bool().unwrap(),
                phase: i["phase"].as_i64().unwrap() as i32,
                stack: vec![0; i["depth"].as_u64().unwrap() as usize],
                ..Default::default()
            };
            s.flags[5] = i["skip"].as_bool().unwrap();
            let mut h = Host {
                case,
                calls: vec![],
                counts: Default::default(),
            };
            let result = s.update(1. / 30., &mut h);
            assert_eq!(result.is_err(), !case["stopped"].is_null(), "{case}");
            assert_eq!(Value::Array(h.calls), case["calls"], "{case}");
            assert_eq!(snapshot(&s), case["output"], "{case}");
        }
        Ok(())
    }
    #[test]
    fn activation_preserves_effect_order_and_tail_publication() -> Result<()> {
        let case = json!({"p18":false,"p20":true,"flags":33});
        let mut h = Host {
            case: &case,
            calls: vec![],
            counts: Default::default(),
        };
        let mut s = Instance::default();
        s.context = Some(88);
        s.stack = vec![6];
        s.event = 99;
        s.active = true;
        s.activate(Some(7), Some(3), Some(123), &mut h)?;
        assert_eq!(
            h.calls
                .iter()
                .map(|v| v["op"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "p18", "p20", "reset", "start", "flags", "restrict", "flags", "p1c", "p24",
                "played"
            ]
        );
        assert_eq!(
            h.calls[3],
            json!({"op":"start","owner":7,"observed_owner":null,"active":false,"sequence":3})
        );
        assert_eq!(s.flags, [true, true, true, false, false, false, false]);
        assert_eq!(s.owner, Some(7));
        assert_eq!(s.context, Some(7));
        assert_eq!(s.sequence, Some(3));
        assert_eq!(s.callback, Some(123));
        assert!(s.active);
        assert_eq!(s.phase, 0);
        assert_eq!(s.jump, -1);
        assert!(s.stack.is_empty());
        let case = json!({"p18":true,"p20":false,"flags":0,"fail":"played"});
        let mut h = Host {
            case: &case,
            calls: vec![],
            counts: Default::default(),
        };
        assert!(s.activate(Some(9), Some(4), Some(123), &mut h).is_err());
        assert_eq!(s.sequence, Some(4));
        assert_eq!(s.owner, None);
        assert_eq!(s.callback, None);
        assert!(!s.active);
        Ok(())
    }
    #[test]
    fn pool_reuses_last_inactive_and_constructor_flags_match_native() -> Result<()> {
        let mut p = Pool::default();
        assert_eq!(p.acquire()?, 0);
        assert_eq!(
            p.slots[0].flags,
            [false, false, true, false, false, false, false]
        );
        p.slots[0].active = true;
        assert_eq!(p.acquire()?, 1);
        p.slots[1].active = true;
        assert_eq!(p.acquire()?, 2);
        p.slots[0].active = false;
        assert_eq!(p.acquire()?, 2);
        p.slots[2].active = true;
        assert_eq!(p.acquire()?, 0);
        p.slots[0].active = true;
        assert_eq!(p.acquire()?, 3);
        assert_eq!(p.slots.len(), 4);
        Ok(())
    }
}
