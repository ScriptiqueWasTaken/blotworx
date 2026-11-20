//! PaintThinnerCounterComponent scalar state. REL3635f8 snapshots the initial
//! value; REL364e3c evaluates thresholds and PreserveStatus against that anchor.
//! Stimulus amounts, regeneration, material callbacks and event publication
//! remain separate required stages; importing this state does not run them.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
/// Impact rim colors selected by3643a8 with force/effect argument zero.
/// These differ from RimTEffect/RimPEffect, used by the other branch.
#[derive(Clone, Debug, serde::Serialize)]
pub struct RimColors {
    pub thinner: [i32; 3],
    pub paint: [i32; 3],
}
impl Default for RimColors {
    fn default() -> Self {
        Self {
            thinner: [255, 5, 5],
            paint: [5, 5, 255],
        }
    }
}
impl RimColors {
    pub fn load(values: &std::collections::BTreeMap<String, String>) -> Result<Self> {
        Self::load_prefixes(values, "RimTIEffect", "RimPIEffect")
    }
    /// Force/effect branch of3643a8, used by counter-hit364178. These are
    /// independent settings from the continuous toon-fraction rim colors.
    pub fn load_impact(values: &std::collections::BTreeMap<String, String>) -> Result<Self> {
        Self::load_prefixes(values, "RimTEffect", "RimPEffect")
    }
    fn load_prefixes(
        values: &std::collections::BTreeMap<String, String>,
        thinner: &str,
        paint: &str,
    ) -> Result<Self> {
        let read = |name: &str, default: i32| -> Result<i32> {
            values
                .get(name)
                .map(|value| {
                    value
                        .parse()
                        .with_context(|| format!("Invalid counter rim setting {name}: {value}"))
                })
                .transpose()
                .map(|value| value.unwrap_or(default))
        };
        Ok(Self {
            thinner: [
                read(&format!("{thinner}R"), 255)?,
                read(&format!("{thinner}G"), 5)?,
                read(&format!("{thinner}B"), 5)?,
            ],
            paint: [
                read(&format!("{paint}R"), 5)?,
                read(&format!("{paint}G"), 5)?,
                read(&format!("{paint}B"), 255)?,
            ],
        })
    }
    fn rgb(&self, thinner: bool) -> [f32; 3] {
        (if thinner { self.thinner } else { self.paint }).map(|value| value as f32 / 255.)
    }
    pub fn impact_rgba(&self, amount: f32) -> Result<[f32; 4]> {
        ensure!(amount.is_finite(), "Nonfinite counter impact color amount");
        let [r, g, b] = self.rgb(amount < 0.);
        Ok([r, g, b, 1.])
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Counter {
    pub value: f32,
    pub anchor: f32,
    pub paint_threshold: f32,
    pub thinner_threshold: f32,
    pub preserve_status: bool,
    pub published_status: u32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Change {
    pub value: f32,
    pub scaled_amount: f32,
    pub displacement: f32,
    pub previous_status: u32,
    pub next_status: u32,
}
/// Value/material stage's retained filter (+9c), REL36523c. Model subtree
/// publication and effect callbacks are required separately by the owner.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct MaterialState {
    pub filtered: f32,
}
/// Constructor361df8..361e08: retained filter0, block byteA0=0.
#[derive(Debug, serde::Serialize)]
pub struct MaterialRuntime {
    pub filter: MaterialState,
    pub blocked: bool,
    pub initialized: bool,
    pub publications: u64,
    pub error: Option<String>,
    pub invulnerable_remaining: f32,
    pub status_not_thinned: bool,
    pub transient: super::render_color_effect::Effect,
    pub transient_initialized: bool,
}
impl Default for MaterialRuntime {
    fn default() -> Self {
        Self {
            filter: Default::default(),
            blocked: false,
            initialized: false,
            publications: 0,
            error: None,
            invulnerable_remaining: 0.,
            status_not_thinned: true,
            transient: Default::default(),
            transient_initialized: false,
        }
    }
}
/// Native16cc00/16ccf8 multiply255, fctiwz, then store the low byte.
pub(super) fn pack_color(color: [f32; 4]) -> Result<[u8; 4]> {
    let mut packed = [0; 4];
    for (byte, value) in packed.iter_mut().zip(color) {
        let value = value * 255.;
        ensure!(
            value.is_finite()
                && (value.trunc() as f64) >= i32::MIN as f64
                && (value.trunc() as f64) <= i32::MAX as f64,
            "Color requires native fctiwz range validation"
        );
        *byte = value.trunc() as i32 as u8;
    }
    Ok(packed)
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialUpdate {
    pub filtered: f32,
    pub constant_color: u8,
    pub rgba: [u8; 4],
}
impl MaterialUpdate {
    /// Material-only publication for an admitted drawable. The owner still
    /// supplies native subtree traversal and ignore-override admission.
    pub fn apply_to_draw(
        &self,
        draw: &mut crate::render_support::viewport3d::GpuDrawCommand,
    ) -> Result<()> {
        ensure!(
            self.constant_color < 4,
            "Invalid object constant-color slot"
        );
        let program = draw
            .preview
            .as_ref()
            .context("Counter drawable has no material program")?
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?;
        let defaults = program
            .register_defaults
            .context("Counter drawable requires a uniform material program")?;
        let registers = draw.material_registers.get_or_insert(defaults);
        registers[3 + usize::from(self.constant_color)] = self.rgba.map(|v| f32::from(v) / 255.);
        Ok(())
    }
}
impl MaterialState {
    /// Effect enum0 enables this path. A0 blocks it; force bypasses only the
    /// equal-value early return, not either enable flag. RGB comes from the
    /// configured impact rim selected by the filtered sign (zero is thinner).
    /// Alpha=1-abs(filtered), packed by16ccf8's multiply/fctiwz/stb.
    /// The interior multiplier is EffectThreshold (native default1), distinct
    /// from ToonPercentThreshold used by other counter decisions.
    pub fn update(
        &mut self,
        value: f32,
        threshold: f32,
        effect_enabled: bool,
        blocked: bool,
        force: bool,
    ) -> Result<Option<MaterialUpdate>> {
        self.update_with_colors(
            value,
            threshold,
            effect_enabled,
            blocked,
            force,
            &RimColors::default(),
        )
    }
    pub fn update_with_colors(
        &mut self,
        value: f32,
        threshold: f32,
        effect_enabled: bool,
        blocked: bool,
        force: bool,
        colors: &RimColors,
    ) -> Result<Option<MaterialUpdate>> {
        ensure!(
            value.is_finite() && threshold.is_finite(),
            "Nonfinite counter material parameter"
        );
        if !effect_enabled || blocked || (!force && self.filtered == value) {
            return Ok(None);
        }
        let target = if value > -1. && value < 1. {
            value * threshold
        } else {
            value
        };
        ensure!(target.is_finite(), "Counter material scaling overflow");
        let factor = f32::from_bits(0x3da3d70a); // Native constant4fa04c: .08f.
        let old_term = (1. - factor) * self.filtered;
        let mut filtered = factor.mul_add(target, old_term);
        if (filtered - target).abs() < f32::from_bits(0x3a83126f) {
            filtered = target;
        }
        let alpha = 1. - filtered.abs();
        let packed = alpha * 255.;
        // Do not replace unvalidated out-of-range PPC conversions with a
        // saturated host color; normal authored counters remain in this range.
        ensure!(
            packed.is_finite()
                && packed.trunc() >= i32::MIN as f32
                && (packed.trunc() as f64) <= i32::MAX as f64,
            "Counter color requires native fctiwz range validation"
        );
        let mut rgba = [0; 4];
        for (out, value) in rgba[..3].iter_mut().zip(colors.rgb(filtered <= 0.)) {
            let value = value * 255.;
            ensure!(
                value.is_finite()
                    && (value.trunc() as f64) >= i32::MIN as f64
                    && (value.trunc() as f64) <= i32::MAX as f64,
                "Counter RGB requires native fctiwz range validation"
            );
            *out = value.trunc() as i32 as u8;
        }
        rgba[3] = packed.trunc() as i32 as u8;
        self.filtered = filtered;
        Ok(Some(MaterialUpdate {
            filtered,
            constant_color: 1,
            rgba,
        }))
    }
}
/// Required downstream stages of a matched, nonzero paint/thinner stimulus.
/// Implementations must execute each stage or fail; there are no default
/// material, event, zero-health, invulnerability or feedback no-ops.
pub trait HitHost {
    fn write_value_and_material(&mut self, counter: &mut Counter, value: f32) -> Result<()>;
    fn broadcast_status(
        &mut self,
        counter: &mut Counter,
        previous: u32,
        current: u32,
    ) -> Result<()>;
    fn send_zero_health(&mut self, counter: &mut Counter) -> Result<()>;
    fn refresh_invulnerability(&mut self, counter: &mut Counter) -> Result<()>;
    fn impact_and_damage_feedback(&mut self, counter: &mut Counter, change: Change) -> Result<()>;
}
impl Counter {
    /// REL364fdc: stage the value adjustment only when published state changes.
    /// Event publication and the cached status write follow this setter stage.
    pub fn publication_value(&self,next:u32)->Option<f32> {
        if self.published_status==next {return None;}
        let value=if self.preserve_status {
            match next {
                1 if self.value<self.anchor=>self.anchor,
                2 if self.value>self.anchor=>self.anchor,
                0 if self.value>=self.paint_threshold=>self.paint_threshold-f32::from_bits(0x3a83126f),
                0 if self.value<=self.thinner_threshold=>self.thinner_threshold+f32::from_bits(0x3a83126f),
                _=>self.value,
            }
        } else {
            match next {1=>self.paint_threshold,2=>self.thinner_threshold,0=>self.anchor,_=>self.value}
        };
        Some(value)
    }

    /// REL36397c clamps to authored thresholds, then normalizes the whole
    /// thinner-to-paint span. This differs from the signed material fraction.
    pub fn painted_percentage(&self)->f32 {
        let clamped=if self.value<self.thinner_threshold {self.thinner_threshold}
            else if self.value>self.paint_threshold {self.paint_threshold} else {self.value};
        (-self.thinner_threshold+clamped)/(self.paint_threshold-self.thinner_threshold)
    }

    /// REL363888: signed toon fraction, independently normalized on each side
    /// of zero. A zero threshold returns zero; negative values clamp at -1.
    pub fn toon_fraction(&self) -> f32 {
        if self.value > 0. {
            if self.paint_threshold == 0. {
                return 0.;
            }
            let ratio = self.value / self.paint_threshold;
            if ratio > 1. {
                1.
            } else {
                ratio
            }
        } else if self.value < 0. {
            if self.thinner_threshold == 0. {
                return 0.;
            }
            let ratio = -(self.value / self.thinner_threshold);
            if ratio < -1. {
                -1.
            } else {
                ratio
            }
        } else {
            0.
        }
    }
    /// Ordered nonzero path, REL363fac..364240. Values/thresholds are captured
    /// before setter callbacks, but PreserveStatus's anchor and published state
    /// are reread after them. Event enqueue happens before the cached-state write.
    pub fn apply_nonzero(
        &mut self,
        change: Change,
        send_zero_hp_event: bool,
        host: &mut impl HitHost,
    ) -> Result<()> {
        ensure!(
            change.scaled_amount != 0.,
            "Zero counter impacts require their separate native response path"
        );
        let paint = self.paint_threshold;
        let thinner = self.thinner_threshold;
        let preserve = self.preserve_status;
        host.write_value_and_material(self, change.value)?;
        let sampled = Counter {
            value: change.value,
            anchor: self.anchor,
            paint_threshold: paint,
            thinner_threshold: thinner,
            preserve_status: preserve,
            published_status: self.published_status,
        };
        let current = sampled.status();
        let previous = self.published_status;
        if previous != current {
            host.broadcast_status(self, previous, current)?;
            // Native1f4438 queues the event; this hook is its publication stage,
            // not synchronous subscriber delivery. Cached state changes next.
            self.published_status = current;
            if current == 2 && send_zero_hp_event {
                host.send_zero_health(self)?;
            }
        }
        host.refresh_invulnerability(self)?;
        host.impact_and_damage_feedback(
            self,
            Change {
                previous_status: previous,
                next_status: current,
                ..change
            },
        )
    }
    /// Arithmetic stage of REL363a48. Caller applies vulnerability and invokes
    /// the native setter/material/state/damage callbacks in their real order.
    /// Returning a proposal does not commit state or pretend callbacks ran.
    pub fn prepare_stimulus(
        &self,
        stimuli: &[(u32, f32)],
        scale: f32,
        self_hit: bool,
    ) -> Result<Option<Change>> {
        if self_hit {
            return Ok(None);
        }
        ensure!(
            scale.is_finite() && stimuli.iter().all(|(_, v)| v.is_finite()),
            "Nonfinite counter stimulus"
        );
        // The receiver searches for type1 first, independent of input order;
        // repeated entries select the first matching amount, not a sum.
        let amount = if let Some((_, amount)) = stimuli.iter().find(|(kind, _)| *kind == 1) {
            *amount
        } else if let Some((_, amount)) = stimuli.iter().find(|(kind, _)| *kind == 0) {
            -*amount
        } else {
            return Ok(None);
        };
        let mut value = self.value;
        if !self.preserve_status {
            if value > self.anchor && amount < 0. {
                value = self.anchor;
            } else if value < self.anchor && amount > 0. {
                value = self.anchor;
            }
        }
        let scaled_amount = amount * scale;
        ensure!(
            scaled_amount.is_finite(),
            "Counter stimulus scaling overflow"
        );
        // A zero amount has separate native impact/audio callbacks. Do not
        // turn it into a successful no-op here or skip its publication path.
        if scaled_amount == 0. {
            return Ok(Some(Change {
                value: self.value,
                scaled_amount,
                displacement: 0.,
                previous_status: self.published_status,
                next_status: self.published_status,
            }));
        }
        let old = value;
        value += scaled_amount;
        if value > self.paint_threshold {
            value = self.paint_threshold;
        } else if value < self.thinner_threshold {
            value = self.thinner_threshold;
        }
        ensure!(value.is_finite(), "Counter stimulus value overflow");
        let mut proposed = self.clone();
        proposed.value = value;
        Ok(Some(Change {
            value,
            scaled_amount,
            displacement: old - value,
            previous_status: self.published_status,
            next_status: proposed.status(),
        }))
    }
    pub fn from_component(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "PaintThinnerCounterComponent",
            "Not a paint/thinner counter"
        );
        let field = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
                .with_context(|| format!("Paint/thinner counter missing {name}"))
        };
        let number = |name: &str| -> Result<f32> {
            let value = field(name)?
                .as_f64()
                .with_context(|| format!("Invalid counter {name}"))? as f32;
            ensure!(value.is_finite(), "Nonfinite counter {name}");
            Ok(value)
        };
        let value = number("CounterStart")?;
        Ok(Self {
            value,
            anchor: value,
            paint_threshold: number("PaintThreshold")?,
            thinner_threshold: number("ThinnerThreshold")?,
            preserve_status: field("PreserveStatus")?
                .as_bool()
                .context("Invalid PreserveStatus")?,
            published_status: 0,
        })
    }
    /// Raw native results: 0 intermediate, 1 paint threshold, 2 thinner
    /// threshold. PreserveStatus uses the initialization anchor, not a midpoint
    /// or the opposite threshold. Paint comparison has precedence.
    pub fn status(&self) -> u32 {
        if self.value >= self.paint_threshold {
            return 1;
        }
        if self.value <= self.thinner_threshold {
            return 2;
        }
        if !self.preserve_status {
            return 0;
        }
        match self.published_status {
            1 => u32::from(self.value > self.anchor),
            2 => {
                if self.value < self.anchor {
                    2
                } else {
                    0
                }
            }
            other => other,
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn impact_settings_are_independent_and_select_by_signed_amount() -> anyhow::Result<()> {
        let values = std::collections::BTreeMap::from([
            ("RimTEffectR".into(), "77".into()),
            ("RimPEffectB".into(), "201".into()),
            ("RimTIEffectR".into(), "33".into()),
            ("RimPIEffectB".into(), "42".into()),
        ]);
        let impact = super::RimColors::load_impact(&values)?;
        let continuous = super::RimColors::load(&values)?;
        assert_eq!(impact.thinner, [77, 5, 5]);
        assert_eq!(continuous.thinner, [33, 5, 5]);
        assert_eq!(impact.paint, [5, 5, 201]);
        assert_eq!(continuous.paint, [5, 5, 42]);
        assert_eq!(
            impact.impact_rgba(-1.)?,
            [77. / 255., 5. / 255., 5. / 255., 1.]
        );
        for amount in [1., 0., -0.] {
            assert_eq!(
                impact.impact_rgba(amount)?,
                [5. / 255., 5. / 255., 201. / 255., 1.]
            );
        }
        assert!(impact.impact_rgba(f32::NAN).is_err());
        assert!(
            super::RimColors::load_impact(&std::collections::BTreeMap::from([(
                "RimTEffectR".into(),
                "invalid".into()
            )]))
            .is_err()
        );
        Ok(())
    }
    use super::*;
    #[test]
    fn material_rgb_uses_filtered_sign_and_native_impact_settings() -> Result<()> {
        let values = std::collections::BTreeMap::from([
            ("RimTIEffectR".into(), "17".into()),
            ("RimPIEffectG".into(), "81".into()),
        ]);
        let colors = RimColors::load(&values)?;
        assert_eq!(colors.thinner, [17, 5, 5]);
        assert_eq!(colors.paint, [5, 81, 255]);
        let mut state = MaterialState { filtered: -0.8 };
        // A paint hit can still have a thinner-colored rim until smoothing
        // crosses zero. Selecting from the input sign would be incorrect.
        let first = state
            .update_with_colors(1., 0.4, true, false, false, &colors)?
            .unwrap();
        assert!(first.filtered < 0.);
        assert_eq!(&first.rgba[..3], &[17, 5, 5]);
        for _ in 0..30 {
            state.update_with_colors(1., 0.4, true, false, false, &colors)?;
        }
        let last = state
            .update_with_colors(1., 0.4, true, false, true, &colors)?
            .unwrap();
        assert!(last.filtered > 0.);
        assert_eq!(&last.rgba[..3], &[5, 81, 255]);
        let mut zero = MaterialState::default();
        assert_eq!(
            &zero
                .update_with_colors(0., 0.4, true, false, true, &colors)?
                .unwrap()
                .rgba[..3],
            &[17, 5, 5]
        );
        assert!(RimColors::load(&std::collections::BTreeMap::from([(
            "RimTIEffectR".into(),
            "invalid".into()
        )]))
        .is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original REL in BLOTWORX_TEST_DATA"]
    fn original_impact_rim_declarations_are_not_the_effect_rim_settings() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(std::path::Path::new(&root).join("MickeyF213923.213924.rel"))?;
        for (offset, name) in [
            (0x181c, "RimTIEffectR"),
            (0x1829, "RimTIEffectG"),
            (0x1836, "RimTIEffectB"),
            (0x1843, "RimPIEffectR"),
            (0x1850, "RimPIEffectG"),
            (0x185d, "RimPIEffectB"),
        ] {
            let at = 0x4cda08 + offset;
            assert_eq!(&bytes[at..at + name.len()], name.as_bytes());
            assert_eq!(bytes[at + name.len()], 0);
        }
        assert_eq!(&bytes[0x4cda08 + 0x17da..0x4cda08 + 0x17da + 4], b"255\0");
        assert_eq!(&bytes[0x4cda08 + 0x17ea..0x4cda08 + 0x17ea + 2], b"5\0");
        Ok(())
    }
    #[test]
    fn native_material_filter_scales_interior_values_and_packs_subtree_color() -> Result<()> {
        let mut state = MaterialState::default();
        let update = state.update(1., 0.4, true, false, false)?.unwrap();
        assert_eq!(update.filtered.to_bits(), 0x3da3d70a);
        assert_eq!(update.constant_color, 1);
        assert_eq!(update.rgba, [5, 5, 255, 234]);
        let mut interior = MaterialState::default();
        let update = interior.update(0.5, 0.4, true, false, false)?.unwrap();
        assert_eq!(
            update.filtered,
            (0.5_f32 * 0.4) * f32::from_bits(0x3da3d70a)
        );
        assert_eq!(update.rgba, [5, 5, 255, 250]);
        let before = interior.filtered;
        assert!(interior.update(1., 0.4, false, false, true)?.is_none());
        assert!(interior.update(1., 0.4, true, true, true)?.is_none());
        assert_eq!(interior.filtered, before);
        // The original equal-value test precedes threshold scaling.
        interior.filtered = 0.5;
        assert!(interior.update(0.5, 0.4, true, false, false)?.is_none());
        assert!(interior.update(0.5, 0.4, true, false, true)?.is_some());
        let mut near = MaterialState { filtered: 0.9995 };
        assert_eq!(
            near.update(1., 0.4, true, false, false)?.unwrap().filtered,
            1.
        );
        Ok(())
    }
    #[test]
    fn native_toon_fraction_uses_independent_signed_thresholds() {
        let mut counter = Counter {
            value: 0.,
            anchor: 0.,
            paint_threshold: 5.,
            thinner_threshold: -10.,
            preserve_status: false,
            published_status: 0,
        };
        for (value, fraction) in [
            (-20., -1.),
            (-10., -1.),
            (-5., -0.5),
            (0., 0.),
            (2.5, 0.5),
            (5., 1.),
            (10., 1.),
        ] {
            counter.value = value;
            assert_eq!(counter.toon_fraction(), fraction);
        }
        counter.paint_threshold = 0.;
        counter.value = 5.;
        assert_eq!(counter.toon_fraction(), 0.);
        counter.thinner_threshold = 0.;
        counter.value = -5.;
        assert_eq!(counter.toon_fraction(), 0.);
    }
    #[test]
    fn hit_callbacks_keep_native_order_cache_timing_and_do_not_silence_missing_stages() -> Result<()>
    {
        struct Host {
            calls: Vec<&'static str>,
            fail: bool,
        }
        impl HitHost for Host {
            fn write_value_and_material(
                &mut self,
                counter: &mut Counter,
                value: f32,
            ) -> Result<()> {
                self.calls.push("value/material");
                if self.fail {
                    anyhow::bail!("Required material callback unavailable");
                }
                counter.value = value;
                Ok(())
            }
            fn broadcast_status(
                &mut self,
                counter: &mut Counter,
                previous: u32,
                current: u32,
            ) -> Result<()> {
                self.calls.push("state event");
                assert_eq!(counter.published_status, previous);
                assert_eq!(current, 2);
                counter.published_status = 99;
                Ok(())
            }
            fn send_zero_health(&mut self, counter: &mut Counter) -> Result<()> {
                self.calls.push("zero health");
                assert_eq!(counter.published_status, 2);
                Ok(())
            }
            fn refresh_invulnerability(&mut self, _: &mut Counter) -> Result<()> {
                self.calls.push("invulnerability");
                Ok(())
            }
            fn impact_and_damage_feedback(
                &mut self,
                _: &mut Counter,
                change: Change,
            ) -> Result<()> {
                self.calls.push("impact/damage");
                assert_eq!(change.next_status, 2);
                Ok(())
            }
        }
        let mut counter = Counter {
            value: 0.,
            anchor: 0.,
            paint_threshold: 5.,
            thinner_threshold: -5.,
            preserve_status: false,
            published_status: 0,
        };
        let change = counter.prepare_stimulus(&[(0, 5.)], 1., false)?.unwrap();
        let mut host = Host {
            calls: vec![],
            fail: true,
        };
        assert!(counter.apply_nonzero(change, true, &mut host).is_err());
        assert_eq!(counter.value, 0.);
        assert_eq!(host.calls, ["value/material"]);
        host.calls.clear();
        host.fail = false;
        counter.apply_nonzero(change, true, &mut host)?;
        assert_eq!(counter.value, -5.);
        assert_eq!(counter.published_status, 2);
        assert_eq!(
            host.calls,
            [
                "value/material",
                "state event",
                "zero health",
                "invulnerability",
                "impact/damage"
            ]
        );
        Ok(())
    }
    #[test]
    fn native_stimulus_precedence_reset_clamping_and_displacement_are_preserved() -> Result<()> {
        let mut counter = Counter {
            value: 3.,
            anchor: 1.,
            paint_threshold: 5.,
            thinner_threshold: -5.,
            preserve_status: false,
            published_status: 0,
        };
        let change = counter.prepare_stimulus(&[(0, 2.)], 1., false)?.unwrap();
        assert_eq!(
            (change.value, change.scaled_amount, change.displacement),
            (-1., -2., 2.)
        );
        assert_eq!(
            counter.value, 3.,
            "Preparing a proposal must not bypass callbacks"
        );
        counter.preserve_status = true;
        assert_eq!(
            counter
                .prepare_stimulus(&[(0, 2.)], 1., false)?
                .unwrap()
                .value,
            1.
        );
        // Type1 wins even when type0 is earlier and duplicate type1 amounts
        // differ. Clamped displacement is not the requested signed amount.
        let change = counter
            .prepare_stimulus(&[(0, 100.), (1, 8.), (1, 99.)], 1., false)?
            .unwrap();
        assert_eq!(
            (
                change.value,
                change.scaled_amount,
                change.displacement,
                change.next_status
            ),
            (5., 8., -2., 1)
        );
        let change = counter.prepare_stimulus(&[(0, 100.)], 1., false)?.unwrap();
        assert_eq!((change.value, change.next_status), (-5., 2));
        counter.preserve_status = false;
        counter.value = -3.;
        let change = counter.prepare_stimulus(&[(1, 2.)], 0.5, false)?.unwrap();
        assert_eq!((change.value, change.scaled_amount), (2., 1.));
        let zero = counter.prepare_stimulus(&[(1, 2.)], 0., false)?.unwrap();
        assert_eq!(zero.value, -3.);
        assert_eq!(zero.scaled_amount, 0.);
        assert!(counter
            .prepare_stimulus(&[(1, f32::NAN)], 1., true)?
            .is_none());
        assert!(counter
            .prepare_stimulus(&[(1, f32::NAN)], 1., false)
            .is_err());
        Ok(())
    }
    #[test]
    fn native_threshold_order_and_preservation_use_the_original_anchor() {
        let mut counter = Counter {
            value: 0.,
            anchor: 1.,
            paint_threshold: 5.,
            thinner_threshold: -5.,
            preserve_status: false,
            published_status: 0,
        };
        for (value, status) in [(-5., 2), (-4.999, 0), (0., 0), (4.999, 0), (5., 1)] {
            counter.value = value;
            assert_eq!(counter.status(), status);
        }
        counter.preserve_status = true;
        counter.published_status = 1;
        for (value, status) in [(1.001, 1), (1., 0), (0.999, 0), (-5., 2)] {
            counter.value = value;
            assert_eq!(counter.status(), status);
        }
        counter.published_status = 2;
        for (value, status) in [(0.999, 2), (1., 0), (1.001, 0), (5., 1)] {
            counter.value = value;
            assert_eq!(counter.status(), status);
        }
        counter.preserve_status = false;
        counter.paint_threshold = -5.;
        counter.thinner_threshold = 5.;
        counter.value = 0.;
        assert_eq!(
            counter.status(),
            1,
            "Paint threshold wins when intervals overlap"
        );
    }
}
