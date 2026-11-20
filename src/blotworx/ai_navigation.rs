//! Runtime route guidance from the authored EM1 Kynapse HPD/prelinked files.
//!
//! This keeps the retail graph as-authored and supplies graph waypoints to the
//! provisional gameplay AI adapter. It does not claim to reproduce Kynapse's
//! agent clearance, movement-type filtering, sector handoff, or behavior policy.
use crate::ai_nav::{Database, Sector};
use epic_mickey_lib_rs::{packfile::VirtualFile, scene_file::Component};
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
    path::{Component as PathComponent, Path},
};

#[derive(Default)]
pub struct Runtime {
    databases: Vec<LoadedDatabase>,
    route_cache: HashMap<(usize, usize, usize, usize), Option<Vec<usize>>>,
    pub warnings: Vec<String>,
}

struct LoadedDatabase {
    path: String,
    sectors: Vec<LoadedSector>,
    mesh: super::ai_runtime_mesh::Store,
}

struct LoadedSector {
    min: [f32; 3],
    max: [f32; 3],
    graph: Sector,
}

fn key(path: &str) -> String {
    path.trim_matches([' ', '"'])
        .replace('\\', "/")
        .trim_matches('/')
        .to_ascii_lowercase()
}

fn string_property<'a>(component: &'a Component, name: &str) -> Option<&'a str> {
    component
        .properties
        .iter()
        .find(|property| property.name == name)
        .and_then(|property| property.value.as_str())
}

impl Runtime {
    /// Load only databases actually referenced by loaded AI path components.
    /// The source bytes remain owned by the parsed graph and are never rewritten.
    pub fn load(
        entities: &[super::assets::RuntimeEntity],
        files: &[VirtualFile],
        data_root: &Path,
    ) -> Self {
        let mut runtime = Self::default();
        runtime.admit(entities,files,data_root);
        runtime
    }
    /// Append graph resources for newly constructed actors/scenes. Stable
    /// database indices keep existing cached routes valid.
    pub(super) fn admit(&mut self,entities:&[super::assets::RuntimeEntity],files:&[VirtualFile],data_root:&Path) {
        let mut requested = BTreeSet::new();
        for entity in entities {
            for component in entity
                .components
                .iter()
                .filter(|component| component.class_name == "AIPathDatabaseComponent")
            {
                if let Some(path) = string_property(component, "PathData HPD Filename") {
                    if !path.trim().is_empty() {
                        requested.insert(key(path));
                    }
                }
            }
        }
        let sources: HashMap<_, _> = files.iter().map(|file| (key(&file.path), file)).collect();
        for path in requested {
            if self.databases.iter().any(|database|database.path==path) {continue;}
            let Some(file) = sources.get(&path) else {
                self.warnings.push(format!(
                    "Authored AI navigation database is missing: {path}"
                ));
                continue;
            };
            let database = match Database::load(&file.data) {
                Ok(database) => database,
                Err(error) => {
                    self.warnings.push(format!(
                        "Could not parse AI navigation database {path}: {error:#}"
                    ));
                    continue;
                }
            };
            let mut loaded = LoadedDatabase {
                path: path.clone(),
                sectors: Vec::with_capacity(database.sectors.len()),
                mesh: Default::default(),
            };
            for record in &database.sectors {
                let Some(sector_path) = Database::sector_path(&path, record.id) else {
                    self.warnings.push(format!(
                        "Could not derive sector resource path for {path}, sector {}",
                        record.id
                    ));
                    continue;
                };
                let Some(sector_bytes) = sources
                    .get(&key(&sector_path))
                    .map(|file| Cow::Borrowed(file.data.as_slice()))
                    .or_else(|| read_loose(data_root, &sector_path).map(Cow::Owned))
                else {
                    self.warnings.push(format!(
                        "Authored AI navigation sector is missing: {sector_path}"
                    ));
                    continue;
                };
                match Sector::load(&sector_bytes) {
                    Ok(graph)
                        if graph.id == record.id
                            && sector_bytes.len() as u64 == record.payload_size as u64 + 16 =>
                    {
                        if let Err(error)=loaded.mesh.insert(&graph) {
                            self.warnings.push(format!("Could not construct native AI mesh {sector_path}: {error:#}"));
                        }
                        loaded.sectors.push(LoadedSector {
                            min: record.min,
                            max: record.max,
                            graph,
                        });
                    }
                    Ok(graph) => self.warnings.push(format!(
                        "AI sector identity/size mismatch for {sector_path}: expected id {} and {} bytes, found id {} and {} bytes",
                        record.id,
                        record.payload_size as u64 + 16,
                        graph.id,
                        sector_bytes.len()
                    )),
                    Err(error) => self.warnings.push(format!(
                        "Could not parse AI navigation sector {sector_path}: {error:#}"
                    )),
                }
            }
            if let Err(error)=loaded.mesh.bind_all() {
                self.warnings.push(format!("Could not bind native AI mesh neighbors for {path}: {error:#}"));
            }
            self.databases.push(loaded);
        }
    }

    /// Find a route's next authored node for a pair of world-space positions.
    /// Both endpoints must be near the same loaded sector; cross-sector routes
    /// intentionally fall through until the HPD handoff graph is implemented.
    pub fn waypoint(&mut self, source: [f32; 3], destination: [f32; 3]) -> Option<[f32; 3]> {
        if source
            .iter()
            .chain(destination.iter())
            .any(|v| !v.is_finite())
        {
            return None;
        }
        let mut best: Option<(f64, usize, usize, Vec<usize>)> = None;
        for (database_index, database) in self.databases.iter().enumerate() {
            for (sector_index, sector) in database.sectors.iter().enumerate() {
                // The serialized bounds cheaply reject unrelated sectors. A
                // small margin admits actors standing just outside a nav edge.
                let in_bounds = |point: [f32; 3]| {
                    (0..3).all(|axis| {
                        point[axis] >= sector.min[axis] - 4.0
                            && point[axis] <= sector.max[axis] + 4.0
                    })
                };
                if !in_bounds(source) || !in_bounds(destination) {
                    continue;
                }
                let nearest = |point: [f32; 3]| -> Option<(usize, f64)> {
                    sector
                        .graph
                        .nodes
                        .iter()
                        .enumerate()
                        .map(|(index, node)| {
                            let distance = (0..3)
                                .map(|axis| {
                                    let delta = point[axis] as f64 - node.position[axis] as f64;
                                    delta * delta
                                })
                                .sum::<f64>()
                                .sqrt();
                            (index, distance)
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                };
                let Some((start, start_distance)) = nearest(source) else {
                    continue;
                };
                let Some((goal, goal_distance)) = nearest(destination) else {
                    continue;
                };
                if start_distance > 8.0 || goal_distance > 8.0 {
                    continue;
                }
                let cache_key = (database_index, sector_index, start, goal);
                let route = if let Some(route) = self.route_cache.get(&cache_key) {
                    route.clone()
                } else {
                    let route = sector.graph.route(start, goal).ok().flatten();
                    self.route_cache.insert(cache_key, route.clone());
                    route
                };
                let Some(route) = route else {
                    continue;
                };
                let score = start_distance + goal_distance + route.len() as f64 * 0.01;
                if best.as_ref().is_none_or(|current| score < current.0) {
                    best = Some((score, database_index, sector_index, route));
                }
            }
        }
        let (_, database_index, sector_index, route) = best?;
        let graph = &self.databases[database_index].sectors[sector_index].graph;
        // Skip nodes already reached so an actor does not oscillate around its
        // nearest node. Returning the goal node still permits a final direct
        // approach on the next route refresh.
        route
            .iter()
            .skip(1)
            .map(|&index| graph.nodes[index].position)
            .find(|position| {
                let dx = position[0] - source[0];
                let dz = position[2] - source[2];
                dx * dx + dz * dz > 0.55 * 0.55
            })
            .or(Some(destination))
    }

    pub fn database_paths(&self) -> impl Iterator<Item = &str> {
        self.databases.iter().map(|database| database.path.as_str())
    }

    pub fn sector_count(&self) -> usize {
        self.databases
            .iter()
            .map(|database| database.sectors.len())
            .sum()
    }
    pub(super) fn native_floor(&mut self,position:[f32;3])->anyhow::Result<Option<(usize,super::ai_runtime_mesh::Floor)>> {
        for (index,database) in self.databases.iter_mut().enumerate() {
            if let Some(floor)=database.mesh.find_floor(position)? {return Ok(Some((index,floor)));}
        }
        Ok(None)
    }
    pub(super) fn native_floor_count(&self)->usize {
        self.databases.iter().map(|database|database.mesh.floor_count).sum()
    }
}

fn read_loose(root: &Path, asset_path: &str) -> Option<Vec<u8>> {
    let relative = Path::new(asset_path);
    // Game resources are relative virtual paths. Never let a malformed asset
    // name escape the imported files root when resolving loose sectors.
    if relative
        .components()
        .any(|component| !matches!(component, PathComponent::Normal(_)))
    {
        return None;
    }
    std::fs::read(root.join(relative)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use epic_mickey_lib_rs::packfile::{EndianDependentString, VirtualFile};
    use epic_mickey_lib_rs::scene_file::{Component, Property, ID};

    #[test]
    fn reads_only_referenced_database_paths_and_normalizes_separators() {
        let component = Component::new(
            "AIPathDatabaseComponent".into(),
            "AI Navigation".into(),
            ID::new(0),
            ID::new(1),
            ID::new(0),
            vec![Property::new(
                "String".into(),
                "PathData HPD Filename".into(),
                true,
                false,
                false,
                serde_json::Value::String("Environments\\ThePark\\GV\\gsa\\data.hpd".into()),
            )],
        );
        let entity = super::super::assets::RuntimeEntity {
            authored_scale: 1.0,
            authored_rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            runtime_rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            model_asset: None,
            mesh_draws: HashMap::new(),
            jigsaw_asset: None,
            jigsaw_states: Vec::new(),
            scene: "fixture.bin".into(),
            id: 1,
            name: "Navigation".into(),
            transform: [0.; 16],
            authored_transform: [0.; 16],
            components: vec![component],
            draws: 0..0,
            collision_parts: 0..0,
        };
        let unused = VirtualFile::new(
            EndianDependentString::new(" OBJ".into()),
            false,
            0,
            "unreferenced/data.hpd".into(),
            Vec::new(),
        );
        let runtime = Runtime::load(
            &[entity],
            &[unused],
            Path::new("this-folder-does-not-exist"),
        );
        assert!(runtime.databases.is_empty());
        assert_eq!(runtime.warnings.len(), 1);
        assert!(runtime.warnings[0].contains("environments/thepark/gv/gsa/data.hpd"));
    }

    #[test]
    fn loose_sector_resolution_stays_inside_imported_files_root() -> std::io::Result<()> {
        let root =
            std::env::temp_dir().join(format!("blotworx-ai-navigation-{}", std::process::id()));
        let sector_dir = root.join("sectors/test_database");
        std::fs::create_dir_all(&sector_dir)?;
        let sector = sector_dir.join("sector_1.prelnk.obj");
        std::fs::write(&sector, b"authored-sector")?;
        assert_eq!(
            read_loose(&root, "sectors/test_database/sector_1.prelnk.obj").as_deref(),
            Some(b"authored-sector".as_slice())
        );
        assert!(read_loose(&root, "../outside.prelnk.obj").is_none());
        std::fs::remove_dir_all(root)
    }

    #[test]
    #[ignore = "Requires imported Epic Mickey 1 data in BLOTWORX_TEST_DATA"]
    fn gv_zone_a_loads_retail_navigation_and_routes_between_authored_nodes() -> anyhow::Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA")
            .context("Set BLOTWORX_TEST_DATA to the imported DATA/files directory")?;
        let mut world = super::super::assets::load(
            Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let duplicate_bytes = world
            .draws
            .iter()
            .map(|draw| draw.v_data.len() * std::mem::size_of::<f32>())
            .sum::<usize>();
        assert_eq!(world.inspection_vertex_buffers.len(), 0);
        eprintln!(
            "inspection-only mesh copies deferred: {duplicate_bytes} bytes across {} draws",
            world.draws.len()
        );
        let draw = world
            .draws
            .iter()
            .position(|draw| draw.v_data.len() >= 12 && draw.v_data.len() % 12 == 0)
            .context("GV Zone A has no regular mesh to test inspection rendering")?;
        let source = world.draws[draw].v_data.clone();
        let inspection = world
            .inspection_vertices(draw)
            .context("Inspection buffer was not created on demand")?;
        assert!(!std::sync::Arc::ptr_eq(&source, &inspection));
        assert!(std::sync::Arc::ptr_eq(
            &inspection,
            &world
                .inspection_vertices(draw)
                .context("Inspection buffer was not cached")?
        ));
        for (source, inspection) in source.chunks_exact(12).zip(inspection.chunks_exact(12)) {
            assert_eq!(&source[..8], &inspection[..8]);
            assert_eq!(&inspection[8..11], &[1.0, 1.0, 1.0]);
            assert_eq!(source[11], inspection[11]);
        }
        if let Some(shared_draw) = world
            .draws
            .iter()
            .enumerate()
            .find(|(index, candidate)| {
                *index != draw && std::sync::Arc::ptr_eq(&source, &candidate.v_data)
            })
            .map(|(index, _)| index)
        {
            let shared = world
                .inspection_vertices(shared_draw)
                .context("Shared inspection buffer was not created")?;
            assert!(std::sync::Arc::ptr_eq(&inspection, &shared));
        }
        assert_eq!(world.ai_navigation.database_paths().count(), 2);
        assert_eq!(
            world.ai_navigation.sector_count(),
            12,
            "{:?}",
            world.ai_navigation.warnings
        );
        assert!(
            world.ai_navigation.warnings.is_empty(),
            "{:?}",
            world.ai_navigation.warnings
        );
        assert!(world.ai_navigation.native_floor_count()>0,"Live World did not construct native runtime floors");
        let mut navigation = std::mem::take(&mut world.ai_navigation);
        // The old waypoint example below uses an approximate Y coordinate;
        // it is not an original-reference floor-search hit. Use the exact
        // authored node coordinates verified by the original mesh loader/search.
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_runtime_mesh.json"))?;
        let sample=fixture["production"][0]["queries"].as_array().unwrap().iter()
            .find(|q|q["result"].as_u64().unwrap()!=0).context("Original floor-search hit missing")?;
        let point=std::array::from_fn(|i|f32::from_bits(sample["point"][i].as_u64().unwrap() as u32));
        assert!(navigation.native_floor(point)?.is_some(),"Original-reference floor hit missing from live World");
        let waypoint = navigation
            .waypoint([20.767, 0.0, -1.205], [6.032, 0.036, 7.027])
            .context("GV Zone A path graph returned no same-sector waypoint")?;
        assert!(waypoint.iter().all(|coordinate| coordinate.is_finite()));
        assert_ne!(waypoint, [6.032, 0.036, 7.027]);
        Ok(())
    }
}
