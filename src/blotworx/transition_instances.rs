//! Character-owned transition clones. Definition identity is distinct from
//! instance identity: interrupted transitions may share the same definition.
use super::{
    behavior_state::State,
    machine_runtime::{Notice, Runtime},
    machine_transitions::{Executor, Source},
    state_machine::Reference,
    transition_effect as effect,
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Handle(u64);
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Binding {
    Generator(String),
    Effect(Handle),
}
pub enum Playback {
    Blend(effect::Blending),
    Generated(effect::Generated),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Flags {
    pub discard_events: bool,
    pub reactivate: bool,
    pub synchronize: bool,
    pub prewarm: bool,
}
impl Flags {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}
pub struct Child {
    pub binding: Binding,
    pub flags: Flags,
}
pub struct Children {
    pub nodes: Vec<Child>,
    pub flags: Flags,
}
pub struct Instance {
    pub definition: String,
    pub source: Option<Binding>,
    pub target: Option<String>,
    pub playback: Option<Playback>,
}
#[derive(Default)]
pub struct Instances {
    active: BTreeMap<Handle, Instance>,
    next: u64,
    pub topology_changed: bool,
}
impl Instances {
    /// Duration-ended scalar timing. Distinct unsynchronized generators have
    /// empty output sync; a self-transition copies child sync, but its elapsed
    /// clock and one-update completion do not depend on that copied clock.
    /// Output synchronization/markers remain a separate required graph pass.
    pub fn advance_duration_blend(
        &mut self,
        h: Handle,
        definitions: &BTreeMap<String, effect::Definition>,
        dt: f32,
    ) -> Result<()> {
        let instance = self.get_mut(h)?;
        let d = match definitions
            .get(&instance.definition)
            .context("Missing transition definition")?
        {
            effect::Definition::Blend(d) => d,
            _ => anyhow::bail!("Generator transition requires synchronized update"),
        };
        let target = instance
            .target
            .as_ref()
            .context("Missing transition target")?;
        let same = instance.source.as_ref() == Some(&Binding::Generator(target.clone()));
        ensure!(
            d.end_mode == 0
                && (same || (d.flags & 2 == 0 && d.to_generator_start_time_fraction == 0.)),
            "Transition {} requires child synchronization",
            instance.definition
        );
        let p = match instance.playback.as_mut().context("Inactive transition")? {
            Playback::Blend(p) => p,
            _ => anyhow::bail!("Transition playback type mismatch"),
        };
        ensure!(
            !p.target_was_active,
            "Transition {} ({}) to {} requires self-transition mode {} handling",
            instance.definition,
            d.name,
            target,
            d.self_transition_mode
        );
        if same {
            //8021d570 selects duration-time for end mode0.8021dd28 advances
            //these two scalar fields independently of copied output sync, and
            //8021e084 finishes a same-generator effect after time becomes>0.
            p.clock.remaining = d.duration - p.clock.time;
            return p.clock.advance(dt);
        }
        // DOL 0x8021d570 distinct unsynchronized output has no sync points,
        // zero frequency/time and field_48=1. End mode zero uses duration.
        let mut sync = super::animation_sync::Sync {
            points: vec![],
            frequency: 0.,
            time: 0.,
            field_48: 1.,
            mapped_events: false,
        };
        p.clock.remaining = d.duration - p.clock.time;
        p.update(&mut sync, dt)
    }
    pub fn finished(&self, h: Handle) -> Result<bool> {
        let instance = self.get(h)?;
        let same = instance
            .target
            .as_ref()
            .map(|t| instance.source.as_ref() == Some(&Binding::Generator(t.clone())))
            .context("Missing transition target")?;
        Ok(
            match instance.playback.as_ref().context("Inactive transition")? {
                Playback::Blend(p) => p.clock.finished(same),
                Playback::Generated(p) => p.clock.finished(),
            },
        )
    }
    pub fn get(&self, h: Handle) -> Result<&Instance> {
        self.active.get(&h).context("Missing transition instance")
    }
    pub fn get_mut(&mut self, h: Handle) -> Result<&mut Instance> {
        self.active
            .get_mut(&h)
            .context("Missing transition instance")
    }
    /// Enumerate retained bindings using the original effect's child-order and
    /// flag rules. An interrupted source remains an instance, not its template.
    pub fn children(
        &mut self,
        h: Handle,
        definitions: &BTreeMap<String, effect::Definition>,
        global_event_mode: u8,
    ) -> Result<Children> {
        let instance = self.get_mut(h)?;
        let source = instance
            .source
            .as_ref()
            .context("Unbound transition source")?;
        let target = instance
            .target
            .as_deref()
            .context("Unbound transition target")?;
        // The token is local to enumeration, never a serialized asset path.
        let token = match source {
            Binding::Generator(g) => g.clone(),
            Binding::Effect(h) => format!("@transition-instance:{}", h.0),
        };
        let definition = definitions
            .get(&instance.definition)
            .context("Missing transition definition")?;
        let children = match (
            instance
                .playback
                .as_mut()
                .context("Inactive transition instance")?,
            definition,
        ) {
            (Playback::Blend(p), effect::Definition::Blend(d)) => {
                p.children(d, &token, target, true, global_event_mode)?
            }
            (Playback::Generated(p), effect::Definition::Generator(d)) => {
                p.children(d, &token, target, true, global_event_mode)?
            }
            _ => anyhow::bail!("Transition playback/definition mismatch"),
        };
        let flags = Flags {
            discard_events: children.discard_events,
            reactivate: children.reactivate,
            synchronize: children.synchronize,
            prewarm: children.flag_7,
        };
        let nodes = children
            .nodes
            .into_iter()
            .map(|c| Child {
                binding: if c.generator == token {
                    source.clone()
                } else {
                    Binding::Generator(c.generator.into())
                },
                flags: Flags {
                    discard_events: c.discard_events,
                    reactivate: c.reactivate,
                    synchronize: c.synchronize,
                    prewarm: c.flag_7,
                },
            })
            .collect();
        Ok(Children { nodes, flags })
    }
    fn clone_effect(&mut self, r: &Reference) -> Result<Handle> {
        let next = self
            .next
            .checked_add(1)
            .context("Transition instance identity overflow")?;
        let handle = Handle(next);
        self.active.insert(
            handle,
            Instance {
                definition: r.id.clone(),
                source: None,
                target: None,
                playback: None,
            },
        );
        self.next = next;
        Ok(handle)
    }
    /// Called during active-tree activation, after source and target binding.
    /// The caller supplies activity from the tree BEFORE rebuilding it.
    pub fn activate(
        &mut self,
        h: Handle,
        definitions: &BTreeMap<String, effect::Definition>,
        was_active: impl Fn(&str) -> bool,
    ) -> Result<()> {
        let instance = self
            .active
            .get_mut(&h)
            .context("Missing transition instance")?;
        ensure!(instance.source.is_some(), "Transition source is not bound");
        let target = instance
            .target
            .as_deref()
            .context("Transition target is not bound")?;
        if instance.playback.is_some() {
            return Ok(());
        }
        instance.playback = Some(
            match definitions
                .get(&instance.definition)
                .context("Missing transition definition")?
            {
                effect::Definition::Blend(d) => {
                    Playback::Blend(effect::Blending::activate(d, was_active(target))?)
                }
                effect::Definition::Generator(d) => Playback::Generated(
                    effect::Generated::activate(d, was_active(&d.transition_generator.id))?,
                ),
            },
        );
        Ok(())
    }
    fn bind_source(&mut self, h: Handle, binding: Binding) -> Result<()> {
        if let Binding::Effect(mut parent) = binding {
            loop {
                ensure!(parent != h, "Cyclic transition source binding");
                match self.get(parent)?.source {
                    Some(Binding::Effect(next)) => parent = next,
                    _ => break,
                }
            }
        }
        self.get_mut(h)?.source = Some(binding);
        Ok(())
    }
    fn release(&mut self, h: Handle) -> Result<()> {
        ensure!(
            !self
                .active
                .values()
                .any(|i| i.source == Some(Binding::Effect(h))),
            "Cannot release a transition still referenced by another effect"
        );
        self.active
            .remove(&h)
            .context("Releasing missing transition instance")?;
        Ok(())
    }
}

/// State writes and lifecycle notifications use the character's real state.
/// Events/listeners must be delivered by the graph owner, never silently lost.
pub struct Owner<'a, F> {
    pub instances: &'a mut Instances,
    pub state: &'a mut State,
    pub notify: F,
}
impl<F> Executor<Handle> for Owner<'_, F>
where
    F: FnMut(&Runtime, Notice<'_>) -> Result<()>,
{
    fn sync_variable(&mut self, i: usize, value: i32) -> Result<()> {
        self.state.set_integer(i, value)
    }
    fn clone_effect(&mut self, r: &Reference) -> Result<Handle> {
        ensure!(
            self.state.transition_effects.contains_key(&r.id),
            "Missing transition definition {}",
            r.id
        );
        self.instances.clone_effect(r)
    }
    fn set_target(&mut self, h: &Handle, g: &str) -> Result<()> {
        self.instances.get_mut(*h)?.target = Some(g.into());
        Ok(())
    }
    fn set_source(&mut self, h: &Handle, s: Source<'_, Handle>) -> Result<()> {
        self.instances.bind_source(
            *h,
            match s {
                Source::Generator(g) => Binding::Generator(g.into()),
                Source::Effect(h) => Binding::Effect(*h),
            },
        )
    }
    fn release_effect(&mut self, h: Handle) -> Result<()> {
        // Retirement drains oldest-first. Detach retiring children before
        // release; the surviving effect has already been rebound by Stack.
        for i in self.instances.active.values_mut() {
            if i.source == Some(Binding::Effect(h)) {
                i.source = None;
            }
        }
        self.instances.release(h)
    }
    fn notice(&mut self, r: &Runtime, n: Notice<'_>) -> Result<()> {
        match n {
            Notice::SyncVariable { index, value } => self.state.set_integer(index, value),
            n => (self.notify)(r, n),
        }
    }
    fn changed(&mut self) -> Result<()> {
        self.instances.topology_changed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn character_owner_starts_and_retires_interrupted_effects() -> Result<()> {
        let graph = super::super::behavior::Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut prepared = super::super::generator_plan::prepare(&graph, State::new(&graph)?, 0)?;
        let machine = prepared.state.machines.remove("2:240").unwrap();
        let runtime = prepared.machines.get_mut("2:240").unwrap();
        let walk = machine.states[machine.state_indices[&0]]
            .transitions
            .iter()
            .find(|t| t.event_id == 0 && t.to_state_id == 5)
            .unwrap();
        let mut stack = super::super::machine_transitions::Stack::default();
        let mut owner = Owner {
            instances: &mut prepared.transitions,
            state: &mut prepared.state,
            notify: |_: &Runtime, _: Notice<'_>| Ok(()),
        };
        stack.start(&machine, runtime, walk, false, &mut owner)?;
        let first = stack.records[0].effect;
        let mut delayed = walk.clone();
        delayed.to_state_id = 0;
        delayed.flags |= 16;
        stack.start(&machine, runtime, &delayed, false, &mut owner)?;
        let second = stack.records[1].effect;
        assert_eq!(
            owner.instances.get(second)?.source,
            Some(Binding::Effect(first))
        );
        stack.retire_through(0, &machine, runtime, &mut owner)?;
        assert!(owner.instances.get(first).is_err());
        assert!(matches!(
            owner.instances.get(second)?.source,
            Some(Binding::Generator(_))
        ));
        assert_eq!(runtime.current, 5);
        stack.retire_through(0, &machine, runtime, &mut owner)?;
        assert_eq!(runtime.current, 0);
        assert!(owner.instances.active.is_empty());
        assert!(owner.instances.topology_changed);
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn repeated_definition_has_independent_clocks_and_rejects_cycles() -> Result<()> {
        let graph = super::super::behavior::Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let state = State::new(&graph)?;
        let machine = &state.machines["2:240"];
        let definition = machine.states[machine.state_indices[&0]]
            .transitions
            .iter()
            .find(|t| t.event_id == 0 && t.to_state_id == 5)
            .unwrap()
            .transition
            .as_ref()
            .unwrap();
        let mut instances = Instances::default();
        let a = instances.clone_effect(definition)?;
        let b = instances.clone_effect(definition)?;
        assert_ne!(a, b);
        instances.bind_source(a, Binding::Generator("source".into()))?;
        instances.bind_source(b, Binding::Effect(a))?;
        assert!(instances.bind_source(a, Binding::Effect(b)).is_err());
        assert!(instances.release(a).is_err());
        for h in [a, b] {
            instances.get_mut(h)?.target = Some("target".into());
            instances.activate(h, &state.transition_effects, |_| false)?;
        }
        if let Some(Playback::Blend(p)) = &mut instances.get_mut(a)?.playback {
            p.clock.time = 0.125;
        } else {
            panic!("Expected original blending effect");
        }
        instances.activate(a, &state.transition_effects, |_| false)?;
        if let Some(Playback::Blend(p)) = &instances.get(b)?.playback {
            assert_eq!(p.clock.time, 0.);
        } else {
            panic!("Expected blending effect");
        }
        if let Some(Playback::Blend(p)) = &instances.get(a)?.playback {
            assert_eq!(p.clock.time, 0.125);
        }
        assert!(!instances.finished(b)?);
        instances.advance_duration_blend(b, &state.transition_effects, 1. / 30.)?;
        assert!(!instances.finished(b)?);
        instances.advance_duration_blend(b, &state.transition_effects, 1. / 30.)?;
        assert!(instances.finished(b)?);
        // Advancing an interrupted effect must not advance its retained source.
        if let Some(Playback::Blend(p)) = &instances.get(a)?.playback {
            assert_eq!(p.clock.time, 0.125);
        }
        instances.release(b)?;
        instances.release(a)?;
        assert!(instances.get(a).is_err());
        Ok(())
    }
}
