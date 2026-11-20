//! Patrol's concrete phase loop, REL1abf8c..1ac4b4. Native node/controller
//! services are mandatory; this does not fabricate navigation or task results.
use anyhow::Result;

#[derive(Default, Debug)]
pub struct State {
    pub target: Option<usize>,
    pub phase: u32,
    pub reverse: u8,
    pub wait: f32,
    pub resume_phase: u32,
    pub index: i32,
    /// Native +44, retained exactly across a getter that declines to write.
    pub saved_motion_scalar: u32,
}
#[derive(Debug, serde::Serialize)]
pub enum Task {
    Move {
        position: [u32; 4],
        radius: u32,
        speed: u32,
        extra: u32,
    },
    Turn {
        heading: u32,
    },
    Animation {
        start: u32,
        done: u32,
        timeout: u32,
        heading: u32,
        apply_heading: bool,
        agent_40: bool,
    },
}
/// Getter addresses remain explicit where the node property's meaning has not
/// been established. All values must come from actual authored/runtime state.
pub trait Host {
    fn resolve_target(&mut self, state: &mut State) -> Result<Option<usize>>;
    fn patrol_component(&mut self, target: Option<usize>) -> Result<Option<usize>>;
    fn boolean(&mut self, function: u32, object: usize) -> Result<bool>;
    fn scalar(&mut self, function: u32, object: usize, index: i32) -> Result<f32>;
    fn integer(&mut self, function: u32, object: usize) -> Result<i32>;
    fn turning(&mut self, state: &mut State, owner: usize, restore: bool) -> Result<()>;
    fn position(&mut self, target: Option<usize>) -> Result<[u32; 4]>;
    fn install(&mut self, state: &mut State, owner: usize, task: Task, wait: bool) -> Result<()>;
    fn notify(&mut self, state: &mut State, owner: usize, kind: u16) -> Result<()>;
    fn choose_index(&mut self, component: usize, previous: i32, index: &mut i32) -> Result<bool>;
    fn reset_clock(&mut self) -> Result<()>;
    fn clock(&mut self) -> Result<f32>;
    fn animation_controller(&mut self, owner: usize) -> Result<Option<usize>>;
    fn variable(&mut self, controller: usize, id: u32, value: i32) -> Result<()>;
    fn animation_event(&mut self, controller: Option<usize>, event: u32) -> Result<()>;
    fn advance_target(&mut self, state: &mut State) -> Result<()>;
}
pub trait LifecycleHost {
    fn motion_controller(&mut self, owner: usize) -> Result<Option<usize>>;
    fn read_motion_scalar(&mut self, controller: usize, destination: &mut u32) -> Result<()>;
    fn moving_turn_ratio_enabled(&mut self, owner: usize) -> Result<bool>;
    fn movement_speed(&mut self, owner: usize) -> Result<f32>;
    fn moving_turn_ratio(&mut self) -> Result<f32>;
    fn cache_moving_turn_rate(&mut self, controller: usize, value: f32) -> Result<()>;
    ///18c908 queries the actual navigation agent and AIBrain Tether Radius.
    ///1b468 converts the owning interface; it is not a physics lookup.
    fn prepare_agent(&mut self, owner: usize) -> Result<()>;
    fn animation_controller(&mut self, owner: usize) -> Result<Option<usize>>;
    fn animation_event(&mut self, controller: usize, event: u32) -> Result<()>;
    fn ai_controller(&mut self, owner: usize) -> Result<Option<usize>>;
    fn animation_variant(&mut self, controller: usize) -> Result<i32>;
    fn animation_variable(&mut self, controller: usize, id: u32, value: i32) -> Result<()>;
    fn reset_motion_turn_rates(&mut self, controller: usize) -> Result<()>;
    fn write_motion_scalar(&mut self, controller: usize, value: u32) -> Result<()>;
}
impl State {
    /// Concrete slot4c,1abd5c. Admission and common entry-state reset belong
    /// to the selector, not this receiver. Never call from ExitCutscene alone.
    pub fn enter(&mut self, owner: usize, host: &mut impl LifecycleHost) -> Result<()> {
        if let Some(motion) = host.motion_controller(owner)? {
            host.read_motion_scalar(motion, &mut self.saved_motion_scalar)?;
            if host.moving_turn_ratio_enabled(owner)? {
                let speed = host.movement_speed(owner)?;
                let ratio = host.moving_turn_ratio()?;
                let rate = f32::from_bits(0x40c90fdb) * (speed * ratio);
                host.cache_moving_turn_rate(motion, rate)?;
            }
        }
        host.prepare_agent(owner)?;
        if let Some(controller) = host.animation_controller(owner)? {
            host.animation_event(controller, 0xc2)?;
        }
        let ai = host.ai_controller(owner)?;
        let animation = host.animation_controller(owner)?;
        if let (Some(ai), Some(animation)) = (ai, animation) {
            let variant = host.animation_variant(ai)?;
            host.animation_variable(animation, 0x2b, variant)?;
        }
        self.phase = 1;
        Ok(())
    }
    /// Concrete slot50,1abee4, including the fresh motion lookup performed
    /// by1ac904 after30c868. A changed component is not a cached old receiver.
    pub fn exit(&mut self, owner: usize, host: &mut impl LifecycleHost) -> Result<()> {
        if let Some(motion) = host.motion_controller(owner)? {
            host.reset_motion_turn_rates(motion)?;
            if let Some(current) = host.motion_controller(owner)? {
                host.write_motion_scalar(current, self.saved_motion_scalar)?;
            }
        }
        if let Some(animation) = host.animation_controller(owner)? {
            host.animation_variable(animation, 0x2b, 0)?;
        }
        self.phase = 0;
        Ok(())
    }
    ///1ac7bc's unsigned range predicate; phases0..2 are not active sequences.
    pub fn active_sequence(&self) -> bool {
        (3..=8).contains(&self.phase)
    }
    /// Supplemental slot60,1abc88. Source/subscription admission is external.
    /// Queue departure before replacing the retained destination.
    pub fn receive_action(
        &mut self,
        kind: u16,
        action: u32,
        reference: Option<usize>,
        owner: usize,
        notify: &mut dyn FnMut(Option<usize>, usize, u16) -> Result<()>,
    ) -> Result<bool> {
        if kind != 0x0c {
            return Ok(false);
        }
        match action {
            25 => {
                if self.active_sequence() {
                    notify(self.target, owner, 0x8e)?;
                }
                let old = self.phase;
                self.target = reference;
                self.phase = 1;
                Ok(old == 0)
            }
            26 => {
                if self.active_sequence() {
                    self.wait = 0.;
                    self.phase = 8;
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
    /// Root slot5c,1ac580, separate from supplemental ordinary actions.
    pub fn receive_root(&mut self, kind: u16) -> bool {
        if kind == 0x6f {
            self.target = None;
            true
        } else {
            false
        }
    }
    /// One native1abffc iteration. Zero requests another iteration immediately;
    /// it is not the common behavior's finished result.
    pub fn step(&mut self, owner: usize, host: &mut impl Host) -> Result<u32> {
        let target = host.resolve_target(self)?;
        let Some(component) = host.patrol_component(target)? else {
            self.target = None;
            return Ok(2);
        };
        match self.phase {
            1 => {
                if !host.boolean(0x2e0240, owner)? {
                    if host.boolean(0x1ac99c, owner)? {
                        let restore = host.boolean(0x1ac9d0, component)?;
                        host.turning(self, owner, restore)?;
                    }
                    let position = host.position(target)?;
                    let radius = host.scalar(0x36d188, component, 0)?;
                    let speed = host.scalar(0x1ac964, owner, 0)?;
                    host.install(
                        self,
                        owner,
                        Task::Move {
                            position,
                            radius: radius.to_bits(),
                            speed: speed.to_bits(),
                            extra: 0,
                        },
                        true,
                    )?;
                    self.phase = 2;
                }
                Ok(1)
            }
            2 => {
                host.notify(self, owner, 0x8d)?;
                if host.boolean(0x36cefc, component)? {
                    self.reverse = 0;
                } else if host.boolean(0x36cf58, component)? {
                    self.reverse = 1;
                }
                if host.boolean(0x36cffc, component)? {
                    self.phase = if host.choose_index(component, -1, &mut self.index)? {
                        3
                    } else {
                        8
                    };
                } else {
                    self.wait = host.scalar(0x36cfb4, component, 0)?;
                    self.resume_phase = 8;
                    self.phase = 6;
                }
                Ok(0)
            }
            3 => {
                if !host.boolean(0x2e0240, owner)? {
                    let heading = host.scalar(0x36d000, component, self.index)?;
                    host.install(
                        self,
                        owner,
                        Task::Turn {
                            heading: heading.to_bits(),
                        },
                        true,
                    )?;
                    self.phase = 4;
                }
                Ok(1)
            }
            4 => {
                self.wait = host.scalar(0x36d078, component, self.index)?;
                self.resume_phase = 5;
                self.phase = 6;
                Ok(0)
            }
            5 => {
                self.phase = if host.choose_index(component, self.index, &mut self.index)? {
                    3
                } else {
                    8
                };
                Ok(0)
            }
            6 => {
                host.reset_clock()?;
                let animation = host.integer(0x36d12c, component)?;
                let mut immediate = true;
                if animation >= 0 {
                    if let Some(controller) = host.animation_controller(owner)? {
                        host.variable(controller, 0x3d, animation)?;
                        // Native ble tests !GT, including unordered.
                        immediate = self.wait > 0.;
                        host.install(
                            self,
                            owner,
                            Task::Animation {
                                start: 0xd3,
                                done: 0xc1,
                                timeout: 5f32.to_bits(),
                                heading: (-1f32).to_bits(),
                                apply_heading: false,
                                agent_40: true,
                            },
                            !immediate,
                        )?;
                    }
                }
                self.phase = 7;
                Ok(if immediate { 0 } else { 1 })
            }
            7 => {
                // Preserve strict GT: unordered does not expire the wait.
                if host.clock()? > self.wait {
                    let controller = host.animation_controller(owner)?;
                    // Native path does not suppress the send on null lookup.
                    host.animation_event(controller, 0xc2)?;
                    self.phase = self.resume_phase;
                    Ok(0)
                } else {
                    Ok(1)
                }
            }
            8 => {
                host.notify(self, owner, 0x8e)?;
                host.advance_target(self)?;
                self.phase = 1;
                Ok(0)
            }
            _ => Ok(2),
        }
    }
    pub fn update(&mut self, owner: usize, host: &mut impl Host) -> Result<u32> {
        loop {
            match self.step(owner, host)? {
                0 => {}
                1 => return Ok(1),
                _ => return Ok(2),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patrol_lifecycle_matches_original_controller_changes_and_saved_property() -> Result<()> {
        use serde_json::{json, Value};
        struct Binding<'a> {
            c: &'a Value,
            motion_at: usize,
            animation_at: usize,
            trace: Vec<Value>,
            cache: Option<[u32; 2]>,
        }
        impl LifecycleHost for Binding<'_> {
            fn motion_controller(&mut self, o: usize) -> Result<Option<usize>> {
                self.trace.push(json!(["motion", o]));
                let v = self.c["motion"][self.motion_at]
                    .as_u64()
                    .map(|v| v as usize);
                self.motion_at += 1;
                Ok(v)
            }
            fn read_motion_scalar(&mut self, c: usize, out: &mut u32) -> Result<()> {
                self.trace.push(json!(["read", c]));
                if let Some(v) = self.c["read"].as_u64() {
                    *out = v as u32;
                }
                Ok(())
            }
            fn moving_turn_ratio_enabled(&mut self, o: usize) -> Result<bool> {
                self.trace.push(json!(["enabled", o]));
                Ok(self.c["enabled"].as_bool().unwrap())
            }
            fn movement_speed(&mut self, o: usize) -> Result<f32> {
                self.trace.push(json!(["speed", o]));
                Ok(f32::from_bits(self.c["speed"].as_u64().unwrap() as u32))
            }
            fn moving_turn_ratio(&mut self) -> Result<f32> {
                self.trace.push(json!(["ratio"]));
                Ok(f32::from_bits(self.c["ratio"].as_u64().unwrap() as u32))
            }
            fn cache_moving_turn_rate(&mut self, c: usize, v: f32) -> Result<()> {
                self.trace.push(json!(["cache", c, v.to_bits()]));
                self.cache = Some([c as u32, v.to_bits()]);
                Ok(())
            }
            fn prepare_agent(&mut self, o: usize) -> Result<()> {
                self.trace.push(json!(["prepare", o]));
                if self.c["unbound"].as_bool() == Some(true) {
                    anyhow::bail!("Actual navigation-agent preparation is unbound");
                }
                Ok(())
            }
            fn animation_controller(&mut self, o: usize) -> Result<Option<usize>> {
                self.trace.push(json!(["animation_controller", o]));
                let v = self.c["animation"][self.animation_at]
                    .as_u64()
                    .map(|v| v as usize);
                self.animation_at += 1;
                Ok(v)
            }
            fn animation_event(&mut self, c: usize, e: u32) -> Result<()> {
                self.trace.push(json!(["event", c, e]));
                Ok(())
            }
            fn ai_controller(&mut self, o: usize) -> Result<Option<usize>> {
                self.trace.push(json!(["ai", o]));
                Ok(self.c["ai"].as_u64().map(|v| v as usize))
            }
            fn animation_variant(&mut self, c: usize) -> Result<i32> {
                self.trace.push(json!(["variant", c]));
                Ok(self.c["variant"].as_i64().unwrap() as i32)
            }
            fn animation_variable(&mut self, c: usize, id: u32, v: i32) -> Result<()> {
                self.trace.push(json!(["variable", c, id, v]));
                Ok(())
            }
            fn reset_motion_turn_rates(&mut self, c: usize) -> Result<()> {
                self.trace.push(json!(["reset", c]));
                Ok(())
            }
            fn write_motion_scalar(&mut self, c: usize, v: u32) -> Result<()> {
                self.trace.push(json!(["write", c, v]));
                Ok(())
            }
        }
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_patrol_lifecycle.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c = &case["input"];
            let mut state = State {
                phase: 7,
                saved_motion_scalar: c["saved"].as_u64().unwrap() as u32,
                ..Default::default()
            };
            let mut host = Binding {
                c,
                motion_at: 0,
                animation_at: 0,
                trace: Vec::new(),
                cache: None,
            };
            if c["exit"].as_bool() == Some(true) {
                state.exit(11, &mut host)?;
            } else {
                state.enter(11, &mut host)?;
            }
            assert_eq!(
                json!({"phase":state.phase,"saved":state.saved_motion_scalar,
                "cache":host.cache,"trace":host.trace}),
                case["output"],
                "{c}"
            );
        }
        let c = json!({"motion":[null,null],"unbound":true});
        let mut state = State {
            phase: 7,
            ..Default::default()
        };
        let mut host = Binding {
            c: &c,
            motion_at: 0,
            animation_at: 0,
            trace: Vec::new(),
            cache: None,
        };
        assert!(state.enter(11, &mut host).is_err());
        assert_eq!(
            state.phase, 7,
            "Missing agent preparation must not publish successful entry"
        );
        assert_eq!(
            host.trace,
            vec![json!(["motion", 11]), json!(["prepare", 11])]
        );
        Ok(())
    }
    #[test]
    fn patrol_receivers_match_original_destination_departure_and_resume_order() -> Result<()> {
        use serde_json::{json, Value};
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_patrol_events.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c = &case["input"];
            let phase = c["phase"].as_u64().unwrap() as u32;
            let mut state = State {
                target: c["target"].as_u64().map(|v| v as usize),
                phase,
                wait: f32::from_bits(c["wait"].as_u64().unwrap() as u32),
                ..Default::default()
            };
            let mut trace = Vec::<Value>::new();
            let result = if c["root"].as_bool() == Some(true) {
                state.receive_root(c["kind"].as_u64().unwrap() as u16)
            } else {
                state.receive_action(
                    c["kind"].as_u64().unwrap() as u16,
                    c["action"].as_u64().unwrap() as u32,
                    c["reference"].as_u64().map(|v| v as usize),
                    11,
                    &mut |target, owner, kind| {
                        trace.push(json!([kind, target, owner, phase]));
                        Ok(())
                    },
                )?
            };
            assert_eq!(
                json!({"target":state.target,"phase":state.phase,"wait":state.wait.to_bits(),
                "result":result,"trace":trace}),
                case["output"],
                "{c}"
            );
        }
        let mut state = State {
            target: Some(0),
            phase: 3,
            wait: 2.,
            ..Default::default()
        };
        assert!(state
            .receive_action(0x0c, 25, Some(42), 11, &mut |_, _, _| anyhow::bail!(
                "queue unavailable"
            ))
            .is_err());
        assert_eq!(state.target, Some(0));
        assert_eq!(state.phase, 3);
        Ok(())
    }
    #[test]
    fn patrol_phase_loop_matches_original_state_tasks_and_callback_order() -> Result<()> {
        use serde_json::{json, Value};
        struct Binding<'a> {
            input: &'a Value,
            trace: Vec<Value>,
        }
        impl Host for Binding<'_> {
            fn resolve_target(&mut self, s: &mut State) -> Result<Option<usize>> {
                self.trace.push(json!(["target", s.target]));
                Ok(self.input["target"].as_u64().map(|v| v as usize))
            }
            fn patrol_component(&mut self, t: Option<usize>) -> Result<Option<usize>> {
                self.trace.push(json!(["component", t]));
                Ok(self.input["component"].as_bool().unwrap().then_some(9))
            }
            fn boolean(&mut self, f: u32, o: usize) -> Result<bool> {
                self.trace.push(json!(["boolean", f, o]));
                Ok(self.input["booleans"][f.to_string()]
                    .as_bool()
                    .unwrap_or(false))
            }
            fn scalar(&mut self, f: u32, o: usize, i: i32) -> Result<f32> {
                self.trace.push(json!(["scalar", f, o, i]));
                Ok(f32::from_bits(
                    self.input["scalars"][f.to_string()].as_u64().unwrap() as u32,
                ))
            }
            fn integer(&mut self, f: u32, o: usize) -> Result<i32> {
                self.trace.push(json!(["integer", f, o]));
                Ok(self.input["animation"].as_i64().unwrap() as i32)
            }
            fn turning(&mut self, _: &mut State, o: usize, r: bool) -> Result<()> {
                self.trace.push(json!(["turning", o, r]));
                Ok(())
            }
            fn position(&mut self, t: Option<usize>) -> Result<[u32; 4]> {
                self.trace.push(json!(["position", t]));
                Ok([0x3f800000, 0x40000000, 0x40400000, 0])
            }
            fn install(&mut self, _: &mut State, o: usize, t: Task, w: bool) -> Result<()> {
                self.trace.push(json!(["install", o, t, w]));
                Ok(())
            }
            fn notify(&mut self, s: &mut State, o: usize, k: u16) -> Result<()> {
                self.trace.push(json!(["notify", s.target, o, k]));
                Ok(())
            }
            fn choose_index(&mut self, c: usize, p: i32, i: &mut i32) -> Result<bool> {
                self.trace.push(json!(["choose", c, p]));
                *i = self.input["next_index"].as_i64().unwrap() as i32;
                Ok(self.input["choose"].as_bool().unwrap())
            }
            fn reset_clock(&mut self) -> Result<()> {
                self.trace.push(json!(["reset_clock"]));
                Ok(())
            }
            fn clock(&mut self) -> Result<f32> {
                self.trace.push(json!(["clock"]));
                Ok(f32::from_bits(
                    self.input["elapsed"].as_u64().unwrap() as u32
                ))
            }
            fn animation_controller(&mut self, o: usize) -> Result<Option<usize>> {
                self.trace.push(json!(["controller", o]));
                Ok(self.input["controller"].as_u64().map(|v| v as usize))
            }
            fn variable(&mut self, c: usize, i: u32, v: i32) -> Result<()> {
                self.trace.push(json!(["variable", c, i, v]));
                Ok(())
            }
            fn animation_event(&mut self, c: Option<usize>, e: u32) -> Result<()> {
                self.trace.push(json!(["animation", c, e]));
                Ok(())
            }
            fn advance_target(&mut self, s: &mut State) -> Result<()> {
                self.trace.push(json!(["advance"]));
                s.target = Some(13);
                Ok(())
            }
        }
        let fixture: Value =
            serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_patrol.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c = &case["input"];
            let mut state = State {
                target: Some(7),
                phase: c["phase"].as_u64().unwrap() as u32,
                reverse: 2,
                wait: f32::from_bits(c["wait"].as_u64().unwrap() as u32),
                resume_phase: 5,
                index: 2,
                saved_motion_scalar: 0,
            };
            let mut host = Binding {
                input: c,
                trace: Vec::new(),
            };
            let result = if c["wrapper"].as_bool() == Some(true) {
                state.update(11, &mut host)?
            } else {
                state.step(11, &mut host)?
            };
            assert_eq!(
                json!({"target":state.target,"phase":state.phase,"reverse":state.reverse,
                "wait":state.wait.to_bits(),"resume":state.resume_phase,"index":state.index,
                "result":result,"trace":host.trace}),
                case["output"],
                "input {c}"
            );
        }
        Ok(())
    }
}
