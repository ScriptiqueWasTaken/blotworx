//! Generator da6f8..daa04 and its final placement continuation. The earlier
//! target/pitch/yaw stages must supply the real prospective view and scratch W.
use super::{camera_base::Information, camera_collision::V, camera_distance::LookThrough};
use anyhow::{ensure, Context, Result};
pub trait Backend {
    fn distance(
        &mut self,
        state: &mut [u8],
        from: V,
        to: V,
        velocity: &mut f32,
        entries: &mut Vec<LookThrough>,
    ) -> Result<f32>;
    fn lookthrough(&mut self, state: &mut [u8], entries: &[LookThrough]) -> Result<()>;
    fn scalar(&mut self, offset: usize) -> Result<f32>;
    fn boolean(&mut self, offset: usize) -> Result<bool>;
    fn special_output(
        &mut self,
        state: &mut [u8],
        information: &mut Information,
        target: V,
        entries: &[LookThrough],
    ) -> Result<()>;
    fn finish(
        &mut self,
        state: &mut [u8],
        information: &mut Information,
        delta: f32,
        scratch_w: f32,
        entries: &[LookThrough],
    ) -> Result<()>;
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, value: f32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
fn vector(s: &[u8], off: usize) -> V {
    std::array::from_fn(|i| f(s, off + i * 4))
}
fn release(entries: &mut Vec<LookThrough>) {
    drop(std::mem::take(entries));
}
pub fn update(
    s: &mut [u8],
    information: &mut Information,
    prospective: V,
    delta: f32,
    scratch_w: f32,
    entries: &mut Vec<LookThrough>,
    b: &mut impl Backend,
) -> Result<()> {
    ensure!(s.len() >= 0x800, "Placement pass owner too short");
    ensure!(delta.is_finite(), "Nonfinite placement-pass delta");
    let controls = u32::from_be_bytes(s[0x634..0x638].try_into().unwrap());
    if controls & 1 == 0 || controls & 0x10 != 0 {
        // The native temporary array grows to max(8,2*capacity) below8.
        // It has no eight-contact cap and is released after the second pass.
        if entries.capacity() < 8 {
            let capacity = 8.max(2 * entries.capacity());
            entries.try_reserve_exact(capacity - entries.len())?;
        }
        let mut velocity = f(s, 0x370);
        let result = b.distance(s, vector(s, 0x2f0), prospective, &mut velocity, entries);
        // The original pointer aliases owner370. Preserve its writes on error.
        put(s, 0x370, velocity);
        let fraction = result?;
        put(s, 0x384, fraction);
        if s[0x1bf] != 0 {
            release(entries);
            return b.special_output(s, information, vector(s, 0x2f0), entries);
        }
        put(s, 0x350, f(s, 0x350) * fraction);
        b.lookthrough(s, entries)?;
        release(entries);
        if s[0x4d8] != 0 && b.boolean(0x2c0)? {
            let elapsed = f(s, 0x4dc) + delta;
            put(s, 0x4dc, elapsed);
            if elapsed > b.scalar(0x2c4)? {
                s[0x4b0..0x4b4].copy_from_slice(&2u32.to_be_bytes());
            }
        }
    } else if s[0x7f3] == 0 {
        let error = f(s, 0x5a0) - f(s, 0x360);
        let coefficient = b.scalar(0x3e8)?;
        ensure!(
            coefficient.is_finite(),
            "Nonfinite outward camera spring property"
        );
        super::camera_attributes::coefficient(s, 0x3dc, coefficient);
        let previous = f(s, 0x370);
        put(s, 0x3e4, previous);
        let next = if error.abs() < f32::from_bits(0x38d1b717) {
            0.
        } else {
            previous + f(s, 0x3dc).mul_add(error, -(f(s, 0x3e0) * previous))
        };
        put(s, 0x3e4, next);
        put(s, 0x370, next);
        if -next >= f(s, 0x384) * f(s, 0x5a0) {
            put(s, 0x370, 0.);
        }
        let radius = f(s, 0x384).mul_add(f(s, 0x5a0), f(s, 0x370));
        put(s, 0x350, radius);
        put(s, 0x384, radius / f(s, 0x5a0));
    }
    b.finish(s, information, delta, scratch_w, entries)
}
struct WorldBackend<'a>(&'a super::assets::World);
impl super::camera_yaw_pass::Backend for WorldBackend<'_> {
    fn dpad_steering(&mut self) -> Result<bool> {
        super::camera_constructor::registered_boolean(
            super::camera_base::DPAD_STEERING.0,
            &self.0.runtime_settings,
        )
    }
    fn yaw(&mut self, s: &mut [u8], requested: &mut V, unfiltered: V) -> Result<bool> {
        let mut state = super::camera_placement::YawState {
            previous_polar: vector(s, 0x360),
            target: vector(s, 0x2d0),
            last_target: vector(s, 0x2e0),
            acquired_inside: s[0x4d8] != 0,
        };
        let result =
            self.0
                .camera_adjust_placement_yaw(&mut state, requested, unfiltered, f(s, 0x580));
        s[0x4d8] = state.acquired_inside as u8;
        result
    }
}
impl Backend for WorldBackend<'_> {
    fn distance(
        &mut self,
        s: &mut [u8],
        from: V,
        to: V,
        v: &mut f32,
        entries: &mut Vec<LookThrough>,
    ) -> Result<f32> {
        self.0.camera_solve_distance(s, from, to, v, true, entries)
    }
    fn lookthrough(&mut self, s: &mut [u8], entries: &[LookThrough]) -> Result<()> {
        self.0.camera_resolve_lookthrough(s, entries)
    }
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        let key = match offset {
            0x2c4 => "FreeCamera_Thinned_MoveThroughTimeLimit",
            0x3e8 => "FreeCamera_Spring_Distance_Out",
            _ => anyhow::bail!("Unbound placement property {offset:x}"),
        };
        super::camera_constructor::registered_number(key, &self.0.runtime_settings)
    }
    fn boolean(&mut self, offset: usize) -> Result<bool> {
        ensure!(offset == 0x2c0, "Unbound placement boolean {offset:x}");
        super::camera_constructor::registered_boolean(
            "FreeCamera_Thinned_MoveThroughOnTimeLimit",
            &self.0.runtime_settings,
        )
    }
    fn special_output(
        &mut self,
        s: &mut [u8],
        info: &mut Information,
        target: V,
        _: &[LookThrough],
    ) -> Result<()> {
        self.0.camera_orient_information(s, info, target)
    }
    fn finish(
        &mut self,
        s: &mut [u8],
        info: &mut Information,
        delta: f32,
        w: f32,
        _: &[LookThrough],
    ) -> Result<()> {
        self.0.camera_finish_placement(s, info, delta, w)
    }
}
impl super::assets::World {
    pub(super) fn camera_complete_generated_placement(
        &self,
        state: &mut [u8],
        information: &mut Information,
        prospective: &mut V,
        gates: super::camera_yaw_pass::Gates,
        delta: f32,
        scratch_w: f32,
        entries: &mut Vec<LookThrough>,
    ) -> Result<()> {
        let selected = i32::from_be_bytes(
            state
                .get(0x14..0x18)
                .context("Placement profile index absent")?
                .try_into()
                .unwrap(),
        );
        ensure!(
            selected == self.camera_profiles.controller.selection.current,
            "Placement pass/profile owner mismatch"
        );
        let mut backend = WorldBackend(self);
        super::camera_yaw_pass::adjust(state, prospective, gates, scratch_w, &mut backend)?;
        update(
            state,
            information,
            *prospective,
            delta,
            scratch_w,
            entries,
            &mut backend,
        )
    }
    pub(super) fn camera_complete_placement_pass(
        &self,
        state: &mut [u8],
        information: &mut Information,
        prospective: V,
        delta: f32,
        scratch_w: f32,
        entries: &mut Vec<LookThrough>,
    ) -> Result<()> {
        let selected = i32::from_be_bytes(
            state
                .get(0x14..0x18)
                .context("Placement profile index absent")?
                .try_into()
                .unwrap(),
        );
        ensure!(
            selected == self.camera_profiles.controller.selection.current,
            "Placement pass/profile owner mismatch"
        );
        update(
            state,
            information,
            prospective,
            delta,
            scratch_w,
            entries,
            &mut WorldBackend(self),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        events: Vec<Value>,
    }
    fn bits(s: &[u8]) -> [u32; 3] {
        [
            f(s, 0x350).to_bits(),
            f(s, 0x370).to_bits(),
            f(s, 0x384).to_bits(),
        ]
    }
    impl Backend for Host<'_> {
        fn distance(
            &mut self,
            s: &mut [u8],
            from: V,
            to: V,
            v: &mut f32,
            entries: &mut Vec<LookThrough>,
        ) -> Result<f32> {
            self.events.push(json!({"op":"distance","from_bits":from.map(f32::to_bits),"to_bits":to.map(f32::to_bits),"state_bits":bits(s),"follow":1}));
            *v += 0.25;
            entries.extend([
                LookThrough {
                    fraction: 0.2,
                    entity: 42,
                },
                LookThrough {
                    fraction: 0.3,
                    entity: 43,
                },
            ]);
            ensure!(self.case["fail"] != "distance", "supplied distance failure");
            Ok(0.4)
        }
        fn lookthrough(&mut self, s: &mut [u8], entries: &[LookThrough]) -> Result<()> {
            self.events
                .push(json!({"op":"lookthrough","state_bits":bits(s),"entries":entries.len()}));
            ensure!(
                self.case["fail"] != "lookthrough",
                "supplied lookthrough failure"
            );
            put(s, 0x350, f(s, 0x350) + 0.25);
            Ok(())
        }
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            self.events.push(json!({"op":"scalar","offset":offset}));
            ensure!(self.case["fail"] != "scalar", "supplied scalar failure");
            Ok(self.case["properties"][offset.to_string()]
                .as_f64()
                .unwrap() as f32)
        }
        fn boolean(&mut self, offset: usize) -> Result<bool> {
            self.events.push(json!({"op":"boolean","offset":offset}));
            ensure!(self.case["fail"] != "boolean", "supplied boolean failure");
            Ok(self.case["properties"][offset.to_string()]
                .as_bool()
                .unwrap())
        }
        fn special_output(
            &mut self,
            s: &mut [u8],
            _: &mut Information,
            target: V,
            entries: &[LookThrough],
        ) -> Result<()> {
            self.events.push(json!({"op":"special","state_bits":bits(s),"target_bits":target.map(f32::to_bits),"entries":entries.len()}));
            ensure!(self.case["fail"] != "special", "supplied special failure");
            Ok(())
        }
        fn finish(
            &mut self,
            s: &mut [u8],
            _: &mut Information,
            delta: f32,
            w: f32,
            entries: &[LookThrough],
        ) -> Result<()> {
            self.events.push(json!({"op":"finish","state_bits":bits(s),"delta_bits":delta.to_bits(),"scratch_w_bits":w.to_bits(),"entries":entries.len()}));
            ensure!(self.case["fail"] != "finish", "supplied finish failure");
            Ok(())
        }
    }
    #[test]
    fn final_distance_pass_matches_original_control_flow_and_partial_writes() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_placement_pass.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut state = vec![0xa5; 0x800];
            for pair in case["initial_words"].as_array().unwrap() {
                let off = pair[0].as_u64().unwrap() as usize;
                state[off..off + 4]
                    .copy_from_slice(&(pair[1].as_u64().unwrap() as u32).to_be_bytes());
            }
            let mut expected = state.clone();
            for pair in case["changed"].as_array().unwrap() {
                expected[pair[0].as_u64().unwrap() as usize] = pair[1].as_u64().unwrap() as u8;
            }
            let mut information = Information {
                transform: [3.; 16],
                field_of_view: 63.,
                focus: [1.; 4],
            };
            let mut entries = Vec::new();
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let result = update(
                &mut state,
                &mut information,
                serde_json::from_value(case["prospective"].clone())?,
                case["delta"].as_f64().unwrap() as f32,
                case["scratch_w"].as_f64().unwrap() as f32,
                &mut entries,
                &mut host,
            );
            assert_eq!(result.is_err(), !case["error"].is_null(), "{case}");
            assert_eq!(
                host.events,
                case["events"].as_array().unwrap().as_slice(),
                "{case}"
            );
            let differences = state
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(i, (a, b))| !(0x7f4..0x7fc).contains(i) && a != b)
                .map(|(i, (a, b))| (i, *a, *b))
                .collect::<Vec<_>>();
            assert!(
                differences.is_empty(),
                "Placement-pass differences {differences:?}; {case}"
            );
            assert_eq!(
                json!(entries
                    .iter()
                    .map(|e| json!({"fraction_bits":e.fraction.to_bits(),"entity":e.entity}))
                    .collect::<Vec<_>>()),
                case["entries"]
            );
        }
        Ok(())
    }
}
