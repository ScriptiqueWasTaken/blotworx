//! Actor frame publication190834. This is not actor registration,
//! service/attribute construction or behavior scheduling.
use super::assets::World;
use anyhow::Result;

///190a8c obtains height through the same29680c profile getter used by the
/// character body.190834 publishes the gameplay entity translation with
/// half that height added toY using one single-precision fused multiply-add.
pub(super) fn position(translation: [f32; 3], height: f32) -> [f32; 3] {
    [
        translation[0],
        height.mul_add(0.5, translation[1]),
        translation[2],
    ]
}
#[derive(Debug, serde::Serialize)]
pub(super) struct Frame {
    pub angles: [f32; 3],
    pub forward: [f32; 3],
    pub speed: f32,
}
///46a79c/46a6d4 convert the source axis to degrees and reconstruct it.
/// Preserve this round trip: simply copying/normalizing the axis differs even
/// at cardinal headings. DOL800249d4 returns its canonical NaN for asin's
/// out-of-domain branch; vertical unit axes can reach it after native sqrt.
pub(super) fn frame(axis: [f32; 3], velocity: [f32; 3]) -> Result<Frame> {
    anyhow::ensure!(
        axis.iter().chain(velocity.iter()).all(|v| v.is_finite()),
        "Nonfinite actor frame source"
    );
    let radians = f32::from_bits(0x3c8efa35);
    let angles = super::native_direction::angles(axis,||Ok([0.;3]))?;
    let forward = if angles[0].is_nan() {
        [f32::from_bits(0x7fffffff); 3]
    } else {
        let (sy, cy) = super::native_angle::sin_cos_medium((angles[1] * radians) as f64)?;
        let (sp, cp) = super::native_angle::sin_cos_medium((angles[0] * radians) as f64)?;
        [
            (sy as f32) * (cp as f32),
            sp as f32,
            (cy as f32) * (cp as f32),
        ]
    };
    //190914..19096c narrows frsqrte BEFORE the Newton step, unlike800071f0.
    let squared = velocity[0] * velocity[0] + velocity[2] * velocity[2];
    let speed = if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    };
    Ok(Frame {
        angles,
        forward,
        speed,
    })
}
impl World {
    pub(super) fn publish_ai_actor_position(&mut self, entity: usize) -> Result<()> {
        if !self.entity_scene_active(entity)
            || self.removed_entities.contains(&entity)
            || self.hidden_entities.contains(&entity)
        {
            return Ok(());
        }
        let Some(Ok(body)) = self.character_physics.get(&entity) else {
            return Ok(());
        };
        if !body.has_navigation_profile() {
            return Ok(());
        }
        let translation = self.entities[entity].transform[12..15].try_into()?;
        let point = position(translation, body.navigation_height);
        let rotation = self.entities[entity].runtime_rotation;
        let velocity = body.motion.current.linear_velocity();
        let frame = frame(
            [rotation[2], rotation[5], rotation[8]],
            [velocity[0], velocity[1], velocity[2]],
        )?;
        // Keep database identity with the result. Choosing an actor's native
        // profile/service/database remains separate from this position stage.
        let floor = self.ai_navigation.native_floor(point)?;
        let state = self.npc_relocations.entry(entity).or_default();
        state.navigation_position = Some(point);
        state.navigation_angles = Some(frame.angles.map(f32::to_bits));
        state.navigation_forward = Some(frame.forward.map(f32::to_bits));
        state.navigation_speed = Some(frame.speed);
        state.navigation_floor = floor;
        state.navigation_position_updates = state.navigation_position_updates.wrapping_add(1);
        Ok(())
    }
    pub(super) fn publish_ai_actor_positions(&mut self) -> Result<()> {
        let owners = self.character_physics.keys().copied().collect::<Vec<_>>();
        for entity in owners {
            self.publish_ai_actor_position(entity)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{ensure, Context};
    #[test]
    fn original_complete_frame_math_matches_float_words() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_actor_frame.json"
        ))?;
        for c in fixture["cases"].as_array().unwrap() {
            let vector = |key: &str| {
                std::array::from_fn(|i| f32::from_bits(c[key][i].as_u64().unwrap() as u32))
            };
            let actual = frame(vector("axis"), vector("velocity"))?;
            for (key, words) in [
                ("angles", actual.angles.map(f32::to_bits)),
                ("forward", actual.forward.map(f32::to_bits)),
            ] {
                let expected =
                    std::array::from_fn::<_, 3, _>(|i| c[key][i].as_u64().unwrap() as u32);
                assert_eq!(words, expected, "{key}: {c}");
            }
            assert_eq!(
                actual.speed.to_bits(),
                c["speed"].as_u64().unwrap() as u32,
                "speed: {c}"
            );
        }
        Ok(())
    }
    #[test]
    fn original_position_publication_matches_float_words() -> Result<()> {
        let f: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_actor_pose.json"
        ))?;
        for c in f["cases"].as_array().unwrap() {
            let translation = std::array::from_fn(|i| {
                f32::from_bits(c["translation"][i].as_u64().unwrap() as u32)
            });
            let height = f32::from_bits(c["height"].as_u64().unwrap() as u32);
            let actual = position(translation, height).map(f32::to_bits);
            let expected =
                std::array::from_fn::<_, 3, _>(|i| c["position"][i].as_u64().unwrap() as u32);
            assert_eq!(actual, expected, "{c}");
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_live_characters_use_actor_centers_for_navigation() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let actor = world
            .scripts
            .resolve_name("gremlinvillage_zonea.SpatterMelee 01")
            .context("Original placed Spatter missing")?;
        let raw: [f32; 3] = world.entities[actor].transform[12..15].try_into()?;
        ensure!(
            world.ai_navigation.native_floor(raw)?.is_none(),
            "Original regression's body-origin miss changed"
        );
        world.publish_ai_actor_position(actor)?;
        let state = &world.npc_relocations[&actor];
        let point = state
            .navigation_position
            .context("Live actor position not published")?;
        ensure!(
            state.navigation_floor.is_some(),
            "Native actor-center point still misses the original mesh"
        );
        ensure!(
            point[0] == raw[0] && point[2] == raw[2],
            "Publication changed horizontal coordinates"
        );
        let before = state.navigation_position_updates;
        let scene = world.entities[actor].scene.clone();
        world.scene_resources.deactivated(&scene);
        world.publish_ai_actor_positions()?;
        ensure!(
            world.npc_relocations[&actor].navigation_position_updates == before,
            "Inactive scene updated an actor position"
        );
        world.scene_resources.published(&scene);
        world.publish_ai_actor_positions()?;
        ensure!(
            world.npc_relocations[&actor].navigation_position_updates > before,
            "Reactivated actor did not resume position publication"
        );
        Ok(())
    }
}
