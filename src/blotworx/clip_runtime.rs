//! Authored clip inputs and retail activation/reset (DOL 0x801fa05c).
//! Clock and event update ordering follows 0x801fb878. The owning graph must
//! provide resource binding, event remapping, motion extraction and poses.
use super::{
    behavior::Document,
    state_machine::{Event, Reference},
};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trigger {
    pub local_time: f32,
    pub event: Event,
    pub relative_to_end_of_clip: bool,
    pub acyclic: bool,
    pub is_annotation: bool,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub variable_binding_set: Option<Reference>,
    pub user_data: u64,
    pub name: String,
    pub animation_name: String,
    pub triggers: Option<Reference>,
    pub crop_start_amount_local_time: f32,
    pub crop_end_amount_local_time: f32,
    pub start_time: f32,
    pub playback_speed: f32,
    pub enforced_duration: f32,
    pub user_controlled_time_fraction: f32,
    pub animation_binding_index: i16,
    pub mode: i8,
    pub flags: i8,
}
impl Settings {
    fn validate(&self) -> Result<()> {
        ensure!(
            [
                self.crop_start_amount_local_time,
                self.crop_end_amount_local_time,
                self.start_time,
                self.playback_speed,
                self.enforced_duration,
                self.user_controlled_time_fraction
            ]
            .into_iter()
            .all(f32::is_finite),
            "Non-finite clip settings: {}",
            self.name
        );
        ensure!(
            (0..=3).contains(&self.mode),
            "Unknown clip mode: {}",
            self.mode
        );
        Ok(())
    }
}
pub struct Clip {
    pub settings: Settings,
    pub triggers: Vec<Trigger>,
    playback_variable: Option<usize>,
}
impl Clip {
    pub fn resolve(&self, words: &[u32]) -> Result<Settings> {
        let mut settings = self.settings.clone();
        if let Some(i) = self.playback_variable {
            settings.playback_speed =
                f32::from_bits(*words.get(i).context("Missing clip variable")?);
        }
        settings.validate()?;
        Ok(settings)
    }
}
pub fn compile(graph: &Document, types: &[u8]) -> Result<BTreeMap<String, Clip>> {
    let mut clips = BTreeMap::new();
    for (id, object) in &graph.objects {
        if object.klass != "hkbClipGenerator" {
            continue;
        }
        let mut fields = object.fields.clone();
        // 2012 adds optional bundle routing and partition selection. The
        // unbundled, unpartitioned case has the same runtime semantics as Wii.
        if let Some(bundle) = fields
            .as_object_mut()
            .unwrap()
            .remove("animationBundleName")
        {
            ensure!(
                bundle.is_null() || bundle.as_str() == Some(""),
                "Clip bundle routing is not implemented: {id}"
            );
        }
        if let Some(mask) = fields.as_object_mut().unwrap().remove("userPartitionMask") {
            ensure!(
                mask.as_u64() == Some(0),
                "Clip partition masks are not implemented: {id}"
            );
        }
        let settings: Settings =
            serde_json::from_value(fields).with_context(|| format!("Invalid clip {id}"))?;
        settings.validate()?;
        let mut playback_variable = None;
        if let Some(reference) = &settings.variable_binding_set {
            let b = graph
                .objects
                .get(&reference.id)
                .context("Missing clip binding set")?;
            ensure!(
                b.klass == "hkbVariableBindingSet",
                "Invalid clip binding set"
            );
            for binding in b.fields["bindings"]
                .as_array()
                .context("Invalid clip bindings")?
            {
                ensure!(
                    binding["memberPath"] == "playbackSpeed"
                        && binding["bindingType"] == 0
                        && binding["bitIndex"] == -1,
                    "Unsupported clip binding: {binding}"
                );
                let index = binding["variableIndex"]
                    .as_u64()
                    .context("Invalid clip variable")? as usize;
                ensure!(
                    types.get(index) == Some(&4),
                    "Clip playback speed requires real variable"
                );
                ensure!(
                    playback_variable.replace(index).is_none(),
                    "Duplicate playback-speed binding"
                );
            }
        }
        let triggers: Vec<Trigger> = if let Some(reference) = &settings.triggers {
            let t = graph
                .objects
                .get(&reference.id)
                .context("Missing clip trigger array")?;
            ensure!(
                t.klass == "hkbClipTriggerArray",
                "Invalid clip trigger array"
            );
            serde_json::from_value(t.fields["triggers"].clone())?
        } else {
            vec![]
        };
        for trigger in &triggers {
            ensure!(trigger.local_time.is_finite(), "Non-finite clip trigger");
            if let Some(payload) = &trigger.event.payload {
                ensure!(
                    graph.objects.contains_key(&payload.id),
                    "Missing clip event payload"
                );
            }
        }
        clips.insert(
            id.clone(),
            Clip {
                settings,
                triggers,
                playback_variable,
            },
        );
    }
    Ok(clips)
}

#[derive(Default, Debug)]
pub struct Runtime {
    pub local_time: f32,
    pub crop_start: f32,
    pub crop_end: f32,
    pub time: f32,
    pub previous_user_fraction: f32,
    pub at_end: bool,
    pub ping_pong_backward: bool,
    pub echoes: Vec<Echo>,
}
#[derive(Debug)]
pub struct Echo {
    pub offset: f32,
    pub weight: f32,
    pub fade_speed: f32,
}
/// The graph must service motion extraction before receiving this tick's
/// events. This mandatory callback prevents silently dropping root motion.
pub enum UpdateEffect<'a> {
    ExtractMotion(&'a Span),
    Event(&'a Event),
}
/// Sampling interval passed to motion extraction and trigger dispatch. Merely
/// calculating it must not change the control or emit events.
#[derive(Debug, PartialEq)]
pub struct Span {
    pub from: f32,
    pub to: f32,
    pub loops: i32,
    pub at_end: bool,
}
impl Runtime {
    /// DOL 0x801fd4c4: restart while retaining fading playback offsets.
    pub fn start_echo(
        &mut self,
        settings: &Settings,
        animation_duration: f32,
        duration: f32,
        triggers: &[Trigger],
        order: &mut Vec<usize>,
    ) -> Result<()> {
        settings.validate()?;
        ensure!(
            duration.is_finite() && duration >= 0.,
            "Invalid clip echo duration"
        );
        if settings.mode == 2 {
            return Ok(());
        }
        let span = self.duration(animation_duration);
        let start = if settings.enforced_duration > 0. {
            (settings.start_time / settings.enforced_duration) * span
        } else {
            settings.start_time
        };
        let mut offset = self.local_time - (start.min(span) + self.crop_start);
        if offset < 0. {
            offset += animation_duration;
        }
        ensure!(offset.is_finite(), "Clip echo offset overflow");
        *order = self.reset(settings, animation_duration, false, triggers)?;
        if duration == 0. {
            self.echoes.clear();
            return Ok(());
        }
        let span = self.duration(animation_duration);
        let mut weight = 0.;
        for echo in &mut self.echoes {
            echo.offset += offset;
            if echo.offset > span && settings.mode == 1 {
                echo.offset -= span;
            }
            weight += echo.weight;
        }
        let fade_speed = -1. / duration;
        ensure!(
            weight.is_finite() && fade_speed.is_finite(),
            "Clip echo overflow"
        );
        self.echoes.push(Echo {
            offset,
            weight: 1. - weight,
            fade_speed,
        });
        Ok(())
    }

    /// DOL 0x801fab18: echoes in insertion order, followed by the current
    /// playback with the remaining weight. Sampling does not emit events.
    pub fn echo_samples(
        &self,
        settings: &Settings,
        animation_duration: f32,
    ) -> Result<Vec<(f32, f32)>> {
        let mut samples = Vec::with_capacity(self.echoes.len() + 1);
        let mut total = 0.;
        for (offset, weight) in self
            .echoes
            .iter()
            .map(|e| (e.offset, Some(e.weight)))
            .chain(std::iter::once((0., None)))
        {
            let (mut time, loops) = control_future(
                self.local_time,
                self.crop_start,
                self.crop_end,
                animation_duration,
                1.,
                offset,
            )?;
            if settings.mode == 0 && loops != 0 {
                time = animation_duration + self.crop_start;
            }
            let weight = if let Some(weight) = weight {
                total += weight;
                weight
            } else {
                1. - total
            };
            ensure!(
                time.is_finite() && weight.is_finite(),
                "Invalid echo sample"
            );
            samples.push((time, weight));
        }
        Ok(samples)
    }
    pub fn duration(&self, animation_duration: f32) -> f32 {
        (animation_duration - (self.crop_end + self.crop_start)).max(0.)
    }
    /// Retail clip update ordering (0x801fb878..0x801fc148). Resource binding,
    /// skeleton mapping and pose generation belong to the owning graph.
    pub fn update(
        &mut self,
        settings: &mut Settings,
        animation_duration: f32,
        dt: f32,
        ignore_start_time: bool,
        triggers: &[Trigger],
        order: &mut Vec<usize>,
        event_flags: Option<&[u32]>,
        mut effect: impl FnMut(&Self, UpdateEffect<'_>) -> Result<()>,
    ) -> Result<()> {
        settings.validate()?;
        ensure!(dt.is_finite(), "Invalid clip timestep");
        if self.at_end && settings.mode != 0 {
            *order = self.reset(settings, animation_duration, ignore_start_time, triggers)?;
        }
        if settings.mode == 2 {
            settings.user_controlled_time_fraction =
                settings.user_controlled_time_fraction.clamp(0., 1.);
        }
        let duration = self.duration(animation_duration);
        let scale = if duration > 0. && settings.enforced_duration > 0. {
            duration / settings.enforced_duration
        } else {
            1.
        };
        let mut speed = scale * settings.playback_speed;
        if settings.mode == 3 && self.ping_pong_backward {
            speed = -speed;
        }
        ensure!(speed.is_finite(), "Clip speed overflow");
        if self.at_end {
            self.at_end = if self.local_time <= self.crop_start {
                speed <= 0.
            } else {
                speed >= 0.
            };
        }
        let span = self.future_span(settings, animation_duration, dt)?;
        if !self.at_end {
            self.local_time = span.to;
        }
        effect(self, UpdateEffect::ExtractMotion(&span))?;
        if !self.at_end {
            self.at_end = span.at_end;
            self.emit_cyclic_events(
                settings,
                animation_duration,
                &span,
                triggers,
                order,
                event_flags,
                |event| effect(self, UpdateEffect::Event(event)),
            )?;
            if span.loops & 1 != 0 && settings.mode != 2 {
                self.ping_pong_backward = !self.ping_pong_backward;
            }
        }
        if settings.mode != 2 {
            // Uses the pre-reflection control speed, even after a ping-pong
            // crossing or after one-shot playback has stopped at its endpoint.
            let next_time = self.time + dt * speed;
            ensure!(next_time.is_finite(), "Accumulated clip time overflow");
            for &i in order.iter() {
                let trigger = triggers.get(i).context("Invalid clip trigger index")?;
                if !trigger.acyclic {
                    continue;
                }
                // In this retail path a missing event-info table suppresses
                // acyclic events (unlike the cyclic path).
                let Some(flags) = event_flags else {
                    continue;
                };
                ensure!(trigger.event.id >= 0, "Negative acyclic event index");
                if flags.get(trigger.event.id as usize).copied().unwrap_or(0) & 1 != 0 {
                    continue;
                }
                if self.time <= trigger.local_time && next_time > trigger.local_time {
                    effect(self, UpdateEffect::Event(&trigger.event))?;
                }
            }
            self.time = next_time;
            for echo in &mut self.echoes {
                echo.weight = dt.mul_add(echo.fade_speed, echo.weight);
                ensure!(echo.weight.is_finite(), "Animation echo weight overflow");
            }
            self.echoes.retain(|echo| echo.weight > 0.);
        }
        self.previous_user_fraction = settings.user_controlled_time_fraction;
        Ok(())
    }
    /// Feed a sampling interval into the retail cyclic-trigger path
    /// (0x801fcd64 -> 0x801fc8ec). The caller supplies reset's stable order and
    /// a queue/remapper callback, so event payloads are not discarded. Acyclic
    /// triggers use the separate accumulated-time path, not this loop clock.
    pub fn emit_cyclic_events(
        &self,
        settings: &Settings,
        animation_duration: f32,
        span: &Span,
        triggers: &[Trigger],
        order: &[usize],
        event_flags: Option<&[u32]>,
        mut emit: impl FnMut(&Event) -> Result<()>,
    ) -> Result<()> {
        settings.validate()?;
        ensure!(
            order.len() == triggers.len(),
            "Incomplete clip trigger order"
        );
        ensure!(
            [
                animation_duration,
                span.from,
                span.to,
                self.crop_start,
                self.crop_end
            ]
            .into_iter()
            .all(f32::is_finite)
                && animation_duration >= 0.,
            "Invalid trigger interval"
        );
        let duration = self.duration(animation_duration);
        ensure!(duration.is_finite(), "Trigger duration overflow");
        let mut backward = self.ping_pong_backward;
        let scale = if duration > 0. && settings.enforced_duration > 0. {
            duration / settings.enforced_duration
        } else {
            1.
        };
        let speed = settings.playback_speed * scale;
        ensure!(speed.is_finite(), "Trigger playback-speed overflow");
        let forward = |backward| if settings.mode == 3 && backward {-speed} else {speed} > 0.;
        let mut interval = |a: f32, b: f32, include_end: bool| -> Result<()> {
            let (mut lower, mut upper) = (a.min(b) - self.crop_start, a.max(b) - self.crop_start);
            let mut event_duration = duration;
            if settings.enforced_duration > 0. {
                ensure!(
                    duration > 0.,
                    "Cannot scale triggers on an empty enforced-duration clip"
                );
                let ratio = settings.enforced_duration / duration;
                lower *= ratio;
                upper *= ratio;
                event_duration = settings.enforced_duration;
            }
            for &i in order {
                let trigger = triggers.get(i).context("Invalid clip trigger index")?;
                ensure!(trigger.local_time.is_finite(), "Non-finite trigger time");
                if let Some(flags) = event_flags {
                    ensure!(
                        trigger.event.id >= 0,
                        "Negative event cannot index retail event flags"
                    );
                    if flags.get(trigger.event.id as usize).copied().unwrap_or(0) & 1 != 0 {
                        continue;
                    }
                }
                if trigger.acyclic {
                    continue;
                }
                let time = if trigger.relative_to_end_of_clip {
                    event_duration + trigger.local_time
                } else {
                    trigger.local_time
                };
                let time = time.clamp(0., event_duration);
                if lower <= time
                    && (if include_end {
                        upper >= time
                    } else {
                        upper > time
                    })
                {
                    emit(&trigger.event)?;
                }
            }
            Ok(())
        };
        if span.loops == 0 {
            return interval(span.from, span.to, span.at_end);
        }
        interval(
            span.from,
            if forward(backward) { duration } else { 0. },
            true,
        )?;
        if settings.mode == 3 {
            backward = !backward;
        }
        // The original uses signed loop count here, not absolute count.
        for _ in 1..span.loops {
            if forward(backward) {
                interval(0., duration, true)?;
            } else {
                interval(duration, 0., true)?;
            }
            if settings.mode == 3 {
                backward = !backward;
            }
        }
        interval(
            if forward(backward) { 0. } else { duration },
            span.to,
            span.at_end,
        )
    }
    /// Retail 0x801fa620, using the unlimited-cycle control constructed at
    /// 0x801f9dbc. The update caller must first perform end-state recovery and
    /// clamp the user-controlled fraction, then consume this interval for pose,
    /// motion and events before committing the resulting state.
    pub fn future_span(
        &self,
        settings: &Settings,
        animation_duration: f32,
        dt: f32,
    ) -> Result<Span> {
        settings.validate()?;
        ensure!(
            [
                animation_duration,
                dt,
                self.local_time,
                self.crop_start,
                self.crop_end,
                self.previous_user_fraction
            ]
            .into_iter()
            .all(f32::is_finite)
                && animation_duration >= 0.,
            "Invalid clip clock input"
        );
        let raw_duration = animation_duration - (self.crop_end + self.crop_start);
        ensure!(raw_duration.is_finite(), "Clip duration overflow");
        let duration = raw_duration.max(0.);
        let mut span = Span {
            from: self.local_time,
            to: self.local_time,
            loops: 0,
            at_end: self.at_end,
        };
        if settings.mode == 2 {
            span.from = self
                .previous_user_fraction
                .mul_add(duration, settings.crop_start_amount_local_time);
            span.to = settings
                .user_controlled_time_fraction
                .mul_add(duration, settings.crop_start_amount_local_time);
        } else {
            let scale = if duration > 0. && settings.enforced_duration > 0. {
                duration / settings.enforced_duration
            } else {
                1.
            };
            let mut speed = scale * settings.playback_speed;
            if settings.mode == 3 && self.ping_pong_backward {
                speed = -speed;
            }
            ensure!(speed.is_finite(), "Clip playback-speed overflow");
            if !self.at_end {
                (span.to, span.loops) = control_future(
                    self.local_time,
                    self.crop_start,
                    self.crop_end,
                    animation_duration,
                    speed,
                    dt,
                )?;
                if settings.mode == 0 {
                    // Separate multiply/add here, unlike the control's FMA.
                    let unwrapped = self.local_time + dt * speed;
                    let end = duration + self.crop_start;
                    if unwrapped >= end {
                        span.to = end;
                        span.at_end = true;
                        span.loops = 0;
                    } else if unwrapped <= 0. {
                        // Retail clamps to zero here, not to crop_start.
                        span.to = 0.;
                        span.at_end = true;
                        span.loops = 0;
                    }
                } else if settings.mode == 3 && span.loops & 1 != 0 {
                    span.to = 2f32.mul_add(self.crop_start, duration - span.to);
                }
            } else if settings.flags & 1 != 0 {
                // CONTINUE_MOTION_AT_END uses dt directly, not scaled speed.
                let end = duration + self.crop_start;
                if speed > 0. {
                    span.from = (end - dt).max(0.);
                    span.to = end;
                } else {
                    span.from = dt.min(end);
                    span.to = 0.;
                }
            } else {
                span.from = 0.;
                span.to = 0.;
            }
        }
        ensure!(
            span.from.is_finite() && span.to.is_finite(),
            "Clip interval overflow"
        );
        Ok(span)
    }
    /// Reset the clip clock and return stable trigger order, including any
    /// annotations already appended by the caller. Payloads remain attached to
    /// their triggers. This does not emit events or initialize a pose.
    pub fn reset(
        &mut self,
        settings: &Settings,
        animation_duration: f32,
        ignore_start_time: bool,
        triggers: &[Trigger],
    ) -> Result<Vec<usize>> {
        settings.validate()?;
        ensure!(
            animation_duration.is_finite() && animation_duration >= 0.,
            "Invalid animation duration"
        );
        ensure!(
            [self.local_time, self.crop_start, self.crop_end]
                .into_iter()
                .all(f32::is_finite),
            "Non-finite retained clip control"
        );
        // Validate first so an invalid trigger cannot partially reset a clip.
        ensure!(
            triggers.iter().all(|t| t.local_time.is_finite()),
            "Non-finite clip trigger"
        );
        let crop_start = if ignore_start_time {
            self.crop_start
        } else {
            settings.crop_start_amount_local_time
        };
        let crop_end = if ignore_start_time {
            self.crop_end
        } else {
            settings.crop_end_amount_local_time
        };
        ensure!(
            (animation_duration - (crop_end + crop_start)).is_finite(),
            "Clip duration overflow"
        );
        if !ignore_start_time {
            let duration = (animation_duration - (crop_end + crop_start)).max(0.);
            let start = if settings.enforced_duration > 0. {
                (settings.start_time / settings.enforced_duration) * duration
            } else {
                settings.start_time
            };
            // Retail has an upper clamp only. Do not invent a lower clamp.
            let time = crop_start + start.min(duration);
            ensure!(
                start.is_finite() && time.is_finite(),
                "Clip start-time overflow"
            );
            self.crop_start = crop_start;
            self.crop_end = crop_end;
            self.local_time = time;
        }
        let duration = self.duration(animation_duration);
        let mut relative = self.local_time - self.crop_start;
        if settings.playback_speed < 0. {
            if relative == 0. {
                relative = duration;
            }
        } else if relative == duration {
            relative = 0.;
        }
        self.local_time = relative + self.crop_start;
        self.at_end = false;
        self.ping_pong_backward = false;
        self.time = 0.;
        self.previous_user_fraction = settings.user_controlled_time_fraction;
        // 0x801fd1fc: acyclic triggers bypass both relative-time conversion and
        // clamping. Retail's adjacent swaps preserve original order for ties.
        let duration = if settings.enforced_duration > 0. {
            settings.enforced_duration
        } else {
            duration
        };
        let key = |t: &Trigger| {
            if t.acyclic {
                t.local_time
            } else {
                let time = if t.relative_to_end_of_clip {
                    duration + t.local_time
                } else {
                    t.local_time
                };
                time.clamp(0., duration)
            }
        };
        let mut order: Vec<_> = (0..triggers.len()).collect();
        order.sort_by(|&a, &b| key(&triggers[a]).partial_cmp(&key(&triggers[b])).unwrap());
        Ok(order)
    }
}

/// hkaDefaultAnimationControl::getFutureTime, 0x801e58b0. Its subtraction order
/// and strict upper comparison differ from the generator's duration helper.
/// Only unlimited cycles are needed by hkbClipGenerator's control constructor.
fn control_future(
    time: f32,
    crop_start: f32,
    crop_end: f32,
    duration: f32,
    speed: f32,
    dt: f32,
) -> Result<(f32, i32)> {
    let duration = (duration - crop_end) - crop_start;
    ensure!(duration.is_finite(), "Control duration overflow");
    if duration <= 0. {
        return Ok((time, 0));
    }
    let mut time = dt.mul_add(speed, time - crop_start);
    ensure!(time.is_finite(), "Control time overflow");
    let mut loops = 0;
    if time > duration || time < 0. {
        let quotient = time / duration;
        // fctiwz's invalid-conversion result is outside the supported finite
        // clock domain. Reject it rather than Rust's saturating float cast.
        ensure!(
            quotient.is_finite() && quotient >= -2147483520. && quotient < 2147483648.,
            "Animation loop count exceeds signed 32-bit control storage"
        );
        loops = quotient as i32;
        if time < 0. {
            loops -= 1;
        }
        time = (-duration).mul_add(loops as f32, time);
    }
    Ok((time + crop_start, loops))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn echo_restart_retains_offsets_and_repeated_echo_weights() -> Result<()> {
        let mut s = settings();
        s.crop_start_amount_local_time = 0.;
        s.crop_end_amount_local_time = 0.;
        let mut r = Runtime::default();
        let mut order = r.reset(&s, 2., false, &[])?;
        r.local_time = 0.75;
        r.start_echo(&s, 2., 0.5, &[], &mut order)?;
        assert_eq!(r.local_time, 0.);
        assert_eq!(r.echo_samples(&s, 2.)?, vec![(0.75, 1.), (0., 0.)]);
        r.update(&mut s, 2., 0.125, false, &[], &mut order, None, |_, _| {
            Ok(())
        })?;
        assert_eq!(r.echo_samples(&s, 2.)?, vec![(0.875, 0.75), (0.125, 0.25)]);
        r.start_echo(&s, 2., 0.25, &[], &mut order)?;
        assert_eq!(
            r.echo_samples(&s, 2.)?,
            vec![(0.875, 0.75), (0.125, 0.25), (0., 0.)]
        );
        assert_eq!(r.echoes[0].fade_speed, -2.);
        assert_eq!(r.echoes[1].fade_speed, -4.);
        r.start_echo(&s, 2., 0., &[], &mut order)?;
        assert!(r.echoes.is_empty());
        r.local_time = 0.4;
        s.mode = 2;
        r.start_echo(&s, 2., 0.5, &[], &mut order)?;
        assert_eq!(r.local_time, 0.4);
        assert!(r.echoes.is_empty());
        Ok(())
    }
    fn settings() -> Settings {
        Settings {
            variable_binding_set: None,
            user_data: 0,
            name: "test".into(),
            animation_name: "test.hkx".into(),
            triggers: None,
            crop_start_amount_local_time: 1.,
            crop_end_amount_local_time: 2.,
            start_time: 0.,
            playback_speed: 1.,
            enforced_duration: 0.,
            user_controlled_time_fraction: 0.3,
            animation_binding_index: -1,
            mode: 1,
            flags: 0,
        }
    }
    #[test]
    fn update_orders_motion_cyclic_and_acyclic_events_and_resumes_reversed_one_shot() -> Result<()>
    {
        let mut s = settings();
        s.mode = 0;
        s.crop_start_amount_local_time = 0.;
        s.crop_end_amount_local_time = 0.;
        let mut r = Runtime::default();
        let triggers: Vec<_> = [(1., false), (1., true)]
            .into_iter()
            .enumerate()
            .map(|(i, (t, a))| Trigger {
                local_time: t,
                event: Event {
                    id: i as i32,
                    payload: None,
                },
                relative_to_end_of_clip: false,
                acyclic: a,
                is_annotation: false,
            })
            .collect();
        let mut order = r.reset(&s, 1., false, &triggers)?;
        r.echoes = vec![Echo {
            offset: 0.,
            weight: 1.,
            fade_speed: -1.,
        }];
        let mut effects = vec![];
        r.update(
            &mut s,
            1.,
            1.,
            false,
            &triggers,
            &mut order,
            Some(&[]),
            |state, e| {
                match e {
                    UpdateEffect::ExtractMotion(span) => {
                        assert_eq!(span.to, 1.);
                        assert_eq!(state.local_time, 1.);
                        assert!(!state.at_end);
                        effects.push(-1);
                    }
                    UpdateEffect::Event(e) => {
                        assert!(state.at_end);
                        effects.push(e.id);
                    }
                };
                Ok(())
            },
        )?;
        assert_eq!(effects, vec![-1, 0]); // acyclic upper bound is exclusive
        assert!(r.echoes.is_empty());
        effects.clear();
        r.update(
            &mut s,
            1.,
            0.25,
            false,
            &triggers,
            &mut order,
            Some(&[]),
            |_, e| {
                match e {
                    UpdateEffect::ExtractMotion(_) => effects.push(-1),
                    UpdateEffect::Event(e) => effects.push(e.id),
                };
                Ok(())
            },
        )?;
        assert_eq!(effects, vec![-1, 1]);
        assert_eq!(r.time, 1.25);
        s.playback_speed = -1.;
        r.update(
            &mut s,
            1.,
            0.25,
            false,
            &triggers,
            &mut order,
            Some(&[]),
            |_, _| Ok(()),
        )?;
        assert!(!r.at_end);
        assert_eq!(r.local_time, 0.75);
        Ok(())
    }
    #[test]
    fn clock_intervals_deliver_payloads_once_at_shared_boundaries_and_repeat_on_loops() -> Result<()>
    {
        let mut s = settings();
        s.crop_start_amount_local_time = 0.;
        s.crop_end_amount_local_time = 0.;
        let mut r = Runtime::default();
        let triggers: Vec<_> = [0., 0.5, 1.]
            .into_iter()
            .enumerate()
            .map(|(i, t)| Trigger {
                local_time: t,
                event: Event {
                    id: i as i32,
                    payload: Some(Reference {
                        id: format!("payload{i}"),
                    }),
                },
                relative_to_end_of_clip: false,
                acyclic: false,
                is_annotation: false,
            })
            .collect();
        let order = r.reset(&s, 1., false, &triggers)?;
        let mut events = vec![];
        let first = r.future_span(&s, 1., 0.5)?;
        r.emit_cyclic_events(&s, 1., &first, &triggers, &order, None, |e| {
            events.push((e.id, e.payload.as_ref().unwrap().id.clone()));
            Ok(())
        })?;
        r.local_time = first.to;
        let second = r.future_span(&s, 1., 0.5)?;
        r.emit_cyclic_events(&s, 1., &second, &triggers, &order, None, |e| {
            events.push((e.id, e.payload.as_ref().unwrap().id.clone()));
            Ok(())
        })?;
        assert_eq!(events, vec![(0, "payload0".into()), (1, "payload1".into())]);
        r.local_time = second.to;
        let wrap = r.future_span(&s, 1., 0.25)?;
        let mut ids = vec![];
        r.emit_cyclic_events(&s, 1., &wrap, &triggers, &order, None, |e| {
            ids.push(e.id);
            Ok(())
        })?;
        assert_eq!(ids, vec![2, 0]);
        ids.clear();
        r.emit_cyclic_events(&s, 1., &wrap, &triggers, &order, Some(&[1, 0, 0]), |e| {
            ids.push(e.id);
            Ok(())
        })?;
        assert_eq!(ids, vec![2]);
        Ok(())
    }
    #[test]
    fn captured_retail_sampling_intervals_match_bit_for_bit() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/clip_clock.json"
        ))?;
        let samples = fixture["samples"]
            .as_array()
            .context("Missing clock samples")?;
        assert!(!samples.is_empty());
        for sample in samples {
            let f = |v: &serde_json::Value, key: &str| v[key].as_f64().unwrap() as f32;
            let p = &sample["settings"];
            let mut s = settings();
            s.crop_start_amount_local_time = f(p, "crop_start");
            s.crop_end_amount_local_time = f(p, "crop_end");
            s.start_time = f(p, "start_time");
            s.playback_speed = f(p, "playback_speed");
            s.enforced_duration = f(p, "enforced_duration");
            s.user_controlled_time_fraction = f(p, "user_fraction");
            s.mode = p["mode"].as_i64().unwrap() as i8;
            s.flags = p["flags"].as_i64().unwrap() as i8;
            let v = &sample["runtime"];
            let r = Runtime {
                local_time: f(v, "local_time"),
                crop_start: f(v, "crop_start"),
                crop_end: f(v, "crop_end"),
                previous_user_fraction: f(v, "previous_user_fraction"),
                at_end: v["at_end"].as_bool().unwrap(),
                ping_pong_backward: v["ping_pong_backward"].as_bool().unwrap(),
                ..Default::default()
            };
            let span = r.future_span(&s, f(sample, "duration"), f(sample, "dt"))?;
            let expected = &sample["result"];
            assert_eq!(span.from.to_bits(), f(expected, "from_time").to_bits());
            assert_eq!(span.to.to_bits(), f(expected, "to_time").to_bits());
            assert_eq!(span.loops, expected["loops"].as_i64().unwrap() as i32);
            assert_eq!(span.at_end, expected["at_end"].as_bool().unwrap());
        }
        Ok(())
    }
    #[test]
    fn loop_endpoints_reverse_exact_cycles_and_ping_pong() -> Result<()> {
        let s = settings(); // cropped range 1..8, length 7
        let mut r = Runtime {
            local_time: 7.,
            crop_start: 1.,
            crop_end: 2.,
            ..Default::default()
        };
        assert_eq!(
            r.future_span(&s, 10., 1.)?,
            Span {
                from: 7.,
                to: 8.,
                loops: 0,
                at_end: false
            }
        );
        assert_eq!(
            r.future_span(&s, 10., 2.)?,
            Span {
                from: 7.,
                to: 2.,
                loops: 1,
                at_end: false
            }
        );
        let mut reverse = s.clone();
        reverse.playback_speed = -1.;
        r.local_time = 1.;
        // Exact negative cycles retain the upper endpoint and one extra loop.
        assert_eq!(
            r.future_span(&reverse, 10., 7.)?,
            Span {
                from: 1.,
                to: 8.,
                loops: -2,
                at_end: false
            }
        );
        let mut ping = s.clone();
        ping.mode = 3;
        r.local_time = 7.;
        assert_eq!(
            r.future_span(&ping, 10., 2.)?,
            Span {
                from: 7.,
                to: 7.,
                loops: 1,
                at_end: false
            }
        );
        r.ping_pong_backward = true;
        r.local_time = 2.;
        assert_eq!(
            r.future_span(&ping, 10., 2.)?,
            Span {
                from: 2.,
                to: 2.,
                loops: -1,
                at_end: false
            }
        );
        Ok(())
    }
    #[test]
    fn one_shot_end_and_continued_motion_are_distinct_intervals() -> Result<()> {
        let mut s = settings();
        s.mode = 0;
        let mut r = Runtime {
            local_time: 7.,
            crop_start: 1.,
            crop_end: 2.,
            ..Default::default()
        };
        assert_eq!(
            r.future_span(&s, 10., 1.)?,
            Span {
                from: 7.,
                to: 8.,
                loops: 0,
                at_end: true
            }
        );
        r.at_end = true;
        r.local_time = 8.;
        assert_eq!(
            r.future_span(&s, 10., 0.5)?,
            Span {
                from: 0.,
                to: 0.,
                loops: 0,
                at_end: true
            }
        );
        s.flags = 1;
        s.playback_speed = 3.;
        assert_eq!(
            r.future_span(&s, 10., 0.5)?,
            Span {
                from: 7.5,
                to: 8.,
                loops: 0,
                at_end: true
            }
        );
        s.playback_speed = -3.;
        assert_eq!(
            r.future_span(&s, 10., 0.5)?,
            Span {
                from: 0.5,
                to: 0.,
                loops: 0,
                at_end: true
            }
        );
        r.at_end = false;
        r.local_time = 1.;
        s.playback_speed = -1.;
        assert_eq!(
            r.future_span(&s, 10., 1.)?,
            Span {
                from: 1.,
                to: 0.,
                loops: 0,
                at_end: true
            }
        );
        Ok(())
    }
    #[test]
    fn controlled_fraction_and_enforced_duration_preserve_control_state() -> Result<()> {
        let mut s = settings();
        s.mode = 2;
        s.user_controlled_time_fraction = 0.75;
        let r = Runtime {
            local_time: 7.,
            crop_start: 1.,
            crop_end: 2.,
            previous_user_fraction: 0.25,
            ..Default::default()
        };
        assert_eq!(
            r.future_span(&s, 10., 100.)?,
            Span {
                from: 2.75,
                to: 6.25,
                loops: 0,
                at_end: false
            }
        );
        s.mode = 1;
        s.enforced_duration = 14.;
        assert_eq!(
            r.future_span(&s, 10., 1.)?,
            Span {
                from: 7.,
                to: 7.5,
                loops: 0,
                at_end: false
            }
        );
        assert_eq!(r.local_time, 7.);
        assert!(r.future_span(&s, 10., f32::INFINITY).is_err());
        assert!(control_future(0., 0., 0., 1., f32::MAX, 1.).is_err());
        Ok(())
    }
    #[test]
    fn reset_direction_endpoints_enforced_duration_and_retained_control() -> Result<()> {
        let mut s = settings();
        let mut r = Runtime::default();
        r.reset(&s, 10., false, &[])?;
        assert_eq!(r.local_time, 1.);
        s.playback_speed = -1.;
        r.reset(&s, 10., false, &[])?;
        assert_eq!(r.local_time, 8.);
        s.playback_speed = 1.;
        s.start_time = 7.;
        r.reset(&s, 10., false, &[])?;
        assert_eq!(r.local_time, 1.);
        s.start_time = 2.;
        s.enforced_duration = 4.;
        r.reset(&s, 10., false, &[])?;
        assert_eq!(r.local_time, 4.5);
        s.start_time = -2.;
        s.enforced_duration = 0.;
        r.reset(&s, 10., false, &[])?;
        assert_eq!(r.local_time, -1.);
        r.local_time = 6.;
        r.at_end = true;
        r.ping_pong_backward = true;
        r.time = 55.;
        s.crop_start_amount_local_time = 9.;
        r.reset(&s, 10., true, &[])?;
        assert_eq!((r.local_time, r.crop_start, r.crop_end), (6., 1., 2.));
        assert!(!r.at_end && !r.ping_pong_backward);
        assert_eq!((r.time, r.previous_user_fraction), (0., 0.3));
        Ok(())
    }
    #[test]
    fn trigger_order_preserves_ties_payloads_and_acyclic_semantics() -> Result<()> {
        let trigger = |time, relative, acyclic| Trigger {
            local_time: time,
            event: Event {
                id: 17,
                payload: Some(Reference {
                    id: "payload".into(),
                }),
            },
            relative_to_end_of_clip: relative,
            acyclic,
            is_annotation: false,
        };
        let triggers = vec![
            trigger(99., false, false),
            trigger(-1., true, false),
            trigger(-2., true, true),
            trigger(-3., false, false),
            trigger(0., false, false),
        ];
        let mut s = settings();
        s.enforced_duration = 4.;
        let order = Runtime::default().reset(&s, 10., false, &triggers)?;
        assert_eq!(order, vec![2, 3, 4, 1, 0]);
        assert_eq!(
            triggers[order[0]].event.payload.as_ref().unwrap().id,
            "payload"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn all_mickey_clips_compile_and_playback_bindings_use_current_words() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let state = super::super::behavior_state::State::new(&graph)?;
        assert_eq!(state.clips.len(), 114);
        assert_eq!(
            state
                .clips
                .values()
                .filter(|c| c.playback_variable.is_some())
                .count(),
            13
        );
        let mut words = state.words().to_vec();
        for clip in state.clips.values() {
            if let Some(i) = clip.playback_variable {
                words[i] = (-2.5f32).to_bits();
            }
            let resolved = clip.resolve(&words)?;
            if clip.playback_variable.is_some() {
                assert_eq!(resolved.playback_speed, -2.5);
            }
        }
        Ok(())
    }
}
