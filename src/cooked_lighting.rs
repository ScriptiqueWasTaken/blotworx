//! EM1 BKLT0001: entity lookup, mesh colour streams and draw-group mappings.
//! Lookup order differs from record order. The trailing lookup storage contains
//! opaque cook-time data and must be retained, including when a file is edited.
use anyhow::{ensure, Context, Result};
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct Mesh {
    pub colours: Range<usize>,
    pub groups: Vec<Vec<u32>>,
}
impl Mesh {
    pub fn vertices(&self) -> usize {
        self.colours.len() / 4
    }
}
#[derive(Clone, Debug)]
pub struct Entity {
    pub id: u32,
    pub meshes: Vec<Mesh>,
}
#[derive(Clone, Debug)]
pub struct Lighting {
    bytes: Vec<u8>,
    pub entities: Vec<Entity>,
}
fn word(data: &[u8], p: &mut usize) -> Result<usize> {
    let b = data
        .get(*p..p.checked_add(4).context("Lighting offset overflow")?)
        .context("Truncated lighting data")?;
    *p += 4;
    Ok(u32::from_be_bytes(b.try_into()?) as usize)
}
impl Lighting {
    pub fn load(data: &[u8]) -> Result<Self> {
        ensure!(
            data.len() <= 128 * 1024 * 1024,
            "Lighting file exceeds 128 MiB"
        );
        ensure!(
            data.get(..8) == Some(b"BKLT0001"),
            "Unsupported cooked lighting version"
        );
        let mut p = 8;
        let index = word(data, &mut p)?;
        let count = word(data, &mut p)?;
        ensure!(
            index >= 16 && index % 4 == 0 && index <= data.len(),
            "Invalid lighting lookup offset"
        );
        ensure!(
            count <= (data.len() - index) / 8,
            "Truncated lighting lookup"
        );
        let mut entries = Vec::with_capacity(count);
        p = index;
        for _ in 0..count {
            let id = word(data, &mut p)? as u32;
            let start = word(data, &mut p)?;
            ensure!(
                start >= 16 && start < index && start % 4 == 0,
                "Invalid lighting record offset"
            );
            entries.push((id, start));
        }
        entries.sort_by_key(|(_, offset)| *offset);
        let mut entities = Vec::with_capacity(count);
        for (i, &(id, start)) in entries.iter().enumerate() {
            let end = entries.get(i + 1).map(|e| e.1).unwrap_or(index);
            ensure!(start < end, "Overlapping lighting records");
            let record = &data[..end];
            p = start;
            let meshes = word(record, &mut p)?;
            ensure!(meshes <= (end - p) / 8, "Invalid lighting mesh count");
            let mut result = Vec::with_capacity(meshes);
            for _ in 0..meshes {
                let vertices = word(record, &mut p)?;
                ensure!(vertices <= (end - p) / 4, "Invalid lighting vertex count");
                let colours = p..p + vertices * 4;
                p = colours.end;
                let groups = word(record, &mut p)?;
                ensure!(groups <= (end - p) / 4, "Invalid lighting group count");
                let mut mapping = Vec::with_capacity(groups);
                for _ in 0..groups {
                    let n = word(record, &mut p)?;
                    ensure!(n <= (end - p) / 4, "Invalid lighting group mapping");
                    let mut values = Vec::with_capacity(n);
                    for _ in 0..n {
                        values.push(word(record, &mut p)? as u32);
                    }
                    mapping.push(values);
                }
                result.push(Mesh {
                    colours,
                    groups: mapping,
                });
            }
            ensure!(p == end, "Unexpected lighting data for entity {id}");
            entities.push(Entity { id, meshes: result });
        }
        entities.sort_by_key(|e| e.id);
        Ok(Self {
            bytes: data.to_vec(),
            entities,
        })
    }
    pub fn save(&self) -> Vec<u8> {
        self.bytes.clone()
    }
    pub fn colours(&self, mesh: &Mesh) -> &[u8] {
        &self.bytes[mesh.colours.clone()]
    }
    /// Operates on RGB only. Alpha, mesh topology, IDs and unknown data survive.
    pub fn grade(
        &mut self,
        entity: Option<usize>,
        mesh: Option<usize>,
        exposure: f32,
        tint: [f32; 3],
        replace: bool,
    ) -> Result<()> {
        ensure!(
            exposure.is_finite() && (-16.0..=16.0).contains(&exposure),
            "Invalid exposure"
        );
        ensure!(
            tint.iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Invalid tint"
        );
        if let Some(i) = entity {
            let e = self.entities.get(i).context("Select a lighting entity")?;
            if let Some(j) = mesh {
                ensure!(j < e.meshes.len(), "Select a lighting mesh");
            }
        } else {
            ensure!(mesh.is_none(), "A mesh requires an entity selection");
        }
        let gain = exposure.exp2();
        for (i, e) in self.entities.iter().enumerate() {
            if entity.is_some_and(|n| n != i) {
                continue;
            }
            for (j, m) in e.meshes.iter().enumerate() {
                if mesh.is_some_and(|n| n != j) {
                    continue;
                }
                for c in self.bytes[m.colours.clone()].chunks_exact_mut(4) {
                    for k in 0..3 {
                        c[k] = ((if replace { 255.0 } else { c[k] as f32 }) * gain * tint[k])
                            .round()
                            .clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = b"BKLT0001".to_vec();
        for v in [40u32, 1, 1, 2] {
            b.extend(v.to_be_bytes());
        }
        b.extend([16, 32, 64, 37, 0, 0, 0, 255]);
        for v in [1u32, 0, 42, 16] {
            b.extend(v.to_be_bytes());
        }
        b.extend([0xa5; 12]);
        b
    }
    #[test]
    fn grading_retains_alpha_index_and_unknown_bytes() {
        let bytes = fixture();
        let mut d = Lighting::load(&bytes).unwrap();
        assert_eq!(d.save(), bytes);
        d.grade(Some(0), Some(0), 1.0, [1.0; 3], false).unwrap();
        let edited = d.save();
        assert_eq!(&edited[24..28], &[32, 64, 128, 37]);
        assert_eq!(&edited[32..], &bytes[32..]);
        Lighting::load(&edited).unwrap();
        d.grade(None, None, 0.0, [1.0; 3], true).unwrap();
        assert_eq!(&d.save()[28..32], &[255, 255, 255, 255]);
    }
    #[test]
    fn truncated_and_hostile_lighting_is_rejected() {
        let bytes = fixture();
        for n in 0..48 {
            assert!(Lighting::load(&bytes[..n]).is_err(), "{n}");
        }
        let mut b = bytes;
        b[20..24].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(Lighting::load(&b).is_err());
    }
}
