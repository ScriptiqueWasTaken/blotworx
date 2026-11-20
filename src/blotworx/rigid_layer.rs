//! Collision Layer property hook37c510 ->37eb30 ->virtual3b4/37ec20.
//! This is distinct from temporary layer override37eeec and bulk37edf0.
use super::{assets::World, body_filter, physics::Collision, physics_filter::GameFilter};
use anyhow::{ensure, Context, Result};

#[derive(Default, Debug, serde::Serialize)]
pub struct State {
    pub requests: u64,
    pub rebuilds: u64,
    pub body_writes: u64,
    pub refreshes: u64,
    pub removed_box3d_pairs: u64,
    pub last: Option<Change>,
}
#[derive(Debug, serde::Serialize)]
pub struct Change {
    pub entity: usize,
    pub component: usize,
    pub old: i32,
    pub new: i32,
    pub filter_before: Option<u32>,
    pub filter_after: Option<u32>,
}
///37eb70..98 is checked AFTER property publication, even for equal values.
pub fn needs_rebuild(old: i32, new: i32, initialization_flags: u8) -> bool {
    initialization_flags & 8 == 0 && old != new
}

struct Body<'a> {
    collision: &'a mut Collision,
    filter: &'a GameFilter,
    shape: usize,
    owner: usize,
    flags: (u8, u8),
    state: &'a mut State,
}
impl body_filter::Backend for Body<'_> {
    fn system(&mut self) -> Result<Option<usize>> {
        Ok(Some(self.shape + 1))
    }
    fn body_count(&mut self, system: usize) -> Result<usize> {
        ensure!(system == self.shape + 1, "Unknown character system");
        Ok(1)
    }
    fn body(&mut self, system: usize, index: usize) -> Result<usize> {
        ensure!(
            system == self.shape + 1 && index == 0,
            "Unknown character body"
        );
        Ok(self.shape + 1)
    }
    fn filter(&mut self, body: usize) -> Result<u32> {
        ensure!(body == self.shape + 1, "Unknown filter body");
        self.collision.simulation_filter_word(self.shape)
    }
    fn write(&mut self, body: usize, value: u32) -> Result<()> {
        ensure!(body == self.shape + 1, "Unknown filter body");
        self.collision.set_simulation_filter(self.shape, value)?;
        self.state.body_writes = self.state.body_writes.wrapping_add(1);
        self.state.last.as_mut().unwrap().filter_after = Some(value);
        Ok(())
    }
    fn world(&mut self, body: usize) -> Result<Option<usize>> {
        ensure!(body == self.shape + 1, "Unknown filter body");
        Ok(self.collision.owner_in_world(self.owner)?.then_some(1))
    }
    fn refresh(&mut self, world: usize, body: usize, mode: u32, collections: u32) -> Result<()> {
        ensure!(
            (world, body, mode, collections) == (1, self.shape + 1, 0, 1),
            "Unsupported body filter refresh operation"
        );
        let removed = self
            .collision
            .refresh_simulation_filter(self.shape, self.filter)?;
        self.state.removed_box3d_pairs =
            self.state.removed_box3d_pairs.wrapping_add(removed as u64);
        self.state.refreshes = self.state.refreshes.wrapping_add(1);
        Ok(())
    }
    fn real(&mut self) -> Result<u8> {
        Ok(self.flags.1)
    }
    fn painted(&mut self) -> Result<u8> {
        Ok(self.flags.0)
    }
}
impl World {
    /// Same scalar-layer callback for admitted static bodies. Keep body group,
    /// subgroup and authored Real/Painted flags; refresh existing solver pairs.
    pub(super) fn set_static_collision_layer(
        &mut self,
        entity: usize,
        component: usize,
        field: usize,
        value: i32,
    ) -> Result<()> {
        ensure!(
            !self.removed_entities.contains(&entity),
            "Collision-layer owner was retired"
        );
        let c = &mut self.entities[entity].components[component];
        ensure!(
            c.class_name == "PhysicsRigidBodyStaticComponent",
            "Expected static layer owner"
        );
        let boolean = |name: &str| -> Result<u8> {
            Ok(u8::from(
                c.properties
                    .iter()
                    .find(|p| p.name == name)
                    .and_then(|p| p.value.as_bool())
                    .with_context(|| format!("Static filter lacks {name}"))?,
            ))
        };
        let real = boolean("Is Real")?;
        let painted = boolean("Is Painted")?;
        let p = &mut c.properties[field];
        ensure!(
            p.name == "Collision Layer" && p.class_name == "Integer" && !p.value.is_array(),
            "Invalid static layer field"
        );
        let old = i32::try_from(p.value.as_i64().context("Invalid previous static layer")?)?;
        p.value = serde_json::json!(value);
        self.rigid_layer_updates.requests += 1;
        self.rigid_layer_updates.last = Some(Change {
            entity,
            component,
            old,
            new: value,
            filter_before: None,
            filter_after: None,
        });
        if !needs_rebuild(old, value, 0) {
            return Ok(());
        }
        self.rigid_layer_updates.rebuilds += 1;
        let owners = self.entities[entity]
            .collision_parts
            .clone()
            .map(|part| self.report.collision_parts[part].owner)
            .collect::<std::collections::BTreeSet<_>>();
        for owner in owners {
            for shape in self.collision.owner_shapes(owner).collect::<Vec<_>>() {
                let previous = self.collision.simulation_filter_word(shape)?;
                let next = body_filter::change_layer(previous, value as u32, painted, real);
                self.collision.set_simulation_filter(shape, next)?;
                self.rigid_layer_updates.body_writes += 1;
                let last = self.rigid_layer_updates.last.as_mut().unwrap();
                last.filter_before.get_or_insert(previous);
                last.filter_after = Some(next);
                if self.collision.owner_in_world(owner)? {
                    self.rigid_layer_updates.removed_box3d_pairs += self
                        .collision
                        .refresh_simulation_filter(shape, &self.dynamic_runtime_filter)?
                        as u64;
                    self.rigid_layer_updates.refreshes += 1;
                }
            }
        }
        Ok(())
    }
    /// Live scalar property storage is published before the required callback.
    /// Initialization/restore-phase property admission remains separate.
    pub(super) fn set_character_collision_layer(
        &mut self,
        entity: usize,
        component: usize,
        field: usize,
        value: i32,
    ) -> Result<()> {
        ensure!(
            !self.removed_entities.contains(&entity),
            "Collision-layer owner was retired"
        );
        let c = self
            .entities
            .get_mut(entity)
            .context("Unknown collision-layer entity")?
            .components
            .get_mut(component)
            .context("Unknown collision-layer component")?;
        ensure!(
            c.class_name == "PhysicsRigidBodyCharacterComponent",
            "Unimplemented collision-layer component callback"
        );
        let property = c
            .properties
            .get_mut(field)
            .context("Collision Layer property is missing")?;
        ensure!(
            property.name == "Collision Layer"
                && property.class_name == "Integer"
                && !property.value.is_array(),
            "Invalid collision-layer scalar property"
        );
        let old = i32::try_from(
            property
                .value
                .as_i64()
                .context("Invalid previous collision layer")?,
        )?;
        property.value = serde_json::json!(value);
        self.rigid_layer_updates.requests = self.rigid_layer_updates.requests.wrapping_add(1);
        self.rigid_layer_updates.last = Some(Change {
            entity,
            component,
            old,
            new: value,
            filter_before: None,
            filter_after: None,
        });
        // These World calls occur after component-range publication. The
        // construction guard is no longer set; no initialization callback is
        // inferred for an unsupported/unadmitted body below.
        if !needs_rebuild(old, value, 0) {
            return Ok(());
        }
        self.rigid_layer_updates.rebuilds = self.rigid_layer_updates.rebuilds.wrapping_add(1);
        let (shape, owner) = if entity == self.player_entity {
            let shape = self
                .collision
                .player_body
                .context("Player collision layer requires its admitted body")?;
            (
                shape,
                self.collision
                    .shape_owner(shape)
                    .context("Player filter owner is absent")?,
            )
        } else {
            let body = self
                .character_physics
                .get(&entity)
                .context("Character collision layer has no initialized body")?
                .as_ref()
                .map_err(|e| {
                    anyhow::anyhow!("Character collision-layer body is unavailable: {e}")
                })?;
            (body.shape, body.owner)
        };
        let previous = self.collision.simulation_filter_word(shape)?;
        self.rigid_layer_updates
            .last
            .as_mut()
            .unwrap()
            .filter_before = Some(previous);
        self.rigid_layer_updates.last.as_mut().unwrap().filter_after = Some(previous);
        let c = &self.entities[entity].components[component];
        let boolean = |name: &str| -> Result<u8> {
            let p = c
                .properties
                .iter()
                .find(|p| p.name == name)
                .with_context(|| format!("Character filter lacks {name}"))?;
            Ok(u8::from(
                p.value
                    .as_bool()
                    .with_context(|| format!("Invalid character {name}"))?,
            ))
        };
        // Virtual37c reads Collision Layer, then Real, then Painted. Real
        // forces the painted bit in pack; it does not rewrite its property.
        let real = boolean("Is Real")?;
        let painted = boolean("Is Painted")?;
        let fields = body_filter::Fields {
            layer: value as u32,
            group: previous >> 17,
            sub_a: (previous >> 5) & 31,
            sub_b: (previous >> 10) & 31,
            painted,
            real,
        };
        let mut backend = Body {
            collision: &mut self.collision,
            filter: &self.dynamic_runtime_filter,
            shape,
            owner,
            flags: (painted, real),
            state: &mut self.rigid_layer_updates,
        };
        body_filter::set_all(fields, &mut backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn property_guard_requires_change_and_finished_initialization() -> Result<()> {
        for flags in 0..=255 {
            assert!(!needs_rebuild(4, 4, flags));
            assert_eq!(needs_rebuild(4, 7, flags), flags & 8 == 0);
            assert_eq!(needs_rebuild(i32::MIN, i32::MAX, flags), flags & 8 == 0);
        }
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/rigid_layer_property.json"
        ))?;
        let cases = fixture["cases"]
            .as_array()
            .context("Missing original layer property cases")?;
        for case in cases {
            let old = i32::try_from(case["old"].as_i64().unwrap())?;
            let new = i32::try_from(case["new"].as_i64().unwrap())?;
            assert_eq!(
                needs_rebuild(old, new, case["flags"].as_u64().unwrap() as u8),
                case["result"]["rebuild"].as_bool().unwrap()
            );
            assert_eq!(
                case["result"]["stored"].as_u64().unwrap(),
                new as u32 as u64
            );
        }
        assert_eq!(cases.len(), 20);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_character_layer_property_updates_live_and_detached_body_without_pose_reset(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .ai_brains
            .iter()
            .find(|(_, b)| b.ai_type.starts_with("Gremlin") && !b.scripted_idle.is_empty())
            .map(|(&entity, _)| entity)
            .context("Entrance Gremlin is absent")?;
        let name = world.entities[owner].name.clone();
        let component = world.entities[owner]
            .components
            .iter()
            .position(|c| c.class_name == "PhysicsRigidBodyCharacterComponent")
            .unwrap();
        let field = world.entities[owner].components[component]
            .properties
            .iter()
            .position(|p| p.name == "Collision Layer")
            .unwrap();
        assert_eq!(
            world.entities[owner].components[component].properties[field].value,
            4
        );
        let body = world.character_physics[&owner].as_ref().unwrap();
        let (shape, body_owner) = (body.shape, body.owner);
        let before = world.collision.simulation_pose(shape)?;
        let old_filter = world.collision.simulation_filter_word(shape)?;
        let hidden = world.hidden_entities.clone();
        let random = world.game_random;
        world.run_callback_fixture(&format!(
            "assert(select('#',SetPropertyInt({name:?},'Collision Layer','7.9'))==0)"
        ))?;
        let next = world.collision.simulation_filter_word(shape)?;
        assert_eq!(next & 31, 7);
        assert_eq!(next >> 17, old_filter >> 17);
        assert_eq!(next & 0x7fe0, old_filter & 0x7fe0);
        assert_eq!(
            (
                world.rigid_layer_updates.rebuilds,
                world.rigid_layer_updates.body_writes,
                world.rigid_layer_updates.refreshes
            ),
            (1, 1, 1)
        );
        assert_eq!(
            world.entities[owner].components[component].properties[field].value,
            7
        );
        let after = world.collision.simulation_pose(shape)?;
        assert_eq!(before.position, after.position);
        assert_eq!(before.orientation, after.orientation);
        assert_eq!(before.linear, after.linear);
        assert_eq!(before.angular, after.angular);
        assert_eq!(world.hidden_entities, hidden);
        assert_eq!(world.game_random, random);
        world.run_callback_fixture(&format!("SetPropertyInt({name:?},'Collision Layer',7); SetPropertyInt({name:?},'Collision Layer',4,1); SetPropertyString({name:?},'Collision Layer','4')"))?;
        assert_eq!(
            (
                world.rigid_layer_updates.requests,
                world.rigid_layer_updates.rebuilds,
                world.rigid_layer_updates.body_writes,
                world.rigid_layer_updates.refreshes
            ),
            (2, 1, 1, 1)
        );
        world.set_entity_hidden(owner, true)?;
        world.run_callback_fixture(&format!("SetPropertyInt({name:?},'Collision Layer',4)"))?;
        assert_eq!(world.collision.simulation_filter_word(shape)? & 31, 4);
        assert!(!world.collision.owner_in_world(body_owner)?);
        assert_eq!(
            world.rigid_layer_updates.refreshes, 1,
            "Detached body incorrectly invoked world refresh"
        );
        world.set_entity_hidden(owner, false)?;
        assert!(world.collision.owner_in_world(body_owner)?);
        // Native OR packing takes the full signed32 word; it does not clamp
        // scripting input to the five category bits.
        world.run_callback_fixture(&format!("SetPropertyInt({name:?},'Collision Layer',-1)"))?;
        assert_eq!(world.collision.simulation_filter_word(shape)?, u32::MAX);
        assert_eq!(
            world.entities[owner].components[component].properties[field].value,
            -1
        );
        // A required engine binding failure retains the preceding property
        // publication, rather than silently reverting a native partial write.
        let retained = world.character_physics.remove(&owner).unwrap();
        let error = world
            .run_callback_fixture(&format!("SetPropertyInt({name:?},'Collision Layer',4)"))
            .unwrap_err();
        assert!(format!("{error:#}").contains("no initialized body"));
        assert_eq!(
            world.entities[owner].components[component].properties[field].value,
            4
        );
        world.character_physics.insert(owner, retained);
        world.retire_entity(owner)?;
        let calls = world.rigid_layer_updates.requests;
        world.run_callback_fixture(&format!("SetPropertyInt({name:?},'Collision Layer',7)"))?;
        assert_eq!(world.rigid_layer_updates.requests, calls);
        Ok(())
    }
}
