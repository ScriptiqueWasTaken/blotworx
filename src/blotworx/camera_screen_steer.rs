//! dbc50 screen-border steering, followed by da1fc input filtering. This is
//! cursor/screen steering, not a world-space ledge collision callback.
use anyhow::{ensure, Result};
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, v: f32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
pub trait Backend {
    fn dimensions(&mut self) -> Result<[u32; 2]>;
    fn scalar(&mut self, offset: usize) -> Result<f32>;
    fn delta(&mut self) -> Result<f32>;
}
/// dac20..dacc4, AFTER active-owner and MouseCursor event-name admission.
/// These are normalized native event lanes, not editor pixel coordinates.
pub fn sample_cursor(s: &mut [u8], input: [f32; 2], b: &mut impl Backend) -> Result<()> {
    ensure!(s.len() >= 0x1a4, "Camera cursor owner too short");
    s.copy_within(0x17c..0x184, 0x19c);
    let dimensions = b.dimensions()?.map(|v| v as f32);
    for axis in 0..2 {
        let value = input[axis] * dimensions[axis];
        let value = if value > 0. { value } else { 0. };
        let value = if value < dimensions[axis] {
            value
        } else {
            dimensions[axis]
        };
        put(s, 0x17c + axis * 4, value);
    }
    Ok(())
}
/// dacd8..dad08, AFTER active-owner and On Screen event-name admission.
pub fn sample_on_screen(s: &mut [u8], value: f32, b: &mut impl Backend) -> Result<()> {
    ensure!(s.len() >= 0x198, "Camera on-screen owner too short");
    if value < 1. {
        let delta = b.delta()?;
        put(s, 0x194, f(s, 0x194) + delta);
    } else {
        put(s, 0x194, 0.);
    }
    Ok(())
}
pub fn update(
    s: &mut [u8],
    commands: &mut [f32; 2],
    gain: f32,
    b: &mut impl Backend,
) -> Result<()> {
    ensure!(s.len() >= 0x800, "Screen-steering owner too short");
    let [width, height] = b.dimensions()?.map(|v| v as f32);
    let bx = b.scalar(0x184)?;
    let x = f(s, 0x17c);
    let y = f(s, 0x180);
    // Preserve unordered branch behavior and lazy property reads.
    if !(x < bx) && !(x > width - bx) {
        let by = b.scalar(0x188)?;
        if !(y < by) && !(y > height - by) {
            put(s, 0x198, 0.);
            return Ok(());
        }
    }
    let start = b.scalar(0x6cc)?;
    let timer = f(s, 0x198);
    if timer > start {
        let delay = b.scalar(0x6d4)?;
        if f(s, 0x194) < delay {
            let bx = b.scalar(0x184)?;
            let px = b.scalar(0x18c)?;
            let py = b.scalar(0x190)?;
            let push_x = gain * (f32::from_bits(0x3c23d70a) * px);
            let push_y = gain * (f32::from_bits(0x3c23d70a) * py);
            if x > width - bx {
                let q = 1. - ((width - x) / bx);
                commands[0] = (-push_x).mul_add(q * q, commands[0]);
            }
            let bx = b.scalar(0x184)?;
            if x < bx {
                let q = 1. - x / bx;
                commands[0] = push_x.mul_add(q * q, commands[0]);
            }
            let by = b.scalar(0x188)?;
            if y > height - by {
                let q = 1. - ((height - y) / by);
                commands[1] = push_y.mul_add(q * q, commands[1]);
            }
            let by = b.scalar(0x188)?;
            if y < by {
                let q = 1. - y / by;
                commands[1] = (-push_y).mul_add(q * q, commands[1]);
            }
        } else {
            let offscreen_start = b.scalar(0x6d0)?;
            put(s, 0x198, start - offscreen_start);
        }
    } else {
        let delta = b.delta()?;
        put(s, 0x198, timer + delta);
    }
    Ok(())
}
struct WorldBackend<'a> {
    world: &'a super::assets::World,
    viewport: [u32; 2],
    delta: f32,
}
impl Backend for WorldBackend<'_> {
    fn dimensions(&mut self) -> Result<[u32; 2]> {
        Ok(self.viewport)
    }
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        super::camera_constructor::registered_slot_number(offset, &self.world.runtime_settings)
    }
    fn delta(&mut self) -> Result<f32> {
        Ok(self.delta)
    }
}
impl super::assets::World {
    /// Commands have already passed native direction retention and ramping.
    /// Screen point17c/180 and age194 must come from earlier native sampling.
    pub(super) fn filter_camera_commands(
        &self,
        s: &mut [u8],
        commands: &mut [f32; 2],
        viewport: [u32; 2],
        delta: f32,
    ) -> Result<()> {
        ensure!(s.len() >= 0x800, "Camera input owner too short");
        let controls = u32::from_be_bytes(s[0x634..0x638].try_into().unwrap());
        if controls & 8 != 0
            || super::camera_constructor::registered_boolean(
                "PlayerMouseSteering",
                &self.runtime_settings,
            )?
        {
            update(
                s,
                commands,
                1.,
                &mut WorldBackend {
                    world: self,
                    viewport,
                    delta,
                },
            )?;
        } else {
            put(s, 0x198, 0.);
        }
        let yaw = f(s, 0x170);
        let pitch = f(s, 0x174);
        let smoothing =
            super::camera_constructor::registered_slot_number(0x178, &self.runtime_settings)?;
        let yaw = smoothing.mul_add(commands[0] - yaw, yaw);
        put(s, 0x170, yaw);
        put(s, 0x354, f(s, 0x354) + yaw);
        put(s, 0x174, smoothing.mul_add(commands[1] - pitch, pitch));
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        events: Vec<usize>,
    }
    impl Backend for Host<'_> {
        fn dimensions(&mut self) -> Result<[u32; 2]> {
            Ok(serde_json::from_value(self.case["size"].clone())?)
        }
        fn scalar(&mut self, off: usize) -> Result<f32> {
            self.events.push(off);
            ensure!(
                self.case["fail"].as_u64() != Some(off as u64),
                "supplied scalar failure"
            );
            Ok(f32::from_bits(
                self.case["properties_bits"][off.to_string()]
                    .as_u64()
                    .unwrap() as u32,
            ))
        }
        fn delta(&mut self) -> Result<f32> {
            Ok(f32::from_bits(
                self.case["delta_bits"].as_u64().unwrap() as u32
            ))
        }
    }
    #[test]
    fn border_response_delays_and_ordered_failures_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_screen_steer.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut s = vec![0xa5; 0x800];
            for p in case["initial_words"].as_array().unwrap() {
                let off = p[0].as_u64().unwrap() as usize;
                s[off..off + 4].copy_from_slice(&(p[1].as_u64().unwrap() as u32).to_be_bytes());
            }
            let mut expected = s.clone();
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            let mut commands = [0.03123f32, -0.01723];
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let result = update(
                &mut s,
                &mut commands,
                f32::from_bits(case["gain_bits"].as_u64().unwrap() as u32),
                &mut host,
            );
            assert_eq!(result.is_err(), !case["error"].is_null());
            assert_eq!(json!(host.events), case["events"], "{case}");
            assert_eq!(
                json!(commands.map(f32::to_bits)),
                case["commands_bits"],
                "{case}"
            );
            assert_eq!(s, expected, "{case}");
        }
        Ok(())
    }
    #[test]
    fn cursor_history_clamping_and_on_screen_age_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_screen_input.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut s = vec![0xa5; 0x800];
            for p in case["initial_words"].as_array().unwrap() {
                let off = p[0].as_u64().unwrap() as usize;
                s[off..off + 4].copy_from_slice(&(p[1].as_u64().unwrap() as u32).to_be_bytes());
            }
            let mut expected = s.clone();
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            let input = std::array::from_fn(|i| {
                f32::from_bits(case["input_bits"][i].as_u64().unwrap() as u32)
            });
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            if case["mode"] == "cursor" {
                sample_cursor(&mut s, input, &mut host)?;
            } else {
                sample_on_screen(&mut s, input[0], &mut host)?;
            }
            assert_eq!(s, expected, "{case}");
            assert!(host.events.is_empty());
        }
        Ok(())
    }
}
