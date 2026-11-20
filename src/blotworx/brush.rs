//! Native brush configuration and emission clock. Original action ownership,
//! projectile spawning, surface stimuli and rendering are not connected yet.
use super::ability_value::RegisteredProperty;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

/// Brush action uses strafe events, not EVENT_Start_Brush. Verified registration
/// at REL 0x2ec3b0/0x2ec3c0 and variable registration at 0x2ebf40.
#[derive(Clone, Copy)]
pub struct AnimationBindings {
    pub start: i32,
    pub stop: i32,
    pub state: usize,
    pub elevation: usize,
    pub yaw: usize,
    pub shuffle: [i32; 3],
}
impl AnimationBindings {
    pub fn load(graph: &super::behavior::Document) -> Result<Self> {
        let strings = graph.single("hkbBehaviorGraphStringData")?;
        let find = |field: &str, name: &str| -> Result<usize> {
            let entries = strings[field]
                .as_array()
                .with_context(|| format!("Missing {field}"))?;
            let mut matches = entries
                .iter()
                .enumerate()
                .filter(|(_, n)| n.as_str() == Some(name));
            let index = matches
                .next()
                .with_context(|| format!("Missing brush animation binding {name}"))?
                .0;
            ensure!(
                matches.next().is_none(),
                "Ambiguous brush animation binding {name}"
            );
            Ok(index)
        };
        let state = find("variableNames", "VAR_StrafingState")?;
        ensure!(
            graph.single("hkbBehaviorGraphData")?["variableInfos"][state]["type"].as_u64()
                == Some(3),
            "Brush strafing state is not int32"
        );
        let elevation = find("variableNames", "VAR_Dot_Of_Aim_Angle")?;
        let yaw = find("variableNames", "VAR_Yaw_To_Target")?;
        for index in [elevation, yaw] {
            ensure!(
                graph.single("hkbBehaviorGraphData")?["variableInfos"][index]["type"].as_u64()
                    == Some(4),
                "Brush aiming input is not real"
            );
        }
        Ok(Self {
            start: find("eventNames", "EVENT_Start_Strafe")?.try_into()?,
            stop: find("eventNames", "EVENT_Stop_Strafe")?.try_into()?,
            state,
            elevation,
            yaw,
            shuffle: [
                find("eventNames", ShuffleEvent::Right.name())?.try_into()?,
                find("eventNames", ShuffleEvent::Left.name())?.try_into()?,
                find("eventNames", ShuffleEvent::Stop.name())?.try_into()?,
            ],
        })
    }
    /// REL 0xa8a48..0xa8a68. Direction computation, facing adjustment and
    /// target selection are owned by the action; don't infer them from camera
    /// pitch. Preserve original write order and the exact degrees multiplier.
    pub fn apply_aim(
        &self,
        state: &mut super::behavior_state::State,
        elevation_dot: f32,
        yaw_radians: f32,
    ) -> Result<()> {
        state.set_real(self.elevation, elevation_dot)?;
        state.set_real(self.yaw, 57.2957763671875_f32 * yaw_radians)
    }
}

/// REL 0x297cd0: subtract and add 4*pi in single precision, use double fmod
/// with the single-precision 2*pi constant, then narrow and wrap above pi.
/// This intentionally differs from rem_euclid at negative/extreme inputs.
pub fn aim_yaw_delta(from: f32, to: f32) -> f32 {
    let shifted = (to - from) + f32::from_bits(0x41490fdb);
    let period = f32::from_bits(0x40c90fdb);
    let mut result = (shifted as f64 % period as f64) as f32;
    if result > f32::from_bits(0x40490fdb) {
        result -= period;
    }
    result
}

/// Moving branch of REL 0xa8630..0xa86f8. The caller supplies the native
/// atan2 provider (DOL 0x800249d8); host libm is not a parity substitute.
/// None selects the separate stationary aiming branch, not a zero yaw write.
pub fn moving_aim_yaw(
    sample: Option<&super::ground_history::Sample>,
    facing: f32,
    atan2: impl FnOnce(f64, f64) -> Result<f64>,
) -> Result<Option<f32>> {
    let Some(sample) = sample else {
        return Ok(None);
    };
    if sample.speed <= 0.009999999776482582_f32 {
        return Ok(None);
    }
    let direction = sample.direction;
    let heading = atan2(direction[0] as f64, direction[2] as f64)? as f32;
    Ok(Some(aim_yaw_delta(facing, heading)))
}

/// Target geometry at REL 0xa85a8..0xa862c. Origin is the entity position
/// returned by 0x296c44, not the camera or brush-tip position.
pub struct AimDirection {
    pub direction: [f32; 3],
    pub distance: f32,
    pub elevation_dot: f32,
}
pub fn target_direction(origin: [f32; 3], target: [f32; 3]) -> AimDirection {
    let mut direction = std::array::from_fn(|i| target[i] - origin[i]);
    let [x, y, z] = direction;
    let squared = z.mul_add(z, x * x) + y * y;
    let mut distance = if squared == 0. {
        0.
    } else {
        let estimate = super::pose_blend::estimate(squared);
        let correction = (-squared).mul_add(estimate * estimate, 1.);
        squared * correction.mul_add(0.5 * estimate, estimate)
    };
    // Tiny targets keep their unnormalized vector but report zero distance.
    // This differs from replacing the direction with a fallback forward axis.
    if distance <= 0.0001 {
        distance = 0.;
    } else {
        let reciprocal = 1. / distance;
        direction = direction.map(|v| v * reciprocal);
    }
    let elevation_dot = direction[2].mul_add(0., direction[0] * 0.) + direction[1] * 1.;
    AimDirection {
        direction,
        distance,
        elevation_dot,
    }
}

/// Brush action +0x168; REL 0xa8834..0xa8914. The caller supplies the
/// motion-history vector from 0x308190, already-resolved angular rates at
/// motion+0x350/+0x354, and entity timestep from 0x1efed0. These are NOT
/// interchangeable with current velocity, authored rates or host frame time.
#[derive(Default)]
pub struct AimShuffle {
    pub turning: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShuffleEvent {
    Right,
    Left,
    Stop,
}
impl ShuffleEvent {
    pub fn name(self) -> &'static str {
        match self {
            Self::Right => "EVENT_Start_Brush_Shuffle_Right",
            Self::Left => "EVENT_Start_Brush_Shuffle_Left",
            Self::Stop => "EVENT_Stop_Shuffle",
        }
    }
}
impl AimShuffle {
    /// Sends synchronously after committing the latch, including on callback
    /// failure. Turning direction changes do not restart an active shuffle.
    pub fn update(
        &mut self,
        yaw_error: f32,
        history: [f32; 3],
        stationary_rate: f32,
        moving_rate: f32,
        entity_dt: f32,
        mut send: impl FnMut(ShuffleEvent) -> Result<()>,
    ) -> Result<()> {
        let [x, y, z] = history;
        let squared = z.mul_add(z, x * x) + y * y;
        let rate = if squared < 0.009999999776482582_f32 {
            stationary_rate
        } else {
            moving_rate
        };
        let turning = yaw_error.abs() > entity_dt * rate;
        if turning != self.turning {
            self.turning = turning;
            send(if !turning {
                ShuffleEvent::Stop
            } else if yaw_error < 0. {
                ShuffleEvent::Left
            } else {
                ShuffleEvent::Right
            })?;
        }
        Ok(())
    }
}

#[derive(serde::Serialize)]
pub struct Settings {
    pub target_position_y_offset: f32,
    pub run_speed: f32,
    pub turn_rate: f32,
    pub projectile_interval: f32,
    pub burst: i32,
    pub projectile_templates: [String; 2],
    pub channels: BTreeMap<String, BTreeMap<String, RegisteredProperty>>,
}
impl Settings {
    /// Applied after animation-facing aim, before stream/projectile launch.
    /// REL a8ca4..a8cbc offsets world Y, not the camera up vector.
    pub fn emission_target(&self, mut target: [f32; 3]) -> [f32; 3] {
        target[1] += self.target_position_y_offset;
        target
    }
    pub fn load(values: &BTreeMap<String, String>) -> Result<Self> {
        let mut lookup = |key: &str| -> Result<Option<f32>> {
            values
                .get(key)
                .map(|v| {
                    let number: f32 = v
                        .parse()
                        .with_context(|| format!("Invalid brush setting {key}: {v}"))?;
                    ensure!(number.is_finite(), "Non-finite brush setting {key}");
                    Ok(number)
                })
                .transpose()
        };
        // REL 0xa65f0 registers RunSpeed with default string 2.5f.
        // REL a725c..a7280 registers 0.2; retail config overrides to 0.075.
        let target_position_y_offset =
            lookup("MickeyAbilities_Brush_TargetPositionYOffset")?.unwrap_or(0.2);
        let turn_rate = lookup("MickeyAbilities_Brush_TurnRate")?.unwrap_or(5.23598);
        let run_speed = lookup("MickeyAbilities_Brush_RunSpeed")?.unwrap_or(2.5);
        // Require shared values until their absent-key constructor defaults
        // have been recovered; never silently fabricate gameplay settings.
        let projectile_interval = lookup("MickeyAbilities_Brush_ProjectileIntervalTime")?
            .context("Missing brush ProjectileIntervalTime")?;
        let burst: i32 = values
            .get("MickeyAbilities_Brush_NumStimProjectilesToSpawn")
            .context("Missing brush projectile count")?
            .parse()
            .context("Invalid brush projectile count")?;
        let projectile_templates = ["Paint", "Thinner"].map(|c| {
            values
                .get(&format!("MickeyAbilities_Brush_{c}StimProjectileName"))
                .cloned()
        });
        let [paint, thinner] = projectile_templates;
        let projectile_templates = [
            paint.context("Missing paint projectile template")?,
            thinner.context("Missing thinner projectile template")?,
        ];
        let mut channels = BTreeMap::new();
        for channel in ["Paint", "Thinner"] {
            let mut properties = BTreeMap::new();
            for property in [
                "Lifetime",
                "Speed",
                "Range",
                "Size",
                "StimulusScale",
                "BallScale",
                "Cost",
                "PlaystyleMin",
                "PlaystyleMax",
            ] {
                properties.insert(
                    property.into(),
                    RegisteredProperty::load("Brush", channel, property, &mut lookup)?,
                );
            }
            channels.insert(channel.into(), properties);
        }
        Ok(Self {
            target_position_y_offset,
            run_speed,
            turn_rate,
            projectile_interval,
            burst,
            projectile_templates,
            channels,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    Paint,
    Thinner,
}
impl Ink {
    fn name(self) -> &'static str {
        match self {
            Self::Paint => "Paint",
            Self::Thinner => "Thinner",
        }
    }
    fn mode(self) -> i32 {
        match self {
            Self::Paint => 0,
            Self::Thinner => 1,
        }
    }
}
impl Settings {
    /// Owner supplies the original selected zero-based upgrade slot and
    /// playstyle. Keep absent keys distinct from an invented default speed.
    pub fn value(&self, ink: Ink, property: &str, upgrade: usize, playstyle: i32) -> Result<f32> {
        let channel = &self.channels[ink.name()];
        let selected = |name: &str| -> Result<Option<super::ability_value::Property>> {
            channel
                .get(name)
                .map(|p| {
                    p.levels.get(upgrade).copied().with_context(|| {
                        format!(
                            "Brush {} {name} upgrade {upgrade} is not registered",
                            ink.name()
                        )
                    })
                })
                .transpose()
        };
        let endpoint = |name: &str| -> Result<f32> {
            Ok(super::ability_value::evaluate(
                selected(name)?,
                ink.mode(),
                playstyle,
                false,
                1.,
                1.,
            ))
        };
        Ok(super::ability_value::evaluate(
            selected(property)?,
            ink.mode(),
            playstyle,
            true,
            endpoint("PlaystyleMin")?,
            endpoint("PlaystyleMax")?,
        ))
    }
}
/// REL 0x0aa724..0x0aa7cc, including the length helper at 0x01fad0.
/// Origin already includes the original random location variance. This is
/// aim-stage velocity, before the caller's additional direction normalization.
pub fn aim_velocity(origin: [f32; 3], target: [f32; 3], speed: f32) -> [f32; 3] {
    let mut direction = std::array::from_fn(|i| target[i] - origin[i]);
    let squared = direction[2].mul_add(direction[2], direction[0] * direction[0])
        + direction[1] * direction[1];
    let length = if squared == 0. {
        0.
    } else {
        let estimate = super::pose_blend::estimate(squared);
        let correction = (-squared).mul_add(estimate * estimate, 1.);
        squared * correction.mul_add(0.5 * estimate, estimate)
    };
    // Verified constant at REL 0x4ac3e4. Tiny vectors are retained, not
    // replaced with an arbitrary forward direction or divided by zero.
    if length > 0.0001 {
        let reciprocal = 1. / length;
        direction = direction.map(|v| v * reciprocal);
    }
    direction.map(|v| v * speed)
}

/// Final emission normalization (REL 0xaa598..0xaa66c). The caller owns
/// the game's RNG: even zero variance consumes one sample for nonzero aim.
pub fn emission_velocity(
    aim: [f32; 3],
    variance: f32,
    mut random: impl FnMut() -> u16,
) -> [f32; 3] {
    let squared = aim[2].mul_add(aim[2], aim[0] * aim[0]) + aim[1] * aim[1];
    if squared == 0. {
        return aim;
    }
    let estimate = super::pose_blend::estimate(squared);
    let correction = (-squared).mul_add(estimate * estimate, 1.);
    let length = squared * correction.mul_add(0.5 * estimate, estimate);
    let reciprocal = 1. / length;
    let direction = aim.map(|v| v * reciprocal);
    let sample = random();
    assert!(
        sample <= 32767,
        "Original brush RNG produces 15-bit samples"
    );
    let offset = -1. + (2. * sample as f32) / 32767.;
    let speed = offset.mul_add(variance, length);
    direction.map(|v| v * speed)
}

/// Projectile velocity setter 0x3927b4 uses the entity's time-scale policy
/// (0x1eff44). Preserve the unscaled launch vector separately for callbacks.
pub fn body_velocity(velocity: [f32; 3], entity_flag_40: bool, global_scale: f32) -> [f32; 3] {
    let multiplier = if entity_flag_40 || global_scale == 0. {
        1.
    } else {
        1. / global_scale
    };
    velocity.map(|v| v * multiplier)
}

/// Entity lifecycle represented by the brush's two original projectile lists.
/// Body/FX activation and callbacks remain the caller's responsibility.
#[derive(Clone, Debug)]
pub struct PooledProjectile {
    pub ink: Ink,
    pub entity: u64,
    pub active: bool,
    pub lifetime: Lifetime,
    pub owner: u64,
    pub position: [f32; 3],
    pub launch_velocity: [f32; 3],
    pub body_velocity: [f32; 3],
    pub rotation: [[f32; 3]; 3],
    pub hit: bool,
    pub flag_59: bool,
    pub flag_9c: bool,
}
#[derive(Default)]
pub struct ProjectilePool {
    pub entries: Vec<PooledProjectile>,
}
impl ProjectilePool {
    /// REL 0x0a9cd4..0x0a9edc scans the selected ink's list in insertion
    /// order, creates only if no inactive entity exists, and appends new IDs.
    /// Failed creation does not leave a half-initialized pool entry.
    pub fn acquire<E>(
        &mut self,
        ink: Ink,
        owner: u64,
        position: [f32; 3],
        rotation: [[f32; 3]; 3],
        create: impl FnOnce() -> Result<u64, E>,
    ) -> Result<&mut PooledProjectile, E> {
        let index = if let Some(index) = self.entries.iter().position(|p| p.ink == ink && !p.active)
        {
            index
        } else {
            let entity = create()?;
            self.entries.push(PooledProjectile {
                ink,
                entity,
                active: false,
                owner,
                position,
                rotation,
                launch_velocity: [0.; 3],
                body_velocity: [0.; 3],
                hit: false,
                flag_59: false,
                flag_9c: false,
                lifetime: Lifetime::default(),
            });
            self.entries.len() - 1
        };
        let projectile = &mut self.entries[index];
        projectile.owner = owner;
        projectile.position = position;
        projectile.rotation = rotation;
        // Reactivation 0x38eaa0 resets these bytes at +58, +59 and +9c.
        projectile.hit = false;
        projectile.flag_59 = false;
        projectile.flag_9c = false;
        projectile.active = true;
        Ok(projectile)
    }
    /// Apply the brush Lifetime override before activating the projectile in
    /// the world (REL 0x0aa67c..0x0aa6b0). The selected owner upgrade is explicit.
    pub fn launch(
        &mut self,
        settings: &Settings,
        ink: Ink,
        upgrade: usize,
        playstyle: i32,
        owner: u64,
        position: [f32; 3],
        rotation: [[f32; 3]; 3],
        create: impl FnOnce() -> Result<u64>,
    ) -> Result<&mut PooledProjectile> {
        let remaining = settings.value(ink, "Lifetime", upgrade, playstyle)?;
        ensure!(remaining.is_finite(), "Non-finite evaluated brush lifetime");
        let projectile = self.acquire(ink, owner, position, rotation, create)?;
        projectile.lifetime = Lifetime {
            remaining,
            paused: false,
            paused_during_igc: false,
        };
        Ok(projectile)
    }
    /// The world owner dispatches event 0x3b then the brush's deactivation
    /// callback. They observe the old remaining value, as in REL 0x35933c /
    /// 0x359354; the saved subtraction is committed only at 0x35940c.
    pub fn update_lifetimes(&mut self, dt: f32, mut expired: impl FnMut(&mut PooledProjectile)) {
        for projectile in &mut self.entries {
            if !projectile.active {
                continue;
            }
            let mut updated = projectile.lifetime.clone();
            if updated.update(dt) {
                expired(projectile);
                projectile.active = false;
            }
            // A synchronous listener can edit the component, but the retail
            // function still stores its previously calculated timer afterward.
            // Preserve listener changes to other fields (including pause).
            projectile.lifetime.remaining = updated.remaining;
        }
    }
    /// Deactivation preserves identity and storage for subsequent shots.
    pub fn deactivate(&mut self, entity: u64) -> bool {
        if let Some(p) = self.entries.iter_mut().find(|p| p.entity == entity) {
            p.active = false;
            true
        } else {
            false
        }
    }
}

impl PooledProjectile {
    /// Activation copies the launch vector to +130 before selecting the body
    /// activation path. Brush state 9 takes 0x390fc4 -> 0x38e978 with r5=0.
    pub fn set_launch_velocity(
        &mut self,
        velocity: [f32; 3],
        entity_flag_40: bool,
        global_scale: f32,
    ) {
        self.launch_velocity = velocity;
        self.body_velocity = body_velocity(velocity, entity_flag_40, global_scale);
    }
}

#[derive(Default, Clone, Debug)]
pub struct Lifetime {
    pub remaining: f32,
    pub paused: bool,
    pub paused_during_igc: bool,
}
impl Lifetime {
    /// REL 0x359280..0x35940c: zero and negative values are not counting;
    /// expiry is strictly negative after subtraction, not <= zero.
    pub fn update(&mut self, dt: f32) -> bool {
        if self.remaining <= 0. || self.paused || self.paused_during_igc {
            return false;
        }
        self.remaining -= dt;
        // bge skips expiration; unordered (NaN) therefore expires as well.
        !(self.remaining >= 0.)
    }
}

/// Explicit elapsed state allows replay from an observed original action.
#[derive(Default, Debug)]
pub struct EmissionClock {
    pub elapsed: f32,
}
impl EmissionClock {
    /// REL 0x0a8e14..0x0a8e7c. Caller owns active/resource/FX gates and startup.
    /// Check before adding dt; reset discards overshoot. At most one burst
    /// occurs per original update, even when dt exceeds the interval.
    pub fn update(&mut self, dt: f32, interval: f32, burst: i32, startup: i32) -> i32 {
        if startup <= -2 && self.elapsed >= interval {
            self.elapsed = 0.;
            burst.max(0)
        } else {
            self.elapsed += dt;
            0
        }
    }
}
/// Active action's emission phase. Input eligibility and action start/stop are
/// owned by the controller; this state begins only after a successful start.
#[derive(Debug)]
pub struct Firing {
    pub startup: i32,
    pub effect_ink: Ink,
    pub clock: EmissionClock,
}
#[derive(Debug, PartialEq)]
pub struct FiringStep {
    pub deactivate_effect: Option<Ink>,
    pub activate_effect: Option<Ink>,
    pub shots: i32,
    pub debit: Option<f32>,
}
/// Synchronous world operations, in original execution order. An integration
/// must implement every operation; there are no silent no-op defaults.
pub trait FiringHost {
    fn deactivate_effect(&mut self, ink: Ink) -> Result<()>;
    fn activate_effect(&mut self, ink: Ink) -> Result<()>;
    fn can_pay(&mut self, ink: Ink, cost: f32) -> Result<bool>;
    fn spawn(&mut self, ink: Ink) -> Result<()>;
    fn debit(&mut self, ink: Ink, cost: f32) -> Result<bool>;
}
/// Scene/FX side of firing. Resource state is supplied to synchronous entity
/// callbacks because a spawn or effect can change inventory before debit.
pub trait FiringEffects {
    fn deactivate(&mut self, ink: Ink, ammo: &mut super::ammo::State) -> Result<()>;
    fn activate(&mut self, ink: Ink, ammo: &mut super::ammo::State) -> Result<()>;
    fn spawn(&mut self, ink: Ink, ammo: &mut super::ammo::State) -> Result<()>;
    fn insufficient_ammo(&mut self, ammo: &mut super::ammo::State) -> Result<()>;
    fn inventory_written(&mut self, channel: usize, old: f32, new: f32) -> Result<()>;
}
pub struct AmmoFiringHost<'a, E> {
    pub ammo: &'a mut super::ammo::State,
    pub effects: &'a mut E,
}
impl<E: FiringEffects> FiringHost for AmmoFiringHost<'_, E> {
    fn deactivate_effect(&mut self, ink: Ink) -> Result<()> {
        self.effects.deactivate(ink, self.ammo)
    }
    fn activate_effect(&mut self, ink: Ink) -> Result<()> {
        self.effects.activate(ink, self.ammo)
    }
    fn spawn(&mut self, ink: Ink) -> Result<()> {
        self.effects.spawn(ink, self.ammo)
    }
    fn can_pay(&mut self, ink: Ink, cost: f32) -> Result<bool> {
        // REL 0x2e46f0 uses an ordered >= comparison, including its NaN case.
        if self.ammo.available(ink.mode() as usize) >= cost {
            Ok(true)
        } else {
            self.effects.insufficient_ammo(self.ammo)?;
            Ok(false)
        }
    }
    fn debit(&mut self, ink: Ink, cost: f32) -> Result<bool> {
        // The original calls the same availability routine again after shots.
        // A changed balance can fail here without undoing an emitted shot.
        if !self.can_pay(ink, cost)? {
            return Ok(false);
        }
        let effects = &mut self.effects;
        self.ammo
            .change(ink.mode() as usize, -1. * cost, &mut |c, old, new| {
                effects.inventory_written(c, old, new)
            })?;
        Ok(true)
    }
}
impl Firing {
    /// FX portion of REL 0xa9a34..0xa9a98. The owner decides the +8c
    /// eligibility gate; each host lookup must tolerate a missing stream and
    /// resolve it at call time, since stopping paint can change thinner's
    /// resource. Both channels are visited in slot order, not just effect_ink.
    /// The action does not reset +154 on stop or start: return it to the owner.
    /// Animation, motion restoration and action-manager teardown are separate.
    pub fn stop(self, streams_enabled: bool, host: &mut impl FiringHost) -> Result<EmissionClock> {
        self.stop_effects(streams_enabled, |ink| host.deactivate_effect(ink))
    }
    /// Stop needs only stream callbacks, not a fabricated projectile/ammo
    /// context. Resolve each stream at the time its callback is delivered.
    pub fn stop_effects(
        self,
        streams_enabled: bool,
        mut deactivate: impl FnMut(Ink) -> Result<()>,
    ) -> Result<EmissionClock> {
        if streams_enabled {
            deactivate(Ink::Paint)?;
            deactivate(Ink::Thinner)?;
        }
        Ok(self.clock)
    }
    /// REL 0xa98c8..0xa98dc initializes +12c=3 and saves the current effect
    /// channel. Starting the action does not reset the +154 emission clock.
    pub fn begin(ink: Ink, clock: EmissionClock) -> Self {
        Self {
            startup: 3,
            effect_ink: ink,
            clock,
        }
    }
    /// REL 0xa8d50..0xa8e98: update FX countdown first, test affordability,
    /// run emission, then debit. Missing FX pauses countdown; insufficient
    /// resources pause the emission clock, not the earlier FX work.
    /// Effects run before affordability; each spawn runs before clock reset
    /// and debit. The returned record is diagnostic, not deferred commands.
    pub fn update(
        &mut self,
        ink: Ink,
        has_effect: bool,
        dt: f32,
        interval: f32,
        burst: i32,
        cost_per_second: f32,
        host: &mut impl FiringHost,
    ) -> Result<FiringStep> {
        let mut result = self.advance_effects(ink, has_effect, |ink, enabled| {
            if enabled {
                host.activate_effect(ink)
            } else {
                host.deactivate_effect(ink)
            }
        })?;
        let cost = dt * cost_per_second;
        if !host.can_pay(ink, cost)? {
            return Ok(result);
        }
        if self.startup <= -2 && self.clock.elapsed >= interval {
            for _ in 0..burst.max(0) {
                host.spawn(ink)?;
                result.shots += 1;
            }
            self.clock.elapsed = 0.;
        } else {
            self.clock.elapsed += dt;
        }
        if host.debit(ink, cost)? {
            result.debit = Some(cost);
        }
        Ok(result)
    }
    /// Shared FX-only phase, also used by visual playback while gameplay
    /// spawning/ammo are unfinished. This does not advance the emission clock
    /// or claim that a projectile was spawned.
    pub fn advance_effects(
        &mut self,
        ink: Ink,
        has_effect: bool,
        mut set_enabled: impl FnMut(Ink, bool) -> Result<()>,
    ) -> Result<FiringStep> {
        let mut result = FiringStep {
            deactivate_effect: None,
            activate_effect: None,
            shots: 0,
            debit: None,
        };
        if has_effect {
            if ink != self.effect_ink {
                result.deactivate_effect = Some(self.effect_ink);
                set_enabled(self.effect_ink, false)?;
                self.effect_ink = ink;
                self.startup = 2;
            }
            if self.startup >= -2 {
                self.startup -= 1;
                if self.startup == 0 {
                    set_enabled(ink, true)?;
                    result.activate_effect = Some(ink);
                }
            }
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visual_effect_phase_preserves_clock_and_native_start_switch_delays() -> Result<()> {
        let mut phase = Firing::begin(Ink::Paint, EmissionClock { elapsed: 0.75 });
        let mut calls = Vec::new();
        for _ in 0..2 {
            let step = phase.advance_effects(Ink::Paint, true, |ink, on| {
                calls.push((ink, on));
                Ok(())
            })?;
            assert_eq!(step.shots, 0);
            assert_eq!(step.debit, None);
        }
        assert!(calls.is_empty());
        phase.advance_effects(Ink::Paint, true, |ink, on| {
            calls.push((ink, on));
            Ok(())
        })?;
        assert_eq!(calls, vec![(Ink::Paint, true)]);
        phase.advance_effects(Ink::Thinner, true, |ink, on| {
            calls.push((ink, on));
            Ok(())
        })?;
        assert_eq!(calls, vec![(Ink::Paint, true), (Ink::Paint, false)]);
        let startup = phase.startup;
        phase.advance_effects(Ink::Thinner, false, |_, _| {
            panic!("Missing FX executed callback")
        })?;
        assert_eq!(phase.startup, startup);
        phase.advance_effects(Ink::Thinner, true, |ink, on| {
            calls.push((ink, on));
            Ok(())
        })?;
        assert_eq!(
            calls,
            vec![
                (Ink::Paint, true),
                (Ink::Paint, false),
                (Ink::Thinner, true)
            ]
        );
        assert_eq!(phase.clock.elapsed, 0.75);
        Ok(())
    }
    #[test]
    fn moving_aim_uses_latest_direction_and_strict_speed_gate() -> Result<()> {
        let mut sample = super::super::ground_history::Sample::default();
        sample.speed = 0.01;
        assert!(
            moving_aim_yaw(Some(&sample), 0., |_, _| panic!("Stationary queried atan2"))?.is_none()
        );
        sample.speed = 0.02;
        sample.direction = [1., 0., 0.];
        let angle = moving_aim_yaw(Some(&sample), 0.25, |x, z| {
            assert_eq!((x, z), (1., 0.));
            Ok(std::f64::consts::FRAC_PI_2)
        })?
        .unwrap();
        assert_eq!(angle, aim_yaw_delta(0.25, std::f32::consts::FRAC_PI_2));
        assert!(moving_aim_yaw(Some(&sample), 0., |_, _| anyhow::bail!(
            "Native atan2 unavailable"
        ))
        .is_err());
        Ok(())
    }
    #[test]
    fn target_elevation_uses_entity_origin_and_preserves_tiny_direction() {
        let aim = target_direction([10., 20., 30.], [10., 23., 34.]);
        assert!((aim.distance - 5.).abs() < 0.000001);
        assert!((aim.elevation_dot - 0.6).abs() < 0.000001);
        assert!((aim.direction[2] - 0.8).abs() < 0.000001);
        let tiny = target_direction([0.; 3], [0., 0.00001, 0.]);
        assert_eq!(tiny.distance, 0.);
        assert_eq!(tiny.direction, [0., 0.00001, 0.]);
        assert_eq!(tiny.elevation_dot, 0.00001);
        let zero = target_direction([1., 2., 3.], [1., 2., 3.]);
        assert_eq!(zero.distance, 0.);
        assert_eq!(zero.direction, [0.; 3]);
    }
    #[test]
    fn brush_shuffle_edges_use_history_and_commit_before_callback_failure() -> Result<()> {
        let mut shuffle = AimShuffle::default();
        let mut events = Vec::new();
        for angle in [0.25, 0.251, -0.4, -0.25] {
            shuffle.update(angle, [0.; 3], 1., 4., 0.25, |e| {
                events.push(e);
                Ok(())
            })?;
        }
        assert_eq!(events, [ShuffleEvent::Right, ShuffleEvent::Stop]);
        // Moving history chooses the larger threshold, independently of yaw.
        shuffle.update(0.5, [1., 0., 0.], 1., 4., 0.25, |_| {
            panic!("Moving threshold ignored")
        })?;
        shuffle.update(-0.5, [0.; 3], 1., 4., 0.25, |e| {
            events.push(e);
            Ok(())
        })?;
        assert_eq!(events.last(), Some(&ShuffleEvent::Left));
        assert!(shuffle
            .update(0., [0.; 3], 1., 4., 0.25, |_| anyhow::bail!(
                "dispatch failed"
            ))
            .is_err());
        assert!(!shuffle.turning);
        // Unordered comparison does not initiate a turn.
        shuffle.update(f32::NAN, [0.; 3], 1., 4., 0.25, |_| {
            panic!("NaN started shuffle")
        })?;
        Ok(())
    }
    #[test]
    fn aim_yaw_uses_original_wrapping_and_rounding() {
        assert_eq!(aim_yaw_delta(0., 0.).to_bits(), 0);
        let quarter = f32::from_bits(0x3fc90fdb);
        assert!((aim_yaw_delta(0., quarter) - quarter).abs() < 0.000001);
        assert!((aim_yaw_delta(quarter, 0.) + quarter).abs() < 0.000001);
        // The original signed remainder does not remap every negative value
        // into [-pi,pi] after the fixed +4*pi addition.
        assert!(aim_yaw_delta(0., -30.) < 0.);
        assert!(aim_yaw_delta(0., f32::INFINITY).is_nan());
    }
    #[test]
    fn stop_visits_both_streams_and_preserves_emission_clock() {
        struct StopHost {
            channels: Vec<Ink>,
            fail: bool,
        }
        impl FiringHost for StopHost {
            fn deactivate_effect(&mut self, ink: Ink) -> Result<()> {
                self.channels.push(ink);
                ensure!(!self.fail, "Stream teardown failed");
                Ok(())
            }
            fn activate_effect(&mut self, _: Ink) -> Result<()> {
                panic!("Stop activated FX")
            }
            fn can_pay(&mut self, _: Ink, _: f32) -> Result<bool> {
                panic!("Stop queried inventory")
            }
            fn spawn(&mut self, _: Ink) -> Result<()> {
                panic!("Stop emitted a projectile")
            }
            fn debit(&mut self, _: Ink, _: f32) -> Result<bool> {
                panic!("Stop debited inventory")
            }
        }
        let mut host = StopHost {
            channels: Vec::new(),
            fail: false,
        };
        let firing = Firing::begin(Ink::Thinner, EmissionClock { elapsed: 0.03125 });
        let clock = firing.stop(true, &mut host).unwrap();
        assert_eq!(host.channels, [Ink::Paint, Ink::Thinner]);
        assert_eq!(clock.elapsed.to_bits(), 0.03125f32.to_bits());
        host.channels.clear();
        let restarted = Firing::begin(Ink::Paint, clock);
        assert_eq!(restarted.startup, 3);
        assert_eq!(restarted.stop(false, &mut host).unwrap().elapsed, 0.03125);
        assert!(host.channels.is_empty());
        host.fail = true;
        assert!(Firing::begin(Ink::Paint, EmissionClock::default())
            .stop(true, &mut host)
            .is_err());
        assert_eq!(host.channels, [Ink::Paint]);
    }
    struct Host {
        pay: bool,
        trace: Vec<&'static str>,
        fail_spawn: bool,
    }
    impl Host {
        fn new(pay: bool) -> Self {
            Self {
                pay,
                trace: Vec::new(),
                fail_spawn: false,
            }
        }
    }
    impl FiringHost for Host {
        fn deactivate_effect(&mut self, _: Ink) -> Result<()> {
            self.trace.push("deactivate");
            Ok(())
        }
        fn activate_effect(&mut self, _: Ink) -> Result<()> {
            self.trace.push("activate");
            Ok(())
        }
        fn can_pay(&mut self, _: Ink, _: f32) -> Result<bool> {
            self.trace.push("check");
            Ok(self.pay)
        }
        fn spawn(&mut self, _: Ink) -> Result<()> {
            self.trace.push("spawn");
            ensure!(!self.fail_spawn, "Spawn failed");
            Ok(())
        }
        fn debit(&mut self, _: Ink, _: f32) -> Result<bool> {
            self.trace.push("debit");
            Ok(true)
        }
    }
    #[test]
    fn firing_uses_live_inventory_and_rechecks_after_spawn_callback() {
        use super::super::ammo::{
            ChannelSettings, Inventory, Policy, Settings as AmmoSettings, State, Waits,
        };
        struct Effects {
            drain_on_spawn: bool,
            events: Vec<&'static str>,
        }
        impl FiringEffects for Effects {
            fn deactivate(&mut self, _: Ink, _: &mut State) -> Result<()> {
                self.events.push("off");
                Ok(())
            }
            fn activate(&mut self, _: Ink, _: &mut State) -> Result<()> {
                self.events.push("on");
                Ok(())
            }
            fn spawn(&mut self, _: Ink, ammo: &mut State) -> Result<()> {
                self.events.push("spawn");
                if self.drain_on_spawn {
                    ammo.inventory.as_mut().unwrap().amounts[0] = 0.;
                }
                Ok(())
            }
            fn insufficient_ammo(&mut self, _: &mut State) -> Result<()> {
                self.events.push("empty");
                Ok(())
            }
            fn inventory_written(&mut self, _: usize, _: f32, _: f32) -> Result<()> {
                self.events.push("write");
                Ok(())
            }
        }
        let channel = ChannelSettings {
            cost: 1.,
            jump_cost: 1.,
            recharge_rate: 2.,
            recharge_max: 10.,
            waits: Waits {
                recharge: 0.75,
                penalty_threshold: 1.,
                penalty: 2.,
            },
        };
        let mut ammo = State::new(
            Some(Inventory {
                amounts: [1., 7.],
                maxima: [Some(30.); 2],
            }),
            AmmoSettings {
                channels: [channel; 2],
            },
            Policy {
                shared: true,
                infinite: false,
            },
        );
        let mut effects = Effects {
            drain_on_spawn: false,
            events: vec![],
        };
        let mut firing = Firing::begin(Ink::Thinner, EmissionClock { elapsed: 1. });
        firing.startup = -3;
        let step = firing
            .update(
                Ink::Thinner,
                true,
                0.125,
                0.066,
                1,
                2.,
                &mut AmmoFiringHost {
                    ammo: &mut ammo,
                    effects: &mut effects,
                },
            )
            .unwrap();
        assert_eq!(step.shots, 1);
        assert_eq!(step.debit, Some(0.25));
        assert_eq!(ammo.inventory.as_ref().unwrap().amounts, [0.75, 7.]);
        assert_eq!(effects.events, ["spawn", "write"]);
        effects.events.clear();
        effects.drain_on_spawn = true;
        firing.clock.elapsed = 1.;
        let step = firing
            .update(
                Ink::Thinner,
                true,
                0.125,
                0.066,
                1,
                2.,
                &mut AmmoFiringHost {
                    ammo: &mut ammo,
                    effects: &mut effects,
                },
            )
            .unwrap();
        assert_eq!(step.shots, 1);
        assert_eq!(step.debit, None);
        assert_eq!(effects.events, ["spawn", "empty"]);
        effects.events.clear();
        firing.clock.elapsed = 1.;
        let step = firing
            .update(
                Ink::Thinner,
                true,
                0.125,
                0.066,
                1,
                2.,
                &mut AmmoFiringHost {
                    ammo: &mut ammo,
                    effects: &mut effects,
                },
            )
            .unwrap();
        assert_eq!(step.shots, 0);
        assert_eq!(effects.events, ["empty"]);
        assert_eq!(firing.clock.elapsed, 1.);
    }
    #[test]
    fn callbacks_run_synchronously_and_failed_spawn_is_not_charged() {
        let mut firing = Firing::begin(Ink::Paint, EmissionClock { elapsed: 0.5 });
        let mut host = Host::new(true);
        firing.startup = 1;
        firing
            .update(Ink::Paint, true, 1. / 30., 0.066, 2, 3., &mut host)
            .unwrap();
        assert_eq!(host.trace, ["activate", "check", "debit"]);
        host.trace.clear();
        firing.startup = -2;
        firing
            .update(Ink::Paint, true, 1. / 30., 0.066, 2, 3., &mut host)
            .unwrap();
        assert_eq!(host.trace, ["check", "spawn", "spawn", "debit"]);
        assert_eq!(firing.clock.elapsed, 0.);
        host.trace.clear();
        host.fail_spawn = true;
        firing.clock.elapsed = 0.5;
        assert!(firing
            .update(Ink::Paint, true, 1. / 30., 0.066, 2, 3., &mut host)
            .is_err());
        assert_eq!(host.trace, ["check", "spawn"]);
        assert_eq!(firing.clock.elapsed, 0.5);
    }
    #[test]
    fn firing_waits_for_effect_and_checks_resources_before_clock() {
        let mut firing = Firing::begin(Ink::Paint, EmissionClock::default());
        let mut activations = Vec::new();
        let mut shots = Vec::new();
        for tick in 0..9 {
            let step = firing
                .update(
                    Ink::Paint,
                    true,
                    1. / 30.,
                    0.066,
                    1,
                    3.,
                    &mut Host::new(true),
                )
                .unwrap();
            assert_eq!(step.debit, Some((1f32 / 30.) * 3.));
            if step.activate_effect.is_some() {
                activations.push(tick);
            }
            shots.push(step.shots);
        }
        assert_eq!(activations, [2]);
        assert_eq!(shots, [0, 0, 0, 0, 1, 0, 0, 1, 0]);
        assert_eq!(firing.startup, -3);
        let elapsed = firing.clock.elapsed;
        let blocked = firing
            .update(
                Ink::Thinner,
                true,
                1. / 30.,
                0.066,
                1,
                3.,
                &mut Host::new(false),
            )
            .unwrap();
        assert_eq!(blocked.deactivate_effect, Some(Ink::Paint));
        assert_eq!(blocked.debit, None);
        assert_eq!(firing.startup, 1);
        assert_eq!(firing.clock.elapsed, elapsed);
        let step = firing
            .update(
                Ink::Thinner,
                true,
                1. / 30.,
                0.066,
                1,
                3.,
                &mut Host::new(false),
            )
            .unwrap();
        assert_eq!(step.activate_effect, Some(Ink::Thinner));
        assert_eq!(firing.clock.elapsed, elapsed);
    }
    #[test]
    fn missing_effect_does_not_advance_startup_or_change_channel() {
        let mut firing = Firing::begin(Ink::Paint, EmissionClock { elapsed: 0.5 });
        let step = firing
            .update(
                Ink::Thinner,
                false,
                1. / 30.,
                0.066,
                1,
                3.,
                &mut Host::new(true),
            )
            .unwrap();
        assert_eq!(firing.startup, 3);
        assert_eq!(firing.effect_ink, Ink::Paint);
        assert_eq!(step.shots, 0);
        assert_eq!(step.deactivate_effect, None);
        assert!(step.debit.is_some());
        assert!(firing.clock.elapsed > 0.5);
    }
    #[test]
    fn emission_consumes_rng_and_time_scaling_preserves_launch_vector() {
        let mut calls = 0;
        assert_eq!(
            emission_velocity([0.; 3], 1., || {
                calls += 1;
                0
            }),
            [0.; 3]
        );
        assert_eq!(calls, 0);
        let low = emission_velocity([0., 0., 22.5], 2., || {
            calls += 1;
            0
        });
        let high = emission_velocity([0., 0., 22.5], 2., || {
            calls += 1;
            32767
        });
        let stock = emission_velocity([0., 0., 22.5], 0., || {
            calls += 1;
            123
        });
        assert_eq!(calls, 3);
        assert!((low[2] - 20.5).abs() < 0.00001);
        assert!((high[2] - 24.5).abs() < 0.00001);
        assert!((stock[2] - 22.5).abs() < 0.00001);
        let mut pool = ProjectilePool::default();
        let shot = pool
            .acquire(Ink::Paint, 1, [0.; 3], [[0.; 3]; 3], || Ok::<_, ()>(1))
            .unwrap();
        shot.set_launch_velocity(stock, false, 0.5);
        assert_eq!(shot.launch_velocity, stock);
        assert_eq!(shot.body_velocity, stock.map(|v| v * 2.));
        assert_eq!(body_velocity(stock, true, 0.5), stock);
        assert_eq!(body_velocity(stock, false, 0.), stock);
    }
    #[test]
    fn projectile_expiry_crosses_zero_once_and_preserves_pool_identity() {
        let mut life = Lifetime {
            remaining: 0.25,
            ..Default::default()
        };
        assert!(!life.update(0.25));
        assert_eq!(life.remaining, 0.);
        assert!(!life.update(1.)); // Match the original exact-zero edge case.
        life.remaining = 0.2;
        life.paused = true;
        assert!(!life.update(1.));
        assert_eq!(life.remaining, 0.2);
        life.paused = false;
        assert!(life.update(0.25));
        assert!(!life.update(0.25));
        let mut pool = ProjectilePool::default();
        let shot = pool
            .acquire(Ink::Paint, 1, [0.; 3], [[0.; 3]; 3], || Ok::<_, ()>(7))
            .unwrap();
        shot.lifetime.remaining = 0.2;
        let mut events = Vec::new();
        pool.update_lifetimes(0.25, |p| {
            events.push(p.entity);
            assert!(p.active);
            assert_eq!(p.lifetime.remaining, 0.2);
            p.lifetime.remaining = 100.;
            p.lifetime.paused = true;
        });
        assert_eq!(pool.entries[0].lifetime.remaining, 0.2f32 - 0.25);
        assert!(pool.entries[0].lifetime.paused);
        pool.update_lifetimes(0.25, |p| events.push(p.entity));
        assert_eq!(events, [7]);
        assert_eq!(pool.entries[0].entity, 7);
        assert!(!pool.entries[0].active);
        let mut life = Lifetime {
            remaining: 1.,
            ..Default::default()
        };
        assert!(life.update(f32::NAN));
        assert!(life.remaining.is_nan());
    }
    #[test]
    fn projectile_reuse_is_channel_local_and_clears_hit_state() {
        let mut pool = ProjectilePool::default();
        let rotation = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        let first = pool
            .acquire(Ink::Paint, 7, [0.; 3], rotation, || Ok::<_, ()>(10))
            .unwrap();
        first.hit = true;
        first.flag_59 = true;
        first.flag_9c = true;
        assert!(pool.deactivate(10));
        let thinner = pool
            .acquire(Ink::Thinner, 7, [0.; 3], rotation, || Ok::<_, ()>(11))
            .unwrap();
        assert_eq!(thinner.entity, 11);
        let reused = pool
            .acquire(Ink::Paint, 9, [1., 2., 3.], rotation, || Err(()))
            .unwrap();
        assert_eq!(reused.entity, 10);
        assert_eq!(reused.owner, 9);
        assert_eq!(reused.position, [1., 2., 3.]);
        assert!(reused.active && !reused.hit && !reused.flag_59 && !reused.flag_9c);
        assert_eq!(pool.entries.len(), 2);
        assert!(pool
            .acquire(Ink::Paint, 9, [0.; 3], rotation, || Err(()))
            .is_err());
        assert_eq!(pool.entries.len(), 2);
    }
    #[test]
    fn aim_preserves_zero_and_tiny_displacements_and_applies_speed_last() {
        assert_eq!(aim_velocity([1., 2., 3.], [1., 2., 3.], 22.5), [0.; 3]);
        assert_eq!(
            aim_velocity([0.; 3], [0.00005, 0., 0.], 20.),
            [f32::from_bits(0x3a83126e), 0., 0.]
        );
        let v = aim_velocity([1., 2., 3.], [4., 6., 3.], 22.5);
        assert!((v[0] - 13.5).abs() < 0.00001 && (v[1] - 18.).abs() < 0.00001);
        assert_eq!(v[2], 0.);
    }
    #[test]
    fn retail_emission_checks_before_increment_and_discards_overshoot() {
        let mut clock = EmissionClock::default();
        let counts: Vec<_> = (0..9)
            .map(|_| clock.update(1. / 30., 0.066, 1, -2))
            .collect();
        assert_eq!(counts, [0, 0, 1, 0, 0, 1, 0, 0, 1]);
        clock.elapsed = 1.;
        assert_eq!(clock.update(1., 0.066, 3, -2), 3);
        assert_eq!(clock.elapsed, 0.);
        assert_eq!(clock.update(1., 0.066, 3, -1), 0);
        assert_eq!(clock.elapsed, 1.);
    }
    #[test]
    fn settings_preserve_upgrade_and_playstyle_values() {
        let values=crate::render_support::game_config::runtime_settings(
            "[MickeyAbilities_Brush]\nProjectileIntervalTime=0.066\nNumStimProjectilesToSpawn=1\nPaintStimProjectileName=PaintBall\nThinnerStimProjectileName=ThinnerBall\nPaint_Speed_1=22.5\nPaint_Speed_2=30\nPaint_Cost_1=3\nPaint_Cost_PlaystyleMin_1=0.5\n",false);
        let settings = Settings::load(&values).unwrap();
        assert_eq!(settings.emission_target([1., 2., 3.]), [1., 2.2, 3.]);
        assert_eq!(
            settings.channels["Paint"]["Speed"].levels[1].value,
            Some(30.)
        );
        assert_eq!(
            settings.channels["Paint"]["Cost"].levels[0].playstyle_min,
            Some(0.5)
        );
        assert_eq!(settings.channels["Thinner"]["Speed"].levels[0].value, None);
    }
}
