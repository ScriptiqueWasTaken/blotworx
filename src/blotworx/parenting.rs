//! Native parent association bridge.3538a8/353dc4 and312bf4/312cc8.
//! Full dirty-byte scheduling, static/character parenting and socket mode parity remain open.
use anyhow::{Context,Result,ensure};
use epic_mickey_lib_rs::scene_file::Property;
use super::assets::World;
impl World {
    fn prepare_transform_children(&mut self,parent:usize)->Result<()> {
        // Initial constructors and streamed batches publish in entity order.
        // Preserve subsequent native append/swap-remove registration order.
        let mut imported=Vec::new();
        for child in 0..self.entities.len() {
            if child!=parent && !self.removed_entities.contains(&child)
                && self.current_transform_parent(child)?==Some(parent) {imported.push(child);}
        }
        let children=self.live_transform_children.entry(parent).or_default();
        for child in imported {if !children.contains(&child) {children.push(child);}}
        Ok(())
    }
    fn transform_membership(&mut self,parent:usize,child:usize,add:bool)->Result<()> {
        if !self.live_transform_children.contains_key(&parent) {self.prepare_transform_children(parent)?;}
        let children=self.live_transform_children.get_mut(&parent).unwrap();
        if add {
            // Native3536b8 suppresses duplicates and appends.
            if !children.contains(&child) {children.push(child);}
        } else if let Some(index)=children.iter().position(|&item|item==child) {
            // Native3537b0 fills a removed slot from the final element.
            children.swap_remove(index);
        }
        Ok(())
    }
    pub(super) fn detach_retiring_transform(&mut self,parent:usize)->Result<()> {
        if !self.entities[parent].components.iter().any(|c|c.class_name=="JPSTransformationComponent") {return Ok(());}
        self.prepare_transform_children(parent)?;
        let children=self.live_transform_children[&parent].clone();
        // Retail353380..353418 walks transform-owned handles backwards and
        // assigns null through3538a8; ChildComponent destruction is separate.
        for child in children.into_iter().rev() {
            if !self.removed_entities.contains(&child) && self.current_transform_parent(child)?==Some(parent) {
                self.change_transform_parent(child,None)?;
            }
        }
        self.change_transform_parent(parent,None)?;
        Ok(())
    }
    pub(super) fn prepare_child_slots(&mut self,owner:usize)->Result<bool> {
        if self.live_child_slots.contains_key(&owner) {return Ok(true);}
        let entity=&self.entities[owner];
        let Some(component)=entity.components.iter().find(|c|c.class_name=="ChildComponent") else {return Ok(false)};
        let mut slots=Vec::new();
        if let Some(field)=component.properties.iter().find(|p|p.name=="Child Entities") {
            for value in field.value.as_array().context("Invalid child membership array")? {
                let id=u32::try_from(value.as_u64().context("Invalid child membership ID")?)?;
                slots.push((id!=0).then(||self.entities.iter().enumerate().find_map(|(i,e)|
                    (e.scene==entity.scene && e.id==id && !self.removed_entities.contains(&i)).then_some(i))).flatten());
            }
        }
        self.live_child_slots.insert(owner,slots);Ok(true)
    }
    pub(super) fn owned_retirement_children(&mut self,owner:usize)->Result<Vec<Option<usize>>> {
        if !self.prepare_child_slots(owner)? {return Ok(Vec::new());}
        Ok(self.live_child_slots[&owner].clone())
    }
    fn parent_membership(&mut self,owner:usize,child:usize,add:bool)->Result<()> {
        if !self.prepare_child_slots(owner)? {return Ok(());}
        let slots=self.live_child_slots.get_mut(&owner).unwrap();
        if add {
            if let Some(slot)=slots.iter_mut().find(|slot|slot.is_none()) {*slot=Some(child);}
            else {slots.push(Some(child));}
        } else {
            for slot in slots {if *slot==Some(child) {*slot=None;}}
        }
        Ok(())
    }
    pub(super) fn current_transform_parent(&self,child:usize)->Result<Option<usize>> {
        if let Some(local)=self.transform_hierarchy.get(&child) {return Ok(Some(local.parent));}
        let e=&self.entities[child];
        Ok(e.transform_parent_id()?.and_then(|id|self.entities.iter().enumerate().find_map(|(i,p)|
            (p.scene==e.scene && p.id==id && !self.removed_entities.contains(&i)).then_some(i))))
    }
    fn parent_body_transition(&mut self,child:usize,parent:Option<usize>)->Result<()> {
        let sources=self.dynamic_runtime_sources.keys().copied().filter(|&s|self.dynamic_bodies[s].entity==child).collect::<Vec<_>>();
        for source in sources {
            let id=self.dynamic_bodies[source].component;
            let state=self.dynamic_constructions.get_mut(&source).context("Parented dynamic construction missing")?;
            let component=self.entities[child].components.iter_mut().find(|c|c.link_id.to_u32()==id).context("Parented body component missing")?;
            let mut backend=super::body_activation::Admitted {
                records:super::body_activation::PreAdmission {bodies:&mut state.bodies,component,gravity_factor_word:0x3f800000},
                world:&mut self.collision,materials:&state.materials,
            };
            let relations=&mut self.dynamic_parent_bindings;
            super::body_activation::change_parent(&mut backend,parent.is_some(),|system| {
                let flag=|name:&str|system.records.component.properties.iter().find(|p|p.name==name)
                    .and_then(|p|p.value.as_bool()).with_context(||format!("Parent motion lacks {name}"));
                Ok(flag("Is Real")? || flag("Is Painted")?)
            },|_| {relations.insert((child,id),parent);Ok(())})?;
        }
        Ok(())
    }
    /// Force the body pose from the same entity/node binding used for native
    /// keyframe targets. This is a warp, not a synthesized frame of velocity.
    pub(super) fn force_dynamic_entity_pose(&mut self,entity:usize)->Result<()> {
        let sources=self.dynamic_runtime_sources.keys().copied()
            .filter(|&source|self.dynamic_bodies[source].entity==entity).collect::<Vec<_>>();
        for source in sources {
            let state=&self.dynamic_constructions[&source];
            let targets=state.bodies.iter().enumerate().map(|(index,body)| {
                if let Some(binding)=state.node_bindings[index] {
                    return Ok(super::native_hk_transform::compose(
                        &self.rigid_model_node_frame(entity,binding.node,true)?,&binding.offset));
                }
                let binding=body.auxiliary.user_data.as_ref().and_then(|data|data.dynamic_binding)
                    .context("Parent pose forcing requires a recovered body/entity binding")?;
                Ok(super::native_hk_transform::compose(&self.entities[entity].physics_transform(),
                    &binding.local_transform.map(f32::from_bits)))
            }).collect::<Result<Vec<_>>>()?;
            let state=self.dynamic_constructions.get_mut(&source).context("Parent pose construction missing")?;
            for (body,target) in state.bodies.iter_mut().zip(targets) {
                body.warp(&mut self.collision,target)?;
                let shape=*body.shapes.first().context("Parented body has no query shape")?;
                let observed=self.collision.simulation_pose(shape)?;
                let error=(0..3).map(|axis|(observed.position[axis]-target[12+axis]).abs()).fold(0f32,f32::max);
                self.parent_pose_forces=self.parent_pose_forces.wrapping_add(1);
                self.parent_pose_error=self.parent_pose_error.max(error);
            }
        }
        Ok(())
    }
    pub(super) fn change_transform_parent(&mut self,child:usize,parent:Option<usize>)->Result<()> {
        let Some(component)=self.entities[child].components.iter().position(|c|c.class_name=="JPSTransformationComponent") else {return Ok(())};
        let old=self.current_transform_parent(child)?;
        if old==parent {return Ok(());}
        if let Some(parent)=parent {
            ensure!(self.entities[parent].components.iter().any(|c|c.class_name=="JPSTransformationComponent"),"Parent lacks a transform");
            let mut seen=std::collections::BTreeSet::new();let mut current=Some(parent);
            while let Some(i)=current {
                ensure!(i!=child && seen.insert(i),"Cyclic transform parent assignment");
                current=self.current_transform_parent(i)?;
            }
        }
        let socket=if parent.is_some() {super::skeletal_children::Socket::load(&self.entities[child].components[component])?} else {None};
        if parent.is_some() {if let Some(socket)=&socket {ensure!(matches!(socket.mode,0|1|3|4|5),"Unsupported skeletal parenting mode {}",socket.mode);}}
        if let Some(old)=old {
            self.prepare_child_slots(old)?;
            self.sync_keyframed_character_attachment(child)?;
        }
        if let Some(parent)=parent {self.prepare_child_slots(parent)?;}
        let world_position=self.entities[child].transform[12..15].try_into().unwrap();
        let world_rotation=self.entities[child].runtime_rotation;
        let (position,rotation)=if let Some(parent)=parent {
            super::native_transform_record::local_pose(&super::rotator_runtime::parent_cache(&self.entities[parent]),world_position,&world_rotation)
        } else {(world_position,world_rotation)};
        // Body motion changes precede publishing the new base association.
        self.parent_body_transition(child,parent)?;
        if let Some(old)=old {self.transform_membership(old,child,false)?;self.parent_membership(old,child,false)?;}
        if let Some(parent)=parent {self.transform_membership(parent,child,true)?;self.parent_membership(parent,child,true)?;}
        let id=parent.map(|p|self.entities[p].id).unwrap_or(0);
        for (name,class,value) in [
            ("Parent Entity","Entity Pointer",serde_json::json!(id)),
            ("Local Translation","Point3",serde_json::json!({"x":position[0],"y":position[1],"z":position[2]})),
            ("Local Rotation","Matrix3",serde_json::json!({"m":[&rotation[0..3],&rotation[3..6],&rotation[6..9]]})),
        ] {
            let properties=&mut self.entities[child].components[component].properties;
            if let Some(field)=properties.iter_mut().find(|p|p.name==name) {field.value=value;}
            else {properties.push(Property::new(class.into(),name.into(),false,false,false,value));}
        }
        if let Some(parent)=parent {
            self.transform_hierarchy.insert(child,super::rotator_runtime::Local {parent,position,rotation,socket});
            self.update_skeletal_children_range_strict(child..child+1)?;
            self.force_dynamic_entity_pose(child)?;
        } else {self.transform_hierarchy.remove(&child);}
        self.scene_motion_prepared_at=None;
        let limitation="Parent association, child membership slots, render following and admitted dynamic parent motion transitions are connected. Native dirty scheduling, descendant body forcing, static/character parenting and complete socket-mode parity remain incomplete.";
        if !self.report.unsupported.iter().any(|s|s==limitation) {self.report.unsupported.push(limitation.into());}
        Ok(())
    }
    pub(super) fn parenting_lua(&mut self,lua:&mlua::Lua,name:&str,mut args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        let parent=if name=="SetParentEntity" {self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(mlua::Value::Nil))?
            .filter(|i|!self.removed_entities.contains(i))} else {None};
        if let Some(child)=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(mlua::Value::Nil))?
            .filter(|i|!self.removed_entities.contains(i)) {
            self.change_transform_parent(child,parent).map_err(mlua::Error::external)?;
        }
        Ok(mlua::MultiValue::new())
    }
}
