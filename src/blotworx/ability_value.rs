//! Numeric ability-property evaluation recovered from EM1 REL 0x0af550 and
//! 0x0b0b24. Config key registration, upgrade selection and live playstyle
//! ownership are separate; callers must supply the selected original values.

/// REL 0x16da0 returns 50 for a missing playstyle stat. The comparisons at
/// 0x15bc8/0x15be0 also use 50 as neutral. This is not a restored save profile.
pub const NEUTRAL_PLAYSTYLE: i32 = 50;

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Property {
    /// Registered property with no config entry is distinct from unregistered.
    pub value: Option<f32>,
    pub playstyle_min: Option<f32>,
    pub playstyle_max: Option<f32>,
}

/// The registered upgrade slots, including the first absent base-value slot.
/// Values are snapshots; a live config owner must reload on config changes.
#[derive(serde::Serialize)]
pub struct RegisteredProperty {
    pub levels: Vec<Property>,
}

impl RegisteredProperty {
    /// REL 0x0af758..0x0aff78: independent optional endpoint lookups, followed
    /// by the base value. A missing base terminates registration, even if a
    /// later numbered key exists. The original loop limit is exactly 256.
    pub fn load<E>(
        ability: &str,
        channel: &str,
        property: &str,
        mut lookup: impl FnMut(&str) -> Result<Option<f32>, E>,
    ) -> Result<Self, E> {
        let prefix = format!("MickeyAbilities_{ability}_{channel}_{property}");
        let mut levels = Vec::new();
        for level in 1..=256 {
            let playstyle_min = lookup(&format!("{prefix}_PlaystyleMin_{level}"))?;
            let playstyle_max = lookup(&format!("{prefix}_PlaystyleMax_{level}"))?;
            let value = lookup(&format!("{prefix}_{level}"))?;
            levels.push(Property {
                value,
                playstyle_min,
                playstyle_max,
            });
            if value.is_none() {
                break;
            }
        }
        Ok(Self { levels })
    }
}

/// Original +0x14 mode selects which side of playstyle uses each multiplier.
/// The signed threshold is 50 (REL 0x015bc8 / 0x015be0), not a lerp factor.
pub fn multiplier(mode: i32, playstyle: i32, minimum: f32, maximum: f32) -> f32 {
    match mode {
        0 if playstyle < 50 => minimum,
        0 if playstyle > 50 => maximum,
        1 if playstyle < 50 => maximum,
        1 if playstyle > 50 => minimum,
        _ => 1.,
    }
}

/// REL 0x0af550: unknown property returns -1, a missing selected config
/// entry returns zero, and disabling playstyle scaling returns the base value.
/// Missing per-property endpoints use the ability's PlaystyleMin/Max values.
pub fn evaluate(
    property: Option<Property>,
    mode: i32,
    playstyle: i32,
    scale: bool,
    ability_minimum: f32,
    ability_maximum: f32,
) -> f32 {
    let Some(property) = property else {
        return -1.;
    };
    let Some(value) = property.value else {
        return 0.;
    };
    if !scale {
        return value;
    }
    value
        * multiplier(
            mode,
            playstyle,
            property.playstyle_min.unwrap_or(ability_minimum),
            property.playstyle_max.unwrap_or(ability_maximum),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_preserves_absent_endpoints_and_stops_at_first_base_gap() {
        let mut keys = Vec::new();
        let p = RegisteredProperty::load("Spin", "Paint", "Wait", |key| {
            keys.push(key.to_owned());
            Ok::<_, ()>(match key {
                "MickeyAbilities_Spin_Paint_Wait_1" => Some(0.3),
                "MickeyAbilities_Spin_Paint_Wait_PlaystyleMin_2" => Some(0.5),
                "MickeyAbilities_Spin_Paint_Wait_3" => Some(0.1),
                _ => None,
            })
        })
        .unwrap();
        assert_eq!(p.levels.len(), 2);
        assert_eq!(p.levels[0].value, Some(0.3));
        assert_eq!(p.levels[0].playstyle_min, None);
        assert_eq!(p.levels[1].value, None);
        assert_eq!(p.levels[1].playstyle_min, Some(0.5));
        assert_eq!(keys.len(), 6);
        assert!(!keys.iter().any(|k| k.ends_with("_3")));
    }
    #[test]
    fn threshold_is_discrete_and_mode_reverses_endpoints() {
        for p in [0, 49] {
            assert_eq!(multiplier(0, p, 0.5, 2.), 0.5);
            assert_eq!(multiplier(1, p, 0.5, 2.), 2.);
        }
        for p in [51, 100] {
            assert_eq!(multiplier(0, p, 0.5, 2.), 2.);
            assert_eq!(multiplier(1, p, 0.5, 2.), 0.5);
        }
        assert_eq!(multiplier(0, 50, 0.5, 2.), 1.);
        assert_eq!(multiplier(1, 50, 0.5, 2.), 1.);
        assert_eq!(multiplier(2, 0, 0.5, 2.), 1.);
    }
    #[test]
    fn preserves_missing_entries_and_independent_endpoint_fallbacks() {
        let p = Property {
            value: Some(4.),
            playstyle_min: Some(0.25),
            playstyle_max: None,
        };
        assert_eq!(evaluate(Some(p), 0, 49, true, 0.5, 3.), 1.);
        assert_eq!(evaluate(Some(p), 0, 51, true, 0.5, 3.), 12.);
        assert_eq!(evaluate(Some(p), 0, 51, false, 0.5, 3.), 4.);
        assert_eq!(evaluate(None, 0, 50, true, 1., 1.), -1.);
        assert_eq!(
            evaluate(Some(Property { value: None, ..p }), 0, 50, true, 1., 1.),
            0.
        );
    }
}
