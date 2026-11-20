//! EntityTrackerComponent, retail REL318ef0..319bc8. Runtime callers own
//! entity handles, update admission, target motion and reset scheduling.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
use serde::Serialize;
use std::collections::BTreeMap;
pub type V = [f32; 3];
#[derive(Clone, Debug)]
pub struct Parameters {
    pub buffer_size: usize,
    pub offset: V,
    pub ground_on: bool,
    pub ground_states: Vec<i32>,
    pub special_states: Vec<i32>,
    pub special_offsets: Vec<V>,
    pub follow_threshold: f32,
    pub follow_scale: f32,
    pub follow_blend: f32,
}
fn vector(value: &serde_json::Value) -> Result<V> {
    let v = [
        value["x"].as_f64().context("Tracker X missing")? as f32,
        value["y"].as_f64().context("Tracker Y missing")? as f32,
        value["z"].as_f64().context("Tracker Z missing")? as f32,
    ];
    ensure!(v.iter().all(|v| v.is_finite()), "Nonfinite tracker offset");
    Ok(v)
}
impl Parameters {
    pub fn load(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "EntityTrackerComponent",
            "Wrong tracker component"
        );
        let get = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
                .with_context(|| format!("Tracker property {name} missing"))
        };
        let number = |name: &str| -> Result<f32> {
            let v = get(name)?.as_f64().context("Invalid tracker float")? as f32;
            ensure!(v.is_finite(), "Nonfinite tracker property {name}");
            Ok(v)
        };
        let integers = |name: &str| -> Result<Vec<i32>> {
            get(name)?
                .as_array()
                .context("Invalid tracker states")?
                .iter()
                .map(|v| {
                    i32::try_from(v.as_i64().context("Invalid tracker state")?).map_err(Into::into)
                })
                .collect()
        };
        let count = get("BufferSize")?
            .as_i64()
            .context("Invalid tracker buffer size")?;
        // Native property is i32 but backing allocation/count fields are u16.
        // Outside this range needs its original truncation/overflow behavior.
        ensure!(
            (0..=65535).contains(&count),
            "Tracker BufferSize requires native truncated-allocation handling"
        );
        let special_states = integers("SpecialStates")?;
        let special_offsets = get("SpecialStateOffsets")?
            .as_array()
            .context("Invalid tracker special offsets")?
            .iter()
            .map(vector)
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            special_offsets.len() >= special_states.len(),
            "Tracker special state has no matching offset"
        );
        Ok(Self {
            buffer_size: count as usize,
            offset: vector(get("TargetOffset")?)?,
            ground_on: get("GroundPlaneOn")?
                .as_bool()
                .context("Invalid GroundPlaneOn")?,
            ground_states: integers("GroundPlaneStates")?,
            special_states,
            special_offsets,
            follow_threshold: number("FollowUpThreshold")?,
            follow_scale: number("FollowUpScale")?,
            follow_blend: number("FollowUpBlend")?,
        })
    }
    pub fn target_point(
        &self,
        position: V,
        rotation: [f32; 9],
        motion_state: Option<i32>,
    ) -> Result<(V, V)> {
        ensure!(
            position.iter().chain(&rotation).all(|v| v.is_finite()),
            "Nonfinite tracker target pose"
        );
        let offset = motion_state
            .and_then(|state| self.special_states.iter().position(|&v| v == state))
            .map_or(self.offset, |i| self.special_offsets[i]);
        //319384..3c8: Y multiply, X fused add, then Z fused add.
        let offset = std::array::from_fn(|r| {
            rotation[r * 3 + 2].mul_add(
                offset[2],
                rotation[r * 3].mul_add(offset[0], rotation[r * 3 + 1] * offset[1]),
            )
        });
        Ok((std::array::from_fn(|i| position[i] + offset[i]), offset))
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    pub up: f32,
    pub down: f32,
    pub jump_up: f32,
    pub moving_up: f32,
    pub transition: f32,
    pub sync_distance: f32,
}
impl Tuning {
    pub fn load(settings: &BTreeMap<String, String>) -> Result<Self> {
        let read = |key: &str, default: f32| -> Result<f32> {
            let v = settings
                .get(key)
                .map(|s| s.parse::<f64>().with_context(|| format!("Invalid {key}")))
                .transpose()?
                .map_or(default, |v| v as f32);
            ensure!(v.is_finite(), "Nonfinite tracker configuration {key}");
            Ok(v)
        };
        Ok(Self {
            up: read("FreeCamera_GroundPlane_SpeedUp", 0.2)?,
            down: read("FreeCamera_GroundPlane_SpeedDown", 1.)?,
            jump_up: read("FreeCamera_FollowJump_SpeedUp", 0.2)?,
            moving_up: read("FreeCamera_GroundPlane_MovingSpeedUp", 0.2)?,
            transition: read("FreeCamera_GroundPlane_TransitionSpeed", 0.1)?,
            sync_distance: read("FreeCamera_GroundPlane_TransitionSyncDist", 0.1)?,
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    pub state: i32,
    pub support: u32,
    pub distance: f32,
    pub surface_velocity_y: f32,
    pub ground_point: V,
    pub maximum_speed: f32,
    pub inactive: bool,
}
impl Motion {
    pub fn eligible(self) -> bool {
        (matches!(self.support, 1 | 2) || matches!(self.state, 0 | 3) || self.distance == 0.)
            && self.state != 7
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct State {
    pub position: V,
    pub rotation: [f32; 9],
    pub history: Vec<V>,
    pub head: usize,
    pub ground_target: f32,
    pub height: f32,
    pub ground_offset: f32,
    pub eligible: bool,
    pub rotated_offset: V,
    pub follow_jump: bool,
    pub previous_eligible: bool,
    pub moving_surface: bool,
    pub moving_blend: f32,
    pub follow_boost: f32,
    pub previous_ground_on: bool,
    pub toggle_blending: bool,
    pub toggle_height: f32,
    pub inactive_held: bool,
    pub updates: u64,
}
impl State {
    pub fn reset(
        &mut self,
        p: &Parameters,
        position: V,
        rotation: [f32; 9],
        motion_state: Option<i32>,
    ) -> Result<()> {
        let (point, offset) = p.target_point(position, rotation, motion_state)?;
        self.history.clear();
        self.history.try_reserve(p.buffer_size)?;
        self.history.resize(p.buffer_size, point);
        self.head = 0;
        self.height = point[1];
        self.ground_target = point[1];
        self.eligible = true;
        self.ground_offset = 0.;
        self.follow_jump = false;
        self.position = point;
        self.rotation = rotation;
        self.rotated_offset = offset;
        Ok(())
    }
    pub fn update(
        &mut self,
        p: &Parameters,
        t: Tuning,
        position: V,
        rotation: [f32; 9],
        motion: Option<Motion>,
        blocked: bool,
    ) -> Result<()> {
        if blocked {
            return Ok(());
        }
        ensure!(
            self.history.len() == p.buffer_size,
            "Tracker history requires native BufferSize change/reset"
        );
        let (mut point, offset) = p.target_point(position, rotation, motion.map(|m| m.state))?;
        // Validate owned inputs before mutating history. Missing motion is an
        // actual native null branch; an unresolved motion record isn't null.
        if let Some(m) = motion {
            ensure!(
                [m.distance, m.surface_velocity_y, m.maximum_speed]
                    .iter()
                    .chain(&m.ground_point)
                    .all(|v| v.is_finite()),
                "Nonfinite tracker motion"
            );
        }
        self.rotated_offset = offset;
        if p.buffer_size > 1 {
            self.history[self.head] = point;
            self.head = (self.head + 1) % p.buffer_size;
            point = [0.; 3];
            for sample in &self.history {
                for i in 0..3 {
                    point[i] += sample[i];
                }
            }
            let inverse = 1. / p.buffer_size as f32;
            point = point.map(|v| v * inverse);
        }
        if self.previous_ground_on != p.ground_on {
            if !self.toggle_blending {
                self.toggle_height = self.height;
            }
            self.toggle_blending = true;
        }
        self.previous_ground_on = p.ground_on;
        if p.ground_on {
            self.ground_offset = point[1];
            if let Some(m) = motion {
                self.ground_plane(p, t, m, position, &mut point);
            }
            self.ground_offset -= point[1];
        } else {
            self.eligible =
                motion.is_some_and(|m| m.eligible() || p.ground_states.contains(&m.state));
            self.height = point[1];
            self.ground_offset = 0.;
        }
        if self.toggle_blending {
            self.toggle_height = t
                .transition
                .mul_add(self.height - self.toggle_height, self.toggle_height);
            if (self.height - self.toggle_height).abs() < t.sync_distance {
                self.toggle_blending = false;
            }
            point[1] = self.toggle_height;
        }
        self.position = point;
        self.rotation = rotation;
        self.updates += 1;
        Ok(())
    }
    fn ground_plane(
        &mut self,
        p: &Parameters,
        t: Tuning,
        m: Motion,
        target_position: V,
        point: &mut V,
    ) {
        let moving = m.surface_velocity_y != 0.;
        let eligible = m.eligible();
        self.eligible = eligible || p.ground_states.contains(&m.state);
        let floor = if m.maximum_speed == 0. || m.inactive {
            self.inactive_held = true;
            target_position
        } else if self.inactive_held {
            self.inactive_held = false;
            target_position
        } else {
            m.ground_point
        };
        if self.eligible {
            if moving {
                self.moving_surface = true;
                self.ground_target = floor[1] + self.rotated_offset[1];
            } else if eligible && self.previous_eligible {
                self.ground_target = floor[1] + self.rotated_offset[1];
            } else {
                self.ground_target = point[1];
            }
            self.previous_eligible = eligible;
            self.follow_jump = false;
        } else if moving && self.moving_surface {
            if floor[1] > self.ground_target {
                self.ground_target = self.rotated_offset[1] + floor[1];
            }
        } else if point[1] < self.ground_target {
            self.ground_target = point[1] + self.rotated_offset[1];
        }
        if self.eligible && !moving {
            self.moving_surface = false;
            self.moving_blend = t.up;
        } else if self.moving_surface {
            self.moving_blend = 0.1f32.mul_add(t.moving_up - self.moving_blend, self.moving_blend);
        }
        self.follow_boost = 0.;
        let follow = p.follow_scale * ((point[1] - self.height) - p.follow_threshold);
        if follow > 0. {
            let ratio = if p.follow_blend != 0. {
                follow / p.follow_blend
            } else {
                1.
            };
            self.follow_boost = if ratio < 1. {
                follow * (ratio * ratio)
            } else {
                follow
            };
        }
        let (destination, speed) = if point[1] < self.ground_target && !self.eligible {
            (point[1] + self.follow_boost, t.down)
        } else if self.follow_jump {
            (point[1] + self.follow_boost, t.jump_up)
        } else if self.eligible && moving {
            (self.ground_target + self.follow_boost, self.moving_blend)
        } else {
            (self.ground_target + self.follow_boost, t.up)
        };
        self.height = speed.mul_add(destination - self.height, self.height);
        point[1] = self.height;
    }
}
pub struct Runtime {
    pub owner: usize,
    pub target: usize,
    pub parameters: Parameters,
    pub tuning: Tuning,
    pub state: State,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parameters() -> Parameters {
        Parameters {
            buffer_size: 2,
            offset: [0., 0.7, 0.],
            ground_on: true,
            ground_states: vec![],
            special_states: vec![],
            special_offsets: vec![],
            follow_threshold: 0.5,
            follow_scale: 2.,
            follow_blend: 3.,
        }
    }
    const ID: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
    fn motion() -> Motion {
        Motion {
            state: 0,
            support: 2,
            distance: 0.,
            surface_velocity_y: 0.,
            ground_point: [0.; 3],
            maximum_speed: 5.,
            inactive: false,
        }
    }
    #[test]
    fn ground_filter_matches_retail_instruction_reference() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/entity_tracker_ground.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let n = |object: &serde_json::Value, key: &str| object[key].as_f64().unwrap() as f32;
            let flag = |key: &str| case["initial"][key].as_bool().unwrap();
            let v = |object: &serde_json::Value| -> V {
                std::array::from_fn(|i| object[i].as_f64().unwrap() as f32)
            };
            let i = &case["initial"];
            let mut state = State {
                ground_target: n(i, "ground_target"),
                height: n(i, "height"),
                rotated_offset: [0., n(i, "offset_y"), 0.],
                moving_blend: n(i, "moving_blend"),
                follow_boost: n(i, "follow_boost"),
                eligible: flag("eligible"),
                follow_jump: flag("follow_jump"),
                previous_eligible: flag("previous_eligible"),
                moving_surface: flag("moving_surface"),
                inactive_held: flag("inactive_held"),
                ..Default::default()
            };
            let mut p = parameters();
            p.ground_states = case["ground_states"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            p.follow_threshold = n(case, "follow_threshold");
            p.follow_scale = n(case, "follow_scale");
            p.follow_blend = n(case, "follow_blend");
            let t = &case["tuning"];
            let tuning = Tuning {
                up: n(t, "up"),
                down: n(t, "down"),
                jump_up: n(t, "jump_up"),
                moving_up: n(t, "moving_up"),
                transition: 0.1,
                sync_distance: 0.01,
            };
            let m = &case["motion"];
            let motion = Motion {
                state: m["state"].as_i64().unwrap() as i32,
                support: m["support"].as_u64().unwrap() as u32,
                distance: n(m, "distance"),
                surface_velocity_y: n(m, "surface_velocity_y"),
                ground_point: v(&m["ground_point"]),
                maximum_speed: n(m, "maximum_speed"),
                inactive: m["inactive"].as_bool().unwrap(),
            };
            let mut point = v(&case["point"]);
            state.ground_plane(&p, tuning, motion, v(&case["target_position"]), &mut point);
            for (key, value) in [
                ("ground_target", state.ground_target),
                ("height", state.height),
                ("offset_y", state.rotated_offset[1]),
                ("moving_blend", state.moving_blend),
                ("follow_boost", state.follow_boost),
            ] {
                assert_eq!(
                    value.to_bits(),
                    case["expected"]["float_bits"][key].as_u64().unwrap() as u32,
                    "{name}:{key}"
                );
            }
            for (key, value) in [
                ("eligible", state.eligible),
                ("follow_jump", state.follow_jump),
                ("previous_eligible", state.previous_eligible),
                ("moving_surface", state.moving_surface),
                ("inactive_held", state.inactive_held),
            ] {
                assert_eq!(
                    value,
                    case["expected"]["flags"][key].as_bool().unwrap(),
                    "{name}:{key}"
                );
            }
            for axis in 0..3 {
                assert_eq!(
                    point[axis].to_bits(),
                    case["expected"]["point_bits"][axis].as_u64().unwrap() as u32,
                    "{name}:point{axis}"
                );
            }
        }
        Ok(())
    }
    #[test]
    fn reset_history_rotation_gate_and_ground_toggle() -> Result<()> {
        let mut p = parameters();
        let t = Tuning::load(&BTreeMap::new())?;
        let mut state = State::default();
        state.reset(&p, [2., 3., 4.], ID, Some(0))?;
        assert_eq!(state.position, [2., 3.7, 4.]);
        assert_eq!(state.history, vec![[2., 3.7, 4.]; 2]);
        let previous = serde_json::to_value(&state)?;
        state.update(&p, t, [10., 3., 4.], ID, Some(motion()), true)?;
        assert_eq!(serde_json::to_value(&state)?, previous);
        p.ground_on = false;
        state.update(&p, t, [10., 3., 4.], ID, Some(motion()), false)?;
        assert_eq!(state.position, [6., 3.7, 4.]);
        assert_eq!(state.head, 1);
        state.update(&p, t, [14., 3., 4.], ID, Some(motion()), false)?;
        assert_eq!(state.position, [12., 3.7, 4.]);
        assert_eq!(state.head, 0);
        p.ground_on = true;
        state.update(&p, t, [14., 3., 4.], ID, Some(motion()), false)?;
        assert!(state.previous_ground_on);
        assert!(
            !state.toggle_blending,
            "Equal heights synchronize immediately"
        );
        p.ground_on = false;
        state.update(&p, t, [14., 10., 4.], ID, Some(motion()), false)?;
        assert!(state.toggle_blending);
        assert_eq!(state.updates, 4);
        Ok(())
    }
    #[test]
    fn offset_special_state_and_native_eligibility() -> Result<()> {
        let mut p = parameters();
        p.special_states = vec![7, 7];
        p.special_offsets = vec![[2., 3., 4.], [9.; 3]];
        let rotation = [0., 0., 1., 0., 1., 0., -1., 0., 0.];
        assert_eq!(
            p.target_point([10., 20., 30.], rotation, Some(7))?,
            ([14., 23., 28.], [4., 3., -2.])
        );
        assert_eq!(p.target_point([0.; 3], ID, None)?.0, [0., 0.7, 0.]);
        for support in 0..3 {
            for state in 0..12 {
                let m = Motion {
                    support,
                    state,
                    distance: 1.,
                    ..motion()
                };
                assert_eq!(
                    m.eligible(),
                    (support == 1 || support == 2 || state == 0 || state == 3) && state != 7
                );
                assert_eq!(Motion { distance: 0., ..m }.eligible(), state != 7);
            }
        }
        Ok(())
    }
    #[test]
    fn moving_surface_follow_boost_and_inactive_handoff() -> Result<()> {
        let p = parameters();
        let t = Tuning::load(&BTreeMap::new())?;
        let mut state = State::default();
        state.reset(&p, [0.; 3], ID, Some(0))?;
        state.previous_ground_on = true;
        let moving = Motion {
            surface_velocity_y: 2.,
            ground_point: [0., 2., 0.],
            ..motion()
        };
        state.update(&p, t, [0., 2., 0.], ID, Some(moving), false)?;
        assert!(state.moving_surface);
        assert!(state.moving_blend > 0. && state.moving_blend < t.moving_up);
        assert_eq!(state.ground_target, 2. + 0.7);
        assert!(state.follow_boost > 0.);
        let inactive = Motion {
            inactive: true,
            ..motion()
        };
        state.update(&p, t, [0., 3., 0.], ID, Some(inactive), false)?;
        assert!(state.inactive_held);
        state.update(&p, t, [0., 4., 0.], ID, Some(motion()), false)?;
        assert!(!state.inactive_held);
        assert_eq!(state.ground_target, 4. + 0.7);
        Ok(())
    }
}
