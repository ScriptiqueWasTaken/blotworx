//! Weapon damage directions, retail REL3da854..3daa14. The optional component
//! lookup at41098 is WeaponComponent (descriptor321ec), not PhysicsStimulus.
use super::assets::World;
use anyhow::{ensure, Context, Result};

struct ParentFrame {
    center: [f32; 3],
    forward: [f32; 3],
}

fn direction(
    mode: i32,
    target: [f32; 3],
    owner: impl FnOnce() -> Result<[f32; 3]>,
    parent: impl FnOnce() -> Result<ParentFrame>,
) -> Result<[f32; 3]> {
    ensure!(
        target.iter().all(|v| v.is_finite()),
        "Nonfinite stimulus target bound"
    );
    match mode {
        0 => Ok(super::script_stimulus::direction(owner()?, target, 0.)?.unwrap()),
        1 => Ok(super::script_stimulus::direction(parent()?.center, target, 0.)?.unwrap()),
        2 => Ok(super::script_stimulus::direction([0.; 3], parent()?.forward, 0.)?.unwrap()),
        // The native workspace is initialized to the target bound center
        // before the mode switch; out-of-range values leave it untouched.
        _ => Ok(target),
    }
}

impl World {
    pub(super) fn emitter_stimulus_direction(
        &self,
        emitter: usize,
        target: usize,
    ) -> Result<[f32; 3]> {
        let runtime = self
            .entities
            .get(emitter)
            .context("Missing stimulus emitter")?;
        let mode = runtime
            .components
            .iter()
            .find(|c| c.class_name == "WeaponComponent")
            .and_then(|c| {
                c.properties
                    .iter()
                    .find(|p| p.name == "Weapon Damage Direction")
            })
            .map(|p| {
                p.value
                    .as_i64()
                    .context("Invalid weapon damage direction")
                    .and_then(|v| i32::try_from(v).map_err(Into::into))
            })
            .transpose()?
            .unwrap_or(0);
        let target = self
            .entity_position_or_bound_center(target)?
            .context("Stimulus target was removed")?;
        direction(
            mode,
            target,
            || {
                self.entity_position_or_bound_center(emitter)?
                    .context("Stimulus emitter was removed")
            },
            || {
                //353828 returns the immediate transform parent, not the hierarchy root.
                // Authored Parent Entity is an import-time handle. SetParentEntity
                // can replace or detach it at runtime, including across scenes.
                let parent = self
                    .transform_hierarchy
                    .get(&emitter)
                    .context("Weapon direction requires an admitted live transform parent")?
                    .parent;
                let rotation = self
                    .entities
                    .get(parent)
                    .context("Unresolved weapon direction parent")?
                    .runtime_rotation;
                Ok(ParentFrame {
                    center: if mode == 1 {
                        self.entity_position_or_bound_center(parent)?
                            .context("Weapon direction parent was removed")?
                    } else {
                        [0.; 3]
                    },
                    //297110(axis=2) selects the third column of the world rotation.
                    forward: [rotation[2], rotation[5], rotation[8]],
                })
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_modes_choose_owner_immediate_parent_or_parent_forward() -> Result<()> {
        let parent = || {
            Ok(ParentFrame {
                center: [3., 0., 0.],
                forward: [0., 0., -2.],
            })
        };
        assert_eq!(
            direction(0, [3., 0., 4.], || Ok([0.; 3]), parent)?,
            [0.6, 0., 0.8]
        );
        //1fad0 uses frsqrte/frsp plus one Newton step, not host sqrt.
        // For these axis-aligned vectors the subsequent reciprocal/multiply
        // produces the next f32 above one. Keep exact native bits in the test.
        let unit = f32::from_bits(0x3f800001);
        assert_eq!(
            direction(
                1,
                [3., 0., 4.],
                || panic!("Owner must not be queried"),
                parent
            )?,
            [0., 0., unit]
        );
        assert_eq!(
            direction(
                2,
                [3., 0., 4.],
                || panic!("Owner must not be queried"),
                parent
            )?,
            [0., 0., -unit]
        );
        assert!(direction(1, [0.; 3], || Ok([0.; 3]), || anyhow::bail!("No parent")).is_err());
        Ok(())
    }
    #[test]
    fn small_vectors_and_unknown_modes_preserve_native_workspace() -> Result<()> {
        assert_eq!(
            direction(
                0,
                [0.00001, 0., 0.],
                || Ok([0.; 3]),
                || panic!("No parent needed")
            )?,
            [0.00001, 0., 0.]
        );
        for mode in [-1, 3, i32::MAX] {
            assert_eq!(
                direction(
                    mode,
                    [7., 8., 9.],
                    || panic!("No owner needed"),
                    || panic!("No parent needed")
                )?,
                [7., 8., 9.]
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_parented_weapons_use_their_parent_world_forward() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.enable_demo_story()?;
        let mut checked = 0;
        let mut selected = None;
        for (entity, runtime) in world.entities.iter().enumerate() {
            if !runtime.components.iter().any(|c| {
                c.class_name == "WeaponComponent"
                    && c.properties
                        .iter()
                        .any(|p| p.name == "Weapon Damage Direction" && p.value.as_i64() == Some(2))
            }) {
                continue;
            }
            let Some(parent) = runtime.transform_parent_id()? else {
                continue;
            };
            let parent = world
                .entities
                .iter()
                .find(|e| e.scene == runtime.scene && e.id == parent)
                .context("Weapon parent missing")?;
            let r = parent.runtime_rotation;
            let expected =
                super::super::script_stimulus::direction([0.; 3], [r[2], r[5], r[8]], 0.)?.unwrap();
            assert_eq!(
                world.emitter_stimulus_direction(entity, world.player_entity)?,
                expected,
                "{}",
                runtime.name
            );
            selected.get_or_insert(entity);
            checked += 1;
        }
        ensure!(
            checked > 0,
            "No original parent-forward weapon was exercised"
        );
        let emitter = selected.unwrap();
        let original = world.emitter_stimulus_direction(emitter, world.player_entity)?;
        // Choose a live root from another scene whose forward differs. This
        // catches both stale authored IDs and accidental scene-local lookup.
        let replacement = world
            .entities
            .iter()
            .enumerate()
            .find(|(index, e)| {
                e.scene != world.entities[emitter].scene
                    && !world.transform_hierarchy.contains_key(index)
                    && world.scripts.resolve_name(&e.name) == Some(*index)
                    && super::super::script_stimulus::direction(
                        [0.; 3],
                        [
                            e.runtime_rotation[2],
                            e.runtime_rotation[5],
                            e.runtime_rotation[8],
                        ],
                        0.,
                    )
                    .ok()
                    .flatten()
                    .is_some_and(|forward| forward != original)
            })
            .map(|(index, _)| index)
            .context("No distinct cross-scene parent for the original weapon fixture")?;
        let emitter_name = serde_json::to_string(&world.entities[emitter].name)?;
        let replacement_name = serde_json::to_string(&world.entities[replacement].name)?;
        world.run_callback_fixture(&format!(
            "SetParentEntity({emitter_name},{replacement_name})"
        ))?;
        assert_eq!(world.transform_hierarchy[&emitter].parent, replacement);
        let r = world.entities[replacement].runtime_rotation;
        let expected =
            super::super::script_stimulus::direction([0.; 3], [r[2], r[5], r[8]], 0.)?.unwrap();
        assert_eq!(
            world.emitter_stimulus_direction(emitter, world.player_entity)?,
            expected
        );
        assert_ne!(expected, original);
        world.run_callback_fixture(&format!("SetParentEntity({emitter_name},nil)"))?;
        assert!(!world.transform_hierarchy.contains_key(&emitter));
        assert!(
            world.entities[emitter].transform_parent_id()?.is_some(),
            "Fixture must retain the stale imported handle"
        );
        assert!(
            world
                .emitter_stimulus_direction(emitter, world.player_entity)
                .is_err(),
            "Detached weapon fell back to its stale authored parent"
        );
        Ok(())
    }
}
