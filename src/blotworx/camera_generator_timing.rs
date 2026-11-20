//! d9860..d9950 timers and the later d9e74 hold snapshot. Actor input axes
//! belong to9302c's owner+80/+84, not physical velocity or target movement.
use anyhow::{ensure, Result};
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, value: f32) {
    s[off..off + 4].copy_from_slice(&value.to_be_bytes());
}
/// Return r29, retained by the caller across intervening mode updates. Actor
/// resolution is lazy so its failure retains the preceding timer writes.
#[derive(Clone, Copy, Debug)]
pub struct Activity {
    pub target_moved: bool,
    pub actor_input_active: bool,
}
pub fn advance(
    s: &mut [u8],
    input: [f32; 2],
    delta: f32,
    actor: impl FnOnce() -> Result<[f32; 2]>,
) -> Result<Activity> {
    ensure!(s.len() >= 0x458, "Camera timing owner too short");
    // ble tests !GT on PPC, including unordered comparisons. Neither NaN
    // input by itself makes this timer active.
    if input[0].abs() > 0. || input[1].abs() > 0. {
        put(s, 0x44c, 0.);
    } else {
        put(s, 0x44c, f(s, 0x44c) + delta);
    }
    let dx = signed_difference(s, 0x2d0, 0x2e0);
    let dz = signed_difference(s, 0x2d8, 0x2e8);
    let moved = dx.abs() + dz.abs() > f32::from_bits(0x3c23d70a);
    if moved {
        put(s, 0x444, f(s, 0x444) + delta);
        put(s, 0x448, 0.);
    } else {
        put(s, 0x448, f(s, 0x448) + delta);
        put(s, 0x444, 0.);
    }
    let [x, y] = actor()?;
    let magnitude = super::cursor::native_sqrt(x.mul_add(x, y * y));
    let actor_input_active = magnitude > 0.;
    if actor_input_active {
        put(s, 0x454, 0.);
        put(s, 0x450, f(s, 0x450) + delta);
    } else {
        put(s, 0x450, 0.);
        put(s, 0x424, 0.);
        put(s, 0x454, f(s, 0x454) + delta);
    }
    Ok(Activity {
        target_moved: moved,
        actor_input_active,
    })
}
fn signed_difference(s: &[u8], current: usize, previous: usize) -> f32 {
    f(s, current) - f(s, previous)
}
/// Call at d9e74, after the intervening mode work, to capture r28. Do not
/// resample this timer later when applying yaw/placement gates.
pub fn snapshot(
    s: &[u8],
    target_moved: bool,
    hold_wait: f32,
) -> Result<super::camera_yaw_pass::Gates> {
    ensure!(s.len() >= 0x450, "Camera hold owner too short");
    Ok(super::camera_yaw_pass::Gates {
        target_moved,
        input_hold_elapsed: f(s, 0x44c) >= hold_wait,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn timers_actor_magnitude_and_gate_snapshots_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_generator_timing.json"
        ))?;
        let from_bits = |v: &Value| f32::from_bits(v.as_u64().unwrap() as u32);
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
            let mut events = Vec::new();
            let moved = advance(
                &mut s,
                std::array::from_fn(|i| from_bits(&case["input_bits"][i])),
                from_bits(&case["delta_bits"]),
                || {
                    events.push("actor");
                    ensure!(
                        !case["fail"].as_bool().unwrap(),
                        "supplied actor resolution failure"
                    );
                    Ok(std::array::from_fn(|i| from_bits(&case["axes_bits"][i])))
                },
            );
            assert_eq!(moved.is_err(), !case["error"].is_null());
            assert_eq!(json!(events), case["events"]);
            if let Ok(activity) = moved {
                assert_eq!(json!(activity.target_moved), case["moved"]);
                let gates = snapshot(&s, activity.target_moved, from_bits(&case["hold_bits"]))?;
                assert_eq!(json!(gates.input_hold_elapsed), case["hold_elapsed"]);
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
                "Timing byte differences {differences:?}: {case}"
            );
        }
        Ok(())
    }
}
