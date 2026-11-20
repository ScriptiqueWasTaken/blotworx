//! e738..e974: motion-surface projection and persistent AutoLevel pitch.
//! The input is motion+e0 (3068ac), not a replacement raycast normal.
use super::camera_collision::V;
use anyhow::{ensure, Result};
pub trait Backend {
    fn actor(&mut self) -> Result<bool>;
    fn motion(&mut self) -> Result<bool>;
    fn motion_vector(&mut self) -> Result<V>;
    fn scalar(&mut self, offset: usize) -> Result<f32>;
}
/// Caller-owned native motion binding. Acquiring the component precedes the
/// airborne test; reading its retained+e0 vector occurs only on the ground path.
pub trait MotionSource {
    fn motion(&mut self, actor: usize) -> Result<bool>;
    fn motion_vector(&mut self, actor: usize) -> Result<V>;
}
trait Math {
    fn sample(&mut self, kind: &str, args: &[f64]) -> Result<f64>;
}
struct NativeMath;
impl Math for NativeMath {
    fn sample(&mut self, kind: &str, args: &[f64]) -> Result<f64> {
        Ok(match kind {
            "sin" => super::native_angle::sin_cos_medium(args[0])?.0,
            "cos" => super::native_angle::sin_cos_medium(args[0])?.1,
            "atan2" => super::native_angle::atan2(args[0], args[1]),
            _ => unreachable!(),
        })
    }
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, value: f32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
fn root(squared: f32) -> Result<f32> {
    ensure!(
        squared.is_finite() && squared >= 0.,
        "Unsupported AutoLevel vector length"
    );
    Ok(if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    })
}
pub fn pitch(s: &mut [u8], b: &mut impl Backend) -> Result<f32> {
    pitch_with(s, b, &mut NativeMath)
}
fn pitch_with(s: &mut [u8], b: &mut impl Backend, math: &mut impl Math) -> Result<f32> {
    ensure!(s.len() >= 0x750, "AutoLevel owner too short");
    // Actor lookup precedes the enabled check. An absent motion controller or
    // airborne actor retains history; an absent actor/disabled feature decays it.
    if !b.actor()? || s[0x738] == 0 {
        let old = f(s, 0x73c);
        let coefficient = b.scalar(0x744)?;
        put(s, 0x73c, coefficient.mul_add(0. - old, old));
    } else if b.motion()? && s[0x1be] == 0 {
        let surface = b.motion_vector()?;
        let phi = f32::from_bits(0x3fc90fdb) - f(s, 0x358);
        let sp = math.sample("sin", &[phi as f64])? as f32;
        let sy = math.sample("sin", &[f(s, 0x354) as f64])? as f32;
        let x = sy * (f(s, 0x350) * sp);
        let y = f(s, 0x350) * math.sample("cos", &[phi as f64])? as f32;
        let sp = math.sample("sin", &[phi as f64])? as f32;
        let cy = math.sample("cos", &[f(s, 0x354) as f64])? as f32;
        let z = cy * (f(s, 0x350) * sp);
        let mut direction = [x, y, z];
        let length = root(z.mul_add(z, x * x) + y * y)?;
        if length > f32::from_bits(0x38d1b717) {
            let inverse = 1. / length;
            direction = direction.map(|v| v * inverse);
        }
        let projection = direction[0].mul_add(surface[0], direction[2] * surface[2]);
        let x = projection * direction[0];
        let z = projection * direction[2];
        let horizontal = root(x * x + z * z)?;
        let angle = math.sample("atan2", &[horizontal as f64, surface[1] as f64])? as f32;
        let sign = if projection > 0. {
            1.
        } else if projection < 0. {
            -1.
        } else {
            0.
        };
        let angle = angle * sign;
        let desired = -1. * angle;
        let old = f(s, 0x73c);
        let coefficient = b.scalar(0x744)?;
        put(s, 0x73c, coefficient.mul_add(desired - old, old));
    }
    let history = f(s, 0x73c);
    let coefficient = b.scalar(if history > 0. { 0x748 } else { 0x74c })?;
    Ok(history.mul_add(coefficient, f(s, 0x5a8)))
}
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        events: Vec<Value>,
    }
    impl Host<'_> {
        fn event(&mut self, name: &str) -> Result<()> {
            self.events.push(json!({"op":name}));
            ensure!(self.case["fail"] != name, "supplied {name} failure");
            Ok(())
        }
    }
    impl Backend for Host<'_> {
        fn actor(&mut self) -> Result<bool> {
            self.event("actor")?;
            Ok(self.case["actor"].as_bool().unwrap())
        }
        fn motion(&mut self) -> Result<bool> {
            self.event("motion")?;
            Ok(self.case["motion"].as_bool().unwrap())
        }
        fn motion_vector(&mut self) -> Result<V> {
            self.event("vector")?;
            Ok(serde_json::from_value(self.case["vector"].clone())?)
        }
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            self.events.push(json!({"op":"scalar","offset":offset}));
            ensure!(
                self.case["fail"].as_u64() != Some(offset as u64),
                "supplied scalar failure"
            );
            Ok(self.case["properties"][offset.to_string()]
                .as_f64()
                .unwrap() as f32)
        }
    }
    struct Replay<'a> {
        calls: std::slice::Iter<'a, Value>,
    }
    impl Math for Replay<'_> {
        fn sample(&mut self, kind: &str, args: &[f64]) -> Result<f64> {
            let call = self
                .calls
                .next()
                .context("Unexpected AutoLevel math call")?;
            assert_eq!(call["kind"], kind);
            assert_eq!(
                json!(args.iter().map(|v| v.to_bits()).collect::<Vec<_>>()),
                call["args_bits"]
            );
            Ok(f64::from_bits(call["output_bits"].as_u64().unwrap()))
        }
    }
    pub(super) fn initial(case: &Value) -> Vec<u8> {
        let mut s = vec![0xa5; 0x800];
        for p in case["initial_words"].as_array().unwrap() {
            let off = p[0].as_u64().unwrap() as usize;
            s[off..off + 4].copy_from_slice(&(p[1].as_u64().unwrap() as u32).to_be_bytes());
        }
        s
    }
    #[test]
    fn motion_projection_and_history_match_original_instructions() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_polar_prepare.json"
        ))?;
        for case in fixture["auto_level"].as_array().unwrap() {
            let mut expected = initial(case);
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            let mut s = initial(case);
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let mut math = Replay {
                calls: case["math"].as_array().unwrap().iter(),
            };
            let result = pitch_with(&mut s, &mut host, &mut math);
            assert_eq!(result.is_err(), !case["error"].is_null());
            if let Ok(value) = result {
                assert_eq!(json!(value.to_bits()), case["result_bits"], "{case}");
            }
            assert!(math.calls.next().is_none());
            assert_eq!(json!(host.events), case["events"]);
            assert_eq!(s, expected, "{case}");
            let mut direct = initial(case);
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let result = pitch(&mut direct, &mut host);
            assert_eq!(result.is_err(), !case["error"].is_null());
            if let Ok(value) = result {
                assert_eq!(
                    json!(value.to_bits()),
                    case["result_bits"],
                    "Production math: {case}"
                );
            }
            assert_eq!(direct, expected);
        }
        Ok(())
    }
}
