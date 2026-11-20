//! Component initialization for newly published entities only.
use super::*;
impl World {
    fn bind_resource_phantom(
        &mut self,
        index: usize,
        resource: super::super::phantom_assets::Resource,
        filter: u32,
        nif_path: &str,
        hkx_path: &str,
    ) -> Result<()> {
        let entity = &self.entities[index];
        let shapes = resource.shapes;
        let reference = resource.reference;
        let shape = resource.shape_identity;
        let bounds_cache = resource.bounds;
        ensure!(
            !self.resource_phantom_shapes.contains_key(&index),
            "Multiple resource phantoms on one entity"
        );
        let motion =
            super::super::native_motion_step::MotionState::shape_phantom_initial(
                entity.runtime_rotation,
                entity.transform[12..15].try_into().unwrap(),
            )?;
        let mut monitor = super::super::hk_reference::Monitor {
            mutex: &self.pickup_reference_monitor,
            held: None,
        };
        let handle = self.physics_objects.allocate_simple_phantom(
            shape,
            filter,
            &mut monitor,
        )?;
        let system = self.physics_objects.allocate_phantom_system()?;
        self.physics_objects
            .system_add_phantom(system, handle, &mut monitor)?;
        self.physics_objects.bind_phantom_entity(
            handle,
            index
                .checked_add(1)
                .context("Physics entity token overflow")?,
        )?;
        self.resource_phantom_handles.insert(index, handle);
        self.phantom_systems.insert(index, system);
        self.report.geometry.push(serde_json::json!({"kind":"resource_phantom_shapes","entity":entity.name,
        "nif":nif_path,"hkx":hkx_path,"parts":shapes.len(),
        "shape_section":reference.section,"shape_offset":reference.offset,
        "constructor_transform":motion.transform,
        "convex_radii":shapes.iter().map(|s|s.triangle_radius).collect::<Vec<_>>(),
        "phantom":handle,"physics_system":system,"shape_reference":shape,"filter_info":filter,
        "allocation_initialized":true,"physics_world_admitted":false,"overlap_dispatch":false}));
        self.resource_phantom_shapes.insert(index, shapes);
        self.resource_phantom_motion.insert(index, motion);
        self.resource_phantom_bounds.insert(index, bounds_cache);
        Ok(())
    }

    pub(super) fn bind_cloned_phantom(&mut self, source: usize, target: usize) -> Result<()> {
        let Some(shapes) = self.resource_phantom_shapes.get(&source) else {
            return Ok(());
        };
        let resource = self.phantom_resources.find_shapes(shapes)
            .context("Source phantom resource missing from world cache")?;
        let component = self.entities[target].components.iter().find(|c| c.class_name == "PhysicsPhantomComponent")
            .context("Clone missing its resource phantom component")?;
        let filter = super::super::phantom_bounds::authored_size_filter(component)?;
        let path = |name| component.properties.iter().find(|p| p.name == name)
            .and_then(|p| p.value.as_str()).unwrap_or_default().to_owned();
        let nif = path("Phantom NIF File");
        let hkx = path("Phantom HKX File");
        self.bind_resource_phantom(target, resource, filter, &nif, &hkx)
    }

    /// Scene model metadata is decoded once per asset identity, but the world
    /// outlives streamed scenes. Keep only identities still referenced by a
    /// live entity so a sequence of scene transitions does not accumulate
    /// every NIF hierarchy and JPSJigsaw graph visited during the session.
    pub(super) fn prune_unused_scene_model_metadata(&mut self, retired: usize) -> (bool, bool) {
        let Some(entity) = self.entities.get(retired) else {
            return (false, false);
        };
        let model = entity.model_asset.clone();
        let jigsaw = entity.jigsaw_asset.clone();
        let still_uses_model = model.as_ref().is_some_and(|asset| {
            self.entities.iter().enumerate().any(|(index, entity)| {
                index != retired
                    && !self.removed_entities.contains(&index)
                    && entity.model_asset.as_ref() == Some(asset)
            })
        });
        let still_uses_jigsaw = jigsaw.as_ref().is_some_and(|asset| {
            self.entities.iter().enumerate().any(|(index, entity)| {
                index != retired
                    && !self.removed_entities.contains(&index)
                    && entity.jigsaw_asset.as_ref() == Some(asset)
            })
        });
        let removed_bounds = model.is_some_and(|asset| {
            if still_uses_model {
                return false;
            }
            self.model_bounds.remove(&asset).is_some()
        });
        let removed_jigsaws = jigsaw.is_some_and(|asset| {
            if still_uses_jigsaw {
                return false;
            }
            self.jigsaws.remove(&asset).is_some()
        });
        if removed_bounds {
            self.model_bounds.shrink_to_fit();
        }
        if removed_jigsaws {
            self.jigsaws.shrink_to_fit();
        }
        (removed_bounds, removed_jigsaws)
    }

    /// Keep decoded NIF palettes only while a live authored actor or Mickey
    /// needs them. Streamed scenes can add skins after initial loading, and
    /// entity retirement must release a model once its final actor is gone.
    pub(super) fn prune_unused_skin_payloads(&mut self) -> (usize, usize) {
        let mut active_skin_models = self.authored_havok.skin_model_names();
        active_skin_models.extend(self.player_meshes.iter().map(|mesh| mesh.asset.clone()));
        let released_bytes = self
            .skins
            .iter()
            .filter(|(model, _)| !active_skin_models.contains(*model))
            .flat_map(|(_, skins)| skins)
            .map(super::super::skin::owned_buffer_bytes)
            .sum::<usize>();
        let released_models = self
            .skins
            .keys()
            .filter(|model| !active_skin_models.contains(*model))
            .count();
        self.skins
            .retain(|model, _| active_skin_models.contains(model));
        (released_bytes, released_models)
    }

    fn initialize_rigid_trigger_hulls(
        &mut self,
        range: std::ops::Range<usize>,
        files: &[VirtualFile],
    ) {
        let mut resources: BTreeMap<&str, Arc<Vec<super::super::collision::StaticMesh>>> = BTreeMap::new();
        for index in range {
            let entity = &self.entities[index];
            if !entity.components.iter().any(|component| {
                matches!(component.class_name.as_str(), "TriggerVolumeComponent" | "TriggerDamageComponent")
            }) {
                continue;
            }
            let Some(path) = entity.components.iter()
                .filter(|component| component.class_name == "PhysicsRigidBodyPhantomShapeComponent")
                .flat_map(|component| &component.properties)
                .find(|property| property.name == "RB Hull File Path")
                .and_then(|property| property.value.as_str())
                .filter(|path| !path.is_empty())
            else {
                continue;
            };
            let result = (|| -> Result<_> {
                let file = lookup(files, path)
                    .with_context(|| format!("Missing trigger hull {path}"))?;
                let parts = match resources.entry(file.path.as_str()) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.get().clone(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        let hkx = HkxFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
                        entry.insert(std::sync::Arc::new(
                            super::super::collision::body_local_meshes(&hkx)?
                        )).clone()
                    }
                };
                super::super::phantom_bounds::RigidTriggerHull::new(parts, entity.physics_transform())
            })();
            match result {
                Ok(hull) => { self.rigid_trigger_hulls.insert(index, hull); }
                Err(error) => self.report.unsupported.push(format!("Trigger hull {}: {error:#}", entity.name)),
            }
        }
    }

    pub(super) fn initialize_component_range(
        &mut self,
        range: std::ops::Range<usize>,
        files: &[VirtualFile],
        root: &Path,
    ) -> Result<()> {
        ensure!(
            range.start <= range.end && range.end <= self.entities.len(),
            "Invalid component initialization range"
        );
        self.story_sequences.admit_range(&self.entities,range.clone(),|path| {
            Ok(lookup(files,path).map(|file|(file.path.clone(),file.data.clone())))
        })?;
        self.demo_story.load_strings(files);
        if self.entities[range.clone()].iter().any(|entity|entity.components.iter().any(|c|c.class_name=="MoodComponent")) {
            let limitation="Mood counter, limits, thresholds and watch-list properties are retained per entity, including clones. Native watching, state enter/exit reactions and mood-driven animation remain unimplemented; these actors do not yet have full mood behavior.";
            if !self.report.unsupported.iter().any(|line|line==limitation) {self.report.unsupported.push(limitation.into());}
        }
        // Initial loading has already parsed these graphs. Incremental admission
        // handles incoming scenes and avoids decoding cached paths on clones.
        self.ai_navigation.admit(&self.entities[range.clone()],files,root);
        for index in range.clone() {
            if let Err(error) = self
                .shadow_volumes
                .admit(index, &self.entities[index].components)
            {
                self.report.unsupported.push(format!(
                    "Shadow component {}: {error:#}",
                    self.entities[index].name
                ));
            }
        }
        self.initialize_spawners(range.clone())?;
        self.initialize_logic_timers(range.clone())?;
        self.initialize_character_bodies(range.clone());
        self.initialize_multi_position_audio(range.clone());
        self.initialize_rigid_trigger_hulls(range.clone(), files);
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            for component in entity
                .components
                .iter()
                .filter(|component| component.class_name == "MickeyPlayerMotionControllerComponent")
            {
                ensure!(
                    !self.ledge_controls.contains_key(&index),
                    "Multiple Mickey motion controllers on {}",
                    entity.name
                );
                let state = super::super::ledge_control::State::from_component(component)
                    .with_context(|| {
                        format!("Ledge controller {}:{}", entity.scene, entity.name)
                    })?;
                self.ledge_controls.insert(index, state);
            }
        }
        if !self.ledge_controls.is_empty() {
            let message="Mickey ledge prerequisite flags and script gate are connected; native cast selection, context flags, hanging/sidle/climb states and animation/physics integration remain unimplemented.";
            if !self.report.unsupported.iter().any(|entry| entry == message) {
                self.report.unsupported.push(message.into());
            }
        }
        let mut authored_havok = std::mem::take(&mut self.authored_havok);
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            authored_havok.prepare(self, range.clone(), files)
        }));
        self.authored_havok = authored_havok;
        match prepared {
            Ok(errors) => self.report.unsupported.extend(errors),
            Err(panic) => std::panic::resume_unwind(panic),
        }
        // Runtime skinning for authored Havok actors reads `World::skins` on
        // every update. The importer also decoded skins for ordinary static
        // models; those arrays have no runtime consumer after this phase.
        // Keep only model palettes referenced by successfully initialized
        // actor instances. Streamed scenes add their own skins before running
        // this same initialization path.
        let (released_skin_bytes, released_skin_models) = self.prune_unused_skin_payloads();
        #[cfg(feature = "blotworx-memory-profile")]
        {
            eprintln!(
                "[load-memory] unused static-model skin payload released: {:.1} MiB across {} assets",
                released_skin_bytes as f64 / 1_048_576.,
                released_skin_models
            );
        }
        #[cfg(not(feature = "blotworx-memory-profile"))]
        let _ = (released_skin_bytes, released_skin_models);
        ensure!(
            self.player_meshes
                .iter()
                .all(|mesh| self.skins.contains_key(&mesh.asset)),
            "Player mesh skin payload was not retained"
        );
        if self.entities[range.clone()].iter().any(|entity| {
            entity
                .components
                .iter()
                .any(|c| c.class_name == "UsableComponent")
        }) {
            let message="Usable target selection, G/X player input, deferred native OnUse output routing, shared use-once component disablement and conversation-sequence admission are connected; native aim prioritization, indicator effects and full dialogue presentation remain incomplete.";
            if !self.report.unsupported.iter().any(|line| line == message) {
                self.report.unsupported.push(message.into());
            }
        }
        if self.entities[range.clone()].iter().any(|entity| {
            entity
                .components
                .iter()
                .any(|c| c.class_name == "TemplateSpawnerComponent")
        }) {
            let message="TemplateSpawner scalar property writes are connected; constructor enablement, task scheduling, template creation and spawned group registration remain unimplemented.";
            if !self.report.unsupported.iter().any(|line| line == message) {
                self.report.unsupported.push(message.into());
            }
        }
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            for component in entity
                .components
                .iter()
                .filter(|c| c.class_name == "PaintThinnerCounterComponent")
            {
                ensure!(
                    !self.paint_counters.contains_key(&index),
                    "Multiple paint/thinner counters on {}",
                    entity.name
                );
                let counter = super::super::paint_counter::Counter::from_component(component)
                    .with_context(|| {
                        format!("Paint/thinner counter {}:{}", entity.scene, entity.name)
                    })?;
                self.report.geometry.push(serde_json::json!({"kind":"paint_thinner_counter","entity":entity.name,
                "counter":counter,"status":counter.status(),"stimulus_and_regeneration_connected":false}));
                if self.demo_story.enabled {
                    self.demo_story.counters.states.insert(index, counter.status());
                }
                self.paint_counters.insert(index, counter);
                self.counter_materials.insert(index, Default::default());
            }
        }
        if !self.paint_counters.is_empty() {
            self.report.unsupported.push("Counter scalar setters and material initialization/update are connected. Stimulus delivery, regeneration, hit callbacks and receiver delivery remain incomplete; material diagnostics are reported separately.".into());
        }
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            for component in entity
                .components
                .iter()
                .filter(|c| c.class_name == "AIBrainComponent")
            {
                ensure!(
                    !self.ai_brains.contains_key(&index),
                    "Multiple AI brains on {}",
                    entity.name
                );
                let mut brain = super::super::ai_behavior_events::Brain::from_component(component)
                    .with_context(|| format!("AI brain {}:{}", entity.scene, entity.name))?;
                for conversation in brain.conversations.values_mut() {
                    conversation.configure(&brain.ai_type, &mut self.runtime_settings)
                        .with_context(||format!("Conversation attributes {}:{}",entity.scene,entity.name))?;
                }
                if brain.behaviors.iter().any(|name| name == "RobotArmGeneral") {
                    self.robot_arm_controls.entry(index).or_default().configure(&brain.ai_type, &mut self.runtime_settings)?;
                }
                self.cutscene_actions
                    .bind(index, &brain, &mut self.entity_event_registry)?;
                if brain.behaviors.iter().any(|name|name=="RobotArmGeneral") {
                    self.cutscene_actions.bind_robot_arm(index,&mut self.entity_event_registry)?;
                }
                self.report.unsupported.push(format!("AI behavior tree {}:{} ({}; {} slots): {} Paused and {} ScriptedIdle receivers imported; other callbacks, selection and controller output pending",
                entity.scene,entity.name,brain.ai_type,brain.behaviors.len(),brain.paused.len(),brain.scripted_idle.len()));
                self.ai_brains.insert(index, brain);
                self.ai_registry.register(index);
            }
        }
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            let count = entity
                .components
                .iter()
                .filter(|c| c.class_name == "LODComponent")
                .count();
            if count == 1 {
                self.lod_states
                    .insert(index, super::super::lod::State::constructed());
            } else {
                ensure!(
                    count == 0,
                    "Multiple LOD components on {} require native component-slot selection",
                    entity.name
                );
            }
        }
        if !self.lod_states.is_empty() {
            self.report.unsupported.push(format!("{} LOD constructor states and relocation resets installed; camera-distance initialization, event subscriptions, scheduled visibility/enable updates remain unimplemented.",self.lod_states.len()));
        }
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            for component in entity
                .components
                .iter()
                .filter(|c| c.class_name == "InventoryComponent")
            {
                let result = (|| -> Result<()> {
                    ensure!(
                        !self.authored_inventory.contains_key(&index),
                        "Multiple inventory components on one entity"
                    );
                    let state = super::super::inventory_state::Authored::from_component(component)?;
                    self.report.geometry.push(
                        serde_json::json!({"kind":"authored_inventory","entity":entity.name,
                    "state":state,"runtime_initialization":false}),
                    );
                    self.authored_inventory.insert(index, state);
                    Ok(())
                })();
                if let Err(error) = result {
                    self.report
                        .unsupported
                        .push(format!("Inventory state {}: {error:#}", entity.name));
                }
            }
        }
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            for component in entity
                .components
                .iter()
                .filter(|c| c.class_name == "HealthComponent")
            {
                let result = (|| -> Result<()> {
                    ensure!(
                        !self.authored_health.contains_key(&index),
                        "Multiple health components on one entity"
                    );
                    let state = super::super::health::Authored::from_component(component)?;
                    self.report.geometry.push(
                        serde_json::json!({"kind":"authored_health","entity":entity.name,
                    "state":state,"runtime_initialization":false}),
                    );
                    self.runtime_health
                        .insert(index, super::super::health::Runtime::new(&state));
                    self.authored_health.insert(index, state);
                    Ok(())
                })();
                if let Err(error) = result {
                    self.report
                        .unsupported
                        .push(format!("Health state {}: {error:#}", entity.name));
                }
            }
        }
        self.report.unsupported.push("Ability ownership currently retains reconstructed Blast/Brush/Spin constructor fields only; config/save initialization, remaining actions and property upgrade selection remain unimplemented; live Brush/Spin ownership gates are connected. Give command state updates do not imply fully functional abilities.".into());
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            for component in entity
                .components
                .iter()
                .filter(|c| c.class_name == "PlayerPickupComponent")
            {
                let result = (|| -> Result<()> {
                    ensure!(
                        !self.pickup_receivers.contains_key(&index),
                        "Multiple pickup receivers on one entity"
                    );
                    let receiver = super::super::pickup::Receiver::from_component(component)?;
                    self.report.geometry.push(serde_json::json!({"kind":"pickup_receiver","entity":entity.name,
                    "commands":receiver.commands,"physics_event_producer":false,"command_dispatch":false}));
                    self.pickup_receivers.insert(index, receiver);
                    Ok(())
                })();
                if let Err(error) = result {
                    self.report
                        .unsupported
                        .push(format!("Pickup receiver {}: {error:#}", entity.name));
                }
            }
        }
        if !self.pickup_receivers.is_empty() {
            for (&index, _) in self
                .pickup_receivers
                .iter()
                .filter(|(index, _)| range.contains(index))
            {
                let entity = &self.entities[index];
                let component = entity
                    .components
                    .iter()
                    .find(|c| c.class_name == "PlayerPickupComponent")
                    .unwrap();
                match super::super::pickup::Settings::from_component(component) {
                    Ok(settings) => {
                        self.report.geometry.push(serde_json::json!({"kind":"pickup_settings","entity":entity.name,
                        "settings":settings,"constructor_fields":true,"runtime_initialization":false,"update_scheduled":false}));
                        self.pickup_settings.insert(index, settings);
                        self.pickup_update_states.insert(index, Default::default());
                    }
                    Err(error) => self
                        .report
                        .unsupported
                        .push(format!("Pickup settings {}: {error:#}", entity.name)),
                }
            }
            self.report.unsupported.push(format!("{} pickup overlap receivers loaded; physics overlap production, command dispatch and pickup lifecycle remain unimplemented.",self.pickup_receivers.len()));
        }
        for index in range.clone() {
            if self.resource_phantom_shapes.contains_key(&index) {
                continue;
            }
            let components = self.entities[index].components.iter()
                .filter(|c| c.class_name == "PhysicsPhantomComponent")
                .cloned()
                .collect::<Vec<_>>();
            for component in &components {
                let result = (|| -> Result<()> {
                    let resource_path = |name: &str| -> Result<&str> {
                        let properties: Vec<_> = component
                            .properties
                            .iter()
                            .filter(|p| p.name == name)
                            .collect();
                        ensure!(properties.len() <= 1, "Duplicate {name}");
                        let path = properties
                            .first()
                            .map(|p| p.value.as_str().with_context(|| format!("Invalid {name}")))
                            .transpose()?
                            .unwrap_or("");
                        ensure!(!path.contains('\0'), "Embedded NUL in {name}");
                        Ok(path)
                    };
                    let nif_path = resource_path("Phantom NIF File")?;
                    let hkx_path = resource_path("Phantom HKX File")?;
                    if !nif_path.is_empty() && !hkx_path.is_empty() {
                        let nif = lookup(&files, nif_path)
                            .with_context(|| format!("Missing phantom NIF {nif_path}"))?;
                        epic_mickey_lib_rs::nif_file::NifFile::load_bytes(&nif.data)
                            .map_err(anyhow::Error::msg)?;
                        let file = lookup(&files, hkx_path)
                            .with_context(|| format!("Missing phantom HKX {hkx_path}"))?;
                        let resource = self.phantom_resources.acquire(
                            &file.path,
                            &file.data,
                            &mut self.physics_objects,
                        )?;
                        let filter = super::super::phantom_bounds::authored_size_filter(component)?;
                        self.bind_resource_phantom(index, resource, filter, &nif.path, &file.path)?;
                        return Ok(());
                    }
                    let Some(volume) =
                        super::super::phantom_bounds::SizeVolume::from_component(component)?
                    else {
                        return Ok(());
                    };
                    ensure!(
                        !self.size_phantoms.contains_key(&index),
                        "Multiple size phantoms on one entity"
                    );
                    let entity = &self.entities[index];
                    let bounds = volume.bounds(
                        entity.transform[12..15].try_into().unwrap(),
                        entity.runtime_rotation,
                    )?;
                    self.report.geometry.push(
                        serde_json::json!({"kind":"size_phantom_bounds","entity":entity.name,
                    "size":volume.size,"bounds":bounds,"overlap_dispatch":false}),
                    );
                    self.size_phantoms.insert(index, volume);
                    match super::super::phantom_bounds::authored_size_filter(component) {
                        Ok(filter) => {
                            let padded = volume.padded_bounds(
                                entity.transform[12..15].try_into().unwrap(),
                                entity.runtime_rotation,
                            )?;
                            let system = self.physics_objects.allocate_phantom_system()?;
                            let handle =
                                self.physics_objects.allocate_size_phantom(padded, filter)?;
                            let mut monitor = super::super::hk_reference::Monitor {
                                mutex: &self.pickup_reference_monitor,
                                held: None,
                            };
                            self.physics_objects.system_add_phantom(
                                system,
                                handle,
                                &mut monitor,
                            )?;
                            self.phantom_systems.insert(index, system);
                            // Same opaque entity-token convention as the Lua
                            // registry; never interpreted as a collidable address.
                            let token = index
                                .checked_add(1)
                                .context("Physics entity token overflow")?;
                            self.physics_objects.bind_phantom_entity(handle, token)?;
                            self.size_phantom_handles.insert(index, handle);
                            self.size_phantom_filters.insert(index, filter);
                            self.report.geometry.push(serde_json::json!({"kind":"size_phantom_filter","entity":entity.name,
                            "filter_info":filter,"allocation_size":super::super::phantom_bounds::SIZE_PHANTOM_MEMORY,"runtime_initialization":false}));
                        }
                        Err(error) => self
                            .report
                            .unsupported
                            .push(format!("Phantom filter {}: {error:#}", entity.name)),
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    self.report
                        .unsupported
                        .push(format!("Phantom bounds {}: {error:#}", self.entities[index].name));
                }
            }
        }
        if !self.size_phantoms.is_empty() {
            self.report.unsupported.push(format!("{} size-based phantom bounds and {} resource phantom allocations reconstructed. Pickup listeners attach to their owning phantom systems; world admission, filtering and pickup/trigger production remain incomplete; these volumes are not active gameplay sensors.",self.size_phantoms.len(),self.resource_phantom_handles.len()));
        }
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            if let Some(hierarchy) = entity
                .model_asset
                .as_ref()
                .and_then(|asset| self.model_bounds.get(asset))
            {
                for (&node, _) in hierarchy
                    .nodes
                    .iter()
                    .filter(|(_, node)| node.authored_bound.is_some())
                {
                    let mut object = super::super::render_object::State::default();
                    if let Some(effects) = entity.components.iter().find(|c| c.class_name == "R3MTEffectsComponent") {
                        for slot in 0..4u8 {
                            let key = format!("R3MTConstantColor_{slot}");
                            if effects.properties.iter().any(|p| p.name == format!("{key}_Enable") && p.value.as_bool() == Some(true)) {
                                let value = &effects.properties.iter().find(|p| p.name == key).context("Enabled component color missing")?.value;
                                let mut color = [0.; 4];
                                for (i, channel) in ["r", "g", "b", "a"].iter().enumerate() {
                                    color[i] = value.get(*channel).and_then(|v| v.as_f64()).context("Invalid component color")? as f32;
                                }
                                object.publish(slot, super::super::paint_counter::pack_color(color)?)?;
                            }
                        }
                    }

                    if entity.components.iter().any(|component| {
                        component.class_name == "MickeyPlayerMotionControllerComponent"
                    }) {
                        //88998..889ac traverses this controller's model. This is
                        // runtime NiRenderObject state, not serialized AV flags.
                        object.flags.mark_player_depth_target();
                    }
                    ensure!(
                        self.render_objects.insert((index, node), object).is_none(),
                        "Render object already initialized"
                    );
                }
                if let Some(mut instance) = super::super::nif_runtime::Instance::load(hierarchy)
                    .with_context(|| format!("Model playback {}:{}", entity.scene, entity.name))?
                {
                    if let Some(component)=entity.components.iter().find(|c|c.class_name=="JPSSceneGraphComponent") {
                        let boolean=|name:&str,default:bool|->Result<bool> {
                            component.properties.iter().find(|p|p.name==name).map(|p|p.value.as_bool().with_context(||format!("Invalid scene graph {name}"))).transpose().map(|value|value.unwrap_or(default))
                        };
                        let mut clock=super::super::nif_sequence::ComponentClock::default();
                        //203624 defaults Use Delta Time=true;350b40 seeds the
                        //optional StartAnimationUsingGlobalTime read with false.
                        clock.use_delta_time=boolean("Use Delta Time",true)?;
                        clock.initialize(boolean("StartAnimationUsingGlobalTime",false)?,self.animation_time as f64);
                        instance.update(hierarchy,clock.initial_update_time())?;
                        instance.component_clock=Some(clock);
                    }
                    self.model_animation.insert(index, instance);
                }
            }
        }
        if !self.model_animation.is_empty() {
            self.report.unsupported.push("Direct NIF transform animation updates rendering and lighting bounds. Sequence-manager animation, animated collision/body binding and original update-order parity remain incomplete.".into());
        }
        let property_key = |name: &str| {
            name.chars()
                .filter(|c| !c.is_whitespace())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        };
        for (&index, resources) in self
            .actor_sequences
            .iter()
            .filter(|(index, _)| range.contains(index))
        {
            if !self.actor_bindings.contains_key(&index) {
                continue;
            }
            let component = self.entities[index]
                .components
                .iter()
                .find(|c| c.class_name == "AnimationComponentGB")
                .context("Actor component missing")?;
            let property = |name: &str| {
                component
                    .properties
                    .iter()
                    .find(|p| property_key(&p.name) == property_key(name))
                    .map(|p| &p.value)
            };
            let mut actor = super::super::actor_runtime::Actor::new(
                property("Use Delta Time")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                property("StartAnimationUsingGlobalTime")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            );
            let id = property("Active Sequence ID")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            actor.retain_target(id);
            // Loading the KFM selects the initial target independently of
            // Start Active (retail REL2f6384..2f639c). Publish its initial
            // pose without advancing clocks or emitting animation events;
            // exported NIF locals can contain the end of a destruction clip.
            // Exact native manager activation/update ordering remains open.
            if resources.get(id).is_some() {
                match self.actor_bindings[&index].sample(id, 0.) {
                    Ok(pose) => {
                        let model = self.entities[index].model_asset.as_ref()
                            .context("Initial actor pose has no model")?;
                        actor.world = self.model_bounds.get(model)
                            .context("Initial actor pose has no hierarchy")?
                            .base_world_transforms_with_poses(None, &pose.transforms)?;
                        actor.pose = Some(pose);
                    }
                    Err(error) => self.report.unsupported.push(format!(
                        "Authored actor initial pose {}: {error:#}",
                        self.entities[index].name
                    )),
                }
            }
            if property("Start Active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                if let Err(error) = actor.request(id, resources, &self.actor_bindings[&index]) {
                    self.report.unsupported.push(format!(
                        "Authored actor startup {}: {error:#}",
                        self.entities[index].name
                    ));
                }
            } else {
                // REL2f565c..2f5674 initializes idle to !StartActive.
                self.idle_actors.insert(index);
            }
            self.report.geometry.push(
                serde_json::json!({"kind":"actor_instance","entity":self.entities[index].name,
            "start_active":property("Start Active").and_then(|v|v.as_bool()).unwrap_or(false),
            "authored_sequence":property("Active Sequence ID").and_then(|v|v.as_u64()).unwrap_or(0),
            "active_sequence":actor.active_id()}),
            );
            self.actors.insert(index, actor);
        }
        for (index, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            let result = (|| -> Result<()> {
                let prefabs = entity
                    .components
                    .iter()
                    .filter(|c| c.class_name == "JPSPrefabComponent")
                    .collect::<Vec<_>>();
                ensure!(
                    prefabs.len() <= 1,
                    "Multiple prefab components on one entity"
                );
                let Some(component) = prefabs.first() else {
                    return Ok(());
                };
                let rows = |name: &str| -> Result<Vec<String>> {
                    let Some(property) = component.properties.iter().find(|p| p.name == name)
                    else {
                        return Ok(Vec::new());
                    };
                    property
                        .value
                        .as_array()
                        .with_context(|| format!("{name} must be an array"))?
                        .iter()
                        .map(|row| {
                            row.as_str()
                                .map(str::to_owned)
                                .with_context(|| format!("{name} row must be a string"))
                        })
                        .collect()
                };
                self.scripts.prepare_prefab_data(
                    index,
                    &rows("Prefab Data")?,
                    &rows("Prefab Data Override")?,
                )
            })();
            if let Err(error) = result {
                self.report
                    .unsupported
                    .push(format!("Prefab Lua data {}: {error:#}", entity.name));
            }
        }
        let identities = self
            .entities
            .iter()
            .enumerate()
            .map(|(index, entity)| ((entity.scene.clone(), entity.id), index))
            .collect::<HashMap<_, _>>();
        for (owner, entity) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            for component in entity
                .components
                .iter()
                .filter(|c| c.class_name == "JPSPrefabComponent")
            {
                for property in component
                    .properties
                    .iter()
                    .filter(|p| p.name == "Prefab Entities")
                {
                    let result = (|| -> Result<()> {
                        for id in property
                            .value
                            .as_array()
                            .context("Prefab Entities must be an array")?
                        {
                            let id = u32::try_from(
                                id.as_u64().context("Invalid prefab member identity")?,
                            )?;
                            // Cooked membership arrays retain null/deleted slots.
                            // Native entity ID zero is null, never a runtime index.
                            if id == 0 {
                                continue;
                            }
                            let member = *identities
                                .get(&(entity.scene.clone(), id))
                                .with_context(|| format!("Prefab member {id} is not loaded"))?;
                            self.scripts.bind_prefab_member(member, owner)?;
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        self.report
                            .unsupported
                            .push(format!("Prefab Lua ownership {}: {error:#}", entity.name));
                    }
                }
            }
        }
        self.initialize_pickup_listeners_range(range.clone())?;
        // Openfile inside original modules resolves cooked resources, including
        // startup helpers without their own placed ScriptComponent.
        for file in files
            .iter()
            .filter(|file| key(&file.path).ends_with(".lua"))
        {
            self.scripts.import_source(&file.path, &file.data)?;
        }
        let mut script_paths = HashSet::new();
        let mut modules: Vec<(String, std::sync::Arc<[u8]>)> = Vec::new();
        if range.start == 0 {
            let path = "Environments/_Shared/Scripts/globalscripts.lua";
            lookup(files, path).context("Missing shipped global Lua definitions")?;
            script_paths.insert(key(path));
            modules.push((path.to_owned(), self.scripts.imported_source(path)?.context("Global script resource not imported")?));
        }
        for entity in &self.entities[range.clone()] {
            for component in entity
                .components
                .iter()
                .filter(|c| c.class_name == "ScriptComponent")
            {
                for property in component
                    .properties
                    .iter()
                    .filter(|p| property_key(&p.name) == "luafilepath")
                {
                    for path in super::super::actor_scripts::authored_paths(&property.value)
                        .with_context(|| format!("Script paths on {}", entity.name))?
                    {
                        let path = super::super::resource_path::canonical(&path)?;
                        if !script_paths.insert(key(&path)) {
                            continue;
                        }
                        let bytes = if let Some(bytes) = self.scripts.imported_source(&path)? {
                            bytes
                        } else if let Some(file) = lookup(&files, &path) {
                            std::sync::Arc::from(file.data.as_slice())
                        } else {
                            std::sync::Arc::from(std::fs::read(root.join(&path))
                                .with_context(|| format!("Missing game script {path}"))?)
                        };
                        modules.push((path, bytes));
                    }
                }
            }
        }
        for (entity, runtime) in self
            .entities
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
        {
            for component in runtime
                .components
                .iter()
                .filter(|c| c.class_name == "GameEventsComponent")
            {
                self.component_events
                    .bind(entity, component)
                    .with_context(|| {
                        format!("GameEvents binding {}:{}", runtime.scene, runtime.name)
                    })?;
                if component
                    .properties
                    .iter()
                    .any(|p| p.name == "ListenForChildren" && p.value.as_bool() == Some(true))
                {
                    self.report.unsupported.push(format!(
                        "GameEvents child-source remapping pending: {}:{}",
                        runtime.scene, runtime.name
                    ));
                }
            }
        }
        for entity in range.clone() {
            if let Err(error) = self.bind_entity_lifetime(entity) {
                self.report.unsupported.push(format!(
                    "Lifetime initialization {}: {error:#}",
                    self.entities[entity].name
                ));
            }
        }
        // Resolve body-level scene filters before admitting compatibility bodies.
        // Per-child filter and quality-specific solver agents still need Havok
        // compatibility; do not label this as reference-validated simulation.
        for owner in self.entities[range.clone()]
            .iter()
            .flat_map(|entity| entity.collision_parts.clone())
            .map(|part| self.report.collision_parts[part].owner)
        {
            let word = self.initial_static_simulation_filter(owner)?;
            for shape in self.collision.owner_shapes(owner).collect::<Vec<_>>() {
                self.collision.set_simulation_filter(shape, word)?;
            }
        }
        for (&owner, source) in &self.jigsaw_owners {
            if !range.contains(&self.jigsaw_instances[source.instance].entity) {
                continue;
            }
            let word = self.initial_static_simulation_filter(owner)?;
            for shape in self.collision.owner_shapes(owner).collect::<Vec<_>>() {
                self.collision.set_simulation_filter(shape, word)?;
            }
        }
        self.bind_transform_hierarchy_range(range.clone())?;
        self.bind_rotators_range(range.clone())?;
        self.initialize_npc_motion_turn_rates(range.clone())?;
        self.initialize_basic_ai_controllers(range.clone());
        self.initialize_patrol_node_routes(range.clone())?;
        self.initialize_native_patrol_targets(range.clone())?;
        self.initialize_counter_transients(range.clone())?;
        self.advance_counter_materials(range.clone());
        for entity in range.clone() {
            if let Err(error) = self.outputs.bind(
                entity,
                &self.entities[entity].name,
                &self.entities[entity].components,
            ) {
                self.report
                    .unsupported
                    .push(format!("Authored IO binding: {error:#}"));
            }
            self.bind_deferred_output_events(entity)?;
        }
        if range.start == 0 {
            self.restore_transition_player()?;
        }
        // Definitions initialize before map-load dispatch, with component bindings
        // and authored IO already available to their immediate native calls.
        for (path, bytes) in modules {
            if self.scripts.module_loaded(&path)? {
                continue;
            }
            if let Err(error) = self.with_script_world_host(World::script_world_api, |scripts| {
                scripts.ensure_global_module(&path, &bytes)
            }) {
                self.report
                    .unsupported
                    .push(format!("Game script {path}: {error:#}"));
            }
        }
        Ok(())
    }
}
