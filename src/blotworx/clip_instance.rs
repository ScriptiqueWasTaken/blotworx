//! A graph-owned clip instance. Resources remain shared; clocks and trigger
//! ordering belong to each generator, including generators using the same clip.
use super::{
    animation_resources,
    clip_runtime::{self, UpdateEffect},
    state_machine::Event,
};
use anyhow::{Context, Result};
use epic_mickey_lib_rs::{ffi_anim::FfiTransform, hkx_file::HkxSkeleton};
use std::collections::{btree_map::Entry, BTreeMap};

/// Character-local instances keyed by generator identity, never animation name.
/// The scheduler owns activation/deactivation order; replanning alone must not
/// restart clocks or retire a source still needed by a transition effect.
#[derive(Default)]
pub struct Instances {
    active: BTreeMap<String, Instance>,
}
impl Instances {
    pub fn start_echo(
        &mut self,
        id: &str,
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
        duration: f32,
    ) -> Result<()> {
        let instance = self
            .active
            .get_mut(id)
            .context("Echo target clip is inactive")?;
        instance.clock.start_echo(
            &authored.resolve(words)?,
            resource.info.duration,
            duration,
            &authored.triggers,
            &mut instance.order,
        )
    }
    pub fn activate(
        &mut self,
        id: &str,
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
    ) -> Result<&Instance> {
        Ok(match self.active.entry(id.into()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(Instance::activate(authored, words, resource)?),
        })
    }
    pub fn get(&self, id: &str) -> Result<&Instance> {
        self.active
            .get(id)
            .with_context(|| format!("Clip generator {id} is not active"))
    }
    pub fn deactivate(&mut self, id: &str) -> Result<()> {
        self.active
            .remove(id)
            .with_context(|| format!("Cannot deactivate inactive clip generator {id}"))?;
        Ok(())
    }
    pub fn advance(
        &mut self,
        id: &str,
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
        rig: &HkxSkeleton,
        dt: f32,
        event_flags: &[u32],
    ) -> Result<Frame> {
        self.active
            .get_mut(id)
            .with_context(|| format!("Cannot update inactive clip generator {id}"))?
            .advance(authored, words, resource, rig, dt, event_flags)
    }
    pub fn update(
        &mut self,
        id: &str,
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
        dt: f32,
        event_flags: &[u32],
    ) -> Result<Step> {
        self.active
            .get_mut(id)
            .context("Cannot update inactive clip generator")?
            .update(authored, words, resource, dt, event_flags)
    }
    pub fn sample(
        &self,
        id: &str,
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
        bones: usize,
    ) -> Result<Vec<Option<FfiTransform>>> {
        self.get(id)?.sample(authored, words, resource, bones)
    }
}

pub struct Instance {
    pub clock: clip_runtime::Runtime,
    order: Vec<usize>,
}
pub struct Frame {
    pub tracks: Vec<Option<FfiTransform>>,
    pub motion: FfiTransform,
    /// Graph-local events, in retail dispatch order. The owner must map and
    /// dispatch these before the next graph update.
    pub events: Vec<Event>,
}
pub struct Step {
    pub motion: FfiTransform,
    pub events: Vec<Event>,
}
impl Instance {
    pub fn activate(
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
    ) -> Result<Self> {
        let settings = authored.resolve(words)?;
        let mut clock = clip_runtime::Runtime::default();
        let order = clock.reset(&settings, resource.info.duration, false, &authored.triggers)?;
        Ok(Self { clock, order })
    }

    /// Called with the graph's synchronized timestep, never the render delta.
    /// Annotation triggers must already be appended to the authored trigger
    /// list by graph activation. This is bound-transform output, before graph
    /// blending, float tracks, modifiers and skeleton remapping.
    pub fn advance(
        &mut self,
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
        rig: &HkxSkeleton,
        dt: f32,
        event_flags: &[u32],
    ) -> Result<Frame> {
        let step = self.update(authored, words, resource, dt, event_flags)?;
        let tracks = self.sample(authored, words, resource, rig.bones.len())?;
        Ok(Frame {
            tracks,
            motion: step.motion,
            events: step.events,
        })
    }
    /// Update clocks/motion/events without decoding pose tracks. The graph may
    /// retire this generator before its separate generation phase.
    pub fn update(
        &mut self,
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
        dt: f32,
        event_flags: &[u32],
    ) -> Result<Step> {
        let mut settings = authored.resolve(words)?;
        let flags = settings.flags;
        let mut motion = None;
        let mut events = Vec::new();
        self.clock.update(
            &mut settings,
            resource.info.duration,
            dt,
            false,
            &authored.triggers,
            &mut self.order,
            Some(event_flags),
            |clock, effect| {
                match effect {
                    UpdateEffect::ExtractMotion(span) => {
                        motion = Some(resource.motion_delta(flags, clock.at_end, span)?);
                    }
                    UpdateEffect::Event(event) => events.push(event.clone()),
                }
                Ok(())
            },
        )?;
        Ok(Step {
            motion: motion.context("Clip update omitted motion extraction")?,
            events,
        })
    }
    pub fn sample(
        &self,
        authored: &clip_runtime::Clip,
        words: &[u32],
        resource: &animation_resources::Clip,
        bones: usize,
    ) -> Result<Vec<Option<FfiTransform>>> {
        if self.clock.echoes.is_empty() {
            resource.sample_tracks(bones, self.clock.local_time)
        } else {
            let settings = authored.resolve(words)?;
            let samples = self
                .clock
                .echo_samples(&settings, resource.info.duration)?
                .into_iter()
                .map(|(time, weight)| Ok((resource.sample_tracks(bones, time)?, weight)))
                .collect::<Result<Vec<_>>>()?;
            super::pose_blend::echo_tracks(samples)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original locally extracted Mickey assets"]
    fn instances_share_resource_but_not_clock_and_sample_committed_time() -> Result<()> {
        use epic_mickey_lib_rs::hkx_file::HkxFile;
        let root = std::path::Path::new("target/managed-player-assets");
        let rig = HkxFile::load_bytes(&std::fs::read(root.join("Mickey_Rig_Skin.hkx_wii"))?)
            .map_err(anyhow::Error::msg)?
            .parse_skeleton()
            .unwrap();
        let resource = animation_resources::Clip::load(
            &std::fs::read(root.join("Mickey_mov_run_brush.hkx_wii"))?,
            rig.bones.len(),
        )?;
        let graph = super::super::behavior::Document::load(&std::fs::read(
            root.join("MickeyBehavior.hkx_wii"),
        )?)?;
        let mut state = super::super::behavior_state::State::new(&graph)?;
        state.set_real(state.index("VAR_Scaled_Run_Speed")?, 1.)?;
        let authored = state
            .clips
            .values()
            .find(|c| {
                c.settings
                    .animation_name
                    .eq_ignore_ascii_case("Mickey_mov_run_brush.hkx")
            })
            .unwrap();
        let mut first = Instance::activate(authored, state.words(), &resource)?;
        let second = Instance::activate(authored, state.words(), &resource)?;
        let initial = second.clock.local_time;
        let frame = first.advance(
            authored,
            state.words(),
            &resource,
            &rig,
            1. / 30.,
            &vec![0; 163],
        )?;
        assert_eq!(second.clock.local_time, initial);
        assert_ne!(first.clock.local_time, initial);
        assert_eq!(
            super::super::pose_blend::resolve(&rig, &frame.tracks)?,
            resource.sample_bones(&rig, first.clock.local_time)?
        );
        assert_eq!(frame.motion.translation, [0.; 3]);
        // Inject authored-style triggers to verify they survive the combined
        // clock/resource/pose path with payload and order intact.
        let mut compiled = super::super::clip_runtime::compile(&graph, state.types())?;
        let authored = compiled
            .values_mut()
            .find(|c| {
                c.settings
                    .animation_name
                    .eq_ignore_ascii_case("Mickey_mov_run_brush.hkx")
            })
            .unwrap();
        authored.triggers = vec![clip_runtime::Trigger {
            local_time: initial,
            event: Event {
                id: 7,
                payload: Some(super::super::state_machine::Reference {
                    id: "retained-payload".into(),
                }),
            },
            relative_to_end_of_clip: false,
            acyclic: false,
            is_annotation: false,
        }];
        let mut instance = Instance::activate(authored, state.words(), &resource)?;
        let frame = instance.advance(
            authored,
            state.words(),
            &resource,
            &rig,
            1. / 30.,
            &vec![0; 163],
        )?;
        assert_eq!(frame.events.len(), 1);
        assert_eq!(frame.events[0].id, 7);
        assert_eq!(
            frame.events[0].payload.as_ref().unwrap().id,
            "retained-payload"
        );
        let mut instances = Instances::default();
        instances.activate("source", authored, state.words(), &resource)?;
        instances.advance(
            "source",
            authored,
            state.words(),
            &resource,
            &rig,
            1. / 30.,
            &vec![0; 163],
        )?;
        let retained_time = instances.get("source")?.clock.local_time;
        assert_ne!(retained_time, initial);
        // A retained source keeps its clock; another generator sharing this
        // resource starts independently and can retire without touching it.
        assert_eq!(
            instances
                .activate("source", authored, state.words(), &resource)?
                .clock
                .local_time,
            retained_time
        );
        assert_eq!(
            instances
                .activate("target", authored, state.words(), &resource)?
                .clock
                .local_time,
            initial
        );
        instances.deactivate("target")?;
        assert!(instances.get("target").is_err());
        assert_eq!(instances.get("source")?.clock.local_time, retained_time);
        assert_eq!(
            instances
                .activate("target", authored, state.words(), &resource)?
                .clock
                .local_time,
            initial
        );
        Ok(())
    }
}
