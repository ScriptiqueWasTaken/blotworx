//! EM1 Kynogon HPD v2 and prelinked navigation graphs, little endian on Wii.
//! Runtime mesh and link metadata are retained verbatim, not regenerated.
use anyhow::{ensure, Context, Result};
use std::sync::Arc;
fn u32_at(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        data.get(offset..offset.checked_add(4).context("AI offset overflow")?)
            .context("Truncated AI database")?
            .try_into()?,
    ))
}
fn f32_at(data: &[u8], offset: usize) -> Result<f32> {
    let v = f32::from_bits(u32_at(data, offset)?);
    ensure!(v.is_finite(), "Non-finite navigation coordinate");
    Ok(v)
}
#[derive(Clone, Debug)]
pub struct SectorRecord {
    pub id: u32,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub payload_size: u32,
    pub payload_offset: u32,
}
#[derive(Clone, Debug)]
pub struct Database {
    // Immutable source shared with editor undo snapshots; saves write a new buffer.
    bytes: Arc<Vec<u8>>,
    pub sectors: Vec<SectorRecord>,
}
impl Database {
    pub fn load(data: &[u8]) -> Result<Self> {
        ensure!(
            data.get(..12) == Some(b"Kynogon HPD\0"),
            "Not a Kynogon HPD database"
        );
        ensure!(
            u32_at(data, 12)? == 2 && u32_at(data, 16)? == 2,
            "Unsupported Kynogon HPD version"
        );
        let n = u32_at(data, 20)? as usize;
        ensure!(
            n <= 100_000 && data.len() == 156 + 48 * n,
            "Invalid HPD sector count"
        );
        let mut sectors = Vec::with_capacity(n);
        let mut ids = std::collections::BTreeSet::new();
        for i in 0..n {
            let p = 156 + i * 48;
            let id = u32_at(data, p)?;
            ensure!(ids.insert(id), "Duplicate AI sector {id}");
            let mut min = [0.0; 3];
            let mut max = [0.0; 3];
            for k in 0..3 {
                min[k] = f32_at(data, p + 4 + k * 8)?;
                max[k] = f32_at(data, p + 8 + k * 8)?;
                ensure!(min[k] <= max[k], "Inverted navigation bounds");
            }
            sectors.push(SectorRecord {
                id,
                min,
                max,
                payload_size: u32_at(data, p + 32)?,
                payload_offset: u32_at(data, p + 36)?,
            });
        }
        Ok(Self {
            bytes: Arc::new(data.to_vec()),
            sectors,
        })
    }
    pub fn save(&self) -> Result<Vec<u8>> {
        ensure!(
            self.sectors.len() == u32_at(&self.bytes, 20)? as usize,
            "Adding or removing sectors requires rebuilding the database"
        );
        for (i, s) in self.sectors.iter().enumerate() {
            let p = 156 + i * 48;
            ensure!(
                s.id == u32_at(&self.bytes, p)?
                    && s.payload_size == u32_at(&self.bytes, p + 32)?
                    && s.payload_offset == u32_at(&self.bytes, p + 36)?,
                "Changing sector identifiers or payloads requires rebuilding the database"
            );
        }
        let mut bytes = (*self.bytes).clone();
        for (i, s) in self.sectors.iter().enumerate() {
            for k in 0..3 {
                ensure!(
                    s.min[k].is_finite() && s.max[k].is_finite() && s.min[k] <= s.max[k],
                    "Sector {} has invalid bounds",
                    s.id
                );
                let p = 156 + i * 48 + 4 + k * 8;
                bytes[p..p + 4].copy_from_slice(&s.min[k].to_le_bytes());
                bytes[p + 4..p + 8].copy_from_slice(&s.max[k].to_le_bytes());
            }
        }
        Ok(bytes)
    }
    pub fn sector_path(path: &str, id: u32) -> Option<String> {
        let path = path.replace('\\', "/");
        let folder = path.rsplit_once('/')?.0.rsplit('/').next()?;
        Some(format!("sectors/{folder}/sector_{id}.prelnk.obj").to_lowercase())
    }
}
#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub position: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct Edge {
    /// High bit retained; lower 31 bits encode floor(world-space distance * 16).
    pub flags: u32,
    pub from: usize,
    pub to: usize,
}
/// Recovered runtime geometry; linking and traversal policy remain incomplete.
#[derive(Clone, Debug)]
pub struct RuntimeSegment {
    pub start: [f32; 3],
    pub end: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct RuntimeFloor {
    pub cell: [u32; 2],
    pub layer: u32,
    pub min_height: f32,
    pub max_height: f32,
    pub segments: Vec<RuntimeSegment>,
    pub portals: Vec<RuntimePortal>,
    pub polylines: Vec<Vec<[f32; 3]>>,
    pub patches: Vec<RuntimePatch>,
    pub unbound_links: Vec<RuntimeUnboundLink>,
}
#[derive(Clone, Debug)]
pub struct RuntimePatch {
    pub points: Vec<[f32; 3]>,
    pub direction: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct RuntimeUnboundLink {
    /// Byte offset relative to the runtime mesh, used by the neighbor table.
    pub offset: usize,
    pub segment: RuntimeSegment,
    pub link_words: [u32; 2],
    pub direction: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct RuntimeSectorConnection {
    pub source_floor: usize,
    pub source_link: usize,
    pub target_sector: u32,
    pub target_cell: [u32; 2],
    pub target_layer: u32,
}
#[derive(Clone, Debug)]
pub struct RuntimePortal {
    pub segment: RuntimeSegment,
    /// Index in the decoded sector's runtime floor list, never a graph node ID.
    pub target_floor: usize,
}
impl RuntimeFloor {
    /// Geometric outlines only; no connection or walkability inference.
    pub fn outlines(&self) -> impl Iterator<Item = RuntimeSegment> + '_ {
        let paths = self
            .polylines
            .iter()
            .map(Vec::as_slice)
            .chain(self.patches.iter().map(|p| p.points.as_slice()));
        self.segments
            .iter()
            .cloned()
            .chain(paths.flat_map(|points| {
                points.windows(2).map(|pair| RuntimeSegment {
                    start: pair[0],
                    end: pair[1],
                })
            }))
            .chain(self.unbound_links.iter().map(|link| link.segment.clone()))
    }
}
#[derive(Clone, Debug)]
pub struct Sector {
    // Runtime geometry dominates file size and is never edited by node tools.
    // Keep one immutable copy across snapshots instead of duplicating it per edit.
    bytes: Arc<Vec<u8>>,
    pub id: u32,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub mesh_offset: usize,
}
impl Sector {
    /// Immutable serialized mesh payload. Runtime relocation owns a separate
    /// writable arena; editor serialization continues using these source bytes.
    pub(crate) fn runtime_mesh_bytes(&self) -> &[u8] {
        &self.bytes[self.mesh_offset..]
    }
    fn edge_cost(&self, edge: &Edge) -> Result<u32> {
        let a = self.nodes.get(edge.from).context("Edge source is absent")?;
        let b = self
            .nodes
            .get(edge.to)
            .context("Edge destination is absent")?;
        let length = (0..3)
            .map(|axis| {
                let delta = a.position[axis] as f64 - b.position[axis] as f64;
                delta * delta
            })
            .sum::<f64>()
            .sqrt();
        let cost = (length * 16.).floor();
        ensure!(
            cost.is_finite() && cost <= 0x7fff_ffffu32 as f64,
            "Moved node exceeds representable edge distance"
        );
        Ok((edge.flags & 0x8000_0000) | cost as u32)
    }
    pub fn graph_metadata_issues(&self) -> Result<Vec<String>> {
        let mut issues = Vec::new();
        match self.spatial_orders() {
            Ok(orders) => {
                for axis in 0..3 {
                    if !orders[axis]
                        .windows(2)
                        .all(|p| self.nodes[p[0]].position[axis] <= self.nodes[p[1]].position[axis])
                    {
                        issues.push(format!(
                            "{} spatial index is not sorted",
                            ["X", "Y", "Z"][axis]
                        ));
                    }
                }
            }
            Err(e) => issues.push(e.to_string()),
        }
        let mut costs = 0;
        for edge in &self.edges {
            if self.edge_cost(edge)? != edge.flags {
                costs += 1;
            }
        }
        if costs != 0 {
            issues.push(format!("{costs} edge distances differ from node positions"));
        }
        Ok(issues)
    }
    /// Three permutations indexing nodes in coordinate order. The final u16 in
    /// each serialized row is unrelated metadata and must remain untouched.
    pub fn spatial_orders(&self) -> Result<[Vec<usize>; 3]> {
        ensure!(
            self.nodes.len() <= u16::MAX as usize + 1,
            "Too many nodes for spatial indexes"
        );
        let mut orders: [Vec<usize>; 3] =
            std::array::from_fn(|_| Vec::with_capacity(self.nodes.len()));
        for axis in 0..3 {
            let mut seen = vec![false; self.nodes.len()];
            for row in 0..self.nodes.len() {
                let p = 24 + row * 36 + 28 + axis * 2;
                let index = u16::from_le_bytes(
                    self.bytes
                        .get(p..p + 2)
                        .context("Truncated spatial index")?
                        .try_into()?,
                ) as usize;
                ensure!(
                    index < self.nodes.len() && !seen[index],
                    "Invalid spatial index permutation"
                );
                seen[index] = true;
                orders[axis].push(index);
            }
        }
        Ok(orders)
    }
    /// Resolve serialized neighbor-table entries to their local source records.
    /// Destination identities require the referenced sector to be loaded separately.
    pub fn runtime_sector_connections(&self) -> Result<Vec<RuntimeSectorConnection>> {
        let floors = self.runtime_floors()?;
        let data = &self.bytes[self.mesh_offset..];
        let count = u32_at(data, 72)? as usize;
        if count == 0 {
            return Ok(Vec::new());
        }
        let table = u32_at(data, 76)? as usize;
        let end = data
            .len()
            .checked_sub(8)
            .context("Missing runtime footer")?;
        ensure!(
            table >= 80 && table <= end && count <= (end - table) / 12,
            "Invalid runtime neighbor table"
        );
        let metadata_start = table + count * 12;
        let sources: std::collections::HashMap<_, _> = floors
            .iter()
            .enumerate()
            .flat_map(|(f, floor)| {
                floor
                    .unbound_links
                    .iter()
                    .enumerate()
                    .map(move |(i, link)| (link.offset, (f, i)))
            })
            .collect();
        let mut connections = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for row in 0..count {
            let p = table + row * 12;
            let target_sector = u32_at(data, p)?;
            let groups = u32_at(data, p + 4)? as usize;
            let offsets = u32_at(data, p + 8)? as usize;
            ensure!(
                offsets >= metadata_start && offsets <= end && groups <= (end - offsets) / 4,
                "Invalid runtime neighbor groups"
            );
            for group in 0..groups {
                let g = u32_at(data, offsets + group * 4)? as usize;
                ensure!(
                    g >= metadata_start && g <= end && end - g >= 16,
                    "Invalid runtime neighbor group"
                );
                let target_cell = [u32_at(data, g)?, u32_at(data, g + 4)?];
                let target_layer = u32_at(data, g + 8)?;
                let links = u32_at(data, g + 12)? as usize;
                ensure!(
                    links <= (end - g - 16) / 4,
                    "Invalid runtime neighbor link count"
                );
                for link in 0..links {
                    let offset = u32_at(data, g + 16 + link * 4)? as usize;
                    let &(source_floor, source_link) = sources
                        .get(&offset)
                        .context("Neighbor references an absent local link")?;
                    ensure!(
                        seen.insert(offset),
                        "Duplicate runtime neighbor link assignment"
                    );
                    connections.push(RuntimeSectorConnection {
                        source_floor,
                        source_link,
                        target_sector,
                        target_cell,
                        target_layer,
                    });
                }
            }
        }
        Ok(connections)
    }
    /// Decode the observed v2 cell/floor segment arrays without modifying the
    /// runtime mesh, links, or graph. All pointers are checked in their owner.
    pub fn runtime_floors(&self) -> Result<Vec<RuntimeFloor>> {
        let data = self
            .bytes
            .get(self.mesh_offset..)
            .context("Missing runtime mesh")?;
        ensure!(
            u32_at(data, 28)? == self.id,
            "Runtime mesh sector ID mismatch"
        );
        let nx = u32_at(data, 56)? as usize;
        let nz = u32_at(data, 60)? as usize;
        let count = nx.checked_mul(nz).context("Runtime cell count overflow")?;
        ensure!(
            nx > 0 && nz > 0 && count <= data.len() / 4,
            "Invalid runtime grid dimensions"
        );
        let table = u32_at(data, 68)? as usize;
        ensure!(table == 80, "Unsupported runtime cell table");
        let table_end = table
            .checked_add((count + 1) * 4)
            .context("Runtime table overflow")?;
        ensure!(table_end <= data.len(), "Truncated runtime cell table");
        let mut floors = Vec::new();
        let mut floor_ranges = Vec::new();
        for cell in 0..count {
            let start = u32_at(data, table + cell * 4)? as usize;
            let end = u32_at(data, table + (cell + 1) * 4)? as usize;
            ensure!(
                start >= table_end && end >= start + 4 && end <= data.len(),
                "Invalid runtime cell range"
            );
            let n = u32_at(data, start)? as usize;
            if n == 0 {
                ensure!(end == start + 4, "Unexpected empty runtime cell data");
                continue;
            }
            ensure!(n <= (end - start - 4) / 12, "Invalid runtime floor count");
            let header_end = start + 8 + 12 * n;
            ensure!(header_end <= end, "Truncated runtime floor table");
            for index in 0..n {
                let row = start + 4 + index * 12;
                let base = u32_at(data, row)? as usize;
                let next = u32_at(data, row + 12)? as usize;
                ensure!(
                    base >= header_end && next >= base + 80 && next <= end,
                    "Invalid runtime floor range"
                );
                ensure!(
                    u32_at(data, base)? == self.id,
                    "Runtime floor sector ID mismatch"
                );
                let cell_id = [(cell / nz) as u32, (cell % nz) as u32];
                ensure!(
                    [u32_at(data, base + 4)?, u32_at(data, base + 8)?] == cell_id
                        && u32_at(data, base + 12)? == index as u32,
                    "Runtime floor identity disagrees with cell table"
                );
                let min_height = f32_at(data, row + 4)?;
                let max_height = f32_at(data, row + 8)?;
                ensure!(min_height <= max_height, "Inverted runtime floor heights");
                let n_segments = u32_at(data, base + 64)? as usize;
                let mut segments = Vec::new();
                if n_segments != 0 {
                    let offset = u32_at(data, base + 68)? as usize;
                    ensure!(
                        offset >= 80
                            && offset <= next - base
                            && n_segments < (next - base - offset) / 4,
                        "Invalid runtime segment table"
                    );
                    let points_start = offset + (n_segments + 1) * 4;
                    for segment in 0..n_segments {
                        let a = u32_at(data, base + offset + segment * 4)? as usize;
                        let b = u32_at(data, base + offset + (segment + 1) * 4)? as usize;
                        ensure!(
                            a >= points_start && b == a + 24 && b <= next - base,
                            "Unsupported runtime segment record"
                        );
                        let point = |p| -> Result<[f32; 3]> {
                            Ok([f32_at(data, p)?, f32_at(data, p + 4)?, f32_at(data, p + 8)?])
                        };
                        segments.push(RuntimeSegment {
                            start: point(base + a)?,
                            end: point(base + a + 12)?,
                        });
                    }
                }
                let mut polylines = Vec::new();
                let mut patches = Vec::new();
                for (group, stride) in [(56, 4), (72, 16)] {
                    let count = u32_at(data, base + group)? as usize;
                    if count == 0 {
                        continue;
                    }
                    let offset = u32_at(data, base + group + 4)? as usize;
                    ensure!(
                        offset >= 80
                            && offset <= next - base
                            && count < (next - base - offset) / stride,
                        "Invalid runtime geometry table"
                    );
                    let points_start = offset + (count + 1) * stride;
                    for row in 0..count {
                        let a = u32_at(data, base + offset + row * stride)? as usize;
                        let b = u32_at(data, base + offset + (row + 1) * stride)? as usize;
                        ensure!(
                            a >= points_start
                                && b >= a + 24
                                && b <= next - base
                                && (b - a) % 12 == 0,
                            "Invalid runtime geometry point range"
                        );
                        let point = |p| -> Result<[f32; 3]> {
                            Ok([f32_at(data, p)?, f32_at(data, p + 4)?, f32_at(data, p + 8)?])
                        };
                        let points = (a..b)
                            .step_by(12)
                            .map(|p| point(base + p))
                            .collect::<Result<Vec<_>>>()?;
                        if group == 56 {
                            polylines.push(points);
                        } else {
                            ensure!(
                                points.len() >= 4 && points.first() == points.last(),
                                "Runtime patch is not a closed loop"
                            );
                            let direction = point(base + offset + row * stride + 4)?;
                            patches.push(RuntimePatch { points, direction });
                        }
                    }
                }
                let mut unbound_links = Vec::new();
                let count = u32_at(data, base + 48)? as usize;
                if count != 0 {
                    let offset = u32_at(data, base + 52)? as usize;
                    ensure!(
                        offset >= 80
                            && offset <= next - base
                            && count <= (next - base - offset) / 44,
                        "Invalid runtime unbound-link range"
                    );
                    for row in 0..count {
                        let p = base + offset + row * 44;
                        let point = |p| -> Result<[f32; 3]> {
                            Ok([f32_at(data, p)?, f32_at(data, p + 4)?, f32_at(data, p + 8)?])
                        };
                        unbound_links.push(RuntimeUnboundLink {
                            offset: p,
                            segment: RuntimeSegment {
                                start: point(p)?,
                                end: point(p + 12)?,
                            },
                            link_words: [u32_at(data, p + 24)?, u32_at(data, p + 28)?],
                            direction: point(p + 32)?,
                        });
                    }
                }
                floor_ranges.push((base, next - base));
                floors.push(RuntimeFloor {
                    cell: cell_id,
                    layer: index as u32,
                    min_height,
                    max_height,
                    segments,
                    portals: Vec::new(),
                    polylines,
                    patches,
                    unbound_links,
                });
            }
        }
        let targets: std::collections::HashMap<_, _> = floor_ranges
            .iter()
            .enumerate()
            .map(|(index, &(offset, size))| (offset, (index, size)))
            .collect();
        ensure!(
            targets.len() == floors.len(),
            "Overlapping runtime floor identities"
        );
        for (index, &(base, size)) in floor_ranges.iter().enumerate() {
            let count = u32_at(data, base + 40)? as usize;
            if count == 0 {
                continue;
            }
            let offset = u32_at(data, base + 44)? as usize;
            ensure!(
                offset >= 80 && offset <= size && count <= (size - offset) / 32,
                "Invalid runtime portal range"
            );
            for portal in 0..count {
                let p = base + offset + portal * 32;
                let target = u32_at(data, p + 24)? as usize;
                let target_size = u32_at(data, p + 28)? as usize;
                let &(target_floor, size) = targets
                    .get(&target)
                    .context("Runtime portal targets an absent floor")?;
                ensure!(size == target_size, "Runtime portal target size mismatch");
                let point = |p| -> Result<[f32; 3]> {
                    Ok([f32_at(data, p)?, f32_at(data, p + 4)?, f32_at(data, p + 8)?])
                };
                floors[index].portals.push(RuntimePortal {
                    target_floor,
                    segment: RuntimeSegment {
                        start: point(p)?,
                        end: point(p + 12)?,
                    },
                });
            }
        }
        Ok(floors)
    }
    pub fn load(data: &[u8]) -> Result<Self> {
        ensure!(data.len() <= 64 * 1024 * 1024, "AI sector exceeds 64 MiB");
        ensure!(u32_at(data, 0)? == 2, "Unsupported AI sector version");
        let id = u32_at(data, 4)?;
        let n = u32_at(data, 16)? as usize;
        let e = u32_at(data, 20)? as usize;
        ensure!(
            n <= data.len().saturating_sub(24) / 36,
            "Invalid navigation node count"
        );
        let edge_offset = 24 + n * 36;
        ensure!(
            e <= (data.len() - edge_offset) / 12,
            "Invalid navigation edge count"
        );
        let mesh_offset = edge_offset + e * 12;
        ensure!(
            data.get(mesh_offset..mesh_offset + 18) == Some(b"KynogonRuntimeMesh"),
            "Missing navigation runtime mesh"
        );
        ensure!(
            u32_at(data, mesh_offset + 20)? == 2,
            "Unsupported navigation mesh version"
        );
        ensure!(
            data.len() >= mesh_offset + 32
                && u32_at(data, data.len() - 8)? as usize == data.len() - mesh_offset - 8,
            "Invalid runtime mesh length"
        );
        let mut nodes = Vec::with_capacity(n);
        let mut edges = Vec::with_capacity(e);
        let mut ids = std::collections::BTreeSet::new();
        for i in 0..n {
            let p = 24 + i * 36;
            let id = u32_at(data, p)?;
            ensure!(ids.insert(id), "Duplicate navigation node {id}");
            nodes.push(Node {
                id,
                position: [
                    f32_at(data, p + 4)?,
                    f32_at(data, p + 8)?,
                    f32_at(data, p + 12)?,
                ],
            });
        }
        for i in 0..e {
            let p = edge_offset + i * 12;
            let from = u32_at(data, p + 4)? as usize;
            let to = u32_at(data, p + 8)? as usize;
            ensure!(
                from < n && to < n,
                "Navigation edge references an absent node"
            );
            edges.push(Edge {
                flags: u32_at(data, p)?,
                from,
                to,
            });
        }
        Ok(Self {
            bytes: Arc::new(data.to_vec()),
            id,
            nodes,
            edges,
            mesh_offset,
        })
    }
    pub fn save(&self) -> Result<Vec<u8>> {
        self.save_with_metadata(false)
    }
    /// Explicitly rebuild derived data, including malformed permutations left
    /// by older tools. Positions, topology and runtime geometry are preserved.
    pub fn repair_graph_metadata(&self) -> Result<Vec<u8>> {
        self.save_with_metadata(true)
    }
    fn save_with_metadata(&self, repair: bool) -> Result<Vec<u8>> {
        ensure!(
            self.id == u32_at(&self.bytes, 4)?
                && self.nodes.len() == u32_at(&self.bytes, 16)? as usize
                && self.edges.len() == u32_at(&self.bytes, 20)? as usize,
            "Changing navigation topology requires rebuilding the runtime mesh"
        );
        let edge_offset = 24 + self.nodes.len() * 36;
        ensure!(
            self.mesh_offset == edge_offset + self.edges.len() * 12,
            "Changing the runtime mesh offset is unsupported"
        );
        for (i, n) in self.nodes.iter().enumerate() {
            ensure!(
                n.id == u32_at(&self.bytes, 24 + i * 36)?,
                "Changing navigation node identifiers requires rebuilding the runtime mesh"
            );
        }
        for (i, e) in self.edges.iter().enumerate() {
            let p = edge_offset + i * 12;
            ensure!(
                e.from == u32_at(&self.bytes, p + 4)? as usize
                    && e.to == u32_at(&self.bytes, p + 8)? as usize,
                "Changing prelinked connections requires rebuilding the runtime mesh"
            );
            ensure!(
                e.flags & 0x7fff_ffff == u32_at(&self.bytes, p)? & 0x7fff_ffff,
                "Changing edge distance requires rebuilding the runtime mesh"
            );
        }
        let mut bytes = (*self.bytes).clone();
        let mut moved = vec![repair; self.nodes.len()];
        for (i, n) in self.nodes.iter().enumerate() {
            for k in 0..3 {
                ensure!(
                    n.position[k].is_finite(),
                    "Node {} has invalid coordinates",
                    n.id
                );
                let p = 24 + i * 36 + 4 + k * 4;
                moved[i] |= bytes[p..p + 4] != n.position[k].to_le_bytes();
                bytes[p..p + 4].copy_from_slice(&n.position[k].to_le_bytes());
            }
        }
        if moved.iter().any(|v| *v) {
            ensure!(
                self.nodes.len() <= u16::MAX as usize + 1,
                "Too many nodes for spatial indexes"
            );
            let mut orders = match self.spatial_orders() {
                Ok(orders) => orders,
                Err(_) if repair => std::array::from_fn(|_| (0..self.nodes.len()).collect()),
                Err(e) => return Err(e),
            };
            for axis in 0..3 {
                // Stable ties retain the original ordering, including signed zero.
                orders[axis].sort_by(|&a, &b| {
                    self.nodes[a].position[axis]
                        .partial_cmp(&self.nodes[b].position[axis])
                        .unwrap()
                });
                for (row, &index) in orders[axis].iter().enumerate() {
                    let p = 24 + row * 36 + 28 + axis * 2;
                    bytes[p..p + 2].copy_from_slice(&(index as u16).to_le_bytes());
                }
            }
            for (index, edge) in self.edges.iter().enumerate() {
                if !moved[edge.from] && !moved[edge.to] {
                    continue;
                }
                let packed = self.edge_cost(edge)?;
                let p = edge_offset + index * 12;
                bytes[p..p + 4].copy_from_slice(&packed.to_le_bytes());
            }
        }
        for (index, edge) in self.edges.iter().enumerate() {
            let p = edge_offset + index * 12;
            if edge.flags != u32_at(&self.bytes, p)? {
                bytes[p..p + 4].copy_from_slice(&edge.flags.to_le_bytes());
            }
        }
        Ok(bytes)
    }
    /// Shortest directed graph route by geometric distance. This does not
    /// simulate runtime mesh traversal, agent clearance, or game movement rules.
    pub fn route(&self, start: usize, goal: usize) -> Result<Option<Vec<usize>>> {
        ensure!(
            start < self.nodes.len() && goal < self.nodes.len(),
            "Select valid route endpoints"
        );
        #[derive(PartialEq)]
        struct Visit(f64, usize);
        impl Eq for Visit {}
        impl PartialOrd for Visit {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for Visit {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                other
                    .0
                    .total_cmp(&self.0)
                    .then_with(|| other.1.cmp(&self.1))
            }
        }
        let mut adjacent = vec![Vec::new(); self.nodes.len()];
        for edge in &self.edges {
            ensure!(
                edge.from < self.nodes.len() && edge.to < self.nodes.len(),
                "Invalid graph connection"
            );
            let from = self.nodes[edge.from].position;
            let to = self.nodes[edge.to].position;
            let distance = (0..3)
                .map(|k| (from[k] as f64 - to[k] as f64).powi(2))
                .sum::<f64>()
                .sqrt();
            ensure!(distance.is_finite(), "Invalid graph coordinate");
            adjacent[edge.from].push((edge.to, distance));
        }
        let mut distance = vec![f64::INFINITY; self.nodes.len()];
        let mut previous = vec![None; self.nodes.len()];
        let mut queue = std::collections::BinaryHeap::new();
        distance[start] = 0.0;
        queue.push(Visit(0.0, start));
        while let Some(Visit(cost, node)) = queue.pop() {
            if cost > distance[node] {
                continue;
            }
            if node == goal {
                let mut path = vec![node];
                while let Some(parent) = previous[*path.last().unwrap()] {
                    path.push(parent);
                }
                path.reverse();
                return Ok(Some(path));
            }
            for &(next, length) in &adjacent[node] {
                let cost = cost + length;
                if cost < distance[next] {
                    distance[next] = cost;
                    previous[next] = Some(node);
                    queue.push(Visit(cost, next));
                }
            }
        }
        Ok(None)
    }
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for n in &self.nodes {
            for k in 0..3 {
                min[k] = min[k].min(n.position[k]);
                max[k] = max[k].max(n.position[k]);
            }
        }
        if self.nodes.is_empty() {
            ([0.0; 3], [0.0; 3])
        } else {
            (min, max)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires imported retail navigation sectors"]
    fn retail_node_edits_rebuild_lookup_and_incident_costs_only() {
        let catalog: serde_json::Value = serde_json::from_slice(
            &std::fs::read("target/launcher-fixture/catalog-graph.json").unwrap(),
        )
        .unwrap();
        let root = std::path::Path::new(catalog["root"].as_str().unwrap());
        let mut count = 0;
        for path in catalog["loose"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .filter(|p| p.ends_with(".prelnk.obj"))
        {
            let bytes = std::fs::read(root.join(path)).unwrap();
            let mut sector = Sector::load(&bytes).unwrap();
            assert!(sector.graph_metadata_issues().unwrap().is_empty());
            assert_eq!(sector.repair_graph_metadata().unwrap(), bytes);
            for (axis, order) in sector.spatial_orders().unwrap().iter().enumerate() {
                assert!(order.windows(2).all(
                    |p| sector.nodes[p[0]].position[axis] <= sector.nodes[p[1]].position[axis]
                ));
            }
            for edge in &sector.edges {
                let length = (0..3)
                    .map(|k| {
                        (sector.nodes[edge.from].position[k] as f64
                            - sector.nodes[edge.to].position[k] as f64)
                            .powi(2)
                    })
                    .sum::<f64>()
                    .sqrt();
                assert_eq!(edge.flags & 0x7fff_ffff, (length * 16.).floor() as u32);
            }
            let (_, max) = sector.bounds();
            sector.nodes[0].position = max.map(|v| v + 1.);
            let edited = sector.save().unwrap_or_else(|e| panic!("{path}: {e:#}"));
            let mut legacy = bytes.clone();
            legacy[28..40].copy_from_slice(&edited[28..40]);
            let stale = Sector::load(&legacy).unwrap();
            assert!(!stale.graph_metadata_issues().unwrap().is_empty());
            assert_eq!(stale.repair_graph_metadata().unwrap(), edited);
            assert_eq!(&edited[sector.mesh_offset..], &bytes[sector.mesh_offset..]);
            let loaded = Sector::load(&edited).unwrap();
            for (axis, order) in loaded.spatial_orders().unwrap().iter().enumerate() {
                assert!(order.windows(2).all(
                    |p| loaded.nodes[p[0]].position[axis] <= loaded.nodes[p[1]].position[axis]
                ));
            }
            for (a, b) in sector.edges.iter().zip(&loaded.edges) {
                assert_eq!((a.from, a.to), (b.from, b.to));
                if a.from != 0 && a.to != 0 {
                    assert_eq!(a.flags, b.flags);
                } else {
                    let length = (0..3)
                        .map(|k| {
                            (loaded.nodes[b.from].position[k] as f64
                                - loaded.nodes[b.to].position[k] as f64)
                                .powi(2)
                        })
                        .sum::<f64>()
                        .sqrt();
                    assert_eq!(b.flags & 0x7fff_ffff, (length * 16.).floor() as u32);
                    assert_eq!(a.flags & 0x8000_0000, b.flags & 0x8000_0000);
                }
            }
            for i in 0..sector.nodes.len() {
                let p = 24 + i * 36;
                assert_eq!(&edited[p + 16..p + 28], &bytes[p + 16..p + 28]);
                assert_eq!(&edited[p + 34..p + 36], &bytes[p + 34..p + 36]);
            }
            assert_eq!(loaded.save().unwrap(), edited);
            count += 1;
        }
        assert_eq!(count, 100);
        println!("{count} sectors: node edits rebuild spatial permutations and incident metrics; metadata and runtime mesh unchanged");
    }
    #[test]
    fn runtime_polylines_and_closed_patches_preserve_geometry_and_link_words() {
        let mut bytes = vec![0u8; 368];
        for (p, v) in [
            (28, 7u32),
            (56, 1),
            (60, 1),
            (68, 80),
            (80, 88),
            (84, 368),
            (88, 1),
            (92, 108),
            (104, 368),
            (108, 7),
            (156, 1),
            (160, 216),
            (164, 1),
            (168, 80),
            (180, 1),
            (184, 124),
            (188, 88),
            (192, 124),
            (232, 156),
            (248, 216),
            (348, u32::MAX),
            (352, u32::MAX),
        ] {
            bytes[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        let mut point = |p, xyz: [f32; 3]| {
            for (axis, v) in xyz.into_iter().enumerate() {
                bytes[p + axis * 4..p + axis * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        };
        for (i, p) in [[0., 0., 0.], [0., 0., 1.], [0., 0., 2.]]
            .into_iter()
            .enumerate()
        {
            point(196 + i * 12, p);
        }
        for (i, p) in [
            [0., 0., 0.],
            [1., 0., 0.],
            [1., 0., 1.],
            [0., 0., 1.],
            [0., 0., 0.],
        ]
        .into_iter()
        .enumerate()
        {
            point(264 + i * 12, p);
        }
        point(236, [1., 0., 0.]);
        point(324, [2., 0., 0.]);
        point(336, [2., 0., 1.]);
        point(356, [1., 0., 0.]);
        let mut sector = Sector {
            bytes: Arc::new(bytes),
            id: 7,
            nodes: vec![],
            edges: vec![],
            mesh_offset: 0,
        };
        let floor = sector.runtime_floors().unwrap().remove(0);
        assert_eq!(floor.polylines[0].len(), 3);
        assert_eq!(floor.patches[0].direction, [1., 0., 0.]);
        assert_eq!(floor.patches[0].points.len(), 5);
        assert_eq!(floor.unbound_links[0].link_words, [u32::MAX; 2]);
        assert_eq!(floor.unbound_links[0].direction, [1., 0., 0.]);
        assert_eq!(floor.outlines().count(), 7);
        Arc::make_mut(&mut sector.bytes)[312..316].copy_from_slice(&3f32.to_le_bytes());
        assert!(sector
            .runtime_floors()
            .unwrap_err()
            .to_string()
            .contains("closed loop"));
        Arc::make_mut(&mut sector.bytes)[312..316].copy_from_slice(&0f32.to_le_bytes());
        Arc::make_mut(&mut sector.bytes)[196..200].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(sector.runtime_floors().is_err());
        Arc::make_mut(&mut sector.bytes)[196..200].copy_from_slice(&0f32.to_le_bytes());
        Arc::make_mut(&mut sector.bytes).resize(412, 0);
        for (p, v) in [
            (72, 1u32),
            (76, 368),
            (368, 8),
            (372, 1),
            (376, 380),
            (380, 384),
            (384, 2),
            (388, 3),
            (392, 1),
            (396, 1),
            (400, 324),
        ] {
            Arc::make_mut(&mut sector.bytes)[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        let links = sector.runtime_sector_connections().unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!((links[0].source_floor, links[0].source_link), (0, 0));
        assert_eq!(
            (
                links[0].target_sector,
                links[0].target_cell,
                links[0].target_layer
            ),
            (8, [2, 3], 1)
        );
        Arc::make_mut(&mut sector.bytes)[400..404].copy_from_slice(&325u32.to_le_bytes());
        assert!(sector
            .runtime_sector_connections()
            .unwrap_err()
            .to_string()
            .contains("absent local link"));
        Arc::make_mut(&mut sector.bytes)[400..404].copy_from_slice(&324u32.to_le_bytes());
        Arc::make_mut(&mut sector.bytes).resize(416, 0);
        Arc::make_mut(&mut sector.bytes)[396..400].copy_from_slice(&2u32.to_le_bytes());
        Arc::make_mut(&mut sector.bytes)[404..408].copy_from_slice(&324u32.to_le_bytes());
        assert!(sector
            .runtime_sector_connections()
            .unwrap_err()
            .to_string()
            .contains("Duplicate"));
        Arc::make_mut(&mut sector.bytes)[76..80].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(sector.runtime_sector_connections().is_err());
    }
    #[test]
    fn runtime_floor_offsets_are_relative_and_reject_bad_geometry() {
        // One cell, one floor, one pair of points. Runtime mesh starts at byte 16.
        let mut bytes = vec![0u8; 16 + 220];
        let mesh = &mut bytes[16..];
        for (p, v) in [
            (28, 7u32),
            (56, 1),
            (60, 1),
            (68, 80),
            (80, 88),
            (84, 220),
            (88, 1),
            (92, 108),
            (104, 220),
            (108, 7),
            (172, 1),
            (176, 80),
            (188, 88),
            (192, 112),
        ] {
            mesh[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        for (p, v) in [
            (96, 2f32),
            (100, 5.),
            (196, 10.),
            (200, 3.),
            (204, 20.),
            (208, 12.),
            (212, 4.),
            (216, 21.),
        ] {
            mesh[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        let mut sector = Sector {
            bytes: Arc::new(bytes),
            id: 7,
            nodes: vec![],
            edges: vec![],
            mesh_offset: 16,
        };
        let floor = sector.runtime_floors().unwrap().remove(0);
        assert_eq!(floor.cell, [0, 0]);
        assert_eq!((floor.min_height, floor.max_height), (2., 5.));
        assert_eq!(floor.segments[0].start, [10., 3., 20.]);
        assert_eq!(floor.segments[0].end, [12., 4., 21.]);
        let original = sector.bytes.clone();
        Arc::make_mut(&mut sector.bytes)[16 + 192..16 + 196]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(sector.runtime_floors().is_err());
        sector.bytes = original.clone();
        Arc::make_mut(&mut sector.bytes)[16 + 196..16 + 200]
            .copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(sector.runtime_floors().is_err());
        for size in 0..original.len() {
            sector.bytes = Arc::new(original[..size].to_vec());
            assert!(
                sector.runtime_floors().is_err(),
                "Accepted truncated runtime mesh at {size}"
            );
        }
        sector.bytes = original;
        Arc::make_mut(&mut sector.bytes).resize(16 + 252, 0);
        for (p, v) in [
            (84, 252u32),
            (104, 252),
            (148, 1),
            (152, 112),
            (244, 108),
            (248, 144),
        ] {
            Arc::make_mut(&mut sector.bytes)[16 + p..16 + p + 4].copy_from_slice(&v.to_le_bytes());
        }
        assert_eq!(
            sector.runtime_floors().unwrap()[0].portals[0].target_floor,
            0
        );
        Arc::make_mut(&mut sector.bytes)[16 + 248..16 + 252].copy_from_slice(&143u32.to_le_bytes());
        assert!(sector
            .runtime_floors()
            .unwrap_err()
            .to_string()
            .contains("size mismatch"));
        Arc::make_mut(&mut sector.bytes)[16 + 244..16 + 248].copy_from_slice(&109u32.to_le_bytes());
        assert!(sector
            .runtime_floors()
            .unwrap_err()
            .to_string()
            .contains("absent floor"));
    }
    #[test]
    #[ignore = "Requires the imported retail navigation sectors"]
    fn all_retail_runtime_segments_decode_without_modifying_bytes() {
        let catalog: serde_json::Value = serde_json::from_slice(
            &std::fs::read("target/launcher-fixture/catalog-graph.json").unwrap(),
        )
        .unwrap();
        let root = std::path::Path::new(catalog["root"].as_str().unwrap());
        let (mut sectors, mut floors, mut segments, mut portals) = (0, 0, 0, 0);
        let (mut polylines, mut patches, mut unbound_links) = (0, 0, 0);
        let mut floor_ids = std::collections::BTreeSet::new();
        let mut neighbor_targets = Vec::new();
        for path in catalog["loose"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str())
            .filter(|p| p.ends_with(".prelnk.obj"))
        {
            let bytes = std::fs::read(root.join(path)).unwrap();
            let sector = Sector::load(&bytes).unwrap();
            let decoded = sector
                .runtime_floors()
                .unwrap_or_else(|e| panic!("{path}: {e:#}"));
            let folder = path.rsplit_once('/').unwrap().0.to_owned();
            for floor in &decoded {
                floor_ids.insert((folder.clone(), sector.id, floor.cell, floor.layer));
            }
            for link in sector
                .runtime_sector_connections()
                .unwrap_or_else(|e| panic!("{path}: {e:#}"))
            {
                assert!(link.source_link < decoded[link.source_floor].unbound_links.len());
                neighbor_targets.push((
                    folder.clone(),
                    link.target_sector,
                    link.target_cell,
                    link.target_layer,
                ));
            }
            floors += decoded.len();
            segments += decoded.iter().map(|f| f.segments.len()).sum::<usize>();
            portals += decoded.iter().map(|f| f.portals.len()).sum::<usize>();
            polylines += decoded.iter().map(|f| f.polylines.len()).sum::<usize>();
            patches += decoded.iter().map(|f| f.patches.len()).sum::<usize>();
            unbound_links += decoded.iter().map(|f| f.unbound_links.len()).sum::<usize>();
            assert_eq!(sector.save().unwrap(), bytes);
            sectors += 1;
        }
        assert_eq!((sectors, floors, segments), (100, 2757, 55235));
        assert_eq!(portals, 5633);
        assert_eq!((polylines, patches, unbound_links), (2975, 441, 752));
        assert_eq!(neighbor_targets.len(), 726);
        for target in &neighbor_targets {
            assert!(
                floor_ids.contains(target),
                "Missing neighbor floor {target:?}"
            );
        }
        println!("{sectors} sectors, {floors} runtime floors, {segments} runtime segments, {portals} local floor links; exact roundtrips");
        println!(
            "{polylines} polylines, {patches} closed patches, {unbound_links} unbound link records"
        );
        println!(
            "{} cross-sector entries resolve to existing destination floors",
            neighbor_targets.len()
        );
    }
    #[test]
    fn hpd_edits_preserve_sector_identifiers_and_metadata() {
        let mut b = vec![0u8; 204];
        b[..12].copy_from_slice(b"Kynogon HPD\0");
        for (p, v) in [
            (12, 2u32),
            (16, 2),
            (20, 1),
            (156, 1000),
            (188, 991),
            (192, 17),
        ] {
            b[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        for k in 0..3 {
            b[164 + k * 8..168 + k * 8].copy_from_slice(&10f32.to_le_bytes());
        }
        let mut d = Database::load(&b).unwrap();
        assert_eq!(d.save().unwrap(), b);
        d.sectors[0].max[1] = 20.0;
        let out = d.save().unwrap();
        assert_eq!(&out[..172], &b[..172]);
        assert_eq!(&out[176..], &b[176..]);
        d.sectors[0].min[1] = 21.0;
        assert!(d.save().is_err());
        assert_eq!(
            Database::sector_path("Environments/Test/AI_one/data.hpd", 1000).unwrap(),
            "sectors/ai_one/sector_1000.prelnk.obj"
        );
        for n in 0..b.len() {
            assert!(Database::load(&b[..n]).is_err());
        }
        let mut d = Database::load(&b).unwrap();
        d.sectors[0].id += 1;
        assert!(d.save().is_err());
        d.sectors.clear();
        assert!(d.save().is_err());
    }
    #[test]
    fn sector_roundtrip_and_bad_links() {
        let mut b = vec![0u8; 24 + 72 + 12 + 40];
        for (p, v) in [
            (0, 2u32),
            (4, 1000),
            (16, 2),
            (20, 1),
            (24, 101),
            (60, 105),
            (88, 0x00010001),
            (92, 0x12340001),
            (96, 0x80000000),
            (100, 0),
            (104, 1),
            (128, 2),
            (140, 32),
        ] {
            b[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[108..126].copy_from_slice(b"KynogonRuntimeMesh");
        let mut s = Sector::load(&b).unwrap();
        let snapshot = s.clone();
        assert!(Arc::ptr_eq(&s.bytes, &snapshot.bytes));
        assert_eq!(s.save().unwrap(), b);
        let mut marker = Sector::load(&b).unwrap();
        marker.edges[0].flags &= 0x7fff_ffff;
        let marker_bytes = marker.save().unwrap();
        assert_eq!(u32_at(&marker_bytes, 96).unwrap(), 0);
        assert_eq!(&marker_bytes[108..], &b[108..]);
        assert_eq!(&marker_bytes[..96], &b[..96]);
        assert_eq!(&marker_bytes[100..], &b[100..]);
        marker.edges[0].flags |= 0x8000_0000;
        assert_eq!(marker.save().unwrap(), b);
        s.nodes[1].position = [-1.0, 2.0, 3.0];
        let edited = s.save().unwrap();
        assert_eq!(
            snapshot.save().unwrap(),
            b,
            "Saving an edit must not change an undo snapshot"
        );
        assert_eq!(&edited[108..], &b[108..]);
        assert_eq!(&edited[94..96], &b[94..96]);
        assert_eq!(u32_at(&edited, 96).unwrap(), 0x8000003b);
        let reloaded = Sector::load(&edited).unwrap();
        assert_eq!(
            reloaded.spatial_orders().unwrap(),
            [vec![1, 0], vec![0, 1], vec![0, 1]]
        );
        assert_eq!(reloaded.save().unwrap(), edited);
        assert!(reloaded.graph_metadata_issues().unwrap().is_empty());
        assert_eq!(
            Sector::load(&b).unwrap().repair_graph_metadata().unwrap(),
            b
        );
        let mut legacy = b.clone();
        legacy[64..76].copy_from_slice(&edited[64..76]);
        let stale = Sector::load(&legacy).unwrap();
        assert_eq!(stale.graph_metadata_issues().unwrap().len(), 2);
        assert_eq!(
            stale.save().unwrap(),
            legacy,
            "An unchanged file must not be silently repaired"
        );
        assert_eq!(stale.repair_graph_metadata().unwrap(), edited);
        legacy[52..54].copy_from_slice(&7u16.to_le_bytes());
        let corrupt = Sector::load(&legacy).unwrap();
        assert!(corrupt.spatial_orders().is_err());
        assert_eq!(corrupt.repair_graph_metadata().unwrap(), edited);
        s.nodes[1].position = [0.; 3];
        assert_eq!(s.save().unwrap(), b);
        s.nodes[1].position = [f32::MAX; 3];
        assert!(s
            .save()
            .unwrap_err()
            .to_string()
            .contains("representable edge distance"));
        s.nodes[1].position = [-1., 2., 3.];
        s.edges[0].flags ^= 1;
        assert!(s.save().is_err());
        s = Sector::load(&edited).unwrap();
        s.nodes.push(s.nodes[0].clone());
        assert!(s.save().is_err());
        b[104..108].copy_from_slice(&2u32.to_le_bytes());
        assert!(Sector::load(&b).is_err());
        for n in 0..24 {
            assert!(Sector::load(&b[..n]).is_err());
        }
    }
    #[test]
    fn route_respects_direction_distance_and_disconnected_nodes() {
        let mut s = Sector {
            bytes: Arc::new(vec![]),
            id: 1,
            mesh_offset: 0,
            nodes: [
                [0.0, 0.0, 0.0],
                [0.0, 8.0, 0.0],
                [1.0, 0.0, 0.0],
                [2.0, 0.0, 0.0],
                [9.0, 0.0, 0.0],
            ]
            .into_iter()
            .enumerate()
            .map(|(i, position)| Node {
                id: (i + 100) as u32,
                position,
            })
            .collect(),
            edges: [(0, 1), (1, 3), (0, 2), (2, 3), (2, 0)]
                .into_iter()
                .map(|(from, to)| Edge { flags: 0, from, to })
                .collect(),
        };
        assert_eq!(s.route(0, 3).unwrap(), Some(vec![0, 2, 3]));
        assert_eq!(s.route(3, 0).unwrap(), None);
        assert_eq!(s.route(0, 4).unwrap(), None);
        assert_eq!(s.route(0, 0).unwrap(), Some(vec![0]));
        assert!(s.route(0, 5).is_err());
        s.nodes[2].position[1] = 100.0;
        assert_eq!(s.route(0, 3).unwrap(), Some(vec![0, 1, 3]));
    }
}
