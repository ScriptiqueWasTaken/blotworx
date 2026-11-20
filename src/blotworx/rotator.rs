//! Scalar part of EM1 RotatorComponent (retail REL 0x3b2c68..0x3b32cc).
//! Entity clocks, target-entity resolution, axis transforms, event dispatch and
//! physics binding must be supplied by the component runtime. World connects
//! the free/limited paths in rotator_runtime; NIF animation is independent.
use anyhow::{ensure, Result};

// Exact constants at REL 0x4d0d58 / 0x4d0d5c / 0x4d0d74.
const DEGREES: f32 = f32::from_bits(0x3c8efa35);
const TAU: f32 = f32::from_bits(0x40c90fdb);
const PI: f32 = f32::from_bits(0x40490fdb);

#[derive(Clone, Copy)]
pub struct Parameters {
    pub max_speed_degrees: f32,
    pub acceleration_degrees: f32,
    pub min_degrees: i32,
    pub max_degrees: i32,
    pub auto_reverse: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct State {
    pub speed: f32,
    pub stopping_distance: f32,
    pub angle: f32,
    pub reversed: bool,
}

#[derive(Debug, Default)]
pub struct Step {
    pub delta: f32,
    /// Both limit and target completion use event 0x0d in the retail update.
    pub completion_event: bool,
}

impl State {
    /// REL 0x3b2a40: initialization starts at Max Speed, not at rest.
    pub fn new(max_speed_degrees: f32, reversed: bool) -> Result<Self> {
        ensure!(max_speed_degrees.is_finite(), "Nonfinite rotator speed");
        Ok(Self {
            speed: max_speed_degrees * DEGREES,
            stopping_distance: 0.,
            angle: 0.,
            reversed,
        })
    }

    /// REL 0x3b31bc. Keep the separate single-precision operations: the
    /// signed stopping distance is observable in the limit-braking branch.
    fn approach_speed(&mut self, dt: f32, target: f32, acceleration: f32) {
        if self.speed != target {
            if acceleration == 0. {
                self.speed = target;
            } else {
                let amount = (acceleration * dt).abs();
                self.speed = if self.speed < target {
                    (self.speed + amount).min(target)
                } else {
                    (self.speed - amount).max(target)
                };
            }
        }
        self.stopping_distance = if acceleration == 0. {
            0.
        } else {
            (self.speed / acceleration).abs() * (self.speed * 0.5)
        };
    }

    // REL 0x3b32cc free rotation; 0x3b3770 position-target branch. The caller
    // must resolve entity tracking separately, not silently pass None for it.
    fn requested_delta(&mut self, dt: f32, target: Option<f32>) -> Result<f32> {
        if let Some(target) = target {
            let mut delta = wrap(target - self.angle, PI)?;
            let distance = self.speed * dt;
            if delta > 0. {
                if delta > distance {
                    delta = distance;
                }
                self.reversed = false;
            } else if delta < 0. {
                if delta < -distance {
                    delta = -distance;
                }
                self.reversed = true;
            }
            Ok(delta)
        } else {
            let direction = if self.reversed { -1. } else { 1. };
            Ok(dt * (self.speed * direction))
        }
    }

    /// `target` is a resolved angle in radians, as consumed by the native
    /// RotateToPosition path. Disabled returns before reading the entity clock.
    /// Zero dt still updates speed, matching the retail order.
    pub fn tick(
        &mut self,
        p: Parameters,
        dt: f32,
        disabled: bool,
        target: Option<f32>,
    ) -> Result<Step> {
        if disabled {
            return Ok(Step::default());
        }
        ensure!(
            [
                dt,
                p.max_speed_degrees,
                p.acceleration_degrees,
                self.speed,
                self.angle
            ]
            .iter()
            .all(|v| v.is_finite())
                && target.is_none_or(f32::is_finite),
            "Nonfinite rotator input"
        );
        // Commit only after validation/arithmetic succeeds.
        let mut next = *self;
        let old_speed = next.speed;
        let acceleration = p.acceleration_degrees * DEGREES;
        next.approach_speed(dt, p.max_speed_degrees * DEGREES, acceleration);
        if dt == 0. || next.speed == 0. {
            *self = next;
            return Ok(Step::default());
        }
        let delta = next.requested_delta(dt, target)?;
        let mut angle = next.angle + delta;
        let mut reached_limit = false;
        if p.min_degrees != 0 || p.max_degrees != 0 {
            let upper = DEGREES * p.max_degrees as f32;
            let lower = DEGREES * p.min_degrees as f32;
            if delta > 0. && angle >= upper {
                angle = upper;
                next.speed = 0.;
                reached_limit = next.angle < upper;
            } else if delta < 0. && angle <= lower {
                angle = lower;
                next.speed = 0.;
                reached_limit = next.angle > lower;
            } else if (delta > 0. && next.angle + next.stopping_distance >= upper)
                || (delta < 0. && next.angle - next.stopping_distance <= lower)
            {
                next.speed = old_speed;
                next.approach_speed(dt, 0., acceleration);
                angle = next.angle + next.requested_delta(dt, target)?;
            }
        }
        angle = wrap(angle, TAU)?;
        let delta = angle - next.angle;
        let completion_event = if reached_limit {
            if p.auto_reverse {
                next.reversed = !next.reversed;
            }
            true
        } else {
            target.is_some_and(|t| next.angle + delta == t)
        };
        // REL 0x3b3968 adds the actual applied delta, rather than assigning
        // the earlier candidate angle; preserve its rounding boundary.
        if delta != 0. {
            next.angle += delta;
        }
        ensure!(
            [next.speed, next.stopping_distance, next.angle, delta]
                .iter()
                .all(|v| v.is_finite()),
            "Rotator arithmetic overflow"
        );
        *self = next;
        Ok(Step {
            delta,
            completion_event,
        })
    }
}

fn wrap(mut angle: f32, limit: f32) -> Result<f32> {
    ensure!(angle.is_finite(), "Nonfinite rotator angle");
    // Native repeated subtraction, not remainder: different rounding and
    // endpoint semantics. Detect nonprogress instead of imposing a step cap.
    while angle > limit {
        let next = angle - TAU;
        ensure!(
            next != angle,
            "Rotator angle cannot advance at f32 precision"
        );
        angle = next;
    }
    while angle < -limit {
        let next = angle + TAU;
        ensure!(
            next != angle,
            "Rotator angle cannot advance at f32 precision"
        );
        angle = next;
    }
    Ok(angle)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parameters(speed: f32) -> Parameters {
        Parameters {
            max_speed_degrees: speed,
            acceleration_degrees: 0.,
            min_degrees: 0,
            max_degrees: 0,
            auto_reverse: false,
        }
    }
    #[test]
    fn wheel_and_platform_counter_rotate() -> Result<()> {
        let mut wheel = State::new(15., false)?;
        let mut platform = State::new(-15., false)?;
        for _ in 0..1440 {
            let a = wheel.tick(parameters(15.), 1. / 30., false, None)?;
            let b = platform.tick(parameters(-15.), 1. / 30., false, None)?;
            assert_eq!(a.delta, -b.delta);
            assert_eq!(wheel.angle, -platform.angle);
            assert!(!a.completion_event && !b.completion_event);
            assert!(wheel.angle.abs() <= TAU);
        }
        Ok(())
    }
    #[test]
    fn speed_change_brakes_and_clamps_without_overshoot() -> Result<()> {
        let mut s = State::new(60., false)?;
        let mut p = parameters(0.);
        p.acceleration_degrees = 30.;
        s.tick(p, 1., false, None)?;
        assert_eq!(s.speed, 30. * DEGREES);
        assert_eq!(s.stopping_distance, s.speed * 0.5);
        s.tick(p, 2., false, None)?;
        assert_eq!(s.speed, 0.);
        Ok(())
    }
    #[test]
    fn limits_reverse_and_report_completion() -> Result<()> {
        let mut p = parameters(90.);
        p.max_degrees = 90;
        p.auto_reverse = true;
        let mut s = State::new(90., false)?;
        assert!(s.tick(p, 1., false, None)?.completion_event);
        assert!(s.reversed);
        assert_eq!(s.speed, 0.);
        assert!(s.tick(p, 1., false, None)?.completion_event);
        assert!(!s.reversed);
        assert_eq!(s.angle, 0.);
        Ok(())
    }
    #[test]
    fn target_shortest_path_and_disabled_update() -> Result<()> {
        let mut s = State::new(90., false)?;
        s.angle = 350. * DEGREES;
        let target = 10. * DEGREES;
        let step = s.tick(parameters(90.), 0.1, false, Some(target))?;
        assert!(step.delta > 0. && step.delta < 10. * DEGREES);
        let before = s;
        s.tick(parameters(0.), f32::NAN, true, None)?;
        assert_eq!(before.angle, s.angle);
        assert_eq!(before.speed, s.speed);
        s.tick(parameters(0.), 0., false, None)?;
        assert_eq!(s.speed, 0.);
        assert_eq!(s.angle, before.angle);
        Ok(())
    }
    #[test]
    fn target_delta_matches_executed_retail_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/rotator_target.json"
        ))?;
        let cases = fixture["cases"].as_array().unwrap();
        for case in cases {
            let float = |name: &str| f32::from_bits(case[name].as_u64().unwrap() as u32);
            let mut state = State {
                speed: float("speed_bits"),
                angle: float("angle_bits"),
                stopping_distance: 0.,
                reversed: false,
            };
            let delta = state.requested_delta(float("dt_bits"), Some(float("target_bits")))?;
            assert_eq!(
                delta.to_bits(),
                case["delta_bits"].as_u64().unwrap() as u32,
                "{case}"
            );
            assert_eq!(
                state.reversed,
                case["reversed"].as_bool().unwrap(),
                "{case}"
            );
        }
        assert_eq!(cases.len(), 84);
        Ok(())
    }
}
