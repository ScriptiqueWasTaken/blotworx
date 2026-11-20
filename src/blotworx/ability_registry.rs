//! Native registered save groups, REL88ab8..88d80 and212b4..21530.
//! Registry/common ownership do not implement each ability's action or Give override.
use super::{ability_ownership::Ownership, ability_persistence, save_game};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
pub const PRIMARY: [&str; 3] = ["Blast", "Invisible", "Indelible"];
pub const SECONDARY: [&str; 4] = ["Brush", "Slide", "Spin", "Slash"];

fn sketch_maximum_key(name: &str, index: usize) -> Result<String> {
    // REL21454..2146c snprintf size 0x63 includes the final NUL.
    let mut key = format!("{name}_{index}");
    let length = key.len().min(98);
    ensure!(key.is_char_boundary(length), "Native sketch save-key truncation splits a UTF-8 codepoint");
    key.truncate(length);
    Ok(key)
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Sketch {
    pub name: String,
    pub ownership: Ownership,
}
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Registry {
    pub sketches: Vec<Sketch>,
}
impl Registry {
    ///88b78..88c24: contiguous numbered entries, first missing terminates;
    ///256 is the original loop limit. Retain duplicate names as separate slots.
    pub fn load(settings: &BTreeMap<String, String>) -> Self {
        let mut sketches = Vec::new();
        for index in 1..=256 {
            let Some(value) = settings.get(&format!("MickeyAbilities_Sketches_Sketch_{index}"))
            else {
                break;
            };
            sketches.push(Sketch {
                name: format!("Sketch_{}", value.split('\0').next().unwrap_or("")),
                ownership: Ownership::default(),
            });
        }
        Self { sketches }
    }
    pub fn base_ownership() -> BTreeMap<String, Ownership> {
        PRIMARY
            .into_iter()
            .chain(SECONDARY)
            .map(|name| (name.to_owned(), Ownership::default()))
            .collect()
    }
    /// REL21480..214c4 restores each sketch's ownership and indexed capacity.
    pub(super) fn restore_sketches(&mut self, payload: &[u8], maxima: &mut [f32]) -> Result<()> {
        ensure!(maxima.len() >= self.sketches.len(), "Checkpoint player is missing registered SketchMax entries");
        let records = save_game::records(payload)?;
        for (index, (sketch, maximum)) in self.sketches.iter_mut().zip(maxima).enumerate() {
            if let Some(grant) = ability_persistence::grant(&records, &sketch.name, &sketch.ownership)? {
                sketch.ownership.give(grant.selector, grant.level);
            }
            let key = sketch_maximum_key(&sketch.name, index)?.to_ascii_uppercase();
            // 25e590 writes the provided default (Wii REL4ac3e8, zero)
            // even when the named record is absent.
            let value = match records.iter().find(|record| record.name == key) {
                Some(record) => f32::from_be_bytes(record.data.get(..4)
                    .with_context(|| format!("Truncated saved sketch capacity {key}"))?
                    .try_into()?),
                None => 0.,
            };
            ensure!(value.is_finite(), "Nonfinite saved sketch capacity {key}");
            *maximum = value;
        }
        Ok(())
    }

    /// Native stack float destination is uninitialized if an indexed SketchMax
    /// getter fails. Diagnose that condition instead of inventing a capacity.
    pub fn records(&self, base: &BTreeMap<String, Ownership>, maxima: &[f32]) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        self.append(&mut output, base, maxima)?;
        Ok(output)
    }
    /// Named data offsets are section-relative: append into the caller's
    /// stream rather than concatenating a separately encoded record buffer.
    pub fn append(
        &self,
        output: &mut Vec<u8>,
        base: &BTreeMap<String, Ownership>,
        maxima: &[f32],
    ) -> Result<()> {
        ensure!(maxima.len()>=self.sketches.len(),"SketchMax has fewer entries than the registered sketch list; native saved value is uninitialized");
        for name in PRIMARY.into_iter().chain(SECONDARY) {
            ability_persistence::append(
                output,
                name,
                base.get(name)
                    .with_context(|| format!("Registered ability {name} has no live ownership"))?,
            )?;
        }
        for (index, sketch) in self.sketches.iter().enumerate() {
            ability_persistence::append(output, &sketch.name, &sketch.ownership)?;
            let key = sketch_maximum_key(&sketch.name, index)?;
            save_game::append_record(output, &key, &maxima[index].to_bits().to_be_bytes(), 4)?;
        }
        Ok(())
    }
}
impl super::assets::World {
    pub fn player_registered_ability_records(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        self.append_player_registered_ability_records(&mut output)?;
        Ok(output)
    }
    pub(super) fn append_player_registered_ability_records(
        &self,
        output: &mut Vec<u8>,
    ) -> Result<()> {
        let owner = self
            .entities
            .get(self.player_entity)
            .context("Mickey is not owned")?;
        let components = owner
            .components
            .iter()
            .filter(|component| component.class_name == "InventoryComponent")
            .collect::<Vec<_>>();
        ensure!(
            components.len() == 1,
            "Player inventory component selection is ambiguous"
        );
        let maximum = components[0]
            .properties
            .iter()
            .find(|property| property.name == "SketchMax")
            .and_then(|property| property.value.as_array())
            .context("Live SketchMax array missing")?;
        let maxima = maximum
            .iter()
            .map(|value| {
                value
                    .as_f64()
                    .map(|value| value as f32)
                    .context("Non-numeric SketchMax entry")
            })
            .collect::<Result<Vec<_>>>()?;
        self.player_ability_registry
            .append(output, &self.player_abilities, &maxima)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_sketch_capacities_are_indexed_and_missing_records_default_to_zero() -> Result<()> {
        let mut registry = Registry {
            sketches: ["Sketch_TV", "Sketch_TV", "Sketch_Anvil"]
                .into_iter()
                .map(|name| Sketch { name: name.into(), ownership: Ownership::default() })
                .collect(),
        };
        let mut payload = Vec::new();
        save_game::append_record(&mut payload, "Sketch_TV:2", &2i32.to_be_bytes(), 4)?;
        save_game::append_record(&mut payload, "Sketch_TV_0", &8.25f32.to_be_bytes(), 4)?;
        save_game::append_record(&mut payload, "Sketch_TV_1", &3.5f32.to_be_bytes(), 4)?;
        let mut maxima = [10., 10., 10., 20.];
        registry.restore_sketches(&payload, &mut maxima)?;
        assert_eq!(maxima, [8.25, 3.5, 0., 20.]);
        for sketch in &registry.sketches[..2] {
            assert_eq!(sketch.ownership.owned, [false, true]);
            assert_eq!(sketch.ownership.upgrades, [0, 0]);
        }
        assert_eq!(registry.sketches[2].ownership.owned, [false, false]);
        Ok(())
    }

    #[test]
    fn numbered_registry_preserves_slots_gaps_duplicates_and_native_limit() -> Result<()> {
        let mut settings = BTreeMap::new();
        for (index, value) in [(1, "TV"), (2, "TV"), (4, "Watch")] {
            settings.insert(
                format!("MickeyAbilities_Sketches_Sketch_{index}"),
                value.to_owned(),
            );
        }
        let registry = Registry::load(&settings);
        assert_eq!(registry.sketches.len(), 2);
        let mut base = Registry::base_ownership();
        base.get_mut("Spin").unwrap().give("Thinner", 7);
        let mut registry = registry;
        registry.sketches[1].ownership.give("Paint", 2);
        let bytes = registry.records(&base, &[2., 9.])?;
        let records = save_game::records(&bytes)?;
        assert_eq!(
            records.iter().map(|record| record.name).collect::<Vec<_>>(),
            [
                "BLAST:0",
                "INVISIBLE:0",
                "INDELIBLE:0",
                "BRUSH:0",
                "SLIDE:0",
                "SPIN:0",
                "SLASH:0",
                "SKETCH_TV:0",
                "SKETCH_TV_0",
                "SKETCH_TV:2",
                "SKETCH_TV_1"
            ]
        );
        assert_eq!(records[5].data, &2i32.to_be_bytes());
        assert_eq!(records[8].data, &2f32.to_be_bytes());
        assert_eq!(records[10].data, &9f32.to_be_bytes());
        assert!(registry.records(&base, &[2.]).is_err());
        base.remove("Slide");
        assert!(registry.records(&base, &[2., 9.]).is_err());
        for index in 1..=257 {
            settings.insert(
                format!("MickeyAbilities_Sketches_Sketch_{index}"),
                index.to_string(),
            );
        }
        assert_eq!(Registry::load(&settings).sketches.len(), 256);
        Ok(())
    }
    #[test]
    fn sketch_maximum_key_uses_native_byte_buffer_size() -> Result<()> {
        let registry = Registry {
            sketches: vec![Sketch {
                name: "A".repeat(120),
                ownership: Ownership::default(),
            }],
        };
        let bytes = registry.records(&Registry::base_ownership(), &[5.])?;
        let records = save_game::records(&bytes)?;
        assert_eq!(records.last().unwrap().name.len(), 98);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 data and save"]
    fn original_registered_ability_order_matches_native_mick() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        assert_eq!(
            world
                .player_ability_registry
                .sketches
                .iter()
                .map(|slot| slot.name.as_str())
                .collect::<Vec<_>>(),
            ["Sketch_TV", "Sketch_Anvil", "Sketch_Watch"]
        );
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let original = std::fs::read(&path)?;
        let archive = save_game::Archive::parse(&original)?;
        let source = save_game::records(&archive.section(*b"MICK")?.payload)?;
        let bytes = world.player_registered_ability_records()?;
        let actual = save_game::records(&bytes)?;
        let source = source
            .into_iter()
            .filter(|record| {
                record.name.contains(':')
                    || record.name.strip_prefix("SKETCH_").is_some_and(|tail| {
                        tail.rsplit_once('_')
                            .is_some_and(|(_, index)| index.parse::<usize>().is_ok())
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), 13);
        assert_eq!(source.len(), 13);
        for (actual, source) in actual.iter().zip(source) {
            // Ownership/upgrade values differ at constructor vs saved gameplay.
            assert_eq!(actual.name.split(':').next(), source.name.split(':').next());
            if !actual.name.contains(':') {
                assert_eq!(actual.data, source.data);
            }
        }
        assert!(world
            .player_abilities
            .values()
            .all(|state| state.owned == [false, false]));
        assert!(world.player_give_command("Give Slide All 1").is_err());
        assert_eq!(world.player_abilities["Slide"].owned, [false, false]);
        world.enable_demo_abilities()?;
        assert_eq!(world.player_abilities["Brush"].owned, [true, true]);
        for name in ["Invisible", "Indelible", "Slide", "Slash"] {
            assert_eq!(world.player_abilities[name].owned, [false, false]);
        }
        assert_eq!(std::fs::read(path)?, original);
        Ok(())
    }
}

impl super::assets::World {
    /// REL91904/91a2c ->92ad4: despite its name this is a setter,
    /// operating on the primary controller even for the SD variant.
    pub(super) fn player_sketch_availability_lua(&mut self, lua:&mlua::Lua, name:&str, mut args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        let expected=if name.ends_with("SD") {2} else {1};
        if args.len()!=expected {self.report.warnings.push(format!("{name}: expected {expected} arguments; preserving native stack order"));}
        let enabled=super::actor_scripts::native_boolean(lua,args.pop_back().unwrap_or(mlua::Value::Nil))?;
        if expected==2 {let _=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(mlua::Value::Nil))?;}
        if !self.removed_entities.contains(&self.player_entity) && self.entities.get(self.player_entity).is_some_and(|e|e.components.iter().any(|c|c.class_name=="MickeyPlayerControllerComponent")) {
            if name.starts_with("PlayerHasPaintThinner") {self.set_named_camera_actions_disabled(!enabled).map_err(mlua::Error::external)?;} else {
                for sketch in &mut self.player_ability_registry.sketches {sketch.ownership.set_disabled(!enabled);}
            }
        }
        Ok(mlua::MultiValue::new())
    }
}
