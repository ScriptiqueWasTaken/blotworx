//! Incoming scene publication into an existing world. Original lifecycle and
//! scheduling parity remain separate from resource/instance publication.
use super::*;
use std::io::{Read, Write};

fn compress_f32(values: &[f32], allocated_bytes: usize) -> Result<Option<CompressedSceneArray>> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    for chunk in values.chunks(2048) {
        let mut raw = Vec::with_capacity(chunk.len() * 4);
        for value in chunk {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        encoder.write_all(&raw)?;
    }
    let bytes = encoder.finish()?;
    if bytes.len() >= allocated_bytes {
        return Ok(None);
    }
    Ok(Some(CompressedSceneArray {
        elements: values.len(),
        bytes,
    }))
}

fn compress_u32(values: &[u32], allocated_bytes: usize) -> Result<Option<CompressedSceneArray>> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    for chunk in values.chunks(2048) {
        let mut raw = Vec::with_capacity(chunk.len() * 4);
        for value in chunk {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        encoder.write_all(&raw)?;
    }
    let bytes = encoder.finish()?;
    if bytes.len() >= allocated_bytes {
        return Ok(None);
    }
    Ok(Some(CompressedSceneArray {
        elements: values.len(),
        bytes,
    }))
}

fn compress_bytes(values: &[u8]) -> Result<Option<CompressedSceneArray>> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(values)?;
    let bytes = encoder.finish()?;
    if bytes.len() >= values.len() {
        return Ok(None);
    }
    Ok(Some(CompressedSceneArray {
        elements: values.len(),
        bytes,
    }))
}

fn decompress_f32(array: &CompressedSceneArray) -> Result<Vec<f32>> {
    let mut decoder = flate2::read::ZlibDecoder::new(array.bytes.as_slice());
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw)?;
    ensure!(raw.len() == array.elements * 4, "Cold NIF vertex data size mismatch");
    Ok(raw
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
        .collect())
}

fn decompress_u32(array: &CompressedSceneArray) -> Result<Vec<u32>> {
    let mut decoder = flate2::read::ZlibDecoder::new(array.bytes.as_slice());
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw)?;
    ensure!(raw.len() == array.elements * 4, "Cold NIF index data size mismatch");
    Ok(raw
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect())
}

fn decompress_bytes(array: &CompressedSceneArray) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(array.bytes.as_slice());
    let mut raw = Vec::with_capacity(array.elements);
    decoder.read_to_end(&mut raw)?;
    ensure!(raw.len() == array.elements, "Cold texture image size mismatch");
    // `read_to_end` grows geometrically and can leave a large spare allocation
    // for large RGBA images. The cold store is intended to lower resident RAM,
    // so return a tightly sized buffer after validating the decoded payload.
    raw.shrink_to_fit();
    Ok(raw)
}

fn empty_vertices() -> Arc<Vec<f32>> {
    static EMPTY: std::sync::OnceLock<Arc<Vec<f32>>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| Arc::new(Vec::new())).clone()
}

fn empty_indices() -> Arc<Vec<u32>> {
    static EMPTY: std::sync::OnceLock<Arc<Vec<u32>>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| Arc::new(Vec::new())).clone()
}

fn empty_image() -> Arc<Vec<u8>> {
    static EMPTY: std::sync::OnceLock<Arc<Vec<u8>>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(|| Arc::new(Vec::new())).clone()
}

fn cold_image_ref(
    rgba: &Arc<Vec<u8>>,
    cold_images: &HashMap<usize, CompressedSceneArray>,
) -> ColdImageRef {
    let identity = Arc::as_ptr(rgba) as usize;
    if cold_images.contains_key(&identity) {
        ColdImageRef::Stored(identity)
    } else {
        ColdImageRef::Retained(rgba.clone())
    }
}

fn cold_material_image(
    image: &crate::render_support::material_preview::MaterialImage,
    cold_images: &HashMap<usize, CompressedSceneArray>,
) -> ColdMaterialImage {
    ColdMaterialImage {
        slot: image.slot,
        width: image.width,
        height: image.height,
        sampler: image.sampler,
        matrix: image.matrix,
        rgba: cold_image_ref(&image.rgba, cold_images),
    }
}

fn restore_image_ref(
    image: ColdImageRef,
    images: &HashMap<usize, Arc<Vec<u8>>>,
) -> Result<Arc<Vec<u8>>> {
    match image {
        ColdImageRef::Stored(identity) => images
            .get(&identity)
            .cloned()
            .with_context(|| format!("Cold texture image {identity:#x} is missing")),
        ColdImageRef::Retained(rgba) => Ok(rgba),
    }
}

fn restore_material_image(
    image: ColdMaterialImage,
    images: &HashMap<usize, Arc<Vec<u8>>>,
) -> Result<crate::render_support::material_preview::MaterialImage> {
    Ok(crate::render_support::material_preview::MaterialImage {
        slot: image.slot,
        width: image.width,
        height: image.height,
        sampler: image.sampler,
        matrix: image.matrix,
        rgba: restore_image_ref(image.rgba, images)?,
    })
}

impl World {
    /// The interactive host publishes last tick's queued activation before
    /// component/trigger queries. The model phase consumes this marker, so
    /// moving publication earlier does not drain two queue slots per tick.
    /// Model-only callers retain their existing publication boundary.
    pub(crate) fn prepare_gameplay_scene_requests(&mut self) -> Result<()> {
        if !self.scene_requests_prepared_for_models {
            self.process_scene_request()?;
            self.scene_requests_prepared_for_models = true;
        }
        Ok(())
    }
    pub(crate) fn entity_scene_active(&self, entity: usize) -> bool {
        self.entities
            .get(entity)
            .is_some_and(|entity| self.scene_resources.is_active(&entity.scene))
    }
    pub(super) fn process_scene_request(&mut self) -> Result<()> {
        let Some(Some(requested)) = self.scene_requests.consume(false) else {
            return Ok(());
        };
        let key = key(&requested);
        let path = key
            .strip_suffix(".gsa")
            .map(|stem| format!("{stem}.bin"))
            .unwrap_or(key);
        if self
            .report
            .scenes
            .iter()
            .any(|scene| super::key(scene) == path)
        {
            self.activate_streamed_scene(&requested)?;
            return Ok(());
        }
        let root = self.data_root.clone();
        let ctx = self.asset_context.clone();
        self.import_incoming_scene(&root, &requested, &ctx)?;
        let limitation="Scene request admission follows the original queue. Publication currently runs synchronously before the world script update; native loader phase budgets/timing and activation event dispatch remain unvalidated.";
        if !self
            .report
            .unsupported
            .iter()
            .any(|entry| entry == limitation)
        {
            self.report.unsupported.push(limitation.into());
        }
        Ok(())
    }
    pub(crate) fn deactivate_streamed_scene(&mut self, requested: &str) -> Result<usize> {
        let target = super::super::scene_resources::scene_name(requested);
        let player_scene = self
            .entities
            .get(self.player_entity)
            .map(|entity| super::super::scene_resources::scene_name(&entity.scene));
        if player_scene.as_deref() == Some(target.as_str()) {
            return Ok(0);
        }
        let owners = self
            .entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| {
                (!self.removed_entities.contains(&index)
                    && !self.hidden_entities.contains(&index)
                    && super::super::scene_resources::scene_name(&entity.scene) == target)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let newly_hidden = self.set_entities_hidden(&owners, true)?;
        let hidden = newly_hidden.len();
        for entity in newly_hidden {
            self.stream_hidden_entities.insert(entity, target.clone());
        }
        self.scene_resources.deactivated(requested);
        self.cancel_inactive_story();
        self.suspend_inactive_trigger_overlaps();
        self.cold_store_scene_assets(&target)?;
        Ok(hidden)
    }
    pub(crate) fn activate_streamed_scene(&mut self, requested: &str) -> Result<usize> {
        let target = super::super::scene_resources::scene_name(requested);
        self.restore_cold_scene_assets(&target)?;
        let owners = self
            .stream_hidden_entities
            .iter()
            .filter_map(|(&entity, scene)| (scene == &target).then_some(entity))
            .collect::<Vec<_>>();
        let mut restored = 0;
        for entity in owners {
            self.stream_hidden_entities.remove(&entity);
            if self.removed_entities.contains(&entity) {
                continue;
            }
            self.set_entity_hidden(entity, false)?;
            restored += 1;
        }
        self.scene_resources.published(requested);
        self.set_streamed_scene_lookup(requested,true)?;
        Ok(restored)
    }
    pub(crate) fn set_streamed_scene_lookup(&mut self,scene:&str,available:bool)->Result<()> {
        let target=super::super::scene_resources::scene_name(scene);
        let owners=self.entities.iter().enumerate().filter_map(|(id,e)|
            (!self.removed_entities.contains(&id) && super::super::scene_resources::scene_name(&e.scene)==target).then_some(id)).collect::<Vec<_>>();
        self.scripts.set_scene_names_available(&owners,available)
    }

    fn cold_store_scene_assets(&mut self, target: &str) -> Result<usize> {
        let mut cold = self.cold_scene_assets.remove(target).unwrap_or_default();
        let target_draws = self
            .entities
            .iter()
            .enumerate()
            .filter(|(entity, runtime)| {
                !self.removed_entities.contains(entity)
                    && super::super::scene_resources::scene_name(&runtime.scene) == target
                    && runtime.jigsaw_asset.is_none()
            })
            .flat_map(|(_, entity)| entity.draws.clone())
            .filter(|draw| !self.sketch_draws.contains(draw) && !self.hidden_jigsaw_draws.contains(draw))
            .collect::<HashSet<_>>();

        let mut vertex_uses = HashMap::<usize, Vec<usize>>::new();
        let mut index_uses = HashMap::<usize, Vec<usize>>::new();
        for &draw_index in &target_draws {
            let Some(draw) = self.draws.get(draw_index) else {
                continue;
            };
            if !draw.v_data.is_empty() {
                vertex_uses
                    .entry(Arc::as_ptr(&draw.v_data) as usize)
                    .or_default()
                    .push(draw_index);
            }
            if !draw.indices.is_empty() {
                index_uses
                    .entry(Arc::as_ptr(&draw.indices) as usize)
                    .or_default()
                    .push(draw_index);
            }
        }
        for (identity, draws) in vertex_uses {
            let shared_outside_scene = self.draws.iter().enumerate().any(|(index, draw)| {
                !target_draws.contains(&index)
                    && Arc::as_ptr(&draw.v_data) as usize == identity
            });
            if shared_outside_scene {
                continue;
            }
            let draw = &self.draws[draws[0]];
            // Some runtime paths retain an auxiliary copy of draw data. Only
            // evict buffers whose complete ownership is represented by these
            // scene draw slots; this keeps actor/morph resources safe while
            // allowing their inactive render payloads to go cold.
            if Arc::strong_count(&draw.v_data) != draws.len() {
                continue;
            }
            let allocated = draw.v_data.capacity() * std::mem::size_of::<f32>();
            let Some(compressed) = compress_f32(&draw.v_data, allocated)? else {
                continue;
            };
            cold.vertices.insert(identity, compressed);
            for draw in draws {
                cold.draw_vertices.insert(draw, identity);
            }
        }
        for (identity, draws) in index_uses {
            let shared_outside_scene = self.draws.iter().enumerate().any(|(index, draw)| {
                !target_draws.contains(&index)
                    && Arc::as_ptr(&draw.indices) as usize == identity
            });
            if shared_outside_scene {
                continue;
            }
            let draw = &self.draws[draws[0]];
            if Arc::strong_count(&draw.indices) != draws.len() {
                continue;
            }
            let allocated = draw.indices.capacity() * std::mem::size_of::<u32>();
            let Some(compressed) = compress_u32(&draw.indices, allocated)? else {
                continue;
            };
            cold.indices.insert(identity, compressed);
            for draw in draws {
                cold.draw_indices.insert(draw, identity);
            }
        }

        // Images are often a larger CPU allocation than static geometry. Keep
        // them cold only when every strong owner is part of this streamed
        // scene or its lookup cache; active shared materials remain untouched.
        let image_draws = self
            .entities
            .iter()
            .enumerate()
            .filter(|(entity, runtime)| {
                !self.removed_entities.contains(entity)
                    && super::super::scene_resources::scene_name(&runtime.scene) == target
                    && runtime.jigsaw_asset.is_none()
            })
            .flat_map(|(_, entity)| entity.draws.clone())
            .filter(|draw| !self.sketch_draws.contains(draw) && !self.hidden_jigsaw_draws.contains(draw))
            .collect::<HashSet<_>>();
        let mut image_uses = HashMap::<usize, usize>::new();
        let mut image_dimensions = HashMap::<usize, (u32, u32)>::new();
        let mut blocked_images = HashSet::<usize>::new();
        let mut material_lists = HashMap::<usize, (usize, usize)>::new();
        let mut world_material_list_owners = HashMap::<usize, usize>::new();
        for draw in &self.draws {
            *world_material_list_owners
                .entry(Arc::as_ptr(&draw.material_images) as usize)
                .or_default() += 1;
        }
        for &draw in &image_draws {
            let list = &self.draws[draw].material_images;
            let key = Arc::as_ptr(list) as usize;
            let entry = material_lists.entry(key).or_insert((draw, 0));
            entry.1 += 1;
            if let Some((rgba, width, height)) = &self.draws[draw].tex_bytes {
                let identity = Arc::as_ptr(rgba) as usize;
                *image_uses.entry(identity).or_default() += 1;
                let dims = image_dimensions.entry(identity).or_insert((*width, *height));
                if *dims != (*width, *height) {
                    blocked_images.insert(identity);
                }
            }
        }
        for &(draw, owners) in material_lists.values() {
            let list = &self.draws[draw].material_images;
            let external_owner = world_material_list_owners
                .get(&(Arc::as_ptr(list) as usize))
                .copied()
                .unwrap_or_default()
                != owners;
            for image in list.iter() {
                let identity = Arc::as_ptr(&image.rgba) as usize;
                if external_owner {
                    blocked_images.insert(identity);
                }
                // A shared material list owns each image only once, regardless
                // of how many target draws reference that list.
                *image_uses.entry(identity).or_default() += 1;
                let dims = image_dimensions
                    .entry(identity)
                    .or_insert((image.width, image.height));
                if *dims != (image.width, image.height) {
                    blocked_images.insert(identity);
                }
            }
        }
        for (&draw, material) in &self.spherical_materials {
            if !image_draws.contains(&draw) {
                continue;
            }
            if let Ok((_, images)) = material {
                for image in images {
                    let identity = Arc::as_ptr(&image.rgba) as usize;
                    *image_uses.entry(identity).or_default() += 1;
                    let dims = image_dimensions
                        .entry(identity)
                        .or_insert((image.width, image.height));
                    if *dims != (image.width, image.height) {
                        blocked_images.insert(identity);
                    }
                }
            }
        }
        let mut cache_owners = HashMap::<usize, usize>::new();
        for image in self.texture_cache.values().flatten() {
            *cache_owners
                .entry(Arc::as_ptr(&image.rgba) as usize)
                .or_default() += 1;
        }
        let mut cache_keys = HashMap::<usize, Vec<(String, u32, u32)>>::new();
        for (path, image) in &self.texture_cache {
            if let Some(image) = image {
                cache_keys
                    .entry(Arc::as_ptr(&image.rgba) as usize)
                    .or_default()
                    .push((path.clone(), image.width, image.height));
            }
        }
        let candidates = image_uses
            .iter()
            .filter_map(|(&identity, &uses)| {
                if blocked_images.contains(&identity) {
                    return None;
                }
                let source = image_draws.iter().find_map(|&draw| {
                    let command = &self.draws[draw];
                    command
                        .tex_bytes
                        .as_ref()
                        .filter(|(rgba, _, _)| Arc::as_ptr(rgba) as usize == identity)
                        .map(|(rgba, _, _)| rgba)
                        .or_else(|| {
                            command
                                .material_images
                                .iter()
                                .find(|image| Arc::as_ptr(&image.rgba) as usize == identity)
                                .map(|image| &image.rgba)
                        })
                        .or_else(|| {
                            self.spherical_materials
                                .get(&draw)
                                .and_then(|material| material.as_ref().ok())
                                .and_then(|(_, images)| {
                                    images.iter().find(|image| {
                                        Arc::as_ptr(&image.rgba) as usize == identity
                                    })
                                })
                                .map(|image| &image.rgba)
                        })
                })?;
                let expected = uses + cache_owners.get(&identity).copied().unwrap_or_default();
                (Arc::strong_count(source) == expected).then_some((identity, source))
            })
            .collect::<Vec<_>>();
        for (identity, source) in candidates {
            if let Some(compressed) = compress_bytes(source)? {
                cold.images.insert(identity, compressed);
                if let Some(keys) = cache_keys.get(&identity) {
                    for (path, width, height) in keys {
                        cold.texture_cache_entries
                            .insert(path.clone(), (identity, *width, *height));
                    }
                }
            }
        }
        if !cold.images.is_empty() {
            for draw in image_draws {
                let command = &self.draws[draw];
                let has_cold_image = command
                    .tex_bytes
                    .as_ref()
                    .is_some_and(|(rgba, _, _)| {
                        cold.images.contains_key(&(Arc::as_ptr(rgba) as usize))
                    })
                    || command.material_images.iter().any(|image| {
                        cold.images
                            .contains_key(&(Arc::as_ptr(&image.rgba) as usize))
                    });
                if !has_cold_image {
                    continue;
                }
                let state = ColdDrawImages {
                    tex_bytes: command.tex_bytes.as_ref().map(|(rgba, width, height)| {
                        (
                            cold_image_ref(rgba, &cold.images),
                            *width,
                            *height,
                        )
                    }),
                    material_images: command
                        .material_images
                        .iter()
                        .map(|image| cold_material_image(image, &cold.images))
                        .collect(),
                };
                cold.draw_images.insert(draw, state);
                let command = &mut self.draws[draw];
                if command.tex_bytes.as_ref().is_some_and(|(rgba, _, _)| {
                    cold.images
                        .contains_key(&(Arc::as_ptr(rgba) as usize))
                }) {
                    command.tex_bytes = None;
                }
                for image in Arc::make_mut(&mut command.material_images) {
                    if cold
                        .images
                        .contains_key(&(Arc::as_ptr(&image.rgba) as usize))
                    {
                        image.rgba = empty_image();
                    }
                }
            }
            let spherical_draws = self
                .spherical_materials
                .iter()
                .filter_map(|(&draw, material)| {
                    let images = material.as_ref().ok().map(|(_, images)| images)?;
                    images
                        .iter()
                        .any(|image| {
                            cold.images
                                .contains_key(&(Arc::as_ptr(&image.rgba) as usize))
                        })
                        .then_some(draw)
                })
                .collect::<Vec<_>>();
            for draw in spherical_draws {
                let Some(material) = self.spherical_materials.remove(&draw) else {
                    continue;
                };
                let cold_material = match material {
                    Ok((program, images)) => ColdSphericalImages::Ready(
                        program,
                        images
                            .iter()
                            .map(|image| cold_material_image(image, &cold.images))
                            .collect(),
                    ),
                    Err(error) => ColdSphericalImages::Failed(error),
                };
                cold.spherical_images.insert(draw, cold_material);
            }
            self.texture_cache.retain(|_, image| {
                image.as_ref().is_none_or(|image| {
                    !cold.images
                        .contains_key(&(Arc::as_ptr(&image.rgba) as usize))
                })
            });
        }
        let saved = cold.compressed_bytes();
        if cold.is_empty() {
            return Ok(0);
        }
        for (&draw, &identity) in &cold.draw_vertices {
            let _ = identity;
            self.draws[draw].v_data = empty_vertices();
        }
        for (&draw, &identity) in &cold.draw_indices {
            let _ = identity;
            self.draws[draw].indices = empty_indices();
        }
        self.cold_scene_assets.insert(target.to_owned(), cold);
        // GeometryCache is only an index for sharing arrays while any draw
        // uses them; remove dead weak keys immediately after dropping the
        // scene's last hot references.
        self.scene_geometry_cache.prune();
        Ok(saved)
    }

    fn restore_cold_scene_assets(&mut self, target: &str) -> Result<usize> {
        let Some(cold) = self.cold_scene_assets.get(target) else {
            return Ok(0);
        };
        let vertices = cold
            .vertices
            .iter()
            .map(|(&identity, array)| Ok((identity, Arc::new(decompress_f32(array)?))))
            .collect::<Result<HashMap<_, _>>>()?;
        let indices = cold
            .indices
            .iter()
            .map(|(&identity, array)| Ok((identity, Arc::new(decompress_u32(array)?))))
            .collect::<Result<HashMap<_, _>>>()?;
        let mut images = HashMap::<usize, Arc<Vec<u8>>>::new();
        for (&identity, array) in &cold.images {
            let decoded = decompress_bytes(array)?;
            let matching_cache = cold.texture_cache_entries.iter().find_map(
                |(path, (image, width, height))| {
                    if *image != identity {
                        return None;
                    }
                    self.texture_cache
                        .get(path)
                        .and_then(Option::as_ref)
                        .filter(|cached| {
                            cached.width == *width
                                && cached.height == *height
                                && cached.rgba.as_slice() == decoded.as_slice()
                        })
                        .map(|cached| cached.rgba.clone())
                },
            );
            images.insert(
                identity,
                matching_cache.unwrap_or_else(|| Arc::new(decoded)),
            );
        }
        let mut restored_draws = HashSet::new();
        restored_draws.extend(cold.draw_vertices.keys().copied());
        restored_draws.extend(cold.draw_indices.keys().copied());
        restored_draws.extend(cold.draw_images.keys().copied());
        restored_draws.extend(cold.spherical_images.keys().copied());
        for (&draw, &identity) in &cold.draw_vertices {
            let data = vertices
                .get(&identity)
                .context("Cold scene vertex buffer missing")?
                .clone();
            self.draws
                .get_mut(draw)
                .context("Cold scene draw slot no longer exists")?
                .v_data = data;
        }
        for (&draw, &identity) in &cold.draw_indices {
            let data = indices
                .get(&identity)
                .context("Cold scene index buffer missing")?
                .clone();
            self.draws
                .get_mut(draw)
                .context("Cold scene draw slot no longer exists")?
                .indices = data;
        }
        for (&draw, state) in &cold.draw_images {
            let command = self
                .draws
                .get_mut(draw)
                .context("Cold scene texture draw slot no longer exists")?;
            command.tex_bytes = state
                .tex_bytes
                .as_ref()
                .map(|(image, width, height)| {
                    Ok::<_, anyhow::Error>((restore_image_ref(match image {
                        ColdImageRef::Stored(identity) => ColdImageRef::Stored(*identity),
                        ColdImageRef::Retained(rgba) => ColdImageRef::Retained(rgba.clone()),
                    }, &images)?, *width, *height))
                })
                .transpose()?;
            command.material_images = Arc::new(
                state
                    .material_images
                    .iter()
                    .map(|image| {
                        let image = ColdMaterialImage {
                            slot: image.slot,
                            width: image.width,
                            height: image.height,
                            sampler: image.sampler,
                            matrix: image.matrix,
                            rgba: match &image.rgba {
                                ColdImageRef::Stored(identity) => {
                                    ColdImageRef::Stored(*identity)
                                }
                                ColdImageRef::Retained(rgba) => {
                                    ColdImageRef::Retained(rgba.clone())
                                }
                            },
                        };
                        restore_material_image(image, &images)
                    })
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        for (&draw, state) in &cold.spherical_images {
            let material = match state {
                ColdSphericalImages::Ready(program, material_images) => Ok((
                    program.clone(),
                    material_images
                        .iter()
                        .map(|image| {
                            let image = ColdMaterialImage {
                                slot: image.slot,
                                width: image.width,
                                height: image.height,
                                sampler: image.sampler,
                                matrix: image.matrix,
                                rgba: match &image.rgba {
                                    ColdImageRef::Stored(identity) => {
                                        ColdImageRef::Stored(*identity)
                                    }
                                    ColdImageRef::Retained(rgba) => {
                                        ColdImageRef::Retained(rgba.clone())
                                    }
                                },
                            };
                            restore_material_image(image, &images)
                        })
                        .collect::<Result<Vec<_>>>()?,
                )),
                ColdSphericalImages::Failed(error) => Err(error.clone()),
            };
            self.spherical_materials.insert(draw, material);
        }
        for (path, (identity, width, height)) in &cold.texture_cache_entries {
            let rgba = images
                .get(identity)
                .context("Cold texture cache payload is missing")?
                .clone();
            if !matches!(self.texture_cache.get(path), Some(Some(_))) {
                self.texture_cache.insert(
                    path.clone(),
                    Some(crate::render_support::viewport3d::DecodedImage {
                        rgba,
                        width: *width,
                        height: *height,
                    }),
                );
            }
        }
        self.cold_scene_assets.remove(target);
        Ok(restored_draws.len())
    }
    pub(super) fn import_incoming_scene(
        &mut self,
        root: &Path,
        requested: &str,
        ctx: &egui::Context,
    ) -> Result<std::ops::Range<usize>> {
        let incoming = super::super::scene_loading::Source::read(root, Arc::from(requested))?;
        let published_name = incoming.path.clone();
        let incoming_archive=incoming.archive.clone();
        ensure!(
            !self
                .report
                .scenes
                .iter()
                .any(|path| key(path) == key(&incoming.path)),
            "Scene is already imported"
        );
        let mut report = Report::default();
        let mut files = Vec::new();
        pack(root, "_dynamic.pak", &mut files, &mut report)?;
        pack(root, "r3mt.pak", &mut files, &mut report)?;
        let mut existing = files
            .iter()
            .map(|file| key(&file.path))
            .collect::<HashSet<_>>();
        files.extend(
            incoming
                .files
                .into_iter()
                .filter(|file| existing.insert(key(&file.path))),
        );
        let global = lookup(&files, "Palettes/_Dynamic.bin").context("Global palette missing")?;
        let global = super::super::scene_loading::parse_scene(&global.path, &global.data)?;
        let sources = material_preview::native_sources(ctx, &files);
        let mut batch = scene_import::Batch::default();
        batch.external_player_name=Some(self.entities[self.player_entity].name.clone());
        batch.textures = self.texture_cache.clone();
        batch.geometry_cache = self.scene_geometry_cache.clone();
        self.scene_collision_cache.seed(&mut batch);
        self.scene_sequence_cache.seed(&mut batch);
        batch.append_source(
            incoming.path,
            incoming.scene,
            &files,
            Some(&global),
            &sources,
            &mut report,
            None,
        )?;
        self.publish_entity_batch(root, batch, report, &files, &sources, &published_name, Some(&incoming_archive))
    }

    /// Publish prepared instances through the same resource/component lifecycle.
    /// `stream_archive` distinguishes scene arrival from later entity creation:
    /// only arrival changes scene residency and emits OnMapLoadOrStream.
    pub(super) fn publish_entity_batch(
        &mut self,
        root: &Path,
        mut batch: scene_import::Batch,
        mut report: Report,
        files: &[VirtualFile],
        sources: &material_preview::Sources,
        published_name: &str,
        stream_archive: Option<&String>,
    ) -> Result<std::ops::Range<usize>> {
        ensure!(
            !batch.found_player && batch.player_draws.is_empty(),
            "Runtime player creation requires native Add Player support"
        );
        // append_source records its decoding source. Creating an instance from
        // that source does not publish another scene or change scene residency.
        if stream_archive.is_none() {
            report.scenes.clear();
        }
        // Ordinary spawned pickups do not need any paint/thinner surface
        // program. Prepare required programs before mutating the live world,
        // rather than rejecting an unrelated template after publishing it.
        let needs_paint = batch.entities.iter().any(|entity| {
            entity.jigsaw_asset.is_some() || entity.components.iter().any(|component| {
                matches!(component.class_name.as_str(), "PhysicsRigidBodyStaticComponent" | "PhysicsRigidBodyDynamicComponent")
                    && component.properties.iter().find(|p|p.name=="Is Real").and_then(|p|p.value.as_bool()) != Some(true)
            })
        });
        let shaders = if needs_paint {
            super::super::sketch_render::Shaders::load(files, &self.runtime_settings)?
        } else {
            super::super::sketch_render::Shaders::default()
        };
        self.scene_geometry_cache = batch.geometry_cache.clone();
        self.scene_collision_cache.capture(&batch);
        self.scene_sequence_cache.capture(&batch);
        let bindings = scene_import::prepare_actor_bindings(
            &batch.entities,
            &batch.actor_sequences,
            &batch.model_bounds,
            files,
            &mut self.scene_actor_binding_cache,
            &mut report,
        );
        // Do not build a second temporary Box3D world here. The previous
        // preflight cloned the scene's full triangle set, built every native
        // mesh/hull, then immediately destroyed it before admitting the same
        // geometry to `self.collision`. Large streamed scenes therefore had
        // a needless near-2x collision-memory peak. The live add_* calls
        // below validate each shape as it is admitted; importer-side geometry
        // validation has already run before this publication point.
        let entity_base = self.entities.len();
        let draw_base = self.draws.len();
        let part_base = self.report.collision_parts.len();
        let triangle_base = self.collision.triangle_count();
        let range = entity_base..entity_base + batch.entities.len();
        let owner_base = self.collision.next_owner()?.max(
            self.report
                .collision_parts
                .iter()
                .map(|part| part.owner)
                .max()
                .map(|owner| owner.checked_add(1).context("Static owner overflow"))
                .transpose()?
                .unwrap_or(0),
        );
        let owners = report
            .collision_parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                owner_base
                    .checked_add(index)
                    .map(|owner| (part.owner, owner))
                    .context("Incoming owner overflow")
            })
            .collect::<Result<HashMap<_, _>>>()?;
        for part in &report.collision_parts {
            if part.triangle_count == 0 {
                continue;
            }
            let end = part
                .triangle_start
                .checked_add(part.triangle_count)
                .context("Incoming triangle range overflow")?;
            let triangles = batch
                .triangles
                .get(part.triangle_start..end)
                .context("Incoming triangle range missing")?;
            let radius = *batch
                .triangle_radii
                .get(part.triangle_start)
                .context("Incoming triangle margin missing")?;
            self.collision
                .add_static_triangles(triangles, radius, owners[&part.owner])?;
        }
        for (vertices, radius, owner) in &batch.convex_parts {
            self.collision
                .add_static_convex(vertices, *radius, owners[owner])?;
        }
        for (index, part) in report.collision_parts.iter_mut().enumerate() {
            part.owner = owners[&part.owner];
            part.triangle_start += triangle_base;
            self.static_collision_owner_parts
                .insert(part.owner, part_base + index);
        }
        scene_import::retain_fixed_ground_velocities(
            &mut self.collision,
            report.collision_parts.iter(),
        )?;
        for entity in &mut batch.entities {
            entity.draws = entity.draws.start + draw_base..entity.draws.end + draw_base;
            entity.collision_parts =
                entity.collision_parts.start + part_base..entity.collision_parts.end + part_base;
            for indices in entity.mesh_draws.values_mut() {
                for index in indices {
                    *index += draw_base;
                }
            }
        }
        self.actor_bindings.extend(
            bindings
                .into_iter()
                .map(|(entity, binding)| (entity + entity_base, binding)),
        );
        self.actor_sequences.extend(
            batch
                .actor_sequences
                .into_iter()
                .map(|(entity, resource)| (entity + entity_base, resource)),
        );
        self.actor_draw_sources.extend(
            batch
                .actor_draw_sources
                .into_iter()
                .map(|(draw, source)| (draw + draw_base, source)),
        );
        self.model_mesh_inverse.extend(
            batch
                .model_mesh_inverse
                .into_iter()
                .map(|(draw, matrix)| (draw + draw_base, matrix)),
        );
        self.model_local_draws.extend(
            batch
                .model_local_draws
                .into_iter()
                .map(|draw| draw + draw_base),
        );
        for (asset, skins) in batch.skins {
            self.skins.entry(asset).or_insert(skins);
        }
        for (asset, jigsaws) in batch.jigsaws {
            self.jigsaws.entry(asset).or_insert(jigsaws);
        }
        for (asset, bounds) in batch.model_bounds {
            self.model_bounds.entry(asset).or_insert(bounds);
        }
        self.draws.extend(batch.draws);
        self.entities.extend(batch.entities);
        self.lights.extend(batch.lights);
        self.report.scenes.extend(report.scenes);
        self.report.collision_parts.extend(report.collision_parts);
        self.report.spherical_lights.extend(report.spherical_lights);
        for record in report.geometry.iter().filter(|r|r["kind"]=="canonical_player_alias") {
            let alias=record["alias"].as_str().context("Incoming player alias missing")?;
            self.scripts.register_existing_alias(self.player_entity,alias)?;
        }
        self.report.geometry.extend(report.geometry);
        self.report.warnings.extend(report.warnings);
        self.report.unsupported.extend(report.unsupported);
        self.report.placed_entities += report.placed_entities;
        self.report.render_triangles += report.render_triangles;
        self.report.meshes += report.meshes;
        self.report.collision_triangles += report.collision_triangles;
        self.report.global_overrides += report.global_overrides;
        for (class, count) in report.components {
            *self.report.components.entry(class).or_default() += count;
        }
        for entity in range.clone() {
            let runtime = &self.entities[entity];
            let mut aliases = Vec::new();
            for component in runtime
                .components
                .iter()
                .filter(|component| component.class_name == "InputOutputComponent")
            {
                if let Some(groups) = component
                    .properties
                    .iter()
                    .find(|property| property.name == "EntityGroups")
                {
                    for group in groups
                        .value
                        .as_array()
                        .context("Invalid incoming entity groups")?
                    {
                        let name = group
                            .as_str()
                            .context("Invalid incoming entity group name")?;
                        if !name.is_empty() && !name.starts_with('.') {
                            aliases.push(name.to_owned());
                        }
                    }
                }
            }
            self.scripts
                .register_entity(entity, &runtime.name, &aliases)?;
        }
        let archives = stream_archive.map(std::slice::from_ref).unwrap_or(&[]);
        self.named_fx.admit(archives, &[published_name.to_owned()], files, &self.entities[range.clone()])?;
        self.scene_particles.admit_packs(archives)?;
        if let Some(archive) = stream_archive {
            if !self.report.packs.iter().any(|p| p.eq_ignore_ascii_case(archive)) {
                self.report.packs.push(archive.clone());
            }
        }
        let templates = if self.entities[range.clone()]
            .iter()
            .any(|entity| entity.jigsaw_asset.is_some())
        {
            super::super::jigsaw_effects::templates(&files)?
        } else {
            HashMap::new()
        };
        let mut next_owner = self.collision.next_owner()?;
        initialize_jigsaw_entities(
            &self.entities,
            range.clone(),
            &self.jigsaws,
            &mut self.collision,
            &mut self.draws,
            &mut self.report,
            &shaders,
            files,
            &mut batch.textures,
            sources,
            &templates,
            &mut self.sketch_draws,
            &mut self.hidden_jigsaw_draws,
            &mut self.jigsaw_instances,
            &mut self.jigsaw_owners,
            &mut next_owner,
        )?;
        // Component initialization publishes filters for all newly owned shapes,
        // so jigsaw bodies must exist before this phase, as on initial loading.
        self.initialize_component_range(range.clone(), files, root)?;
        if self.demo_story.enabled {
            self.initialize_demo_motion_range(range.clone());
        }
        let dynamic_base = self.dynamic_bodies.len();
        let dynamic = dynamic_collision_assets(&self.entities[range.clone()], &files)
            .into_iter()
            .map(|mut body| {
                body.entity += entity_base;
                body
            })
            .collect::<Vec<_>>();
        self.dynamic_bodies.extend(dynamic);
        self.dynamic_body_sources =
            super::super::dynamic_body::SourceSummary::new(&self.dynamic_bodies);
        self.construct_dynamic_sources(dynamic_base..self.dynamic_bodies.len())?;
        self.admit_dynamic_sources(dynamic_base..self.dynamic_bodies.len(), &mut next_owner)?;
        let mut effects = HashMap::new();
        super::super::model_paint::initialize_range(
            self,
            range.clone(),
            files,
            &shaders,
            sources,
            &mut batch.textures,
            &mut effects,
        )?;
        self.bind_spherical_properties(range.clone(), sources, files, &mut batch.textures)?;
        let contacts = self.report.collision_parts[part_base..]
            .iter()
            .filter(|part| part.triangle_count > 0)
            .map(|part| -> Result<_> {
                let entity = self
                    .collision_entity(part.owner)
                    .context("Static contact entity missing")?;
                Ok(super::super::character_contacts::Source {
                    owner: part.owner,
                    resource: part.native_resource.clone(),
                    part: part.native_part,
                    welding: part.native_welding.clone(),
                    body: part.body_offset,
                    triangle_start: part.triangle_start,
                    initial: super::super::native_hk_transform::initial_body(
                        &entity.physics_transform(),
                        &part.body_transform,
                    )?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.collision
            .append_native_static_character_contacts(contacts)?;
        self.scene_motion_prepared_at = None;
        self.report.collision_triangles = self.collision.triangle_count();
        self.report.textures = batch
            .textures
            .values()
            .filter(|image| image.is_some())
            .count();
        self.texture_cache = batch.textures;
        if stream_archive.is_some() {
            self.scene_resources.published(published_name);
        }
        // Stream construction must reach authored initialization IO after its
        // components and script definitions exist. OnMapLoad is level-wide;
        // do not replay it on all previously loaded actors for a scene import.
        let mut initialization_connections=0;
        for entity in range.clone() {
            if self.outputs.has_connections(entity,"OnMapLoadOrStream") {initialization_connections+=1;}
            if !self.spawner_sources.contains(&entity) {
                self.emit_entity_output(entity,"OnSpawned",entity);
            }
            if stream_archive.is_some() {
                self.emit_entity_output(entity,"OnMapLoadOrStream",entity);
            }
        }
        if stream_archive.is_some() {
            self.report.unsupported.push("Incoming scene resources/entities/known component bindings are published. Native activation events, phase timing, AI/navigation registration, complete character controllers and unloading remain incomplete.".into());
        }
        self.report.geometry.push(serde_json::json!({"kind":if stream_archive.is_some(){"streamed_scene_initialization"}else{"spawned_entity_initialization"},"scene":published_name,"entities":range.len(),"connected_owners":initialization_connections,"native_phase_timing_verified":false}));
        Ok(range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cold_scene_arrays_roundtrip_bit_exactly() -> Result<()> {
        let vertices = (0..16_384)
            .map(|index| (index % 17) as f32 * 0.25 - 2.)
            .collect::<Vec<_>>();
        let indices = (0..16_384).map(|index| index % 256).collect::<Vec<_>>();
        let pixels = (0..65_536)
            .map(|index| [((index / 4) % 251) as u8, 32, 128, 255][index % 4])
            .collect::<Vec<_>>();
        let cold_vertices = compress_f32(&vertices, vertices.capacity() * 4)?
            .context("Expected repeated NIF vertices to compress")?;
        let cold_indices = compress_u32(&indices, indices.capacity() * 4)?
            .context("Expected repeated NIF indices to compress")?;
        let cold_pixels = compress_bytes(&pixels)?.context("Expected authored RGBA texture to compress")?;
        assert!(cold_vertices.bytes.len() < vertices.len() * 4);
        assert!(cold_indices.bytes.len() < indices.len() * 4);
        assert!(cold_pixels.bytes.len() < pixels.len());
        assert_eq!(decompress_f32(&cold_vertices)?, vertices);
        assert_eq!(decompress_u32(&cold_indices)?, indices);
        assert_eq!(decompress_bytes(&cold_pixels)?, pixels);
        Ok(())
    }

    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_streamed_scene_geometry_cold_store_restores_draws_exactly() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = load(
            Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let player_scene = super::super::super::scene_resources::scene_name(
            &world.entities[world.player_entity].scene,
        );
        let candidate = world
            .entities
            .iter()
            .enumerate()
            .filter(|(index, entity)| {
                *index != world.player_entity
                    && !world.removed_entities.contains(index)
                    && super::super::super::scene_resources::scene_name(&entity.scene) != player_scene
                    && !world.actors.contains_key(index)
                    && !world.model_animation.contains_key(index)
                    && !world.authored_havok.contains(*index)
                    && !world.havok_actors.contains_key(index)
                    && entity.jigsaw_asset.is_none()
            })
            .flat_map(|(_, entity)| {
                let scene = super::super::super::scene_resources::scene_name(&entity.scene);
                entity.draws.clone().map(move |draw| (scene.clone(), draw))
            })
            .filter(|(_, draw)| {
                !world.sketch_draws.contains(draw) && !world.hidden_jigsaw_draws.contains(draw)
            })
            .filter(|(_, draw)| {
                world.draws.get(*draw).is_some_and(|draw| {
                    !draw.v_data.is_empty()
                        && compress_f32(
                            &draw.v_data,
                            draw.v_data.capacity() * std::mem::size_of::<f32>(),
                        )
                        .is_ok_and(|array| array.is_some())
                })
            })
            .next()
            .context("Expected compressible geometry in a non-player streamed scene")?;
        let target = candidate.0;
        let draw = candidate.1;
        let original_vertices = world.draws[draw].v_data.as_ref().clone();
        let original_indices = world.draws[draw].indices.as_ref().clone();
        let hidden = world.deactivate_streamed_scene(&target)?;
        assert!(hidden > 0, "streamed scene did not hide any entities");
        assert!(world.draws[draw].v_data.is_empty());
        let cold_bytes = world
            .cold_scene_assets
            .get(&target)
            .context("inactive scene cold store missing")?
            .compressed_bytes();
        assert!(cold_bytes > 0);
        world.activate_streamed_scene(&target)?;
        assert_eq!(world.draws[draw].v_data.as_ref(), &original_vertices);
        assert_eq!(world.draws[draw].indices.as_ref(), &original_indices);
        assert!(!world.cold_scene_assets.contains_key(&target));
        assert!(world.entities.iter().enumerate().any(|(index, entity)| {
            !world.removed_entities.contains(&index)
                && super::super::super::scene_resources::scene_name(&entity.scene) == target
                && !world.hidden_entities.contains(&index)
        }));
        println!("Stream cold store compressed {cold_bytes} bytes and restored draw {draw} bit-exactly");
        Ok(())
    }

    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_airlock_publication_preserves_live_world_player_lua_and_actor_state() -> Result<()>
    {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let root = Path::new(&root);
        let ctx = egui::Context::default();
        // Zone A exploration preloads this airlock. Start in Zone B so
        // publication actually adds a scene instead of reimporting one.
        let mut world = load(root, "Levels/GV_ZoneB_Start.level", &ctx)?;
        ensure!(
            !world
                .report
                .scenes
                .iter()
                .any(|scene| key(scene).ends_with("/gv_zonea_airlock.bin")),
            "Incoming-scene fixture must start without the target airlock"
        );
        world.run_script("BwxPersist={value=73}; BwxOldPlayer=GetPlayer()")?;
        world.advance_models(0.25)?;
        let player = world.player_entity;
        let pose = world.entities[player].transform;
        let checkpoint_scenes = world.scene_resources.save_names(&world.scene_requests);
        let entities = world.entities.len();
        let draws = world.draws.len();
        let parts = world.report.collision_parts.len();
        let actor_state = world
            .actors
            .iter()
            .map(|(&index, actor)| (index, actor.clock.initial_update_time(), actor.active_id()))
            .collect::<Vec<_>>();
        let transforms = world
            .entities
            .iter()
            .map(|entity| entity.transform)
            .collect::<Vec<_>>();
        let texture = world.texture_cache.iter().find_map(|(name, image)| {
            image
                .as_ref()
                .map(|image| (name.clone(), image.rgba.clone()))
        });
        let havok_state = world.authored_havok.snapshots();
        let ai_registry = world.ai_registry.order()?;
        ensure!(
            ai_registry.len() == world.ai_brains.len(),
            "AI registry lost a loaded brain"
        );
        ensure!(
            !havok_state.is_empty(),
            "Expected initialized original Havok actors"
        );
        let ai_owner = world
            .ai_brains
            .iter()
            .find(|(_, brain)| !brain.paused.is_empty())
            .map(|(&owner, _)| owner)
            .context("Expected an original Paused receiver")?;
        world.receive_known_ai_behavior_event(
            ai_owner,
            super::super::super::ai_behavior_events::Event {
                kind: 0x0c,
                source: Some(ai_owner),
                action: 13,
            },
        )?;
        world.receive_known_ai_behavior_event(
            ai_owner,
            super::super::super::ai_behavior_events::Event {
                kind: 9,
                source: Some(ai_owner),
                action: 0x8e,
            },
        )?;
        let ai_state = world
            .ai_brains
            .iter()
            .map(|(&owner, brain)| {
                (
                    owner,
                    brain.ai_type.clone(),
                    brain.root.clone(),
                    brain.behaviors.clone(),
                    brain
                        .paused
                        .iter()
                        .map(|(&slot, state)| (slot, state.active))
                        .collect::<Vec<_>>(),
                    brain.reevaluation_requested,
                    brain.selector.ordered.clone(),
                    brain.selector.current,
                    brain.motion_suspended,
                    brain.uninterruptible_fidget,
                )
            })
            .collect::<Vec<_>>();
        let range = world.import_incoming_scene(
            root,
            "Environments/thepark/fantasyLand/SmallWorld/GSA/GV_ZoneA_airlock.gsa",
            &ctx,
        )?;
        assert_eq!(range.start, entities);
        assert_eq!(range.end, world.entities.len());
        assert_eq!(world.player_entity, player);
        assert_eq!(world.entities[player].transform, pose);
        assert_eq!(
            world.scene_resources.save_names(&world.scene_requests),
            checkpoint_scenes,
            "An imported scene outside current part records must not automatically enter WRLD"
        );
        assert!(world.draws.len() > draws && world.report.collision_parts.len() > parts);
        assert!(world.entities[..entities]
            .iter()
            .map(|entity| entity.transform)
            .eq(transforms));
        let current_havok = world.authored_havok.snapshots();
        let current_registry = world.ai_registry.order()?;
        ensure!(
            current_registry.len() == world.ai_brains.len(),
            "Streaming lost AI registry membership"
        );
        assert_eq!(
            current_registry
                .iter()
                .copied()
                .filter(|owner| *owner < entities)
                .collect::<Vec<_>>(),
            ai_registry,
            "Streaming changed existing AI registration order"
        );
        let mut expected_incoming = range
            .clone()
            .filter(|owner| world.ai_brains.contains_key(owner))
            .collect::<Vec<_>>();
        expected_incoming.reverse();
        assert_eq!(
            current_registry
                .iter()
                .copied()
                .filter(|owner| range.contains(owner))
                .collect::<Vec<_>>(),
            expected_incoming
        );
        for previous in havok_state {
            assert_eq!(
                current_havok.iter().find(|state| state.0 == previous.0),
                Some(&previous),
                "Incoming scene changed an existing Havok actor"
            );
        }
        for (index, time, active) in actor_state {
            assert_eq!(world.actors[&index].clock.initial_update_time(), time);
            assert_eq!(world.actors[&index].active_id(), active);
        }
        for (
            owner,
            kind,
            root,
            slots,
            paused,
            requested,
            ordered,
            current,
            motion_suspended,
            uninterruptible_fidget,
        ) in ai_state
        {
            let brain = world
                .ai_brains
                .get(&owner)
                .context("Streaming discarded an existing AI brain")?;
            assert_eq!(brain.ai_type, kind);
            assert_eq!(brain.root, root);
            assert_eq!(brain.behaviors, slots);
            assert_eq!(
                brain
                    .paused
                    .iter()
                    .map(|(&slot, state)| (slot, state.active))
                    .collect::<Vec<_>>(),
                paused
            );
            assert_eq!(brain.reevaluation_requested, requested);
            assert_eq!(brain.selector.ordered, ordered);
            assert_eq!(brain.selector.current, current);
            assert_eq!(brain.motion_suspended, motion_suspended);
            assert_eq!(brain.uninterruptible_fidget, uninterruptible_fidget);
        }
        if let Some((name, image)) = texture {
            assert!(Arc::ptr_eq(
                &image,
                &world.texture_cache[&name].as_ref().unwrap().rgba
            ));
        }
        for entity in range.clone() {
            let runtime = &world.entities[entity];
            assert!(runtime.draws.start >= draws && runtime.draws.end <= world.draws.len());
            for part in runtime.collision_parts.clone() {
                let owner = world.report.collision_parts[part].owner;
                assert_eq!(world.collision_entity(owner).unwrap().scene, runtime.scene);
                assert_eq!(world.collision_entity(owner).unwrap().id, runtime.id);
                assert!(world.collision.has_owner(owner));
                let source = &world.report.collision_parts[part];
                let mesh = &source.native_resource[source.native_part];
                if mesh.motion_type == 5 {
                    for shape in world.collision.owner_shapes(owner) {
                        assert_eq!(
                            world.collision.ground_body_linear_velocity(shape),
                            Some(mesh.linear_velocity),
                            "Incoming owner remapping lost fixed-body source velocity"
                        );
                    }
                }
            }
        }
        let new_name = range
            .clone()
            .find(|&entity| {
                world.scripts.resolve_name(&world.entities[entity].name) == Some(entity)
            })
            .context("Expected a registered incoming entity name")?;
        let name = serde_json::to_string(&world.entities[new_name].name)?;
        world.run_script(&format!("assert(BwxPersist.value==73); assert(GetPlayer()==BwxOldPlayer); assert(GetEntity({name})~=nil)"))?;
        world.advance_models(world.animation_time)?;
        ensure!(
            !world
                .report
                .unsupported
                .iter()
                .any(|line| line.contains("@runtime animation console")),
            "Scene publication broke the persistent Lua state"
        );
        println!("Published incoming airlock: {} entities, {} additional meshes, {} static records; existing player/Lua/actor state preserved",
            range.len(),world.draws.len()-draws,world.report.collision_parts.len()-parts);
        Ok(())
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate_streamed_story()->Result<()> {
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let root=Path::new(&root);let ctx=egui::Context::default();
    let mut world=load(root,"Levels/GV_ZoneA_Start.level",&ctx)?;
    world.enable_demo_story()?;
    let original=world.story_sequences.resources.iter().map(|r|(r.key.clone(),r.bytes.clone())).collect::<Vec<_>>();
    ensure!(!original.is_empty(),"Original sequence catalog missing");
    let retained=super::super::story_sequences::Request{entity:world.player_entity,resource:0};
    world.story_sequences.queue.push_back(retained);
    let navigation_before=world.ai_navigation.database_paths().map(str::to_owned).collect::<Vec<_>>();
    let range=world.import_incoming_scene(root,"Environments/ThePark/FantasyLand/SmallWorld/GSA/gv_zoneb.gsa",&ctx)?;
    let navigation_after=world.ai_navigation.database_paths().map(str::to_owned).collect::<Vec<_>>();
    ensure!(navigation_after.starts_with(&navigation_before),"Streaming replaced existing navigation graph identities");
    // Original ZoneB contains no HPD: both GV databases belong to the
    // already loaded GV shared scene. Verify that streaming does not duplicate
    // them, then exercise incremental admission with those original records.
    ensure!(navigation_after==navigation_before,"ZoneB duplicated shared navigation graphs");
    let shared=crate::pack_io::read(std::fs::read(root.join("packfiles/gv_shared_za.pak"))?)?.pak.files;
    let first=world.entities.iter().position(|entity|entity.components.iter().any(|c|c.class_name=="AIPathDatabaseComponent" && c.properties.iter().any(|p|p.name=="PathData HPD Filename" && p.value.as_str().is_some_and(|s|!s.is_empty())))).context("Original GV navigation owner missing")?;
    let mut navigation=super::super::ai_navigation::Runtime::load(&world.entities[first..first+1],&shared,root);
    let partial=navigation.database_paths().map(str::to_owned).collect::<Vec<_>>();
    ensure!(!partial.is_empty(),"Original first HPD failed to load");
    navigation.admit(&world.entities,&shared,root);
    let full=navigation.database_paths().map(str::to_owned).collect::<Vec<_>>();
    ensure!(full.starts_with(&partial) && full.len()>partial.len() && full.len()==navigation_before.len() && navigation.sector_count()==world.ai_navigation.sector_count() && navigation.warnings.is_empty(),"Incremental original navigation graph admission failed: {:?}",navigation.warnings);
    navigation.admit(&world.entities,&shared,root);
    ensure!(navigation.database_paths().count()==full.len(),"Repeated navigation admission duplicated graph resources");
    let mut deliveries=Vec::new();
    for owner in range.clone() {deliveries.extend(world.outputs.ready_for_owner(f32::MAX,owner));}
    let initialization=deliveries.iter().filter(|d|d.connection.output=="OnMapLoadOrStream").collect::<Vec<_>>();
    ensure!(!initialization.is_empty(),"Stream import did not enqueue original initialization IO");
    ensure!(deliveries.iter().all(|d|d.connection.output!="OnMapLoad"),"Stream import incorrectly repeated level-wide map-load IO");
    ensure!(initialization.iter().all(|d|d.activator==Some(d.connection.owner)),"Initialization owner context changed");
    println!("Original streamed initialization rows: {}",serde_json::to_string(&initialization.iter().map(|d|serde_json::json!({"owner":world.entities[d.connection.owner].name,"input":d.connection.input,"target":d.connection.target,"delay":d.connection.delay})).collect::<Vec<_>>())?);
    let scene=world.entities[range.start].scene.clone();
    world.deactivate_streamed_scene(&scene)?;world.activate_streamed_scene(&scene)?;
    ensure!(range.clone().all(|owner|!world.outputs.has_pending_for_owner(owner)),"Reactivation repeated one-time construction IO");
    for (id,(key,bytes)) in original.iter().enumerate() {
        ensure!(world.story_sequences.resources[id].key==*key && Arc::ptr_eq(bytes,&world.story_sequences.resources[id].bytes),"Streaming replaced an existing resource identity");
    }
    ensure!(world.story_sequences.queue.contains(&retained),"Streaming discarded pending story requests");
    let mut admitted=None;
    for entity in range.clone() {
        for c in &world.entities[entity].components {
            if c.class_name!="ConversationComponent" {continue;}
            let Some(p)=c.properties.iter().find(|p|p.name=="Sequences") else {continue;};
            for path in super::super::actor_scripts::authored_paths(&p.value)? {
                let key=path.replace('\\',"/");let name=key.rsplit('/').next().unwrap_or("").split('.').next().unwrap_or("");
                if world.story_sequences.resources.iter().any(|r|r.key==name) {admitted=Some((entity,name.to_owned()));break;}
            }
            if admitted.is_some(){break;}
        }
        if admitted.is_some(){break;}
    }
    let (owner,name)=admitted.context("Original streamed scene registered no conversation resources")?;
    ensure!(matches!(world.force_story_sequence(owner,&name)?,super::super::story_sequences::Admission::Queued),"Streamed actor cannot request its original BSQ");
    let resources=world.story_sequences.resources.len();let queued=world.story_sequences.queue.clone();
    let before=world.story_sequences.summary()["registered_entities"].clone();
    world.story_sequences.admit_range(&world.entities,range,|_|Ok(None))?;
    ensure!(world.story_sequences.resources.len()==resources && world.story_sequences.queue==queued && world.story_sequences.summary()["registered_entities"]==before,"Re-admission duplicated or reset story state");
    println!("PASS: actual GV ZoneB streaming registers original actor BSQs/navigation graphs; retained resource identities/queued requests; repeat admission is idempotent; selected {name}");
    Ok(())
}
