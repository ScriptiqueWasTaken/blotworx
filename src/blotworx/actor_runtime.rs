//! Per-entity Gamebryo activation, separate from the manual preview override.
use super::nif_sequence::{
    ActorBindings, ActorPose, ComponentClock, FadeDirection, FadeEnvelope, FadeSample,
    SequenceResources, UnsynchronizedClock,
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

struct Clip {
    id: u32,
    activation: u64,
    clock: UnsynchronizedClock,
}
impl Clip {
    fn new(resources: &SequenceResources, id: u32, activation: u64) -> Result<Self> {
        let (sequence, _) = resources.get(id).context("Unknown actor sequence ID")?;
        Ok(Self {
            id,
            activation,
            clock: UnsynchronizedClock::new(0., sequence.frequency)?,
        })
    }
    fn sample(
        &mut self,
        resources: &SequenceResources,
        bindings: &ActorBindings,
        time: f32,
    ) -> Result<ActorPose> {
        let (sequence, _) = resources.get(self.id).context("Missing active sequence")?;
        let scaled = self.clock.sample(time, sequence.duration, sequence.cycle_type)?;
        bindings.sample_activation(self.id, scaled, self.activation)
    }
}
struct CrossFade {
    source: Clip,
    outgoing: FadeEnvelope,
    incoming: FadeEnvelope,
}
pub struct Actor {
    pub clock: ComponentClock,
    current: Option<Clip>,
    transition: Option<CrossFade>,
    pending: Option<u32>,
    last_activation: u64,
    /// AnimationComponentGB's retained default sequence (+7c), updated by
    /// REL2f3ecc before manager playback. Reset retains this target.
    requested: Option<u32>,
    end_event: Option<(u32, f32)>,
    ended: Option<u32>,
    pub pose: Option<ActorPose>,
    pub world: BTreeMap<u32, epic_mickey_lib_rs::nif_file::NiTransform>,
}
impl Actor {
    pub fn new(use_delta_time: bool, start_global: bool) -> Self {
        let mut clock = ComponentClock::default();
        clock.use_delta_time = use_delta_time;
        clock.initialize(start_global, 0.);
        Self {
            clock,
            current: None,
            transition: None,
            pending: None,
            last_activation: 0,
            requested: None,
            end_event: None,
            ended: None,
            pose: None,
            world: BTreeMap::new(),
        }
    }
    pub fn active_id(&self) -> Option<u32> {
        self.current.as_ref().map(|clip| clip.id)
    }
    pub fn requested_id(&self) -> Option<u32> {
        self.requested
    }
    pub(super) fn bind_end_event(&mut self, id: u32, enabled: bool, resources: &SequenceResources) -> Result<()> {
        self.end_event = None;
        if !enabled {
            return Ok(());
        }
        let (sequence, nif) = resources.get(id).context("Unknown end-event sequence ID")?;
        let Some(mut end) = sequence.end_key(nif)? else {return Ok(())};
        if let Some(weighted) = self.current.as_ref().filter(|clip| clip.id == id)
            .and_then(|clip| clip.clock.weighted_time())
        {
            if weighted > end || (weighted == end && end == sequence.duration) {
                if sequence.cycle_type != 0 || sequence.duration <= 0. {
                    return Ok(());
                }
                let loops = ((weighted - end) / sequence.duration).floor() + 1.;
                end += loops * sequence.duration;
            }
        }
        self.end_event = Some((id, end));
        Ok(())
    }
    pub(super) fn take_ended(&mut self) -> Option<u32> {
        self.ended.take()
    }
    /// AnimationComponentGB retains +7c independently of Start Active.
    /// The property setter REL2f3ecc marks the target dirty; activation
    /// controls when the manager first consumes it, not whether it exists.
    pub fn retain_target(&mut self, id: u32) {
        self.requested = Some(id);
    }
    pub fn request(
        &mut self,
        id: u32,
        resources: &SequenceResources,
        bindings: &ActorBindings,
    ) -> Result<()> {
        bindings.sample(id, 0.)?; // Reject unsupported destination before changing live state.
        if self.end_event.is_some_and(|(owner, _)| owner != id) {
            self.end_event = None;
        }
        self.requested = Some(id);
        if self.active_id() == Some(id) {
            self.pending = None;
            return Ok(());
        } // Latest target cancels an older pending request.
        if self.transition.is_some() {
            self.pending = Some(id);
            return Ok(());
        }
        let activation = self.last_activation + 1;
        if let Some(current) = &self.current {
            let policy = resources
                .transition(current.id, id)?
                .context("No authored KFM transition between requested sequences")?;
            ensure!(
                policy.kind == 2 && policy.blend_pairs.is_empty() && policy.chain.is_empty(),
                "Unsupported actor transition policy {}",
                policy.kind
            );
            ensure!(
                policy.duration.is_finite() && policy.duration >= 0.,
                "Invalid KFM transition duration"
            );
            let duration = policy.duration;
            let destination = Clip::new(resources, id, activation)?;
            if duration > 0. {
                let mut source = self.current.take().unwrap();
                source.clock.prepare_crossfade_source()?;
                self.transition = Some(CrossFade {
                    source,
                    outgoing: FadeEnvelope::new(duration)?,
                    incoming: FadeEnvelope::new(duration)?,
                });
            }
            self.current = Some(destination);
        } else {
            self.current = Some(Clip::new(resources, id, activation)?)
        }
        self.last_activation = activation;
        Ok(())
    }
    pub fn reset(&mut self) {
        self.current = None;
        self.transition = None;
        self.pending = None;
        self.end_event = None;
        self.ended = None;
    }
    pub fn update(
        &mut self,
        resources: &SequenceResources,
        bindings: &ActorBindings,
        time: f32,
        delta: f32,
    ) -> Result<()> {
        // REL2f5d40..2f5dcc consumes the property's retained target on
        // periodic update after activation. This also covers waking through
        // SetIdle(false), not just a successful named-sequence request.
        if self.current.is_none() {
            if let Some(id) = self.requested.filter(|&id| resources.get(id).is_some()) {
                self.request(id, resources, bindings)?;
            }
        }
        self.clock.advance(true, delta);
        let time = self.clock.periodic_update_time(time);
        let Some(current) = &mut self.current else {
            return Ok(());
        };
        let destination = current.sample(resources, bindings, time)?;
        if let Some((id, end)) = self.end_event {
            if current.id == id && current.clock.weighted_time().is_some_and(|frame| frame >= end) {
                self.end_event = None;
                self.ended = Some(id);
            }
        }
        let mut complete = false;
        self.pose = Some(if let Some(transition) = &mut self.transition {
            let outgoing = transition.outgoing.sample(time, FadeDirection::Out)?;
            let incoming = transition.incoming.sample(time, FadeDirection::In)?;
            match (outgoing, incoming) {
                (FadeSample::Weight(a), FadeSample::Weight(b)) => {
                    let source = transition.source.sample(resources, bindings, time)?;
                    blend(source, a, destination, b)?
                }
                (FadeSample::Complete { end_time }, _) => {
                    // Outgoing sequence receives its final update at fade end.
                    transition.source.sample(resources, bindings, end_time)?;
                    complete = true;
                    destination
                }
                _ => anyhow::bail!("Actor fade envelopes lost their shared update cadence"),
            }
        } else {
            destination
        });
        if complete {
            self.transition = None;
            if let Some(id) = self.pending.take() {
                self.request(id, resources, bindings)?;
            }
        }
        Ok(())
    }
}

// Equal-priority valid channels normalize independently. NiPoseBlender uses
// hemisphere-corrected normalized quaternion sums, not slerp (2.6 source).
// Wii finalized-weight quantization/normalization arithmetic remains a parity
// item; do not reuse the unrelated Havok skeletal-pose blender here.
fn blend(mut a: ActorPose, wa: f32, b: ActorPose, wb: f32) -> Result<ActorPose> {
    ensure!(
        wa.is_finite() && wb.is_finite() && wa >= 0. && wb >= 0. && wa + wb > 0.,
        "Invalid actor blend weights"
    );
    let x = wa / (wa + wb);
    let y = wb / (wa + wb);
    // NiPoseBlender forwards the highest weighted referenced evaluator.
    // Zero-weight sequences cannot contribute a reference of their own.
    if wa == 0. {
        a.emissions.clear();
    }
    if wb > 0. {
        for (controller, emission) in b.emissions {
            if wb > wa {
                a.emissions.insert(controller, emission);
            } else {
                a.emissions.entry(controller).or_insert(emission);
            }
        }
    }
    for (key, value) in b.colors {
        if let Some(old) = a.colors.get_mut(&key) {
            *old = std::array::from_fn(|i| old[i] * x + value[i] * y);
        } else {
            a.colors.insert(key, value);
        }
    }
    // NiPoseBlender accumulates floats; NiMultiTargetPoseHandler thresholds
    // the final visibility at 0.5, after all channel weights are normalized.
    for (key, value) in b.visibility {
        if let Some(old) = a.visibility.get_mut(&key) {
            *old = *old * x + value * y;
        } else {
            a.visibility.insert(key, value);
        }
    }
    for (node, q) in b.transforms {
        let Some(p) = a.transforms.get_mut(&node) else {
            a.transforms.insert(node, q);
            continue;
        };
        p.translation = match (p.translation, q.translation) {
            (Some(a), Some(b)) => Some(std::array::from_fn(|i| a[i] * x + b[i] * y)),
            (a, b) => b.or(a),
        };
        p.scale = match (p.scale, q.scale) {
            (Some(a), Some(b)) => Some(a * x + b * y),
            (a, b) => b.or(a),
        };
        p.rotation = match (p.rotation, q.rotation) {
            (Some(a), Some(b)) => {
                let dot = a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
                let sign = if dot < 0. { -1. } else { 1. };
                let mut result: [f32; 4] = std::array::from_fn(|i| a[i] * x + b[i] * y * sign);
                let length = result.iter().map(|v| v * v).sum::<f32>().sqrt();
                ensure!(
                    length > 0. && length.is_finite(),
                    "Degenerate actor quaternion blend"
                );
                for v in &mut result {
                    *v /= length
                }
                Some(result)
            }
            (a, b) => b.or(a),
        };
    }
    // Morph geometry is linear in its weights. Matching full channel bindings
    // permit the same blend without recomputing shared target geometry.
    for (mesh, (first, vertices)) in b.morphs {
        if let Some((start, source)) = a.morphs.get_mut(&mesh) {
            ensure!(
                *start == first && source.len() == vertices.len(),
                "Incompatible morph source regions during transition"
            );
            for (a, b) in source.iter_mut().zip(vertices) {
                for i in 0..3 {
                    a[i] = a[i] * x + b[i] * y
                }
            }
        } else {
            a.morphs.insert(mesh, (first, vertices));
        }
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blend_preserves_missing_channels_and_quaternion_hemisphere() -> Result<()> {
        let pose = |rotation, translation| ActorPose {
            transforms: BTreeMap::from([(
                7,
                super::super::nif_transform::Pose {
                    rotation,
                    translation,
                    scale: None,
                },
            )]),
            morphs: BTreeMap::new(),
            emissions: BTreeMap::new(),
            colors: BTreeMap::new(),
            visibility: BTreeMap::new(),
        };
        let result = blend(
            pose(Some([1., 0., 0., 0.]), Some([2., 0., 0.])),
            0.5,
            pose(Some([-1., 0., 0., 0.]), None),
            0.5,
        )?;
        assert_eq!(result.transforms[&7].translation, Some([2., 0., 0.]));
        assert_eq!(result.transforms[&7].rotation, Some([1., 0., 0., 0.]));
        Ok(())
    }
}
