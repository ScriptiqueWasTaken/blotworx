//! Guardian tier records (20 parameter references) and REL7d7ac binding.
//! This reproduces parameter ownership and initial caches, not movement,
//! combat, phantom collision or the guardian component's complete lifecycle.
use super::guardians::Costs;
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

// Native record offsets0..4c, verified against default construction79e00..
// 7aa30 and the binding reads7d7ac..7e104. PointCost is a separate state input.
pub const FIELDS: [&str; 20] = [
    "MaxHealth",
    "MovementSpeed",
    "TargetSpeed",
    "UndulatingSpeed",
    "Acceleration",
    "FollowingDistance",
    "DamageToEnemies",
    "DamageToToon",
    "DamageFromEnemyCollision",
    "NormalHorizAmplitude",
    "HorizontalAmplitude",
    "HorizontalAmplitudeSpeed",
    "HorizontalFrequency",
    "NormalVertAmplitude",
    "VerticalAmplitude",
    "VerticalAmplitudeSpeed",
    "VerticalFrequency",
    "LengthOfUndulation",
    "IntervalBetweenUndulation",
    "ChanceToChangeDirections",
];
pub struct Parameter {
    bits: AtomicU32,
}
impl Parameter {
    pub fn value(&self) -> f32 {
        f32::from_bits(self.bits.load(Ordering::Relaxed))
    }
}
pub struct Parameters {
    pub fields: [Arc<Parameter>; 20],
}
pub struct Library {
    values: BTreeMap<String, Arc<Parameter>>,
    paint: Vec<Arc<Parameters>>,
    thinner: Vec<Arc<Parameters>>,
}
fn parse(name: &str, value: &str) -> Result<f32> {
    // Numeric configuration path REL14aeac first parses a double, rounds it
    // with frsp, then clamps to the parameter's original +/-FLT_MAX bounds.
    // Direct decimal-to-f32 parsing can differ at double-rounding boundaries.
    let value: f64 = value
        .parse()
        .with_context(|| format!("Unsupported guardian numeric text {name}"))?;
    ensure!(value.is_finite(), "Non-finite guardian parameter {name}");
    Ok((value as f32).clamp(-f32::MAX, f32::MAX))
}
impl Library {
    pub fn load(costs: &Costs, settings: &BTreeMap<String, String>) -> Result<Self> {
        let mut library = Self {
            values: BTreeMap::new(),
            paint: Vec::new(),
            thinner: Vec::new(),
        };
        for (kind, count) in [
            ("Paint", costs.paint.len()),
            ("Thinner", costs.thinner.len()),
        ] {
            for level in 1..=count {
                let mut fields = Vec::new();
                for field in FIELDS {
                    let name = format!("Guardians_{kind}_Guardian_Level{level}_{field}");
                    let text=settings.get(&name).with_context(||format!("Missing guardian parameter {name}; compiled fallback not reconstructed"))?;
                    let value = parse(&name, text)?;
                    let parameter = library
                        .values
                        .entry(name)
                        .or_insert_with(|| {
                            Arc::new(Parameter {
                                bits: AtomicU32::new(value.to_bits()),
                            })
                        })
                        .clone();
                    fields.push(parameter);
                }
                let fields = fields
                    .try_into()
                    .ok()
                    .context("Guardian record has incorrect field count")?;
                let parameters = Arc::new(Parameters { fields });
                if kind == "Paint" {
                    library.paint.push(parameters);
                } else {
                    library.thinner.push(parameters);
                }
            }
        }
        Ok(library)
    }
    pub fn tier(&self, paint: bool, level: usize) -> Result<Arc<Parameters>> {
        let levels = if paint { &self.paint } else { &self.thinner };
        let index = level
            .checked_sub(1)
            .context("Guardian tiers start at one")?;
        levels
            .get(index)
            .cloned()
            .context("Guardian tier has no admitted parameter record")
    }
    /// Changes shared parameter storage. Native initial-value caches are
    /// deliberately untouched until an instance is rebound. Gameplay config
    /// change callbacks still need to call this when they are implemented.
    pub fn set(&self, name: &str, text: &str) -> Result<()> {
        let parameter = self
            .values
            .get(name)
            .context("Guardian parameter is not registered")?;
        parameter
            .bits
            .store(parse(name, text)?.to_bits(), Ordering::Relaxed);
        Ok(())
    }
}
// Native float-cache offsets in the same order as Binding.cached. Several
// parameters are retained as references AND copied into evolving local state.
pub const CACHE_OFFSETS: [u16; 9] = [0xec, 0xf4, 0x10c, 0x104, 0x100, 0xf8, 0x118, 0x110, 0x114];
const CACHE_FIELDS: [usize; 9] = [1, 3, 11, 9, 15, 13, 3, 1, 1];
const BOUND_FIELDS: [usize; 17] = [0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 16, 17, 18, 19];
pub struct Binding {
    pub references: [Option<Arc<Parameter>>; 20],
    pub health: i32,
    pub cached: [f32; 9],
}
impl Binding {
    pub fn new(parameters: Arc<Parameters>) -> Result<Self> {
        // Config numeric cache REL14aeec uses fctiwz after frsp/clamping;
        // binding7e0e4 copies that integer cache, not a rounded health value.
        let health = parameters.fields[0].value();
        ensure!(
            health as f64 >= i32::MIN as f64 && (health as f64) < i32::MAX as f64 + 1.,
            "Guardian MaxHealth outside recovered integer conversion range"
        );
        let cached = CACHE_FIELDS.map(|index| parameters.fields[index].value());
        let references = std::array::from_fn(|index| {
            BOUND_FIELDS
                .contains(&index)
                .then(|| parameters.fields[index].clone())
        });
        Ok(Self {
            references,
            health: health.trunc() as i32,
            cached,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_tiers_bind_native_cache_slots_and_share_live_parameter_references() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let text = std::fs::read_to_string(std::path::Path::new(&root).join("ConfigFiles.ini"))?;
        let settings = crate::render_support::game_config::runtime_settings(&text, false);
        let library = Library::load(&Costs::load(&settings)?, &settings)?;
        let paint = library.tier(true, 1)?;
        let first = Binding::new(paint.clone())?;
        assert!(Arc::ptr_eq(&paint, &library.tier(true, 1)?));
        assert_eq!(first.health, 1);
        assert_eq!(
            first
                .references
                .iter()
                .filter(|reference| reference.is_some())
                .count(),
            17
        );
        for index in [3, 11, 15] {
            assert!(first.references[index].is_none());
        }
        assert_eq!(first.cached, [1.5, 1., 0.2, 0.1, 0.2, 0.05, 1., 1.5, 1.5]);
        let thinner = Binding::new(library.tier(false, 1)?)?;
        assert_eq!(thinner.cached, [1., 0.75, 0.5, 0.2, 0.5, 0., 0.75, 1., 1.]);
        for side in [true, false] {
            for level in 1..=3 {
                Binding::new(library.tier(side, level)?)?;
            }
        }
        assert!(library.tier(true, 0).is_err());
        assert!(library.tier(true, 4).is_err());
        library.set("Guardians_Paint_Guardian_Level1_MovementSpeed", "2.25")?;
        assert_eq!(first.references[1].as_ref().unwrap().value(), 2.25);
        assert_eq!(first.cached[0], 1.5);
        assert_eq!(Binding::new(paint.clone())?.cached[0], 2.25);
        assert!(library
            .set("Guardians_Paint_Guardian_Level1_MovementSpeed", "NaN")
            .is_err());
        assert_eq!(first.references[1].as_ref().unwrap().value(), 2.25);
        library.set("Guardians_Paint_Guardian_Level1_MaxHealth", "2.9")?;
        assert_eq!(Binding::new(paint)?.health, 2);
        Ok(())
    }
}
