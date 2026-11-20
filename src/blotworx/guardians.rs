//! Guardian system configuration/lifecycle from EM1 REL14c9c,16398,
//! 16dbc,16e20 and16ecc. Population, point spending and controller motion
//! are distinct native systems; a caller must supply their real effects.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

/// Retain only the two guardian event action lists, not the bank's media.
/// The audio device must drive process_block; game frames are not audio ticks.
pub struct Audio {
    actions: [Vec<Vec<u8>>; 2],
    pub nodes: super::audio_nodes::Nodes,
    pub schedule: super::audio_schedule::Schedule,
}
impl Audio {
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let bank = super::audio_bank::Bank::parse(bytes)?;
        let mut nodes = super::audio_nodes::Nodes::new()?;
        nodes.register_bank(&bank)?;
        let mut actions: [Vec<Vec<u8>>; 2] = Default::default();
        for (index, name) in ["Mute_Guardians", "Unmute_Guardians"]
            .into_iter()
            .enumerate()
        {
            for id in bank.named_event_actions(name)? {
                let object = bank
                    .objects
                    .get(&id)
                    .context("Missing guardian audio action")?;
                let action = object.action()?;
                let mute = action.global_mute()?;
                ensure!(
                    action.delay[1] == action.delay[2] && mute.transition[1] == mute.transition[2],
                    "Randomized guardian audio actions require the native RNG binding"
                );
                actions[index].push(object.payload.to_vec());
            }
        }
        Ok(Self {
            actions,
            nodes,
            schedule: Default::default(),
        })
    }
    pub fn present(&mut self, visible: bool) -> Result<()> {
        for payload in &self.actions[usize::from(visible)] {
            self.schedule.post_action(payload, &mut self.nodes, || {
                unreachable!("Validated nonrandom action")
            })?;
        }
        Ok(())
    }
    pub fn process_block(&mut self) -> Result<()> {
        self.schedule.process_block(&mut self.nodes, || {
            unreachable!("Validated nonrandom action")
        })?;
        Ok(())
    }
}

pub struct Costs {
    pub paint: Vec<f32>,
    pub thinner: Vec<f32>,
}
/// MickeyPlayerController+40/+44 and alignment+50. Store runtime handles,
/// never palette-local IDs: two source scenes can reuse an authored ID.
pub struct Ownership<T> {
    entities: Vec<T>,
    pub alignment: i32,
}
impl<T> Default for Ownership<T> {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            alignment: 0x32,
        }
    }
}
impl<T: Copy + Eq> Ownership<T> {
    pub fn entities(&self) -> &[T] {
        &self.entities
    }
    /// REL86a20 inserts after initialization/configuration of the new guardian.
    /// Null roots and failed component lookup never reach this publication.
    pub fn insert(&mut self, index: u16, entity: T) -> Result<()> {
        let index = usize::from(index);
        ensure!(
            index <= self.entities.len(),
            "Guardian insertion index is outside the owned list"
        );
        ensure!(
            self.entities.len() < usize::from(u16::MAX),
            "Guardian list exceeds the native u16 count; overflow behavior is unsupported"
        );
        self.entities.insert(index, entity);
        Ok(())
    }
    /// REL865f8 removes the first match and shifts subsequent entries. It does
    /// not rewrite surviving GuardianController+b4 creation-slot values.
    pub fn remove(&mut self, entity: Option<T>) -> Option<usize> {
        let index =
            entity.and_then(|entity| self.entities.iter().position(|&value| value == entity));
        if let Some(index) = index {
            self.entities.remove(index);
        }
        if self.entities.is_empty() {
            self.alignment = 0x32;
        }
        index
    }
}
impl Costs {
    pub fn load(settings: &BTreeMap<String, String>) -> Result<Self> {
        fn costs(settings: &BTreeMap<String, String>, kind: &str) -> Result<Vec<f32>> {
            let mut result = Vec::new();
            let mut level = 1u32;
            loop {
                let name = format!("Guardians_{kind}_Guardian_Level{level}_PointCost");
                let Some(value) = settings.get(&name) else {
                    break;
                };
                let value: f32 = value.parse().with_context(|| format!("Invalid {name}"))?;
                ensure!(value.is_finite(), "Non-finite {name}");
                // Do not impose a three-tier cap: the original constructor
                // stops on the first absent next-level configuration variable.
                result.push(value);
                level = level
                    .checked_add(1)
                    .context("Guardian tier identity overflow")?;
            }
            ensure!(
                !result.is_empty(),
                "Missing Guardians_{kind}_Guardian_Level1_PointCost"
            );
            Ok(result)
        }
        Ok(Self {
            paint: costs(settings, "Paint")?,
            thinner: costs(settings, "Thinner")?,
        })
    }
    /// REL16ecc builds cumulative-cost inputs from the neutral state followed
    /// by the same side's states before its terminal sentinel. REL16c24 sums
    /// the first active count entries in order, using fadds at each step.
    pub fn active_cost(&self, paint: bool, count: usize) -> f32 {
        let costs = if paint { &self.paint } else { &self.thinner };
        costs
            .iter()
            .take(count)
            .fold(0f32, |total, &value| total + value)
    }
}
pub trait Effects {
    /// Native16a18 recomputes the selected guardian state when dirty.
    /// Returns whether the recomputation remains deferred (entity-manager+8c).
    fn recompute(&mut self) -> Result<bool>;
    /// Native16e44->86be0: audio event followed by visibility on the
    /// primary controller's guardian entity list, in its stored order.
    fn present(&mut self, visible: bool) -> Result<()>;
}
#[derive(serde::Serialize)]
pub struct State {
    pub enabled: bool,
    pub visible: bool,
    pub dirty: bool,
    /// Global guardian state-machine fields, distinct from the player's
    /// owned guardian list. REL14c9c initializes these to neutral (50).
    pub selected_kind: i32,
    pub selected_id: i32,
    pub hero_terminal: i32,
    pub scrapper_terminal: i32,
    pub pending_points: f32,
    pending_presentation: Option<bool>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            enabled: true,
            visible: true,
            dirty: false,
            selected_kind: 50,
            selected_id: 50,
            hero_terminal: 50,
            scrapper_terminal: 50,
            pending_points: 0.,
            pending_presentation: None,
        }
    }
}
impl State {
    /// REL18d38..18d64 adds signed single-precision points while enabled.
    /// The guardian update consumes this balance separately.
    pub fn add_points(&mut self, hero: bool, amount: f32) -> Result<()> {
        let delta = if hero { amount } else { -amount };
        if self.enabled {
            let next = self.pending_points + delta;
            ensure!(next.is_finite(), "Guardian point balance is not finite");
            self.pending_points = next;
        }
        Ok(())
    }
    pub fn drain_presentation(&mut self, effects:&mut impl Effects)->Result<()> {
        if let Some(visible)=self.pending_presentation {
            effects.present(visible)?;
            self.pending_presentation=None;
        }
        Ok(())
    }
    fn present(&mut self,visible:bool,effects:&mut impl Effects)->Result<()> {
        self.pending_presentation=Some(visible);
        self.drain_presentation(effects)
    }
    /// REL15ab8/15b40: side gate first, then integer-to-double conversion,
    /// subtracting neutral50 and rounding the division once to single.
    /// No clamp: the native getters do not constrain the result.
    pub fn percentage(&self, hero: bool) -> f32 {
        if self.selected_kind != if hero { 51 } else { 49 } { return 0.; }
        let end = if hero { self.hero_terminal } else { self.scrapper_terminal };
        ((self.selected_id.wrapping_sub(50) as f64)
            / (end.wrapping_sub(50) as f64)) as f32
    }
    pub fn set_enabled(&mut self, enabled: bool, effects: &mut impl Effects) -> Result<()> {
        self.drain_presentation(effects)?;
        if self.enabled == enabled {
            return Ok(());
        }
        self.enabled = enabled;
        if enabled && self.dirty {
            self.dirty = effects.recompute()?;
        }
        if self.visible {
            self.present(self.enabled && self.visible,effects)?;
        }
        Ok(())
    }
    pub fn set_visible(&mut self, visible: bool, effects: &mut impl Effects) -> Result<()> {
        self.drain_presentation(effects)?;
        if self.visible == visible {
            return Ok(());
        }
        self.visible = visible;
        if self.enabled {
            self.present(self.enabled && self.visible,effects)?;
        }
        Ok(())
    }
}
#[cfg(test)]
#[path="../../tools/blotworx/tests/guardian_visibility_cases.rs"]
mod visibility_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Trace(Vec<&'static str>);
    impl Effects for Trace {
        fn recompute(&mut self) -> Result<bool> {
            self.0.push("recompute");
            Ok(false)
        }
        fn present(&mut self, visible: bool) -> Result<()> {
            self.0.push(if visible {
                "Unmute_Guardians / unhide"
            } else {
                "Mute_Guardians / hide"
            });
            Ok(())
        }
    }
    #[test]
    fn lifecycle_keeps_native_transition_gates_and_side_effect_order() -> Result<()> {
        let mut state = State::default();
        let mut effects = Trace::default();
        state.set_enabled(true, &mut effects)?;
        assert!(effects.0.is_empty());
        state.set_enabled(false, &mut effects)?;
        state.set_visible(false, &mut effects)?; // Hidden changes while disabled are not presented.
        state.dirty = true;
        state.set_enabled(true, &mut effects)?;
        state.set_visible(true, &mut effects)?;
        assert_eq!(
            effects.0,
            [
                "Mute_Guardians / hide",
                "recompute",
                "Unmute_Guardians / unhide"
            ]
        );
        assert!(!state.dirty);
        state.set_enabled(true, &mut effects)?;
        state.set_visible(true, &mut effects)?;
        assert_eq!(effects.0.len(), 3);
        Ok(())
    }
    #[test]
    fn ownership_preserves_native_insertion_removal_and_empty_alignment() -> Result<()> {
        let mut owned = Ownership::default();
        owned.insert(0, 10)?;
        owned.insert(0, 20)?;
        owned.insert(1, 30)?;
        owned.alignment = 0x33;
        assert_eq!(owned.entities(), [20, 30, 10]);
        assert_eq!(owned.remove(None), None);
        assert_eq!(owned.remove(Some(99)), None);
        assert_eq!(owned.remove(Some(30)), Some(1));
        assert_eq!(owned.entities(), [20, 10]);
        assert_eq!(owned.alignment, 0x33);
        // Native publication does not deduplicate entries; removal is first-match.
        owned.insert(2, 20)?;
        owned.remove(Some(20));
        assert_eq!(owned.entities(), [10, 20]);
        assert!(owned.insert(4, 40).is_err());
        assert_eq!(owned.entities(), [10, 20]);
        owned.remove(Some(10));
        owned.remove(Some(20));
        assert_eq!(owned.alignment, 0x32);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_lua_guardian_callbacks_drive_world_visibility_and_audio_actions() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level",
            &egui::Context::default(),
        )?;
        // Import callbacks can already have disabled the system. Establish
        // the intended enabled precondition through the real synchronous API,
        // settling its audio before publishing the synthetic owned entity.
        world.run_callback_fixture("EnableGuardianSystem()")?;
        for _ in 0..833 {
            world.guardian_audio.process_block()?;
        }
        assert!(world.guardians.enabled);
        // Use a real imported entity to observe recursive renderer visibility;
        // ownership publication is synthetic here, not guardian creation proof.
        let entity = world.player_entity;
        world.guardian_entities.insert(0, entity)?;
        world.run_callback_fixture("assert(select('#',DisableGuardianSystem(GetPlayer()))==0)")?;
        assert!(!world.guardians.enabled, "{:?}", world.report.unsupported);
        assert!(world.hidden_entities.contains(&entity));
        for _ in 0..167 {
            world.guardian_audio.process_block()?;
        }
        for target in [0x284456db, 0x1c6d40f0] {
            assert_eq!(world.guardian_audio.nodes.byte(target), Some(0));
        }
        let pending = world.guardian_audio.schedule.pending();
        world.run_callback_fixture("DisableGuardianSystem()")?;
        assert_eq!(world.guardian_audio.schedule.pending(), pending);
        world.run_callback_fixture(
            "assert(select('#',EnableGuardianSystem('missing-entity'))==0)",
        )?;
        assert!(world.guardians.enabled);
        assert!(!world.hidden_entities.contains(&entity));
        assert_eq!(world.guardian_audio.schedule.pending(), 2);
        for _ in 0..833 {
            world.guardian_audio.process_block()?;
        }
        for target in [0x284456db, 0x1c6d40f0] {
            assert_eq!(world.guardian_audio.nodes.byte(target), Some(254));
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_guardian_prefab_graphs_survive_world_import() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        assert!(world.guardian_entities.entities().is_empty());
        for name in ["paintguardian", "thinnerguardian"] {
            let graph = &world.guardian_templates[name];
            let root = &graph.definitions[&graph.root];
            ensure!(
                root.components
                    .iter()
                    .any(|component| component.class_name == "GuardianControllerComponent"),
                "Missing authored guardian controller"
            );
            ensure!(
                root.source_scene
                    .eq_ignore_ascii_case("Palettes/_Dynamic.bin"),
                "Guardian resolved outside global palette"
            );
            for definition in graph.definitions.values() {
                for dependency in definition
                    .children
                    .iter()
                    .chain(definition.effects.iter().map(|effect| &effect.template))
                {
                    ensure!(
                        graph
                            .definitions
                            .contains_key(&dependency.to_ascii_lowercase()),
                        "Missing guardian prefab dependency {dependency}"
                    );
                }
                if let Some(path) = &definition.model {
                    epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&graph.models[path])
                        .map_err(anyhow::Error::msg)
                        .with_context(|| format!("Guardian model {path}"))?;
                }
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_guardian_root_effects_support_attached_particle_playback() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let root = std::path::Path::new(&root);
        let mut files = Vec::new();
        for name in ["_dynamic.pak", "r3mt.pak"] {
            files.extend(
                crate::pack_io::read(std::fs::read(root.join("packfiles").join(name))?)?
                    .pak
                    .files,
            );
        }
        for (name, path) in [
            ("paintguardian", "Effects/_Shared/fx_guardian_paint.nif"),
            ("thinnerguardian", "Effects/_Shared/fx_guardian_thinner.nif"),
        ] {
            let file = files
                .iter()
                .find(|file| {
                    file.path.eq_ignore_ascii_case(path)
                        || file.path.eq_ignore_ascii_case(&format!("{path}_wii"))
                })
                .with_context(|| format!("Missing original {path}"))?;
            let mut effect = super::super::brush_particles::Hose::load_attached(&file.data, &files)
                .with_context(|| format!("Guardian root playback {name}"))?;
            let attachment = [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 10., 20., 30., 1.,
            ];
            let mut rendered = false;
            for _ in 0..60 {
                effect.advance_attached(1. / 30., true, attachment)?;
                let mut draws = Vec::new();
                effect.append([1., 0., 0.], [0., 1., 0.], &mut draws)?;
                rendered |= !draws.is_empty();
            }
            ensure!(
                rendered,
                "Guardian effect emitted no renderable particles: {name}"
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_config_retains_guardian_costs_and_contiguous_tier_discovery() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let text = std::fs::read_to_string(std::path::Path::new(&root).join("ConfigFiles.ini"))?;
        let mut settings = crate::render_support::game_config::runtime_settings(&text, false);
        let costs = Costs::load(&settings)?;
        assert_eq!(costs.paint, [15., 15., 15.]);
        assert_eq!(costs.thinner, [-15., -15., -15.]);
        assert_eq!(costs.active_cost(true, 0), 0.);
        assert_eq!(costs.active_cost(true, 3), 45.);
        assert_eq!(costs.active_cost(false, 3), -45.);
        settings.insert(
            "Guardians_Paint_Guardian_Level4_PointCost".into(),
            "17".into(),
        );
        assert_eq!(Costs::load(&settings)?.paint, [15., 15., 15., 17.]);
        settings.remove("Guardians_Paint_Guardian_Level2_PointCost");
        assert_eq!(Costs::load(&settings)?.paint, [15.]);
        Ok(())
    }
}
