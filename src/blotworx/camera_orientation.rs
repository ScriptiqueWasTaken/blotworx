//! Native final camera orientation e112c and basis builder1e29e0.
//! Does not construct/acquire or advance a gameplay camera controller.
use super::camera_collision::V;
use anyhow::{ensure, Result};
pub trait Backend {
    fn look_min_pitch(&mut self) -> Result<f32>;
    fn look_max_pitch(&mut self) -> Result<f32>;
    fn upside_down(&mut self) -> Result<bool>;
}
trait Math {
    fn sin(&mut self, v: f32) -> Result<f32>;
    fn cos(&mut self, v: f32) -> Result<f32>;
    fn atan2(&mut self, y: f32, x: f32) -> Result<f32>;
    fn acos(&mut self, v: f32) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn sin(&mut self, v: f32) -> Result<f32> {
        Ok(super::native_angle::sin_cos_medium(v as f64)?.0 as f32)
    }
    fn cos(&mut self, v: f32) -> Result<f32> {
        Ok(super::native_angle::sin_cos_medium(v as f64)?.1 as f32)
    }
    fn atan2(&mut self, y: f32, x: f32) -> Result<f32> {
        Ok(super::native_angle::atan2(y as f64, x as f64) as f32)
    }
    fn acos(&mut self, v: f32) -> Result<f32> {
        Ok(super::native_angle::acos(v as f64) as f32)
    }
}
fn scalar(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn length(v: V) -> Result<f32> {
    let sq = v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    ensure!(sq.is_finite(), "Native camera orientation length overflow");
    Ok(if sq == 0. {
        0.
    } else {
        sq * super::native_hk_transform::inverse_root(sq)?
    })
}
fn normalized(v: V) -> Result<V> {
    let len = length(v)?;
    Ok(if len > f32::from_bits(0x38d1b717) {
        let inverse = 1. / len;
        v.map(|x| x * inverse)
    } else {
        v
    })
}
/// Native paired cross-product storage has Y duplicated in W. The negated
/// fused forms for Y/Z differ from an algebraically reordered host cross.
fn cross(a: V, b: V) -> V {
    let x = a[1].mul_add(b[2], -(b[1] * a[2]));
    let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
    let z = -a[1].mul_add(b[0], -(b[1] * a[0]));
    [x, y, z, y]
}
///1e29e0: leaves all output untouched for a short vector. Reads the live upside
/// setting only after that test. Translation and other information are retained.
pub fn basis(transform: &mut [f32; 16], direction: V, b: &mut impl Backend) -> Result<bool> {
    ensure!(
        direction[..3].iter().all(|v| v.is_finite()),
        "Nonfinite native camera basis direction"
    );
    let len = length(direction)?;
    if len <= f32::from_bits(0x38d1b717) {
        return Ok(false);
    }
    let inverse = 1. / len;
    let direction = direction.map(|v| v * inverse);
    let up = [0., if b.upside_down()? { -1. } else { 1. }, 0., 0.];
    let right = normalized(cross(direction, up))?;
    let up = normalized(cross(right, direction))?;
    // Original setters1daf20,1daf34,1daf48, in this order.
    transform[8..12].copy_from_slice(&direction);
    transform[4..8].copy_from_slice(&up);
    transform[..4].copy_from_slice(&right.map(|v| -v));
    Ok(true)
}
/// Input scalar storage is the actual derived-controller owner. Eye position
/// is already set by the main update. There is no fallback identity view.
pub fn orient(
    state: &[u8],
    transform: &mut [f32; 16],
    unfiltered: V,
    b: &mut impl Backend,
) -> Result<()> {
    orient_with(state, transform, unfiltered, b, &mut NativeMath)
}
fn orient_with(
    state: &[u8],
    transform: &mut [f32; 16],
    unfiltered: V,
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<()> {
    ensure!(
        state.len() >= 0x764,
        "Native camera orientation owner is too short"
    );
    let raw = scalar(state, 0x384);
    ensure!(raw.is_finite(), "Nonfinite native camera distance fraction");
    let mut weight = if raw < 0. {
        0.
    } else if raw > 1. {
        1.
    } else {
        raw
    };
    if weight < 0.5 {
        weight = 1. - weight;
    }
    let other = 1. - weight;
    let target: V = std::array::from_fn(|i| {
        weight.mul_add(scalar(state, 0x300 + i * 4), unfiltered[i] * other)
    });
    let mut direction: V = std::array::from_fn(|i| target[i] - transform[12 + i]);
    let distance = length(direction)?;
    let mut yaw = math.atan2(direction[0], direction[2])?;
    let ratio = direction[1] / distance;
    // Source saturates only the acos argument's pole case. Coincident/NaN
    // directions need separate original coverage; never invent a look vector.
    ensure!(
        ratio.is_finite(),
        "Coincident/nonfinite final camera target is not reconstructed"
    );
    let angle = if ratio.abs() >= 1. {
        if ratio > 0. {
            0.
        } else {
            f32::from_bits(0x40490fdb)
        }
    } else {
        math.acos(ratio)?
    };
    let half_pi = f32::from_bits(0x3fc90fdb);
    let mut pitch = half_pi - angle;
    let yaw_offset = scalar(state, 0x5b4);
    if yaw_offset.abs() > f32::from_bits(0x3a83126f) {
        yaw -= yaw_offset;
    }
    let pitch_offset = scalar(state, 0x760) + scalar(state, 0x5b8);
    if pitch_offset.abs() > f32::from_bits(0x3a83126f) {
        pitch += pitch_offset;
    }
    // Source loads the lower value before the upper value, and tests
    // the lower endpoint first. Pointer-load order is not value-read order.
    let min = b.look_min_pitch()?;
    let max = b.look_max_pitch()?;
    ensure!(
        [pitch, yaw, max, min, distance]
            .into_iter()
            .all(f32::is_finite),
        "Nonfinite native final camera angles"
    );
    pitch = if pitch < min {
        min
    } else if pitch > max {
        max
    } else {
        pitch
    };
    let phi = half_pi - pitch;
    let sp = math.sin(phi)?;
    let sy = math.sin(yaw)?;
    direction[0] = sy * (distance * sp);
    direction[1] = distance * math.cos(phi)?;
    let sp = math.sin(phi)?;
    let cy = math.cos(yaw)?;
    direction[2] = cy * (distance * sp);
    // W remains the earlier target-minus-eye W, exactly as the native scratch
    // vector does when only XYZ are replaced by its polar reconstruction.
    basis(transform, direction, b)?;
    Ok(())
}
struct WorldBackend<'a>(&'a super::assets::World);
impl WorldBackend<'_> {
    fn number(&self, key: &str, default: &str) -> Result<f32> {
        use anyhow::Context;
        let value = self
            .0
            .runtime_settings
            .get(key)
            .map(String::as_str)
            .unwrap_or(default)
            .parse::<f64>()
            .with_context(|| format!("Invalid camera orientation property {key}"))?
            as f32;
        ensure!(
            value.is_finite(),
            "Nonfinite camera orientation property {key}"
        );
        Ok(value)
    }
}
impl Backend for WorldBackend<'_> {
    fn look_min_pitch(&mut self) -> Result<f32> {
        self.number("FreeCamera_Limits_LookMinPitch", "-0.8123")
    }
    fn look_max_pitch(&mut self) -> Result<f32> {
        self.number("FreeCamera_Limits_LookMaxPitch", "1.5123")
    }
    fn upside_down(&mut self) -> Result<bool> {
        use anyhow::Context;
        let text = self
            .0
            .runtime_settings
            .get("CameraUpsidedown")
            .map(String::as_str)
            .unwrap_or("false");
        match text {
            "false" => Ok(false),
            "true" => Ok(true),
            _ => {
                let value = text.parse::<f64>().context("Invalid CameraUpsidedown")?;
                ensure!(
                    value.is_finite()
                        && value.trunc() >= i32::MIN as f64
                        && value.trunc() <= i32::MAX as f64,
                    "Unsupported CameraUpsidedown conversion"
                );
                Ok(value.trunc() as i32 != 0)
            }
        }
    }
}
impl super::assets::World {
    /// Changes the generator's caller information; only the complete manager
    /// update is allowed to publish it as the active application view.
    pub(super) fn camera_orient_information(
        &self,
        state: &[u8],
        information: &mut super::camera_base::Information,
        unfiltered: V,
    ) -> Result<()> {
        orient(
            state,
            &mut information.transform,
            unfiltered,
            &mut WorldBackend(self),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Host<'a> {
        case: &'a serde_json::Value,
        events: Vec<String>,
    }
    impl Backend for Host<'_> {
        fn look_max_pitch(&mut self) -> Result<f32> {
            self.events.push("max".into());
            Ok(self.case["limits"][1].as_f64().unwrap() as f32)
        }
        fn look_min_pitch(&mut self) -> Result<f32> {
            self.events.push("min".into());
            Ok(self.case["limits"][0].as_f64().unwrap() as f32)
        }
        fn upside_down(&mut self) -> Result<bool> {
            self.events.push("upside".into());
            Ok(self.case["upside"].as_bool().unwrap())
        }
    }
    struct Replay<'a> {
        calls: &'a [serde_json::Value],
        next: usize,
    }
    impl Replay<'_> {
        fn sample(&mut self, name: &str, inputs: &[f64]) -> Result<f32> {
            let call = &self.calls[self.next];
            self.next += 1;
            assert_eq!(call["name"], name, "Math call {}", self.next);
            assert_eq!(
                serde_json::from_value::<Vec<u64>>(call["inputs"].clone())?,
                inputs.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                "Math argument {}",
                self.next
            );
            Ok(f32::from_bits(call["output"].as_u64().unwrap() as u32))
        }
    }
    impl Math for Replay<'_> {
        fn sin(&mut self, v: f32) -> Result<f32> {
            self.sample("sin", &[v as f64])
        }
        fn cos(&mut self, v: f32) -> Result<f32> {
            self.sample("cos", &[v as f64])
        }
        fn atan2(&mut self, y: f32, x: f32) -> Result<f32> {
            self.sample("atan2", &[y as f64, x as f64])
        }
        fn acos(&mut self, v: f32) -> Result<f32> {
            self.sample("acos", &[v as f64])
        }
    }
    fn write(s: &mut [u8], off: usize, v: f32) {
        s[off..off + 4].copy_from_slice(&v.to_be_bytes());
    }
    #[test]
    fn final_orientation_and_padded_basis_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_orientation.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut pose: [f32; 16] = std::array::from_fn(|i| {
                f32::from_bits(case["pose_bits"][i].as_u64().unwrap() as u32)
            });
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let mut math = Replay {
                calls: case["math_calls"].as_array().unwrap(),
                next: 0,
            };
            if case["kind"] == "basis" {
                basis(
                    &mut pose,
                    serde_json::from_value(case["direction"].clone())?,
                    &mut host,
                )?;
            } else {
                let mut state = vec![0; 0x800];
                write(&mut state, 0x384, case["fraction"].as_f64().unwrap() as f32);
                for (i, v) in [1., 2., 3., 4.].into_iter().enumerate() {
                    write(&mut state, 0x300 + i * 4, v);
                }
                for (off, lane) in [(0x5b4, 0), (0x5b8, 1), (0x760, 2)] {
                    write(
                        &mut state,
                        off,
                        case["offsets"][lane].as_f64().unwrap() as f32,
                    );
                }
                orient_with(&state, &mut pose, [2., 1., 8., 7.], &mut host, &mut math)?;
            }
            assert_eq!(
                pose.map(f32::to_bits),
                serde_json::from_value::<[u32; 16]>(case["output_bits"].clone())?,
                "Matrix case {index}"
            );
            assert_eq!(
                host.events,
                serde_json::from_value::<Vec<String>>(case["events"].clone())?,
                "Getter order case {index}"
            );
            assert_eq!(
                math.next,
                math.calls.len(),
                "Unused math leaves case {index}"
            );
        }
        Ok(())
    }
    #[test]
    fn native_math_ports_match_supplied_orientation_leaves() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_orientation.json"
        ))?;
        let mut math = NativeMath;
        for case in fixture["cases"].as_array().unwrap() {
            for call in case["math_calls"].as_array().unwrap() {
                let inputs: Vec<f32> = call["inputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| f64::from_bits(v.as_u64().unwrap()) as f32)
                    .collect();
                let result = match call["name"].as_str().unwrap() {
                    "sin" => math.sin(inputs[0])?,
                    "cos" => math.cos(inputs[0])?,
                    "atan2" => math.atan2(inputs[0], inputs[1])?,
                    "acos" => math.acos(inputs[0])?,
                    _ => unreachable!(),
                };
                assert_eq!(
                    result.to_bits(),
                    call["output"].as_u64().unwrap() as u32,
                    "{call}"
                );
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_gv_information_uses_native_orientation_and_live_upside_setting() -> Result<()> {
        use anyhow::Context;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.camera_lens.set_renderer_aspect(4. / 3.)?;
        for tick in 1..=6 {
            world
                .advance_models_with_delta(tick as f32 * super::super::STEP, super::super::STEP)?;
        }
        let original = world.camera_pipeline.output()?;
        let publications = world.camera_pipeline.publications;
        let mut state = vec![0; 0x800];
        write(&mut state, 0x384, 0.4);
        let target = [
            world.report.spawn[0],
            world.report.spawn[1] + 1.,
            world.report.spawn[2],
            0.,
        ];
        for i in 0..4 {
            write(&mut state, 0x300 + i * 4, target[i]);
        }
        let mut ordinary = original;
        world
            .runtime_settings
            .insert("CameraUpsidedown".into(), "false".into());
        world.camera_orient_information(&state, &mut ordinary, target)?;
        let mut inverted = original;
        world
            .runtime_settings
            .insert("CameraUpsidedown".into(), "true".into());
        world.camera_orient_information(&state, &mut inverted, target)?;
        assert_eq!(ordinary.transform[8..12], inverted.transform[8..12]);
        for i in 0..3 {
            assert_eq!(ordinary.transform[i], -inverted.transform[i]);
            assert_eq!(ordinary.transform[4 + i], -inverted.transform[4 + i]);
        }
        for information in [ordinary, inverted] {
            assert_eq!(information.transform[12..16], original.transform[12..16]);
            assert_eq!(
                information.field_of_view.to_bits(),
                original.field_of_view.to_bits()
            );
            assert_eq!(
                information.focus.map(f32::to_bits),
                original.focus.map(f32::to_bits)
            );
            let frame = super::super::nif_camera::Frame::from_information(
                information,
                world.camera_lens.sample(information.field_of_view),
            )?;
            assert!(frame
                .view()
                .iter()
                .chain(&frame.projection()?)
                .all(|v| v.is_finite()));
        }
        assert_eq!(
            world.camera_pipeline.publications, publications,
            "A helper must not take over application camera ownership"
        );
        assert_eq!(world.camera_pipeline.output()?, original);
        Ok(())
    }
}
