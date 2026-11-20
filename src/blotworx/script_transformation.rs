//! Native SetTransformation26b3e0: position and normalized facing direction.
//! Complete controller/AI/spline resets and native event scheduling remain open.
use anyhow::{ensure, Context, Result};
use mlua::{Lua, MultiValue, Value};
impl super::assets::World {
    /// REL26bf78 ->29731c changes the transform rotation and resets the
    /// bound motion rotation cache. It does not detach or relocate the owner.
    pub(super) fn facing_lua(&mut self,lua:&Lua,mut args:MultiValue)->mlua::Result<MultiValue> {
        if args.len()!=2 {self.report.warnings.push("SetFacing: expected entity and vector; preserving native stack order".into());}
        let value=args.pop_back().unwrap_or(Value::Nil);
        let Value::UserData(value)=value else {return Err(mlua::Error::external("SetFacing requires native vector4 userdata"));};
        let direction=(*value.borrow::<super::lua_vector::Vector4>()?).normalized3()?;
        let Some(owner)=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(Value::Nil))?
            .filter(|i|!self.removed_entities.contains(i)) else {return Ok(MultiValue::new());};
        if !self.entities[owner].components.iter().any(|c|c.class_name=="JPSTransformationComponent") {return Ok(MultiValue::new());}
        if owner==self.player_entity && !self.script_player_available {return Err(mlua::Error::external("SetFacing requires the live player scope"));}
        let rotation=super::facing_rotation::from_direction(direction.0).map_err(mlua::Error::external)?;
        let entity=&mut self.entities[owner];entity.runtime_rotation=rotation;
        entity.transform=super::render_transform::from_entity(rotation,entity.transform[12..15].try_into().unwrap(),entity.authored_scale);
        for draw in entity.draws.clone() {self.draws[draw].trans_mat=entity.transform;}
        super::rotator_runtime::accept_physics_pose(owner,&mut self.transform_hierarchy,&mut self.entities,&mut self.draws).map_err(mlua::Error::external)?;
        if owner==self.player_entity {
            let yaw=super::native_angle::atan2(rotation[2] as f64,rotation[8] as f64) as f32;
            self.script_player.accept_script_facing(yaw);
        } else if self.entities[owner].components.iter().any(|c|c.class_name=="BasicMotionControllerComponent") {
            self.npc_relocations.entry(owner).or_default().rotation_cache=[1.,0.,0.,0.,1.,0.,0.,0.,1.];
        }
        self.scene_motion_prepared_at=None;
        let limitation="SetFacing updates live rotation, retains position/parent and preserves player motion state. Complete native dirty propagation, physics publication timing, skeletal socket modes and player pitch parity remain unvalidated.";
        if !self.report.unsupported.iter().any(|v|v==limitation) {self.report.unsupported.push(limitation.into());}
        Ok(MultiValue::new())
    }
    /// REL267780 pops B then A,296cb0 computes native paired XYZ norm.
    pub(super) fn entity_distance_lua(&mut self,lua:&Lua,mut args:MultiValue)->mlua::Result<MultiValue> {
        if args.len()!=2 {self.report.warnings.push("GetDistanceBetweenEntities argument-count diagnostic; preserving native stack order".into());}
        let mut point=||->mlua::Result<[f32;3]> {
            let entity=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(Value::Nil))?
                .filter(|i|!self.removed_entities.contains(i))
                .ok_or_else(||mlua::Error::external("Distance query with missing owner requires native null-transform recovery"))?;
            Ok(self.entities[entity].transform[12..15].try_into().unwrap())
        };
        let b=point()?;let a=point()?;
        let value=super::lua_vector::Vector4([b[0]-a[0],b[1]-a[1],b[2]-a[2],0.]).length3()?;
        Ok(MultiValue::from_vec(vec![Value::Number(value as f64)]))
    }
    /// Shared model publication used by both native relocation entry points.
    pub(super) fn refresh_relocated_model(&mut self, entity: usize, time: f32) -> Result<()> {
        if let Some(animation) = self.model_animation.get_mut(&entity) {
            let asset = self.entities[entity]
                .model_asset
                .as_ref()
                .context("Relocated animated owner lacks model")?;
            animation.update(
                self.model_bounds
                    .get(asset)
                    .context("Relocated model hierarchy missing")?,
                time,
            )?;
        }
        if let Some(actor) = self.actors.get_mut(&entity) {
            if let Some(pose) = &actor.pose {
                let asset = self.entities[entity]
                    .model_asset
                    .as_ref()
                    .context("Relocated actor lacks model")?;
                let hierarchy = self
                    .model_bounds
                    .get(asset)
                    .context("Relocated actor hierarchy missing")?;
                actor.world = hierarchy.base_world_transforms_with_poses(None, &pose.transforms)?;
            }
        }
        Ok(())
    }
    pub(super) fn reset_relocated_lod(&mut self, entity: usize) -> Result<()> {
        self.lod_states
            .get_mut(&entity)
            .context("Relocation LOD state missing")?
            .reset();
        Ok(())
    }
    fn transformation_tree(&self, root: usize) -> Result<Vec<usize>> {
        let mut order = vec![root];
        let mut seen = std::collections::BTreeSet::from([root]);
        let mut cursor = 0;
        while cursor < order.len() {
            let parent = order[cursor];
            cursor += 1;
            for (&child, local) in &self.transform_hierarchy {
                if local.parent == parent && !self.removed_entities.contains(&child) {
                    ensure!(seen.insert(child), "Cyclic transformation descendant graph");
                    order.push(child);
                }
            }
        }
        Ok(order)
    }
    fn publish_static_entity_pose(&mut self, entity: usize) -> Result<()> {
        let e = &self.entities[entity];
        let initial = e.initial_physics_transform();
        let next = e.physics_transform();
        let identity =
            super::native_hk_transform::from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [0.; 3]);
        let inverse = super::native_hk_transform::relative(&initial, &identity);
        let delta = super::native_hk_transform::compose(&next, &inverse);
        let q = super::native_hk_transform::rotation_quaternion(&delta)?;
        let owners = e
            .collision_parts
            .clone()
            .map(|p| self.report.collision_parts[p].owner)
            .collect::<Vec<_>>();
        for owner in owners {
            self.collision
                .set_owner_transform(owner, delta[12..15].try_into().unwrap(), q)?;
        }
        Ok(())
    }
    pub(super) fn transformation_lua(
        &mut self,
        lua: &Lua,
        mut args: MultiValue,
    ) -> mlua::Result<MultiValue> {
        if args.len() < 3 {
            return Err(mlua::Error::external(
                "SetTransformation requires entity, position and direction",
            ));
        }
        let vector = |value: Value| -> mlua::Result<super::lua_vector::Vector4> {
            let Value::UserData(value) = value else {
                return Err(mlua::Error::external(
                    "Transformation vectors must use native vector4 userdata",
                ));
            };
            let value = *value.borrow::<super::lua_vector::Vector4>()?;
            Ok(value)
        };
        let direction = vector(args.pop_back().unwrap())?.normalized3()?;
        let position = vector(args.pop_back().unwrap())?.0;
        let Some(entity) = self
            .scripts
            .resolve_native_entity_argument(lua, args.pop_back().unwrap())?
            .filter(|i| !self.removed_entities.contains(i))
        else {
            return Ok(MultiValue::new());
        };
        let rotation =
            super::facing_rotation::from_direction(direction.0).map_err(mlua::Error::external)?;
        if !position.iter().all(|v| v.is_finite()) {
            return Err(mlua::Error::external("Nonfinite transformation position"));
        }
        if entity == self.player_entity {
            self.relocate_player_pose(position[..3].try_into().unwrap(), rotation, true)
                .map_err(mlua::Error::external)?;
            self.script_transformations = self.script_transformations.wrapping_add(1);
            return Ok(MultiValue::new());
        }
        if self.entities[entity].components.iter().any(|component|component.class_name=="PhysicsRigidBodyCharacterComponent") {
            self.transform_npc_pose(entity,position,rotation).map_err(mlua::Error::external)?;
            self.script_transformations=self.script_transformations.wrapping_add(1);
            let limitation="NPC SetTransformation shares the ordered MoveToEntity pose/body/AI/attachment/LOD path with reset enabled and no ground probe. Native controller support/animation reset, assigned splines, phantom interfaces and full phase parity remain incomplete.";
            if !self.report.unsupported.iter().any(|entry|entry==limitation) {self.report.unsupported.push(limitation.into());}
            return Ok(MultiValue::new());
        }
        let tree = self
            .transformation_tree(entity)
            .map_err(mlua::Error::external)?;
        // Reject known missing callbacks BEFORE detaching/mutating any owner.
        for &index in &tree {
            let e = &self.entities[index];
            if e.jigsaw_asset.is_some() {
                return Err(mlua::Error::external(
                    "SetTransformation jigsaw body publication is not connected",
                ));
            }
            if e.components.iter().any(|c| c.class_name == "LODComponent")
                && !self.lod_states.contains_key(&index)
            {
                return Err(mlua::Error::external("SetTransformation LOD state missing"));
            }
            for c in &e.components {
                if matches!(
                    c.class_name.as_str(),
                    "BasicAIControllerComponent"
                        | "BasicMotionControllerComponent"
                        | "MickeyPlayerControllerComponent"
                        | "MickeyPlayerMotionControllerComponent"
                        | "PhysicsRigidBodyCharacterComponent"
                        | "PhysicsCharacterControllerComponent"
                        | "PhysicsPhantomComponent"
                        | "PhysicsRigidBodyPhantomShapeComponent"
                        | "SplineFollowerComponent"
                        | "SplineTrackerComponent"
                ) {
                    return Err(mlua::Error::external(format!(
                        "SetTransformation native {} reset/publication is not connected",
                        c.class_name
                    )));
                }
            }
        }
        self.change_transform_parent(entity, None)
            .map_err(mlua::Error::external)?;
        let e = &mut self.entities[entity];
        e.runtime_rotation = rotation;
        e.transform = super::render_transform::from_entity(
            rotation,
            position[..3].try_into().unwrap(),
            e.authored_scale,
        );
        for draw in e.draws.clone() {
            self.draws[draw].trans_mat = e.transform;
        }
        super::rotator_runtime::propagate(
            entity,
            &self.transform_hierarchy,
            &mut self.entities,
            &mut self.draws,
        )
        .map_err(mlua::Error::external)?;
        for &child in tree.iter().skip(1) {
            self.update_skeletal_children_range_strict(child..child + 1)
                .map_err(mlua::Error::external)?;
        }
        for index in tree {
            self.publish_static_entity_pose(index)
                .map_err(mlua::Error::external)?;
            self.force_dynamic_entity_pose(index)
                .map_err(mlua::Error::external)?;
        }
        // REL29866c refreshes the root model before resetting LOD timing.
        self.refresh_relocated_model(entity, self.animation_time)
            .map_err(mlua::Error::external)?;
        if self.entities[entity]
            .components
            .iter()
            .any(|c| c.class_name == "LODComponent")
        {
            self.reset_relocated_lod(entity)
                .map_err(mlua::Error::external)?;
            self.script_transform_lod_resets = self.script_transform_lod_resets.wrapping_add(1);
        }
        self.scene_motion_prepared_at = None;
        self.script_transformations = self.script_transformations.wrapping_add(1);
        let limitation="SetTransformation publishes admitted scene/model/static/dynamic poses and descendants using native direction arithmetic. AI/controller/character/phantom/spline resets, complete animation-manager callbacks and native phase ordering remain incomplete.";
        if !self.report.unsupported.iter().any(|s| s == limitation) {
            self.report.unsupported.push(limitation.into());
        }
        Ok(MultiValue::new())
    }
}
