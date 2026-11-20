//! Authored hose asset playback. This is an explicitly provisional visual
//! preview: gameplay emission, force integration and Wii indirect rendering
//! still need reference validation. Never mutates imported resources.
use crate::render_support::{material_preview, nif, viewport3d::GpuDrawCommand};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{
    nif_file::{NiObject, NifFile},
    packfile::VirtualFile,
};
use std::{collections::HashMap, sync::Arc};
#[path = "particle_fields.rs"]
mod fields;
#[cfg(any(test, debug_assertions))]
pub fn validate_radial_fields() -> Result<()> {
    fields::validate_radial_instructions()
}
#[path = "particle_nonuniform.rs"]
mod nonuniform;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    little: bool,
}
impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let end = self.at.checked_add(N).context("Particle offset overflow")?;
        let out = self
            .bytes
            .get(self.at..end)
            .with_context(|| {
                format!(
                    "Truncated particle block: read {N} bytes at {} of {}",
                    self.at,
                    self.bytes.len()
                )
            })?
            .try_into()?;
        self.at = end;
        Ok(out)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }
    fn short(&mut self) -> Result<u16> {
        let b = self.take()?;
        Ok(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }
    fn uint(&mut self) -> Result<u32> {
        let b = self.take()?;
        Ok(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
    fn float(&mut self) -> Result<f32> {
        let f = f32::from_bits(self.uint()?);
        ensure!(f.is_finite(), "Nonfinite particle field");
        Ok(f)
    }
    fn done(&self) -> Result<()> {
        ensure!(
            self.at == self.bytes.len(),
            "Unconsumed particle bytes: {}",
            self.bytes.len() - self.at
        );
        Ok(())
    }
}
fn raw<'a>(nif: &'a NifFile, index: u32, kind: &str) -> Result<Reader<'a>> {
    let Some(NiObject::Unknown(b)) = nif.blocks.get(index as usize) else {
        anyhow::bail!("Missing {kind} at {index}")
    };
    ensure!(b.type_name == kind, "Expected {kind}, got {}", b.type_name);
    Ok(Reader {
        bytes: &b.raw_data,
        at: 0,
        little: nif.source_is_little_endian,
    })
}

/// Serialized NiPSEmitParticlesCtlr inheritance, checked against the supplied
/// Gamebryo sources. Scheduling and animated interpolators remain separate.
#[derive(Clone, Debug)]
pub struct EmissionController {
    pub next: u32,
    pub flags: u16,
    pub frequency: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
    pub target: u32,
    pub rate: u32,
    pub emitter_name: u32,
    pub active: u32,
}
#[derive(Clone, Debug)]
pub enum ActivationTrack {
    Constant(bool),
    Step(Vec<(f32, bool)>),
}
#[derive(Clone, Debug)]
pub struct RateKey {
    pub time: f32,
    pub value: f32,
    pub incoming: f32,
    pub outgoing: f32,
}
#[derive(Clone, Debug)]
pub enum RateTrack {
    Constant(f32),
    Keys { kind: u32, keys: Vec<RateKey> },
}
impl RateTrack {
    pub fn load(nif: &NifFile, index: u32) -> Result<Self> {
        let mut r = raw(nif, index, "NiFloatInterpolator")?;
        let cached = r.float()?;
        let data = r.uint()?;
        r.done()?;
        if data == u32::MAX {
            ensure!(cached != -f32::MAX, "Invalid posed particle rate");
            return Ok(Self::Constant(cached));
        }
        let mut r = raw(nif, data, "NiFloatData")?;
        let count = r.uint()? as usize;
        if count == 0 {
            r.done()?;
            ensure!(
                cached != -f32::MAX,
                "Empty particle rate track has no value"
            );
            return Ok(Self::Constant(cached));
        }
        let kind = r.uint()?;
        ensure!(
            matches!(kind, 1 | 2 | 5),
            "Unsupported particle rate interpolation {kind}"
        );
        let stride = if kind == 2 { 16 } else { 8 };
        ensure!(
            count <= r.bytes.len().saturating_sub(r.at) / stride,
            "Truncated particle rate keys"
        );
        let mut keys: Vec<RateKey> = Vec::with_capacity(count);
        for _ in 0..count {
            let time = r.float()?;
            let value = r.float()?;
            let (incoming, outgoing) = if kind == 2 {
                (r.float()?, r.float()?)
            } else {
                (0., 0.)
            };
            ensure!(
                keys.last().is_none_or(|last| time > last.time),
                "Unordered particle rate keys"
            );
            keys.push(RateKey {
                time,
                value,
                incoming,
                outgoing,
            });
        }
        r.done()?;
        Ok(Self::Keys { kind, keys })
    }
    pub fn sample(&self, time: f32) -> Result<f32> {
        ensure!(time.is_finite(), "Invalid particle rate time");
        match self {
            Self::Constant(value) => Ok(*value),
            Self::Keys { kind, keys } => {
                ensure!(!keys.is_empty(), "Empty particle rate keys");
                if keys.len() == 1 {
                    return Ok(keys[0].value);
                }
                let next = 1 + keys[1..].partition_point(|key| key.time < time);
                ensure!(next < keys.len(), "Particle rate time exceeds key range");
                let a = &keys[next - 1];
                let b = &keys[next];
                let t = (time - a.time) / (b.time - a.time);
                Ok(match kind {
                    1 => a.value * (1. - t) + b.value * t,
                    2 => {
                        let delta = b.value - a.value;
                        let quadratic = 3. * delta - (b.incoming + 2. * a.outgoing);
                        let cubic = a.outgoing + b.incoming - 2. * delta;
                        a.value + (a.outgoing + (quadratic + cubic * t) * t) * t
                    }
                    5 => {
                        if t < 1. {
                            a.value
                        } else {
                            b.value
                        }
                    }
                    _ => anyhow::bail!("Unsupported particle rate interpolation {kind}"),
                })
            }
        }
    }
}
#[derive(Debug, PartialEq)]
pub struct ActiveSpan {
    pub from: f32,
    pub to: f32,
    pub start: f32,
    pub stop: f32,
    pub rate_sample_time: f32,
}
/// Ages supplied to the emitter, oldest first. Generated lazily rather than
/// allocating a temporary array or imposing an arbitrary particle-count cap.
pub struct BirthAges {
    current_delta: f32,
    interval: f32,
    last: u32,
    current: u32,
}
/// Emission update history, independent of rendering. Raw time identifies a
/// reset; decreasing scaled time while raw time advances identifies a loop.
#[derive(Clone, Default)]
pub struct EmissionState {
    last_raw: Option<f32>,
    last_scaled: Option<f32>,
    last_rate: Option<u64>,
    last_active: bool,
}
/// Particle-system clock is separate from controller-scaled animation time.
/// Bounds are the controller's complete key range, not just activation keys.
pub struct EmissionClock {
    pub system_last: Option<f32>,
    pub key_start: f32,
    pub key_stop: f32,
}
/// Direct (non-manager) controller playback. Target/emitter binding is supplied
/// by the owning particle system; sequence-manager dispatch remains separate.
#[derive(Clone)]
pub struct EmissionPlayback {
    block: u32,
    application_start: Option<f32>,
    pub controller: EmissionController,
    active: ActivationTrack,
    rate: RateTrack,
    time: ControllerTime,
    state: EmissionState,
    scaled: Option<f32>,
}
struct SimulatorPlan {
    submit: Vec<u16>,
    complete: Vec<u16>,
    steps: Vec<u32>,
}
impl SimulatorPlan {
    fn load(nif: &NifFile, index: u32) -> Result<Self> {
        let mut r = raw(nif, index, "NiPSSimulator")?;
        let mut points = || -> Result<Vec<u16>> {
            let count = r.uint()? as usize;
            ensure!(
                count <= r.bytes.len().saturating_sub(r.at) / 2,
                "Truncated simulator sync points"
            );
            (0..count).map(|_| r.short()).collect()
        };
        let submit = points()?;
        let complete = points()?;
        let count = r.uint()? as usize;
        ensure!(
            count <= r.bytes.len().saturating_sub(r.at) / 4,
            "Truncated simulator steps"
        );
        let mut steps = Vec::with_capacity(count);
        for _ in 0..count {
            let link = r.uint()?;
            ensure!(
                link == u32::MAX
                    || nif
                        .blocks
                        .get(link as usize)
                        .is_some_and(|b| b.type_name().starts_with("NiPSSimulator")
                            || b.type_name() == "NonUniformSimulatorGeneralStep"),
                "Invalid simulator step {link}: {:?}",
                nif.blocks.get(link as usize).map(|b| b.type_name())
            );
            steps.push(link);
        }
        r.done()?;
        Ok(Self {
            submit,
            complete,
            steps,
        })
    }
}
impl EmissionPlayback {
    /// NiTimeController lists can start with a field/property controller.
    /// Bind the named emitter on this system, rather than treating the list
    /// head as an emission controller. Preserve the full list for the other
    /// controller owners; this method does not evaluate their properties.
    pub fn for_emitter(
        nif: &NifFile,
        system: u32,
        head: Option<u32>,
        emitter: &str,
    ) -> Result<(Self, Vec<super::scene_bounds::Controller>)> {
        let chain = super::scene_bounds::controllers(nif, head)?;
        let mut selected = None;
        for controller in &chain {
            if controller.class_name != "NiPSEmitParticlesCtlr" {
                continue;
            }
            let decoded = EmissionController::load(nif, controller.block)?;
            let name = nif
                .string_table
                .get(decoded.emitter_name as usize)
                .context("Missing emission-controller emitter name")?;
            if decoded.target != system || name != emitter {
                continue;
            }
            ensure!(selected.is_none(),"Multiple emission controllers for system {system}, emitter {emitter} require ordered multi-controller playback");
            selected = Some(Self::load(nif, controller.block)?);
        }
        Ok((
            selected.with_context(|| {
                format!("No emission controller targets system {system}, emitter {emitter}")
            })?,
            chain,
        ))
    }
    pub fn load(nif: &NifFile, index: u32) -> Result<Self> {
        let mut controller = EmissionController::load(nif, index)?;
        // NiTimeController::LoadBinary restores transient flags.
        controller.flags = (controller.flags | 0x40) & !0x80;
        let active = ActivationTrack::load(nif, controller.active)?;
        let rate = RateTrack::load(nif, controller.rate)?;
        Ok(Self {
            block: index,
            application_start: None,
            controller,
            active,
            rate,
            time: ControllerTime::default(),
            state: EmissionState::default(),
            scaled: None,
        })
    }
    pub fn stop(&mut self) {
        self.controller.flags &= !8;
        self.time.reset();
        self.state.reset();
    }
    pub fn start(&mut self) {
        self.controller.flags |= 8;
        self.time.reset();
    }
    ///804bf1bc /804d2ac4. Keep the base scaled-value cache (+28);
    /// stopping clears emission history (+48), not that different cache.
    fn native_command(&mut self, time: Option<f32>) {
        if let Some(time) = time {
            self.start();
            if self.controller.flags & 1 != 0 {
                self.application_start = Some(time);
            }
        } else {
            self.stop();
            if self.controller.flags & 1 != 0 {
                self.application_start = None;
            }
        }
    }
    pub fn update(
        &mut self,
        time: f32,
        bound: bool,
        system_last: Option<f32>,
        emit: impl FnMut(f32, ActiveSpan, BirthAges) -> Result<()>,
    ) -> Result<()> {
        ensure!(time.is_finite(), "Invalid effect update time");
        if self.time.last.is_some_and(|last| last > time) {
            self.state.reset();
        }
        let flags = self.controller.flags;
        if !bound || flags & 0x20 != 0 || flags & 8 == 0 {
            return Ok(());
        }
        let force = flags & 0x80 != 0;
        if self.time.last == Some(time) && !force {
            return Ok(());
        }
        ensure!(
            flags & 0x40 != 0,
            "Direct effect playback requires controller time scaling"
        );
        let scaled = self.time.scale(time, &self.controller)?;
        if self.scaled == Some(scaled) && !force {
            return Ok(());
        }
        self.scaled = Some(scaled);
        self.controller.flags &= !0x80;
        let clock = EmissionClock {
            system_last,
            key_start: self.controller.start,
            key_stop: self.controller.stop,
        };
        self.state.update_timed(
            time,
            scaled,
            false,
            self.controller.rate as u64,
            &self.active,
            &self.rate,
            &clock,
            emit,
        )
    }
}
impl EmissionState {
    pub fn reset(&mut self) {
        self.last_scaled = None;
    }
    pub fn update_timed(
        &mut self,
        raw_time: f32,
        scaled_time: f32,
        managed: bool,
        rate_id: u64,
        active: &ActivationTrack,
        rate: &RateTrack,
        clock: &EmissionClock,
        mut emit: impl FnMut(f32, ActiveSpan, BirthAges) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            clock.key_start.is_finite()
                && clock.key_stop.is_finite()
                && clock.key_start <= clock.key_stop
                && clock.system_last.is_none_or(f32::is_finite),
            "Invalid particle-system clock"
        );
        let mut previous = self.last_scaled.unwrap_or(scaled_time);
        let mut loop_delta = 0.;
        self.update(
            raw_time,
            scaled_time,
            managed,
            rate_id,
            active,
            rate,
            |span, ages| {
                let time = match active {
                    ActivationTrack::Constant(_) => {
                        let system = clock.system_last.context(
                            "Posed emission requires an initialized particle-system clock",
                        )?;
                        match rate {
                            RateTrack::Constant(_) => {
                                system
                                    + (scaled_time
                                        - if previous > scaled_time { 0. } else { previous })
                            }
                            RateTrack::Keys { .. } => {
                                let delta = span.to - span.from;
                                // First batch of a wrap is the tail; its duration
                                // is carried into the following head timestamp.
                                let time = system + delta + loop_delta;
                                loop_delta += delta;
                                time
                            }
                        }
                    }
                    ActivationTrack::Step(_) => {
                        let mut system = clock.system_last.unwrap_or(scaled_time);
                        if previous > scaled_time {
                            system += clock.key_stop - clock.key_start;
                        }
                        let time = system + scaled_time - previous;
                        previous = span.to;
                        time
                    }
                };
                ensure!(time.is_finite(), "Particle emission timestamp overflow");
                emit(time, span, ages)
            },
        )
    }
    pub fn update(
        &mut self,
        raw_time: f32,
        scaled_time: f32,
        managed: bool,
        rate_id: u64,
        active: &ActivationTrack,
        rate: &RateTrack,
        mut emit: impl FnMut(ActiveSpan, BirthAges) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            raw_time.is_finite() && scaled_time.is_finite(),
            "Invalid particle scheduler time"
        );
        if self.last_raw.is_some_and(|last| raw_time < last) {
            self.reset();
        }
        self.last_raw = Some(raw_time);
        if managed && self.last_rate != Some(rate_id) {
            self.last_scaled = None;
        }
        self.last_rate = Some(rate_id);
        let Some(mut from) = self.last_scaled else {
            self.last_active = active.sample(scaled_time)?;
            self.last_scaled = Some(scaled_time);
            return Ok(());
        };
        // Posed inactive interpolation returns without advancing scaled time.
        if matches!(active, ActivationTrack::Constant(false)) {
            return Ok(());
        }
        let previous_active = self.last_active;
        self.last_active = active.sample(scaled_time)?;
        if from > scaled_time {
            if matches!(active, ActivationTrack::Step(_)) {
                self.last_scaled = Some(scaled_time);
                return active.loop_batches(rate, from, scaled_time, previous_active, emit);
            }
            if let (ActivationTrack::Constant(true), RateTrack::Keys { keys, .. }) = (active, rate)
            {
                let begin = keys.first().context("Empty particle rate keys")?.time;
                let end = keys.last().unwrap().time;
                ensure!(
                    begin <= scaled_time && from <= end,
                    "Particle loop times outside rate track"
                );
                self.last_scaled = Some(scaled_time);
                active.emission_batches(rate, from, end, &mut emit)?;
                return active.emission_batches(rate, begin, scaled_time, emit);
            }
            // NiPSEmitParticlesCtlr's both-posed branch deliberately discards
            // the loop tail and starts this emission interval at zero.
            ensure!(
                matches!(active, ActivationTrack::Constant(true))
                    && matches!(rate, RateTrack::Constant(_)),
                "Keyed particle loop scheduling is not implemented"
            );
            from = 0.;
        }
        self.last_scaled = Some(scaled_time);
        active.emission_batches(rate, from, scaled_time, emit)
    }
}
/// Stateful NiTimeController scaling. Frequency changes apply only to elapsed
/// time since the previous update, rather than retiming the entire animation.
#[derive(Clone, Default)]
pub struct ControllerTime {
    last: Option<f32>,
    weighted: f32,
}
impl ControllerTime {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn last_time(&self) -> Option<f32> {
        self.last
    }
    pub fn scale(&mut self, time: f32, controller: &EmissionController) -> Result<f32> {
        let c = controller;
        self.scale_parameters(time, c.flags, c.frequency, c.phase, c.start, c.stop)
    }
    /// Shared NiTimeController prefix, independent of the animated value type.
    pub fn scale_parameters(
        &mut self,
        time: f32,
        flags: u16,
        frequency: f32,
        phase: f32,
        start: f32,
        stop: f32,
    ) -> Result<f32> {
        ensure!(
            [time, frequency, phase, start, stop]
                .into_iter()
                .all(f32::is_finite)
                && start <= stop,
            "Invalid controller time parameters"
        );
        let cycle = (flags >> 1) & 3;
        ensure!(cycle < 3, "Unknown controller cycle mode");
        let delta = self
            .last
            .map_or(if flags & 1 != 0 { 0. } else { time }, |last| time - last);
        //804bf2ac is fmadds: retain the product until the single fused result.
        let weighted = delta.mul_add(frequency, self.weighted);
        let mut scaled = weighted + phase;
        let span = stop - start;
        ensure!(
            weighted.is_finite() && scaled.is_finite() && span.is_finite(),
            "Controller time overflow"
        );
        match cycle {
            0 => {
                scaled = if span == 0. {
                    start
                } else {
                    (scaled - start) % span + start
                };
                if scaled < start {
                    scaled += span;
                }
            }
            1 => {
                if span == 0. {
                    scaled = start;
                } else {
                    let double = 2. * span;
                    ensure!(double.is_finite(), "Controller reverse period overflow");
                    // Source reverse mode uses absolute scaled time here,
                    // unlike loop mode's subtraction of the lower key time.
                    let mut fraction = scaled % double;
                    if fraction < 0. {
                        fraction += double;
                    }
                    scaled = if fraction > span {
                        double - fraction + start
                    } else {
                        fraction + start
                    };
                }
            }
            _ => {}
        }
        scaled = scaled.clamp(start, stop);
        if flags & 0x10 != 0 {
            scaled = stop - (scaled - start);
        }
        ensure!(scaled.is_finite(), "Invalid scaled controller time");
        self.last = Some(time);
        self.weighted = weighted;
        Ok(scaled)
    }
}
#[cfg(any(test, debug_assertions))]
pub fn validate_controller_time() -> Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/controller_weighted_time.json"
    ))?;
    let cases = fixture["cases"].as_array().unwrap();
    ensure!(
        cases.len() == 1296,
        "Controller-time source coverage changed"
    );
    for case in cases {
        let scalar = |key: &str| case[key].as_f64().unwrap() as f32;
        let mut clock = ControllerTime {
            last: case["last"].as_f64().map(|v| v as f32),
            weighted: scalar("weighted"),
        };
        let scaled = clock.scale_parameters(
            scalar("current"),
            case["flags"].as_u64().unwrap() as u16,
            scalar("frequency"),
            scalar("phase"),
            -10000.,
            10000.,
        )?;
        ensure!(
            u64::from(clock.weighted.to_bits()) == case["weighted_bits"].as_u64().unwrap()
                && u64::from(scaled.to_bits()) == case["scaled_bits"].as_u64().unwrap(),
            "Controller-time native recurrence differs: {case}"
        );
    }
    println!("PASS: 1296 original controller-time recurrence cases, including APP_INIT and frequency/phase arithmetic");
    Ok(())
}
impl Iterator for BirthAges {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.last == self.current {
            return None;
        }
        self.last += 1;
        Some(self.current_delta - self.last as f32 * self.interval)
    }
}
/// NiPSEmitParticlesCtlr::EmitParticles supporting-source arithmetic. Counts
/// are anchored to the authored emission start, not an accumulated frame credit.
/// World/particle-system time conversion is handled by the caller.
pub fn birth_ages(current: f32, last: f32, start: f32, stop: f32, rate: f32) -> Result<BirthAges> {
    ensure!(
        [current, last, start, stop, rate]
            .into_iter()
            .all(f32::is_finite),
        "Invalid particle birth inputs"
    );
    ensure!(last <= current, "Backwards particle birth interval");
    if !(rate > 0. && start < stop && current > start && last < stop) {
        return Ok(BirthAges {
            current_delta: 0.,
            interval: 0.,
            last: 0,
            current: 0,
        });
    }
    let current_delta = current.min(stop) - start;
    let last_delta = if last >= start { last - start } else { 0. };
    let current_count = rate * current_delta;
    let last_count = rate * last_delta;
    ensure!(
        current_count.is_finite()
            && current_count < 4294967296.
            && last_count >= 0.
            && last_count <= current_count,
        "Particle birth count exceeds native u32 range"
    );
    Ok(BirthAges {
        current_delta,
        interval: 1. / rate,
        last: last_count as u32,
        current: current_count as u32,
    })
}
impl ActivationTrack {
    /// Animated-activation loop branch from the supplied Gamebryo controller.
    /// Its stop-key continuation condition is deliberately retained: a tail
    /// ending beyond the current scaled time does not also emit the head.
    /// This is supporting-source behavior, pending retail EM1 comparison.
    fn loop_batches(
        &self,
        rate: &RateTrack,
        mut from: f32,
        to: f32,
        mut was_active: bool,
        mut emit: impl FnMut(ActiveSpan, BirthAges) -> Result<()>,
    ) -> Result<()> {
        let Self::Step(keys) = self else {
            anyhow::bail!("Expected keyed activation");
        };
        let begin = keys.first().context("Empty particle activation keys")?.0;
        let end = keys.last().unwrap().0;
        ensure!(
            begin <= to && to < from && from <= end,
            "Particle loop times outside activation track"
        );
        loop {
            let wrapped = from > to;
            let start_index = if was_active {
                Some(
                    keys.iter()
                        .rposition(|&(t, on)| t <= from && !on)
                        .map_or(0, |i| i + 1),
                )
            } else {
                keys.iter()
                    .position(|&(t, on)| on && t > from && (wrapped || t <= to))
                    .or_else(|| {
                        if wrapped {
                            keys.iter().position(|&(t, on)| on && t < to)
                        } else {
                            None
                        }
                    })
            };
            let Some(index) = start_index else {
                return Ok(());
            };
            let start = keys
                .get(index)
                .context("Active history has no activation start key")?
                .0;
            let stop = keys[index..]
                .iter()
                .find(|&&(_, on)| !on)
                .map_or(end, |&(t, _)| t);
            let span = if wrapped {
                if stop > from {
                    ActiveSpan {
                        from,
                        to: end,
                        start,
                        stop,
                        rate_sample_time: start + (stop - start) * 0.5,
                    }
                } else {
                    let stop = stop.min(to);
                    ActiveSpan {
                        from: begin,
                        to,
                        start,
                        stop,
                        rate_sample_time: start + (stop - start) * 0.5,
                    }
                }
            } else {
                ActiveSpan {
                    from,
                    to: to.min(stop),
                    start,
                    stop,
                    rate_sample_time: if stop < to {
                        start + (stop - start) * 0.5
                    } else {
                        to
                    },
                }
            };
            let next = span.to;
            let ages = birth_ages(
                span.to,
                span.from,
                span.start,
                span.stop,
                rate.sample(span.rate_sample_time)?,
            )?;
            emit(span, ages)?;
            if stop >= to {
                return Ok(());
            }
            // A wrapped head may jump directly to `to`, as in the source.
            // The next search then terminates without replaying earlier keys.
            ensure!(next != from, "Particle activation walk did not advance");
            from = next;
            was_active = self.sample(from)?;
        }
    }
    /// Compose a forward interval into rate samples and birth-age batches.
    /// The controller must supply scaled times and suppress its first update.
    /// Looping/system-time mapping is deliberately not inferred here.
    pub fn emission_batches(
        &self,
        rate: &RateTrack,
        from: f32,
        to: f32,
        mut emit: impl FnMut(ActiveSpan, BirthAges) -> Result<()>,
    ) -> Result<()> {
        self.active_spans(from, to, |mut span| {
            if matches!(self, Self::Constant(_)) {
                span.start = match rate {
                    RateTrack::Constant(_) => 0.,
                    RateTrack::Keys { keys, .. } => {
                        keys.first().context("Empty particle rate keys")?.time
                    }
                };
            }
            let value = rate.sample(span.rate_sample_time)?;
            let ages = birth_ages(span.to, span.from, span.start, span.stop, value)?;
            emit(span, ages)
        })
    }
    /// Forward, non-wrapping part of NiPSEmitParticlesCtlr's key walk.
    /// Preserves complete active runs (including repeated true keys). Clock
    /// reset, looping and the first-update suppression belong to the scheduler.
    pub fn active_spans(
        &self,
        from: f32,
        to: f32,
        mut emit: impl FnMut(ActiveSpan) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            from.is_finite() && to.is_finite() && from <= to,
            "Invalid emission time interval"
        );
        match self {
            Self::Constant(active) => {
                if *active && from < to {
                    emit(ActiveSpan {
                        from,
                        to,
                        start: 0.,
                        stop: to,
                        rate_sample_time: from + (to - from) * 0.5,
                    })?;
                }
            }
            Self::Step(keys) => {
                ensure!(!keys.is_empty(), "Empty particle activation keys");
                ensure!(
                    to <= keys.last().unwrap().0,
                    "Emission interval exceeds activation keys"
                );
                let mut index = 0;
                while index < keys.len() {
                    if !keys[index].1 {
                        index += 1;
                        continue;
                    }
                    let start = keys[index].0;
                    index += 1;
                    while index < keys.len() && keys[index].1 {
                        index += 1;
                    }
                    let stop = keys[index.min(keys.len() - 1)].0;
                    let lo = from.max(start);
                    let hi = to.min(stop);
                    if lo < hi {
                        // The original samples the whole start/stop midpoint
                        // only when stop is strictly before the current time.
                        emit(ActiveSpan {
                            from: lo,
                            to: hi,
                            start,
                            stop,
                            rate_sample_time: if stop < to {
                                start + (stop - start) * 0.5
                            } else {
                                to
                            },
                        })?;
                    }
                    if stop >= to {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn load(nif: &NifFile, index: u32) -> Result<Self> {
        let mut r = raw(nif, index, "NiBoolInterpolator")?;
        let cached = r.byte()?;
        let data = r.uint()?;
        r.done()?;
        if data == u32::MAX {
            ensure!(cached <= 1, "Invalid posed particle activation");
            return Ok(Self::Constant(cached != 0));
        }
        let mut r = raw(nif, data, "NiBoolData")?;
        let count = r.uint()? as usize;
        if count == 0 {
            r.done()?;
            ensure!(
                cached <= 1,
                "Empty activation track has no valid cached value"
            );
            return Ok(Self::Constant(cached != 0));
        }
        ensure!(
            r.uint()? == 5,
            "Unsupported particle activation interpolation"
        );
        ensure!(
            count <= r.bytes.len().saturating_sub(r.at) / 5,
            "Truncated particle activation keys"
        );
        let mut keys = Vec::with_capacity(count);
        for _ in 0..count {
            let time = r.float()?;
            let value = r.byte()? != 0;
            ensure!(
                keys.last().is_none_or(|&(last, _)| time > last),
                "Unordered particle activation keys"
            );
            keys.push((time, value));
        }
        r.done()?;
        Ok(Self::Step(keys))
    }
    /// Sample already-scaled animation time. Controller looping and crossing
    /// every activation boundary during emission remain caller responsibilities.
    pub fn sample(&self, time: f32) -> Result<bool> {
        ensure!(time.is_finite(), "Invalid particle activation time");
        Ok(match self {
            Self::Constant(value) => *value,
            Self::Step(keys) => {
                ensure!(!keys.is_empty(), "Empty particle activation keys");
                if keys.len() == 1 {
                    keys[0].1
                } else {
                    // NiBoolKey::GenInterp expects controller-scaled time no
                    // later than the last key. Do not invent a looping policy.
                    let next = 1 + keys[1..].partition_point(|&(at, _)| at < time);
                    ensure!(
                        next < keys.len(),
                        "Particle activation time exceeds key range"
                    );
                    let fraction = (time - keys[next - 1].0) / (keys[next].0 - keys[next - 1].0);
                    if fraction < 1. {
                        keys[next - 1].1
                    } else {
                        keys[next].1
                    }
                }
            }
        })
    }
}
impl EmissionController {
    pub fn load(nif: &NifFile, index: u32) -> Result<Self> {
        let mut r = raw(nif, index, "NiPSEmitParticlesCtlr")?;
        let result = Self {
            next: r.uint()?,
            flags: r.short()?,
            frequency: r.float()?,
            phase: r.float()?,
            start: r.float()?,
            stop: r.float()?,
            target: r.uint()?,
            rate: r.uint()?,
            emitter_name: r.uint()?,
            active: r.uint()?,
        };
        r.done()?;
        ensure!(
            result.target == u32::MAX || (result.target as usize) < nif.blocks.len(),
            "Invalid emission target"
        );
        ensure!(
            result.emitter_name == u32::MAX
                || (result.emitter_name as usize) < nif.string_table.len(),
            "Invalid emitter name"
        );
        Ok(result)
    }
    pub fn constant_active(&self, nif: &NifFile) -> Result<bool> {
        let mut r = raw(nif, self.active, "NiBoolInterpolator")?;
        let value = r.byte()?;
        let data = r.uint()?;
        r.done()?;
        ensure!(
            data == u32::MAX,
            "Animated particle activation requires its key track"
        );
        ensure!(value <= 1, "Invalid posed particle activation");
        Ok(value != 0)
    }
}
#[derive(Clone, Debug)]
struct BoxVolume([f32; 3]);
impl BoxVolume {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self([r.float()?, r.float()?, r.float()?]))
    }
    /// Random samples are supplied by the caller so the original RNG can be
    /// integrated independently. Zero-width axes consume no random number.
    fn local_position(&self, mut unit: impl FnMut() -> f32) -> Result<[f32; 3]> {
        let mut local = [0.; 3];
        for (out, &extent) in local.iter_mut().zip(&self.0) {
            *out = if extent == 0. {
                extent
            } else {
                let random = unit();
                ensure!(
                    random.is_finite() && (0. ..=1.).contains(&random),
                    "Invalid particle random sample"
                );
                extent * (random - 0.5)
            };
        }
        Ok(local)
    }
    fn position(&self, transform: &[f32; 16], unit: impl FnMut() -> f32) -> Result<[f32; 3]> {
        let point = super::physics::point(transform, self.local_position(unit)?);
        ensure!(
            point.into_iter().all(f32::is_finite),
            "Invalid box-emitter transform"
        );
        Ok(point)
    }
}
#[derive(Clone, Debug)]
enum EmitterVolume {
    Box(BoxVolume),
    Cylinder {
        radius: f32,
        height: f32,
    },
    Sphere {
        radius: f32,
    },
    Torus {
        radius: f32,
        section_radius: f32,
    },
    Mesh {
        links: Vec<u32>,
        emission: u32,
        velocity: u32,
    },
}
impl EmitterVolume {
    fn local_position(&self, mut unit: impl FnMut() -> f32) -> Result<[f32; 3]> {
        if let Self::Box(volume) = self {
            return volume.local_position(unit);
        }
        let mut random = || -> Result<f32> {
            let value = unit();
            ensure!(
                value.is_finite() && (0. ..=1.).contains(&value),
                "Invalid particle random sample"
            );
            Ok(value)
        };
        let tau = std::f32::consts::TAU;
        let local = match *self {
            Self::Cylinder { radius, height } => {
                let (x, y) = if radius == 0. {
                    (0., 0.)
                } else {
                    let r = radius * random()?;
                    let (sin, cos) = (random()? * tau).sin_cos();
                    (r * cos, r * sin)
                };
                let z = if height == 0. {
                    height
                } else {
                    height * (random()? - 0.5)
                };
                [x, y, z]
            }
            Self::Sphere { radius } => {
                if radius == 0. {
                    [0.; 3]
                } else {
                    let r = radius * random()?;
                    let phi = random()? * tau;
                    let theta = random()? * tau;
                    let (sp, cp) = phi.sin_cos();
                    let (st, ct) = theta.sin_cos();
                    [r * st * cp, r * st * sp, r * ct]
                }
            }
            Self::Torus {
                radius,
                section_radius,
            } => super::particle_torus::position(radius, section_radius, &mut unit)?,
            _ => anyhow::bail!("Emitter position sampling is not implemented for {self:?}"),
        };
        Ok(local)
    }
    fn position(&self, transform: &[f32; 16], unit: impl FnMut() -> f32) -> Result<[f32; 3]> {
        let point = super::physics::point(transform, self.local_position(unit)?);
        ensure!(
            point.into_iter().all(f32::is_finite),
            "Invalid emitter position"
        );
        Ok(point)
    }
    fn position_native(
        &self,
        frame: &super::native_transform_record::Transform,
        unit: impl FnMut() -> f32,
    ) -> Result<[f32; 3]> {
        let point = super::particle_space::point(frame, self.local_position(unit)?);
        ensure!(
            point.into_iter().all(f32::is_finite),
            "Invalid native emitter position"
        );
        Ok(point)
    }
    fn read(r: &mut Reader, kind: &str) -> Result<Self> {
        Ok(match kind {
            "NiPSBoxEmitter" => Self::Box(BoxVolume::read(r)?),
            "NiPSCylinderEmitter" => Self::Cylinder {
                radius: r.float()?,
                height: r.float()?,
            },
            "NiPSSphereEmitter" => Self::Sphere { radius: r.float()? },
            "NiPSTorusEmitter" => Self::Torus {
                radius: r.float()?,
                section_radius: r.float()?,
            },
            _ => anyhow::bail!("Unsupported emitter shape {kind}"),
        })
    }
}
#[derive(Clone, Debug, Default)]
struct EmitterRotation {
    angle: f32,
    angle_var: f32,
    speed: f32,
    speed_var: f32,
    axis: [f32; 3],
    random_sign: bool,
    random_axis: bool,
}
#[derive(Debug, PartialEq)]
struct InitialRotation {
    angles: Option<(f32, f32)>,
    axis: Option<[f32; 3]>,
}
impl EmitterRotation {
    fn sample(
        &self,
        has_rotations: bool,
        has_axes: bool,
        mut unit: impl FnMut() -> f32,
    ) -> Result<InitialRotation> {
        let mut random = || -> Result<f32> {
            let value = unit();
            ensure!(
                value.is_finite() && (0. ..=1.).contains(&value),
                "Invalid rotation random sample"
            );
            Ok(value)
        };
        let angles = if has_rotations {
            let mut angle = self.angle;
            if self.angle_var != 0. {
                angle += self.angle_var * (2. * random()? - 1.);
            }
            let mut speed = self.speed;
            if self.speed_var != 0. {
                speed += self.speed_var * (2. * random()? - 1.);
            }
            if self.random_sign && random()? <= 0.5 {
                speed = -speed;
            }
            ensure!(
                angle.is_finite() && speed.is_finite(),
                "Invalid initial particle rotation"
            );
            Some((angle, speed))
        } else {
            None
        };
        let axis = if has_axes {
            let value = if self.random_axis {
                let phi = random()? * std::f32::consts::PI;
                let z = phi.cos();
                let radial = (1. - z * z).sqrt();
                let theta = random()? * std::f32::consts::TAU;
                [radial * theta.cos(), radial * theta.sin(), z]
            } else {
                self.axis
            };
            ensure!(
                value.into_iter().all(f32::is_finite),
                "Invalid initial particle rotation axis"
            );
            Some(value)
        } else {
            None
        };
        Ok(InitialRotation { angles, axis })
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            angle: r.float()?,
            angle_var: r.float()?,
            speed: r.float()?,
            speed_var: r.float()?,
            axis: [r.float()?, r.float()?, r.float()?],
            random_sign: r.byte()? != 0,
            random_axis: r.byte()? != 0,
        })
    }
    fn is_zero(&self) -> bool {
        self.angle == 0.
            && self.angle_var == 0.
            && self.speed == 0.
            && self.speed_var == 0.
            && self.axis == [0.; 3]
            && !self.random_sign
            && !self.random_axis
    }
}
#[derive(Clone, Debug)]
struct Emitter {
    speed: f32,
    speed_var: f32,
    speed_flip: f32,
    declination: f32,
    declination_var: f32,
    planar: f32,
    planar_var: f32,
    size: f32,
    size_var: f32,
    life: f32,
    life_var: f32,
    node: u32,
    volume: EmitterVolume,
    rotation: EmitterRotation,
}
#[derive(Debug)]
struct BirthKinematics {
    age: f32,
    life: f32,
    position: [f32; 3],
    velocity: [f32; 3],
    last_update: f32,
}
impl BirthKinematics {
    /// Final simulator step only: forces/collisions must have already run.
    /// Spawn handling retains the death timestamp for the spawner consumer.
    fn final_step(&mut self, time: f32, mut flags: u32, death_spawner: Option<u16>) -> Result<u32> {
        ensure!(time.is_finite(), "Invalid particle simulation time");
        let delta = time - self.last_update;
        let position = std::array::from_fn(|i| self.position[i] + self.velocity[i] * delta);
        let age = self.age + delta;
        let mut last = self.last_update;
        if age > self.life {
            flags |= 0x01000000;
            if let Some(spawner) = death_spawner {
                last = time - (age - self.life);
                flags =
                    (flags & !0x00fff000) | ((u32::from(spawner) << 12) & 0x00fff000) | 0x02000000;
            }
        }
        if flags & 0x02000000 == 0 {
            last = time;
        }
        ensure!(
            position.into_iter().all(f32::is_finite) && age.is_finite() && last.is_finite(),
            "Particle final-step overflow"
        );
        self.position = position;
        self.age = age;
        self.last_update = last;
        Ok(flags)
    }
}
#[derive(Debug)]
struct InitialParticle {
    motion: BirthKinematics,
    radius: f32,
    size: f32,
    rotation: InitialRotation,
}
#[derive(Clone)]
struct SizeEnvelope {
    grow: f32,
    shrink: f32,
    grow_generation: u16,
    shrink_generation: u16,
}
impl SizeEnvelope {
    fn initialize(&self, particle: &mut InitialParticle, generation: u16) -> Result<()> {
        let age = particle.motion.age;
        let life = particle.motion.life;
        ensure!(
            [self.grow, self.shrink, age, life]
                .into_iter()
                .all(f32::is_finite),
            "Invalid particle size envelope"
        );
        let grow = if generation == self.grow_generation && age < self.grow && self.grow > 0. {
            age / self.grow
        } else {
            1.
        };
        let left = life - age;
        let shrink =
            if generation == self.shrink_generation && left < self.shrink && self.shrink > 0. {
                left / self.shrink
            } else {
                1.
            };
        particle.size = grow.min(shrink).max(0.0001);
        Ok(())
    }
}
/// New particles are staged separately while the simulator owns active data.
/// Capacity comes from the authored particle system, not a preview limit.
struct PendingBirths {
    capacity: usize,
    active: usize,
    particles: Vec<InitialParticle>,
}
impl PendingBirths {
    /// Resolve after simulator completion (and before a new submission).
    /// When active particles were removed, native packing fills the gap from
    /// the final pending particle first, rather than preserving birth order.
    fn resolve_into(&mut self, active: &mut Vec<InitialParticle>) -> Result<()> {
        ensure!(
            active.len() <= self.active
                && self.active <= self.capacity
                && self.particles.len() <= self.capacity - self.active,
            "Invalid pending particle range"
        );
        let holes = self.active - active.len();
        let fill = holes.min(self.particles.len());
        for _ in 0..fill {
            active.push(self.particles.pop().unwrap());
        }
        active.append(&mut self.particles);
        self.active = active.len();
        Ok(())
    }
    fn emit(
        &mut self,
        emitter: &Emitter,
        time: f32,
        ages: impl IntoIterator<Item = f32>,
        transform: &[f32; 16],
        rotation: &[f32; 9],
        has_rotations: bool,
        has_axes: bool,
        mut unit: impl FnMut() -> f32,
    ) -> Result<()> {
        ensure!(
            self.active <= self.capacity && self.particles.len() <= self.capacity - self.active,
            "Invalid particle allocation state"
        );
        for age in ages {
            let Some(motion) =
                emitter.birth_kinematics(time, age, transform, rotation, &mut unit)?
            else {
                continue;
            };
            // Native AddParticle is attempted after initial kinematics. A full
            // system ends this emission batch, preserving RNG consumption.
            if self.particles.len() == self.capacity - self.active {
                return Ok(());
            }
            let mut radius = emitter.size;
            if emitter.size_var != 0. {
                let value = unit();
                ensure!(
                    value.is_finite() && (0. ..=1.).contains(&value),
                    "Invalid radius random sample"
                );
                radius += emitter.size_var * (2. * value - 1.);
            }
            ensure!(radius.is_finite(), "Invalid particle radius");
            let rotation = emitter
                .rotation
                .sample(has_rotations, has_axes, &mut unit)?;
            self.particles.push(InitialParticle {
                motion,
                radius,
                size: 1.,
                rotation,
            });
        }
        Ok(())
    }
}
impl Emitter {
    ///804d38dc /804f42cc draw unconditionally, including zero variance.
    /// Their fmadds is one rounded operation, not separate multiply/add.
    fn varied(base: f32, variance: f32, unit: f32) -> Result<f32> {
        ensure!(
            unit.is_finite() && (0. ..=1.).contains(&unit),
            "Invalid particle random sample"
        );
        Ok(variance.mul_add(unit - 0.5, base))
    }
    fn volume_direction(speed: f32, declination: [f32; 2], planar: [f32; 2]) -> [f32; 3] {
        //804f4368..4398, after the two sin/cos calls return stored floats.
        [
            declination[1] * planar[0],
            -declination[0],
            declination[1] * planar[1],
        ]
        .map(|v| speed * v)
    }
    fn mesh_birth(
        &self,
        time: f32,
        age: f32,
        surfaces: &[super::particle_mesh::Surface],
        parent: &[f32; 16],
        mut random: impl FnMut() -> u32,
    ) -> Result<Option<BirthKinematics>> {
        ensure!(
            time.is_finite() && age.is_finite(),
            "Invalid particle birth time"
        );
        let life = Self::varied(
            self.life,
            self.life_var,
            super::particle_mesh::unit(random())?,
        )?;
        ensure!(life.is_finite(), "Invalid mesh particle lifespan");
        if age > life {
            return Ok(None);
        }
        let which = if surfaces.len() == 1 {
            0
        } else {
            super::particle_mesh::choose(surfaces.len(), &mut random)?
        };
        let surface = &surfaces[which];
        // Mesh dispatch always samples speed, even when its variance is zero.
        let speed = self
            .speed_var
            .mul_add(super::particle_mesh::unit(random())? - 0.5, self.speed);
        let emission = match &self.volume {
            EmitterVolume::Mesh { emission, .. } if *emission == 1 => 1,
            _ => 3,
        };
        let (position, velocity) = surface.sample(
            [self.declination_var, self.planar_var],
            speed,
            emission,
            &mut random,
        )?;
        let transform = super::skin::matrix_product(parent, &surface.transform);
        let scale = (0..3)
            .map(|i| transform[i] * transform[i])
            .sum::<f32>()
            .sqrt();
        ensure!(
            scale.is_finite() && scale > 0.,
            "Singular mesh-emitter transform"
        );
        let rotation = std::array::from_fn(|i| transform[(i % 3) * 4 + i / 3] / scale);
        let position = super::native_matrix::transform(&rotation, position).map(|v| v * scale);
        let position = std::array::from_fn(|i| position[i] + transform[12 + i]);
        let velocity = self
            .flip_velocity(super::native_matrix::transform(&rotation, velocity), || {
                random() as f32 / 32767.
            })?;
        Ok(Some(BirthKinematics {
            age,
            life,
            position,
            velocity,
            last_update: time - age,
        }))
    }
    /// Initial kinematics only; allocation, size/rotation channels and simulator
    /// catch-up follow this step. Rotation is supplied separately from the full
    /// transform because the source does not scale velocity with volume size.
    fn birth_kinematics(
        &self,
        time: f32,
        age: f32,
        transform: &[f32; 16],
        rotation: &[f32; 9],
        unit: impl FnMut() -> f32,
    ) -> Result<Option<BirthKinematics>> {
        self.birth_in_frame(time, age, transform, rotation, None, unit)
    }
    fn birth_in_frame(
        &self,
        time: f32,
        age: f32,
        transform: &[f32; 16],
        rotation: &[f32; 9],
        native: Option<&super::native_transform_record::Transform>,
        mut unit: impl FnMut() -> f32,
    ) -> Result<Option<BirthKinematics>> {
        ensure!(
            time.is_finite() && age.is_finite(),
            "Invalid particle birth time"
        );
        let life = Self::varied(self.life, self.life_var, unit())?;
        ensure!(life.is_finite(), "Invalid particle lifespan");
        if age > life {
            return Ok(None);
        }
        let local_velocity = self.local_velocity(&mut unit)?;
        let position = if let Some(frame) = native {
            self.volume.position_native(frame, &mut unit)?
        } else {
            self.volume.position(transform, &mut unit)?
        };
        let velocity = if let Some(frame) = native {
            super::particle_space::direction(frame, local_velocity)
        } else {
            std::array::from_fn(|i| {
                rotation[i] * local_velocity[0]
                    + rotation[i + 3] * local_velocity[1]
                    + rotation[i + 6] * local_velocity[2]
            })
        };
        let velocity = self.flip_velocity(velocity, unit)?;
        let last_update = time - age;
        ensure!(
            velocity.into_iter().all(f32::is_finite) && last_update.is_finite(),
            "Invalid particle birth transform"
        );
        Ok(Some(BirthKinematics {
            age,
            life,
            position,
            velocity,
            last_update,
        }))
    }
    ///804d3948..804d3990 runs after the shape's initial velocity and before
    /// AddParticle. A positive probability consumes one draw, including when
    /// allocation will subsequently fail. Equality does not flip the vector.
    fn flip_velocity(&self, velocity: [f32; 3], mut unit: impl FnMut() -> f32) -> Result<[f32; 3]> {
        if self.speed_flip <= 0. {
            return Ok(velocity);
        }
        let sample = unit();
        ensure!(
            sample.is_finite() && (0. ..=1.).contains(&sample),
            "Invalid speed-flip random sample"
        );
        Ok(if sample < self.speed_flip {
            velocity.map(|v| v * -1.)
        } else {
            velocity
        })
    }
    /// Source emitter-local velocity, before volume rotation. The caller owns
    /// the RNG stream; symmetric samples use the supplied unit sample mapping.
    fn local_velocity(&self, mut unit: impl FnMut() -> f32) -> Result<[f32; 3]> {
        let mut random = || -> Result<f32> {
            let value = unit();
            ensure!(
                value.is_finite() && (0. ..=1.).contains(&value),
                "Invalid particle random sample"
            );
            Ok(value)
        };
        let speed = Self::varied(self.speed, self.speed_var, random()?)?;
        let mut dec = self.declination;
        if self.declination_var != 0. {
            dec = self.declination_var.mul_add(2. * random()? - 1., dec);
        }
        let mut plan = self.planar;
        if self.planar_var != 0. {
            plan = self.planar_var.mul_add(2. * random()? - 1., plan);
        }
        //803e5250 calls double sin/cos with a promoted float, then stores
        //single results. Platform transcendental parity remains unverified.
        let (sin_dec, cos_dec) = (dec as f64).sin_cos();
        let (sin_plan, cos_plan) = (plan as f64).sin_cos();
        let velocity = Self::volume_direction(
            speed,
            [sin_dec as f32, cos_dec as f32],
            [sin_plan as f32, cos_plan as f32],
        );
        ensure!(
            velocity.into_iter().all(f32::is_finite),
            "Invalid particle initial velocity"
        );
        Ok(velocity)
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Self::read_shape(r, "NiPSBoxEmitter")
    }
    fn read_shape(r: &mut Reader, kind: &str) -> Result<Self> {
        let _name = r.uint()?;
        let speed = r.float()?;
        let speed_var = r.float()?;
        let speed_flip = r.float()?;
        let declination = r.float()?;
        let declination_var = r.float()?;
        let planar = r.float()?;
        let planar_var = r.float()?;
        let size = r.float()?;
        let size_var = r.float()?;
        let life = r.float()?;
        let life_var = r.float()?;
        let rotation = EmitterRotation::read(r)?;
        // EM1 adds this byte to NiPSVolumeEmitter, not NiPSEmitter.
        // Mesh emitters inherit directly from the latter and omit it.
        if kind != "NiPSMeshEmitter" {
            ensure!(r.byte()? <= 1, "Unknown EM1 emitter flag");
        }
        let (node, volume) = if kind == "NiPSMeshEmitter" {
            let count = r.uint()? as usize;
            ensure!(
                count <= r.bytes.len().saturating_sub(r.at + 12) / 4,
                "Truncated mesh-emitter links"
            );
            let mut links = Vec::with_capacity(count);
            for _ in 0..count {
                links.push(r.uint()?);
            }
            // EM1's newer NIF revision stores an emitter object reference in
            // place of the older Gamebryo source's explicit emission axis.
            let node = r.uint()?;
            let emission = r.uint()?;
            let velocity = r.uint()?;
            ensure!(emission <= 4 && velocity <= 2, "Unknown mesh-emitter mode");
            (
                node,
                EmitterVolume::Mesh {
                    links,
                    emission,
                    velocity,
                },
            )
        } else {
            (r.uint()?, EmitterVolume::read(r, kind)?)
        };
        r.done()?;
        ensure!(
            life > 0. && speed >= 0. && size >= 0.,
            "Invalid hose emitter"
        );
        Ok(Self {
            speed,
            speed_var,
            speed_flip,
            declination,
            declination_var,
            planar,
            planar_var,
            size,
            size_var,
            life,
            life_var,
            node,
            volume,
            rotation,
        })
    }
}
#[derive(Clone)]
struct Curves {
    size: Vec<(f32, [f32; 1])>,
    color: Vec<(f32, [f32; 4])>,
    color_loop: u32,
    envelope: SizeEnvelope,
    height: Option<nonuniform::Track>,
}
impl Curves {
    fn read(r: &mut Reader) -> Result<Self> {
        Self::read_kind(r, false, 15)
    }
    fn read_kind(r: &mut Reader, nonuniform: bool, user: u32) -> Result<Self> {
        let mut size = Vec::new();
        for _ in 0..r.byte()? {
            let value = r.float()?;
            size.push((r.float()?, [value]));
        }
        let size_loop = r.uint()?;
        ensure!(
            size.is_empty() || size_loop == 2,
            "Unsupported size loop mode"
        );
        if size.is_empty() {
            size.push((0., [1.]));
        }
        let mut color = Vec::new();
        for _ in 0..r.byte()? {
            let value = r.take::<4>()?.map(|b| b as f32 / 255.);
            color.push((r.float()?, value));
        }
        // NiKeyframeData cycle types: clamp, loop and reverse. The old
        // reader accepted only reverse, which dropped whole authored particle
        // systems (including steam/fire) when their color keys used the other
        // two valid modes.
        let color_loop = r.uint()?;
        ensure!(color_loop <= 2, "Unsupported color loop mode {color_loop}");
        ensure!(r.byte()? == 0, "Rotation keys not implemented");
        let _rotation_loop = r.uint()?;
        let envelope = SizeEnvelope {
            grow: r.float()?,
            shrink: r.float()?,
            grow_generation: r.short()?,
            shrink_generation: r.short()?,
        };
        // DOL804edbc0..804edbd8: this byte was added at user revision13.
        let _em1_flag = if user >= 13 { r.byte()? } else { 0 };
        let height = nonuniform
            .then(|| nonuniform::Track::read(r, size_loop))
            .transpose()?;
        r.done()?;
        normalize_particle_keys(&mut size)?;
        if color.is_empty() {
            color.push((0., [1.; 4]));
        }
        normalize_particle_keys(&mut color)?;
        // Authored NiPS key arrays can be unsorted or contain duplicate time
        // stamps. NIF playback samples them in temporal order; keeping their
        // storage order made a single malformed color track disable the whole
        // particle effect (including DBC's tentacle emergence FX).
        Ok(Self {
            size,
            color,
            color_loop,
            envelope,
            height,
        })
    }

    fn color_at(&self, t: f32) -> [f32; 4] {
        sample_cycled(&self.color, t, self.color_loop)
    }
}
fn normalize_particle_keys<const N: usize>(keys: &mut Vec<(f32, [f32; N])>) -> Result<()> {
    ensure!(
        keys.iter()
            .all(|(time, value)| time.is_finite() && value.iter().all(|x| x.is_finite())),
        "Nonfinite particle keys"
    );
    keys.sort_by(|a, b| a.0.total_cmp(&b.0));
    keys.dedup_by(|a, b| {
        if a.0 == b.0 {
            *a = b.clone();
            true
        } else {
            false
        }
    });
    ensure!(!keys.is_empty(), "Empty particle key track");
    Ok(())
}
fn sample<const N: usize>(keys: &[(f32, [f32; N])], t: f32) -> [f32; N] {
    if t <= keys[0].0 {
        return keys[0].1;
    }
    for w in keys.windows(2) {
        if t < w[1].0 {
            let a = (t - w[0].0) / (w[1].0 - w[0].0);
            return std::array::from_fn(|i| w[0].1[i] + a * (w[1].1[i] - w[0].1[i]));
        }
    }
    keys.last().unwrap().1
}
fn sample_cycled<const N: usize>(keys: &[(f32, [f32; N])], t: f32, mode: u32) -> [f32; N] {
    let first = keys[0].0;
    let last = keys.last().unwrap().0;
    let span = last - first;
    let time = if span <= 0. {
        first
    } else {
        match mode {
            0 => t.clamp(first, last),
            1 => first + (t - first).rem_euclid(span),
            2 => {
                let phase = (t - first).rem_euclid(2. * span);
                if phase > span {
                    last - (phase - span)
                } else {
                    first + phase
                }
            }
            _ => unreachable!("parser validates color cycle mode"),
        }
    };
    sample(keys, time)
}
#[cfg(test)]
mod tests {
    #[test]
    fn particle_color_keys_honor_all_gamebryo_cycle_modes() {
        use super::sample_cycled;
        let keys = [(0., [0., 0., 0., 0.]), (1., [1., 0.5, 0.25, 1.])];
        assert_eq!(sample_cycled(&keys, -0.25, 0), keys[0].1);
        assert_eq!(sample_cycled(&keys, 1.25, 0), keys[1].1);
        assert_eq!(sample_cycled(&keys, 1.25, 1), [0.25, 0.125, 0.0625, 0.25]);
        assert_eq!(sample_cycled(&keys, 1.25, 2), [0.75, 0.375, 0.1875, 0.75]);
    }

    #[test]
    fn native_volume_birth_keeps_velocity_when_emitter_scale_collapses_or_reflects(
    ) -> anyhow::Result<()> {
        use super::*;
        let emitter = Emitter {
            speed: 3.,
            speed_var: 0.,
            speed_flip: 0.,
            declination: 0.,
            declination_var: 0.,
            planar: 0.,
            planar_var: 0.,
            size: 1.,
            size_var: 0.,
            life: 1.,
            life_var: 0.,
            node: 0,
            volume: EmitterVolume::Box(BoxVolume([2., 0., 0.])),
            rotation: EmitterRotation::default(),
        };
        let identity = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        for scale in [-2., 0., 3.] {
            let frame = [0., -1., 0., 1., 0., 0., 0., 0., 1., 10., 20., 30., scale];
            let mut draws = 0;
            let birth = emitter
                .birth_in_frame(1., 0., &identity, &[0.; 9], Some(&frame), || {
                    draws += 1;
                    if draws == 3 {
                        0.75
                    } else {
                        0.5
                    }
                })?
                .unwrap();
            assert_eq!(birth.position, [10., 20. + 0.5 * scale, 30.]);
            assert_eq!(birth.velocity, [0., 0., 3.]);
            assert_eq!(
                draws, 3,
                "Frame selection changed the original lifespan/speed/volume RNG order"
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_steam_jet_late_start_preserves_particle_lifetimes() -> anyhow::Result<()> {
        use super::*;
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut store = super::super::resource_store::Store::new(
            &root,
            &[
                "_dynamic.pak".into(),
                "gremlinvillage_zonea.pak".into(),
                "r3mt.pak".into(),
            ],
        )?;
        let model = store.read("Effects/_Shared/Steam_Jet_02.nif")?;
        let files = store.particle_files(&model.data)?;
        let prototype = Hose::load_attached(&model.data, &files)?;
        let mut early = prototype.instantiate()?;
        let mut late = prototype.instantiate()?;
        let owner = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        // Binary-exact steps separate clock-origin handling from accumulated
        // 30Hz rounding. Tolerances cover single-precision timestamp offsets,
        // not different emission counts or substantially different trajectories.
        for _ in 0..80 {
            late.advance_attached(0.125, false, owner)?;
        }
        for _ in 0..8 {
            early.advance_attached(0.125, true, owner)?;
            late.advance_attached(0.125, true, owner)?;
        }
        assert!(early.particle_count() > 0);
        assert_eq!(early.particle_count(), late.particle_count());
        for (a, b) in early.layers.iter().zip(&late.layers) {
            assert_eq!(a.particles.len(), b.particles.len());
            for (a, b) in a.particles.iter().zip(&b.particles) {
                assert!(
                    (a.age - b.age).abs() <= 1e-5,
                    "Late start changed age: {} vs {}",
                    a.age,
                    b.age
                );
                assert!((b.last_update - late.time).abs() <= 1e-5);
                for i in 0..3 {
                    assert!(
                        (a.p[i] - b.p[i]).abs() <= 0.001,
                        "Late start changed trajectory: {:?} vs {:?}",
                        a.p,
                        b.p
                    );
                }
            }
        }
        early.advance_attached(0.125, false, owner)?;
        late.advance_attached(0.125, false, owner)?;
        assert!(
            late.particle_count() > 0,
            "STOP prematurely erased the late-started jet"
        );
        assert_eq!(early.particle_count(), late.particle_count());
        println!(
            "Original steam jet: {} retained particles after late START and STOP",
            late.particle_count()
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_emission_controller_inventory() -> anyhow::Result<()> {
        use super::*;
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            root.join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let mut count = 0;
        let mut posed = 0;
        let mut animated = 0;
        for file in &pak.files {
            if !file.path.to_ascii_lowercase().ends_with(".nif_wii") {
                continue;
            }
            let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            for (index, block) in nif.blocks.iter().enumerate() {
                if block.type_name() != "NiPSEmitParticlesCtlr" {
                    continue;
                }
                let ctl = EmissionController::load(&nif, index as u32)
                    .with_context(|| format!("{} block {index}", file.path))?;
                count += 1;
                let playback = EmissionPlayback::load(&nif, index as u32)?;
                assert_eq!(playback.controller.flags & 0xc0, 0x40);
                let rate = RateTrack::load(&nif, ctl.rate)
                    .with_context(|| format!("{} rate block {index}", file.path))?;
                if let RateTrack::Keys { keys, .. } = &rate {
                    for key in keys {
                        ensure!(
                            rate.sample(key.time)?.is_finite(),
                            "Invalid sampled original rate"
                        );
                    }
                }
                match ActivationTrack::load(&nif, ctl.active)
                    .with_context(|| format!("{} block {index}", file.path))?
                {
                    ActivationTrack::Constant(_) => posed += 1,
                    ActivationTrack::Step(keys) => {
                        animated += 1;
                        let track = ActivationTrack::Step(keys);
                        if let ActivationTrack::Step(keys) = &track {
                            for &(time, value) in keys {
                                assert_eq!(track.sample(time)?, value);
                            }
                        }
                    }
                }
            }
        }
        ensure!(count > 0, "No production particle controllers");
        println!(
            "Controllers: {count}, constant activation: {posed}, other activation: {animated}"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_direct_emission_playback() -> Result<()> {
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            root.join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let mut tested = 0;
        let mut managed = 0;
        let mut births = 0usize;
        let mut emitting = 0;
        let mut failures = Vec::new();
        for file in &pak.files {
            if !file.path.to_ascii_lowercase().ends_with(".nif_wii") {
                continue;
            }
            let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            for (index, block) in nif.blocks.iter().enumerate() {
                if block.type_name() != "NiPSEmitParticlesCtlr" {
                    continue;
                }
                let mut playback = EmissionPlayback::load(&nif, index as u32)?;
                if playback.controller.flags & 0x20 != 0 {
                    managed += 1;
                    continue;
                }
                tested += 1;
                playback.start();
                let before = births;
                // Ten seconds of direct playback with a separately advancing
                // system clock. This probes scheduling, not particle physics.
                for frame in 0..=1200 {
                    let time = frame as f32 / 120.;
                    let result = playback.update(
                        time,
                        true,
                        Some((frame as f32 - 1.).max(0.) / 120.),
                        |stamp, _, ages| {
                            ensure!(stamp.is_finite(), "Nonfinite emission timestamp");
                            for age in ages {
                                ensure!(age.is_finite(), "Nonfinite birth age");
                                births += 1;
                            }
                            Ok(())
                        },
                    );
                    if let Err(error) = result {
                        failures.push(format!(
                            "{} block {index} frame {frame}: {error:#}",
                            file.path
                        ));
                        break;
                    }
                }
                if births > before {
                    emitting += 1;
                }
            }
        }
        println!("Direct controllers: {tested}; emitting: {emitting}; manager-owned: {managed}; births: {births}");
        ensure!(
            failures.is_empty(),
            "Production playback failures:\n{}",
            failures.join("\n")
        );
        ensure!(
            tested > 0 && emitting > 0 && births > 0,
            "Production playback did not exercise emission"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_emitter_support_audit() -> Result<()> {
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            root.join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let mut rows = Vec::new();
        let mut supported = 0;
        for file in &pak.files {
            if !file.path.to_ascii_lowercase().ends_with(".nif_wii") {
                continue;
            }
            let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            for (index, block) in nif.blocks.iter().enumerate() {
                let kind = block.type_name();
                if !kind.starts_with("NiPS") || !kind.ends_with("Emitter") {
                    continue;
                }
                let result = if matches!(
                    kind,
                    "NiPSBoxEmitter"
                        | "NiPSCylinderEmitter"
                        | "NiPSSphereEmitter"
                        | "NiPSTorusEmitter"
                        | "NiPSMeshEmitter"
                ) {
                    Emitter::read_shape(&mut raw(&nif, index as u32, kind)?, kind)
                } else {
                    Err(anyhow::anyhow!("Unsupported emitter shape {kind}"))
                };
                let status = match result {
                    Ok(emitter) => {
                        if let EmitterVolume::Mesh { links, .. } = &emitter.volume {
                            for &link in links {
                                ensure!(
                                    link == u32::MAX
                                        || nif
                                            .blocks
                                            .get(link as usize)
                                            .is_some_and(|b| b.as_av_object().is_some()),
                                    "{} emitter {index}: invalid mesh link {link}",
                                    file.path
                                );
                            }
                        }
                        emitter.local_velocity(|| 0.5)?;
                        supported += 1;
                        serde_json::json!({"decoded":true,"volume":format!("{:?}",emitter.volume)})
                    }
                    Err(error) => {
                        serde_json::json!({"decoded":false,"reason":format!("{error:#}")})
                    }
                };
                rows.push(serde_json::json!({"asset":file.path,"block":index,"type":kind,"status":status}));
            }
        }
        ensure!(!rows.is_empty(), "No emitters audited");
        println!(
            "Emitter audit: {supported}/{} decoded; see target/particle-emitter-audit.json",
            rows.len()
        );
        std::fs::write(
            "target/particle-emitter-audit.json",
            serde_json::to_vec_pretty(&rows)?,
        )?;
        // This is a coverage inventory, not a claim that rejected emitters work.
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_general_step_audit() -> Result<()> {
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            root.join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let mut rows = Vec::new();
        let mut decoded = 0;
        for file in &pak.files {
            if !file.path.to_ascii_lowercase().ends_with(".nif_wii") {
                continue;
            }
            let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            for (index, block) in nif.blocks.iter().enumerate() {
                if block.type_name() != "NiPSSimulatorGeneralStep" {
                    continue;
                }
                let status = match Curves::read_kind(
                    &mut raw(&nif, index as u32, block.type_name())?,
                    false,
                    nif.user_version,
                ) {
                    Ok(curves) => {
                        decoded += 1;
                        serde_json::json!({"decoded":true,"size_keys":curves.size.len(),
                        "color_keys":curves.color.len(),"grow":curves.envelope.grow,"shrink":curves.envelope.shrink,
                        "grow_generation":curves.envelope.grow_generation,"shrink_generation":curves.envelope.shrink_generation})
                    }
                    Err(error) => {
                        serde_json::json!({"decoded":false,"reason":format!("{error:#}")})
                    }
                };
                rows.push(serde_json::json!({"asset":file.path,"block":index,"status":status}));
            }
        }
        ensure!(!rows.is_empty(), "No general steps audited");
        println!("General steps decoded: {decoded}/{}", rows.len());
        std::fs::write(
            "target/particle-general-step-audit.json",
            serde_json::to_vec_pretty(&rows)?,
        )?;
        Ok(())
    }
    #[test]
    fn particle_activation_changes_on_authored_key_boundaries() {
        let track =
            ActivationTrack::Step(vec![(0., false), (0.1, true), (0.2, false), (0.3, true)]);
        for (time, value) in [
            (-1., false),
            (0., false),
            (0.099, false),
            (0.1, true),
            (0.199, true),
            (0.2, false),
            (0.3, true),
        ] {
            assert_eq!(track.sample(time).unwrap(), value);
        }
        assert!(track.sample(f32::NAN).is_err());
        assert!(track.sample(1.).is_err());
        assert!(ActivationTrack::Step(vec![]).sample(0.).is_err());
    }
    #[test]
    fn emission_walk_retains_short_windows_and_native_rate_sampling() {
        let track = ActivationTrack::Step(vec![
            (0., false),
            (1., true),
            (1.25, true),
            (2., false),
            (3., true),
            (4., false),
            (5., false),
        ]);
        let mut spans = Vec::new();
        track
            .active_spans(1.5, 4.5, |s| {
                spans.push(s);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            spans,
            vec![
                ActiveSpan {
                    from: 1.5,
                    to: 2.,
                    start: 1.,
                    stop: 2.,
                    rate_sample_time: 1.5
                },
                ActiveSpan {
                    from: 3.,
                    to: 4.,
                    start: 3.,
                    stop: 4.,
                    rate_sample_time: 3.5
                }
            ]
        );
        spans.clear();
        track
            .active_spans(1.5, 2., |s| {
                spans.push(s);
                Ok(())
            })
            .unwrap();
        assert_eq!(
            spans,
            vec![ActiveSpan {
                from: 1.5,
                to: 2.,
                start: 1.,
                stop: 2.,
                rate_sample_time: 2.
            }]
        );
        assert!(track.active_spans(4., 1., |_| Ok(())).is_err());
    }
    #[test]
    fn particle_rate_interpolation_preserves_step_and_cubic_tangents() {
        let keys = || {
            vec![
                RateKey {
                    time: 0.,
                    value: 0.,
                    incoming: 0.,
                    outgoing: 4.,
                },
                RateKey {
                    time: 2.,
                    value: 8.,
                    incoming: -4.,
                    outgoing: 0.,
                },
            ]
        };
        let linear = RateTrack::Keys {
            kind: 1,
            keys: keys(),
        };
        assert_eq!(linear.sample(1.).unwrap(), 4.);
        let step = RateTrack::Keys {
            kind: 5,
            keys: keys(),
        };
        assert_eq!(step.sample(1.).unwrap(), 0.);
        assert_eq!(step.sample(2.).unwrap(), 8.);
        let mut curve = keys();
        curve[1].value = 0.;
        let cubic = RateTrack::Keys {
            kind: 2,
            keys: curve,
        };
        assert_eq!(cubic.sample(0.).unwrap(), 0.);
        assert_eq!(cubic.sample(1.).unwrap(), 1.);
        assert_eq!(cubic.sample(2.).unwrap(), 0.);
        assert!(cubic.sample(3.).is_err());
        assert!(cubic.sample(f32::NAN).is_err());
    }
    #[test]
    fn particle_births_use_absolute_window_phase_and_oldest_first_ages() {
        assert_eq!(
            birth_ages(1., 0., 0., 1., 4.).unwrap().collect::<Vec<_>>(),
            vec![0.75, 0.5, 0.25, 0.]
        );
        assert_eq!(
            birth_ages(1., 0.5, 0., 1., 4.).unwrap().collect::<Vec<_>>(),
            vec![0.25, 0.]
        );
        assert_eq!(
            birth_ages(1., 0., 0.25, 1., 4.)
                .unwrap()
                .collect::<Vec<_>>(),
            vec![0.5, 0.25, 0.]
        );
        assert_eq!(
            birth_ages(2., 0., 0., 1., 4.).unwrap().collect::<Vec<_>>(),
            vec![0.75, 0.5, 0.25, 0.]
        );
        assert_eq!(birth_ages(1., 1., 0., 1., 4.).unwrap().count(), 0);
        assert_eq!(birth_ages(1., 0., 0., 1., 0.).unwrap().count(), 0);
        assert_eq!(birth_ages(0.125, 0., 0., 1., 4.).unwrap().count(), 0);
        assert!(birth_ages(1., 0., 0., 1., f32::MAX).is_err());
        assert!(birth_ages(0., 1., 0., 1., 4.).is_err());
    }
    #[test]
    fn emission_batches_preserve_birth_phase_across_frame_splits() -> Result<()> {
        let active = ActivationTrack::Step(vec![(0., false), (0.25, true), (1., false)]);
        let rate = RateTrack::Constant(8.);
        let mut full = Vec::new();
        active.emission_batches(&rate, 0., 1., |span, ages| {
            full.extend(ages.map(|age| span.to - age));
            Ok(())
        })?;
        let mut split = Vec::new();
        for (from, to) in [(0., 0.375), (0.375, 0.625), (0.625, 1.)] {
            active.emission_batches(&rate, from, to, |span, ages| {
                split.extend(ages.map(|age| span.to - age));
                Ok(())
            })?;
        }
        assert_eq!(full, vec![0.375, 0.5, 0.625, 0.75, 0.875, 1.]);
        assert_eq!(split, full);
        let mut count = 0;
        ActivationTrack::Constant(true).emission_batches(&rate, 0.375, 0.5, |_, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 1);
        assert!(active
            .emission_batches(&rate, 0., 1., |_, _| anyhow::bail!("Emitter failed"))
            .is_err());
        Ok(())
    }
    #[test]
    fn emission_scheduler_suppresses_initial_reset_and_manager_switch_updates() -> Result<()> {
        let mut state = EmissionState::default();
        let active = ActivationTrack::Constant(true);
        let rate = RateTrack::Constant(8.);
        let mut count = 0;
        state.update(10., 0.5, false, 1, &active, &rate, |_, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 0);
        state.update(10.25, 0.75, false, 1, &active, &rate, |_, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 2);
        state.update(9., 0.25, false, 1, &active, &rate, |_, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 2);
        state.update(9.25, 0.5, true, 2, &active, &rate, |_, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 2);
        state.update(9.5, 0.75, true, 2, &active, &rate, |_, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 4);
        state.update(9.75, 0.125, true, 2, &active, &rate, |_, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(
            count, 5,
            "Posed loop must restart from zero, without replaying tail"
        );
        state.reset();
        state.update(10., 0.375, true, 2, &active, &rate, |_, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 5);
        Ok(())
    }
    #[test]
    fn posed_activation_rate_loop_emits_tail_then_head_with_distinct_samples() -> Result<()> {
        let active = ActivationTrack::Constant(true);
        let rate = RateTrack::Keys {
            kind: 1,
            keys: vec![
                RateKey {
                    time: 2.,
                    value: 4.,
                    incoming: 0.,
                    outgoing: 0.,
                },
                RateKey {
                    time: 4.,
                    value: 12.,
                    incoming: 0.,
                    outgoing: 0.,
                },
            ],
        };
        let mut state = EmissionState::default();
        let mut batches = Vec::new();
        state.update(10., 3.5, false, 1, &active, &rate, |_, _| {
            panic!("First update emitted")
        })?;
        state.update(11., 2.5, false, 1, &active, &rate, |span, ages| {
            batches.push((span, ages.count()));
            Ok(())
        })?;
        assert_eq!(
            batches,
            vec![
                (
                    ActiveSpan {
                        from: 3.5,
                        to: 4.,
                        start: 2.,
                        stop: 4.,
                        rate_sample_time: 3.75
                    },
                    6
                ),
                (
                    ActiveSpan {
                        from: 2.,
                        to: 2.5,
                        start: 2.,
                        stop: 2.5,
                        rate_sample_time: 2.25
                    },
                    2
                )
            ]
        );
        Ok(())
    }
    #[test]
    fn keyed_activation_loop_retains_source_stop_key_and_strict_start_rules() -> Result<()> {
        let active = ActivationTrack::Step(vec![
            (0., false),
            (0.25, true),
            (0.5, false),
            (0.75, true),
            (1., false),
        ]);
        let rate = RateTrack::Constant(8.);
        let mut state = EmissionState::default();
        let mut batches = Vec::new();
        state.update(10., 0.875, false, 1, &active, &rate, |_, _| {
            panic!("First update emitted")
        })?;
        state.update(11., 0.375, false, 1, &active, &rate, |span, ages| {
            batches.push((span, ages.collect::<Vec<_>>()));
            Ok(())
        })?;
        // The source ends after this tail, despite a true key in the head.
        assert_eq!(
            batches,
            vec![(
                ActiveSpan {
                    from: 0.875,
                    to: 1.,
                    start: 0.75,
                    stop: 1.,
                    rate_sample_time: 0.875
                },
                vec![0.]
            )]
        );
        state.reset();
        batches.clear();
        state.update(12., 1., false, 1, &active, &rate, |_, _| {
            panic!("Reset emitted")
        })?;
        state.update(13., 0.375, false, 1, &active, &rate, |span, ages| {
            batches.push((span, ages.collect::<Vec<_>>()));
            Ok(())
        })?;
        assert_eq!(
            batches,
            vec![(
                ActiveSpan {
                    from: 0.,
                    to: 0.375,
                    start: 0.25,
                    stop: 0.375,
                    rate_sample_time: 0.3125
                },
                vec![0.]
            )]
        );
        state.reset();
        state.update(14., 1., false, 1, &active, &rate, |_, _| {
            panic!("Reset emitted")
        })?;
        state.update(15., 0.25, false, 1, &active, &rate, |_, _| {
            panic!("Wrapped start lookup must be strict")
        })?;
        // History still advances: the subsequent forward interval emits.
        let mut births = 0;
        state.update(16., 0.375, false, 1, &active, &rate, |_, ages| {
            births += ages.count();
            Ok(())
        })?;
        assert_eq!(births, 1);
        Ok(())
    }
    #[test]
    fn emission_timestamps_use_system_clock_and_controller_loop_range() -> Result<()> {
        let clock = EmissionClock {
            system_last: Some(100.),
            key_start: 0.,
            key_stop: 2.,
        };
        let active = ActivationTrack::Step(vec![
            (0., false),
            (0.25, true),
            (0.5, false),
            (0.75, true),
            (1., false),
        ]);
        let rate = RateTrack::Constant(8.);
        let mut state = EmissionState::default();
        let mut times = Vec::new();
        state.update_timed(10., 0.875, false, 1, &active, &rate, &clock, |_, _, _| {
            panic!("First update emitted")
        })?;
        state.update_timed(
            11.,
            0.375,
            false,
            1,
            &active,
            &rate,
            &clock,
            |time, _, _| {
                times.push(time);
                Ok(())
            },
        )?;
        // Controller range is two seconds although activation ends at one.
        assert_eq!(times, vec![101.5]);
        let active = ActivationTrack::Constant(true);
        let rate = RateTrack::Keys {
            kind: 1,
            keys: vec![
                RateKey {
                    time: 2.,
                    value: 4.,
                    incoming: 0.,
                    outgoing: 0.,
                },
                RateKey {
                    time: 4.,
                    value: 12.,
                    incoming: 0.,
                    outgoing: 0.,
                },
            ],
        };
        state.reset();
        times.clear();
        state.update_timed(12., 3.5, false, 1, &active, &rate, &clock, |_, _, _| {
            panic!("Reset emitted")
        })?;
        state.update_timed(13., 2.5, false, 1, &active, &rate, &clock, |time, _, _| {
            times.push(time);
            Ok(())
        })?;
        assert_eq!(times, vec![100.5, 101.]);
        // Animated activation alone has a source fallback for an uninitialized
        // system clock; posed emission must not invent that fallback.
        let clock = EmissionClock {
            system_last: None,
            key_start: 0.,
            key_stop: 1.,
        };
        state.reset();
        state.update_timed(
            14.,
            0.,
            false,
            1,
            &active,
            &RateTrack::Constant(8.),
            &clock,
            |_, _, _| Ok(()),
        )?;
        assert!(state
            .update_timed(
                15.,
                0.25,
                false,
                1,
                &active,
                &RateTrack::Constant(8.),
                &clock,
                |_, _, _| Ok(())
            )
            .is_err());
        Ok(())
    }
    #[test]
    fn controller_time_preserves_authored_modes_and_frequency_changes() -> Result<()> {
        let mut c = EmissionController {
            next: u32::MAX,
            flags: 4,
            frequency: 1.,
            phase: 0.,
            start: 0.,
            stop: 100.,
            target: u32::MAX,
            rate: 0,
            emitter_name: 0,
            active: 0,
        };
        let mut time = ControllerTime::default();
        assert_eq!(time.scale(10., &c)?, 10.);
        c.frequency = 2.;
        assert_eq!(time.scale(11., &c)?, 12.);
        time.reset();
        c.flags = 5;
        c.phase = 3.;
        assert_eq!(time.scale(10., &c)?, 3.);
        assert_eq!(time.scale(11., &c)?, 5.);
        c.frequency = 1.;
        c.phase = 0.;
        c.start = 2.;
        c.stop = 4.;
        c.flags = 0;
        time.reset();
        assert_eq!(time.scale(-0.5, &c)?, 3.5);
        assert_eq!(time.scale(4., &c)?, 2.);
        c.flags = 2;
        time.reset();
        assert_eq!(time.scale(0.5, &c)?, 2.5);
        assert_eq!(time.scale(2.5, &c)?, 3.5);
        c.flags = 2 | 0x10;
        time.reset();
        assert_eq!(time.scale(0.5, &c)?, 3.5);
        c.stop = c.start;
        assert_eq!(time.scale(1., &c)?, 2.);
        c.flags = 6;
        assert!(time.scale(2., &c).is_err());
        Ok(())
    }
    #[test]
    fn direct_playback_gates_inactive_managed_unbound_and_duplicate_updates() -> Result<()> {
        let controller = EmissionController {
            next: u32::MAX,
            flags: 0x4c,
            frequency: 1.,
            phase: 0.,
            start: 0.,
            stop: 10.,
            target: 0,
            rate: 0,
            emitter_name: 0,
            active: 0,
        };
        let mut playback = EmissionPlayback {
            block: 0,
            application_start: None,
            controller,
            active: ActivationTrack::Constant(true),
            rate: RateTrack::Constant(8.),
            time: ControllerTime::default(),
            state: EmissionState::default(),
            scaled: None,
        };
        playback.update(0., false, Some(0.), |_, _, _| panic!("Unbound emitted"))?;
        playback.update(0., true, Some(0.), |_, _, _| panic!("First update emitted"))?;
        let mut count = 0;
        playback.update(0.25, true, Some(0.), |time, _, ages| {
            assert_eq!(time, 0.25);
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 2);
        playback.update(0.25, true, Some(0.25), |_, _, _| {
            panic!("Duplicate update emitted")
        })?;
        playback.controller.flags |= 0x20;
        playback.update(0.5, true, Some(0.25), |_, _, _| {
            panic!("Manager controller updated directly")
        })?;
        assert_eq!(playback.time.last, Some(0.25));
        playback.controller.flags &= !0x20;
        playback.stop();
        playback.update(1., true, Some(0.5), |_, _, _| panic!("Stopped emitted"))?;
        playback.start();
        playback.update(1., true, Some(1.), |_, _, _| {
            panic!("Restart replayed old births")
        })?;
        playback.update(1.25, true, Some(1.), |_, _, ages| {
            count += ages.count();
            Ok(())
        })?;
        assert_eq!(count, 4);
        playback.controller.flags |= 1;
        let scaled = playback.scaled;
        playback.native_command(None);
        assert_eq!(playback.scaled, scaled);
        assert_eq!(playback.state.last_scaled, None);
        playback.native_command(Some(15.25));
        assert_eq!(playback.scaled, scaled);
        assert_eq!(playback.application_start, Some(15.25));
        playback.native_command(Some(16.5));
        assert_eq!(playback.application_start, Some(16.5));
        playback.native_command(None);
        assert_eq!(playback.application_start, None);
        Ok(())
    }
    #[test]
    fn box_emission_preserves_axis_order_zero_axis_rng_and_transform() -> Result<()> {
        let volume = BoxVolume([2., 0., 4.]);
        let transform = [
            0., 2., 0., 0., -2., 0., 0., 0., 0., 0., 2., 0., 10., 20., 30., 1.,
        ];
        let mut samples = [0., 1.].into_iter();
        assert_eq!(
            volume.position(&transform, || samples
                .next()
                .expect("Zero axis consumed RNG"))?,
            [10., 18., 34.]
        );
        assert!(samples.next().is_none());
        assert_eq!(
            BoxVolume([0.; 3]).position(&transform, || panic!("Point emitter consumed RNG"))?,
            [10., 20., 30.]
        );
        assert!(volume.position(&transform, || f32::NAN).is_err());
        Ok(())
    }
    #[test]
    fn lifespan_speed_and_direction_match_bounded_retail_arithmetic() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/particle_birth.json"
        ))?;
        for case in fixture["scalars"].as_array().unwrap() {
            let actual = Emitter::varied(
                case["base"].as_f64().unwrap() as f32,
                case["variance"].as_f64().unwrap() as f32,
                case["raw"].as_u64().unwrap() as f32 / 32767.,
            )?;
            assert_eq!(
                u64::from(actual.to_bits()),
                case["bits"].as_u64().unwrap(),
                "{case}"
            );
            assert_eq!(case["draws"], 1);
        }
        for case in fixture["directions"].as_array().unwrap() {
            let trig: [[f32; 2]; 2] = serde_json::from_value(case["trig"].clone())?;
            let actual =
                Emitter::volume_direction(case["speed"].as_f64().unwrap() as f32, trig[0], trig[1]);
            assert_eq!(
                serde_json::json!(actual.map(f32::to_bits)),
                case["bits"],
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn speed_reversal_matches_original_instructions_and_precedes_allocation() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/particle_speed_flip.json"
        ))?;
        let mut emitter = Emitter {
            speed: 10.,
            speed_var: 0.,
            speed_flip: 0.,
            declination: 0.,
            declination_var: 0.,
            planar: 0.,
            planar_var: 0.,
            size: 1.,
            size_var: 0.,
            life: 1.,
            life_var: 0.,
            node: 0,
            volume: EmitterVolume::Box(BoxVolume([0.; 3])),
            rotation: EmitterRotation::default(),
        };
        for case in fixture["cases"].as_array().unwrap() {
            emitter.speed_flip = case["probability"].as_f64().unwrap() as f32;
            let velocity: [f32; 3] = serde_json::from_value(case["velocity"].clone())?;
            let mut draws = 0;
            let actual = emitter.flip_velocity(velocity, || {
                draws += 1;
                case["raw"].as_u64().unwrap() as f32 / 32767.
            })?;
            assert_eq!(
                serde_json::json!(actual.map(f32::to_bits)),
                case["bits"],
                "{case}"
            );
            assert_eq!(draws, case["draws"].as_u64().unwrap(), "{case}");
        }
        emitter.speed_flip = 0.5;
        let transform = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let rotation = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let mut pending = PendingBirths {
            capacity: 0,
            active: 0,
            particles: Vec::new(),
        };
        let mut draws = 0;
        pending.emit(
            &emitter,
            1.,
            [0., 0.],
            &transform,
            &rotation,
            false,
            false,
            || {
                draws += 1;
                0.25
            },
        )?;
        assert_eq!(
            draws, 3,
            "Full allocation consumes lifespan, speed and flip before stopping"
        );
        assert!(pending.particles.is_empty());
        let mut samples = [0.5].into_iter();
        assert!(emitter
            .birth_kinematics(1., 2., &transform, &rotation, || samples
                .next()
                .expect("Expired birth reached speed/flip"))?
            .is_none());
        assert!(samples.next().is_none());
        Ok(())
    }
    #[test]
    fn initial_velocity_preserves_em1_axes_and_variation_order() -> Result<()> {
        let mut emitter = Emitter {
            speed: 10.,
            speed_var: 0.,
            speed_flip: 0.,
            declination: 0.,
            declination_var: 0.,
            planar: 0.,
            planar_var: 0.,
            size: 1.,
            size_var: 0.,
            life: 1.,
            life_var: 0.,
            node: 0,
            volume: EmitterVolume::Box(BoxVolume([0.; 3])),
            rotation: EmitterRotation::default(),
        };
        let mut samples = [0.5].into_iter();
        assert_eq!(
            emitter.local_velocity(|| samples.next().unwrap())?,
            [0., 0., 10.]
        );
        assert!(
            samples.next().is_none(),
            "Zero variance still consumes speed"
        );
        emitter.speed_var = 4.;
        emitter.declination_var = std::f32::consts::PI;
        emitter.planar = 0.;
        emitter.planar_var = std::f32::consts::PI;
        let mut samples = [1., 0.75, 0.5].into_iter();
        let velocity = emitter.local_velocity(|| samples.next().expect("Unexpected RNG call"))?;
        assert!(samples.next().is_none());
        assert!(
            velocity[0].abs() < 1e-5
                && (velocity[1] + 12.).abs() < 1e-5
                && velocity[2].abs() < 1e-5
        );
        assert!(emitter.local_velocity(|| -1.).is_err());
        Ok(())
    }
    #[test]
    fn cylinder_and_sphere_retain_source_radial_distribution() -> Result<()> {
        let matrix = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 10., 20., 30., 1.,
        ];
        let mut samples = [0.25, 0., 1.].into_iter();
        let cylinder = EmitterVolume::Cylinder {
            radius: 8.,
            height: 4.,
        };
        assert_eq!(
            cylinder.position(&matrix, || samples.next().unwrap())?,
            [12., 20., 32.]
        );
        assert!(samples.next().is_none());
        let mut samples = [0.25, 0., 0.25].into_iter();
        let sphere = EmitterVolume::Sphere { radius: 8. };
        let point = sphere.position(&matrix, || samples.next().unwrap())?;
        assert!((point[0] - 12.).abs() < 1e-5 && point[1] == 20. && (point[2] - 30.).abs() < 1e-5);
        assert!(samples.next().is_none());
        for volume in [
            EmitterVolume::Sphere { radius: 0. },
            EmitterVolume::Cylinder {
                radius: 0.,
                height: 0.,
            },
        ] {
            assert_eq!(
                volume.position(&matrix, || panic!("Zero volume consumed RNG"))?,
                [10., 20., 30.]
            );
        }
        let mut torus_draws = 0;
        let point = EmitterVolume::Torus {
            radius: 1.,
            section_radius: 0.5,
        }
        .position(&matrix, || {
            torus_draws += 1;
            0.5
        })?;
        assert_eq!(torus_draws, 3);
        assert!(point.into_iter().all(f32::is_finite));
        Ok(())
    }
    #[test]
    fn birth_kinematics_skips_expired_and_keeps_velocity_unscaled() -> Result<()> {
        let emitter = Emitter {
            speed: 10.,
            speed_var: 0.,
            speed_flip: 0.,
            declination: 0.,
            declination_var: 0.,
            planar: 0.,
            planar_var: 0.,
            size: 1.,
            size_var: 0.,
            life: 1.,
            life_var: 2.,
            node: 0,
            volume: EmitterVolume::Box(BoxVolume([2., 0., 0.])),
            rotation: EmitterRotation::default(),
        };
        let transform = [
            2., 0., 0., 0., 0., 2., 0., 0., 0., 0., 2., 0., 10., 20., 30., 1.,
        ];
        let rotation = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let mut samples = [0.5].into_iter();
        assert!(emitter
            .birth_kinematics(5., 1.01, &transform, &rotation, || samples
                .next()
                .expect("Expired particle consumed more RNG"))?
            .is_none());
        let mut samples = [0.5, 0.5, 1.].into_iter();
        let birth = emitter
            .birth_kinematics(5., 1., &transform, &rotation, || samples.next().unwrap())?
            .unwrap();
        assert_eq!(birth.age, 1.);
        assert_eq!(birth.life, 1.);
        assert_eq!(birth.last_update, 4.);
        assert_eq!(birth.position, [12., 20., 30.]);
        assert_eq!(birth.velocity, [0., 0., 10.]);
        assert!(samples.next().is_none());
        Ok(())
    }
    #[test]
    fn rotation_channels_preserve_random_order_and_sign_boundary() -> Result<()> {
        let rotation = EmitterRotation {
            angle: 1.,
            angle_var: 2.,
            speed: 3.,
            speed_var: 4.,
            axis: [2., 3., 4.],
            random_sign: true,
            random_axis: true,
        };
        assert_eq!(
            rotation.sample(false, false, || panic!("Disabled channel consumed RNG"))?,
            InitialRotation {
                angles: None,
                axis: None
            }
        );
        let mut samples = [1., 0.75, 0.5, 0.5, 0.].into_iter();
        let initial = rotation.sample(true, true, || samples.next().unwrap())?;
        assert_eq!(initial.angles, Some((3., -5.)));
        let axis = initial.axis.unwrap();
        assert!((axis[0] - 1.).abs() < 1e-6 && axis[1] == 0. && axis[2].abs() < 1e-6);
        assert!(samples.next().is_none());
        let fixed = EmitterRotation {
            axis: [2., 3., 4.],
            ..EmitterRotation::default()
        };
        assert_eq!(
            fixed
                .sample(false, true, || panic!("Fixed axis consumed RNG"))?
                .axis,
            Some([2., 3., 4.])
        );
        Ok(())
    }
    #[test]
    fn pending_births_respect_native_capacity_and_initialization_order() -> Result<()> {
        let emitter = Emitter {
            speed: 10.,
            speed_var: 0.,
            speed_flip: 0.,
            declination: 0.,
            declination_var: 0.,
            planar: 0.,
            planar_var: 0.,
            size: 2.,
            size_var: 2.,
            life: 1.,
            life_var: 2.,
            node: 0,
            volume: EmitterVolume::Box(BoxVolume([0.; 3])),
            rotation: EmitterRotation::default(),
        };
        let transform = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let rotation = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        let mut pending = PendingBirths {
            capacity: 2,
            active: 1,
            particles: Vec::new(),
        };
        // First lifespan/speed/radius, then the failed allocation's lifespan/speed.
        let mut samples = [0.5, 0.5, 0.75, 0.5, 0.5].into_iter();
        pending.emit(
            &emitter,
            2.,
            [0.25, 0.125, 0.],
            &transform,
            &rotation,
            false,
            false,
            || samples.next().expect("Batch continued after full capacity"),
        )?;
        assert!(samples.next().is_none());
        assert_eq!(pending.active, 1);
        assert_eq!(pending.particles.len(), 1);
        let particle = &pending.particles[0];
        assert_eq!(particle.radius, 3.);
        assert_eq!(particle.size, 1.);
        assert_eq!(particle.motion.last_update, 1.75);
        assert_eq!(
            particle.rotation,
            InitialRotation {
                angles: None,
                axis: None
            }
        );
        Ok(())
    }
    #[test]
    fn initial_size_uses_generation_and_minimum_of_grow_and_shrink() -> Result<()> {
        let mut particle = InitialParticle {
            motion: BirthKinematics {
                age: 0.,
                life: 1.,
                position: [0.; 3],
                velocity: [0.; 3],
                last_update: 0.,
            },
            radius: 2.,
            size: 1.,
            rotation: InitialRotation {
                angles: None,
                axis: None,
            },
        };
        let envelope = SizeEnvelope {
            grow: 2.,
            shrink: 2.,
            grow_generation: 0,
            shrink_generation: 0,
        };
        envelope.initialize(&mut particle, 0)?;
        assert_eq!(particle.size, 0.0001);
        particle.motion.age = 0.5;
        envelope.initialize(&mut particle, 0)?;
        assert_eq!(particle.size, 0.25); // minimum, not product (0.0625)
        envelope.initialize(&mut particle, 1)?;
        assert_eq!(particle.size, 1.);
        particle.motion.age = 1.;
        envelope.initialize(&mut particle, 0)?;
        assert_eq!(particle.size, 0.0001);
        assert_eq!(particle.radius, 2.);
        Ok(())
    }
    #[test]
    fn em1_general_step_retains_envelope_times_and_u16_generations() -> Result<()> {
        for little in [false, true] {
            let mut bytes = Vec::new();
            let word = |out: &mut Vec<u8>, v: u32| {
                out.extend(if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                })
            };
            bytes.push(0);
            word(&mut bytes, 2); // no size keys
            bytes.push(1);
            bytes.extend([255, 255, 255, 255]);
            word(&mut bytes, 0); // color at zero
            word(&mut bytes, 2);
            bytes.push(0);
            word(&mut bytes, 2); // no rotation keys
            word(&mut bytes, 0.25f32.to_bits());
            word(&mut bytes, 0.75f32.to_bits());
            for generation in [3u16, 513] {
                bytes.extend(if little {
                    generation.to_le_bytes()
                } else {
                    generation.to_be_bytes()
                });
            }
            bytes.push(0);
            let curves = Curves::read(&mut Reader {
                bytes: &bytes,
                at: 0,
                little,
            })?;
            assert_eq!(curves.envelope.grow, 0.25);
            assert_eq!(curves.envelope.shrink, 0.75);
            assert_eq!(curves.envelope.grow_generation, 3);
            assert_eq!(curves.envelope.shrink_generation, 513);
            let legacy = &bytes[..bytes.len() - 1];
            for user in [9, 12] {
                let curves = Curves::read_kind(
                    &mut Reader {
                        bytes: legacy,
                        at: 0,
                        little,
                    },
                    false,
                    user,
                )?;
                assert_eq!(curves.envelope.grow_generation, 3);
                assert_eq!(curves.envelope.shrink_generation, 513);
            }
            assert!(Curves::read_kind(
                &mut Reader {
                    bytes: legacy,
                    at: 0,
                    little
                },
                false,
                13
            )
            .is_err());
            assert!(Curves::read_kind(
                &mut Reader {
                    bytes: &bytes,
                    at: 0,
                    little
                },
                false,
                12
            )
            .is_err());
        }
        Ok(())
    }
    #[test]
    fn final_step_preserves_death_boundary_and_spawn_timestamp() -> Result<()> {
        let mut motion = BirthKinematics {
            age: 0.5,
            life: 1.,
            position: [1., 2., 3.],
            velocity: [2., 0., 0.],
            last_update: 10.,
        };
        let flags = motion.final_step(10.5, 7, None)?;
        assert_eq!(flags, 7);
        assert_eq!(motion.age, 1.);
        assert_eq!(motion.position, [2., 2., 3.]);
        let flags = motion.final_step(10.75, flags, Some(513))?;
        assert_eq!(flags & 0xfff, 7);
        assert_eq!((flags >> 12) & 0xfff, 513);
        assert_eq!(flags & 0x03000000, 0x03000000);
        assert_eq!(motion.last_update, 10.5);
        assert_eq!(motion.position, [2.5, 2., 3.]);
        Ok(())
    }
    #[test]
    fn pending_resolution_preserves_native_tail_fill_order() -> Result<()> {
        let particle = |tag: f32| InitialParticle {
            motion: BirthKinematics {
                age: 0.,
                life: 1.,
                position: [tag, 0., 0.],
                velocity: [0.; 3],
                last_update: 0.,
            },
            radius: 1.,
            size: 1.,
            rotation: InitialRotation {
                angles: None,
                axis: None,
            },
        };
        let mut active = vec![particle(0.)];
        let mut pending = PendingBirths {
            capacity: 8,
            active: 3,
            particles: vec![particle(3.), particle(4.), particle(5.)],
        };
        pending.resolve_into(&mut active)?;
        assert_eq!(
            active
                .iter()
                .map(|p| p.motion.position[0])
                .collect::<Vec<_>>(),
            vec![0., 5., 4., 3.]
        );
        assert_eq!(pending.active, 4);
        assert!(pending.particles.is_empty());
        active.truncate(1);
        pending.particles.push(particle(6.));
        pending.resolve_into(&mut active)?;
        assert_eq!(
            active
                .iter()
                .map(|p| p.motion.position[0])
                .collect::<Vec<_>>(),
            vec![0., 6.]
        );
        assert_eq!(pending.active, 2);
        pending.resolve_into(&mut active)?;
        assert_eq!(active.len(), 2);
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_simulator_plans() -> Result<()> {
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            root.join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let mut rows = Vec::new();
        for file in &pak.files {
            if !file.path.to_ascii_lowercase().ends_with(".nif_wii") {
                continue;
            }
            let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            for (index, block) in nif.blocks.iter().enumerate() {
                if block.type_name() != "NiPSSimulator" {
                    continue;
                }
                let plan = SimulatorPlan::load(&nif, index as u32)
                    .with_context(|| format!("{} simulator {index}", file.path))?;
                let mut steps = Vec::new();
                for link in plan.steps {
                    if link == u32::MAX {
                        continue;
                    }
                    let kind = nif.blocks[link as usize].type_name();
                    let mut children = Vec::new();
                    if matches!(
                        kind,
                        "NiPSSimulatorForcesStep" | "NiPSSimulatorCollidersStep"
                    ) {
                        let mut r = raw(&nif, link, kind)?;
                        let count = r.uint()?;
                        for _ in 0..count {
                            let child = r.uint()?;
                            children.push(
                                nif.blocks
                                    .get(child as usize)
                                    .context("Invalid simulator child")?
                                    .type_name()
                                    .to_owned(),
                            );
                        }
                        r.done()?;
                    }
                    steps.push(serde_json::json!({"type":kind,"children":children}));
                }
                rows.push(serde_json::json!({"asset":file.path,"block":index,"submit":plan.submit,"complete":plan.complete,"steps":steps}));
            }
        }
        ensure!(!rows.is_empty(), "No simulators audited");
        println!("Simulator plans: {}", rows.len());
        std::fs::write(
            "target/particle-simulator-audit.json",
            serde_json::to_vec_pretty(&rows)?,
        )?;
        Ok(())
    }
    use super::*;
    #[test]
    fn em1_emitter_version_fields_and_endianness() {
        for little in [false, true] {
            let mut bytes = Vec::new();
            let word = |out: &mut Vec<u8>, v: u32| {
                out.extend(if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                })
            };
            word(&mut bytes, 11);
            for value in [
                200f32,
                40.,
                0.,
                0.,
                0.017453292,
                0.,
                0.017453292,
                0.8,
                0.25,
                1.,
                0.,
                0.,
                0.,
                0.,
                0.,
                0.,
                0.,
                0.,
            ] {
                word(&mut bytes, value.to_bits());
            }
            bytes.extend([0, 0, 1]);
            word(&mut bytes, 38);
            for _ in 0..3 {
                word(&mut bytes, 0);
            }
            assert_eq!(bytes.len(), 95);
            let e = Emitter::read(&mut Reader {
                bytes: &bytes,
                at: 0,
                little,
            })
            .unwrap();
            assert_eq!(
                (e.speed, e.speed_var, e.size, e.life, e.node),
                (200., 40., 0.8, 1., 38)
            );
            for len in 0..bytes.len() {
                assert!(Emitter::read(&mut Reader {
                    bytes: &bytes[..len],
                    at: 0,
                    little
                })
                .is_err());
            }
            bytes.push(0);
            assert!(Emitter::read(&mut Reader {
                bytes: &bytes,
                at: 0,
                little
            })
            .is_err());
        }
    }
    #[test]
    fn age_scaled_curve_clamps_and_interpolates() {
        let keys = [(0., [1., 0.]), (0.5, [0.5, 1.]), (1., [0., 0.])];
        assert_eq!(sample(&keys, -1.), [1., 0.]);
        assert_eq!(sample(&keys, 0.25), [0.75, 0.5]);
        assert_eq!(sample(&keys, 0.75), [0.25, 0.5]);
        assert_eq!(sample(&keys, 2.), [0., 0.]);
    }
}
/// NiPSAlignedQuadGenerator fields, verified by EM1 LoadBinary804cb5c0.
/// Native derived factors are amount/(limit-rest), with epsilon38d1b717.
/// These are not ordinary billboard width/height multipliers.
#[derive(Clone, Copy, Debug)]
struct QuadAlignment {
    u: [f32; 3],
    v: [f32; 3],
    center: [f32; 2],
    factors: [f32; 2],
}
impl QuadAlignment {
    /// EM1 no-rotation kernel804f5a64..5aa4. Both zero factors bypass velocity
    /// and use unit scales. Otherwise preserve its rational, fused arithmetic.
    fn scales(&self, velocity: [f32; 3]) -> Result<[f32; 2]> {
        if self.factors == [0.; 2] {
            return Ok([1.; 2]);
        }
        ensure!(
            velocity.iter().all(|v| v.is_finite()),
            "Invalid aligned particle velocity"
        );
        // NiPoint3::Length805aea20: paired dot, double frsqrte estimate,
        // single products and one Newton refinement, without an initial frsp.
        let squared =
            velocity[2].mul_add(velocity[2], velocity[0] * velocity[0]) + velocity[1] * velocity[1];
        ensure!(squared.is_finite(), "Aligned particle speed overflow");
        let speed = if squared == 0. {
            0.
        } else {
            let estimate = super::pose_blend::estimate_double(squared);
            let square = super::pose_blend::square_estimate(estimate);
            let half = (estimate * 0.5) as f32;
            let correction = (-squared).mul_add(square, 3.);
            squared * (correction * half)
        };
        let scales = std::array::from_fn(|axis| {
            let values = if axis == 0 { self.u } else { self.v };
            let weighted = self.factors[axis] * speed;
            values[1].mul_add(weighted, values[2]) / self.factors[axis].mul_add(speed, 1.)
        });
        ensure!(
            scales.iter().all(|v| v.is_finite()),
            "Nonfinite aligned-quad scale"
        );
        Ok(scales)
    }
    fn read(reader: &mut Reader) -> Result<Self> {
        let u = [reader.float()?, reader.float()?, reader.float()?];
        let v = [reader.float()?, reader.float()?, reader.float()?];
        let center = [reader.float()?, reader.float()?];
        let factor = |values: [f32; 3]| {
            let delta = values[1] - values[2];
            if delta.abs() < f32::from_bits(0x38d1b717) {
                0.
            } else {
                values[0] / delta
            }
        };
        let factors = [factor(u), factor(v)];
        ensure!(
            factors.iter().all(|v| v.is_finite()),
            "Nonfinite aligned-quad factor"
        );
        Ok(Self {
            u,
            v,
            center,
            factors,
        })
    }
}
#[derive(Clone)]
struct Particle {
    p: [f32; 3],
    v: [f32; 3],
    age: f32,
    life: f32,
    size: f32,
    height: Option<f32>,
    frame: u32,
    rotation: (f32, f32),
    last_update: f32,
}
#[derive(Clone, Copy, Default)]
struct AtlasAnimation {
    enabled: bool,
    start_frame: u16,
    frame_count: u16,
    frame_rate: f32,
}
impl AtlasAnimation {
    fn frame(self, initial: u32, age: f32, atlas: [u16; 2]) -> u32 {
        let total = u32::from(atlas[0]) * u32::from(atlas[1]);
        if total == 0 {
            return 0;
        }
        let start = u32::from(self.start_frame) % total;
        if !self.enabled || self.frame_count == 0 || !self.frame_rate.is_finite() {
            return (start + initial) % total;
        }
        let count = u32::from(self.frame_count).min(total).max(1);
        let step = (age.max(0.0) * self.frame_rate).floor();
        let step = if step.is_finite() { step as i64 } else { 0 };
        let local = (initial as i64 + step).rem_euclid(count as i64) as u32;
        (start + local) % total
    }
}
#[cfg(test)]
mod atlas_animation_tests {
    use super::AtlasAnimation;

    #[test]
    fn animated_atlas_cycles_inside_authored_frame_range() {
        let animation = AtlasAnimation {
            enabled: true,
            start_frame: 2,
            frame_count: 2,
            frame_rate: 2.0,
        };
        assert_eq!(animation.frame(0, 0.0, [2, 2]), 2);
        assert_eq!(animation.frame(0, 0.5, [2, 2]), 3);
        assert_eq!(animation.frame(0, 1.0, [2, 2]), 2);
        assert_eq!(animation.frame(1, 0.0, [2, 2]), 3);
    }

    #[test]
    fn inactive_atlas_keeps_authored_start_frame() {
        let animation = AtlasAnimation {
            enabled: false,
            start_frame: 3,
            frame_count: 0,
            frame_rate: 0.0,
        };
        assert_eq!(animation.frame(0, 10.0, [2, 2]), 3);
    }
}
#[derive(Clone)]
struct Layer {
    node: u32,
    nonuniform: Option<nonuniform::System>,
    custom_quad: bool,
    texture_seeds: bool,
    world_space: bool,
    system_local: [f32; 16],
    emitter_in_system: [f32; 16],
    // Native volume velocity uses rotation alone, including reflected and
    // collapsed emitter scales. Recovering axes from a flattened matrix loses it.
    native_emitter: Option<super::native_transform_record::Transform>,
    render_transform: [f32; 16],
    emitter: Emitter,
    mesh_surfaces: Option<Vec<super::particle_mesh::Surface>>,
    curves: Curves,
    playback: EmissionPlayback,
    enabled: bool,
    command_driven: bool,
    capacity: usize,
    scale: f32,
    particles: Vec<Particle>,
    draw: GpuDrawCommand,
    atlas: [u16; 2],
    frame_var: f32,
    atlas_animation: AtlasAnimation,
    indirect: bool,
    alignment: QuadAlignment,
    up_method: u32,
    facing: bool,
    emitter_local: [f32; 16],
    rotations: bool,
    forces: Vec<fields::Force>,
    prepared_forces: Vec<fields::Prepared>,
    started_at: f32,
}

// NiTransform uniform scale is retained for world-space particle systems.
// Flattened attachment matrices require recovering its sign as well as length.
fn attachment_scale(matrix: &[f32; 16]) -> Result<f32> {
    let length=(matrix[0]*matrix[0]+matrix[1]*matrix[1]+matrix[2]*matrix[2]).sqrt();
    let determinant=matrix[0]*(matrix[5]*matrix[10]-matrix[9]*matrix[6])
        -matrix[4]*(matrix[1]*matrix[10]-matrix[9]*matrix[2])
        +matrix[8]*(matrix[1]*matrix[6]-matrix[5]*matrix[2]);
    ensure!(length>0. && length.is_finite() && determinant.is_finite() && determinant!=0.,"Singular particle attachment frame");
    Ok(if determinant<0. {-length} else {length})
}

/// Camera-normal / local-velocity specialization used by the shipped hoses.
/// DOL 804f65a0 onward crosses transformed velocity with the facing normal,
/// normalizes the cross product, then crosses normal with that result. A
/// parallel velocity collapses the quad rather than inventing a fallback axis.
fn velocity_quad_axes(right: [f32; 3], up: [f32; 3], velocity: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }
    let normal = cross(right, up);
    let width = cross(velocity, normal);
    let length = width.iter().map(|v| v * v).sum::<f32>().sqrt();
    let width = if length > f32::from_bits(0x358637bd) {
        width.map(|v| v / length)
    } else {
        [0.; 3]
    };
    (width, cross(normal, width))
}
#[derive(Clone)]
pub struct Hose {
    layers: Vec<Layer>,
    random: u32,
    time: f32,
    absolute_time: Option<f32>,
    pub warnings: Vec<String>,
    colors: Option<Colors>,
}
#[derive(Clone)]
struct Colors {
    hierarchy: Arc<super::scene_bounds::Hierarchy>,
    objects: std::collections::BTreeMap<u32, super::render_object::State>,
}
#[cfg(test)]
mod live_hose_tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gremlin_emission_binds_by_target_and_name_through_controller_chains() -> Result<()>
    {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let files = crate::pack_io::read_filtered(
            std::fs::read(root.join("packfiles/gremlinvillage_zonea.pak"))?,
            |_, path| {
                [
                    "Effects/_Shared/fx_gremlin_warp.nif_wii",
                    "Effects/_Shared/fx_gremlin_warp_in.nif_wii",
                ]
                .iter()
                .any(|wanted| path.eq_ignore_ascii_case(wanted))
            },
        )?
        .pak
        .files;
        let mut systems = 0;
        let mut property_heads = 0;
        let mut births = 0;
        for file in files.iter().filter(|file| !file.data.is_empty()) {
            let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            for (index, block) in nif.blocks.iter().enumerate() {
                let NiObject::NiPSParticleSystem(system) = block else {
                    continue;
                };
                let head = system.base.base.base.base.controller_link;
                for emitter in system.emitter_links.iter().flatten() {
                    let kind = nif.blocks[*emitter as usize].type_name();
                    let mut source = raw(&nif, *emitter, kind)?;
                    let name = &nif.string_table[source.uint()? as usize];
                    let (mut playback, chain) =
                        EmissionPlayback::for_emitter(&nif, index as u32, head, name)?;
                    assert_eq!(playback.controller.target, index as u32);
                    assert_eq!(
                        &nif.string_table[playback.controller.emitter_name as usize],
                        name
                    );
                    if chain[0].class_name == "NiPSFieldMagnitudeCtlr" {
                        property_heads += 1;
                        assert_eq!(chain.len(), 2);
                    }
                    playback.start();
                    let before = births;
                    for frame in 0..=60 {
                        let time = frame as f32 / 30.;
                        playback.update(
                            time,
                            true,
                            Some((time - 1. / 30.).max(0.)),
                            |_, _, ages| {
                                births += ages.count();
                                Ok(())
                            },
                        )?;
                    }
                    assert!(
                        births > before,
                        "No authored emission for {}:{index}:{name}",
                        file.path
                    );
                    assert!(EmissionPlayback::for_emitter(
                        &nif,
                        index as u32,
                        head,
                        "BwxMissingEmitter"
                    )
                    .is_err());
                    let mut malformed =
                        NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
                    let head = head.context("Missing source controller")?;
                    let NiObject::Unknown(controller) = &mut malformed.blocks[head as usize] else {
                        anyhow::bail!("Expected controller prefix")
                    };
                    controller.raw_data[..4].copy_from_slice(&head.to_be_bytes());
                    assert!(
                        EmissionPlayback::for_emitter(&malformed, index as u32, Some(head), name)
                            .is_err(),
                        "A cyclic list was admitted"
                    );
                    systems += 1;
                }
            }
        }
        assert_eq!(systems, 7);
        assert_eq!(property_heads, 5);
        assert!(births > 0);
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_guardian_local_particles_follow_owner_without_advancing_simulation() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut files = Vec::new();
        for name in ["_dynamic.pak", "r3mt.pak"] {
            files.extend(
                crate::pack_io::read(std::fs::read(
                    std::path::Path::new(&root).join("packfiles").join(name),
                )?)?
                .pak
                .files,
            );
        }
        let file = files
            .iter()
            .find(|file| {
                file.path
                    .eq_ignore_ascii_case("Effects/_Shared/fx_guardian_paint.nif_wii")
            })
            .context("Missing original guardian")?;
        let mut effect = Hose::load_attached(&file.data, &files)?;
        for layer in &effect.layers {
            let program = layer.draw.preview.as_ref().unwrap().as_ref().unwrap();
            assert!(program.vertex.contains("v_toon="));
            assert!(program.fragment.contains("vec3(v_toon,1.0)"));
            let gradient = layer
                .draw
                .material_images
                .iter()
                .find(|image| image.slot == 1)
                .unwrap();
            assert!(
                gradient
                    .rgba
                    .chunks_exact(4)
                    .any(|color| color[0] != color[1] || color[1] != color[2]),
                "Guardian palette decoded as grayscale"
            );
            let glow = layer
                .draw
                .material_images
                .iter()
                .find(|image| image.slot == 0)
                .unwrap();
            assert!(
                glow.rgba.chunks_exact(4).any(|color| color[3] < 255),
                "Guardian glow lost its intensity alpha"
            );
        }
        let mut owner = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 10., 20., 30., 1.,
        ];
        for _ in 0..8 {
            effect.advance_attached(1. / 30., true, owner)?;
        }
        let old: Vec<_> = effect
            .layers
            .iter()
            .map(|layer| {
                layer
                    .particles
                    .iter()
                    .map(|particle| particle.p)
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut before = Vec::new();
        effect.append([1., 0., 0.], [0., 1., 0.], &mut before)?;
        let local_draw = effect
            .layers
            .iter()
            .filter(|layer| !layer.particles.is_empty() && !layer.indirect)
            .position(|layer| !layer.world_space)
            .context("No populated local-space layer")?;
        let time = effect.time;
        owner[12] += 5.;
        effect.advance_attached(0., true, owner)?;
        assert_eq!(effect.time, time);
        for (layer, positions) in effect.layers.iter().zip(old) {
            assert_eq!(
                layer
                    .particles
                    .iter()
                    .map(|particle| particle.p)
                    .collect::<Vec<_>>(),
                positions
            );
        }
        let mut after = Vec::new();
        effect.append([1., 0., 0.], [0., 1., 0.], &mut after)?;
        assert_eq!(before.len(), after.len());
        for (before, after) in before[local_draw]
            .v_data
            .chunks_exact(12)
            .zip(after[local_draw].v_data.chunks_exact(12))
        {
            assert!((after[0] - before[0] - 5.).abs() < 1e-5);
            assert_eq!(before[1], after[1]);
            assert_eq!(before[2], after[2]);
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_spin_layers_emit_after_delayed_activation_and_expire() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = crate::blotworx::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let effect = world
            .spin_particles
            .as_mut()
            .context("Missing attached spin particles")?;
        assert_eq!(effect.layers.len(), 2);
        assert_eq!(
            effect.layers.iter().map(|l| l.capacity).collect::<Vec<_>>(),
            [45, 30]
        );
        assert!(effect.layers[0].forces.is_empty());
        assert_eq!(effect.layers[1].forces.len(), 1);
        assert_eq!(effect.layers[1].forces[0].kind, fields::Kind::Drag);
        assert_eq!(effect.layers[1].forces[0].magnitude, 3.);
        let mut attachment = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 10., 20., 30., 1.,
        ];
        effect.advance_attached(8., false, attachment)?;
        let mut seen = [false; 2];
        for _ in 0..20 {
            attachment[12] += 0.01;
            effect.advance_attached(1. / 30., true, attachment)?;
            for (i, layer) in effect.layers.iter().enumerate() {
                seen[i] |= !layer.particles.is_empty();
                assert!(layer.particles.len() <= layer.capacity);
                for particle in &layer.particles {
                    assert!(particle.p.iter().all(|v| v.is_finite()));
                    assert!(
                        particle.p[0] > 8. && particle.p[1] > 18. && particle.p[2] > 28.,
                        "Emitter lost its world attachment"
                    );
                    assert!(particle.rotation.0.is_finite());
                }
            }
        }
        assert_eq!(
            seen,
            [true, true],
            "Original auxiliary effect layer emitted nothing"
        );
        let mut draws = Vec::new();
        effect.append([1., 0., 0.], [0., 1., 0.], &mut draws)?;
        assert_eq!(draws.len(), 2);
        assert!(draws
            .iter()
            .all(|d| !d.indices.is_empty() && d.v_data.iter().all(|v| v.is_finite())));
        for _ in 0..60 {
            effect.advance_attached(1. / 30., false, attachment)?;
        }
        draws.clear();
        effect.append([1., 0., 0.], [0., 1., 0.], &mut draws)?;
        assert!(draws.is_empty());
        // Pop is authored to stay off until 0.583333s, unlike sparkles.
        for _ in 0..8 {
            effect.advance_attached(1. / 30., true, attachment)?;
        }
        assert!(!effect.layers[0].particles.is_empty());
        assert!(
            effect.layers[1].particles.is_empty(),
            "Pop emitted before its authored activation key"
        );
        for _ in 8..20 {
            effect.advance_attached(1. / 30., true, attachment)?;
        }
        assert!(
            effect.layers.iter().all(|l| !l.particles.is_empty()),
            "Repeated activation failed to restart effect clock"
        );
        Ok(())
    }
    #[test]
    fn hose_velocity_alignment_follows_projected_travel_and_collapses_parallel_motion() {
        assert_eq!(
            velocity_quad_axes([1., 0., 0.], [0., 1., 0.], [8., 0., 0.]),
            ([0., -1., 0.], [1., 0., -0.])
        );
        assert_eq!(
            velocity_quad_axes([1., 0., 0.], [0., 1., 0.], [0., 0., 8.]),
            ([0.; 3], [0.; 3])
        );
    }
    #[test]
    fn aligned_scale_uses_native_rational_limit_and_joint_zero_bypass() -> Result<()> {
        let quad = QuadAlignment {
            u: [1., 1., 1.],
            v: [1., 3., 1.],
            center: [0.; 2],
            factors: [0., 0.5],
        };
        assert_eq!(quad.scales([0.; 3])?, [1., 1.]);
        let at_two = quad.scales([0., 0., 2.])?;
        // The recovered Gekko length/refinement gives a one-ULP-lower result
        // than ideal host sqrt here. Keep the instruction-derived float bits.
        assert_eq!(at_two.map(f32::to_bits), [0x3f800000, 0x3fffffff]);
        let fast = quad.scales([0., 0., 200.])?;
        ensure!(
            fast[1] > 2.98 && fast[1] < 3.,
            "Quad did not approach its authored limit"
        );
        let disabled = QuadAlignment {
            u: [0., 4., 2.],
            v: [0., 5., 3.],
            factors: [0.; 2],
            ..quad
        };
        assert_eq!(disabled.scales([f32::NAN; 3])?, [1.; 2]);
        ensure!(
            quad.scales([f32::NAN; 3]).is_err(),
            "Active scale accepted invalid velocity"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_hoses_use_full_radius_variation_and_expire_after_release() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = crate::blotworx::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        ensure!(
            world.brush.hoses.len() == 2,
            "Original brush streams missing"
        );
        for hose in &mut world.brush.hoses {
            for layer in &hose.layers {
                assert_eq!(layer.alignment.u, [1., 1., 1.]);
                assert_eq!(layer.alignment.center, [0., 0.]);
                assert_eq!(layer.alignment.factors[0], 0.);
                ensure!(
                    layer.alignment.v == [1., 2., 1.] || layer.alignment.v == [1., 3., 1.],
                    "Unexpected original aligned-quad V settings"
                );
                assert_eq!(
                    layer.alignment.factors[1],
                    if layer.alignment.v[1] == 2. { 1. } else { 0.5 }
                );
            }
            let mut wider_than_old_preview = false;
            let mut births_seen = false;
            for _ in 0..120 {
                hose.advance(1. / 30., true, [1., 2., 3.], [0., 0., 1.])?;
                for layer in &hose.layers {
                    ensure!(
                        layer.particles.len() <= layer.capacity,
                        "Live emission exceeded authored capacity"
                    );
                    for particle in &layer.particles {
                        births_seen = true;
                        ensure!(
                            particle.p.iter().chain(&particle.v).all(|v| v.is_finite())
                                && particle.life.is_finite()
                                && particle.age <= particle.life,
                            "Invalid live particle"
                        );
                        let radius = particle.size / layer.scale;
                        ensure!(
                            radius >= layer.emitter.size - layer.emitter.size_var - 0.00001
                                && radius <= layer.emitter.size + layer.emitter.size_var + 0.00001,
                            "Live radius outside authored variation"
                        );
                        if layer.emitter.size_var > 0. {
                            wider_than_old_preview |=
                                (radius - layer.emitter.size).abs() > 0.5 * layer.emitter.size_var;
                        }
                    }
                }
            }
            ensure!(
                births_seen && wider_than_old_preview,
                "Live renderer did not consume the recovered birth/radius path"
            );
            let mut draws = Vec::new();
            hose.append([1., 0., 0.], [0., 1., 0.], &mut draws)?;
            ensure!(
                !draws.is_empty(),
                "Original hose produced no live quad geometry"
            );
            for draw in &draws {
                ensure!(draw.v_data.len() % 48 == 0, "Malformed live particle quads");
                for quad in draw.v_data.chunks_exact(48) {
                    let width = (0..3)
                        .map(|i| (quad[12 + i] - quad[i]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    let height = (0..3)
                        .map(|i| (quad[24 + i] - quad[12 + i]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    if width > 0.00001 {
                        ensure!(
                            height > width * 1.9,
                            "Original quad lost authored speed stretch"
                        );
                    }
                }
            }
            for _ in 0..90 {
                hose.advance(1. / 30., false, [1., 2., 3.], [0., 0., 1.])?;
            }
            ensure!(
                hose.layers.iter().all(|layer| layer.particles.is_empty()),
                "Released hose retained expired particles"
            );
        }
        Ok(())
    }
}
pub(super) fn authored_registers(
    n: &NifFile,
    links: &[Option<u32>],
) -> Result<crate::tev::Registers> {
    let mut registers = crate::tev::Registers::default();
    for link in links.iter().flatten() {
        if let Some(NiObject::Unknown(extra)) = n.blocks.get(*link as usize) {
            if extra.type_name == "NiColorExtraData" {
                let mut r = raw(n, *link, "NiColorExtraData")?;
                let name = n
                    .string_table
                    .get(r.uint()? as usize)
                    .context("Missing particle color name")?;
                let slot = name
                    .strip_prefix("Constant_Color_")
                    .context("Unsupported particle color register")?
                    .parse::<usize>()?;
                let mut color = [0; 4];
                for c in &mut color {
                    *c = ((r.float()? * 255.) as i32) as u8;
                }
                r.done()?;
                *registers
                    .constants
                    .get_mut(slot)
                    .context("Invalid particle constant slot")? = Some(color);
            }
        }
    }
    Ok(registers)
}

impl Hose {
    pub fn owns_controller(&self, block: u32) -> bool {
        self.layers.iter().any(|layer| {
            layer.playback.block == block
                || layer
                    .forces
                    .iter()
                    .any(|force| force.controls.iter().any(|c| c.block == block))
        })
    }
    /// StartAnimations includes force/property controllers too; the narrower
    /// START_EMITTERS command deliberately does not call this method.
    pub fn start_controller(&mut self, block: u32, time: f32) -> Result<()> {
        ensure!(
            self.owns_controller(block),
            "Particle controller {block} has no owner"
        );
        for layer in &mut self.layers {
            if layer.playback.block == block {
                layer.command_driven = true;
                layer.playback.native_command(Some(time));
            }
            for force in &mut layer.forces {
                for control in &mut force.controls {
                    if control.block == block {
                        control.start_at(time);
                    }
                }
            }
        }
        Ok(())
    }
    /// StopAnimations804bf624 stops controller playback, not particles or
    /// force values already published by those controllers.
    pub fn stop_controller(&mut self, block: u32) -> Result<()> {
        ensure!(
            self.owns_controller(block),
            "Particle controller {block} has no owner"
        );
        for layer in &mut self.layers {
            if layer.playback.block == block {
                layer.command_driven = true;
                layer.playback.native_command(None);
            }
            for force in &mut layer.forces {
                for control in &mut force.controls {
                    if control.block == block {
                        control.stop();
                    }
                }
            }
        }
        Ok(())
    }
    ///15b620 changes controller flags while registering an effect template.
    /// Only controllers reachable from the selected stream root are changed.
    pub fn prepare_effect_template(&mut self, controllers: &std::collections::BTreeSet<u32>) {
        for layer in &mut self.layers {
            if controllers.contains(&layer.playback.block) {
                layer.playback.controller.flags =
                    super::nif_runtime::effect_template_flags(layer.playback.controller.flags);
            }
            for force in &mut layer.forces {
                for control in &mut force.controls {
                    if controllers.contains(&control.block) {
                        control.prepare_effect_template();
                    }
                }
            }
        }
    }
    pub fn supports_emitter_command(&self, block: u32) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.playback.block == block)
    }
    #[cfg(any(test, debug_assertions))]
    pub fn emitter_command_state(&self) -> Vec<(u32, bool, Option<f32>, bool)> {
        self.layers
            .iter()
            .map(|l| {
                (
                    l.playback.block,
                    l.playback.controller.flags & 8 != 0,
                    l.playback.application_start,
                    l.command_driven,
                )
            })
            .collect()
    }
    /// Dispatch to existing per-instance playback state; do not reset field
    /// forces, particles, or the particle-system clock on emitter START/STOP.
    pub fn emitter_command(&mut self, block: u32, time: Option<f32>) -> Result<()> {
        ensure!(
            self.supports_emitter_command(block),
            "Emitter controller {block} has no playback owner"
        );
        for layer in self
            .layers
            .iter_mut()
            .filter(|layer| layer.playback.block == block)
        {
            layer.command_driven = true;
            layer.playback.native_command(time);
        }
        Ok(())
    }
    /// Decoded resources are shared by the draw commands. Mutable clocks,
    /// material overrides, emission state and particles belong to each owner.
    pub fn instantiate(&self) -> Result<Self> {
        ensure!(
            self.absolute_time.is_none()
                && self.time == 0.
                && self
                    .layers
                    .iter()
                    .all(|layer| !layer.enabled && layer.particles.is_empty()),
            "Particle prototype has already been played"
        );
        Ok(self.clone())
    }
    /// Match the selected NiNode clone, rather than playing systems from
    /// unrelated NIF stream roots. Immutable decoded draw data remains shared.
    pub fn instantiate_subtree(
        &self,
        hierarchy: Arc<super::scene_bounds::Hierarchy>,
    ) -> Result<Self> {
        let mut instance = self.instantiate()?;
        instance
            .layers
            .retain(|layer| hierarchy.nodes.contains_key(&layer.node));
        if let Some(colors) = &mut instance.colors {
            colors
                .objects
                .retain(|node, _| hierarchy.nodes.contains_key(node));
            colors.hierarchy = hierarchy;
        }
        Ok(instance)
    }
    pub fn particle_count(&self) -> usize {
        self.layers.iter().map(|layer| layer.particles.len()).sum()
    }
    #[cfg(debug_assertions)]
    pub fn validate_retained_world_scale(&self, owner: [f32;16]) -> Result<()> {
        let mut changed=self.clone();
        let mut moved=owner;
        for column in 0..3 {for row in 0..3 {moved[column*4+row]*=2.;}}
        moved[12]+=8.;moved[13]-=4.;moved[14]+=2.;
        changed.attach_for_render(Some(&moved))?;
        let mut checked=0;
        for (before,after) in self.layers.iter().zip(&changed.layers) {
            if !before.world_space {continue;}
            for (a,b) in before.particles.iter().zip(&after.particles) {
                for axis in 0..3 {
                    let old=a.p[axis]*before.render_transform[axis*5];
                    let new=b.p[axis]*after.render_transform[axis*5];
                    ensure!(old.to_bits()==new.to_bits(),"Scale completion moved existing world particle");
                    ensure!(a.v[axis].to_bits()==b.v[axis].to_bits(),"Scale completion changed native velocity storage");
                }
                ensure!(a.size.to_bits()==b.size.to_bits(),"Scale completion changed native radius storage");
                checked+=1;
            }
        }
        ensure!(checked>0,"Scale validation requires populated world-space particles");
        Ok(())
    }
    /// Bounded live-data probe for render diagnosis: one actual particle per
    /// system, without changing simulation, draw submission or asset values.
    pub fn render_probe(&self) -> serde_json::Value {
        serde_json::json!(self.layers.iter().map(|layer| {
            let first=layer.particles.first().map(|p| {
                let world: [f32;3]=std::array::from_fn(|row|
                    layer.render_transform[12+row]+(0..3).map(|col|layer.render_transform[col*4+row]*p.p[col]).sum::<f32>());
                serde_json::json!({"position_system":p.p,"position_world":world,"velocity_system":p.v,
                    "age":p.age,"life":p.life,"radius":p.size,"size_factor":sample(&layer.curves.size,p.age/p.life)[0],
                    "color":layer.curves.color_at(p.age/p.life),"atlas_frame":p.frame})
            });
            let texture=layer.draw.tex_bytes.as_ref().map(|(pixels,width,height)| {
                let alpha=pixels.chunks_exact(4).map(|p|p[3]).fold([255u8,0u8],|a,v|[a[0].min(v),a[1].max(v)]);
                serde_json::json!({"size":[width,height],"alpha_range":alpha})
            });
            serde_json::json!({"system":layer.node,"particles":layer.particles.len(),"world_space":layer.world_space,
                "indirect_omitted":layer.indirect,"system_scale":layer.scale,"render_transform":layer.render_transform,
                "first_particle":first,"texture":texture,"material":layer.draw.material.as_ref().map(|m|serde_json::json!({
                    "shader":m.shader,"colors":m.legacy_colors,"vertex_colors":m.has_vertex_colors,
                    "vertex_source":format!("{:?}",m.vertex_color_flags),"alpha_flags":m.legacy_alpha_flags,
                    "depth_flags":m.legacy_depth_flags,"stencil_flags":m.legacy_stencil_flags}))})
        }).collect::<Vec<_>>())
    }
    pub fn mesh_emission_summary(&self) -> Vec<serde_json::Value> {
        self.layers
            .iter()
            .filter_map(|layer| {
                layer.mesh_surfaces.as_ref().map(|sources| {
                    serde_json::json!({"system":layer.node,"particles":layer.particles.len(),
                "sources":sources.iter().map(|s|s.summary()).collect::<Vec<_>>()})
                })
            })
            .collect()
    }
    pub fn publish_material(&self, node: u32, draw: &mut GpuDrawCommand) -> Result<()> {
        if let Some(state) = self
            .colors
            .as_ref()
            .and_then(|colors| colors.objects.get(&node))
        {
            state.apply(draw)?;
        }
        Ok(())
    }
    pub fn publish_model_transforms(
        &mut self,
        world: &std::collections::BTreeMap<u32, epic_mickey_lib_rs::nif_file::NiTransform>,
    ) -> Result<()> {
        for layer in &mut self.layers {
            let system = world
                .get(&layer.node)
                .context("Animated particle system transform missing")?;
            layer.system_local = super::nif_runtime::matrix(system);
            layer.scale = system.scale;
            if layer.mesh_surfaces.is_none() {
                let emitter = world
                    .get(&layer.emitter.node)
                    .context("Animated particle emitter transform missing")?;
                layer.emitter_local = super::nif_runtime::matrix(emitter);
            }
            let inverse = if layer.world_space {
                None
            } else {
                ensure!(
                    system.scale != 0.,
                    "Singular animated particle system scale"
                );
                let source = std::array::from_fn(|i| {
                    if i < 9 {
                        system.rotation.rows[i / 3][i % 3]
                    } else if i < 12 {
                        [
                            system.translation.x,
                            system.translation.y,
                            system.translation.z,
                        ][i - 9]
                    } else {
                        system.scale
                    }
                });
                let inverse = super::native_transform_record::inverse(&source);
                Some(super::nif_runtime::matrix(
                    &epic_mickey_lib_rs::nif_file::NiTransform {
                        rotation: epic_mickey_lib_rs::nif_file::Matrix3 {
                            rows: std::array::from_fn(|r| {
                                std::array::from_fn(|c| inverse[r * 3 + c])
                            }),
                        },
                        translation: epic_mickey_lib_rs::nif_file::Vector3 {
                            x: inverse[9],
                            y: inverse[10],
                            z: inverse[11],
                        },
                        scale: inverse[12],
                    },
                ))
            };
            layer.emitter_in_system = inverse.as_ref().map_or(layer.emitter_local, |inverse| {
                super::skin::matrix_product(inverse, &layer.emitter_local)
            });
            layer.native_emitter = if !layer.world_space && layer.mesh_surfaces.is_none() {
                let record = |t: &epic_mickey_lib_rs::nif_file::NiTransform| {
                    std::array::from_fn(|i| {
                        if i < 9 {
                            t.rotation.rows[i / 3][i % 3]
                        } else if i < 12 {
                            [t.translation.x, t.translation.y, t.translation.z][i - 9]
                        } else {
                            t.scale
                        }
                    })
                };
                Some(super::particle_space::relative(
                    &record(system),
                    &record(
                        world
                            .get(&layer.emitter.node)
                            .context("Particle emitter pose missing")?,
                    ),
                )?)
            } else {
                None
            };
            layer.scale = if layer.world_space { system.scale } else { 1. };
            if let Some(surfaces) = &mut layer.mesh_surfaces {
                for surface in surfaces {
                    surface.transform = super::nif_runtime::matrix(
                        world
                            .get(&surface.node)
                            .context("Animated mesh-emitter source missing")?,
                    );
                }
            }
            for force in &mut layer.forces {
                force.publish_model_transform(world, inverse.as_ref())?;
            }
        }
        Ok(())
    }
    pub fn load(bytes: &[u8], files: &[VirtualFile], _ctx: &egui::Context) -> Result<Self> {
        Self::load_layers(bytes, files, false, false)
    }
    /// Authored camera-facing layers attached to an animated effect owner.
    /// Ribbon systems in the same NIF are owned by `Ribbon`.
    pub fn load_attached(bytes: &[u8], files: &[VirtualFile]) -> Result<Self> {
        // Runtime keeps valid authored EM1 particle modes even when the
        // current renderer only has the common camera-facing quad path.
        // Unsupported modes are recorded below and use that bounded fallback
        // instead of preventing a gameplay effect from being instantiated.
        Self::load_layers(bytes, files, true, true)
    }
    /// Functional demo fallback: unsupported mesh distribution modes emit
    /// from the actual source faces. Keep strict/native admission separate.
    pub fn load_attached_for_demo(bytes: &[u8], files: &[VirtualFile]) -> Result<Self> {
        Self::load_layers(bytes, files, true, true)
    }
    fn load_layers(
        bytes: &[u8],
        files: &[VirtualFile],
        attached: bool,
        demo_mesh: bool,
    ) -> Result<Self> {
        let n = NifFile::load_bytes(bytes).map_err(anyhow::Error::msg)?;
        ensure!(
            n.version == 0x14060500
                && (n.user_version == 15 || (attached && (9..=14).contains(&n.user_version))),
            "Unsupported hose NIF version {:08x}/{}",
            n.version,
            n.user_version
        );
        let mut textures = HashMap::new();
        let mut layers = Vec::new();
        let mut warnings = Vec::new();
        for (index, block) in n.blocks.iter().enumerate() {
            let NiObject::NiPSParticleSystem(system) = block else {
                continue;
            };
            if attached
                && n.block_types[n.block_type_indices[index] as usize] == "RibbonParticleSystem"
            {
                continue;
            }
            let nonuniform = if n.block_types[n.block_type_indices[index] as usize]
                == "NonUniformParticleSystem"
            {
                Some(nonuniform::System::load(
                    &system.extension,
                    n.source_is_little_endian,
                    n.user_version,
                )?)
            } else {
                None
            };
            ensure!(
                !system.emitter_links.is_empty(),
                "Particle system has no emitters"
            );
            for emitter_link in &system.emitter_links {
                let nonuniform = nonuniform.clone();
                if attached && (system.normal_method != 16 || !matches!(system.up_method, 9 | 16)) {
                    warnings.push(format!("Particle alignment normal {}, up {} uses the camera-facing gameplay fallback; original alignment mode remains unimplemented",
                    system.normal_method,system.up_method));
                }
                let emitter_link = emitter_link.context("Null emitter")?;
                let emitter_kind = n.blocks[emitter_link as usize].type_name();
                let emitter =
                    Emitter::read_shape(&mut raw(&n, emitter_link, emitter_kind)?, emitter_kind)
                        .with_context(|| {
                            format!(
                                "{emitter_kind} block {emitter_link} for particle system {index}"
                            )
                        })?;
                let mesh_surfaces = if let EmitterVolume::Mesh {
                    links,
                    emission,
                    velocity,
                } = &emitter.volume
                {
                    ensure!(attached && (demo_mesh || (matches!(*emission,1|3) && *velocity==0)),"Mesh emitter requires supported attached face-center/surface and normal velocity modes (got {emission}/{velocity})");
                    if demo_mesh && (!matches!(*emission, 1 | 3) || *velocity != 0) {
                        warnings.push(format!("Demo mesh emission uses source faces/normals for distribution {emission}, velocity {velocity}; exact mode behavior remains incomplete"));
                    }
                    ensure!(!links.is_empty(), "Mesh emitter has no source geometry");
                    let warning="Rigid mesh-surface emission uses retail face/normal arithmetic; global RNG, transform arithmetic and original-frame parity remain unverified".to_string();
                    if !warnings.contains(&warning) {
                        warnings.push(warning);
                    }
                    Some(
                        links
                            .iter()
                            .map(|&node| super::particle_mesh::Surface::load(&n, node))
                            .collect::<Result<Vec<_>>>()?,
                    )
                } else {
                    None
                };
                if !attached {
                    ensure!(matches!(&emitter.volume,EmitterVolume::Box(v) if v.0==[0.;3]),"Hose preview requires a point emitter; scene volume transforms are not connected");
                    ensure!(
                        emitter.rotation.is_zero(),
                        "Rotating hose particle playback is not implemented"
                    );
                }
                ensure!(!attached || (system.flag5<=1 && system.flag3==0 && system.flag4<=1),
                "Attached particles do not support rotation axes (system {index}: flags3/4/5={}/{}/{})",system.flag3,system.flag4,system.flag5);
                let emitter_local = if mesh_surfaces.is_some() {
                    // In normals mode mesh transforms supply the spawn frame. The
                    // optional emitter object is only used by direction mode.
                    [
                        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                    ]
                } else {
                    let node = n
                        .blocks
                        .get(emitter.node as usize)
                        .and_then(NiObject::as_av_object)
                        .context("Missing particle emitter node")?;
                    super::brush_render::named_transform(&n, &node.base.name)?
                        .context("Missing particle emitter transform")?
                };
                let mut scale = 1.;
                // Size and velocity are particle-system-space quantities. Emitter
                // scale transforms its spawn volume, not its velocity (see
                // NiPSBoxEmitter::ComputeVolumeInitialPositionAndVelocity). Using
                // the emitter node here incorrectly scales the entire stream.
                let mut current = index;
                let mut seen = std::collections::HashSet::new();
                loop {
                    ensure!(seen.insert(current), "Cyclic emitter hierarchy");
                    let av = n
                        .blocks
                        .get(current)
                        .and_then(|b| b.as_av_object())
                        .context("Missing emitter transform")?;
                    scale *= av.transform.scale;
                    let parent = n
                        .blocks
                        .iter()
                        .position(|b| b.children().contains(&Some(current as u32)));
                    if let Some(parent) = parent {
                        current = parent;
                    } else {
                        break;
                    }
                }
                let system_local = super::brush_render::node_transform(&n, index)?;
                let world_space = !attached || system.flag5 != 0;
                let mut inverse_system = None;
                let emitter_in_system = if world_space {
                    emitter_local
                } else {
                    ensure!(
                        scale != 0. && scale.is_finite(),
                        "Singular local particle system scale"
                    );
                    // NiPSVolumeEmitter uses inverse(system) * emitter. Keep the
                    // native NiTransform inverse's transpose/reciprocal semantics.
                    let mut source = [0.; 13];
                    source[12] = scale;
                    for row in 0..3 {
                        for col in 0..3 {
                            source[row * 3 + col] = system_local[col * 4 + row] / scale;
                        }
                    }
                    source[9..12].copy_from_slice(&system_local[12..15]);
                    let inverse = super::native_transform_record::inverse(&source);
                    let mut matrix = [0.; 16];
                    matrix[15] = 1.;
                    for row in 0..3 {
                        for col in 0..3 {
                            matrix[col * 4 + row] = inverse[row * 3 + col] * inverse[12];
                        }
                    }
                    matrix[12..15].copy_from_slice(&inverse[9..12]);
                    inverse_system = Some(matrix);
                    super::skin::matrix_product(&matrix, &emitter_local)
                };
                if !world_space {
                    scale = 1.;
                }
                let mut source_emitter = raw(&n, emitter_link, emitter_kind)?;
                let emitter_name = n
                    .string_table
                    .get(source_emitter.uint()? as usize)
                    .context("Missing particle emitter name")?;
                let (mut playback, controller_chain) = EmissionPlayback::for_emitter(
                    &n,
                    index as u32,
                    system.base.base.base.base.controller_link,
                    emitter_name,
                )?;
                playback.stop();
                let sim = SimulatorPlan::load(
                    &n,
                    system
                        .simulator_link
                        .context("Missing particle simulator")?,
                )?;
                let mut curves = None;
                let mut forces = Vec::new();
                for link in sim.steps {
                    if let Some(NiObject::Unknown(step)) = n.blocks.get(link as usize) {
                        if matches!(
                            step.type_name.as_str(),
                            "NiPSSimulatorGeneralStep" | "NonUniformSimulatorGeneralStep"
                        ) {
                            let custom = step.type_name == "NonUniformSimulatorGeneralStep";
                            ensure!(
                                !custom || nonuniform.is_some(),
                                "Nonuniform general step requires its particle-system subclass"
                            );
                            curves = Some(
                                Curves::read_kind(
                                    &mut raw(&n, link, &step.type_name)?,
                                    custom,
                                    n.user_version,
                                )
                                .with_context(|| {
                                    format!(
                                        "{} block {link} for particle system {index}",
                                        step.type_name
                                    )
                                })?,
                            );
                        }
                        if step.type_name == "NiPSSimulatorForcesStep" && step.raw_data.len() > 4 {
                            if attached {
                                let mut r = raw(&n, link, &step.type_name)?;
                                for _ in 0..r.uint()? {
                                    forces.push(fields::Force::load(
                                        &n,
                                        r.uint()?,
                                        inverse_system.as_ref(),
                                    )?);
                                }
                                r.done()?;
                            } else {
                                warnings.push(
                                    "Hose drag force is not yet simulated in the visual preview"
                                        .into(),
                                );
                            }
                        }
                        if attached && step.type_name == "NiPSSimulatorCollidersStep" {
                            let mut r = raw(&n, link, &step.type_name)?;
                            ensure!(
                                r.uint()? == 0,
                                "Attached particle colliders are not implemented"
                            );
                            r.done()?;
                        }
                    }
                }
                for controller in controller_chain
                    .iter()
                    .filter(|c| c.class_name != "NiPSEmitParticlesCtlr")
                {
                    if controller.class_name == "NiPSFieldMagnitudeCtlr" {
                        let control = fields::Magnitude::load(&n, controller, index as u32)?;
                        let field = forces
                            .iter_mut()
                            .find(|force| force.name == control.name)
                            .with_context(|| {
                                format!(
                                    "Particle property controller has no force named {}",
                                    control.name
                                )
                            })?;
                        field.controls.push(control);
                    } else if controller.class_name.starts_with("NiPSEmitter") {
                        // Emitter-property interpolators target a different
                        // object than field-magnitude tracks. Keep the parsed
                        // emitter defaults live instead of rejecting its full
                        // effect until this property channel is evaluated.
                        warnings.push(format!(
                            "Emitter property controller {} uses its authored static value",
                            controller.class_name
                        ));
                    } else if controller.class_name == "NiPSResetOnLoopCtlr" {
                        // This chain entry is not a field-magnitude track. Keep
                        // the effect live; clearing its particles at controller
                        // wrap remains a separate simulation step.
                        warnings.push(
                            "Particle reset-on-loop is parsed; clearing live particles on controller wrap remains incomplete".into(),
                        );
                    } else {
                        anyhow::bail!(
                            "Unsupported particle property controller {}",
                            controller.class_name
                        );
                    }
                }
                let surface = nif::find_mesh_surface(&n, index);
                let mut bindings = surface.material;
                if attached {
                    bindings.has_vertex_colors = system.flag1 != 0;
                }
                if attached && bindings.shader.is_none() {
                    //80520950 reads the standard map array, not ShaderMaps. Do
                    // not let a same-slot shader map shadow the base texture.
                    bindings.textures.retain(|map| map.shader_id.is_none());
                }
                let material = Arc::new(bindings);
                let registers =
                    authored_registers(&n, &system.base.base.base.base.extra_data_links)?;
                let generator = system
                    .base
                    .modifiers
                    .iter()
                    .flatten()
                    .find(|link| {
                        let kind = n.blocks[**link as usize].type_name();
                        kind == "NiPSAlignedQuadGenerator"
                            || (attached
                                && matches!(
                                    kind,
                                    "NiPSFacingQuadGenerator" | "NonUniformQuadGenerator"
                                ))
                    })
                    .context("Missing quad generator")?;
                let generator_kind = n.blocks[*generator as usize].type_name();
                let custom_quad = generator_kind == "NonUniformQuadGenerator";
                ensure!(
                    !custom_quad || nonuniform.is_some(),
                    "Nonuniform quad generator requires its particle-system subclass"
                );
                let facing = matches!(
                    generator_kind,
                    "NiPSFacingQuadGenerator" | "NonUniformQuadGenerator"
                );
                let mut generator = raw(&n, *generator, generator_kind)?;
                generator.at = 12;
                let (alignment, atlas, frame_var, atlas_animation) = if custom_quad {
                    let config = nonuniform::Generator::read(
                        &mut generator,
                        n.user_version,
                        system.flag4 != 0,
                    )?;
                    ensure!(
                        !config.animated || config.frame_count > 0,
                        "Animated nonuniform atlas has no frames"
                    );
                    ensure!(
                        config.frame_rate.is_finite(),
                        "Nonfinite nonuniform atlas frame rate"
                    );
                    (
                        QuadAlignment {
                            u: [1.; 3],
                            v: [1.; 3],
                            center: config.center,
                            factors: [0.; 2],
                        },
                        config.atlas,
                        config.frame_variation,
                        AtlasAnimation {
                            enabled: config.animated,
                            start_frame: config.start_frame,
                            frame_count: config.frame_count,
                            frame_rate: config.frame_rate,
                        },
                    )
                } else if facing {
                    generator.done()?;
                    (
                        QuadAlignment {
                            u: [1.; 3],
                            v: [1.; 3],
                            center: [0.; 2],
                            factors: [0.; 2],
                        },
                        [1, 1],
                        0.,
                        AtlasAnimation::default(),
                    )
                } else {
                    let alignment = QuadAlignment::read(&mut generator)?;
                    warnings.push(format!("Aligned particle quads retain U {:?}, V {:?}, center {:?}, native factors {:?}; velocity alignment and native speed scaling are active, perspective facing/rotation and distortion remain incomplete",
                alignment.u,alignment.v,alignment.center,alignment.factors));
                    ensure!(
                        generator.byte()? == 0,
                        "Scrolling particle atlas not implemented"
                    );
                    let atlas = [generator.short()?, generator.short()?];
                    ensure!(atlas.iter().all(|v| *v > 0), "Empty particle atlas");
                    let animated = generator.byte()? != 0;
                    let start_frame = generator.short()?;
                    let frame_var = generator.float()?;
                    let frame_count = generator.short()?;
                    let _scroll = [generator.float()?, generator.float()?];
                    let frame_rate = generator.float()?;
                    ensure!(
                        !animated || frame_count > 0,
                        "Animated particle atlas has no frames"
                    );
                    ensure!(frame_rate.is_finite(), "Nonfinite particle atlas rate");
                    generator.done()?;
                    (
                        alignment,
                        atlas,
                        frame_var,
                        AtlasAnimation {
                            enabled: animated,
                            start_frame,
                            frame_count,
                            frame_rate,
                        },
                    )
                };
                let indirect =
                    material.shader.as_deref() == Some("SFX_accumulatedIndirect_Positive");
                if indirect {
                    warnings.push("Hose distortion layer decoded but not drawn: indirect framebuffer compositing is not implemented".into());
                }
                let tex = surface
                    .texture
                    .as_ref()
                    .and_then(|name| nif::find_pak_texture(files, name))
                    .and_then(|f| nif::decode_image_bytes(&f.data))
                    .map(|(rgba, w, h)| (Arc::new(rgba), w, h));
                ensure!(
                    surface.texture.is_none() || tex.is_some(),
                    "Missing hose texture {:?}",
                    surface.texture
                );
                let mut images = Vec::new();
                let program = (|| -> Result<_> {
                if attached && material.shader.is_none() {
                    if material.legacy_texture_flags.is_none() {
                        warnings.push(
                            "Particle without NiTexturingProperty uses vertex-color fallback".into(),
                        );
                    }
                    return Ok(Arc::new(crate::render_support::wii_default::compile_particle(&material)?));
                }
                let name = material
                    .shader
                    .as_deref()
                    .context("No active R3MT material: the native shader-selection path for this particle model is not implemented")?;
                let filename = format!("{name}.r3mt");
                let source = files
                    .iter()
                    .rev()
                    .find(|f| {
                        f.path
                            .replace('\\', "/")
                            .rsplit('/')
                            .next()
                            .is_some_and(|n| n.eq_ignore_ascii_case(&filename))
                    })
                    .context("Missing particle shader source")?;
                let text=std::str::from_utf8(&source.data)?;
                Ok(Arc::new(if attached {material_preview::compile_native_with_registers(text,&material,&registers)?}
                    else{material_preview::compile_with_registers(text,false,&registers)?}))
            })()
            .map_err(|e| format!("{e:#}"))
            .and_then(|p| {
                images = material_preview::images(&p, &material, files, &mut textures)?;
                Ok(p)
            });
                if let Err(e) = &program {
                    ensure!(!attached, "Attached particle shader: {e}");
                    warnings.push(format!("Hose shader: {e}"));
                }
                let draw = GpuDrawCommand {
                    shader_time:None,source_attributes: None,
                    extra_uvs: None,
                    projection_bound: None,
                    projected_shadow: false,
                    material_registers: None,
                    material_indirect: None,
                    material_images: Arc::new(images),
                    baked: false,
                    baked_error: None,
                    preview: Some(program),
                    uv_transform: surface.transform,
                    material: Some(material),
                    sampler: surface.sampler,
                    v_data: Arc::new(Vec::new()),
                    indices: Arc::new(Vec::new()),
                    trans_mat: [
                        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                    ],
                    color: [1.; 4],
                    tex_bytes: tex,
                    aabb_min: [0.; 3],
                    aabb_max: [0.; 3],
                    light_group: Arc::from(""),
                };
                layers.push(Layer {
                    node: index as u32,
                    nonuniform,
                    custom_quad,
                    world_space,
                    system_local,
                    emitter_in_system,
                    native_emitter: None,
                    render_transform: system_local,
                    emitter,
                    mesh_surfaces,
                    curves: {
                        let curves = curves.context("Missing particle curves")?;
                        ensure!(
                            curves.envelope.grow == 0. && curves.envelope.shrink == 0.,
                            "Hose grow/shrink playback is not implemented"
                        );
                        curves
                    },
                    playback,
                    enabled: false,
                    capacity: system.max_num_particles as usize,
                    scale,
                    particles: Vec::new(),
                    draw,
                    atlas,
                    frame_var,
                    atlas_animation,
                    indirect,
                    alignment,
                    up_method: system.up_method,
                    facing,
                    emitter_local,
                    rotations: system.flag2 != 0,
                    texture_seeds: system.flag4 != 0,
                    prepared_forces: Vec::with_capacity(forces.len()),
                    forces,
                    started_at: 0.,
                    command_driven: false,
                });
            }
        }
        ensure!(!layers.is_empty(), "Hose has no particle layers");
        let colors = if attached {
            let hierarchy = Arc::new(super::scene_bounds::Hierarchy::load(&n)?);
            let objects = hierarchy
                .nodes
                .iter()
                .filter(|(_, node)| node.authored_bound.is_some())
                .map(|(&id, _)| (id, Default::default()))
                .collect();
            Some(Colors { hierarchy, objects })
        } else {
            None
        };
        let mut instance = Self {
            layers,
            random: 1,
            time: 0.,
            absolute_time: None,
            warnings,
            colors,
        };
        if let Some(colors) = &instance.colors {
            let world = colors.hierarchy.base_world_transforms(None)?;
            instance.publish_model_transforms(&world)?;
        }
        Ok(instance)
    }
    /// Visual preview only. RNG sequence, full alignment and emission transforms
    /// still require original-runtime comparison; no gameplay effects are applied.
    pub fn advance(
        &mut self,
        dt: f32,
        enabled: bool,
        origin: [f32; 3],
        direction: [f32; 3],
    ) -> Result<()> {
        self.advance_with_attachment(dt, enabled, origin, direction, None)
    }
    pub fn set_subgraph_constant(
        &mut self,
        name: Option<&str>,
        slot: i32,
        color: [f32; 4],
    ) -> Result<usize> {
        let colors = self
            .colors
            .as_mut()
            .context("Native material overrides require attached particle model ownership")?;
        let mut graph = super::subgraph_color::Graph {
            hierarchy: &colors.hierarchy,
            root: super::subgraph_color::model_root(&colors.hierarchy)?,
            visitor: |node, slot, rgba| {
                colors
                    .objects
                    .get_mut(&node)
                    .context("Particle render object state missing")?
                    .publish(slot, rgba)
            },
        };
        super::subgraph_color::set_constant(&mut graph, name, slot, color)
    }
    pub fn set_subgraph_regular(&mut self, name: Option<&str>, slot: i32, color: [f32;4]) -> Result<usize> {
        if !(0..3).contains(&slot) {return Ok(0);}
        let colors=self.colors.as_mut().context("Regular particle colors require attached model ownership")?;
        let mut graph=super::subgraph_color::Graph {
            hierarchy:&colors.hierarchy,
            root:super::subgraph_color::model_root(&colors.hierarchy)?,
            visitor:|node,slot,rgba|colors.objects.get_mut(&node).context("Particle render object missing")?.publish_regular(slot,rgba),
        };
        super::subgraph_color::set_constant(&mut graph,name,slot,color)
    }
    pub fn color_summary(&self) -> serde_json::Value {
        serde_json::json!(self
            .colors
            .as_ref()
            .map(|colors| {
                colors
                    .objects
                    .iter()
                    .filter(|(_, state)| state.constant_colors.iter().any(Option::is_some))
                    .map(|(node, state)| {
                        serde_json::json!({"node":node,"name":colors.hierarchy.nodes[node].name,
                "constant_colors":state.constant_colors})
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default())
    }
    pub fn advance_attached(
        &mut self,
        dt: f32,
        enabled: bool,
        attachment: [f32; 16],
    ) -> Result<()> {
        ensure!(
            attachment.iter().all(|v| v.is_finite()),
            "Invalid effect attachment"
        );
        self.advance_with_attachment(dt, enabled, [0.; 3], [0., 0., 1.], Some(attachment))
    }
    fn advance_with_attachment(
        &mut self,
        dt: f32,
        enabled: bool,
        origin: [f32; 3],
        direction: [f32; 3],
        attachment: Option<[f32; 16]>,
    ) -> Result<()> {
        ensure!(
            self.absolute_time.is_none(),
            "Cannot mix preview deltas with absolute particle playback"
        );
        ensure!(
            dt.is_finite() && dt >= 0. && origin.iter().chain(&direction).all(|v| v.is_finite()),
            "Invalid live hose input"
        );
        let time = self.time + dt;
        ensure!(time.is_finite(), "Live hose time overflow");
        if dt == 0. {
            return self.attach_for_render(attachment.as_ref());
        }
        let mut random = super::particle_random::Stream::Preview { seed: self.random };
        let result = self.update_sample(
            time,
            self.time,
            Some((enabled, dt)),
            origin,
            direction,
            attachment,
            &mut random,
        );
        if let super::particle_random::Stream::Preview { seed } = random {
            self.random = seed;
        }
        result
    }
    /// Absolute application timestamp and caller-owned global random stream.
    /// This removes the preview clock/RNG from this path, but does not admit
    /// the remaining simulator, collision, alignment or rendering behavior as
    /// native parity. Backwards clocks require the unconnected full reset.
    pub fn advance_attached_at(
        &mut self,
        time: f32,
        attachment: [f32; 16],
        random: &mut super::game_random::State,
    ) -> Result<()> {
        ensure!(
            time.is_finite() && attachment.iter().all(|v| v.is_finite()),
            "Invalid absolute particle input"
        );
        ensure!(
            self.colors.is_some(),
            "Absolute particle playback requires an attached model"
        );
        let last = if let Some(last) = self.absolute_time {
            ensure!(
                time >= last,
                "Backward particle clock requires native ResetParticleSystem"
            );
            last
        } else {
            ensure!(
                self.time == 0.,
                "Cannot switch an already played preview to an absolute particle clock"
            );
            time
        };
        self.absolute_time = Some(time);
        self.update_sample(
            time,
            last,
            None,
            [0.; 3],
            [0., 0., 1.],
            Some(attachment),
            &mut super::particle_random::Stream::Game(random),
        )
    }
    fn attach_for_render(&mut self, attachment: Option<&[f32; 16]>) -> Result<()> {
        for layer in &mut self.layers {
            if layer.world_space {
                let scale=layer.scale*attachment.map(attachment_scale).transpose()?.unwrap_or(1.);
                ensure!(scale!=0. && scale.is_finite(),"Singular world particle scale");
                let previous=attachment_scale(&layer.render_transform)?;
                if previous!=scale {
                    // NiPS completion804e5a10 preserves existing world positions
                    // when the retained world scale changes. Velocities stay in
                    // native system units; do not rescale their stored values.
                    let ratio=previous/scale;
                    for particle in &mut layer.particles {particle.p=particle.p.map(|v|v*ratio);}
                }
                layer.render_transform=[scale,0.,0.,0.,0.,scale,0.,0.,0.,0.,scale,0.,0.,0.,0.,1.];
            } else {
                let owner = attachment.context("Local particles require their owner transform")?;
                layer.render_transform = super::skin::matrix_product(owner, &layer.system_local);
            }
        }
        Ok(())
    }
    fn update_sample(
        &mut self,
        time: f32,
        system_last: f32,
        preview: Option<(bool, f32)>,
        origin: [f32; 3],
        direction: [f32; 3],
        attachment: Option<[f32; 16]>,
        random: &mut super::particle_random::Stream<'_>,
    ) -> Result<()> {
        self.attach_for_render(attachment.as_ref())?;
        self.time = time;
        let length = (direction[0] * direction[0] + direction[2] * direction[2]).sqrt();
        let right = if length > 0. {
            [direction[2] / length, 0., -direction[0] / length]
        } else {
            [1., 0., 0.]
        };
        let up = [
            -direction[1] * right[2],
            direction[2] * right[0] - direction[0] * right[2],
            direction[1] * right[0],
        ];
        for l in &mut self.layers {
            let simulation_owner = if l.world_space {
                let inverse=1./l.render_transform[0];
                let inverse=[inverse,0.,0.,0.,0.,inverse,0.,0.,0.,0.,inverse,0.,0.,0.,0.,1.];
                attachment.as_ref().map(|owner|super::skin::matrix_product(&inverse,owner))
            } else {None};
            let enabled = preview.map_or(true, |(enabled, _)| enabled);
            if preview.is_some() && !l.command_driven && enabled != l.enabled {
                if enabled {
                    if attachment.is_some() {
                        l.started_at = self.time;
                        l.playback.scaled = None;
                    }
                    for force in &mut l.forces {
                        force.reset();
                    }
                    l.playback.start();
                } else {
                    l.playback.stop();
                }
                l.enabled = enabled;
            }
            let time = if preview.is_some() {
                self.time - l.started_at
            } else {
                self.time
            };
            l.prepared_forces.clear();
            for force in &mut l.forces {
                if let Some(mut prepared) = force.prepare(
                    time,
                    simulation_owner.as_ref(),
                )? {
                    if force.world_direction && !l.world_space {
                        // Inverse system rotation puts native world-down into
                        // the local particle simulation (DOL804da7ec).
                        let matrix = l.render_transform;
                        let scale =
                            (matrix[0] * matrix[0] + matrix[1] * matrix[1] + matrix[2] * matrix[2])
                                .sqrt();
                        ensure!(scale > 0., "Singular gravity field particle transform");
                        prepared.direction =
                            std::array::from_fn(|col| -matrix[col * 4 + 1] / scale);
                    }
                    l.prepared_forces.push(prepared);
                }
            }
            for p in &mut l.particles {
                let elapsed = self.time - p.last_update;
                for force in &l.prepared_forces {
                    p.v = force.apply_with_random(p.p, p.v, self.time, p.last_update, random)?;
                }
                p.age += elapsed;
                p.rotation.0 += p.rotation.1 * elapsed;
                for i in 0..3 {
                    p.p[i] += p.v[i] * elapsed;
                }
                p.last_update = self.time;
            }
            l.particles.retain(|p| p.age < p.life);
            // Native emitter commands own the controller's active flag.
            // The preview's global toggle must not suppress their births.
            if !l.command_driven && !enabled {
                continue;
            }
            let mut births = Vec::new();
            let previous = preview.map_or(system_last, |(_, dt)| (time - dt).max(0.));
            // Controller time is rebased on a preview START; the simulator's
            // particle timestamps remain on the persistent system clock.
            // Publishing controller-local birth times makes a late-started
            // effect age by its entire pre-start wait on the following frame.
            let system_offset = self.time - time;
            l.playback
                .update(time, true, Some(previous), |birth_time, _, ages| {
                    births.extend(ages.map(|age| (birth_time + system_offset, age)));
                    Ok(())
                })?;
            for (birth_time, age) in births {
                let e = &l.emitter;
                // Preserve the recovered birth/RNG order. Delta previews use
                // their private RNG; absolute playback uses the shared game
                // stream. Full native emitter
                // transforms and catch-up ordering remain pending. Birth scheduling uses
                // the imported controller rather than a second rate counter.
                let transform = if !l.world_space {
                    l.emitter_in_system
                } else if let Some(owner) = &simulation_owner {
                    super::skin::matrix_product(owner, &l.emitter_local)
                } else {
                    let inverse=1./l.render_transform[0];
                    [
                        inverse, 0., 0., 0., 0., inverse, 0., 0., 0., 0., inverse, 0., origin[0]*inverse, origin[1]*inverse,
                        origin[2]*inverse, 1.,
                    ]
                };
                let rotation = if l.native_emitter.is_some() {
                    [1., 0., 0., 0., 1., 0., 0., 0., 1.]
                } else if attachment.is_some() {
                    // Positions use the full relative frame. Velocity remains
                    // in system units and uses emitter orientation only;
                    // retained system scale is applied during rendering.
                    let mut axes = [0.; 9];
                    for col in 0..3 {
                        let norm = (0..3)
                            .map(|row| transform[col * 4 + row].powi(2))
                            .sum::<f32>()
                            .sqrt();
                        ensure!(norm > 0., "Singular effect emitter transform");
                        for row in 0..3 {
                            axes[col * 3 + row] = transform[col * 4 + row] / norm;
                        }
                    }
                    axes
                } else {
                    [
                        right[0], right[1], right[2],
                        up[0], up[1], up[2],
                        direction[0], direction[1], direction[2],
                    ]
                };
                let birth = if let Some(surfaces) = &l.mesh_surfaces {
                    // For mesh emitters the source geometry carries its own
                    // model transform; this parent is owner or inverse-system.
                    e.mesh_birth(birth_time, age, surfaces, &transform, &mut || {
                        random.raw15()
                    })?
                } else {
                    e.birth_in_frame(
                        birth_time,
                        age,
                        &transform,
                        &rotation,
                        l.native_emitter.as_ref(),
                        &mut || random.unit(),
                    )?
                };
                let Some(birth) = birth else { continue };
                // Native allocation is attempted AFTER kinematics; a full
                // system ends this batch without sampling size/rotation.
                if l.particles.len() >= l.capacity {
                    break;
                }
                let mut radius = e.size;
                if e.size_var != 0. {
                    radius += e.size_var * (2. * random.unit() - 1.);
                }
                ensure!(radius.is_finite(), "Invalid live particle radius");
                let initial_rotation = e
                    .rotation
                    .sample(l.rotations, false, &mut || random.unit())?
                    .angles
                    .unwrap_or((0., 0.));
                // Base InitializeParticle publishes the signed texture seed
                // before the nonuniform subclass samples its two radii.
                let frame = if l.custom_quad && l.texture_seeds {
                    nonuniform::atlas_frame(random.raw15(), l.atlas)?
                } else if l.facing {
                    0
                } else {
                    (l.frame_var * random.unit()).round() as u32
                };
                // The subclass needs raw15-bit draws, independently of the
                // unit-float conversion. Don't reconstruct native integers
                // from a rounded normalized sample.
                let dimensions = l
                    .nonuniform
                    .as_ref()
                    .map(|system| system.sizes(|| random.raw15()))
                    .transpose()?;
                let mut velocity = birth.velocity;
                for force in &l.prepared_forces {
                    velocity = force.apply_with_random(
                        birth.position,
                        velocity,
                        birth_time,
                        birth.last_update,
                        random,
                    )?;
                }
                let elapsed = birth_time - birth.last_update;
                let p = std::array::from_fn(|i| birth.position[i] + velocity[i] * elapsed);
                l.particles.push(Particle {
                    p,
                    v: velocity,
                    age,
                    life: birth.life,
                    size: dimensions.map_or(radius, |d| d[0]),
                    height: dimensions.map(|d| d[1]),
                    frame,
                    rotation: (
                        initial_rotation.0 + initial_rotation.1 * age,
                        initial_rotation.1,
                    ),
                    last_update: self.time,
                });
            }
        }
        Ok(())
    }
    pub fn append(
        &self,
        right: [f32; 3],
        up: [f32; 3],
        draws: &mut Vec<GpuDrawCommand>,
    ) -> Result<()> {
        for l in &self.layers {
            if l.particles.is_empty() || l.indirect {
                continue;
            }
            let mut vertices = Vec::with_capacity(l.particles.len() * 48);
            let mut indices = Vec::with_capacity(l.particles.len() * 6);
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            let (right, up) = if l.world_space {
                (right, up)
            } else {
                let scale = (0..3)
                    .map(|row| l.render_transform[row] * l.render_transform[row])
                    .sum::<f32>()
                    .sqrt();
                ensure!(
                    scale > 0. && scale.is_finite(),
                    "Singular local particle render transform"
                );
                // FacingQuadGenerator transforms camera axes by world rotation
                // into particle-system space, before generating local quads.
                let axis = |value: [f32; 3]| {
                    std::array::from_fn(|col| {
                        (0..3)
                            .map(|row| value[row] * l.render_transform[col * 4 + row] / scale)
                            .sum::<f32>()
                    })
                };
                (axis(right), axis(up))
            };
            for p in &l.particles {
                let t = p.age / p.life;
                let factors = [
                    sample(&l.curves.size, t)[0],
                    l.curves
                        .height
                        .as_ref()
                        .map(|track| track.sample(p.age, p.life))
                        .transpose()?
                        .unwrap_or(1.),
                ];
                let factors = l.nonuniform.as_ref().map_or(factors, |system| {
                    system.animated_factors(factors, p.v)
                });
                let size = p.size * factors[0];
                let height = p.height.map_or(size, |radius| radius * factors[1]);
                // Alignment consumes the same unscaled system velocity as
                // the native generator, before render-world publication.
                let scales = l.alignment.scales(p.v)?;
                let (right, up) = if l.up_method == 9 {
                    velocity_quad_axes(right, up, p.v)
                } else {
                    (right, up)
                };
                let color = l.curves.color_at(t);
                let base = (vertices.len() / 12) as u32;
                let corners = if l.facing {
                    [
                        (-1., 1., 0., 0.),
                        (-1., -1., 0., 1.),
                        (1., -1., 1., 1.),
                        (1., 1., 1., 0.),
                    ]
                } else {
                    [
                        (-1., -1., 0., 1.),
                        (1., -1., 1., 1.),
                        (1., 1., 1., 0.),
                        (-1., 1., 0., 0.),
                    ]
                };
                for (corner, (x, y, u, v)) in corners.into_iter().enumerate() {
                    // Facing generator rotates the camera plane clockwise;
                    // preserve its source corner/UV ordering.
                    let (x, y) = if l.facing {
                        let (s, c) = p.rotation.0.sin_cos();
                        (x * c + y * s, -x * s + y * c)
                    } else {
                        (x, y)
                    };
                    let mut point: [f32; 3] = if p.height.is_some() {
                        std::array::from_fn(|i| {
                            p.p[i]
                                + size * (x - l.alignment.center[0]) * scales[0] * right[i]
                                + height * (y - l.alignment.center[1]) * scales[1] * up[i]
                        })
                    } else {
                        std::array::from_fn(|i| {
                            p.p[i]
                                + size
                                    * ((x - l.alignment.center[0]) * scales[0] * right[i]
                                        + (y - l.alignment.center[1]) * scales[1] * up[i])
                        })
                    };
                    {
                        point = std::array::from_fn(|row| {
                            l.render_transform[12 + row]
                                + (0..3)
                                    .map(|col| l.render_transform[col * 4 + row] * point[col])
                                    .sum::<f32>()
                        });
                    }
                    for i in 0..3 {
                        min[i] = min[i].min(point[i]);
                        max[i] = max[i].max(point[i]);
                    }
                    let [u, v] = if l.custom_quad && !l.texture_seeds {
                        nonuniform::unseeded_uvs()[corner]
                    } else {
                        let frame = l.atlas_animation.frame(p.frame, p.age, l.atlas);
                        [
                            (u + (frame % u32::from(l.atlas[0])) as f32) / f32::from(l.atlas[0]),
                            (v + (frame / u32::from(l.atlas[0])) as f32) / f32::from(l.atlas[1]),
                        ]
                    };
                    vertices.extend(point);
                    vertices.extend([0., 1., 0., u, v]);
                    vertices.extend(color);
                }
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            let mut draw = l.draw.clone();
            if let Some(colors) = &self.colors {
                colors
                    .objects
                    .get(&l.node)
                    .context("Particle render state missing")?
                    .apply(&mut draw)?;
            }
            draw.v_data = Arc::new(vertices);
            draw.indices = Arc::new(indices);
            draw.aabb_min = min;
            draw.aabb_max = max;
            draws.push(draw);
        }
        Ok(())
    }
}

/// The stock spin's camera-facing ribbon. Sparkle/pop systems are separate;
/// this instance retains source emission timing, lifespan and trail geometry.
#[derive(Clone)]
pub struct Ribbon {
    generator: super::ribbon::Generator,
    playback: EmissionPlayback,
    curves: Curves,
    draw: GpuDrawCommand,
    emitter_local: [f32; 16],
    system_scale: f32,
    size: f32,
    life: f32,
    capacity: usize,
    points: Vec<([f32; 3], f32, [f32; 3], f32)>,
    radius_track: Option<(RateTrack, ControllerTime, u16, [f32; 4])>,
    emitter_node: u32,
    moving: bool,
    launch_speed: f32,
    maximum_spacing: f32,
    forces: Vec<fields::Force>,
    time: f32,
    started_at: f32,
    enabled: bool,
}
impl Ribbon {
    pub fn load(bytes: &[u8], files: &[VirtualFile]) -> Result<Self> {
        Self::load_inner(bytes, files, false)
    }
    pub fn load_drip(bytes: &[u8], files: &[VirtualFile]) -> Result<Self> {
        Self::load_inner(bytes, files, true)
    }
    fn load_inner(bytes: &[u8], files: &[VirtualFile], moving: bool) -> Result<Self> {
        let n = NifFile::load_bytes(bytes).map_err(anyhow::Error::msg)?;
        let candidates = n
            .block_type_indices
            .iter()
            .enumerate()
            .filter(|(_, kind)| n.block_types[**kind as usize] == "RibbonParticleSystem")
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(candidates.len() == 1, "Expected one spin ribbon system");
        let index = candidates[0];
        let NiObject::NiPSParticleSystem(system) = &n.blocks[index] else {
            anyhow::bail!("Ribbon base system missing")
        };
        let blocks = super::jigsaw::raw_blocks(bytes)?;
        let extension = blocks[index]
            .get(blocks[index].len().saturating_sub(14)..)
            .context("Missing ribbon extension")?;
        let mut extra = Reader {
            bytes: extension,
            at: 0,
            little: n.source_is_little_endian,
        };
        let version = extra.short()?;
        let facing = extra.uint()?;
        let constraint = extra.float()?;
        let mode = extra.uint()?;
        ensure!(version == 3 && facing == 0 && mode == 2 && constraint >= 0.
            && (moving || constraint == 0.), "Unsupported ribbon facing/constraint mode");
        // The drip resources add a spacing constraint. The gameplay solver
        // limits adjacent samples in their retained scale. Exact native iteration
        // order/velocity correction parity remains unverified.

        extra.done()?;
        ensure!(
            system.emitter_links.len() == 1,
            "Multiple ribbon emitters unsupported"
        );
        let mut emitter = raw(
            &n,
            system.emitter_links[0].context("Missing ribbon emitter")?,
            "RibbonEmitter",
        )?;
        let emission_name = n.string_table.get(emitter.uint()? as usize).context("Missing ribbon emitter name")?.clone();
        let speed = emitter.float()?;
        let variance = emitter.float()?;
        ensure!(
            (moving || speed == 0.) && variance == 0.,
            "Moving ribbon particles require force integration"
        );
        for _ in 0..5 {
            ensure!(
                emitter.float()? == 0.,
                "Rotated ribbon launch velocity unsupported"
            );
        }
        let size = emitter.float()?;
        ensure!(emitter.float()? == 0., "Variable ribbon size unsupported");
        let life = emitter.float()?;
        ensure!(emitter.float()? == 0., "Variable ribbon life unsupported");
        EmitterRotation::read(&mut emitter)?;
        ensure!(
            emitter.short()? == 2 && emitter.byte()? == 0,
            "Infinite ribbon lifetime unsupported"
        );
        let node = emitter.uint()?;
        emitter.done()?;
        ensure!((moving || size >= 0.) && life > 0., "Invalid ribbon size/lifetime");
        let emitter_name = &n
            .blocks
            .get(node as usize)
            .and_then(|b| b.as_av_object())
            .context("Missing ribbon emitter node")?
            .base
            .name;
        let emitter_local = super::brush_render::named_transform(&n, emitter_name)?
            .context("Missing ribbon node transform")?;
        let system_name = &system.base.base.base.base.name;
        let system_matrix = super::brush_render::named_transform(&n, system_name)?
            .context("Missing ribbon system transform")?;
        let system_scale = (system_matrix[0] * system_matrix[0]
            + system_matrix[1] * system_matrix[1]
            + system_matrix[2] * system_matrix[2])
            .sqrt();
        let mut radius_track = None;
        for (index, block) in n.blocks.iter().enumerate() {
            if block.type_name() == "NiPSEmitterRadiusCtlr" {
                let mut r = raw(&n, index as u32, "NiPSEmitterRadiusCtlr")?;
                r.uint()?;
                let flags = r.short()?;
                let parameters = [r.float()?, r.float()?, r.float()?, r.float()?];
                ensure!(r.uint()? == candidates[0] as u32, "Ribbon radius controller target mismatch");
                let track = RateTrack::load(&n, r.uint()?)?;
                r.uint()?;
                r.done()?;
                radius_track = Some((track, ControllerTime::default(), flags, parameters));
            }
        }
        let (mut playback, controller_chain) = EmissionPlayback::for_emitter(
            &n, index as u32, system.base.base.base.base.controller_link, &emission_name)?;
        playback.stop();
        let generator_link = system
            .base
            .modifiers
            .iter()
            .flatten()
            .find(|&&link| n.blocks[link as usize].type_name() == "RibbonQuadGenerator")
            .context("Missing ribbon generator")?;
        let generator = super::ribbon::Generator::load(
            raw(&n, *generator_link, "RibbonQuadGenerator")?.bytes,
            n.source_is_little_endian,
        )?;
        let simulator = SimulatorPlan::load(
            &n,
            system.simulator_link.context("Missing ribbon simulator")?,
        )?;
        let mut curves = None;
        let mut forces = Vec::new();
        for link in simulator.steps {
            if let Some(NiObject::Unknown(step)) = n.blocks.get(link as usize) {
                match step.type_name.as_str() {
                    "NiPSSimulatorGeneralStep" => {
                        curves = Some(Curves::read_kind(
                            &mut raw(&n, link, &step.type_name)?,
                            false,
                            n.user_version,
                        )?)
                    }
                    "NiPSSimulatorForcesStep" if moving => {
                        let mut r = raw(&n, link, &step.type_name)?;
                        for _ in 0..r.uint()? { forces.push(fields::Force::load_animated(&n, r.uint()?)?); }
                        r.done()?;
                    }
                    "NiPSSimulatorForcesStep" | "NiPSSimulatorCollidersStep" => ensure!(
                        step.raw_data == [0, 0, 0, 0],
                        "Ribbon forces/colliders require simulation"
                    ),
                    "NiPSSimulatorFinalStep" => {}
                    _ => anyhow::bail!("Unsupported ribbon simulator step {}", step.type_name),
                }
            }
        }
        for controller in &controller_chain {
            if controller.class_name == "NiPSFieldMagnitudeCtlr" {
                let magnitude = fields::Magnitude::load(&n, controller, index as u32)?;
                let force = forces.iter_mut().find(|force| force.name == magnitude.name)
                    .context("Ribbon magnitude controller has no matching field")?;
                force.controls.push(magnitude);
            }
        }
        let curves = curves.context("Missing ribbon curves")?;
        ensure!(
            curves.envelope.grow == 0. && curves.envelope.shrink == 0.,
            "Ribbon size envelope unsupported"
        );
        let surface = nif::find_mesh_surface(&n, index);
        let material = Arc::new(surface.material);
        let mut registers = crate::tev::Registers::default();
        for &link in system.base.base.base.base.extra_data_links.iter().flatten() {
            if let Some(NiObject::Unknown(extra)) = n.blocks.get(link as usize) {
                if extra.type_name == "NiColorExtraData" {
                    let mut r = raw(&n, link, "NiColorExtraData")?;
                    let name = n
                        .string_table
                        .get(r.uint()? as usize)
                        .context("Missing ribbon constant name")?;
                    let (constant, slot) = if let Some(slot) = name.strip_prefix("Constant_Color_") {
                        (true, slot.parse::<usize>()?)
                    } else if let Some(slot) = name.strip_prefix("Color_Register_") {
                        (false, slot.parse::<usize>()?)
                    } else { continue; };
                    let mut color = [0; 4];
                    for c in &mut color {
                        *c = ((r.float()? * 255.) as i32) as u8;
                    }
                    r.done()?;
                    if constant {
                        *registers.constants.get_mut(slot).context("Invalid ribbon constant slot")? = Some(color);
                    } else {
                        *registers.colors.get_mut(slot).context("Invalid ribbon regular-color slot")? = Some(color);
                    }
                }
            }
        }
        let shader = format!(
            "{}.r3mt",
            material
                .shader
                .as_deref()
                .context("Missing ribbon shader")?
        );
        let source = files
            .iter()
            .rev()
            .find(|f| {
                f.path
                    .replace('\\', "/")
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&shader))
            })
            .context("Missing ribbon shader source")?;
        let text = std::str::from_utf8(&source.data)?;
        let program = Arc::new(if moving {
            material_preview::compile_native_with_registers(text, &material, &registers)?
        } else {
            material_preview::compile_with_registers(text, false, &registers)?
        });
        let images = material_preview::images(&program, &material, files, &mut HashMap::new())
            .map_err(anyhow::Error::msg)?;
        let tex = surface
            .texture
            .as_ref()
            .and_then(|name| nif::find_pak_texture(files, name))
            .and_then(|f| nif::decode_image_bytes(&f.data))
            .map(|(data, w, h)| (Arc::new(data), w, h));
        ensure!(
            surface.texture.is_none() || tex.is_some(),
            "Missing ribbon texture"
        );
        let draw = GpuDrawCommand {
            shader_time:None,source_attributes: None,
            extra_uvs: None,
            projection_bound: None,
            projected_shadow: false,
            material_registers: None,
            material_indirect: None,
            material_images: Arc::new(images),
            baked: false,
            baked_error: None,
            preview: Some(Ok(program)),
            uv_transform: surface.transform,
            material: Some(material),
            sampler: surface.sampler,
            v_data: Arc::new(Vec::new()),
            indices: Arc::new(Vec::new()),
            trans_mat: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            color: [1.; 4],
            tex_bytes: tex,
            aabb_min: [0.; 3],
            aabb_max: [0.; 3],
            light_group: Arc::from(""),
        };
        Ok(Self {
            generator,
            playback,
            curves,
            draw,
            emitter_local,
            system_scale,
            size,
            life,
            capacity: system.max_num_particles as usize,
            points: Vec::new(),
            radius_track,
            emitter_node: node,
            moving,
            launch_speed: speed,
            maximum_spacing: constraint,
            forces,
            time: 0.,
            started_at: 0.,
            enabled: false,
        })
    }
    pub fn particle_count(&self) -> usize { self.points.len() }
    pub fn render_probe(&self) -> serde_json::Value {
        serde_json::json!({"shader":self.draw.material.as_ref().and_then(|m|m.shader.as_ref()),
            "system_scale":self.system_scale,"points":self.points.iter().map(|(p,b,_,r)|
                serde_json::json!({"position":p,"age":self.time-b,"radius":r})).collect::<Vec<_>>()})
    }
    pub fn restart(&mut self) { self.enabled = false; }
    pub fn update_emitter_pose(&mut self, poses: &std::collections::BTreeMap<u32, epic_mickey_lib_rs::nif_file::NiTransform>) -> Result<()> {
        if let Some(pose) = poses.get(&self.emitter_node) {
            self.emitter_local = super::render_transform::from_entity(
                std::array::from_fn(|i| pose.rotation.rows[i / 3][i % 3]),
                [pose.translation.x, pose.translation.y, pose.translation.z], pose.scale);
        }
        for force in &mut self.forces { force.publish_model_transform(poses, None)?; }
        Ok(())
    }
    pub fn advance(&mut self, dt: f32, enabled: bool, attachment: [f32; 16]) -> Result<()> {
        ensure!(
            dt.is_finite() && dt >= 0. && attachment.iter().all(|v| v.is_finite()),
            "Invalid ribbon update"
        );
        if dt == 0. {
            return Ok(());
        }
        self.time += dt;
        self.points
            .retain(|(_, birth, _, _)| self.time - *birth < self.life);
        let scale = (0..3).map(|i| attachment[i] * attachment[i]).sum::<f32>().sqrt();
        let simulation_scale = scale * self.system_scale;
        let mut prepared = Vec::new();
        for force in &mut self.forces {
            if let Some(force) = force.prepare(self.time, Some(&attachment))? { prepared.push(force); }
        }
        for (position, _, velocity, _) in &mut self.points {
            for force in &prepared {
                let next = force.apply(*position, velocity.map(|v| v / simulation_scale), self.time, self.time - dt)?;
                *velocity = next.map(|v| v * simulation_scale);
            }
            for axis in 0..3 { position[axis] += velocity[axis] * dt; }
        }
        if enabled != self.enabled {
            if enabled {
                // Each attack activates a new effect clock. APP_TIME belongs
                // to that effect instance, not the level's running uptime.
                self.started_at = self.time;
                if let Some((_, clock, _, _)) = &mut self.radius_track { clock.reset(); }
                self.playback.stop();
                self.playback.scaled = None;
                self.playback.start();
            } else {
                self.playback.stop();
            }
            self.enabled = enabled;
        }
        if !enabled {
            return Ok(());
        }
        let transform = super::skin::matrix_product(&attachment, &self.emitter_local);
        let mut births = Vec::new();
        let local_time = self.time - self.started_at;
        self.playback.update(
            local_time,
            true,
            Some((local_time - dt).max(0.)),
            |time, _, mut ages| {
                // REL153614/1537fc: a nonempty request allocates ONE ribbon
                // sample, even when the ordinary emitter requested several births.
                if ages.next().is_some() {
                    births.push(time);
                }
                Ok(())
            },
        )?;
        for birth in births {
            let radius = if let Some((track, clock, flags, parameters)) = &mut self.radius_track {
                let t = clock.scale_parameters(birth, *flags, parameters[0], parameters[1], parameters[2], parameters[3])?;
                track.sample(t)?
            } else { self.size };
            if self.points.len() < self.capacity {
                self.points.push((
                    [transform[12], transform[13], transform[14]],
                    self.started_at + birth,
                    std::array::from_fn(|row| transform[row] * self.launch_speed),
                    radius * if self.moving { scale } else { 1. },
                ));
            }
        }
        if self.maximum_spacing > 0. {
            let limit = self.maximum_spacing * simulation_scale;
            // Anchor at the newest sample and constrain back along the strand.
            // Never join distant positions left by movement/animated fields.
            for index in (0..self.points.len().saturating_sub(1)).rev() {
                let anchor = self.points[index + 1].0;
                let delta: [f32; 3] = std::array::from_fn(|axis| self.points[index].0[axis] - anchor[axis]);
                let length = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                if length > limit {
                    self.points[index].0 = std::array::from_fn(|axis| anchor[axis] + delta[axis] * limit / length);
                }
            }
        }
        Ok(())
    }
    pub fn append(
        &self,
        right: [f32; 3],
        up: [f32; 3],
        draws: &mut Vec<GpuDrawCommand>,
    ) -> Result<()> {
        if self.points.len() < 2 {
            return Ok(());
        }
        let cross = |a: [f32; 3], b: [f32; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let normalize = |v: [f32; 3]| {
            let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if length > 1e-6 {
                v.map(|x| x / length)
            } else {
                [0.; 3]
            }
        };
        let normal = cross(right, up);
        let mut edges = Vec::with_capacity(self.points.len());
        let mut colors = Vec::with_capacity(self.points.len());
        for (i, &(point, birth, _, radius)) in self.points.iter().enumerate() {
            let (a, b) = if i + 1 < self.points.len() {
                (point, self.points[i + 1].0)
            } else {
                (self.points[i - 1].0, point)
            };
            let tangent = normalize(std::array::from_fn(|j| b[j] - a[j]));
            let age = (self.time - birth) / self.life;
            let width = radius * self.system_scale * sample(&self.curves.size, age)[0];
            let offset = normalize(cross(tangent, normal)).map(|v| v * width);
            edges.push(std::array::from_fn(|j| {
                if j < 3 {
                    point[j] - offset[j]
                } else {
                    point[j - 3] + offset[j - 3]
                }
            }));
            colors.push(self.curves.color_at(age));
        }
        let points = self.generator.interpolate_pairs(&edges)?;
        let uv = self
            .generator
            .texcoords(self.points.len() as u32, self.capacity as u32)?;
        let mut vertices = Vec::with_capacity(points.len() * 24);
        let mut indices = Vec::with_capacity((points.len() - 1) * 6);
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for (section, pair) in points.iter().enumerate() {
            let segments = usize::from(self.generator.subdivisions);
            let a = (section / segments).min(colors.len() - 1);
            let b = (a + 1).min(colors.len() - 1);
            let t = (section % segments) as f32 / segments as f32;
            let color = std::array::from_fn::<_, 4, _>(|j| {
                colors[a][j] + (colors[b][j] - colors[a][j]) * t
            });
            for side in 0..2 {
                let point = &pair[side * 3..side * 3 + 3];
                for j in 0..3 {
                    min[j] = min[j].min(point[j]);
                    max[j] = max[j].max(point[j]);
                }
                vertices.extend_from_slice(point);
                vertices.extend(normal);
                vertices.extend(uv[section * 2 + side]);
                vertices.extend(color);
            }
            if section + 1 < points.len() {
                let base = (section * 2) as u32;
                indices.extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
            }
        }
        let mut draw = self.draw.clone();
        draw.v_data = Arc::new(vertices);
        draw.indices = Arc::new(indices);
        draw.aabb_min = min;
        draw.aabb_max = max;
        draws.push(draw);
        Ok(())
    }
}
