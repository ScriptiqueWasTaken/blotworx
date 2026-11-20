//! World-bound merging used by SH sampling, recovered from the EM1 DOL.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NiTransform, NifFile};
use std::collections::BTreeMap;

/// Editor mesh caches include preview cards for particle-system blocks.
/// Gameplay renders these through particle emission, never as static meshes.
pub(super) fn particle_geometry_blocks(nif:&NifFile)->std::collections::HashSet<usize> {
    nif.block_type_indices.iter().enumerate().filter_map(|(index,&kind)|{
        nif.block_types.get(kind as usize).is_some_and(|name|name.contains("ParticleSystem")).then_some(index)
    }).collect()
}

/// Serialized NiTimeController prefix. Keep chain order and target identity:
/// controller presence alone does not identify which runtime update is missing.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Controller {
    pub block: u32,
    pub class_name: String,
    pub flags: u16,
    pub frequency: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
    pub target: Option<u32>,
    pub transform: Option<Result<super::nif_transform::Interpolator, String>>,
    pub(super) color: Option<super::nif_properties::DirectColor>,
    pub(super) indirect_scale: Option<super::nif_properties::DirectIndirectScale>,
}
impl Controller {
    /// EM1 Vis update80425124 changes only NiAVObject flags18 bit0;
    /// FloatExtra update803e9f84 writes only the extra-data scalar0c.
    /// ColorExtra update803d8910 calls8044e4b4, which writes only RGBA data.
    /// None is a geometry/transform bound producer.
    fn affects_bound(&self) -> bool {
        !matches!(
            self.class_name.as_str(),
            "NiVisController" | "NiFloatExtraDataController" | "NiColorExtraDataController"
        )
    }
}
pub(super) fn controllers(nif: &NifFile, mut next: Option<u32>) -> Result<Vec<Controller>> {
    let mut result = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = next {
        ensure!(seen.insert(id), "Cyclic controller chain at block {id}");
        let block = nif
            .blocks
            .get(id as usize)
            .with_context(|| format!("Missing controller block {id}"))?;
        let NiObject::Unknown(raw) = block else {
            anyhow::bail!(
                "Controller prefix unavailable for block {id} ({})",
                block.type_name()
            )
        };
        ensure!(
            raw.raw_data.len() >= 26,
            "Truncated controller {id} ({})",
            raw.type_name
        );
        let bytes = &raw.raw_data;
        let uint = |at: usize| {
            let b = bytes[at..at + 4].try_into().unwrap();
            if nif.source_is_little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let link = |at: usize| -> Result<Option<u32>> {
            let value = uint(at);
            if value == u32::MAX {
                return Ok(None);
            }
            ensure!(
                (value as usize) < nif.blocks.len(),
                "Controller {id} has missing link {value} at {at}"
            );
            Ok(Some(value))
        };
        next = link(0)?;
        let flags = if nif.source_is_little_endian {
            u16::from_le_bytes(bytes[4..6].try_into()?)
        } else {
            u16::from_be_bytes(bytes[4..6].try_into()?)
        };
        let transform = if raw.type_name == "NiTransformController" {
            ensure!(
                bytes.len() == 30,
                "Unexpected transform controller size at {id}"
            );
            link(26)?.map(|interpolator| {
                super::nif_transform::Interpolator::load(nif, interpolator)
                    .map_err(|e| format!("{e:#}"))
            })
        } else {
            None
        };
        let color = if raw.type_name == "NiColorExtraDataController" && bytes.len() == 34 {
            match super::nif_properties::DirectColor::load(nif, uint(30), uint(26)) {
                Ok(color) => color,
                Err(error) => {
                    log::warn!("Direct material color controller {id}: {error:#}");
                    None
                }
            }
        } else {
            None
        };
        let indirect_scale = if raw.type_name == "NiFloatExtraDataController" && bytes.len() == 34 {
            match super::nif_properties::DirectIndirectScale::load(nif, uint(30), uint(26)) {
                Ok(channel) => channel,
                Err(error) => {
                    log::warn!("Direct indirect scale controller {id}: {error:#}");
                    None
                }
            }
        } else {
            None
        };
        result.push(Controller {
            block: id,
            class_name: raw.type_name.clone(),
            flags,
            frequency: f32::from_bits(uint(6)),
            phase: f32::from_bits(uint(10)),
            start: f32::from_bits(uint(14)),
            stop: f32::from_bits(uint(18)),
            target: link(22)?,
            transform,
            color,
            indirect_scale,
        });
    }
    Ok(result)
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Node {
    /// Cooked NiBillboardNode face flags, retained separately from AV flags.
    pub billboard_mode: Option<u16>,
    pub flags: u16,
    pub decal_type: i32,
    pub name: String,
    pub class_name: String,
    pub local: NiTransform,
    /// Null entries and authored child order are deliberately preserved.
    pub children: Vec<Option<u32>>,
    pub authored_bound: Option<Sphere>,
    /// Authored mesh bounds alone are insufficient for these runtime nodes.
    pub needs_runtime_bound: bool,
    pub runtime_geometry: bool,
    /// Native NiMorphMeshModifier marks rigid bound propagation; its cooked
    /// model sphere encloses the exported targets rather than a sampled pose.
    pub morph_bound: bool,
    pub controllers: Vec<Controller>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Hierarchy {
    pub roots: Vec<Option<u32>>,
    pub stream_roots: Vec<Option<u32>>,
    pub nodes: BTreeMap<u32, Node>,
    pub cameras: BTreeMap<u32, epic_mickey_lib_rs::nif_file::NiCameraData>,
}
impl Hierarchy {
    /// Native PC model-level wrapper layout. Caller supplies actual managed
    /// skin probe indices; None explicitly means no qualifying manager.
    pub(super) fn pc_sh_bindings(&self,root:u32,managed:Option<&[i32]>)->Result<Vec<super::d3d9_sh_probe::Binding>> {
        let group=self.find_named_node(root,"SH_probes")?;
        let count=group.map(|id|{
            let node=self.nodes.get(&id).context("SH probe group disappeared")?;
            ensure!(super::nif_types::is_a(&node.class_name,"NiNode")?,"SH_probes is not a NiNode");
            Ok::<_,anyhow::Error>(node.children.len())
        }).transpose()?;
        super::d3d9_sh_probe::bindings(managed,count,|name|self.find_named_node(group.context("Named SH bone lookup has no group")?,name))
    }
    /// REL2ea6a4: ordered depth-first NiNode children; return the first
    /// NiMesh-kind object and its immediate NiNode parent. A mesh root is
    /// not a substitute for the actor's required NiNode root.
    pub fn primary_actor_mesh(&self, root: u32) -> Result<Option<(u32, u32)>> {
        let node = self.nodes.get(&root).context("Actor model root missing")?;
        if !super::nif_types::is_a(&node.class_name, "NiNode")? {
            return Ok(None);
        }
        let mut pending: Vec<_> = node
            .children
            .iter()
            .rev()
            .flatten()
            .map(|&id| (root, id))
            .collect();
        let mut seen = std::collections::BTreeSet::new();
        while let Some((parent, id)) = pending.pop() {
            ensure!(seen.insert(id), "Repeated/cyclic actor child");
            let node = self.nodes.get(&id).context("Actor child missing")?;
            if super::nif_types::is_a(&node.class_name, "NiMesh")? {
                return Ok(Some((parent, id)));
            }
            if super::nif_types::is_a(&node.class_name, "NiNode")? {
                pending.extend(
                    node.children
                        .iter()
                        .rev()
                        .flatten()
                        .map(|&child| (id, child)),
                );
            }
        }
        Ok(None)
    }
    /// A streamed model can expose several roots. Cloning one NiNode owns
    /// only its reachable graph, not unrelated stream objects or aliases.
    pub fn subtree(&self, root: u32) -> Result<Self> {
        let mut nodes = BTreeMap::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            ensure!(
                !nodes.contains_key(&id),
                "Repeated/cyclic model subtree node {id}"
            );
            let node = self.nodes.get(&id).context("Model subtree node missing")?;
            pending.extend(node.children.iter().rev().flatten().copied());
            nodes.insert(id, node.clone());
        }
        let cameras = self
            .cameras
            .iter()
            .filter(|(id, _)| nodes.contains_key(id))
            .map(|(id, camera)| (*id, camera.clone()))
            .collect();
        Ok(Self {
            roots: vec![Some(root)],
            stream_roots: vec![Some(root)],
            nodes,
            cameras,
        })
    }
    /// Base NiNode/NiAVObject name search: root first, then children in authored
    /// order (DOL 0x804799b0). Derived search overrides require separate support.
    pub fn find_named_node(&self, root: u32, name: &str) -> Result<Option<u32>> {
        self.find_named_node_case(root,name,false)
    }
    /// Attachment binding uses NiNode slot5c (80479a68), whose base
    /// comparison804447a8 calls the case-folded8050388c/80024af8 routine.
    /// Ordinary slot50 above retains exact NiFixedString semantics.
    pub fn find_attachment_node(&self, root:u32, name:&str)->Result<Option<u32>> {
        self.find_named_node_case(root,name,true)
    }
    fn find_named_node_case(&self, root:u32, name:&str, insensitive:bool)->Result<Option<u32>> {
        ensure!(
            !name.is_empty(),
            "Empty runtime name requires NiFixedString null/empty provenance"
        );
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            let node = self
                .nodes
                .get(&id)
                .context("Name lookup root/child missing")?;
            // NonUniformParticleSystem slot50 inherits80444778; sorting
            // nodes inherit NiNode804799b0. Neither changes name lookup.
            ensure!(
                matches!(
                    node.class_name.as_str(),
                    "NiNode"
                        | "NiSortAdjustNode"
                        | "NiMesh"
                        | "NiBillboardNode"
                        | "NiPSParticleSystem"
                        | "NonUniformParticleSystem"
                        | "NiCamera"
                ),
                "Unverified name-search implementation for {}",
                node.class_name
            );
            if node.name == name || (insensitive && node.name.eq_ignore_ascii_case(name)) {
                return Ok(Some(id));
            }
            pending.extend(node.children.iter().rev().flatten().copied());
        }
        Ok(None)
    }
    /// Evaluate static graphs; dynamic bounds must come from their runtime
    /// update rather than silently substituting the authored rest-pose sphere.
    pub fn static_world_bounds(
        &self,
        parent: Option<&NiTransform>,
        previous: &BTreeMap<u32, Sphere>,
    ) -> Result<BTreeMap<u32, Sphere>> {
        let world = self.base_world_transforms(parent)?;
        self.bounds_from_world(&world, previous, false, false)
    }
    pub fn direct_world_bounds(
        &self,
        parent: &NiTransform,
        previous: &BTreeMap<u32, Sphere>,
        poses: &BTreeMap<u32, super::nif_transform::Pose>,
    ) -> Result<BTreeMap<u32, Sphere>> {
        let world = self.base_world_transforms_with_poses(Some(parent), poses)?;
        self.bounds_from_world(&world, previous, true, false)
    }
    /// The installed actor manager owns transform state, including its resting
    /// state before a sequence starts. Geometry-deforming controllers still
    /// need their own bound producer and cannot use this rigid-transform path.
    pub fn actor_world_bounds(
        &self,
        parent: &NiTransform,
        previous: &BTreeMap<u32, Sphere>,
        poses: &BTreeMap<u32, super::nif_transform::Pose>,
    ) -> Result<BTreeMap<u32, Sphere>> {
        let world = self.base_world_transforms_with_poses(Some(parent), poses)?;
        self.bounds_from_world(&world, previous, true, true)
    }
    /// A mesh's NiAVObject bound depends on its ancestor transforms and its
    /// own geometry producer, not a sibling's particle bound. Keep each
    /// unresolved branch explicit instead of rejecting every rigid sibling.
    pub fn mesh_world_bounds(
        &self,
        parent: &NiTransform,
        poses: &BTreeMap<u32, super::nif_transform::Pose>,
        direct: bool,
        managed: bool,
    ) -> Result<BTreeMap<u32, std::result::Result<Sphere, String>>> {
        ensure!(
            poses.keys().all(|id| self.nodes.contains_key(id)),
            "Animation targets a missing transform node"
        );
        let mut bounds = BTreeMap::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut pending: Vec<_> = self
            .roots
            .iter()
            .rev()
            .flatten()
            .map(|id| (*id, Ok(parent.clone())))
            .collect();
        while let Some((id, parent)) = pending.pop() {
            ensure!(seen.insert(id), "Repeated/cyclic mesh bound node {id}");
            let node = self.nodes.get(&id).context("Missing mesh bound node")?;
            let transform = (|| -> Result<NiTransform> {
                let parent = parent
                    .as_ref()
                    .map_err(|error: &String| anyhow::anyhow!(error.clone()))?;
                ensure!(
                    matches!(
                        node.class_name.as_str(),
                        "NiNode" | "JPSJigsawNode" | "NiSortAdjustNode" | "NiMesh"
                    ),
                    "Node {id} ({}) requires derived transform update",
                    node.class_name
                );
                ensure!(
                    !node.runtime_geometry,
                    "Node {id} ({}) requires runtime geometry bound",
                    node.class_name
                );
                ensure!(
                    node.controllers
                        .iter()
                        .all(|controller| !controller.affects_bound()
                            || (node.morph_bound
                                && controller.class_name == "NiMorphWeightsController")
                            || (direct
                                && controller.class_name == "NiTransformController"
                                && (managed
                                    || controller.flags & 0x20 == 0
                                    || controller.transform.is_none()))),
                    "Node {id} ({}) requires runtime controller update",
                    node.class_name
                );
                let local = poses
                    .get(&id)
                    .map(|pose| pose.apply_to(&node.local))
                    .transpose()?;
                compose(Some(parent), local.as_ref().unwrap_or(&node.local))
            })()
            .map_err(|error| format!("{error:#}"));
            if let Some(authored) = node.authored_bound {
                let bound = transform.as_ref().map_err(Clone::clone).and_then(|world| {
                    authored
                        .transformed(world)
                        .map_err(|error| format!("{error:#}"))
                });
                bounds.insert(id, bound);
            }
            pending.extend(
                node.children
                    .iter()
                    .rev()
                    .flatten()
                    .map(|child| (*child, transform.clone())),
            );
        }
        Ok(bounds)
    }
    pub(super) fn bounds_from_world(
        &self,
        world: &BTreeMap<u32, NiTransform>,
        previous: &BTreeMap<u32, Sphere>,
        direct: bool,
        managed: bool,
    ) -> Result<BTreeMap<u32, Sphere>> {
        let mut order = Vec::new();
        let mut pending: Vec<_> = self.roots.iter().rev().flatten().copied().collect();
        while let Some(id) = pending.pop() {
            let node = &self.nodes[&id];
            // Native8042115c returns immediately for manager-controlled objects.
            // An owned controller with a null interpolator retains the current
            // local transform; it is not an unresolved transform producer.
            let resolved = !node.runtime_geometry
                && !node.controllers.is_empty()
                && node.controllers.iter().all(|c| {
                    !c.affects_bound()
                        || (node.morph_bound && c.class_name == "NiMorphWeightsController")
                        || (direct
                            && c.class_name == "NiTransformController"
                            && (managed || c.flags & 0x20 == 0 || c.transform.is_none()))
                });
            ensure!(
                !node.needs_runtime_bound || resolved,
                "Node {id} ({}) requires runtime bound update; controllers: {:?}",
                node.class_name,
                node.controllers
                    .iter()
                    .map(|c| format!("{} #{} -> {:?}", c.class_name, c.block, c.target))
                    .collect::<Vec<_>>()
            );
            order.push(id);
            pending.extend(node.children.iter().rev().flatten().copied());
        }
        let mut bounds = BTreeMap::new();
        for id in order.into_iter().rev() {
            let node = &self.nodes[&id];
            let bound = if self.cameras.contains_key(&id) {
                // EM1 Camera UpdateWorldBound8044d16c moves the center only.
                let t = &world[&id].translation;
                Sphere {
                    center: [t.x, t.y, t.z],
                    radius: previous.get(&id).map_or(0., |sphere| sphere.radius),
                }
            } else if let Some(authored) = node.authored_bound {
                authored.transformed(&world[&id])?
            } else {
                let mut bound = previous.get(&id).copied().unwrap_or(Sphere {
                    center: [0.; 3],
                    radius: 0.,
                });
                bound.update_children(node.children.iter().flatten().map(|child| bounds[child]))?;
                bound
            };
            bounds.insert(id, bound);
        }
        Ok(bounds)
    }
    /// Base NiAVObject propagation. Derived nodes can change their world
    /// transforms later (billboards, controllers); this is not that update.
    pub fn base_world_transforms(
        &self,
        parent: Option<&NiTransform>,
    ) -> Result<BTreeMap<u32, NiTransform>> {
        self.base_world_transforms_with_poses(parent, &BTreeMap::new())
    }
    /// Apply resolved local animation channels before native parent propagation.
    /// This supplies transforms, not derived mesh/billboard bound updates.
    pub fn base_world_transforms_with_poses(
        &self,
        parent: Option<&NiTransform>,
        poses: &BTreeMap<u32, super::nif_transform::Pose>,
    ) -> Result<BTreeMap<u32, NiTransform>> {
        self.base_world_transforms_with_overrides(parent, poses, &BTreeMap::new())
    }
    /// Native model placement can copy a complete matrix without quaternion
    /// conversion. Local replacements win over controller channels for the
    /// supplied nodes, preserving every authored child transform.
    pub fn base_world_transforms_with_overrides(
        &self,
        parent: Option<&NiTransform>,
        poses: &BTreeMap<u32, super::nif_transform::Pose>,
        locals: &BTreeMap<u32, NiTransform>,
    ) -> Result<BTreeMap<u32, NiTransform>> {
        ensure!(
            poses.keys().all(|id| self.nodes.contains_key(id)),
            "Animation targets a missing transform node"
        );
        ensure!(
            locals.keys().all(|id| self.nodes.contains_key(id)),
            "Local override targets a missing transform node"
        );
        let mut world = BTreeMap::new();
        let mut pending: Vec<_> = self
            .roots
            .iter()
            .rev()
            .flatten()
            .map(|id| (*id, parent.cloned()))
            .collect();
        while let Some((id, parent)) = pending.pop() {
            ensure!(
                !world.contains_key(&id),
                "Repeated/cyclic transform node {id}"
            );
            let node = self
                .nodes
                .get(&id)
                .with_context(|| format!("Missing transform node {id}"))?;
            let local = poses
                .get(&id)
                .filter(|_| !locals.contains_key(&id))
                .map(|pose| pose.apply_to(&node.local))
                .transpose()?;
            let transform = compose(
                parent.as_ref(),
                locals.get(&id).or(local.as_ref()).unwrap_or(&node.local),
            )?;
            pending.extend(
                node.children
                    .iter()
                    .rev()
                    .flatten()
                    .map(|child| (*child, Some(transform.clone()))),
            );
            world.insert(id, transform);
        }
        Ok(world)
    }
    pub fn load_with_source(nif:&NifFile,source:&[u8])->Result<Self> {
        let mut graph=Self::load(nif)?;
        if graph.nodes.values().any(|n|n.class_name=="NiBillboardNode") {
            let blocks=super::jigsaw::raw_blocks(source)?;
            for (&id,node) in &mut graph.nodes {
                if node.class_name!="NiBillboardNode" {continue;}
                let raw=blocks.get(id as usize).context("Billboard source block missing")?;
                let bytes=raw.get(raw.len().checked_sub(2).context("Billboard flags missing")?..).context("Billboard flags truncated")?;
                let flags=if nif.source_is_little_endian {u16::from_le_bytes(bytes.try_into()?)}else{u16::from_be_bytes(bytes.try_into()?)};
                node.billboard_mode=Some(flags&7);
            }
        }
        Ok(graph)
    }
    /// Retain the actual NIF graph, not the editor's flattened draw geometry.
    /// Runtime subclasses remain identified even when the file parser exposes
    /// their shared NiNode prefix as NiObject::NiNode.
    pub fn load(nif: &NifFile) -> Result<Self> {
        let mut nodes = BTreeMap::new();
        let mut cameras = BTreeMap::new();
        let mut pending: Vec<_> = nif.roots.iter().rev().flatten().copied().collect();
        while let Some(id) = pending.pop() {
            // A stream root can alias a child (the shipped projector camera).
            // Decode once, then validate parent ownership/cycles below.
            if nodes.contains_key(&id) {
                continue;
            }
            let block = nif
                .blocks
                .get(id as usize)
                .with_context(|| format!("Missing bound node {id}"))?;
            let class_name = nif
                .block_type_indices
                .get(id as usize)
                .and_then(|i| nif.block_types.get(*i as usize))
                .map(String::as_str)
                .unwrap_or(block.type_name());
            let av = block.as_av_object().with_context(|| {
                let reason = if let NiObject::Unknown(raw) = block {
                    nif.decode_raw_block(raw)
                        .err()
                        .unwrap_or_else(|| "No typed AV prefix decoder".into())
                } else {
                    "No AV prefix".into()
                };
                format!("Unsupported bound node {id}: {class_name}: {reason}")
            })?;
            let t = &av.transform;
            ensure!(
                t.rotation
                    .rows
                    .iter()
                    .flatten()
                    .copied()
                    .chain([t.translation.x, t.translation.y, t.translation.z, t.scale])
                    .all(f32::is_finite),
                "Nonfinite transform on bound node {id}"
            );
            let (mesh, dynamic, morph_bound) = match block {
                // Cooked hardware skin data deforms render vertices, not the
                // ordinary NiMesh model sphere. Retail Mickey retains the
                // authored sphere through animation and uses 0x805455d4;
                // see fixtures/mickey_sh_bounds.json. Morph modifier selective
                // flags80561e00..14 are true/false/true (rigid); other modifier
                // policies and instancing still require their bound producers.
                NiObject::NiMesh(mesh) => {
                    let modifiers = mesh.modifiers.iter().flatten().copied().collect::<Vec<_>>();
                    let morph = !modifiers.is_empty()
                        && modifiers.iter().all(|modifier| {
                            nif.blocks
                                .get(*modifier as usize)
                                .is_some_and(|block| block.type_name() == "NiMorphMeshModifier")
                        });
                    if morph {
                        for modifier in &modifiers {
                            super::nif_sequence::MorphModifier::load(nif, *modifier)
                                .with_context(|| format!("Morph bound modifier {modifier}"))?;
                        }
                    }
                    let dynamic = class_name != "NiMesh"
                        || mesh.instancing_enabled != 0
                        || (!modifiers.is_empty() && !morph);
                    (Some(mesh), dynamic, morph && !dynamic)
                }
                NiObject::NiPSParticleSystem(p) => (Some(&p.base), true, false),
                // JPS factory2462c0 constructs2477d4 with vtable4e6b30.
                // Slots78/7c inherit804c4480/80478440, so its transform and
                // child-bound update use the ordinary NiNode implementation.
                NiObject::NiNode(_) => (
                    None,
                    !matches!(class_name, "NiNode" | "JPSJigsawNode"),
                    false,
                ),
                // EM1 NiSortAdjustNode vtable806d2970 slots78/7c inherit
                // 804c4480/80478440. Sorting does not alter bound propagation.
                NiObject::NiSortAdjustNode(_) => (None, false, false),
                NiObject::NiCamera(camera) => {
                    cameras.insert(id, camera.clone());
                    (None, false, false)
                }
                _ => unreachable!("as_av_object accepted an unhandled node"),
            };
            let authored_bound = mesh.map(|m| Sphere {
                center: [m.bound_center.x, m.bound_center.y, m.bound_center.z],
                radius: m.bound_radius,
            });
            if let Some(bound) = authored_bound {
                bound
                    .validate()
                    .with_context(|| format!("Mesh bound {id}"))?;
            }
            let children = block.children().to_vec();
            pending.extend(children.iter().rev().flatten().copied());
            let decal_type = av
                .base
                .extra_data_links
                .iter()
                .flatten()
                .find_map(|link| match nif.blocks.get(*link as usize) {
                    Some(NiObject::NiIntegerExtraData(data)) if data.base.name == "DecalType" => {
                        Some(data.value)
                    }
                    _ => None,
                })
                .unwrap_or(-1);
            let controllers = controllers(nif, av.base.controller_link)
                .with_context(|| format!("Bound node {id} controller chain"))?;
            let needs_runtime_bound = dynamic
                || controllers.iter().any(|controller| {
                    controller.affects_bound()
                        && !(morph_bound && controller.class_name == "NiMorphWeightsController")
                });
            nodes.insert(
                id,
                Node {
                    billboard_mode: None,
                    flags: av.flags,
                    decal_type,
                    name: av.base.name.clone(),
                    class_name: class_name.into(),
                    local: t.clone(),
                    children,
                    authored_bound,
                    needs_runtime_bound,
                    runtime_geometry: dynamic,
                    morph_bound,
                    controllers,
                },
            );
        }
        let mut parents = BTreeMap::new();
        for (&parent, node) in &nodes {
            for child in node.children.iter().flatten() {
                ensure!(
                    parents.insert(*child, parent).is_none(),
                    "Repeated/multiple-parent child node {child}"
                );
            }
        }
        let mut root_ids = std::collections::BTreeSet::new();
        let roots = nif
            .roots
            .iter()
            .copied()
            .filter(|root| match root {
                None => true,
                Some(id) => !parents.contains_key(id) && root_ids.insert(*id),
            })
            .collect::<Vec<_>>();
        let mut visited = std::collections::BTreeSet::new();
        let mut pending: Vec<_> = roots.iter().flatten().copied().collect();
        while let Some(id) = pending.pop() {
            ensure!(visited.insert(id), "Repeated/cyclic bound node {id}");
            pending.extend(nodes[&id].children.iter().flatten().copied());
        }
        ensure!(
            visited.len() == nodes.len(),
            "Cyclic bound graph has no independent runtime root"
        );
        Ok(Self {
            roots,
            stream_roots: nif.roots.clone(),
            nodes,
            cameras,
        })
    }
}

/// DOL 0x804c4480: root copies, then parent/local uniform transform
/// composition. Preserve paired-single accumulation order and FMA boundaries.
pub(super) fn compose(parent: Option<&NiTransform>, local: &NiTransform) -> Result<NiTransform> {
    let valid = |t: &NiTransform| {
        t.rotation
            .rows
            .iter()
            .flatten()
            .copied()
            .chain([t.translation.x, t.translation.y, t.translation.z, t.scale])
            .all(f32::is_finite)
    };
    ensure!(valid(local), "Invalid local transform");
    let Some(parent) = parent else {
        return Ok(local.clone());
    };
    ensure!(valid(parent), "Invalid parent transform");
    let mut result = local.clone();
    result.scale = parent.scale * local.scale;
    for row in 0..3 {
        let p = parent.rotation.rows[row];
        for col in 0..3 {
            let l = &local.rotation.rows;
            result.rotation.rows[row][col] =
                l[2][col].mul_add(p[2], l[1][col].mul_add(p[1], l[0][col] * p[0]));
        }
    }
    let v = [
        local.translation.x,
        local.translation.y,
        local.translation.z,
    ];
    let origin = [
        parent.translation.x,
        parent.translation.y,
        parent.translation.z,
    ];
    let translated: [f32; 3] = std::array::from_fn(|i| {
        let p = parent.rotation.rows[i];
        let rotated = p[2].mul_add(v[2], p[0] * v[0]) + p[1] * v[1];
        origin[i] + rotated * parent.scale
    });
    result.translation = epic_mickey_lib_rs::nif_file::Vector3 {
        x: translated[0],
        y: translated[1],
        z: translated[2],
    };
    ensure!(valid(&result), "World transform overflow");
    Ok(result)
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Sphere {
    pub center: [f32; 3],
    pub radius: f32,
}

impl Sphere {
    /// DOL 0x805455d4: Y product, then X and Z FMAs; scale and
    /// translation are separate operations (0x805ae990 / 0x805ae930).
    pub fn transformed(self, world: &NiTransform) -> Result<Self> {
        self.validate()?;
        let world = compose(None, world)?;
        let origin = [
            world.translation.x,
            world.translation.y,
            world.translation.z,
        ];
        let center = std::array::from_fn(|i| {
            let r = world.rotation.rows[i];
            let rotated = r[2].mul_add(
                self.center[2],
                r[0].mul_add(self.center[0], r[1] * self.center[1]),
            );
            rotated * world.scale + origin[i]
        });
        let result = Self {
            center,
            radius: world.scale * self.radius,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(self) -> Result<()> {
        ensure!(
            self.center.iter().all(|v| v.is_finite())
                && self.radius.is_finite()
                && self.radius >= 0.,
            "Invalid scene bounding sphere"
        );
        Ok(())
    }

    /// DOL 0x804c46b8. Containment retains an input exactly; expansion uses
    /// native paired-single dot order and the two-step Gekko square root.
    pub fn merge(self, other: Self) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        let delta: [f32; 3] = std::array::from_fn(|i| self.center[i] - other.center[i]);
        let squared = delta[0].mul_add(delta[0], delta[1] * delta[1]) + delta[2] * delta[2];
        let difference = other.radius - self.radius;
        let radius_squared = difference * difference;
        ensure!(
            squared.is_finite() && radius_squared.is_finite(),
            "Scene bound merge overflow"
        );
        if radius_squared >= squared {
            return Ok(if difference >= 0. { other } else { self });
        }
        let distance = super::spherical_lighting::native_sqrt(squared);
        let center = if distance > 1e-6_f32 {
            let factor = (distance - difference) / (2. * distance);
            std::array::from_fn(|i| other.center[i] + delta[i] * factor)
        } else {
            self.center
        };
        let result = Self {
            center,
            radius: 0.5 * (self.radius + (other.radius + distance)),
        };
        result.validate()?;
        Ok(result)
    }

    /// DOL 0x80478440..0x80478518. An empty node resets radius but retains
    /// its previous center; zero-radius children are nonvisual in this path.
    /// Do not reorder children: repeated sphere merging is not associative.
    pub fn update_children(&mut self, children: impl IntoIterator<Item = Self>) -> Result<()> {
        self.validate()?;
        let mut result = Self {
            center: self.center,
            radius: 0.,
        };
        for child in children {
            child.validate()?;
            if child.radius == 0. {
                continue;
            }
            result = if result.radius == 0. {
                child
            } else {
                result.merge(child)?
            };
        }
        *self = result;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_dbc_sort_adjust_hierarchy_loads_and_retains_dynamic_guards() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let stored = crate::pack_io::read_filtered(
            std::fs::read(std::path::Path::new(&root).join("packfiles/dbc_1st_zonea.pak"))?,
            |_, path| {
                path.replace('\\', "/").eq_ignore_ascii_case(
                    "Environments/ThePark/DarkBeautyCastle/Zones/DBC_ZoneD_Break_01.nif_wii",
                )
            },
        )?;
        let file = stored
            .pak
            .files
            .iter()
            .find(|file| !file.data.is_empty())
            .context("Missing original DBC model")?;
        let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
        let NiObject::NiSortAdjustNode(sort) = &nif.blocks[225] else {
            anyhow::bail!("Original block225 lost sorting node class")
        };
        ensure!(
            sort.base.children.is_empty() && sort.base.effects.is_empty(),
            "Original empty sorting node changed links"
        );
        ensure!(
            sort.sorting_mode == 0 && sort.base.base.flags == 0x117,
            "Original sorting metadata changed"
        );
        let hierarchy = Hierarchy::load(&nif)?;
        ensure!(
            hierarchy.nodes.contains_key(&225),
            "Original empty sorting node lost its hierarchy entry"
        );
        ensure!(
            hierarchy
                .nodes
                .values()
                .any(|node| node.authored_bound.is_some()),
            "Original model lost mesh bounds"
        );
        // This effect-bearing model also has animated nodes and particles.
        // Loading their graph must not silently authorize static bound substitution.
        let error = hierarchy
            .static_world_bounds(None, &BTreeMap::new())
            .unwrap_err();
        ensure!(
            format!("{error:#}").contains("requires runtime bound update"),
            "Unexpected dynamic-bound guard: {error:#}"
        );
        let meshes = hierarchy.mesh_world_bounds(
            &NiTransform {
                scale: 1.,
                ..Default::default()
            },
            &BTreeMap::new(),
            true,
            true,
        )?;
        ensure!(
            meshes.values().any(|bound| bound.is_ok()),
            "Original model lost all supported rigid mesh bounds"
        );
        ensure!(
            meshes.values().any(|bound| bound.is_err()),
            "Original particles were silently admitted as rigid geometry"
        );
        Ok(())
    }
    #[test]
    fn independent_mesh_bounds_do_not_admit_unsupported_ancestors_or_siblings() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{
            NiMeshData, NiNodeData, NiPSParticleSystemData, NiSortAdjustNodeData,
        };
        let mut root = NiNodeData::default();
        root.children = vec![Some(1), Some(2), Some(3)];
        root.base.transform.translation.x = 4.;
        root.base.transform.scale = 1.;
        let mut mesh = NiMeshData {
            bound_radius: 2.,
            ..Default::default()
        };
        mesh.base.base.transform.translation.y = 3.;
        mesh.base.base.transform.scale = 1.;
        let mut unsafe_parent = NiNodeData::default();
        unsafe_parent.children = vec![Some(4)];
        unsafe_parent.base.transform.scale = 1.;
        let nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![
                NiObject::NiNode(root),
                NiObject::NiMesh(mesh.clone()),
                NiObject::NiPSParticleSystem(NiPSParticleSystemData::default()),
                NiObject::NiNode(unsafe_parent),
                NiObject::NiMesh(mesh),
            ],
            block_types: vec![
                "NiNode".into(),
                "NiMesh".into(),
                "NiPSParticleSystem".into(),
                "NiBillboardNode".into(),
            ],
            block_type_indices: vec![0, 1, 2, 3, 1],
            ..Default::default()
        };
        let graph = Hierarchy::load(&nif)?;
        assert!(graph.static_world_bounds(None, &BTreeMap::new()).is_err());
        let bounds = graph.mesh_world_bounds(
            &NiTransform {
                scale: 1.,
                ..Default::default()
            },
            &BTreeMap::new(),
            false,
            false,
        )?;
        let rigid = bounds[&1]
            .as_ref()
            .map_err(|error| anyhow::anyhow!(error.clone()))?;
        assert_eq!(
            *rigid,
            Sphere {
                center: [4., 3., 0.],
                radius: 2.
            }
        );
        assert!(bounds[&2].is_err());
        assert!(bounds[&4].is_err());
        // A verified inherited node preserves the same child transform result.
        let mut sorted = nif.clone();
        let NiObject::NiNode(parent) = sorted.blocks[3].clone() else {
            unreachable!()
        };
        sorted.blocks[3] = NiObject::NiSortAdjustNode(NiSortAdjustNodeData {
            base: parent,
            ..Default::default()
        });
        sorted.block_types[3] = "NiSortAdjustNode".into();
        let sorted = Hierarchy::load(&sorted)?.mesh_world_bounds(
            &NiTransform {
                scale: 1.,
                ..Default::default()
            },
            &BTreeMap::new(),
            false,
            false,
        )?;
        assert_eq!(sorted[&4], bounds[&1]);
        Ok(())
    }
    #[test]
    fn sort_adjust_node_preserves_child_bound_propagation() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiMeshData, NiSortAdjustNodeData};
        let mut root = NiSortAdjustNodeData::default();
        root.base.children = vec![None, Some(1)];
        root.sorting_mode = 1;
        let mesh = NiMeshData {
            bound_radius: 2.,
            ..Default::default()
        };
        let nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![NiObject::NiSortAdjustNode(root), NiObject::NiMesh(mesh)],
            ..Default::default()
        };
        let hierarchy = Hierarchy::load(&nif)?;
        let bounds = hierarchy.static_world_bounds(None, &BTreeMap::new())?;
        assert_eq!(bounds[&0], bounds[&1]);
        assert_eq!(hierarchy.nodes[&0].class_name, "NiSortAdjustNode");
        Ok(())
    }
    #[test]
    fn jigsaw_node_uses_inherited_bound_update_without_admitting_unknown_subclasses() -> Result<()>
    {
        use epic_mickey_lib_rs::nif_file::{NiMeshData, NiNodeData, Vector3};
        let mut root = NiNodeData::default();
        root.base.transform.scale = 1.;
        root.children = vec![Some(1)];
        let mut mesh = NiMeshData::default();
        mesh.base.base.transform.scale = 1.;
        mesh.bound_center = Vector3 {
            x: 1.,
            y: 2.,
            z: 3.,
        };
        mesh.bound_radius = 4.;
        let mut nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![NiObject::NiNode(root), NiObject::NiMesh(mesh)],
            block_types: vec!["JPSJigsawNode".into(), "NiMesh".into()],
            block_type_indices: vec![0, 1],
            ..Default::default()
        };
        let graph = Hierarchy::load(&nif)?;
        let bounds = graph.static_world_bounds(None, &BTreeMap::new())?;
        assert_eq!(
            bounds[&0],
            Sphere {
                center: [1., 2., 3.],
                radius: 4.
            }
        );
        assert_eq!(bounds[&0], bounds[&1]);
        nif.block_types[0] = "UnverifiedNodeSubclass".into();
        assert!(Hierarchy::load(&nif)?
            .static_world_bounds(None, &BTreeMap::new())
            .is_err());
        Ok(())
    }
    #[test]
    fn controller_chains_preserve_order_targets_and_endianness() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiNodeData, RawBlockData};
        for little in [false, true] {
            let raw = |next: u32, name: &str| {
                let mut bytes = Vec::new();
                bytes.extend(if little {
                    next.to_le_bytes()
                } else {
                    next.to_be_bytes()
                });
                bytes.extend(if little {
                    0x004cu16.to_le_bytes()
                } else {
                    0x004cu16.to_be_bytes()
                });
                for bits in [
                    2f32.to_bits(),
                    (-1f32).to_bits(),
                    0f32.to_bits(),
                    3f32.to_bits(),
                    0,
                ] {
                    bytes.extend(if little {
                        bits.to_le_bytes()
                    } else {
                        bits.to_be_bytes()
                    });
                }
                if name == "NiTransformController" {
                    bytes.extend([0xff; 4]);
                }
                NiObject::Unknown(RawBlockData {
                    type_name: name.into(),
                    raw_data: bytes,
                })
            };
            let mut root = NiNodeData::default();
            root.base.base.controller_link = Some(1);
            let mut nif = NifFile {
                source_is_little_endian: little,
                roots: vec![Some(0)],
                blocks: vec![
                    NiObject::NiNode(root),
                    raw(2, "NiTransformController"),
                    raw(u32::MAX, "NiVisController"),
                ],
                ..Default::default()
            };
            let graph = Hierarchy::load(&nif)?;
            let chain = &graph.nodes[&0].controllers;
            assert_eq!(chain.iter().map(|c| c.block).collect::<Vec<_>>(), [1, 2]);
            assert_eq!(chain[0].target, Some(0));
            assert_eq!(chain[0].frequency, 2.);
            assert_eq!(chain[0].phase, -1.);
            assert_eq!(chain[0].stop, 3.);
            assert_eq!(chain[0].flags, 0x4c);
            assert!(graph
                .static_world_bounds(None, &BTreeMap::new())
                .unwrap_err()
                .to_string()
                .contains("NiTransformController #1"));
            nif.blocks[2] = raw(1, "NiVisController");
            assert!(
                format!("{:#}", Hierarchy::load(&nif).unwrap_err()).contains("Cyclic controller")
            );
            nif.blocks[2] = raw(99, "NiVisController");
            assert!(format!("{:#}", Hierarchy::load(&nif).unwrap_err()).contains("missing link"));
            if let NiObject::Unknown(data) = &mut nif.blocks[2] {
                data.raw_data.truncate(25);
            }
            assert!(format!("{:#}", Hierarchy::load(&nif).unwrap_err())
                .contains("Truncated controller"));
        }
        Ok(())
    }
    #[test]
    fn passive_controller_chains_keep_bounds_without_admitting_geometry_updates() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiMeshData, NiNodeData, RawBlockData, Vector3};
        for little in [false, true] {
            let raw = |next: u32, name: &str| {
                let mut bytes = Vec::new();
                bytes.extend(if little {
                    next.to_le_bytes()
                } else {
                    next.to_be_bytes()
                });
                bytes.extend(if little {
                    0x20u16.to_le_bytes()
                } else {
                    0x20u16.to_be_bytes()
                });
                for bits in [1f32.to_bits(), 0, 0, 1f32.to_bits(), 0] {
                    bytes.extend(if little {
                        bits.to_le_bytes()
                    } else {
                        bits.to_be_bytes()
                    });
                }
                NiObject::Unknown(RawBlockData {
                    type_name: name.into(),
                    raw_data: bytes,
                })
            };
            let mut root = NiNodeData::default();
            root.children = vec![Some(1)];
            root.base.transform.scale = 1.;
            root.base.base.controller_link = Some(2);
            let mut mesh = NiMeshData::default();
            mesh.bound_center = Vector3 {
                x: 2.,
                y: 3.,
                z: 4.,
            };
            mesh.bound_radius = 5.;
            mesh.base.base.transform.scale = 1.;
            let nif = NifFile {
                source_is_little_endian: little,
                roots: vec![Some(0)],
                blocks: vec![
                    NiObject::NiNode(root),
                    NiObject::NiMesh(mesh),
                    raw(3, "NiVisController"),
                    raw(4, "NiFloatExtraDataController"),
                    raw(u32::MAX, "NiColorExtraDataController"),
                ],
                ..Default::default()
            };
            let mut graph = Hierarchy::load(&nif)?;
            assert!(!graph.nodes[&0].needs_runtime_bound);
            let original = graph.static_world_bounds(None, &BTreeMap::new())?;
            assert_eq!(original[&0], original[&1]);
            graph.nodes.get_mut(&0).unwrap().flags |= 1;
            assert_eq!(graph.static_world_bounds(None, &original)?, original);
            graph.nodes.get_mut(&0).unwrap().needs_runtime_bound = true;
            graph.nodes.get_mut(&0).unwrap().runtime_geometry = true;
            assert!(graph.static_world_bounds(None, &original).is_err());
            graph.nodes.get_mut(&0).unwrap().runtime_geometry = false;
            graph.nodes.get_mut(&0).unwrap().controllers[1].class_name =
                "UnknownGeometryController".into();
            assert!(graph.static_world_bounds(None, &original).is_err());
        }
        Ok(())
    }
    #[test]
    fn known_morph_modifier_preserves_cooked_envelope_but_instancing_and_bad_modifiers_reject(
    ) -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiMeshData, NiNodeData, RawBlockData, Vector3};
        for little in [false, true] {
            let word = |value: u32| {
                if little {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                }
            };
            let mut modifier = Vec::new();
            for value in [0, 0] {
                modifier.extend(word(value));
            }
            modifier.push(0);
            modifier.extend(if little {
                2u16.to_le_bytes()
            } else {
                2u16.to_be_bytes()
            });
            for value in [1, 0, 0, 0] {
                modifier.extend(word(value));
            }
            let mut controller = Vec::new();
            controller.extend(word(u32::MAX));
            controller.extend(if little {
                0x20u16.to_le_bytes()
            } else {
                0x20u16.to_be_bytes()
            });
            for value in [1f32.to_bits(), 0, 0, 1f32.to_bits(), 1] {
                controller.extend(word(value));
            }
            let mut root = NiNodeData::default();
            root.children = vec![Some(1)];
            root.base.transform.scale = 1.;
            let mut mesh = NiMeshData::default();
            mesh.bound_center = Vector3 {
                x: 2.,
                y: 3.,
                z: 4.,
            };
            mesh.bound_radius = 5.;
            mesh.base.base.transform.scale = 1.;
            mesh.modifiers = vec![Some(2)];
            mesh.base.base.base.controller_link = Some(3);
            let mut nif = NifFile {
                version: 0x14060500,
                source_is_little_endian: little,
                roots: vec![Some(0)],
                string_table: vec!["POSITION".into()],
                blocks: vec![
                    NiObject::NiNode(root),
                    NiObject::NiMesh(mesh),
                    NiObject::Unknown(RawBlockData {
                        type_name: "NiMorphMeshModifier".into(),
                        raw_data: modifier,
                    }),
                    NiObject::Unknown(RawBlockData {
                        type_name: "NiMorphWeightsController".into(),
                        raw_data: controller,
                    }),
                ],
                ..Default::default()
            };
            let graph = Hierarchy::load(&nif)?;
            assert!(graph.nodes[&1].morph_bound);
            assert!(!graph.nodes[&1].runtime_geometry);
            let bounds = graph.static_world_bounds(None, &BTreeMap::new())?;
            assert_eq!(
                bounds[&1],
                Sphere {
                    center: [2., 3., 4.],
                    radius: 5.
                }
            );
            if let NiObject::NiMesh(mesh) = &mut nif.blocks[1] {
                mesh.instancing_enabled = 1;
            }
            assert!(Hierarchy::load(&nif)?
                .static_world_bounds(None, &bounds)
                .is_err());
            if let NiObject::NiMesh(mesh) = &mut nif.blocks[1] {
                mesh.instancing_enabled = 0;
            }
            if let NiObject::Unknown(raw) = &mut nif.blocks[2] {
                raw.raw_data.truncate(1);
            }
            assert!(Hierarchy::load(&nif).is_err());
            if let NiObject::Unknown(raw) = &mut nif.blocks[2] {
                raw.type_name = "UnsupportedBoundModifier".into();
            }
            assert!(Hierarchy::load(&nif)?
                .static_world_bounds(None, &bounds)
                .is_err());
        }
        Ok(())
    }
    #[test]
    fn original_character_sh_spheres_match_native_mesh_transform_bit_for_bit() -> Result<()> {
        let decode = |value: &serde_json::Value| -> Result<Vec<f32>> {
            let text = value.as_str().context("Missing captured floats")?;
            ensure!(text.len() % 8 == 0, "Malformed captured float bits");
            (0..text.len())
                .step_by(8)
                .map(|at| Ok(f32::from_bits(u32::from_str_radix(&text[at..at + 8], 16)?)))
                .collect()
        };
        for text in [
            include_str!("../../tools/blotworx/fixtures/mickey_sh_bounds.json"),
            include_str!("../../tools/blotworx/fixtures/brush_sh_bound.json"),
        ] {
            let evidence: serde_json::Value = serde_json::from_str(text)?;
            for sample in evidence["samples"]
                .as_array()
                .context("Missing captured frames")?
            {
                let model = decode(&sample["model_bound"])?;
                let values = decode(&sample["world_transform"])?;
                let expected = decode(&sample["world_bound"])?;
                ensure!(
                    model.len() == 4 && values.len() == 13 && expected.len() == 4,
                    "Malformed captured layout"
                );
                let world = NiTransform {
                    rotation: epic_mickey_lib_rs::nif_file::Matrix3 {
                        rows: std::array::from_fn(|r| std::array::from_fn(|c| values[r * 3 + c])),
                    },
                    translation: epic_mickey_lib_rs::nif_file::Vector3 {
                        x: values[9],
                        y: values[10],
                        z: values[11],
                    },
                    scale: values[12],
                };
                let sphere = Sphere {
                    center: model[..3].try_into()?,
                    radius: model[3],
                }
                .transformed(&world)?;
                let actual = [
                    sphere.center[0],
                    sphere.center[1],
                    sphere.center[2],
                    sphere.radius,
                ];
                assert_eq!(
                    actual.map(f32::to_bits),
                    expected
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>()
                        .as_slice()
                );
            }
        }
        Ok(())
    }
    #[test]
    fn owned_empty_managed_transform_preserves_bounds_but_unresolved_interpolator_still_rejects(
    ) -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiMeshData, NiNodeData, Vector3};
        let mut root = NiNodeData::default();
        root.children = vec![Some(1)];
        root.base.transform.scale = 1.;
        let mut mesh = NiMeshData::default();
        mesh.base.base.transform.scale = 1.;
        mesh.bound_center = Vector3 {
            x: 3.,
            y: 4.,
            z: 5.,
        };
        mesh.bound_radius = 2.;
        let mut graph = Hierarchy::load(&NifFile {
            roots: vec![Some(0)],
            blocks: vec![NiObject::NiNode(root), NiObject::NiMesh(mesh)],
            ..Default::default()
        })?;
        let node = graph.nodes.get_mut(&0).unwrap();
        node.needs_runtime_bound = true;
        node.controllers.push(Controller {
            block: 9,
            class_name: "NiTransformController".into(),
            color: None,
            indirect_scale: None,
            flags: 108,
            frequency: 1.,
            phase: 0.,
            start: f32::MAX,
            stop: -f32::MAX,
            target: Some(0),
            transform: None,
        });
        let mut parent = NiTransform::default();
        parent.scale = 2.;
        parent.translation.x = 10.;
        assert!(graph
            .static_world_bounds(Some(&parent), &BTreeMap::new())
            .is_err());
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/managed_transform.json"
        ))?;
        for case in reference["cases"]
            .as_array()
            .context("Missing managed transform cases")?
        {
            let flags = case["flags"].as_u64().context("Missing native flags")? as u16;
            assert_eq!(case["target_unchanged"], true);
            assert_eq!(case["controller_unchanged"], true);
            graph.nodes.get_mut(&0).unwrap().controllers[0].flags = flags;
            let mut playback =
                super::super::nif_transform::Playback::load(&graph.nodes[&0].controllers[0])?;
            assert!(matches!(
                playback.update(case["time"].as_f64().unwrap() as f32)?,
                super::super::nif_transform::Update::ManagerControlled
            ));
            assert!(graph
                .direct_world_bounds(&parent, &BTreeMap::new(), &BTreeMap::new())
                .is_ok());
        }
        let result = graph.direct_world_bounds(&parent, &BTreeMap::new(), &BTreeMap::new())?;
        assert_eq!(
            result[&1],
            Sphere {
                center: [16., 8., 10.],
                radius: 4.
            }
        );
        assert_eq!(
            graph.mesh_world_bounds(&parent, &BTreeMap::new(), true, false)?[&1]
                .as_ref()
                .unwrap(),
            &result[&1]
        );
        graph.nodes.get_mut(&0).unwrap().controllers[0].transform =
            Some(Err("Unresolved interpolation payload".into()));
        assert!(graph
            .direct_world_bounds(&parent, &BTreeMap::new(), &BTreeMap::new())
            .is_err());
        assert!(graph.mesh_world_bounds(&parent, &BTreeMap::new(), true, false)?[&1].is_err());
        Ok(())
    }
    #[test]
    fn actor_bound_owner_uses_resting_and_animated_transform_state_without_admitting_deforming_nodes(
    ) -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiMeshData, NiNodeData, Vector3};
        let mut root = NiNodeData::default();
        root.children = vec![Some(1)];
        root.base.transform.translation = Vector3 {
            x: 10.,
            y: 0.,
            z: 0.,
        };
        root.base.transform.scale = 2.;
        let mut mesh = NiMeshData::default();
        mesh.base.base.transform.scale = 1.;
        mesh.bound_center = Vector3 {
            x: 3.,
            y: 4.,
            z: 5.,
        };
        mesh.bound_radius = 2.;
        let nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![NiObject::NiNode(root), NiObject::NiMesh(mesh)],
            ..Default::default()
        };
        let mut graph = Hierarchy::load(&nif)?;
        let node = graph.nodes.get_mut(&1).unwrap();
        node.needs_runtime_bound = true;
        node.controllers.push(Controller {
            block: 2,
            class_name: "NiTransformController".into(),
            color: None,
            indirect_scale: None,
            flags: 0x20,
            frequency: 1.,
            phase: 0.,
            start: 0.,
            stop: 1.,
            target: Some(1),
            transform: None,
        });
        let previous = BTreeMap::new();
        let mut poses = BTreeMap::new();
        let parent = NiTransform {
            scale: 1.,
            ..Default::default()
        };
        assert!(graph.static_world_bounds(Some(&parent), &previous).is_err());
        assert!(graph
            .direct_world_bounds(&parent, &previous, &poses)
            .is_ok());
        let rest = graph.actor_world_bounds(&parent, &previous, &poses)?;
        assert_eq!(rest[&1].center, [16., 8., 10.]);
        poses.insert(
            1,
            super::super::nif_transform::Pose {
                translation: Some([5., 0., 0.]),
                rotation: None,
                scale: None,
            },
        );
        let animated = graph.actor_world_bounds(&parent, &rest, &poses)?;
        assert_eq!(animated[&1].center, [26., 8., 10.]);
        assert_eq!(animated[&1].radius, 4.);
        graph.nodes.get_mut(&1).unwrap().runtime_geometry = true;
        assert!(graph.actor_world_bounds(&parent, &rest, &poses).is_err());
        graph.nodes.get_mut(&1).unwrap().runtime_geometry = false;
        graph.nodes.get_mut(&1).unwrap().controllers[0].class_name =
            "NiFloatExtraDataController".into();
        assert_eq!(
            graph.actor_world_bounds(&parent, &rest, &poses)?[&1],
            animated[&1]
        );
        graph.nodes.get_mut(&1).unwrap().controllers[0].class_name =
            "NiMorphWeightsController".into();
        assert!(graph.actor_world_bounds(&parent, &rest, &poses).is_err());
        Ok(())
    }
    #[test]
    fn static_bounds_use_authored_center_and_reject_dynamic_nodes() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiMeshData, NiNodeData, Vector3};
        let mut root = NiNodeData::default();
        root.children = vec![Some(1)];
        root.base.transform.translation = Vector3 {
            x: 10.,
            y: 0.,
            z: 0.,
        };
        root.base.transform.scale = 2.;
        let mut mesh = NiMeshData::default();
        mesh.bound_center = Vector3 {
            x: 3.,
            y: 4.,
            z: 5.,
        };
        mesh.bound_radius = 2.;
        mesh.base.base.transform.scale = 1.;
        let nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![NiObject::NiNode(root), NiObject::NiMesh(mesh)],
            ..Default::default()
        };
        let mut graph = Hierarchy::load(&nif)?;
        let bounds = graph.static_world_bounds(None, &BTreeMap::new())?;
        assert_eq!(
            bounds[&0],
            Sphere {
                center: [16., 8., 10.],
                radius: 4.
            }
        );
        assert_eq!(bounds[&0], bounds[&1]);
        graph.nodes.get_mut(&1).unwrap().needs_runtime_bound = true;
        assert!(graph.static_world_bounds(None, &bounds).is_err());
        Ok(())
    }
    #[test]
    fn exact_name_search_obeys_authored_child_order_and_root_precedence() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::NiNodeData;
        let mut root = NiNodeData::default();
        root.base.base.name = "root".into();
        root.children = vec![None, Some(2), Some(1)];
        let mut first = NiNodeData::default();
        first.base.base.name = "body".into();
        let second = first.clone();
        let nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![
                NiObject::NiNode(root),
                NiObject::NiNode(first),
                NiObject::NiNode(second),
            ],
            ..Default::default()
        };
        let mut graph = Hierarchy::load(&nif)?;
        assert_eq!(graph.find_named_node(0, "body")?, Some(2));
        assert_eq!(graph.find_named_node(0, "Body")?, None);
        assert_eq!(graph.find_named_node(1, "root")?, None);
        graph.nodes.get_mut(&0).unwrap().name = "body".into();
        assert_eq!(graph.find_named_node(0, "body")?, Some(0));
        assert!(graph.find_named_node(0, "").is_err());
        assert!(graph.find_named_node(99, "body").is_err());
        Ok(())
    }
    #[test]
    fn parent_rotation_scale_and_translation_propagate() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiNodeData, Vector3};
        let mut root = NiNodeData::default();
        root.base.transform.rotation.rows = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
        root.base.transform.translation = Vector3 {
            x: 10.,
            y: 20.,
            z: 30.,
        };
        root.base.transform.scale = 2.;
        root.children = vec![Some(1)];
        let mut child = NiNodeData::default();
        child.base.transform.translation = Vector3 {
            x: 3.,
            y: 4.,
            z: 5.,
        };
        child.base.transform.scale = 0.5;
        let nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![NiObject::NiNode(root), NiObject::NiNode(child)],
            ..Default::default()
        };
        let graph = Hierarchy::load(&nif)?;
        let world = graph.base_world_transforms(None)?;
        let t = &world[&1];
        assert_eq!(
            [t.translation.x, t.translation.y, t.translation.z],
            [2., 26., 40.]
        );
        assert_eq!(t.scale, 1.);
        assert_eq!(t.rotation.rows, world[&0].rotation.rows);
        let mut invalid = t.clone();
        invalid.scale = f32::INFINITY;
        assert!(graph.base_world_transforms(Some(&invalid)).is_err());
        Ok(())
    }
    #[test]
    fn hierarchy_keeps_null_slots_spheres_and_dynamic_subclasses() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiMeshData, NiNodeData, Vector3};
        let mut parent = NiNodeData::default();
        parent.children = vec![None, Some(2), Some(1)];
        let mut mesh = NiMeshData::default();
        mesh.bound_center = Vector3 {
            x: 3.,
            y: 4.,
            z: 5.,
        };
        mesh.bound_radius = 2.;
        let mut nif = NifFile {
            roots: vec![Some(0)],
            blocks: vec![
                NiObject::NiNode(parent),
                NiObject::NiMesh(mesh),
                NiObject::NiNode(NiNodeData::default()),
            ],
            block_types: vec!["NiNode".into(), "NiMesh".into(), "NiBillboardNode".into()],
            block_type_indices: vec![0, 1, 2],
            ..Default::default()
        };
        let graph = Hierarchy::load(&nif)?;
        assert_eq!(graph.nodes[&0].children, vec![None, Some(2), Some(1)]);
        assert_eq!(
            graph.nodes[&1].authored_bound,
            Some(Sphere {
                center: [3., 4., 5.],
                radius: 2.
            })
        );
        assert!(graph.nodes[&2].needs_runtime_bound);
        assert!(!graph.nodes[&1].needs_runtime_bound);
        if let NiObject::NiMesh(mesh) = &mut nif.blocks[1] {
            mesh.instancing_enabled = 1;
        }
        assert!(Hierarchy::load(&nif)?.nodes[&1].needs_runtime_bound);
        if let NiObject::NiNode(node) = &mut nif.blocks[2] {
            node.children.push(Some(0));
        }
        assert!(Hierarchy::load(&nif).is_err());
        if let NiObject::NiNode(node) = &mut nif.blocks[2] {
            node.children = vec![Some(99)];
        }
        assert!(Hierarchy::load(&nif).is_err());
        Ok(())
    }
    #[test]
    fn containment_and_expansion_keep_native_center() -> Result<()> {
        let a = Sphere {
            center: [0.; 3],
            radius: 2.,
        };
        let inside = Sphere {
            center: [0.5, 0., 0.],
            radius: 0.5,
        };
        assert_eq!(a.merge(inside)?, a);
        assert_eq!(inside.merge(a)?, a);
        let b = Sphere {
            center: [6., 0., 0.],
            radius: 2.,
        };
        let merged = a.merge(b)?;
        assert!((merged.center[0] - 3.).abs() < 1e-6);
        assert!((merged.radius - 5.).abs() < 1e-6);
        let b = Sphere {
            center: [6., 0., 0.],
            radius: 4.,
        };
        let merged = a.merge(b)?;
        assert!((merged.center[0] - 4.).abs() < 1e-6);
        assert!((merged.radius - 6.).abs() < 1e-6);
        Ok(())
    }
    #[test]
    fn empty_children_and_order_match_node_update() -> Result<()> {
        let mut node = Sphere {
            center: [10., 20., 30.],
            radius: 7.,
        };
        node.update_children([Sphere {
            center: [999.; 3],
            radius: 0.,
        }])?;
        assert_eq!(
            node,
            Sphere {
                center: [10., 20., 30.],
                radius: 0.
            }
        );
        let a = Sphere {
            center: [0.; 3],
            radius: 1.,
        };
        let b = Sphere {
            center: [6., 0., 0.],
            radius: 1.,
        };
        let c = Sphere {
            center: [0., 6., 0.],
            radius: 1.,
        };
        node.update_children([a, b, c])?;
        assert_eq!(node, a.merge(b)?.merge(c)?);
        assert_ne!(node, a.merge(c)?.merge(b)?);
        let before = node;
        assert!(node
            .update_children([Sphere {
                center: [f32::NAN; 3],
                radius: 1.
            }])
            .is_err());
        assert_eq!(node, before);
        Ok(())
    }
}
