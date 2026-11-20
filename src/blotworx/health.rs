//! Native health acceptance only. Damage/death/effects and setters are separate.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;

/// Resolved authored fields. Native configuration, save restoration and health
/// setter side effects are not implied by importing this state.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Authored {
    pub health: i32,
    pub maximum: i32,
    pub infinite: bool,
}
impl Authored {
    pub fn from_component(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "HealthComponent",
            "Not a HealthComponent"
        );
        let field = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
                .with_context(|| {
                    format!("Health property {name} missing; native default unverified")
                })
        };
        let integer = |name: &str| -> Result<i32> {
            let value = field(name)?
                .as_i64()
                .with_context(|| format!("Health property {name} must be an integer"))?;
            i32::try_from(value)
                .with_context(|| format!("Health property {name} is outside signed 32-bit range"))
        };
        Ok(Self {
            health: integer("Health")?,
            maximum: integer("MaxHealth")?,
            infinite: field("InfiniteHealth")?
                .as_bool()
                .context("InfiniteHealth must be boolean")?,
        })
    }
    pub fn accepts_pickup(&self, amount: i32, inactive: u32) -> bool {
        alive(self.health, u8::from(self.infinite))
            && preview_heal(self.health, self.maximum, amount, inactive).is_some()
    }
}

/// REL324c48: InfiniteHealth byte +3c overrides the signed current-health check
/// (registration321d58, getter322088 ->320c20).
pub fn alive(current: i32, override_byte: u8) -> bool {
    override_byte != 0 || current > 0
}

/// HealthComponent's stimulus mask, initialized at REL3240c4..3240fc.
/// Knockback, magic and status effects do not directly subtract health.
pub const DAMAGES_HEALTH: [bool; 15] = [
    true, true, true, false, true, true, true, false, true, false, false, false, false, false, true,
];

/// REL324d6c looks up the FIRST entry of each type, even for repeated types,
/// and rounds each positive effective amount up before summing in a GPR.
pub fn stimulus_damage(stimuli: &[(u32, f32)], contact_suppressed: bool) -> Result<i32> {
    let mut total = 0i32;
    for &(kind, _) in stimuli {
        let admitted = *DAMAGES_HEALTH
            .get(kind as usize)
            .context("Unknown health stimulus type")?;
        if !admitted || (kind == 4 && contact_suppressed) {
            continue;
        }
        let amount = stimuli.iter().find(|s| s.0 == kind).unwrap().1;
        if amount > 0. {
            let rounded = amount.ceil();
            ensure!(
                rounded.is_finite() && rounded < 2147483648.,
                "Health damage exceeds verified Wii integer conversion"
            );
            total = total.wrapping_add(rounded as i32);
        }
    }
    Ok(total)
}

#[derive(Clone, Debug)]
pub struct Runtime {
    pub health: i32,
    pub invulnerable_until: f32,
    death_processed: bool,
}
impl Runtime {
    ///324ca8 reads the inverse of the processed-death bit, not current HP.
    pub(super) fn knockback_eligible(&self) -> bool {
        !self.death_processed
    }
    pub fn new(authored: &Authored) -> Self {
        Self {
            health: authored.health,
            invulnerable_until: 0.,
            death_processed: false,
        }
    }
    /// REL320c28: unchanged or above-MaxHealth writes are declined, rather
    /// than clamped. Negative values are admitted. Death is handled later by
    /// the ordinary component update, not by this property setter.
    pub fn set_health(&mut self, value: i32, maximum: i32) -> bool {
        if self.health == value || value > maximum { return false; }
        self.health = value;
        true
    }
    /// REL323be8..323c58 latches zero-health processing;323ffc clears the
    /// latch once health becomes positive again. This runs on component update.
    pub fn take_death(&mut self) -> bool {
        if self.health > 0 {
            self.death_processed = false;
            return false;
        }
        if self.death_processed {
            return false;
        }
        self.death_processed = true;
        true
    }
    /// Arithmetic from REL32477c..3247e8. Returns accepted damage, including
    /// infinite-health reactions; death actions and effect callbacks are separate.
    pub fn damage(
        &mut self,
        amount: i32,
        infinite: bool,
        force: bool,
        now: f32,
        duration: f32,
    ) -> Result<bool> {
        ensure!(
            now.is_finite() && duration.is_finite(),
            "Invalid health timer"
        );
        if amount <= 0 || force || now < self.invulnerable_until {
            return Ok(false);
        }
        let next = if infinite {
            self.health
        } else {
            self.health.wrapping_sub(amount).max(0)
        };
        if next >= self.health && !infinite {
            return Ok(false);
        }
        self.health = next;
        self.invulnerable_until = now + duration;
        Ok(true)
    }
}

/// REL324534 /324598 with preview flag=1. The original adds in a 32-bit
/// register, then clamps to MaxHealth with signed comparisons; it does not use
/// saturating addition or write health during this query.
pub fn preview_heal(current: i32, maximum: i32, amount: i32, inactive: u32) -> Option<i32> {
    if inactive != 0 || amount <= 0 {
        return None;
    }
    let next = current.wrapping_add(amount).min(maximum);
    (next > current).then_some(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn death_processing_is_latched_until_health_recovers() {
        let mut state = Runtime::new(&Authored {
            health: 1,
            maximum: 1,
            infinite: false,
        });
        assert!(!state.take_death());
        state.health = 0;
        assert!(state.take_death());
        assert!(!state.take_death());
        state.health = 1;
        assert!(!state.take_death());
        state.health = -1;
        assert!(state.take_death());
    }
    #[test]
    fn grouped_stimulus_rounding_excludes_knockback_and_preserves_first_match() -> Result<()> {
        assert_eq!(stimulus_damage(&[(3, 100.), (5, 0.2), (8, 0.2)], false)?, 2);
        assert_eq!(stimulus_damage(&[(8, 0.2), (8, 99.)], false)?, 2);
        assert_eq!(stimulus_damage(&[(4, 1.), (1, -4.), (0, 0.)], true)?, 0);
        assert!(stimulus_damage(&[(15, 1.)], false).is_err());
        let authored = Authored {
            health: 5,
            maximum: 5,
            infinite: false,
        };
        let mut runtime = Runtime::new(&authored);
        assert!(runtime.damage(2, false, false, 0., 1.)?);
        assert_eq!(runtime.health, 3);
        assert!(!runtime.damage(2, false, false, 0.5, 1.)?);
        assert!(runtime.damage(9, false, false, 1., 1.)?);
        assert_eq!(runtime.health, 0);
        assert_eq!(authored.health, 5);
        Ok(())
    }
    #[test]
    fn preview_matches_signed_clamp_and_native_overflow() {
        assert_eq!(preview_heal(3, 5, 1, 0), Some(4));
        assert_eq!(preview_heal(3, 5, 10, 0), Some(5));
        assert_eq!(preview_heal(5, 5, 1, 0), None);
        assert_eq!(preview_heal(5, 3, 1, 0), None);
        assert_eq!(preview_heal(i32::MAX - 1, i32::MAX, 3, 0), None);
        assert_eq!(preview_heal(1, 5, 1, 0x100), None);
        assert_eq!(preview_heal(1, 5, 0, 0), None);
        assert_eq!(preview_heal(1, 5, -1, 0), None);
        // This operation itself does not enforce living health; its pickup
        // caller performs the separate alive check before requesting preview.
        assert_eq!(preview_heal(0, 5, 1, 0), Some(1));
    }
    #[test]
    fn alive_override_is_independent_of_health_capacity() {
        assert!(!alive(0, 0));
        assert!(!alive(-1, 0));
        assert!(alive(1, 0));
        assert!(alive(0, 1));
        assert!(alive(-5, 128));
        assert_eq!(preview_heal(5, 5, 1, 0), None);
    }
}
