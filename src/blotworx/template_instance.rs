//! Palette-root construction through the common decoder and live publisher.
use super::*;
use epic_mickey_lib_rs::scene_file::{Component, ID};
fn strings(value: &serde_json::Value, out: &mut Vec<String>) {
    if let Some(s) = value.as_str() {
        if !s.trim_end_matches('\0').is_empty() {
            out.push(s.trim_end_matches('\0').into());
        }
    } else if let Some(a) = value.as_array() {
        for v in a {
            strings(v, out);
        }
    }
}
fn remap_root(value: &mut serde_json::Value, old: u32, new: u32) -> Result<()> {
    if let Some(a) = value.as_array_mut() {
        for v in a {
            remap_root(v, old, new)?;
        }
    } else {
        let id = value.as_u64().context("Invalid template entity pointer")?;
        ensure!(
            id == 0 || id == u64::from(old),
            "Template external reference {id} requires graph instantiation"
        );
        if id != 0 {
            *value = serde_json::json!(new);
        }
    }
    Ok(())
}
fn set_transform(
    c: &mut Component,
    name: &str,
    value: serde_json::Value,
    scene: &SceneFile,
) -> Result<()> {
    if let Some(p) = c.properties.iter_mut().find(|p| p.name == name) {
        p.value = value;
    } else {
        let defaults = crate::component_schema::CATALOG.components["JPSTransformationComponent"]
            .instantiate("JPSTransformationComponent", scene)?;
        let mut p = defaults
            .properties
            .into_iter()
            .find(|p| p.name == name)
            .context("Missing transform schema")?;
        p.value = value;
        c.properties.push(p);
    }
    Ok(())
}
fn dependency(
    store: &mut super::super::resource_store::Store,
    owner: &str,
    path: &str,
) -> Result<VirtualFile> {
    let path = path.replace('\\', "/");
    let mut candidates = vec![path.clone()];
    if let Some(stem) = path
        .strip_suffix(".gsa")
        .or_else(|| path.strip_suffix(".pal"))
    {
        candidates.insert(0, format!("{stem}.bin"));
    }
    if let Some(stem) = path.strip_suffix(".dds") {
        candidates.insert(0, format!("{stem}_tex.nif_wii"));
    }
    if let Some((dir, _)) = owner.rsplit_once('/') {
        candidates.extend(candidates.clone().into_iter().map(|p| format!("{dir}/{p}")));
    }
    for candidate in candidates {
        if let Ok(file) = store.read(&candidate) {
            return Ok(file);
        }
    }
    anyhow::bail!("Template dependency absent: {owner} -> {path}")
}
impl World {
    pub(super) fn spawn_palette_root(
        &mut self,
        owner: usize,
        name: &str,
        position: V,
        rotation: [f32; 9],
        group: &str,
    ) -> Result<usize> {
        ensure!(
            position.iter().chain(&rotation).all(|v| v.is_finite()),
            "Invalid template placement"
        );
        let scope = self
            .entities
            .get(owner)
            .context("Template owner missing")?
            .scene
            .clone();
        let mut next = self
            .entities
            .iter()
            .flat_map(|e| {
                std::iter::once(e.id).chain(e.components.iter().map(|c| c.link_id.to_u32()))
            })
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .context("Runtime identity exhausted")?;
        let id = next;
        let store = self.named_fx.resource_store();
        let mut files = store.palette_files()?;
        let scenes = files
            .iter()
            .map(|f| {
                Ok((
                    f.path.clone(),
                    super::super::scene_loading::parse_scene(&f.path, &f.data)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let graph = super::super::brush_assets::Assets::declarations(name, &scenes)?;
        let definition = &graph[&name.to_ascii_lowercase()];
        let (_, source) = scenes
            .iter()
            .find(|(p, _)| p == &definition.source_scene)
            .context("Template source missing")?;
        let mut entity = source
            .objects
            .iter()
            .find(|e| e.link_id.to_u32() == definition.entity_id)
            .context("Template root missing")?
            .clone();
        entity.components = definition.components.clone();
        super::super::actor_io::inherit_connections(
            &mut entity.components,
            &super::super::actor_io::component_index(&source.objects),
        )?;
        ensure!(
            !entity
                .components
                .iter()
                .any(|c| c.class_name == "JPSPrefabComponent"),
            "Template prefab member creation is not connected: {name}"
        );
        ensure!(
            definition.children.is_empty(),
            "Template child graph instantiation needs full namespace remapping: {name}"
        );
        if !group.is_empty() {
            let io = entity
                .components
                .iter_mut()
                .find(|c| c.class_name == "InputOutputComponent")
                .context("Spawn group needs InputOutputComponent")?;
            if !io.properties.iter().any(|p| p.name == "EntityGroups") {
                let defaults = crate::component_schema::CATALOG.components["InputOutputComponent"]
                    .instantiate("InputOutputComponent", source)?;
                let mut property = defaults
                    .properties
                    .into_iter()
                    .find(|p| p.name == "EntityGroups")
                    .context("Missing EntityGroups schema")?;
                property.value = serde_json::json!([]);
                io.properties.push(property);
            }
            let groups = io
                .properties
                .iter_mut()
                .find(|p| p.name == "EntityGroups")
                .unwrap()
                .value
                .as_array_mut()
                .context("Invalid spawn groups")?;
            groups.push(serde_json::json!(group));
        }
        let old = entity.link_id.to_u32();
        entity.link_id = ID::new(id as u128);
        entity.master_link_id = ID::new(0);
        entity.name = format!("{} [runtime {id}]", entity.name);
        let transform = entity
            .components
            .iter_mut()
            .find(|c| c.class_name == "JPSTransformationComponent")
            .context("Template has no transform")?;
        set_transform(
            transform,
            "Translation",
            serde_json::json!({"x":position[0],"y":position[1],"z":position[2]}),
            source,
        )?;
        set_transform(
            transform,
            "Rotation",
            serde_json::json!({"m":[[rotation[0],rotation[1],rotation[2]],[rotation[3],rotation[4],rotation[5]],[rotation[6],rotation[7],rotation[8]]]}),
            source,
        )?;
        set_transform(transform, "Parent Entity", serde_json::json!(0), source)?;
        for c in &mut entity.components {
            next = next
                .checked_add(1)
                .context("Runtime component identity exhausted")?;
            c.link_id = ID::new(next as u128);
            c.master_link_id = ID::new(0);
            for p in &mut c.properties {
                if p.class_name == "Entity Pointer" {
                    remap_root(&mut p.value, old, id)?;
                }
            }
        }
        let mut queue = std::collections::VecDeque::new();
        for d in graph.values() {
            for c in &d.components {
                for p in &c.properties {
                    if p.asset || p.palette {
                        let mut paths = Vec::new();
                        strings(&p.value, &mut paths);
                        for path in paths {
                            queue.push_back((d.source_scene.clone(), path));
                        }
                    }
                }
            }
        }
        let mut loaded = files.iter().map(|f| key(&f.path)).collect::<HashSet<_>>();
        let mut requested = HashSet::new();
        while let Some((owner, path)) = queue.pop_front() {
            if !requested.insert((key(&owner), key(&path))) {
                continue;
            }
            let file = dependency(store, &owner, &path)?;
            if !loaded.insert(key(&file.path)) {
                continue;
            }
            let identity = key(&file.path);
            if identity.ends_with(".nif") || identity.ends_with(".nif_wii") {
                for material in store.particle_files(&file.data)? {
                    if loaded.insert(key(&material.path)) {
                        files.push(material);
                    }
                }
            } else if [".kfm", ".kfm_wii", ".hkw", ".hkx", ".hkx_wii"]
                .iter()
                .any(|ext| identity.ends_with(ext))
            {
                for r in crate::asset_graph::inspect(&file.path, &file.data)?.references {
                    if r.kind == crate::asset_graph::ReferenceKind::Asset {
                        queue.push_back((file.path.clone(), r.value));
                    }
                }
            }
            files.push(file);
        }
        let global = scenes
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case("Palettes/_Dynamic.bin"))
            .map(|(_, s)| s);
        let scene = SceneFile::new(
            vec![entity],
            vec![ID::new(id as u128)],
            vec![],
            ID::new(0),
            source.version.clone(),
        );
        let sources = material_preview::native_sources(&self.asset_context, &files);
        let mut batch = scene_import::Batch::default();
        batch.textures = self.texture_cache.clone();
        batch.geometry_cache = self.scene_geometry_cache.clone();
        self.scene_collision_cache.seed(&mut batch);
        self.scene_sequence_cache.seed(&mut batch);
        let mut report = Report::default();
        batch.append_source(
            scope.clone(),
            scene,
            &files,
            global,
            &sources,
            &mut report,
            None,
        )?;
        ensure!(
            report.unsupported.is_empty(),
            "Template {name} preparation failed: {}",
            report.unsupported.join("; ")
        );
        let root = self.data_root.clone();
        let mut range =
            self.publish_entity_batch(&root, batch, report, &files, &sources, &scope, None)?;
        let index = range
            .find(|&i| self.entities[i].id == id)
            .context("Published template root missing")?;
        self.report.geometry.push(serde_json::json!({"kind":"palette_root_instance","template":name,"entity":index,"owner":owner,"source":definition.source_scene,"native_generated_name_verified":false}));
        Ok(index)
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate_palette_root() -> Result<()> {
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world = load(
        Path::new(&root),
        "Levels/GV_ZoneA_Start.level",
        &egui::Context::default(),
    )?;
    let owner = world.player_entity;
    let scenes = world.report.scenes.clone();
    let position = std::array::from_fn(|axis| world.entities[owner].transform[12 + axis]);
    let rotation = world.entities[owner].runtime_rotation;
    let first = world.spawn_palette_root(owner, "eticket_e_parentable", position, rotation, "")?;
    ensure!(
        world.entity_scene_active(first),
        "Created template is not in its owner's active scene"
    );
    ensure!(
        world.pickup_settings.contains_key(&first),
        "Created ticket has no pickup settings"
    );
    ensure!(
        world.pickup_receivers.contains_key(&first),
        "Created ticket has no pickup receiver"
    );
    ensure!(
        !world.entities[first].draws.is_empty(),
        "Created ticket has no rendered meshes"
    );
    ensure!(
        world.report.scenes == scenes,
        "Entity creation republished scene residency"
    );
    let second = world.spawn_palette_root(owner, "eticket_e_parentable", position, rotation, "")?;
    ensure!(
        world.entities[first].id != world.entities[second].id,
        "Template roots share identity"
    );
    ensure!(
        world.pickup_settings.contains_key(&second) && world.pickup_receivers.contains_key(&second),
        "Second ticket lifecycle missing"
    );
    // Exercise the live collection path, including the crossing update.
    // Repeated overlap queries must neither bypass nor advance the delay.
    let pickup_component = world.entities[first].components.iter()
        .position(|c| c.class_name == "PlayerPickupComponent").unwrap();
    world.pickup_update_states.get_mut(&first).unwrap().gate.delay(0.5);
    world.component_control.disabled.entry(first).or_default().insert(pickup_component, true);
    world.advance_pickup_collection_gates(0.25)?;
    ensure!(world.pickup_update_states[&first].gate.elapsed == 0., "Disabled pickup advanced its delay");
    world.advance_pickup_overlaps(position)?;
    ensure!(!world.removed_entities.contains(&first), "Disabled pickup was collected");
    world.component_control.disabled.get_mut(&first).unwrap().insert(pickup_component, false);
    world.advance_pickup_collection_gates(0.25)?;
    world.advance_pickup_overlaps(position)?;
    world.advance_pickup_overlaps(position)?;
    ensure!(!world.removed_entities.contains(&first)
        && world.pickup_update_states[&first].gate.elapsed == 0.25,
        "Overlap bypassed or advanced the collection delay");
    world.advance_pickup_collection_gates(0.25)?;
    ensure!(!world.pickup_update_states[&first].gate.delayed, "Delay did not clear at its endpoint");
    world.advance_pickup_overlaps(position)?;
    ensure!(!world.removed_entities.contains(&first), "Pickup collected on the delay-clearing update");
    world.advance_pickup_collection_gates(0.)?;
    world.advance_pickup_overlaps(position)?;
    ensure!(world.removed_entities.contains(&first), "Ready original ticket was not collected");
    ensure!(!world.pickup_collection_blocked.contains(&first), "Retired pickup retained collection admission state");
    println!("PASS: live original ticket waits through disabled, pending and delay-clearing updates, then collects and retires; repeated overlap queries do not advance its clock");
    // Original unparented ticket, outside attraction range: use the exact
    // source-derived bob kernel and require a stable baseline across updates.
    let bobbing=world.spawn_palette_root(owner,"eticket_e_parentable",
        [position[0]+20.,position[1],position[2]],rotation,"")?;
    ensure!(world.pickup_settings[&bobbing].should_bob,"Original ticket unexpectedly disables bobbing");
    let baseline=world.pickup_update_states[&bobbing].bob.base_height;
    let mut expected_bob=crate::blotworx::pickup::Bob::default();
    expected_bob.base_height=baseline;
    for _ in 0..2 {
        world.advance_pickup_collection_gates(0.25)?;
        let height=expected_bob.advance_height(0.25,world.pickup_bob_globals.frequency,world.pickup_bob_globals.amplitude)?;
        world.advance_pickup_attraction(0.25)?;
        ensure!(world.entities[bobbing].transform[13]==height
            && world.pickup_update_states[&bobbing].bob.base_height==baseline,
            "Original ticket bob drifts or differs from the native arithmetic");
    }
    let phase=world.pickup_update_states[&bobbing].bob.phase;
    let component=world.entities[bobbing].components.iter().position(|c|c.class_name=="PlayerPickupComponent").unwrap();
    world.component_control.disabled.entry(bobbing).or_default().insert(component,true);
    world.advance_pickup_collection_gates(0.25)?;
    world.advance_pickup_attraction(0.25)?;
    ensure!(world.pickup_update_states[&bobbing].bob.phase==phase,"Disabled ticket advanced its bob phase");
    world.retire_entity(bobbing)?;
    println!("PASS: original ticket bobs using native phase/height arithmetic with a stable baseline and pauses while disabled");
    let spatter = world
        .entities
        .iter()
        .position(|entity| entity.name == "gv_zonea_airlock.SpatterMelee 05")
        .context("Original Spatter missing")?;
    let scope = world.entities[spatter].scene.clone();
    world.activate_streamed_scene(&scope)?;
    // Late creation must start the source model's own local clock, not
    // enter its bounce curve at the level's seventeen-second phase.
    world.animation_time = 17.25;
    let before = world.entities.len();
    world.receive_death_rewards(spatter, 2)?;
    let record = world
        .report
        .geometry
        .iter()
        .find(|record| {
            record["kind"] == "death_reward_spawner_dispatch"
                && record["owner"].as_u64() == Some(spatter as u64)
        })
        .context("Original Spatter death reward not dispatched")?;
    let reward = record["spawned"]
        .as_array()
        .and_then(|array| array.first())
        .and_then(|id| id.as_u64())
        .context("Original Spatter reward did not create ammo")? as usize;
    ensure!(
        reward >= before && world.pickup_receivers.contains_key(&reward),
        "Spatter ammo has no pickup receiver"
    );
    ensure!(
        world.pickup_update_states[&reward].gate.duration == 1.2999999523162842_f32
            && world.pickup_update_states[&reward].gate.delayed,
        "Spatter ammo lost its authored collection delay"
    );
    ensure!(
        world.runtime_lifetimes[&reward].timer.remaining == 30.,
        "Spatter ammo lost its authored thirty-second lifetime"
    );
    let local = world
        .transform_hierarchy
        .get(&reward)
        .context("Original reward has no distribution parent")?;
    ensure!(
        local
            .socket
            .as_ref()
            .is_some_and(|socket| socket.name == "vfx_01"),
        "Original reward socket was not normalized/bound"
    );
    let parent = local.parent;
    let clock = world
        .model_animation
        .get(&parent)
        .and_then(|animation| animation.component_clock.as_ref())
        .context("Distribution local model clock missing")?;
    ensure!(
        clock.initial_update_time() == 0.,
        "New distribution inherited the level's current time"
    );
    let initial = world.entities[reward].transform;
    world.advance_models_with_delta(17.5, 0.25)?;
    world.update_skeletal_children_range_strict(reward..reward + 1)?;
    ensure!(
        world.model_animation[&parent]
            .component_clock
            .as_ref()
            .unwrap()
            .periodic_update_time(17.5)
            == 0.25,
        "Distribution did not accumulate its own component delta"
    );
    ensure!(
        (world.entities[reward].transform[13] - initial[13]).abs() > 0.01,
        "Original bounce controller did not move its attached pickup vertically"
    );
    let animated = world.entities[reward].transform;
    world.change_transform_parent(reward, None)?;
    ensure!(
        world.entities[reward].transform == animated
            && !world.transform_hierarchy.contains_key(&reward),
        "Detaching the pickup changed its published world pose"
    );
    // Full tanks reject attraction; changing live ammo (not the imported
    // snapshot) enables it. Keep Mickey close without producing collection yet.
    let pickup_position: [f32;3]=world.entities[reward].transform[12..15].try_into()?;
    world.entities[owner].set_controller_pose([pickup_position[0]+2.,pickup_position[1],pickup_position[2]],0.);
    let maxima=world.ammo.inventory.as_ref().context("Player live tanks missing")?.maxima;
    world.set_inventory_float(owner,"Paint",0,maxima[0].context("PaintMax missing")?)?;
    world.set_inventory_float(owner,"Thinner",0,maxima[1].context("ThinnerMax missing")?)?;
    ensure!(!world.pickup_attraction_eligible(reward)?,"Full tanks accepted an ammo attraction");
    world.set_inventory_float(owner,"Paint",0,0.)?;
    ensure!(world.pickup_attraction_eligible(reward)?,"Spent live paint did not enable ammo attraction");
    world.change_transform_parent(reward,Some(parent))?;
    world.advance_pickup_collection_gates(2.)?;
    world.advance_pickup_collection_gates(0.)?;
    world.advance_pickup_attraction(0.)?;
    ensure!(world.pickup_update_states[&reward].attraction.active
        && !world.transform_hierarchy.contains_key(&reward),"Original ammo did not start attraction and detach from its distribution");
    let wait=world.pickup_update_states[&reward].attraction.wait;
    if wait>0. {world.advance_pickup_attraction(wait)?;}
    let current=crate::blotworx::native_hk_transform::entity_position_vector(world.entities[reward].transform[12..15].try_into()?);
    let target=crate::blotworx::native_hk_transform::entity_position_vector(world.entities[owner].transform[12..15].try_into()?);
    let expected=crate::blotworx::pickup::attraction_step(current,target,world.pickup_settings[&reward].speed,0.01)?;
    world.advance_pickup_attraction(0.01)?;
    ensure!(world.entities[reward].transform[12..15]==expected[..3],"Live ammo movement differs from the original attraction arithmetic");
    ensure!(current[..3]!=expected[..3],"Ammo attraction did not move");
    let pickup_position=world.entities[reward].transform[12..15].try_into()?;
    world.advance_pickup_overlaps(pickup_position)?;
    ensure!(world.removed_entities.contains(&reward),"Attracted ammo did not collect through the live overlap path");
    ensure!(world.ammo.inventory.as_ref().unwrap().amounts[0]>0.,"Attracted ammo did not refill live paint");
    println!("PASS: full tanks reject original ammo attraction; spent live paint enables it; original wait/detachment/movement arithmetic and collection refill are connected");
    // Shared original distribution: only the paint drop can attract. Retiring
    // the source must preserve the other drop, detached at its current pose.
    let position=world.entities[owner].transform[12..15].try_into()?;
    let rotation=[1.,0.,0.,0.,1.,0.,0.,0.,1.];
    let shared=world.spawn_palette_root(owner,"5xdistribution",position,rotation,"")?;
    let paint=world.spawn_palette_root(owner,"ammo_paint_nif",position,rotation,"")?;
    let thinner=world.spawn_palette_root(owner,"ammo_thinner_parentable",position,rotation,"")?;
    world.change_transform_parent(paint,Some(shared))?;
    world.change_transform_parent(thinner,Some(shared))?;
    world.set_inventory_float(owner,"Paint",0,0.)?;
    world.set_inventory_float(owner,"Thinner",0,maxima[1].unwrap())?;
    world.advance_pickup_attraction(0.)?;
    ensure!(world.current_transform_parent(paint)?.is_none(),"Paint did not detach to attract");
    ensure!(world.current_transform_parent(thinner)?==Some(shared),"Full thinner tank unexpectedly attracted the sibling");
    ensure!(!world.report.unsupported.iter().any(|line|line.contains("Distribution retirement with other attached")),"Shared distribution cleanup was rejected");
    let pose=world.entities[thinner].transform;
    let rotation=world.entities[thinner].runtime_rotation;
    world.retire_entity(shared)?;
    ensure!(world.removed_entities.contains(&shared),"Distribution parent was not retired");
    for child in [paint,thinner] {
        ensure!(!world.removed_entities.contains(&child) && world.current_transform_parent(child)?.is_none(),"Parent retirement killed or retained a live pickup child");
        ensure!(world.pickup_receivers.contains_key(&child),"Detached pickup lost its receiver");
    }
    ensure!(world.entities[thinner].transform==pose && world.entities[thinner].runtime_rotation==rotation,"Parent retirement changed a surviving pickup's world pose");
    ensure!(world.entities[thinner].draws.clone().all(|draw|!world.removed_draws.contains(&draw)),"Parent retirement removed the sibling's draw geometry");
    world.retire_entity(shared)?;
    ensure!(world.entities[thinner].transform==pose,"Duplicate retirement altered detached child");
    println!("PASS: first original ammo pickup queues shared distribution cleanup; source retirement preserves the other pickup's pose, rendering and collection lifecycle");
    let after = world.entities.len();
    world.receive_death_rewards(spatter, 2)?;
    ensure!(
        world.entities.len() == after,
        "Death reward once flag did not suppress repeated events"
    );
    println!("PASS: original Spatter ammo binds to vfx_01, follows its original bounce controller from a fresh local clock, and detaches without changing world pose; delay/lifetime and once guard retained");
    println!("PASS: two original ticket palette roots publish distinct identities, meshes and pickup receivers without republishing a scene");
    Ok(())
}
