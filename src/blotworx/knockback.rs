//! Character motion state10, REL2205b0..220b4c. This ports its velocity/
//! support/animation lifecycle, not Havok's contact solver or complete action
//! arbitration. Admission scalar/rotation comes from305ed8..306230.
use super::{game_clock::Clock, support::State as Support};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Settings {
    pub maximum_time: f32,
    pub maximum_ground_distance: f32,
    pub launch_up_speed: f32,
    pub maximum_stimulus: f32,
    pub up_angle: f32,
    pub friction: f32,
}
impl Settings {
    pub fn load(values: &BTreeMap<String, String>, component: &Component) -> Result<Self> {
        let config = |name: &str, default: f32| -> Result<f32> {
            let value = values
                .get(name)
                .map(|value| value.trim().parse::<f32>())
                .transpose()
                .with_context(|| format!("Invalid {name}"))?
                .unwrap_or(default);
            ensure!(value.is_finite(), "Nonfinite {name}");
            Ok(value)
        };
        let property = |name: &str| -> Result<f32> {
            let value = component
                .properties
                .iter()
                .find(|p| p.name == name)
                .and_then(|p| p.value.as_f64())
                .with_context(|| format!("Missing native knockback property {name}"))?
                as f32;
            ensure!(value.is_finite(), "Nonfinite knockback property {name}");
            Ok(value)
        };
        Ok(Self {
            maximum_time: config("CharacterKnockbackMaxTime", 0.5)?,
            maximum_ground_distance: config("CharacterKnockbackMaxDistanceFromGround", 0.5)?,
            launch_up_speed: config("CharacterKnockbackLaunchUpSpeed", 5.)?,
            maximum_stimulus: property("KnockbackMaxStim")?,
            up_angle: property("KnockbackUpAngle")?,
            friction: property("KnockbackFriction")?,
        })
    }
}
///305fac requires the native health/dead latch, not an invented damage amount
/// threshold. The caller supplies concrete component enable/death state.
pub fn admits(
    amount: f32,
    maximum: f32,
    disabled: bool,
    already_active: bool,
    eligible: bool,
) -> bool {
    maximum > 0. && amount > 0. && !disabled && !already_active && eligible
}
pub fn launch(direction: [f32; 3], amount: f32, s: &Settings) -> Result<[f32; 4]> {
    ensure!(
        direction
            .into_iter()
            .chain([amount, s.up_angle, s.maximum_stimulus, s.launch_up_speed])
            .all(f32::is_finite),
        "Nonfinite knockback launch"
    );
    let yaw = super::native_angle::atan2(direction[0] as f64, direction[2] as f64) as f32;
    let (a, b) = super::native_angle::sin_cos_medium((-yaw) as f64)?;
    let (a, b) = (a as f32, b as f32);
    let (c, d) = super::native_angle::sin_cos_medium(yaw as f64)?;
    let (c, d) = (c as f32, d as f32);
    let angle = f32::from_bits(0x3c8efa35) * s.up_angle;
    let (e, f) = super::native_angle::sin_cos_medium(angle as f64)?;
    let (e, f) = (e as f32, f as f32);
    let left = [b, 0., a, 0., 0., 1., 0., 0., -a, 0., b, 0., 0., 0., 0., 0.];
    let right = [d, 0., c, 0., 0., 1., 0., 0., -c, 0., d, 0., 0., 0., 0., 0.];
    let tilt = [1., 0., 0., 0., 0., f, e, 0., 0., -e, f, 0., 0., 0., 0., 0.];
    let rotation = super::native_hk_transform::compose(
        &super::native_hk_transform::compose(&left, &tilt),
        &right,
    );
    let mut vector =
        super::native_hk_transform::rotate_vector(&rotation, [direction[0], 0., direction[2], 0.]);
    //2209d0 intentionally discards tilt Y, normalizes horizontal XYZ through
    //1fad0/1fab0, multiplies by min(stimulus,cap)*10, then writes fixed up speed.
    vector[1] = 0.;
    let square = vector[2].mul_add(vector[2], vector[0] * vector[0]) + vector[1] * vector[1];
    ensure!(square.is_finite(), "Knockback direction magnitude overflow");
    let length = if square == 0. {
        0.
    } else {
        let estimate = super::pose_blend::estimate(square);
        let correction = (-square).mul_add(estimate * estimate, 1.);
        square * correction.mul_add(0.5 * estimate, estimate)
    };
    if length > 0.0001 {
        let inverse = 1. / length;
        for v in &mut vector {
            *v *= inverse;
        }
    }
    let capped = if amount < s.maximum_stimulus {
        amount
    } else {
        s.maximum_stimulus
    };
    let speed = capped * 10.;
    for v in &mut vector {
        *v *= speed;
    }
    vector[1] = s.launch_up_speed;
    Ok(vector)
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct State {
    pub active: bool,
    pub phase: u32,
    pub launched: bool,
    pub started: u64,
    pub launch_velocity: [f32; 4],
    pub airborne_velocity: [f32; 4],
    pub admissions: u64,
    pub recoveries: u64,
    pub respawns: u32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Animation(u32),
    LandingSpeed(f32),
}
impl State {
    pub fn begin(&mut self, velocity: [f32; 4], respawns: u32) {
        self.active = true;
        self.phase = 0;
        self.launched = false;
        self.started = 0;
        self.launch_velocity = velocity;
        self.airborne_velocity = [0.; 4];
        self.admissions += 1;
        self.respawns = respawns;
    }
    /// Input is the current context velocity; output is committed once to the
    /// shared body. Native fall/recovery never runs the normal movement solver.
    pub fn advance(
        &mut self,
        velocity: &mut [f32; 4],
        support: Support,
        clock: &Clock,
        dt: f32,
        gravity: [f32; 2],
        s: &Settings,
        mut emit: impl FnMut(Effect),
    ) -> Result<()> {
        ensure!(
            dt.is_finite()
                && dt >= 0.
                && velocity.iter().chain(gravity.iter()).all(|v| v.is_finite()),
            "Invalid knockback state input"
        );
        if !self.active {
            return Ok(());
        }
        let gravity_step = |velocity: &mut [f32; 4]| {
            let g = if velocity[1] > 0. {
                gravity[0]
            } else {
                gravity[1]
            };
            // Preserve paired multiply/add lanes, including the padded W lane.
            for i in 0..4 {
                velocity[i] = (if i == 1 { g } else { 0. }).mul_add(dt, velocity[i]);
            }
        };
        match self.phase {
            0 => {
                self.phase = 1;
                self.started = clock.scaled_ticks.max(1);
            }
            1 => {
                if !self.launched {
                    emit(Effect::Animation(32));
                    *velocity = self.launch_velocity;
                    self.launched = true;
                }
                let elapsed = clock
                    .rate
                    .seconds(clock.scaled_ticks.wrapping_sub(self.started) as i64);
                if support != Support::Supported || elapsed > s.maximum_time {
                    self.phase = 2;
                }
            }
            2 => {
                if support == Support::Supported {
                    emit(Effect::LandingSpeed(-self.airborne_velocity[1]));
                    emit(Effect::Animation(33));
                    self.phase = 3;
                } else {
                    self.airborne_velocity = *velocity;
                }
                gravity_step(velocity);
            }
            3 => {
                let retain = 1. - s.friction;
                for value in velocity.iter_mut() {
                    *value *= retain;
                }
                if support == Support::Unsupported {
                    gravity_step(velocity);
                }
            }
            4 => {} // Recovery's motion switch follows; this phase has no update.
            _ => anyhow::bail!("Invalid native knockback phase {}", self.phase),
        }
        Ok(())
    }
    ///220944, after the state update. Native contact/body flags are supplied by
    /// the caller; a nearby ground query is not itself a supported contact.
    pub fn next_motion(
        &self,
        support: Support,
        contact_flag: bool,
        distance: f32,
        s: &Settings,
    ) -> Option<i32> {
        if !(3..=4).contains(&self.phase) {
            return None;
        }
        if support == Support::Unsupported && !contact_flag && distance > s.maximum_ground_distance
        {
            Some(2)
        } else if self.phase == 4 {
            Some(0)
        } else {
            None
        }
    }
    ///305a74 consumes ordinary animation id35. No duration guess releases it.
    pub fn animation(&mut self, id: u32) -> bool {
        if id != 35 {
            return false;
        }
        self.phase = 4;
        if self.active {
            self.active = false;
            self.recoveries += 1;
            return true;
        }
        false
    }
}

#[cfg(debug_assertions)]
pub fn validate_instructions() -> Result<()> {
    let data: serde_json::Value =
        serde_json::from_slice(&std::fs::read("tools/blotworx/fixtures/knockback.json")?)?;
    let support = |value: u64| match value {
        0 => Support::Unsupported,
        1 => Support::Sliding,
        2 => Support::Supported,
        _ => panic!("Invalid support fixture"),
    };
    let base = Settings {
        maximum_time: 0.5,
        maximum_ground_distance: 0.5,
        launch_up_speed: 5.,
        maximum_stimulus: 1.2,
        up_angle: 30.,
        friction: 0.25,
    };
    let bits = |v: [f32; 4]| serde_json::json!(v.map(f32::to_bits));
    for (index, case) in data["updates"]
        .as_array()
        .context("Missing knockback update corpus")?
        .iter()
        .enumerate()
    {
        let mut settings = base.clone();
        settings.friction = case["friction"].as_f64().unwrap() as f32;
        let mut state = State {
            active: true,
            phase: case["phase"].as_u64().unwrap() as u32,
            launched: case["launched"].as_bool().unwrap(),
            started: 1 << 20,
            launch_velocity: [3., 5., -4., 0.],
            airborne_velocity: [1., -7., 2., 0.],
            ..Default::default()
        };
        let mut clock = Clock::new(super::game_clock::Rate::from_bus_clock(4 << 20)?, 1 << 20);
        clock.scaled_ticks += (case["elapsed"].as_f64().unwrap() * (1 << 20) as f64) as u64;
        let mut velocity = [1., -2., 3., 0.];
        let mut events = Vec::new();
        state.advance(
            &mut velocity,
            support(case["support"].as_u64().unwrap()),
            &clock,
            case["dt"].as_f64().unwrap() as f32,
            [-30., -40.],
            &settings,
            |effect| {
                events.push(match effect {
                    Effect::Animation(id) => serde_json::json!(["animation", id]),
                    Effect::LandingSpeed(speed) => serde_json::json!(["landing", speed]),
                })
            },
        )?;
        ensure!(
            bits(velocity) == case["velocity_bits"],
            "Native knockback velocity mismatch in case {index}: {:?} vs {}",
            velocity,
            case["velocity_bits"]
        );
        ensure!(
            bits(state.airborne_velocity) == case["airborne_bits"],
            "Native knockback retained air velocity mismatch {index}"
        );
        ensure!(
            state.phase == case["output_phase"].as_u64().unwrap() as u32
                && state.launched == case["output_launched"].as_u64().unwrap().ne(&0),
            "Knockback phase mismatch {index}"
        );
        let expected = case["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event[0] != "reset")
            .cloned()
            .collect::<Vec<_>>();
        ensure!(
            events == expected,
            "Knockback event-order mismatch {index}: {events:?} vs {expected:?}"
        );
        if case["phase"] == 0 {
            ensure!(
                state.started == clock.scaled_ticks,
                "Native knockback timer reset lost the shared clock"
            );
        }
    }
    for case in data["next_states"].as_array().unwrap() {
        let state = State {
            active: true,
            phase: case["phase"].as_u64().unwrap() as u32,
            ..Default::default()
        };
        let actual = state.next_motion(
            support(case["support"].as_u64().unwrap()),
            case["flag"].as_bool().unwrap(),
            case["distance"].as_f64().unwrap() as f32,
            &base,
        );
        ensure!(
            serde_json::json!(actual.into_iter().collect::<Vec<_>>()) == case["requests"],
            "Native knockback support transition mismatch: {case}"
        );
    }
    for case in data["launches"].as_array().unwrap() {
        let mut settings = base.clone();
        settings.maximum_stimulus = case["cap"].as_f64().unwrap() as f32;
        settings.up_angle = case["angle"].as_f64().unwrap() as f32;
        //305ed8 is called only after305d84's positive-stimulus receiver gate.
        //Its direct zero-amount corpus case still enters state10; preserve that
        //distinction instead of claiming the lower helper is the whole receiver.
        let lower = settings.maximum_stimulus > 0.
            && !case["disabled"].as_bool().unwrap()
            && case["eligible"].as_bool().unwrap()
            && case["current"] != 10;
        ensure!(
            lower == (!case["requests"].as_array().unwrap().is_empty()),
            "Knockback lower-helper admission mismatch"
        );
        if lower {
            let direction = std::array::from_fn(|i| case["direction"][i].as_f64().unwrap() as f32);
            let actual = launch(
                direction,
                case["amount"].as_f64().unwrap() as f32,
                &settings,
            )?;
            ensure!(
                bits(actual) == case["launch_bits"],
                "Native knockback launch mismatch: {actual:?} vs {} for {case}",
                case["launch_bits"]
            );
        }
    }
    println!("PASS: original knockback instructions:360 updates,90 support transitions,21 launch/admission cases; exact velocity bits and event order");
    Ok(())
}
