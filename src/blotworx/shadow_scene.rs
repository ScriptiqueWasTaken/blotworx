//! ShadowComponent model/actor presentation. The native mask compositor is
//! separate from Mickey's primary-depth mesh and silhouette registrations.
use super::assets::World;
use crate::render_support::volume_shadow::Mesh;
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Debug)]
struct Definition {
    scale: [f32; 3],
    authored_scale: [f32; 3],
    bone: String,
    rotate: bool,
    cast_layer: i32,
}
#[derive(Default)]
pub struct Runtime {
    definitions: BTreeMap<usize, Definition>,
    script_scale_updates: u64,
    pub mesh: Option<Arc<Mesh>>,
    pub matrices: Vec<[f32; 16]>,
    pub owners: Vec<usize>,
    pub errors: BTreeMap<usize, String>,
}
impl Runtime {
    /// REL3b5c78 resolves the component, then 3b51d8 stores XYZ. Entities
    /// without a shadow component are deliberately unchanged by the native API.
    pub fn set_scale(&mut self, entity: usize, scale: [f32; 3]) {
        if let Some(definition) = self.definitions.get_mut(&entity) {
            definition.scale = scale;
            self.script_scale_updates += 1;
        }
    }
    pub fn restore_scale(&mut self, entity: usize) {
        if let Some(definition) = self.definitions.get_mut(&entity) {
            definition.scale = definition.authored_scale;
            self.script_scale_updates += 1;
        }
    }
    pub fn admit(
        &mut self,
        index: usize,
        components: &[epic_mickey_lib_rs::scene_file::Component],
    ) -> Result<()> {
        for c in components
            .iter()
            .filter(|c| c.class_name == "ShadowComponent")
        {
            let get = |name: &str| {
                c.properties
                    .iter()
                    .find(|p| p.name == name)
                    .map(|p| &p.value)
            };
            let scale = get("ShadowScale")
                .map(|v| crate::render_support::scene::parse_point3(v).context("Invalid ShadowScale"))
                .transpose()?
                .unwrap_or([1.; 3]);
            ensure!(scale.iter().all(|v| v.is_finite()), "Nonfinite ShadowScale");
            let bone = get("ShadowBone")
                .map(|v| v.as_str().context("Invalid ShadowBone"))
                .transpose()?
                .unwrap_or("")
                .to_owned();
            let rotate = get("ShadowRotate")
                .map(|v| v.as_bool().context("Invalid ShadowRotate"))
                .transpose()?
                .unwrap_or(false);
            let cast_layer = i32::try_from(
                get("ShadowCastCollisionLayer")
                    .map(|v| v.as_i64().context("Invalid shadow collision layer"))
                    .transpose()?
                    .unwrap_or(0),
            )?;
            ensure!(
                self.definitions
                    .insert(
                        index,
                        Definition {
                            scale,
                            authored_scale: scale,
                            bone,
                            rotate,
                            cast_layer
                        }
                    )
                    .is_none(),
                "Multiple shadow components on entity{index}"
            );
        }
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"definitions":self.definitions.len(),"submitted":self.matrices.len(),"errors":self.errors,
            "owners":self.owners,"origins":self.matrices.iter().map(|m|[m[12],m[13],m[14]]).collect::<Vec<_>>(),
            "script_scale_updates":self.script_scale_updates,
            "zero_scale_definitions":self.definitions.values().filter(|definition|definition.scale==[0.;3]).count(),
            "collision_projection_connected":false,"native_trigonometry_validated":false,"original_frame_parity_validated":false})
    }
    pub fn refresh(&mut self, world: &World) -> Result<()> {
        if self.mesh.is_none() && !self.definitions.is_empty() {
            let g = super::shadow_volume::Geometry::retail(|a| {
                Ok(((a as f64).sin(), (a as f64).cos()))
            })?;
            self.mesh = Some(Arc::new(Mesh {
                positions: g.positions.to_vec(),
                indices: g.indices.iter().map(|&i| u32::from(i)).collect(),
            }));
        }
        self.matrices.clear();
        self.owners.clear();
        self.errors.clear();
        self.definitions
            .retain(|index, _| !world.removed_entities.contains(index));
        for (&index, cfg) in &self.definitions {
            let entity = &world.entities[index];
            //3b5950..3b5968: missing LOD admits; a present LOD's byte59
            //must be nonzero. This is separate from model/owner visibility.
            if world.lod_states.get(&index).is_some_and(|lod|lod.visible==0) {
                continue;
            }
            if cfg.scale == [0.; 3]
                || world.hidden_entities.contains(&index)
                || entity.draws.clone().all(|draw| {
                    world.hidden_entity_draws.contains(&draw) || world.removed_draws.contains(&draw)
                })
            {
                continue;
            }
            let result = (|| -> Result<[f32; 16]> {
                ensure!(
                    cfg.cast_layer == 0,
                    "Native shadow collision-layer projection is not yet connected"
                );
                let mut origin: [f32; 3] = entity.transform[12..15].try_into().unwrap();
                if entity
                    .components
                    .iter()
                    .any(|c| c.class_name == "AnimationComponent")
                {
                    ensure!(
                        world.authored_havok.contains(index),
                        "Shadow actor pose is unavailable"
                    );
                    if let Some(bone) = world.authored_havok.shadow_bone_pose(index, &cfg.bone)? {
                        let parent: super::native_transform_record::Transform =
                            std::array::from_fn(|i| {
                                if i < 9 {
                                    entity.runtime_rotation[i]
                                } else if i < 12 {
                                    origin[i - 9]
                                } else {
                                    entity.authored_scale
                                }
                            });
                        origin = super::native_transform_record::world_pose(
                            &parent,
                            bone[..3].try_into().unwrap(),
                            &[1., 0., 0., 0., 1., 0., 0., 0., 1.],
                        )
                        .0;
                    }
                } else {
                    ensure!(
                        cfg.bone.is_empty(),
                        "Named NiNode shadow reference is not yet connected"
                    );
                }
                let rotation = if cfg.rotate {
                    entity.runtime_rotation
                } else {
                    [1., 0., 0., 0., 1., 0., 0., 0., 1.]
                };
                let mut matrix = super::render_transform::from_entity(rotation, origin, 1.);
                for c in 0..3 {
                    for r in 0..3 {
                        matrix[c * 4 + r] *= cfg.scale[c];
                    }
                }
                ensure!(
                    matrix.iter().all(|v| v.is_finite()),
                    "Nonfinite shadow model transform"
                );
                Ok(matrix)
            })();
            match result {
                Ok(matrix) => {self.matrices.push(matrix);self.owners.push(index);},
                Err(error) => {
                    self.errors
                        .insert(index, format!("{}: {error:#}", entity.name));
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_npc_shadow_tracks_character_body_and_reports_its_owner()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
        world.enable_demo_story()?;
        world.advance_models(0.)?;
        world.refresh_shadow_volumes()?;
        let entity=world.scripts.resolve_name("gremlinvillage_zonea.SpatterMelee 02").context("Original Spatter missing")?;
        let before_index=world.shadow_volumes.owners.iter().position(|&owner|owner==entity).context("Original Spatter shadow not submitted")?;
        let before=world.shadow_volumes.matrices[before_index];
        let body=world.character_physics[&entity].as_ref().map_err(|e|anyhow::anyhow!("{e}"))?;
        let shape=body.shape;
        let pose=world.collision.simulation_pose(shape)?;
        let mut position=pose.position;position[0]+=7.;
        world.collision.set_shape_transform(shape,position,pose.orientation)?;
        world.publish_character_physics(super::super::STEP)?;
        world.refresh_shadow_volumes()?;
        let after_index=world.shadow_volumes.owners.iter().position(|&owner|owner==entity).context("Moved Spatter shadow disappeared")?;
        let after=world.shadow_volumes.matrices[after_index];
        ensure!((after[12]-before[12]-7.).abs()<0.00001,"Character shadow did not follow solver translation");
        ensure!(after[13]==before[13] && after[14]==before[14],"Horizontal translation changed other shadow coordinates");
        if let Some(lod)=world.lod_states.get_mut(&entity) {
            let visible=lod.visible;lod.visible=0;
            world.refresh_shadow_volumes()?;
            ensure!(!world.shadow_volumes.owners.contains(&entity),"LOD-invisible owner still submitted a shadow");
            world.lod_states.get_mut(&entity).unwrap().visible=visible;
            world.refresh_shadow_volumes()?;
            ensure!(world.shadow_volumes.owners.contains(&entity),"LOD visibility did not restore the caster");
        } else {anyhow::bail!("Original Spatter lacks LOD test coverage");}
        let casters=world.shadow_volumes.owners.iter().zip(&world.shadow_volumes.matrices).map(|(&owner,m)|serde_json::json!({"entity":owner,"name":world.entities[owner].name,"origin":[m[12],m[13],m[14]],"scene_active":world.entity_scene_active(owner),"hidden":world.hidden_entities.contains(&owner)})).collect::<Vec<_>>();
        std::fs::write("target/npc-shadow-owner-probe.json",serde_json::to_vec_pretty(&serde_json::json!({"moved_entity":entity,"before":before,"after":after,"casters":casters}))?)?;
        Ok(())
    }
}
impl World {
    pub fn refresh_shadow_volumes(&mut self) -> Result<()> {
        let mut runtime = std::mem::take(&mut self.shadow_volumes);
        let result = runtime.refresh(self);
        self.shadow_volumes = runtime;
        result
    }
}
