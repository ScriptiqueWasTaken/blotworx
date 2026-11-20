//! EM1 e6f78..e72e8, the placement pass's second obstruction-distance stage.
//! Entries come from dceb8. These are not renderer-opacity commands.
use super::{camera_collision::FILTER, camera_distance::LookThrough, camera_placement::Overlap};
use anyhow::{ensure, Result};
pub trait Backend {
    /// Only XYZ is consumed by the query adapter. The original stack scratch
    /// fourth lane is not claimed to have a recovered value.
    fn overlap(&mut self, center: [f32; 3], radius: f32, filter: u32) -> Result<Vec<Overlap>>;
    fn allow_look_through(&mut self, body: usize) -> Result<bool>;
    fn material(&mut self, body: usize, key: u32) -> Result<u32>;
    fn entity(&mut self, body: usize) -> Result<usize>;
    fn scalar(&mut self, offset: usize) -> Result<f32>;
}
trait Math {
    fn sample(&mut self, kind: &str, value: f32) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn sample(&mut self, kind: &str, value: f32) -> Result<f32> {
        let (sin, cos) = super::native_angle::sin_cos_medium(value as f64)?;
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
fn put(s: &mut [u8], off: usize, value: f32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
fn polar(s: &[u8], math: &mut impl Math) -> Result<[f32; 3]> {
    let radius = f(s, 0x350);
    let yaw = f(s, 0x354);
    let pitch = f(s, 0x358);
    ensure!(
        [radius, yaw, pitch].into_iter().all(f32::is_finite),
        "Nonfinite look-through camera polar state"
    );
    let phi = f32::from_bits(0x3fc90fdb) - pitch;
    let sp = math.sample("sin", phi)?;
    let sy = math.sample("sin", yaw)?;
    let x = sy * (radius * sp);
    let y = radius * math.sample("cos", phi)?;
    let sp = math.sample("sin", phi)?;
    let cy = math.sample("cos", yaw)?;
    Ok([x, y, cy * (radius * sp)])
}
pub fn resolve(s: &mut [u8], entries: &[LookThrough], b: &mut impl Backend) -> Result<()> {
    resolve_with(s, entries, b, &mut NativeMath)
}
fn resolve_with(
    s: &mut [u8],
    entries: &[LookThrough],
    b: &mut impl Backend,
    math: &mut impl Math,
) -> Result<()> {
    ensure!(s.len() >= 0x800, "Look-through camera owner too short");
    if !entries.is_empty() {
        let offset = polar(s, math)?;
        let center = std::array::from_fn(|i| offset[i] + f(s, 0x2d0 + 4 * i));
        let radius = f(s, 0x580);
        ensure!(
            center.into_iter().all(f32::is_finite) && radius.is_finite() && radius >= 0.,
            "Invalid look-through overlap volume"
        );
        let hits = b.overlap(center, radius, FILTER)?;
        if !hits.is_empty() {
            let mut fraction = f(s, 0x384);
            for hit in hits {
                if !b.allow_look_through(hit.body)? {
                    continue;
                }
                if b.material(hit.body, hit.key)? & 0x18000 == 0 {
                    continue;
                }
                // e7100 repeats this getter for EVERY visited list record.
                // Do not collapse duplicates or reduce all matching records.
                for entry in entries {
                    if entry.entity == b.entity(hit.body)? {
                        ensure!(
                            entry.fraction.is_finite(),
                            "Nonfinite look-through entry fraction"
                        );
                        if entry.fraction < fraction {
                            fraction = entry.fraction;
                        }
                        break;
                    }
                }
            }
            let previous = f(s, 0x388);
            let slot = if fraction > previous && s[0x38d] != 0 && s[0x38c] != 0 {
                0x674
            } else {
                0x670
            };
            let coefficient = b.scalar(slot)?;
            ensure!(
                coefficient.is_finite(),
                "Nonfinite look-through coefficient"
            );
            let value = coefficient.mul_add(fraction - previous, previous);
            put(s, 0x388, value);
            s[0x38d] = 1;
            s[0x38c] = 1;
            put(s, 0x350, value * f(s, 0x5a0));
            return Ok(());
        }
        let previous = f(s, 0x388);
        let fraction = f(s, 0x384);
        let expanding = previous < fraction;
        let coefficient = b.scalar(if expanding { 0x670 } else { 0x674 })?;
        ensure!(
            coefficient.is_finite(),
            "Nonfinite look-through coefficient"
        );
        let value = coefficient.mul_add(fraction - previous, previous);
        put(s, 0x388, value);
        s[0x38d] = 0;
        s[0x38c] = 1;
        put(s, 0x350, value * f(s, 0x5a0));
        // e7264 jumps directly to return; e72a4 instead falls through to
        // e72e0 and overwrites the newly smoothed history with fraction384.
        if !expanding {
            put(s, 0x388, f(s, 0x384));
        }
        return Ok(());
    }
    if s[0x38c] != 0 && f(s, 0x388) < f(s, 0x384) {
        let previous = f(s, 0x388);
        put(s, 0x384, previous);
        put(s, 0x350, previous * f(s, 0x5a0));
    }
    s[0x38c] = 0;
    s[0x38d] = 0;
    put(s, 0x388, f(s, 0x384));
    Ok(())
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
        fn overlap(&mut self, center: [f32; 3], radius: f32, filter: u32) -> Result<Vec<Overlap>> {
            self.events.push(json!({"op":"overlap","center_bits":center.map(f32::to_bits),"radius_bits":radius.to_bits(),"filter":filter}));
            ensure!(self.case["fail"] != "query", "supplied query failure");
            Ok(self.case["hits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| Overlap {
                    body: h["body"].as_u64().unwrap() as usize,
                    key: h["key"].as_u64().unwrap() as u32,
                })
                .collect())
        }
        fn allow_look_through(&mut self, body: usize) -> Result<bool> {
            self.events.push(json!({"op":"look","body":body}));
            Ok(body != 20)
        }
        fn material(&mut self, body: usize, key: u32) -> Result<u32> {
            self.events
                .push(json!({"op":"material","body":body,"key":key}));
            Ok(if key == 6 { 0 } else { 0x10000 })
        }
        fn entity(&mut self, body: usize) -> Result<usize> {
            self.events.push(json!({"op":"entity","body":body}));
            Ok(body + 1000)
        }
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            self.events.push(json!({"op":"scalar","offset":offset}));
            ensure!(self.case["fail"] != "property", "supplied property failure");
            Ok(self.case["properties"][offset.to_string()]
                .as_f64()
                .unwrap() as f32)
        }
    }
    struct Replay<'a> {
        calls: std::slice::Iter<'a, Value>,
    }
    impl Math for Replay<'_> {
        fn sample(&mut self, kind: &str, value: f32) -> Result<f32> {
            let c = self
                .calls
                .next()
                .context("Unexpected look-through math call")?;
            assert_eq!(c["kind"], kind);
            assert_eq!(c["input_bits"].as_u64().unwrap(), value.to_bits() as u64);
            Ok(f32::from_bits(c["output_bits"].as_u64().unwrap() as u32))
        }
    }
    fn initial(case: &Value) -> Vec<u8> {
        let mut s = vec![0xa5; 0x800];
        for pair in case["initial_words"].as_array().unwrap() {
            let off = pair[0].as_u64().unwrap() as usize;
            s[off..off + 4].copy_from_slice(&(pair[1].as_u64().unwrap() as u32).to_be_bytes());
        }
        s
    }
    #[test]
    fn native_overlap_history_and_callback_order_match_retail_cases() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_lookthrough.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let entries = case["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| LookThrough {
                    fraction: e["fraction"].as_f64().unwrap() as f32,
                    entity: e["entity"].as_u64().unwrap() as usize,
                })
                .collect::<Vec<_>>();
            let mut s = initial(case);
            let mut expected = s.clone();
            for pair in case["changed"].as_array().unwrap() {
                expected[pair[0].as_u64().unwrap() as usize] = pair[1].as_u64().unwrap() as u8;
            }
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let mut math = Replay {
                calls: case["math"].as_array().unwrap().iter(),
            };
            let result = resolve_with(&mut s, &entries, &mut host, &mut math);
            assert_eq!(result.is_err(), !case["error"].is_null(), "{case}");
            assert!(math.calls.next().is_none());
            assert_eq!(
                host.events,
                case["events"].as_array().unwrap().as_slice(),
                "{case}"
            );
            let differences = s
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, (a, b))| (i, *a, *b))
                .collect::<Vec<_>>();
            assert!(
                differences.is_empty(),
                "Look-through differences {differences:?}; {case}"
            );
            // Exercise production transcendental composition as well as the
            // supplied-boundary arithmetic test; this is not new body proof.
            let mut direct = initial(case);
            let mut host = Host {
                case,
                events: Vec::new(),
            };
            let result = resolve(&mut direct, &entries, &mut host);
            assert_eq!(result.is_err(), !case["error"].is_null());
            assert_eq!(direct, expected);
            assert_eq!(host.events, case["events"].as_array().unwrap().as_slice());
        }
        Ok(())
    }
}
