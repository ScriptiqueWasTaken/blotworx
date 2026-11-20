//! Recovered spin timing, REL 0x0b78a8 and 0x0b7d94.
//! Not yet the complete ability: ownership gates, damage queries, FX and the
//! animation-to-gameplay dispatcher must be connected before enabling combat.
#[derive(serde::Serialize)]
pub struct Settings {
    pub effect_template: String,
    pub gravity: f32,
    pub stimulus_period: f32,
    pub channels: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, super::ability_value::RegisteredProperty>,
    >,
}
impl Settings {
    pub fn preview_value(&self, name: &str) -> f32 {
        let channel = &self.channels["Paint"];
        let endpoint = |key: &str| {
            super::ability_value::evaluate(
                Some(channel[key].levels[0]),
                0,
                super::ability_value::NEUTRAL_PLAYSTYLE,
                false,
                1.,
                1.,
            )
        };
        super::ability_value::evaluate(
            Some(channel[name].levels[0]),
            0,
            super::ability_value::NEUTRAL_PLAYSTYLE,
            true,
            endpoint("PlaystyleMin"),
            endpoint("PlaystyleMax"),
        )
    }
    /// Current slice has no save/progression owner. Keep its explicit initial
    /// Paint upgrade / neutral playstyle scenario separate from retail state.
    pub fn initial_preview_wait(&self) -> f32 {
        let value = |name: &str| self.channels["Paint"][name].levels[0];
        super::ability_value::evaluate(
            Some(value("Wait")),
            0,
            super::ability_value::NEUTRAL_PLAYSTYLE,
            true,
            super::ability_value::evaluate(
                Some(value("PlaystyleMin")),
                0,
                super::ability_value::NEUTRAL_PLAYSTYLE,
                false,
                1.,
                1.,
            ),
            super::ability_value::evaluate(
                Some(value("PlaystyleMax")),
                0,
                super::ability_value::NEUTRAL_PLAYSTYLE,
                false,
                1.,
                1.,
            ),
        )
    }
    pub fn load(values: &std::collections::BTreeMap<String, String>) -> anyhow::Result<Self> {
        use anyhow::Context;
        let mut lookup = |key: &str| -> anyhow::Result<Option<f32>> {
            values
                .get(key)
                .map(|value| {
                    let number: f32 = value
                        .parse()
                        .with_context(|| format!("Invalid ability setting {key}: {value}"))?;
                    anyhow::ensure!(number.is_finite(), "Non-finite ability setting {key}");
                    Ok(number)
                })
                .transpose()
        };
        let gravity = lookup("MickeyAbilities_Spin_SpinGravity")?.unwrap_or(0.);
        let stimulus_period = lookup("SpinStimRate")?.unwrap_or(0.25);
        let mut channels = std::collections::BTreeMap::new();
        for channel in ["Paint", "Thinner"] {
            let mut properties = std::collections::BTreeMap::new();
            for property in [
                "SpinStim",
                "KnockbackStim",
                "Range",
                "Wait",
                "PlaystyleMin",
                "PlaystyleMax",
            ] {
                properties.insert(
                    property.into(),
                    super::ability_value::RegisteredProperty::load(
                        "Spin",
                        channel,
                        property,
                        &mut lookup,
                    )?,
                );
            }
            channels.insert(channel.into(), properties);
        }
        Ok(Self {
            effect_template: values
                .get("MickeyAbilities_Spin_FXName")
                .cloned()
                .unwrap_or_default(),
            gravity,
            stimulus_period,
            channels,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Lifecycle {
    pub active: bool,
    pub cooldown: f32,
    stimulus_remaining: f32,
}

impl Default for Lifecycle {
    fn default() -> Self {
        // Base ability constructor, REL 0x0aec60..0x0aec6c.
        Self {
            active: false,
            cooldown: -f32::MAX,
            stimulus_remaining: 0.,
        }
    }
}

impl Lifecycle {
    /// Additional ownership/enable gates are supplied by the ability owner.
    /// State exclusions and cooldown comparison: REL 0x0b7ae8..0x0b7b80.
    pub fn start(&mut self, character_state: i32, allowed: bool) -> Option<&'static str> {
        if !allowed
            || self.active
            || matches!(character_state, 8 | 9 | 10)
            || !(self.cooldown <= 0.)
        {
            return None;
        }
        self.active = true;
        self.stimulus_remaining = 0.;
        Some(if character_state == 0 {
            "EVENT_Start_SpinAttack"
        } else {
            "EVENT_Start_SpinAttack_Air"
        })
    }

    /// Stop_SpinAttack (engine event 88) invokes this path, rather than a
    /// hardcoded animation duration. `wait` is the evaluated ability Wait value.
    pub fn animation_stopped(&mut self, wait: f32) {
        self.active = false;
        self.cooldown = wait;
    }

    /// One original update. Returns whether the ability must run its stimulus
    /// query. Preserve overshoot and single-query behavior: no timer clamping,
    /// carry subtraction or catch-up loop is present in REL 0x0b78d0..0x0b7940.
    pub fn update(&mut self, dt: f32, stimulus_period: f32) -> bool {
        if self.cooldown > 0. {
            self.cooldown -= dt;
        }
        if self.active && self.stimulus_remaining >= 0. {
            self.stimulus_remaining -= dt;
            if self.stimulus_remaining <= 0. {
                self.stimulus_remaining = stimulus_period;
                return true;
            }
        }
        false
    }
}

/// The target has already passed the original overlap, owner exclusion,
/// per-attack hit-set and visibility queries (REL b80b0..b825c). Keep that
/// selection separate: distance-to-entity-origin is not the overlap query.
pub fn dispatch_target(
    owner: u64,
    target: u64,
    position: [f32; 3],
    target_position: [f32; 3],
    knockback: f32,
    spin: f32,
    tag: &str,
    deliver: impl FnOnce([super::projectile_stimulus::Emission; 3]) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    use super::projectile_stimulus::{Delivery, Emission};
    anyhow::ensure!(owner != target, "Spin target must exclude its owner");
    anyhow::ensure!(
        position
            .into_iter()
            .chain(target_position)
            .chain([knockback, spin])
            .all(f32::is_finite),
        "Non-finite spin stimulus input"
    );
    let mut direction = std::array::from_fn::<_, 3, _>(|i| target_position[i] - position[i]);
    // REL b8260..b82ac uses the target's position and normalizes only when
    // length exceeds the original 1e-4 constant. REL1fad0 uses paired
    // dot ordering and the Gekko reciprocal-root refinement, not host sqrt.
    let squared = direction[2].mul_add(direction[2], direction[0] * direction[0])
        + direction[1] * direction[1];
    let length = if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    };
    if length > 1.0e-4_f32 {
        let inverse = 1. / length;
        for value in &mut direction {
            *value *= inverse;
        }
    }
    // Original arrays at REL 4d64d8: 3,5,8; amounts at b801c..b8068:
    // KnockbackStim, SpinStim, SpinStim. REL287d44..287da8 adds all three
    // to ONE event before sending it; receivers must not see three events.
    deliver(
        [(3, knockback), (5, spin), (8, spin)].map(|(stimulus, amount)| Emission {
            stimulus,
            amount,
            position,
            direction,
            target: Some(target),
            owner: Some(owner),
            tag: tag.into(),
            delivery: Delivery::Target,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_dispatch_preserves_original_stimulus_order_and_direction() -> anyhow::Result<()> {
        let mut events = Vec::new();
        dispatch_target(1, 2, [1., 2., 3.], [4., 2., 7.], 12., 4., "Spin", |e| {
            events.extend(e);
            Ok(())
        })?;
        assert_eq!(
            events
                .iter()
                .map(|e| (e.stimulus, e.amount))
                .collect::<Vec<_>>(),
            vec![(3, 12.), (5, 4.), (8, 4.)]
        );
        for event in &events {
            assert_eq!(event.direction, [0.6, 0., 0.8]);
            assert_eq!((event.owner, event.target), (Some(1), Some(2)));
        }
        let mut calls = 0;
        assert!(
            dispatch_target(1, 2, [0.; 3], [0.; 3], 1., 2., "Spin", |e| {
                calls += 1;
                assert!(e.iter().all(|v| v.direction == [0.; 3]));
                anyhow::bail!("receiver failed")
            })
            .is_err()
        );
        assert_eq!(calls, 1);
        dispatch_target(1, 2, [0.; 3], [0.00005, 0., 0.], 1., 2., "Spin", |e| {
            assert!(e.iter().all(|v| v.direction == [0.00005, 0., 0.]));
            Ok(())
        })?;
        assert!(dispatch_target(1, 1, [0.; 3], [0.; 3], 1., 2., "Spin", |_| Ok(())).is_err());
        Ok(())
    }
    #[test]
    fn imported_settings_use_qualified_names_and_command_overrides() {
        let mut values=crate::render_support::game_config::runtime_settings(
            "[MickeyAbilities_Spin]\nPaint_Wait_1=0.3\nSpinGravity=-20.0\n[Other]\nPaint_Wait_1=9\n",false);
        values.extend(crate::render_support::game_config::runtime_settings(
            "-Set MickeyAbilities_Spin_Paint_Wait_1=0.6",
            true,
        ));
        let settings = Settings::load(&values).unwrap();
        assert_eq!(settings.gravity, -20.);
        assert_eq!(settings.stimulus_period, 0.25);
        assert_eq!(
            settings.channels["Paint"]["Wait"].levels[0].value,
            Some(0.6)
        );
        assert_eq!(settings.channels["Thinner"]["Wait"].levels[0].value, None);
        values.insert("MickeyAbilities_Spin_Paint_Wait_1".into(), "broken".into());
        assert!(Settings::load(&values).is_err());
    }
    #[test]
    fn cooldown_begins_on_stop_and_preserves_update_overshoot() {
        let mut spin = Lifecycle::default();
        assert_eq!(spin.start(0, true), Some("EVENT_Start_SpinAttack"));
        assert_eq!(spin.start(0, true), None);
        spin.update(1., 0.1);
        assert!(spin.active && spin.cooldown == -f32::MAX);
        spin.animation_stopped(0.3);
        assert_eq!(spin.start(0, true), None);
        spin.update(0.2, 0.1);
        assert_eq!(spin.start(0, true), None);
        spin.update(0.2, 0.1);
        assert!(spin.cooldown < 0.);
        assert_eq!(spin.start(2, true), Some("EVENT_Start_SpinAttack_Air"));
    }
    #[test]
    fn stimulus_has_one_query_per_update_and_negative_period_disables_followups() {
        let mut spin = Lifecycle::default();
        spin.start(0, true);
        assert!(spin.update(0.03, 0.1));
        assert!(!spin.update(0.03, 0.1));
        assert!(spin.update(0.2, 0.1));
        assert!(!spin.update(0.03, 0.1));
        assert!(spin.update(0.2, -1.));
        assert!(!spin.update(2., 0.1));
        spin.animation_stopped(0.);
        assert!(!spin.update(2., 0.1));
    }
    #[test]
    fn rejected_requests_do_not_change_state() {
        for state in [8, 9, 10] {
            let mut spin = Lifecycle::default();
            assert_eq!(spin.start(state, true), None);
            assert!(!spin.active);
        }
        let mut spin = Lifecycle::default();
        assert_eq!(spin.start(0, false), None);
        spin.cooldown = f32::NAN;
        assert_eq!(spin.start(0, true), None);
    }
}
