//! Explicit forced object cloning, REL3cc8c4 ->3cbf6c ->3cbbc0 ->3cc118.
//! Automatic scheduling, template resources and deferred receivers remain open.
use super::*;
#[derive(Default, Debug, serde::Serialize)]
pub struct State {
    pub attempts: u32,
    /// Known writes only; native constructor/init state is not reconstructed.
    pub disabled: Option<bool>,
    pub live: Vec<usize>,
    pub destinations: BTreeMap<usize, usize>,
    pub occupied_destinations: u32,
    pub automatic: super::super::spawner_schedule::State,
}
fn field<'a>(
    component: &'a epic_mickey_lib_rs::scene_file::Component,
    name: &str,
) -> Option<&'a serde_json::Value> {
    component
        .properties
        .iter()
        .find(|p| p.name == name)
        .map(|p| &p.value)
}
fn ids(component: &epic_mickey_lib_rs::scene_file::Component, name: &str) -> Result<Vec<u32>> {
    field(component, name)
        .map(|value| {
            value
                .as_array()
                .context("Spawner reference list is not an array")?
                .iter()
                .map(|id| {
                    id.as_u64()
                        .context("Spawner reference is not an entity ID")
                        .and_then(|id| u32::try_from(id).map_err(Into::into))
                })
                .collect()
        })
        .transpose()
        .map(Option::unwrap_or_default)
}
fn clone_component_supported(name: &str) -> bool {
    matches!(
        name,
        "JPSTransformationComponent"
            | "JPSSceneGraphComponent"
            | "InputOutputComponent"
            | "ParticleSystemControllerComponent"
            | "LifetimeComponent"
            | "SplineFollowerComponent"
            | "UsableComponent"
            | "GameEventsComponent"
            | "AudioControllerComponent"
            | "ChildComponent"
            | "FXComponent"
            | "HealthComponent"
            | "InventoryComponent"
            | "AnimationComponent"
            | "AnimationComponentGB"
            | "PhysicsRigidBodyCharacterComponent"
            | "BasicMotionControllerComponent"
            | "BasicAIControllerComponent"
            | "AIBrainComponent"
            | "PaintThinnerCounterComponent"
            | "ArsenalComponent"
            | "WeaponComponent"
            | "ShooterComponent"
            | "TriggerDamageComponent"
            | "PhysicsRigidBodyPhantomShapeComponent"
            | "PhysicsPhantomComponent"
            | "PlayerPickupComponent"
            | "TargetableComponent"
            | "VulnerabilityComponent"
            | "TeamComponent"
            | "TrackingComponent"
            | "DeathRewardComponent"
            | "LODComponent"
            | "DynamicallyLitComponent"
            | "ShadowComponent"
            | "ScriptComponent"
            | "TemplateSpawnerComponent"
            | "R3MTEffectsComponent"
            | "ConversationComponent"
            | "MoodComponent"
    )
}
/// REL3cbfe0 starts with (1,1,1), rejects squared lengths greater than one,
/// then generates three rand lanes. Called even when Spawn Radius is zero.
fn radius_direction(random: &mut super::super::game_random::State) -> [f32; 3] {
    loop {
        let point = std::array::from_fn(|_| -1. + (2. * random.draw() as f32) / 32767.);
        if point[2].mul_add(point[2], point[0] * point[0]) + point[1] * point[1] <= 1. {
            return point;
        }
    }
}
enum Prototype { Object(usize), Template(String) }
impl World {
    pub(in crate::blotworx) fn queue_spawner_status(
        &mut self,
        kind: u16,
        owner: usize,
        header: usize,
    ) -> Result<()> {
        self.entity_event_queue.enqueue(
            &super::super::entity_events::Basic {
                event_kind: kind,
                source: Some(owner),
                header: Some(header),
            }
            .into(),
            None,
        )?;
        Ok(())
    }
    pub(in crate::blotworx) fn force_spawn(
        &mut self,
        owner: usize,
        count: i32,
    ) -> Result<Vec<Option<usize>>> {
        self.force_spawn_with_index(owner,count,None)
    }
    pub(super) fn spawn_reward_index(&mut self,owner:usize,index:i32)->Result<Vec<usize>> {
        Ok(self.force_spawn_with_index(owner,1,Some(index))?.into_iter().flatten().collect())
    }
    fn force_spawn_with_index(&mut self,owner:usize,count:i32,index_override:Option<i32>)->Result<Vec<Option<usize>>> {
        let source = self
            .entities
            .get(owner)
            .context("Spawner owner is absent")?;
        ensure!(
            !self.removed_entities.contains(&owner),
            "Spawner owner was retired"
        );
        let Some(component) = source
            .components
            .iter()
            .find(|c| c.class_name == "TemplateSpawnerComponent")
        else {
            return Ok(Vec::new());
        };
        if count <= 0 {
            return Ok(Vec::new());
        }
        let objects = ids(component, "Objects To Spawn")?;
        let templates = field(component, "Templates To Spawn").map(|value| -> Result<Vec<String>> {
            value.as_array().context("Invalid Templates To Spawn array")?.iter().map(|entry| {
                Ok(entry.as_str().context("Invalid template name")?.trim_end_matches('\0').to_owned())
            }).collect()
        }).transpose()?.unwrap_or_default();
        let named = templates.iter().any(|name| !name.is_empty());
        ensure!(!named || objects.iter().all(|&id|id==0), "Mixed object/template source precedence is not integrated");
        ensure!(!named || templates.iter().all(|name|!name.is_empty()), "Sparse template source indexing is not integrated");
        let source_count = if named {templates.len()} else {objects.len()};
        if source_count == 0 {
            self.spawners.entry(owner).or_default().disabled = Some(true);
            let mut empty = Vec::new();
            empty.try_reserve(count as usize)?;
            empty.resize(count as usize, None);
            return Ok(empty);
        }
        let index=match index_override {
            Some(index)=>index,
            None=>i32::try_from(field(component,"Index To Spawn").and_then(|v|v.as_i64()).context("ForceSpawn requires the authored/live Index To Spawn")?)?,
        };
        let destinations = ids(component, "Spawn Destinations")?;
        let Some(pairs) =
            super::super::spawn_selection::pairs(source_count, destinations.len(), index)
        else {
            self.spawners.entry(owner).or_default().disabled = Some(true);
            return Ok(vec![None; count as usize]);
        };
        let above_ground = field(component, "TSPSpawnAboveGround")
            .map(|value| value.as_bool().context("Invalid TSPSpawnAboveGround flag"))
            .transpose()?
            .unwrap_or(false);
        let group = field(component, "Spawn Group")
            .map(|v| v.as_str().context("Invalid spawn group"))
            .transpose()?
            .unwrap_or("")
            .to_owned();
        let radius = field(component, "Spawn Radius")
            .map(|v| v.as_f64().context("Invalid spawn radius"))
            .transpose()?
            .unwrap_or(0.) as f32;
        let offset = field(component, "Spawn Offset")
            .map(|v| -> Result<V> {
                Ok([
                    v["x"].as_f64().context("Invalid spawn offset")? as f32,
                    v["y"].as_f64().context("Invalid spawn offset")? as f32,
                    v["z"].as_f64().context("Invalid spawn offset")? as f32,
                ])
            })
            .transpose()?
            .unwrap_or([0.; 3]);
        ensure!(
            radius.is_finite() && offset.iter().all(|v| v.is_finite()),
            "Invalid spawn placement"
        );
        let total = field(component, "Total Spawns")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let scene = source.scene.clone();
        let prototypes = pairs
            .iter()
            .map(|pair| {
                if named { return Ok(Some(Prototype::Template(templates[pair.object].clone()))); }
                let id = objects[pair.object];
                if id == 0 {
                    return Ok(None);
                }
                let prototype = self
                    .entities
                    .iter()
                    .enumerate()
                    .find(|(index, entity)| {
                        entity.scene == scene
                            && entity.id == id
                            && !self.removed_entities.contains(index)
                    })
                    .map(|(index, _)| index)
                    .with_context(|| format!("Spawner {} source slot {} references absent entity {id} in {scene}", source.name, pair.object))?;
                self.preflight_render_clone(prototype)?;
                Ok(Some(Prototype::Object(prototype)))
            })
            .collect::<Result<Vec<_>>>()?;
        if source_count == 1 && prototypes.first().is_some_and(Option::is_none) {
            self.spawners.entry(owner).or_default().disabled = Some(true);
            return Ok(vec![None; count as usize]);
        }
        let mut output = Vec::new();
        output.try_reserve(count as usize)?;
        for _ in 0..count {
            let mut last = None;
            for (pair, prototype) in pairs.iter().zip(&prototypes) {
                let Some(prototype) = prototype.as_ref() else {
                    continue;
                };
                let state = self.spawners.entry(owner).or_default();
                let slot = pair.destination.unwrap_or_else(|| {
                    super::super::spawn_selection::random_destination(
                        destinations.len(),
                        state.live.len(),
                        state.occupied_destinations,
                        || self.game_random.draw(),
                    )
                });
                let destination = match destinations.get(slot).copied().unwrap_or(0) {
                    0 if slot == 0 => owner,
                    id => self
                        .entities
                        .iter()
                        .enumerate()
                        .find(|(index, entity)| {
                            id != 0
                                && entity.scene == scene
                                && entity.id == id
                                && !self.removed_entities.contains(index)
                        })
                        .map(|(index, _)| index)
                        .context("Spawn destination is not a live entity")?,
                };
                let origin: V = self.entities[destination].transform[12..15]
                    .try_into()
                    .unwrap();
                let rotation = self.entities[destination].runtime_rotation;
                let point = radius_direction(&mut self.game_random);
                let position = [
                    origin[0] + point[0] * radius + offset[0],
                    origin[1] + offset[1],
                    origin[2] + point[2] * radius + offset[2],
                ];
                let entity = match prototype {
                    Prototype::Object(prototype) => self.clone_render_entity(*prototype, position, rotation, &group)?,
                    Prototype::Template(name) => self.spawn_palette_root(owner, name, position, rotation, &group)?,
                };
                if above_ground {
                    if let Err(error) = self.place_spawn_above_ground(entity, position, rotation) {
                        self.retire_entity(entity)?;
                        return Err(error);
                    }
                }
                let state = self.spawners.entry(owner).or_default();
                state.live.push(entity);
                state.destinations.insert(entity, slot);
                state.occupied_destinations |= super::super::spawn_selection::destination_bit(slot);
                self.queue_dynamic_spawn_output(entity)?;
                self.queue_spawner_status(0x18, owner, entity)?;
                last = Some(entity);
            }
            let state = self.spawners.entry(owner).or_default();
            state.attempts = state.attempts.wrapping_add(1);
            if total > 0 && u64::from(state.attempts) == total as u64 {
                self.queue_spawner_status(0x19, owner, owner)?;
            }
            output.push(last);
        }
        let limitation="Object clones retain reflected gameplay components, fresh Havok/Gamebryo animation instances and generated character bodies through the common lifecycle. Prefab template member graphs, dynamic/static collision cloning and full child/prefab lifecycle remain incomplete; AI and physics retain their placed-actor compatibility limitations.";
        if !self
            .report
            .unsupported
            .iter()
            .any(|line| line == limitation)
        {
            self.report.unsupported.push(limitation.into());
        }
        Ok(output)
    }

    fn place_spawn_above_ground(&mut self, entity: usize, position: V, rotation: [f32; 9]) -> Result<()> {
        let body = self.character_physics.get(&entity).and_then(|body| body.as_ref().ok());
        let half = if let Some(phantom) = self.size_phantoms.get(&entity) {
            phantom.size.map(|extent| extent * 0.5)
        } else if let Some(body) = body {
            self.collision.capsule_half_extents(body.shape)?.map(|extent| extent + 0.01)
        } else {
            [1.; 3]
        };
        ensure!(half.iter().all(|extent| extent.is_finite() && *extent > 0.),
            "Invalid spawn placement box");
        // REL3cc768..3cc870 uses a rounded box and raises Y to the maximum
        // contact height plus half its height. A blocked retry keeps the
        // authored pose. Box3D supplies the contact witnesses.
        let flags = super::super::physics_filter::QueryFlags {
            bytes: [1, 1, 1, 0, 1],
            excluded_group: 0,
        };
        let shape = body.map(|body| body.shape);
        let collision = &self.collision;
        let adjusted = self.dynamic_runtime_filter.query(Some(flags), |filter| {
            let contacts = |center| collision.box_contact_height(center, half, 0.05, |candidate| {
                Some(candidate) != shape && collision.simulation_filter(candidate)
                    .is_some_and(|word| filter.collision_info(0x600001, word) != 0)
            });
            let Some(height) = contacts(position)? else { return Ok(None) };
            let raised = [position[0], height.max(0.) + half[1], position[2]];
            Ok(contacts(raised)?.is_none().then_some(raised))
        })?;
        if let Some(raised) = adjusted {
            self.relocate_entity(entity, raised, rotation)?;
        }
        self.report.geometry.push(serde_json::json!({
            "kind": "spawn_above_ground", "entity": entity, "origin": position, "adjusted": adjusted,
        }));
        Ok(())
    }
    fn queue_dynamic_spawn_output(&mut self, entity: usize) -> Result<()> {
        // REL182718..182740 emits 3e after insertion in the world. The
        // template spawner reaches that path at 3cc558. REL270d7c binds
        // the distinct OnSpawnedDynamically output, separate from kind 3d.
        self.entity_event_queue.enqueue(&super::super::entity_events::Basic {
            event_kind: 0x3e,
            source: Some(entity),
            header: None,
        }.into(), None)?;
        Ok(())
    }

    fn preflight_render_clone(&self, source: usize) -> Result<()> {
        let entity = &self.entities[source];
        ensure!(
            !self.rigid_trigger_hulls.contains_key(&source),
            "Resource-backed weapon trigger cloning requires rebinding on {}", entity.name
        );
        let unsupported=entity.components.iter().filter(|component|!clone_component_supported(&component.class_name))
            .map(|component|component.class_name.as_str()).collect::<Vec<_>>();
        ensure!(unsupported.is_empty(),"ForceSpawn clone lifecycle on {} has unsupported components: {}",entity.name,unsupported.join(", "));
        ensure!(
            entity.jigsaw_asset.is_none()
                && entity.jigsaw_states.is_empty()
                && entity.collision_parts.is_empty(),
            "Jigsaw/collision clone instantiation is not implemented on {}",
            entity.name
        );
        if entity.components.iter().any(|component|component.class_name=="AnimationComponentGB") {
            ensure!(self.actor_sequences.contains_key(&source) && self.actor_bindings.contains_key(&source),
                "Source Gamebryo actor has no prepared sequence resources/bindings: {}",entity.name);
        }
        if entity
            .components
            .iter()
            .any(|component| component.class_name == "AnimationComponent")
        {
            ensure!(
                self.authored_havok.contains(source),
                "Source actor has no initialized Havok resources: {}",
                entity.name
            );
        }
        if entity
            .components
            .iter()
            .any(|component| component.class_name == "PhysicsRigidBodyCharacterComponent")
        {
            ensure!(
                self.character_physics
                    .get(&source)
                    .is_some_and(Result::is_ok),
                "Source character has no admitted physics body: {}",
                entity.name
            );
        }
        for component in entity
            .components
            .iter()
            .filter(|component| component.class_name == "ScriptComponent")
        {
            for property in component.properties.iter().filter(|property| {
                property
                    .name
                    .split_whitespace()
                    .collect::<String>()
                    .eq_ignore_ascii_case("LuaFilePath")
            }) {
                for path in super::super::actor_scripts::authored_paths(&property.value)? {
                    ensure!(
                        self.scripts.module_loaded(&path)?,
                        "Source script module is not initialized: {path}"
                    );
                }
            }
        }
        Ok(())
    }
    fn clone_render_entity(
        &mut self,
        source: usize,
        position: V,
        rotation: [f32; 9],
        group: &str,
    ) -> Result<usize> {
        self.clone_render_branch(source, position, rotation, group, &mut Vec::new())
    }

    fn clone_render_branch(
        &mut self,
        source: usize,
        position: V,
        rotation: [f32; 9],
        group: &str,
        ancestry: &mut Vec<usize>,
    ) -> Result<usize> {
        ensure!(!ancestry.contains(&source), "Cyclic spawn child ownership on {}", self.entities[source].name);
        ensure!(position.iter().chain(&rotation).all(|value| value.is_finite()), "Invalid spawn pose");
        self.preflight_render_clone(source)?;
        let slots = self.owned_retirement_children(source)?;
        let original = &self.entities[source];
        let transform = super::super::render_transform::from_entity(rotation, position, original.authored_scale);
        let mut entity = RuntimeEntity {
            authored_scale: original.authored_scale,
            authored_rotation: original.authored_rotation,
            runtime_rotation: rotation,
            model_asset: original.model_asset.clone(),
            mesh_draws: original.mesh_draws.clone(),
            jigsaw_asset: None,
            jigsaw_states: Vec::new(),
            scene: original.scene.clone(),
            id: original.id,
            name: original.name.clone(),
            transform,
            authored_transform: transform,
            components: original.components.clone(),
            draws: original.draws.clone(),
            collision_parts: 0..0,
        };
        // This is a fresh component, not an attached copy of the live source.
        // REL1ea8dc constructs it, then virtual1b4=13002c retains only the
        // source-property link. Transform constructor352f70 initializes its
        // inline parent handle+ b0 through202bbc ->2b4074 ->13b9e0 (null).
        // Getter353828 reads that inline handle, not the source link+28.
        // Do not run detach on the source or copy its parent's child ownership.
        if let Some(transform) = entity
            .components
            .iter_mut()
            .find(|c| c.class_name == "JPSTransformationComponent")
        {
            if let Some(parent) = transform
                .properties
                .iter_mut()
                .find(|p| p.name == "Parent Entity")
            {
                parent.value = serde_json::json!(0);
            }
        }
        if let Some(field) = entity.components.iter_mut()
            .find(|c| c.class_name == "ChildComponent")
            .and_then(|c| c.properties.iter_mut().find(|p| p.name == "Child Entities"))
        {
            field.value = serde_json::json!(vec![0u32; slots.len()]);
        }
        let mut aliases = Vec::new();
        if let Some(io) = entity
            .components
            .iter_mut()
            .find(|c| c.class_name == "InputOutputComponent")
        {
            if !group.is_empty() && !io.properties.iter().any(|p| p.name == "EntityGroups") {
                let definition = crate::component_schema::CATALOG.components
                    ["InputOutputComponent"]
                    .fields["EntityGroups"]
                    .variants
                    .first()
                    .context("Missing group field schema")?;
                let mut property = crate::component_schema::observed_property(definition);
                property.value = serde_json::json!([]);
                io.properties.push(property);
            }
            if let Some(property) = io.properties.iter_mut().find(|p| p.name == "EntityGroups") {
                let groups = property
                    .value
                    .as_array_mut()
                    .context("EntityGroups is not an array")?;
                if !group.is_empty() {
                    groups.push(serde_json::json!(group));
                }
                aliases = groups
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .context("Invalid entity group")
                    })
                    .collect::<Result<_>>()?;
            }
        }
        let source_draws = entity.draws.clone();
        let draw_base = self.draws.len();
        let id = self
            .entities
            .iter()
            .map(|e| e.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("Spawned entity ID exhausted")?;
        entity.id = id;
        entity.name = format!("{} [spawn {id}]", entity.name);
        entity.draws = draw_base..draw_base + source_draws.len();
        entity.collision_parts = 0..0;
        for indices in entity.mesh_draws.values_mut() {
            for index in indices {
                *index = draw_base + *index - source_draws.start;
            }
        }
        let mut draws = self.draws[source_draws.clone()].to_vec();
        for draw in &mut draws {
            draw.trans_mat = transform;
        }
        let bindings = self
            .spherical_bindings
            .iter()
            .filter(|binding| binding.entity == source)
            .cloned()
            .collect::<Vec<_>>();
        let index = self.entities.len();
        self.entities.push(entity);
        self.draws.extend(draws);
        if !slots.is_empty() {
            self.live_child_slots.insert(index, vec![None; slots.len()]);
        }
        ancestry.push(source);
        let mut owned = Vec::new();
        let result = (|| -> Result<()> {
            // Share immutable model-local resources, never the source actor's
            // live clip, transition, clock or sampled pose. The common
            // component constructor below creates a fresh playback instance.
            if let Some(resources)=self.actor_sequences.get(&source).cloned() {
                self.actor_sequences.insert(index,resources);
            }
            if let Some(bindings)=self.actor_bindings.get(&source).cloned() {
                self.actor_bindings.insert(index,bindings);
            }
            self.scripts
                .register_entity(index, &self.entities[index].name, &aliases)?;
            // Component constructors consume the spawn pose directly. A
            // relocation would require a phantom that has not been built yet.
            for draw in source_draws.clone() {
                let target = draw_base + draw - source_draws.start;
                if let Some(material) = self.spherical_materials.get(&draw).cloned() {
                    self.spherical_materials.insert(target, material);
                }
                if self.model_local_draws.contains(&draw) {
                    self.model_local_draws.insert(target);
                }
                if let Some(matrix) = self.model_mesh_inverse.get(&draw).copied() {
                    self.model_mesh_inverse.insert(target, matrix);
                }
                if let Some(morph_source)=self.actor_draw_sources.get(&draw).cloned() {
                    self.actor_draw_sources.insert(target,morph_source);
                }
            }
            let mut properties = BTreeMap::new();
            for mut binding in bindings {
                let property = if let Some(property) = properties.get(&binding.property) {
                    *property
                } else {
                    let root = *self
                        .entity_bound_hierarchy(index)?
                        .context("Spawned model hierarchy is absent")?
                        .roots
                        .iter()
                        .flatten()
                        .next()
                        .context("Spawned model has no root")?;
                    let property = self.spherical_properties.allocate(
                        super::super::spherical_lighting::BoundOwner {
                            entity: index as u64,
                            node: root,
                        },
                    )?;
                    properties.insert(binding.property, property);
                    property
                };
                binding.property = self
                    .spherical_properties
                    .attach((index as u64, binding.mesh), property)?;
                binding.entity = index;
                binding.draw = draw_base + binding.draw - source_draws.start;
                self.spherical_bindings.push(binding);
            }
            let root = self.data_root.clone();
            self.bind_cloned_phantom(source, index)?;
            self.initialize_component_range(index..index + 1, &[], &root)?;
            if self.entities[index].components.iter().any(|component|component.class_name=="AnimationComponentGB") {
                ensure!(self.actors.contains_key(&index),"Spawned Gamebryo actor initialization failed");
            }
            if self.entities[index]
                .components
                .iter()
                .any(|component| component.class_name == "AnimationComponent")
            {
                ensure!(
                    self.authored_havok.contains(index),
                    "Spawned actor animation initialization failed"
                );
            }
            if self.entities[index]
                .components
                .iter()
                .any(|component| component.class_name == "PhysicsRigidBodyCharacterComponent")
            {
                ensure!(
                    self.character_physics
                        .get(&index)
                        .is_some_and(Result::is_ok),
                    "Spawned character physics initialization failed"
                );
            }
            if self.demo_story.enabled {
                self.initialize_demo_motion_range(index..index + 1);
            }
            self.entities[index].authored_transform = self.entities[index].transform;
            for (slot, child) in slots.into_iter().enumerate() {
                let Some(child) = child.filter(|child| !self.removed_entities.contains(child)) else {
                    continue;
                };
                let Some(local) = self.transform_hierarchy.get(&child).filter(|local| local.parent == source) else {
                    let message = format!("Spawn child {} has a separate transform owner; rebinding is pending", self.entities[child].name);
                    if !self.report.unsupported.contains(&message) {
                        self.report.unsupported.push(message);
                    }
                    continue;
                };
                if let Err(error) = self.preflight_render_clone(child) {
                    let message = format!("Spawn child {}: {error:#}", self.entities[child].name);
                    if !self.report.unsupported.contains(&message) {
                        self.report.unsupported.push(message);
                    }
                    continue;
                }
                let parent = super::super::rotator_runtime::parent_cache(&self.entities[index]);
                let (position, rotation) = super::super::native_transform_record::world_pose(
                    &parent, local.position, &local.rotation,
                );
                let spawned = self.clone_render_branch(child, position, rotation, "", ancestry)?;
                owned.push(spawned);
                self.change_transform_parent(spawned, Some(index))?;
                self.queue_dynamic_spawn_output(spawned)?;
                let slots = self.live_child_slots.get_mut(&index).context("Spawn lost its child slots")?;
                // Parenting selects the first free slot. The factory preserves
                // the original indexed layout, including empty weapon slots.
                let assigned = slots.iter().position(|id| *id == Some(spawned))
                    .context("Spawned child was not registered by parenting")?;
                slots.swap(assigned, slot);
            }
            if let Some(slots) = self.live_child_slots.get(&index) {
                let ids = slots.iter().map(|child| child.map_or(0, |child| self.entities[child].id))
                    .collect::<Vec<_>>();
                if let Some(field) = self.entities[index].components.iter_mut()
                    .find(|c| c.class_name == "ChildComponent")
                    .and_then(|c| c.properties.iter_mut().find(|p| p.name == "Child Entities"))
                {
                    field.value = serde_json::json!(ids);
                }
            }
            self.report.geometry.push(serde_json::json!({
                    "kind": "spawned_object_clone",
                    "entity": index,
                    "source": self.entities[source].name,
                    "clone": self.entities[index].name,
                    "component_count": self.entities[index].components.len(),
                    "components": self.entities[index].components.iter().map(|component|component.class_name.as_str()).collect::<Vec<_>>(),
                    "components_preserved": true,
                    "common_component_initialization": true,
                    "havok_actor_initialized": self.authored_havok.contains(index),
                    "gamebryo_actor_initialized": self.actors.contains_key(&index),
                    "character_body_initialized": self.character_physics.get(&index).is_some_and(Result::is_ok),
                    "ai_brain_initialized": self.ai_brains.contains_key(&index),
                    "full_native_clone_parity_verified": false
                }));
            Ok(())
        })();
        ancestry.pop();
        if let Err(error) = result {
            for child in owned.into_iter().rev() {
                self.queue_entity_retirement(child)?;
            }
            self.retire_entity(index)?;
            return Err(error);
        }
        // REL1ee170..1ee1a4 queues Basic3d after component construction,
        // source=new entity and null header. A clone is not a map load.
        self.entity_event_queue.enqueue(&super::super::entity_events::Basic {
            event_kind:0x3d,source:Some(index),header:None,
        }.into(),None)?;
        Ok(index)
    }
}
#[cfg(debug_assertions)]
pub(in crate::blotworx) fn validate_airlock_spawns()->Result<()> {
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world=load(Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
    world.enable_demo_story()?;
    ensure!(!world.report.unsupported.iter().any(|s|s.starts_with("Authored IO ") && s.contains("Spatter_SetReward")),"Original placed enemy reward initialization failed");
    let door=world.entities.iter().position(|e|e.name=="gv_zonea_airlock.GV_ZoneB_EntryDoorA1 01").context("Original airlock door missing")?;
    ensure!(world.actors[&door].requested_id()==Some(1),"Inactive door lost its authored Active Sequence ID");
    ensure!(world.actors[&door].active_id()==Some(1),"Placed door's original OnSpawned callback was not delivered during loading");
    world.actors.get_mut(&door).unwrap().reset();
    world.idle_actors.insert(door);
    world.request_actor_sequence(door,"Close")?;
    ensure!(!world.idle_actors.contains(&door),"Unknown authored door sequence did not wake component");
    world.advance_models(0.1)?;
    ensure!(world.actors[&door].active_id()==Some(1),"Woken door did not consume retained target");
    ensure!(world.actors[&door].pose.is_some(),"Woken door did not publish animation pose");
    let source=world.dynamic_bodies.iter().position(|body|body.entity==door).context("Airlock door collision source missing")?;
    ensure!(world.dynamic_runtime_sources.contains_key(&source),"Airlock door collision was not admitted");
    for tick in 1..=90 {
        world.advance_models(0.1+tick as f32*super::super::STEP)?;
        world.advance_dynamic_physics(super::super::STEP)?;
    }
    let closed=world.dynamic_constructions[&source].bodies.iter().map(|body|body.state().transform).collect::<Vec<_>>();
    world.request_actor_sequence(door,"Open2")?;
    ensure!(world.actors[&door].active_id()==Some(2),"Exit opening did not replace retained target");
    for tick in 91..=180 {
        world.advance_models(0.1+tick as f32*super::super::STEP)?;
        world.advance_dynamic_physics(super::super::STEP)?;
    }
    let construction=&world.dynamic_constructions[&source];
    ensure!(!construction.bodies.is_empty(),"Airlock collision has no bodies");
    let mut moved=false;
    for (index,body) in construction.bodies.iter().enumerate() {
        let binding=construction.node_bindings[index].context("Airlock collision has no animated model-node binding")?;
        let expected=super::super::native_hk_transform::compose(&world.rigid_model_node_frame(door,binding.node,true)?,&binding.offset);
        let actual=world.collision.simulation_pose(body.shapes[0])?;
        ensure!((0..3).all(|axis|(actual.position[axis]-expected[12+axis]).abs()<0.005),"Airlock collision detached from animated door: {:?} vs {:?}",actual.position,&expected[12..15]);
        let pose=body.state().transform;
        moved|=pose.iter().zip(closed[index]).any(|(a,b)|(a-b).abs()>0.1);
    }
    ensure!(moved,"Airlock door collision did not move when opening");
    println!("PASS: airlock collision follows the animated model through closing/opening (5mm position tolerance)");
    let steam=world.entities.iter().position(|e|e.name=="gv_zonea_airlock.pipesteamer+markers 06.Steam_Blocker_01 01").context("Original airlock steam model missing")?;
    world.admit_scene_particles();
    ensure!(world.scene_particles.live.contains_key(&steam),"Airlock steam was not admitted: {:?}",world.scene_particles.errors.get(&steam));
    let mut maximum_particles=0;
    // This NIF's emitter controllers span 0..8.333 seconds. Observe a full
    // cycle instead of mistaking a quiet interval for an absent effect.
    for _ in 0..600 {
        world.advance_scene_particles(super::super::STEP)?;
        maximum_particles=maximum_particles.max(world.scene_particles.live[&steam].particles.particle_count());
    }
    ensure!(maximum_particles>0,"Original airlock steam remained empty through its complete emitter cycle");
    world.scene_particles.live[&steam].particles.validate_retained_world_scale(world.entities[steam].transform)?;
    println!("PASS: original steam particles preserve world positions and native velocities/radii across owner motion and doubled scale");
    println!("PASS: airlock steam emits from its authored NIF (peak {maximum_particles} particles across {:.2} seconds)",600.*super::super::STEP);
    println!("PASS: placed airlock startup consumes retained sequence; unknown Close wakes it; Open2 selects the exit sequence");
    let spawn_io_before=world.deferred_event_runtime.spawn_io_deliveries;
    world.run_callback_fixture("FireUser1('GV_ZoneA_PipeSpawner01'); FireUser1('GV_ZoneA_PipeSpawner02')")?;
    for _ in 0..60 {world.advance_models_with_delta(world.animation_time+super::super::STEP,super::super::STEP)?;}
    for name in ["gv_zonea_airlock.spatterpipespawner 01.Spawner 01", "gv_zonea_airlock.spatterpipespawner 02.Spawner 01"] {
        let owner=world.entities.iter().position(|e|e.name==name).context("Original airlock spawner missing")?;
        let spawned=world.spawners[&owner].live.clone();
        ensure!(spawned.len()==1,"Airlock spawner did not create exactly one enemy: {name}");
        let child=spawned[0];
        ensure!(world.authored_havok.contains(child),"Airlock enemy animation missing");
        ensure!(world.character_physics.get(&child).is_some_and(Result::is_ok),"Airlock enemy collision missing");
        ensure!(world.ai_brains.contains_key(&child),"Airlock enemy AI missing");
        println!("PASS: {name} creates {} with animation, collision and AI",world.entities[child].name);
    }
    world.process_deferred_entity_events()?;
    ensure!(world.deferred_event_runtime.spawn_io_deliveries>spawn_io_before,
        "Spawned enemies did not deliver their original OnSpawned connections");
    println!("PASS: spawned enemies deliver their original deferred OnSpawned initialization with null activator");
    let live=world.scripts.resolve_name("gremlinvillage_zonea.SpatterMelee 05").context("Live Spatter alias missing")?;
    ensure!(world.entities[live].name.contains("[spawn"),"Unfreeze alias still selects the hidden source template");
    world.run_callback_fixture("EnableMotion(GetEntity('gremlinvillage_zonea.SpatterMelee 05'))")?;
    ensure!(world.character_physics[&live].as_ref().unwrap().motion.current.motion_type()==7,
        "Authored Spatter alias did not enable the live spawned body");
    println!("PASS: original airlock unfreeze alias selects the live spawned Spatter and enables its motion");
    let barrel=world.entities.iter().position(|e|e.name=="gremlinvillage_zonea.Breakable Barrel Health 03").context("Original barrel missing")?;
    let before=world.named_fx.created;
    let emission=super::super::projectile_stimulus::Emission {
        stimulus:5,amount:100.,position:world.entities[barrel].transform[12..15].try_into()?,direction:[0.;3],
        target:Some(barrel as u64),owner:Some(world.player_entity as u64),tag:"Spin".into(),delivery:super::super::projectile_stimulus::Delivery::Target,
    };
    world.receive_combat(barrel,&[emission])?;
    let started = std::time::Instant::now();
    world.advance_health_lifecycle()?;
    println!("Barrel break lifecycle: {:.2} ms", started.elapsed().as_secs_f64() * 1000.);
    ensure!(world.named_fx.created>before,"Original barrel break effect was not created: {:?}",world.report.unsupported.iter().filter(|s|s.starts_with("Death effect")).collect::<Vec<_>>());
    let effect=*world.named_fx.live.iter().find(|(_,fx)|fx.owner==barrel && fx.fire_and_forget).context("Detached barrel effect missing")?.0;
    ensure!(!world.transform_hierarchy.contains_key(&effect),"Death effect remained attached to hidden scenery");
    let dispatch=world.report.geometry.iter().find(|record|record["kind"]=="death_reward_spawner_dispatch" && record["owner"].as_u64()==Some(barrel as u64)).context("Barrel death did not dispatch its authored spawner reward")?;
    let reward=dispatch["spawned"].as_array().and_then(|array|array.first()).and_then(|id|id.as_u64()).context("Barrel reward created no entity")? as usize;
    ensure!(world.entities[reward].components.iter().any(|c|c.class_name=="AudioControllerComponent"),"Barrel reward audio component missing");
    ensure!(world.runtime_lifetimes.get(&reward).is_some_and(|state|state.timer.remaining==4.),"Death reward overwrote the original nonzero lifetime");
    let dynamic=world.report.geometry.iter().find(|record|record["kind"]=="dynamic_reward_selection" && record["owner"].as_u64()==Some(barrel as u64))
        .with_context(||format!("Barrel dynamic reward missing: {:?}",world.report.unsupported.iter().filter(|line|line.starts_with("Death reward")).collect::<Vec<_>>()))?;
    let pickup=dynamic["entity"].as_u64().context("Dynamic reward identity missing")? as usize;
    ensure!(world.pickup_receivers.contains_key(&pickup),"Dynamic barrel reward has no pickup receiver");
    ensure!(world.pickup_update_states[&pickup].gate.delayed,"Dynamic reward collection delay was not applied");
    println!("PASS: original barrel dynamic row creates a real pickup through live selection, pending reservation and collection delay");
    let dispatch_count=world.report.geometry.iter().filter(|record|record["kind"]=="death_reward_spawner_dispatch").count();
    world.receive_death_rewards(barrel,2)?;
    ensure!(world.report.geometry.iter().filter(|record|record["kind"]=="death_reward_spawner_dispatch").count()==dispatch_count,"Retired barrel received another death reward");
    println!("PASS: original barrel lethal spin creates detached break effect and authored audio reward, retaining its four-second lifetime");
    // Use an actual prepared, renderable nonphysical Gamebryo prop. Physical
    // props still fail preflight until their body construction is implemented.
    let source=world.actors.keys().copied().find(|source|
        !world.entities[*source].draws.is_empty() && world.preflight_render_clone(*source).is_ok())
        .context("Original renderable Gamebryo clone source missing")?;
    let source_name=world.entities[source].name.clone();
    let resources=world.actor_sequences[&source].clone();
    let bindings=world.actor_bindings[&source].clone();
    let source_active=world.actors[&source].active_id();
    let source_requested=world.actors[&source].requested_id();
    let sequence=resources.sequences().next().context("Original prop has no sequence")?.0;
    let position=world.entities[source].transform[12..15].try_into()?;
    let clone=world.clone_render_entity(source,position,world.entities[source].runtime_rotation,"")?;
    ensure!(std::sync::Arc::ptr_eq(&world.actor_sequences[&clone],&resources)
        && std::sync::Arc::ptr_eq(&world.actor_bindings[&clone],&bindings),"Clone duplicated immutable Gamebryo resources");
    world.actors.get_mut(&clone).unwrap().request(sequence,&resources,&bindings)?;
    ensure!(world.actors[&clone].active_id()==Some(sequence),"Clone did not activate its own animation");
    world.actors.get_mut(&clone).unwrap().reset();
    ensure!(world.actors[&source].active_id()==source_active && world.actors[&source].requested_id()==source_requested,
        "Clone reset changed the source actor's playback");
    world.retire_entity(clone)?;
    ensure!(world.actors.contains_key(&source) && !world.actors.contains_key(&clone)
        && !world.actor_sequences.contains_key(&clone) && !world.actor_bindings.contains_key(&clone),
        "Gamebryo clone retirement leaked instance resources or retired the source");
    println!("PASS: original Gamebryo prop {source_name} clones with shared resources, independent animation/reset and isolated retirement");
    let npc=world.spawners.values().flat_map(|state|state.live.iter().copied())
        .find(|entity|world.character_physics.get(entity).is_some_and(Result::is_ok))
        .context("Spawned character reset fixture missing")?;
    world.suspend_character_motion(npc,false)?;
    let shape=world.character_physics[&npc].as_ref().unwrap().shape;
    world.character_physics.get_mut(&npc).unwrap().as_mut().unwrap().motion.current
        .set_linear_velocity([2.,-3.,4.,0.]);
    world.collision.set_simulation_velocity(shape,[2.,-3.,4.])?;
    world.suspend_character_motion(npc,true)?;
    let name=world.entities[npc].name.clone();
    world.run_callback_fixture(&format!("local e=GetEntity({name:?}); SetTransformation(e,GetPosition(e),GetFacing(e))"))?;
    ensure!(world.character_physics[&npc].as_ref().unwrap().motion.current.linear_velocity()==[0.;4],
        "Relocation left stale retained character velocity");
    ensure!(world.collision.simulation_pose(shape)?.linear==[0.;3],"Relocation left stale solver velocity");
    world.suspend_character_motion(npc,false)?;
    ensure!(world.collision.simulation_pose(shape)?.linear==[0.;3]
        && world.character_physics[&npc].as_ref().unwrap().motion.current.linear_velocity()==[0.;4],
        "Resuming character motion restored pre-relocation velocity");
    println!("PASS: suspended original character relocation clears retained/solver velocity and resuming motion does not restore its old trajectory");
    Ok(())
}
#[cfg(debug_assertions)]
pub(in crate::blotworx) fn validate_conversation_clones()->Result<()> {
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world=load(Path::new(&root),"Levels/GV_ZoneD_Start.level",&egui::Context::default())?;
    world.enable_demo_story()?;
    let owner=world.entities.iter().position(|entity|entity.name.ends_with(".Spawner_Oswald")).context("Original Oswald spawner missing")?;
    let resource_count=world.story_sequences.resources.len();
    let spawned=world.force_spawn(owner,1)?.into_iter().flatten().collect::<Vec<_>>();
    ensure!(spawned.len()==1,"Original Oswald spawner did not create one actor");
    let clone=spawned[0];
    ensure!(world.authored_havok.contains(clone),"Oswald clone animation missing");
    ensure!(world.character_physics.get(&clone).is_some_and(Result::is_ok),"Oswald clone body missing");
    ensure!(world.ai_brains.contains_key(&clone),"Oswald clone AI owner missing");
    let source=world.entities.iter().enumerate().find(|(index,entity)|*index!=clone && entity.name.ends_with(".AI_Oswald")).map(|(index,_)|index).context("Original Oswald source missing")?;
    for class in ["MoodComponent","ConversationComponent"] {
        let a=world.entities[source].components.iter().find(|c|c.class_name==class).context("Source component missing")?;
        let b=world.entities[clone].components.iter().find(|c|c.class_name==class).context("Clone component missing")?;
        ensure!(a.properties.len()==b.properties.len() && a.properties.iter().zip(&b.properties).all(|(a,b)|a.name==b.name && a.class_name==b.class_name && a.value==b.value),"Clone lost {class} properties");
    }
    ensure!(world.story_sequences.registered_resources(source)==world.story_sequences.registered_resources(clone),"Oswald clone conversation registrations differ");
    let speaker=world.entities.iter().enumerate().find(|(index,entity)|!world.story_sequences.registered_resources(*index).is_empty() && entity.name.contains("Caged_Gremlin01")).map(|(index,_)|index).context("Original gremlin conversation owner missing")?;
    let speaker_clone=world.clone_render_entity(speaker,[0.,5.,0.],world.entities[speaker].runtime_rotation,"")?;
    ensure!(!world.story_sequences.registered_resources(speaker_clone).is_empty(),"Spawned speaker has no conversation registration");
    ensure!(world.story_sequences.registered_resources(speaker)==world.story_sequences.registered_resources(speaker_clone),"Spawned speaker changed resource IDs");
    ensure!(world.story_sequences.resources.len()==resource_count,"Cloning duplicated BSQ resources");
    let sequence=world.story_sequences.registered_resources(speaker_clone)[0];
    let key=world.story_sequences.resources[sequence].key.clone();
    world.force_story_sequence(speaker_clone,&key)?;
    ensure!(world.story_sequences.queue.iter().any(|request|request.entity==speaker_clone && request.resource==sequence),"Clone did not own queued sequence");
    world.retire_entity(speaker_clone)?;
    ensure!(!world.story_sequences.registered_resources(speaker).is_empty() && world.story_sequences.registered_resources(speaker_clone).is_empty(),"Clone retirement removed original registration or leaked clone");
    println!("PASS: original Oswald actor spawns with animation, body, AI and retained mood data; original gremlin clone owns shared conversation resources and independent lifecycle");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_radius_sampling_consumes_rejected_draws_and_keeps_all_lanes() {
        let mut random = super::super::super::game_random::State::default();
        let point = radius_direction(&mut random);
        assert!(point.iter().all(|v| (-1.0..=1.0).contains(v)));
        assert!(point.iter().map(|v| v * v).sum::<f32>() <= 1.);
        assert!(random.draws >= 3 && random.draws % 3 == 0);
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_parented_projector_screen_clone_starts_unattached() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = load(
            Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .entities
            .iter()
            .position(|e| e.name.ends_with(".SpawnerPortalWarpFX"))
            .context("Warp spawner absent")?;
        let component = world.entities[owner]
            .components
            .iter()
            .find(|c| c.class_name == "TemplateSpawnerComponent")
            .unwrap();
        let id = ids(component, "Objects To Spawn")?[2];
        let source = world
            .entities
            .iter()
            .position(|e| e.scene == world.entities[owner].scene && e.id == id)
            .context("Screen prototype absent")?;
        let parent = world.entities[source]
            .transform_parent_id()?
            .context("Original inactive screen must be parented")?;
        let source_pose = world.entities[source].transform;
        let source_parent = world
            .transform_hierarchy
            .get(&source)
            .context("Original parent hierarchy absent")?
            .parent;
        let name = world.entities[owner].name.clone();
        world.run_callback_fixture(&format!(
            "local s=GetEntity({name:?}); SetPropertyInt(s,'Index To Spawn',2); ForceSpawn(s,1)"
        ))?;
        let clone = *world.spawners[&owner]
            .live
            .last()
            .context("No spawned screen")?;
        assert_ne!(clone, source);
        assert_eq!(world.entities[clone].transform_parent_id()?, None);
        assert!(!world.transform_hierarchy.contains_key(&clone));
        assert_eq!(
            world.entities[clone].transform[12..15],
            world.entities[owner].transform[12..15]
        );
        assert_eq!(world.entities[source].transform_parent_id()?, Some(parent));
        assert_eq!(world.entities[source].transform, source_pose);
        assert_eq!(world.transform_hierarchy[&source].parent, source_parent);
        let before = world.entities.len();
        world.run_callback_fixture(&format!("ForceSpawn(GetEntity({name:?}),1)"))?;
        assert_eq!(
            world.entities.len(),
            before + 1,
            "Repeated cloning inherited the previous clone's parent state"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_projector_force_spawn_creates_independent_render_object() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = load(
            Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .entities
            .iter()
            .position(|e| e.name.ends_with(".SpawnerPortalWarpFX"))
            .context("Warp spawner is absent")?;
        let before = world.entities.len();
        let draws = world.draws.len();
        let random = world.game_random.draws;
        let name = &world.entities[owner].name;
        world.run_callback_fixture(&format!("local s=GetEntity({name:?}); Disable(s); SetPropertyInt(s,'Index To Spawn',0,0); local result=ForceSpawn(s,1); assert(type(result)=='table' and #result==1); assert(result[1]~=0); assert(#ForceSpawn(s,0)==0); assert(select('#',ForceSpawn('missing',1))==0)"))?;
        assert_eq!(
            world.spawners[&owner].disabled,
            Some(true),
            "Forced path must bypass and retain the disabled bit"
        );
        assert_eq!(world.entities.len(), before + 1);
        assert!(world.draws.len() > draws);
        assert!(world.game_random.draws > random);
        assert_eq!(world.spawners[&owner].live, vec![before]);
        assert!(world.entity_event_queue.entries().iter().any(|entry|
            matches!(&entry.event,crate::blotworx::entity_events::Event::Basic(event)
                if event.event_kind==0x18 && event.source==Some(owner) && event.header==Some(before))));
        assert_eq!(
            world.entities[before].transform[12..15],
            world.entities[owner].transform[12..15]
        );
        assert!(!world.hidden_entities.contains(&before));
        let group = field(
            world.entities[owner]
                .components
                .iter()
                .find(|c| c.class_name == "TemplateSpawnerComponent")
                .unwrap(),
            "Spawn Group",
        )
        .and_then(|v| v.as_str())
        .context("Original spawn group is absent")?;
        assert!(world
            .scripts
            .group_entity_names(group)?
            .contains(&world.entities[before].name));
        let object = world.entities[owner]
            .components
            .iter()
            .find(|c| c.class_name == "TemplateSpawnerComponent")
            .unwrap();
        let source = ids(object, "Objects To Spawn")?[0];
        let source = world
            .entities
            .iter()
            .position(|e| e.id == source && e.scene == world.entities[owner].scene)
            .context("Prototype absent")?;
        assert!(
            Arc::ptr_eq(
                &world.draws[draws].v_data,
                &world.draws[world.entities[source].draws.start].v_data
            ),
            "Spawn duplicated immutable geometry"
        );
        world.recover_failed_preview_script(0);
        let camera = world
            .entities
            .iter()
            .position(|e| e.name.ends_with(".ExitProjectorCamera"))
            .context("Projector camera is absent")?;
        world.select_nif_camera(Some(camera), None, 1, 0.)?;
        let since = world.report.unsupported.len();
        world
            .report
            .unsupported
            .push("Game Lua: Mickey_Projector_Exit: Audio event unsupported".into());
        assert!(world.recover_failed_preview_script(since));
        assert!(
            world.nif_camera.is_none(),
            "External binding failure retained the cutscene camera"
        );
        Ok(())
    }
}
