//! Generator da5a4..da6f8: automatic obstruction-yaw gates and prospective
//! position refresh. Gate snapshots originate earlier in the same d9650 call.
use super::camera_collision::V;
use anyhow::{ensure, Result};
#[derive(Clone, Copy, Debug)]
pub struct Gates {
    pub target_moved: bool,       // r29: XZ target movement >0.01 at d98d4
    pub input_hold_elapsed: bool, // r28: timer44c >= property440 at d9e80
}
pub trait Backend {
    fn dpad_steering(&mut self) -> Result<bool>;
    fn yaw(&mut self, state: &mut [u8], requested: &mut V, unfiltered: V) -> Result<bool>;
}
trait Math {
    fn sample(&mut self, kind: &str, angle: f32) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn sample(&mut self, kind: &str, angle: f32) -> Result<f32> {
        let (sin, cos) = super::native_angle::sin_cos_medium(angle as f64)?;
        Ok(if kind == "sin" {
            sin as f32
        } else {
            cos as f32
        })
    }
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn vector(s: &[u8], off: usize) -> V {
    std::array::from_fn(|i| f(s, off + 4 * i))
}
pub fn adjust(
    s: &mut [u8],
    prospective: &mut V,
    gates: Gates,
    scratch_w: f32,
    b: &mut impl Backend,
) -> Result<bool> {
    adjust_with(s, prospective, gates, scratch_w, b, &mut NativeMath)
}
fn adjust_with(
    s: &mut [u8],
    prospective: &mut V,
    gates: Gates,
    scratch_w: f32,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<bool> {
    ensure!(s.len() >= 0x800, "Camera yaw-pass owner too short");
    // Preserve the native branch comparisons, including their unordered case.
    // These timers are not consumed when the earlier target-movement gate won.
    if !gates.target_moved && !(f(s, 0x450) > 0.) && !(f(s, 0x454) < 0.5) {
        return Ok(false);
    }
    let controls = u32::from_be_bytes(s[0x634..0x638].try_into().unwrap());
    if controls & 5 != 0 || s[0x645] != 0 {
        return Ok(false);
    }
    if b.dpad_steering()? && !gates.input_hold_elapsed {
        return Ok(false);
    }
    if f(s, 0x5c4) != f32::MAX
        || u32::from_be_bytes(s[0x200..0x204].try_into().unwrap()) != 0
        || s[0xdd] != 0
    {
        return Ok(false);
    }
    let mut requested = vector(s, 0x350);
    let unfiltered = vector(s, 0x2f0);
    let result = b.yaw(s, &mut requested, unfiltered);
    // The native destination aliases owner350. Retain writes on failure, and
    // do not refresh the prospective view when the helper declines the change.
    for i in 0..4 {
        s[0x350 + i * 4..0x354 + i * 4].copy_from_slice(&requested[i].to_be_bytes());
    }
    let changed = result?;
    if !changed {
        return Ok(false);
    }
    ensure!(
        requested[..3].iter().all(|v| v.is_finite()),
        "Nonfinite corrected camera polar state"
    );
    let phi = f32::from_bits(0x3fc90fdb) - requested[2];
    let sp = math.sample("sin", phi)?;
    let sy = math.sample("sin", requested[1])?;
    let x = sy * (requested[0] * sp);
    let y = requested[0] * math.sample("cos", phi)?;
    let sp = math.sample("sin", phi)?;
    let cy = math.sample("cos", requested[1])?;
    let offset = [x, y, cy * (requested[0] * sp), scratch_w];
    let target = vector(s, 0x2d0);
    *prospective = std::array::from_fn(|i| target[i] + offset[i]);
    Ok(true)
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
    impl Backend for Host<'_> {
        fn dpad_steering(&mut self) -> Result<bool> {
            self.events.push(json!({"op":"dpad"}));
            ensure!(self.case["fail"] != "property", "supplied property failure");
            Ok(self.case["dpad"].as_bool().unwrap())
        }
        fn yaw(&mut self, s: &mut [u8], requested: &mut V, target: V) -> Result<bool> {
            self.events.push(json!({"op":"yaw","requested_bits":requested.map(f32::to_bits),"unfiltered_bits":target.map(f32::to_bits)}));
            requested[1] += 0.125;
            s[0x4d8] = 1;
            ensure!(self.case["fail"] != "yaw", "supplied yaw failure");
            Ok(self.case["helper_changed"].as_bool().unwrap())
        }
    }
    struct Replay<'a> {
        calls: std::slice::Iter<'a, Value>,
    }
    impl Math for Replay<'_> {
        fn sample(&mut self, kind: &str, angle: f32) -> Result<f32> {
            let c = self.calls.next().context("Unexpected yaw-pass math call")?;
            assert_eq!(c["kind"], kind);
            assert_eq!(c["input_bits"].as_u64().unwrap(), angle.to_bits() as u64);
            Ok(f32::from_bits(c["output_bits"].as_u64().unwrap() as u32))
        }
    }
    fn initial(case: &Value) -> Vec<u8> {
        let mut s = vec![0xa5; 0x800];
        for p in case["initial_words"].as_array().unwrap() {
            let off = p[0].as_u64().unwrap() as usize;
            s[off..off + 4].copy_from_slice(&(p[1].as_u64().unwrap() as u32).to_be_bytes());
        }
        s
    }
    #[test]
    fn yaw_gates_and_refresh_match_original_instructions() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_yaw_pass.json"
        ))?;
        assert_eq!(
            fixture["base_property"]["key"],
            super::super::camera_base::DPAD_STEERING.0
        );
        assert_eq!(
            fixture["base_property"]["default"],
            super::super::camera_base::DPAD_STEERING.1
        );
        assert_eq!(fixture["base_property"]["slot"], 0xc8);
        for case in fixture["cases"].as_array().unwrap() {
            let gates = Gates {
                target_moved: case["moved"].as_bool().unwrap(),
                input_hold_elapsed: case["hold"].as_bool().unwrap(),
            };
            let mut s = initial(case);
            let mut expected = s.clone();
            for p in case["changed"].as_array().unwrap() {
                expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
            }
            let prospective: V = serde_json::from_value(case["prospective"].clone())?;
            let mut output = prospective;
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let mut math = Replay {
                calls: case["math"].as_array().unwrap().iter(),
            };
            let result = adjust_with(
                &mut s,
                &mut output,
                gates,
                case["scratch_w"].as_f64().unwrap() as f32,
                &mut host,
                &mut math,
            );
            assert_eq!(result.is_err(), !case["error"].is_null(), "{case}");
            if let Ok(changed) = result {
                assert_eq!(json!(changed), case["changed_result"]);
            }
            assert!(math.calls.next().is_none());
            assert_eq!(host.events, case["events"].as_array().unwrap().as_slice());
            assert_eq!(json!(output.map(f32::to_bits)), case["output_bits"]);
            assert_eq!(s, expected, "Yaw-pass state differs: {case}");
            let mut direct = initial(case);
            let mut direct_output = prospective;
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let result = adjust(
                &mut direct,
                &mut direct_output,
                gates,
                case["scratch_w"].as_f64().unwrap() as f32,
                &mut host,
            );
            assert_eq!(result.is_err(), !case["error"].is_null());
            assert_eq!(direct, expected);
            assert_eq!(json!(direct_output.map(f32::to_bits)), case["output_bits"]);
        }
        Ok(())
    }
}
