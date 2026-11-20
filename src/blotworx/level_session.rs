//! Live singleton transfer across deferred level replacement. This is a session
//! handoff, not a disk-save restore: nonpersistent globals remain live too.
use super::{assets::World, *};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct ScriptTransfer(Arc<Mutex<Option<actor_scripts::Scripts>>>);
impl std::fmt::Debug for ScriptTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScriptTransfer(single-use Lua execution owner)")
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    source_level: String,
    globals: global_state::State,
    quests: quest_state::State,
    options: save_options::State,
    instances: save_instances::State,
    extras: extras::State,
    collectibles: collectibles::State,
    tracking: BTreeSet<String>,
    player: Player,
    countdown: countdown::State,
    input_presentation: input_presentation::State,
    scripts: ScriptTransfer,
    source_player: usize,
}
#[derive(Clone, Debug)]
pub struct Player {
    components: Vec<epic_mickey_lib_rs::scene_file::Component>,
    ammo: Option<ammo::Inventory>,
    ammo_settings: ammo::Settings,
    abilities: BTreeMap<String, ability_ownership::Ownership>,
    sketch_registry: ability_registry::Registry,
}
impl Session {
    pub fn capture(world: &mut World) -> anyhow::Result<Self> {
        // Pickup/sketch commands update live component properties; the
        // authored_inventory map is only an import-time snapshot.
        let mut components = world.entities[world.player_entity]
            .components
            .iter()
            .filter(|component| {
                matches!(
                    component.class_name.as_str(),
                    "InventoryComponent" | "HealthComponent"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        for component in &mut components {
            for property in &mut component.properties {
                if component.class_name == "HealthComponent" && property.name == "Health" {
                    if let Some(runtime) = world.runtime_health.get(&world.player_entity) {
                        property.value = serde_json::json!(runtime.health);
                    }
                } else if component.class_name == "InventoryComponent" {
                    if let Some(ammo) = &world.ammo.inventory {
                        let channel = match property.name.as_str() {
                            "Paint" => Some(0),
                            "Thinner" => Some(1),
                            _ => None,
                        };
                        if let Some(channel) = channel {
                            property.value = serde_json::json!(ammo.amounts[channel]);
                        }
                    }
                }
            }
        }
        let mut collectibles = world.collectibles.clone();
        collectibles.notifications.clear();
        let mut abilities = world.player_abilities.clone();
        for ability in abilities.values_mut() {
            // Temporary cutscene/ability disablement belongs to the old world.
            ability.disabled = false;
        }
        let mut sketch_registry=world.player_ability_registry.clone();
        for sketch in &mut sketch_registry.sketches {
            sketch.ownership.disabled=false;
        }
        let scripts = world.scripts.take_for_transition()?;
        Ok(Self {
            countdown: world.countdown.clone(),
            input_presentation: world.input_presentation.clone(),
            scripts: ScriptTransfer(Arc::new(Mutex::new(Some(scripts)))),
            source_player: world.player_entity,
            source_level: world.report.level.clone(),
            globals: world.globals.clone(),
            quests: world.quests.clone(),
            options: world.game_options,
            instances: world.instance_changes.clone(),
            extras: world.extras.clone(),
            collectibles,
            tracking: world.playstyle_tracking.clone(),
            player: Player {
                components,
                ammo: world.ammo.inventory.clone(),
                ammo_settings: world.ammo.settings,
                abilities,
                sketch_registry,
            },
        })
    }
    pub fn install(self, world: &mut World) -> anyhow::Result<()> {
        let mut scripts = self
            .scripts
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Lua session transfer lock poisoned"))?
            .take()
            .ok_or_else(|| anyhow::anyhow!("Lua session transfer was already consumed"))?;
        let report =
            scripts.adopt_world(&world.scripts, self.source_player, world.player_entity)?;
        world.scripts = scripts;
        world.report.geometry.push(report);
        world.globals = self.globals;
        world.countdown = self.countdown;
        world.input_presentation = self.input_presentation;
        world.quests = self.quests;
        world.game_options = self.options;
        world.instance_changes = self.instances;
        world.extras = self.extras;
        world.collectibles = self.collectibles;
        world.playstyle_tracking = self.tracking;
        world.pending_player_transfer = Some(self.player);
        world.report.geometry.push(serde_json::json!({
            "kind":"level_session_transfer", "source_level":self.source_level,
            "destination_level":world.report.level, "globals":world.globals.names_len(),
            "quests":world.quests.summary(), "collectibles":world.collectibles.summary(),
            "scope":"live singleton/player handoff; disk-save and checkpoint restore remain separate"
        }));
        Ok(())
    }
}
impl World {
    /// Called after component construction and before any destination Lua module
    /// or map-load callback can query player state. Old physics/timer handles
    /// never cross the world boundary.
    pub(super) fn restore_transition_player(&mut self) -> anyhow::Result<()> {
        let Some(player) = self.pending_player_transfer.take() else {
            return Ok(());
        };
        for component in player.components {
            let destination = self.entities[self.player_entity]
                .components
                .iter_mut()
                .find(|candidate| candidate.class_name == component.class_name)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Destination player lacks transferred {}",
                        component.class_name
                    )
                })?;
            // The destination owns its component IDs and inheritance links.
            // Only the persistent player's property values cross this boundary.
            destination.properties = component.properties;
            match destination.class_name.as_str() {
                "InventoryComponent" => {
                    // Validate live values without replacing the destination's
                    // immutable import snapshot with the travelling player.
                    inventory_state::Authored::from_component(destination)?;
                }
                "HealthComponent" => {
                    let health = health::Authored::from_component(destination)?;
                    self.runtime_health
                        .insert(self.player_entity, health::Runtime::new(&health));
                    self.authored_health.insert(self.player_entity, health);
                }
                _ => unreachable!(),
            }
        }
        self.ammo.inventory = player.ammo;
        self.ammo.settings = player.ammo_settings;
        self.player_abilities = player.abilities;
        self.player_ability_registry = player.sketch_registry;
        self.report.geometry.push(serde_json::json!({
            "kind":"level_session_player_restored",
            "before_destination_scripts":true,
            "inventory":self.player_inventory_save_state().ok(),
            "health":self.runtime_health.get(&self.player_entity).map(|state| state.health),
            "ammo":self.ammo.inventory, "abilities":self.player_abilities,
            "sketch_abilities":self.player_ability_registry,
        }));
        Ok(())
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate_player_handoff()->anyhow::Result<()> {
    use anyhow::{Context,ensure};
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let root=std::path::Path::new(&root);
    let ctx=egui::Context::default();
    let mut world=assets::load(root,"Levels/GV_ZoneA_Start.level",&ctx)?;
    let baseline=world.authored_inventory[&world.player_entity].clone();
    world.set_inventory_float(world.player_entity,"ETicket",0,77.75)?;
    world.set_inventory_float(world.player_entity,"Paint",0,6.25)?;
    world.player_abilities.get_mut("Brush").context("Missing Brush")?.give("Paint",2);
    let sketch=world.player_ability_registry.sketches.first_mut().context("No registered sketches")?;
    sketch.ownership.give("Paint",2);
    let expected=serde_json::to_value(&world.player_ability_registry)?;
    ensure!(world.authored_inventory[&world.player_entity]==baseline,"Source import snapshot changed");
    for destination in ["Levels/GV_ZoneB_Start.level","Levels/GV_ZoneA_Start.level"] {
        let session=Session::capture(&mut world)?;
        drop(world);
        world=assets::load_with_score_state(root,destination,&ctx,None,None,Default::default(),Default::default(),score_state::InitialState{session:Some(session),..Default::default()})?;
        ensure!(serde_json::to_value(&world.player_ability_registry)?==expected,"Sketch registration/ownership/upgrades lost on transition");
        ensure!(world.player_abilities["Brush"].upgrades[0]==2,"Base ability upgrade lost");
        ensure!(world.inventory_float(world.player_entity,"ETicket",0)?==77.75,"Live ticket state lost");
        ensure!(world.inventory_float(world.player_entity,"Paint",0)?==6.25,"Live ammo lost");
        ensure!(world.authored_inventory[&world.player_entity].floats.get("ETicket")==baseline.floats.get("ETicket"),"Destination import snapshot overwritten by live inventory");
        world.run_callback_fixture("assert(GetTicketCount(nil)==77); assert(GetPropertyFloat(GetPlayer(),'ETicket')==77.75)")?;
    }
    println!("PASS: GV A -> B -> A player handoff preserves sketch registry/ownership/upgrades, base upgrade, live tickets/ammo and immutable imported inventory");
    Ok(())
}
