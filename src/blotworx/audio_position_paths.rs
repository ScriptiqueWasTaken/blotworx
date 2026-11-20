//! Path-array replacement80354670..8035483c. Allocation/free/copy and arena
//! writes are explicit services; malformed native out-of-bounds domains error.
use super::{
    audio_bank::positioning::{Path, Vertex},
    audio_position_state::{Array, State},
};
use anyhow::{ensure, Context, Result};
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub first: u32,
    pub count: u32,
    pub range: [f32; 2],
}
pub trait Control {
    fn free_vertices(&mut self, handle: u32) -> Result<()>;
    fn free_paths(&mut self, handle: u32) -> Result<()>;
    fn allocate_vertices(&mut self, count: u32) -> Result<Option<u32>>;
    fn copy_vertices(&mut self, handle: u32, vertices: &[Vertex]) -> Result<()>;
    fn allocate_paths(&mut self, count: u32) -> Result<Option<u32>>;
    fn write_path(&mut self, handle: u32, index: u32, entry: Entry) -> Result<()>;
    fn set_duration(&mut self, vertices: u32, index: u32, duration: i32) -> Result<()>;
}
pub fn clear_paths(state: &mut State, host: &mut impl Control) -> Result<()> {
    let mut first = None;
    if let Some(handle) = state.vertices.handle.take() {
        if let Err(error) = host.free_vertices(handle) {
            first = Some(error);
        }
    }
    if let Some(handle) = state.paths.handle.take() {
        if let Err(error) = host.free_paths(handle) {
            if first.is_none() {
                first = Some(error);
            }
        }
    }
    state.vertices.count = 0;
    state.paths.count = 0;
    if let Some(error) = first {
        return Err(error);
    }
    Ok(())
}
pub fn set_paths(
    state: &mut State,
    vertices: Option<&[Vertex]>,
    paths: Option<&[Path]>,
    host: &mut impl Control,
) -> Result<u32> {
    clear_paths(state, host)?;
    let (Some(vertices), Some(paths)) = (vertices, paths) else {
        return Ok(31);
    };
    if vertices.is_empty() || paths.is_empty() {
        return Ok(31);
    }
    let vertex_count =
        u32::try_from(vertices.len()).context("Position vertices exceed native count")?;
    let path_count = u32::try_from(paths.len()).context("Position paths exceed native count")?;
    vertex_count
        .checked_mul(16)
        .context("Native position vertex allocation would overflow")?;
    path_count
        .checked_mul(16)
        .context("Native position path allocation would overflow")?;
    let Some(vertex_handle) = host.allocate_vertices(vertex_count)? else {
        return Ok(52);
    };
    state.vertices.handle = Some(vertex_handle);
    host.copy_vertices(vertex_handle, vertices)?;
    state.vertices.count = vertex_count;
    let Some(path_handle) = host.allocate_paths(path_count)? else {
        return Ok(52);
    };
    state.paths = Array {
        handle: Some(path_handle),
        count: path_count,
    };
    for (index, path) in paths.iter().enumerate() {
        ensure!(
            path.first < vertex_count,
            "Native path setup would return2 with partially uninitialized path entries"
        );
        host.write_path(
            path_handle,
            index as u32,
            Entry {
                first: path.first,
                count: path.count,
                range: [0.; 2],
            },
        )?;
    }
    for path in paths {
        // Native checks the count as signed before writing its final duration.
        if path.count as i32 > 0 {
            let last = path
                .first
                .checked_add(path.count - 1)
                .context("Native path end would overflow")?;
            ensure!(
                last < vertex_count,
                "Native path end would write outside the vertex array"
            );
            host.set_duration(vertex_handle, last, state.transition)?;
        }
    }
    Ok(1)
}

#[derive(Default)]
pub struct Arrays {
    next: u32,
    vertices: std::collections::BTreeMap<u32, Vec<Vertex>>,
    paths: std::collections::BTreeMap<u32, Vec<Option<Entry>>>,
}
impl Arrays {
    fn handle(&mut self) -> Result<u32> {
        self.next = self
            .next
            .checked_add(1)
            .context("Position array handle space exhausted")?;
        Ok(self.next)
    }
    pub fn vertices(&self, handle: u32) -> Result<&[Vertex]> {
        Ok(self
            .vertices
            .get(&handle)
            .context("Unknown positioning vertex array")?)
    }
    pub fn paths(&self, handle: u32) -> Result<&[Option<Entry>]> {
        Ok(self
            .paths
            .get(&handle)
            .context("Unknown positioning path array")?)
    }
    pub fn set_range(&mut self, handle: u32, index: usize, range: [f32; 2]) -> Result<()> {
        self.paths
            .get_mut(&handle)
            .context("Unknown positioning path array")?
            .get_mut(index)
            .context("Position path index out of range")?
            .as_mut()
            .context("Position path is uninitialized")?
            .range = range;
        Ok(())
    }
    pub fn live_arrays(&self) -> usize {
        self.vertices.len() + self.paths.len()
    }
}
impl Control for Arrays {
    fn free_vertices(&mut self, handle: u32) -> Result<()> {
        self.vertices
            .remove(&handle)
            .context("Unknown positioning vertex array")?;
        Ok(())
    }
    fn free_paths(&mut self, handle: u32) -> Result<()> {
        self.paths
            .remove(&handle)
            .context("Unknown positioning path array")?;
        Ok(())
    }
    fn allocate_vertices(&mut self, count: u32) -> Result<Option<u32>> {
        let mut vertices = Vec::new();
        if vertices.try_reserve_exact(count as usize).is_err() {
            return Ok(None);
        }
        let handle = self.handle()?;
        self.vertices.insert(handle, vertices);
        Ok(Some(handle))
    }
    fn copy_vertices(&mut self, handle: u32, vertices: &[Vertex]) -> Result<()> {
        let target = self
            .vertices
            .get_mut(&handle)
            .context("Unknown positioning vertex array")?;
        target.clear();
        target.extend_from_slice(vertices);
        Ok(())
    }
    fn allocate_paths(&mut self, count: u32) -> Result<Option<u32>> {
        let mut paths = Vec::new();
        if paths.try_reserve_exact(count as usize).is_err() {
            return Ok(None);
        }
        paths.resize(count as usize, None);
        let handle = self.handle()?;
        self.paths.insert(handle, paths);
        Ok(Some(handle))
    }
    fn write_path(&mut self, handle: u32, index: u32, entry: Entry) -> Result<()> {
        *self
            .paths
            .get_mut(&handle)
            .context("Unknown positioning path array")?
            .get_mut(index as usize)
            .context("Position path index out of range")? = Some(entry);
        Ok(())
    }
    fn set_duration(&mut self, handle: u32, index: u32, duration: i32) -> Result<()> {
        self.vertices
            .get_mut(&handle)
            .context("Unknown positioning vertex array")?
            .get_mut(index as usize)
            .context("Position vertex index out of range")?
            .duration = duration;
        Ok(())
    }
}
pub struct Provision {
    pub state: State,
    pub status: u32,
}
/// Build a new bank-local 3D definition. The native loader assigns the node ID
/// to position+4, provisions user paths, then applies their authored ranges.
pub fn provision(
    node: u32,
    position: &super::audio_bank::positioning::Positioning,
    storage: super::audio_position_state::Storage,
    arrays: &mut Arrays,
) -> Result<Option<Provision>> {
    use super::audio_bank::positioning::{Mode, Positioning, SpatialSource};
    let Positioning::Local {
        mode:
            Mode::ThreeD {
                kind,
                attenuation,
                spatialized,
                source,
            },
        ..
    } = position
    else {
        return Ok(None);
    };
    let mut state = State::new(storage);
    state.path_id = node;
    state.kind = *kind;
    state.attenuation_id = *attenuation;
    state.spatialized = u8::from(*spatialized);
    let result: Result<u32> = (|| match source {
        SpatialSource::Game { dynamic } => {
            state.flags[1] = u8::from(*dynamic);
            Ok(1)
        }
        SpatialSource::User {
            mode,
            looping,
            transition,
            follow,
            vertices,
            paths,
        } => {
            state.path_mode = *mode;
            state.flags[3] = u8::from(*looping);
            state.transition = *transition;
            state.flags[2] = u8::from(*follow);
            let status = set_paths(&mut state, Some(vertices), Some(paths), arrays)?;
            if !paths.is_empty() {
                let handle = state
                    .paths
                    .handle
                    .context("Native range update would target an unavailable path array")?;
                for (index, path) in paths.iter().enumerate() {
                    arrays.set_range(handle, index, path.range)?;
                }
            }
            Ok(status)
        }
        SpatialSource::Other => Ok(1),
    })();
    match result {
        Ok(status) => Ok(Some(Provision { state, status })),
        Err(error) => {
            let released = clear_paths(&mut state, arrays);
            match released {
                Ok(()) => Err(error),
                Err(cleanup) => {
                    Err(error.context(format!("Position cleanup also failed: {cleanup:#}")))
                }
            }
        }
    }
}
impl super::audio_bank::Bank<'_> {
    pub fn provision_voice_position(
        &self,
        node: u32,
        storage: super::audio_position_state::Storage,
        arrays: &mut Arrays,
    ) -> Result<Option<Provision>> {
        provision(node, &self.parameter(node)?.position, storage, arrays)
    }
}

#[cfg(test)]
mod tests {
    use super::super::audio_position_state::Storage;
    use super::*;
    use serde_json::{json, Value};
    struct Host {
        arrays: Arrays,
        calls: Vec<Value>,
        fail: i64,
        allocation: i64,
    }
    impl Control for Host {
        fn free_vertices(&mut self, h: u32) -> Result<()> {
            self.calls.push(json!({"op":"free_vertices"}));
            self.arrays.free_vertices(h)
        }
        fn free_paths(&mut self, h: u32) -> Result<()> {
            self.calls.push(json!({"op":"free_paths"}));
            self.arrays.free_paths(h)
        }
        fn allocate_vertices(&mut self, count: u32) -> Result<Option<u32>> {
            self.calls
                .push(json!({"op":"allocate_vertices","count":count}));
            let fail = self.allocation == self.fail;
            self.allocation += 1;
            if fail {
                Ok(None)
            } else {
                self.arrays.allocate_vertices(count)
            }
        }
        fn copy_vertices(&mut self, h: u32, v: &[Vertex]) -> Result<()> {
            self.calls.push(json!({"op":"copy_vertices"}));
            self.arrays.copy_vertices(h, v)
        }
        fn allocate_paths(&mut self, count: u32) -> Result<Option<u32>> {
            self.calls
                .push(json!({"op":"allocate_paths","count":count}));
            let fail = self.allocation == self.fail;
            self.allocation += 1;
            if fail {
                Ok(None)
            } else {
                self.arrays.allocate_paths(count)
            }
        }
        fn write_path(&mut self, h: u32, i: u32, p: Entry) -> Result<()> {
            self.arrays.write_path(h, i, p)
        }
        fn set_duration(&mut self, h: u32, i: u32, d: i32) -> Result<()> {
            self.arrays.set_duration(h, i, d)
        }
    }
    fn storage() -> Storage {
        Storage {
            byte11: 0,
            scalar_bits: [0; 4],
        }
    }
    #[test]
    fn original_path_replacement_statuses_partial_allocations_and_duration_bits() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_position_paths.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let vertices: Vec<Vertex> = serde_json::from_value(case["vertices"].clone())?;
            let paths: Vec<Path> = serde_json::from_value(case["paths"].clone())?;
            let mut state = State::new(storage());
            state.transition = -150;
            let mut host = Host {
                arrays: Arrays::default(),
                calls: Vec::new(),
                fail: case["fail"].as_i64().unwrap(),
                allocation: 0,
            };
            if case["old"] == true {
                state.vertices = Array {
                    handle: host.arrays.allocate_vertices(3)?,
                    count: 3,
                };
                state.paths = Array {
                    handle: host.arrays.allocate_paths(2)?,
                    count: 2,
                };
            }
            let status = set_paths(
                &mut state,
                if case["vertex_present"] == true {
                    Some(&vertices)
                } else {
                    None
                },
                if case["path_present"] == true {
                    Some(&paths)
                } else {
                    None
                },
                &mut host,
            )?;
            assert_eq!(
                status as u64,
                case["status"].as_u64().unwrap(),
                "case {index}"
            );
            assert_eq!(json!(host.calls), case["calls"], "case {index}");
            assert_eq!(
                json!(state.vertices.handle.is_some()),
                case["vertex_present_out"]
            );
            assert_eq!(
                json!(state.paths.handle.is_some()),
                case["path_present_out"]
            );
            assert_eq!(json!(state.vertices.count), case["vertex_count"]);
            assert_eq!(json!(state.paths.count), case["path_count"]);
            let vertex_bits = if let Some(h) = state.vertices.handle {
                host.arrays
                    .vertices(h)?
                    .iter()
                    .map(|v| {
                        [
                            v.position[0].to_bits(),
                            v.position[1].to_bits(),
                            v.position[2].to_bits(),
                            v.duration as u32,
                        ]
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            assert_eq!(json!(vertex_bits), case["vertex_bits"], "case {index}");
            let entries = if let Some(h) = state.paths.handle {
                host.arrays.paths(h)?.iter().map(|p|{let p=p.as_ref().unwrap();json!({"first":p.first,"count":p.count,"range_bits":p.range.map(f32::to_bits)})}).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            assert_eq!(json!(entries), case["output_paths"], "case {index}");
            clear_paths(&mut state, &mut host.arrays)?;
            assert_eq!(host.arrays.live_arrays(), 0);
        }
        Ok(())
    }
    #[test]
    fn authored_user_definition_keeps_ranges_and_shared_arrays_and_rejects_unsafe_indices(
    ) -> Result<()> {
        use super::super::audio_bank::positioning::{Mode, Positioning, SpatialSource};
        let mut arrays = Arrays::default();
        let mut authored = Positioning::Local {
            center: 0,
            pan: [0.; 2],
            mode: Mode::ThreeD {
                kind: 2,
                attenuation: 55,
                spatialized: true,
                source: SpatialSource::User {
                    mode: 3,
                    looping: true,
                    transition: -150,
                    follow: true,
                    vertices: (0..4)
                        .map(|i| Vertex {
                            position: [i as f32, 0., 1.],
                            duration: 250 + i,
                        })
                        .collect(),
                    paths: vec![
                        Path {
                            first: 0,
                            count: 2,
                            range: [0.25, 0.75],
                        },
                        Path {
                            first: 2,
                            count: 2,
                            range: [1.25, 1.75],
                        },
                    ],
                },
            },
        };
        let mut provision = provision(17, &authored, storage(), &mut arrays)?.unwrap();
        assert_eq!(provision.status, 1);
        assert_eq!(provision.state.path_id, 17);
        assert_eq!(
            arrays
                .vertices(provision.state.vertices.handle.unwrap())?
                .iter()
                .map(|v| v.duration)
                .collect::<Vec<_>>(),
            [250, -150, 252, -150]
        );
        let paths = arrays.paths(provision.state.paths.handle.unwrap())?;
        assert_eq!(paths[0].as_ref().unwrap().range, [0.25, 0.75]);
        assert_eq!(paths[1].as_ref().unwrap().range, [1.25, 1.75]);
        let mut copy = State::new(storage());
        copy.copy_definition(&provision.state);
        assert_eq!(copy.vertices, provision.state.vertices);
        assert_eq!(copy.paths, provision.state.paths);
        assert_eq!(arrays.live_arrays(), 2);
        clear_paths(&mut provision.state, &mut arrays)?;
        assert_eq!(arrays.live_arrays(), 0);
        if let Positioning::Local {
            mode:
                Mode::ThreeD {
                    source: SpatialSource::User { paths, .. },
                    ..
                },
            ..
        } = &mut authored
        {
            paths[0].first = 99;
        }
        assert!(super::provision(17, &authored, storage(), &mut arrays).is_err());
        assert_eq!(arrays.live_arrays(), 0);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_bank_position_definitions_provision_and_release_arrays() -> Result<()> {
        use super::super::audio_bank::Bank;
        fn collect(dir: &std::path::Path, paths: &mut Vec<std::path::PathBuf>) -> Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    collect(&path, paths)?;
                } else if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("bnk"))
                {
                    paths.push(path);
                }
            }
            Ok(())
        }
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut paths = Vec::new();
        collect(&root.join("Audio/Wii"), &mut paths)?;
        paths.sort();
        ensure!(!paths.is_empty(), "No owned banks");
        let mut counts = std::collections::BTreeMap::<String, u64>::new();
        let mut arrays = Arrays::default();
        for path in &paths {
            let bytes = std::fs::read(path)?;
            let bank = Bank::parse(&bytes)?;
            for (&id, object) in &bank.objects {
                if !matches!(object.kind, 2 | 5 | 7) {
                    continue;
                }
                *counts.entry("queried_nodes".into()).or_default() += 1;
                let provision = bank
                    .provision_voice_position(id, storage(), &mut arrays)
                    .with_context(|| format!("Provision {} node {id:08x}", path.display()))?;
                if let Some(mut provision) = provision {
                    *counts
                        .entry(format!("kind_{}", provision.state.kind))
                        .or_default() += 1;
                    *counts
                        .entry(format!("status_{}", provision.status))
                        .or_default() += 1;
                    assert_eq!(provision.state.path_id, id);
                    *counts.entry("vertices".into()).or_default() +=
                        provision.state.vertices.count as u64;
                    *counts.entry("paths".into()).or_default() +=
                        provision.state.paths.count as u64;
                    clear_paths(&mut provision.state, &mut arrays)?;
                } else {
                    *counts.entry("non_3d_nodes".into()).or_default() += 1;
                }
                assert_eq!(arrays.live_arrays(), 0);
            }
        }
        std::fs::create_dir_all("target")?;
        std::fs::write(
            "target/audio-position-provision-corpus.json",
            serde_json::to_vec_pretty(
                &json!({"scope":"Bank-local positioning provisioning, not loaded-world or audio playback parity","banks":paths.iter().map(|p|p.strip_prefix(&root).unwrap().to_string_lossy()).collect::<Vec<_>>(),"counts":counts}),
            )?,
        )?;
        Ok(())
    }
}
