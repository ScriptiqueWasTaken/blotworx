//! e2eb4..e360c, desired-yaw portion of e2998. Earlier setup and later
//! desired pitch/look lanes stay ordered caller responsibilities.
use anyhow::{ensure, Context, Result};
type V = [f32; 4];
pub trait Backend {
    fn scalar(&mut self, offset: usize) -> Result<f32>;
    fn target(&mut self) -> Result<bool>;
    fn target_forward(&mut self) -> Result<V>;
    fn lock_position(&mut self, record: &[u8]) -> Result<V>;
    fn lookahead(&mut self, yaw: f32) -> Result<f32>;
    fn power(&mut self, ratio: f32, falloff: f32) -> Result<f32>;
}
trait Math {
    fn atan2(&mut self, y: f32, x: f32) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn atan2(&mut self, y: f32, x: f32) -> Result<f32> {
        Ok(super::native_angle::atan2(y as f64, x as f64) as f32)
    }
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn word(s: &[u8], off: usize) -> u32 {
    u32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, v: f32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn put_word(s: &mut [u8], off: usize, v: u32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn angle(left: f32, right: f32) -> Result<f32> {
    super::camera_wrapper::angle_difference(left, right)
}
fn sign(value: f32) -> f32 {
    if value > 0. {
        1.
    } else if value < 0. {
        -1.
    } else {
        0.
    }
}
fn lerp_angle(current: f32, target: f32, weight: f32) -> Result<f32> {
    let difference = angle(target, current)?;
    //147c84 fuses with zero, THEN adds current separately.
    Ok(current + weight.mul_add(difference - 0., 0.))
}
fn spring(s: &mut [u8], base: usize, error: f32) {
    let velocity = if error.abs() < f32::from_bits(0x38d1b717) {
        0.
    } else {
        let old = f(s, base + 8);
        old + f(s, base).mul_add(error, -(f(s, base + 4) * old))
    };
    put(s, base + 8, velocity);
}
pub fn yaw(
    s: &mut [u8],
    information: super::camera_base::Information,
    primary: Option<&mut [u8]>,
    b: &mut impl Backend,
) -> Result<()> {
    yaw_with(s, information, primary, b, &mut NativeMath)
}
fn yaw_with(
    s: &mut [u8],
    information: super::camera_base::Information,
    mut primary: Option<&mut [u8]>,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<()> {
    ensure!(s.len() >= 0x800, "Desired camera yaw owner too short");
    if word(s, 0x62c) != 0 {
        if b.target()? {
            let forward = b.target_forward()?;
            put(
                s,
                0x5a4,
                f32::from_bits(0x40490fdb) + math.atan2(forward[0], forward[2])?,
            );
        }
        put_word(s, 0x62c, word(s, 0x62c).wrapping_sub(1));
        return Ok(());
    }
    let hinted = f(s, 0x5c4);
    if hinted < f32::MAX {
        let error = angle(hinted, f(s, 0x5a4))?;
        if error.abs() - f32::from_bits(0x3a83126f) > f(s, 0x5e4) {
            let error = (-f(s, 0x5e4)).mul_add(sign(error), error);
            if f(s, 0x5f0) < f32::MAX {
                spring(s, 0x5f0, error);
                put(s, 0x5a4, f(s, 0x5a4) + f(s, 0x5f8));
            } else {
                let rate = b.scalar(0x688)?;
                put(s, 0x5a4, error.mul_add(rate, f(s, 0x5a4)));
            }
        } else {
            let bearing = math.atan2(
                information.transform[12] - f(s, 0x2d0),
                information.transform[14] - f(s, 0x2d8),
            )?;
            let error = angle(bearing, hinted)?;
            let desired = if error.abs() > f(s, 0x5e4) {
                f(s, 0x5e4).mul_add(sign(error), hinted)
            } else {
                bearing
            };
            put(s, 0x5a4, desired);
        }
        return Ok(());
    }
    if s[0x645] != 0 {
        put(s, 0x5a4, f(s, 0x364));
        return Ok(());
    }
    if word(s, 0x630) != 0 {
        put_word(s, 0x630, 0);
        return Ok(());
    }
    if word(s, 0x200) != 0 {
        let p = primary
            .as_deref_mut()
            .context("Desired yaw primary-lock record not bound")?;
        ensure!(p.len() >= 0x20, "Desired yaw primary-lock record too short");
        match word(p, 4) {
            1 => {
                let coefficient = if f(p, 0x10) < f32::MAX {
                    f(p, 0x10)
                } else {
                    b.scalar(0x248)?
                };
                super::camera_attributes::coefficient(s, 0x24c, coefficient);
                if f(s, 0x24c) == 0. {
                    put(s, 0x268, 0.);
                    let look = b.lookahead(f(s, 0x5a4))?;
                    put(s, 0x6ec, look);
                    put(s, 0x5b4, look);
                    put_word(p, 4, 2);
                } else {
                    spring(s, 0x24c, -f(s, 0x268));
                    let error = f(s, 0x268) + f(s, 0x254);
                    put(s, 0x268, error);
                    put(s, 0x5a4, f(s, 0x264) - error);
                }
            }
            2 => {
                put(s, 0x5a4, f(s, 0x264));
                if f(p, 0x10) == 0. {
                    let look = b.lookahead(f(s, 0x5a4))?;
                    put(s, 0x6ec, look);
                    put(s, 0x5b4, look);
                    put(p, 0x10, f32::MAX);
                }
                return Ok(());
            }
            3 => {
                let position = b.lock_position(p)?;
                let dx = f(s, 0x2d0) - position[0];
                let dz = f(s, 0x2d8) - position[2];
                let squared = dx * dx + dz * dz;
                let distance = b.scalar(0x284)?;
                let falloff = b.scalar(0x288)?;
                ensure!(
                    squared.is_finite() && squared >= 0.,
                    "Nonfinite soft-lock distance requires native compatibility handling"
                );
                let root = if squared == 0. {
                    0.
                } else {
                    squared * super::native_hk_transform::inverse_root(squared)?
                };
                let factor = b.power(root / distance, falloff)?;
                let factor = if factor < 1. { factor } else { 1. };
                let old = f(s, 0x268);
                let error = factor.mul_add(0. - old, old);
                put(s, 0x268, error);
                put(s, 0x5a4, f(s, 0x264) - error);
                if factor == 1. {
                    put_word(p, 4, 1);
                }
            }
            _ => return Ok(()),
        }
        if f(s, 0x268).abs() < f32::from_bits(0x3a83126f) {
            put_word(p, 4, 2);
        }
        return Ok(());
    }
    if word(s, 0x634) & 4 != 0 {
        if b.target()? {
            let rate = b.scalar(0x6c4)?;
            let forward = b.target_forward()?;
            let heading = f32::from_bits(0x40490fdb) + math.atan2(forward[0], forward[2])?;
            put(s, 0x5a4, lerp_angle(f(s, 0x5a4), heading, rate)?);
        }
        return Ok(());
    }
    if s[0xdd] != 0 {
        put(s, 0x5a4, f(s, 0x354));
        return Ok(());
    }
    let dx = information.transform[12] - f(s, 0x2d0);
    let dz = information.transform[14] - f(s, 0x2d8);
    if !(dx.abs() + dz.abs() > f32::from_bits(0x3a83126f)) {
        return Ok(());
    }
    let bearing = math.atan2(dx, dz)?;
    let selected = if f(s, 0x5f0) < f32::MAX {
        super::camera_attributes::coefficient(s, 0x42c, f32::MAX);
        Some(0x5f0)
    } else if f(s, 0x42c) < f32::MAX {
        Some(0x42c)
    } else {
        None
    };
    if let Some(base) = selected {
        spring(s, base, 1. - f(s, 0x43c));
        let blend = f(s, 0x43c) + f(s, base + 8);
        put(s, 0x43c, blend);
        put(s, 0x5a4, lerp_angle(f(s, 0x5a4), bearing, blend)?);
        if (1. - f(s, 0x43c)).abs() < f32::from_bits(0x3a83126f) {
            super::camera_attributes::coefficient(s, base, f32::MAX);
        }
    } else {
        put(s, 0x5a4, bearing);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::cell::Cell;
    fn math_call(case: &Value, index: &Cell<usize>, kind: &str, args: [f32; 2]) -> f32 {
        let call = &case["math"][index.get()];
        index.set(index.get() + 1);
        assert_eq!(call["kind"], kind);
        assert_eq!(json!(args.map(f32::to_bits)), call["args_bits"]);
        f32::from_bits(call["result_bits"].as_u64().unwrap() as u32)
    }
    struct Host<'a> {
        case: &'a Value,
        index: &'a Cell<usize>,
        events: Vec<Value>,
    }
    impl Host<'_> {
        fn event(&mut self, op: &str) -> Result<()> {
            self.events.push(json!({"op":op}));
            ensure!(self.case["fail"] != op, "supplied {op} failure");
            Ok(())
        }
        fn vector(&self, key: &str) -> V {
            std::array::from_fn(|i| f32::from_bits(self.case[key][i].as_u64().unwrap() as u32))
        }
    }
    impl Backend for Host<'_> {
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            self.events.push(json!({"op":"scalar","offset":offset}));
            ensure!(
                self.case["fail"].as_u64() != Some(offset as u64),
                "supplied scalar failure"
            );
            Ok(f32::from_bits(
                self.case["properties_bits"][offset.to_string()]
                    .as_u64()
                    .unwrap() as u32,
            ))
        }
        fn target(&mut self) -> Result<bool> {
            self.event("target")?;
            Ok(!self.case["target_bits"].is_null())
        }
        fn target_forward(&mut self) -> Result<V> {
            self.event("forward")?;
            Ok(self.vector("target_bits"))
        }
        fn lock_position(&mut self, _: &[u8]) -> Result<V> {
            self.event("lock_position")?;
            ensure!(
                !self.case["lock_position_bits"].is_null(),
                "supplied lock position failure"
            );
            Ok(self.vector("lock_position_bits"))
        }
        fn lookahead(&mut self, yaw: f32) -> Result<f32> {
            self.events
                .push(json!({"op":"lookahead","yaw_bits":yaw.to_bits()}));
            ensure!(
                self.case["fail"] != "lookahead",
                "supplied lookahead failure"
            );
            Ok(f32::from_bits(
                self.case["lookahead_bits"].as_u64().unwrap() as u32,
            ))
        }
        fn power(&mut self, ratio: f32, falloff: f32) -> Result<f32> {
            ensure!(self.case["fail"] != "power", "supplied math failure");
            Ok(math_call(self.case, self.index, "power", [ratio, falloff]))
        }
    }
    struct Replay<'a> {
        case: &'a Value,
        index: &'a Cell<usize>,
    }
    impl Math for Replay<'_> {
        fn atan2(&mut self, y: f32, x: f32) -> Result<f32> {
            ensure!(self.case["fail"] != "atan2", "supplied math failure");
            Ok(math_call(self.case, self.index, "atan2", [y, x]))
        }
    }
    #[test]
    fn desired_yaw_modes_lock_recovery_and_follow_history_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_desired_yaw.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            for direct in [false, true] {
                let mut s = vec![0xa5; 0x800];
                for p in case["initial_words"].as_array().unwrap() {
                    let off = p[0].as_u64().unwrap() as usize;
                    s[off..off + 4].copy_from_slice(&(p[1].as_u64().unwrap() as u32).to_be_bytes());
                }
                let mut expected = s.clone();
                for p in case["changed"].as_array().unwrap() {
                    expected[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
                }
                let mut primary = case["primary_bits"].as_array().map(|values| {
                    values
                        .iter()
                        .flat_map(|v| (v.as_u64().unwrap() as u32).to_be_bytes())
                        .collect::<Vec<_>>()
                });
                let mut information = super::super::camera_base::Information {
                    transform: [0.; 16],
                    field_of_view: 45.,
                    focus: [0.; 4],
                };
                for i in 0..4 {
                    information.transform[12 + i] =
                        f32::from_bits(case["position_bits"][i].as_u64().unwrap() as u32);
                }
                let index = Cell::new(0);
                let mut host = Host {
                    case,
                    index: &index,
                    events: Vec::new(),
                };
                let result = if direct {
                    yaw(&mut s, information, primary.as_deref_mut(), &mut host)
                } else {
                    yaw_with(
                        &mut s,
                        information,
                        primary.as_deref_mut(),
                        &mut host,
                        &mut Replay {
                            case,
                            index: &index,
                        },
                    )
                };
                assert_eq!(
                    result.is_err(),
                    !case["error"].is_null(),
                    "direct={direct} {case}"
                );
                assert_eq!(json!(host.events), case["events"], "direct={direct} {case}");
                if !direct {
                    assert_eq!(index.get(), case["math"].as_array().unwrap().len());
                }
                if let Some(primary) = primary {
                    assert_eq!(
                        json!(primary
                            .chunks_exact(4)
                            .map(|v| u32::from_be_bytes(v.try_into().unwrap()))
                            .collect::<Vec<_>>()),
                        case["primary_final_bits"],
                        "direct={direct} {case}"
                    );
                }
                let differences = s
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| (i, *a, *b))
                    .collect::<Vec<_>>();
                assert!(
                    differences.is_empty(),
                    "Desired yaw differences {differences:?}; direct={direct} {case}"
                );
            }
        }
        Ok(())
    }
}
