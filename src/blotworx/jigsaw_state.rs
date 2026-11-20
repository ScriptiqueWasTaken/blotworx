//! Original EM1 jigsaw initial paint state (REL 0x373b30..0x373da4).
//! Render/physics synchronization and stimulus scheduling are separate work.
use anyhow::{Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
/// JPS node constructor REL 0x24795c..0x247968; bound to all extended-mesh
/// subparts by 0x249a08 with four-byte material stride and zero index stride.
pub const RUNTIME_MATERIALS: [u32; 4] = [0x8016, 0x16, 0x8002, 2];
/// REL 0x246d1c..0x246d7c selects a material-table index, not a replacement
/// filter word. The subsequent loop writes this byte through each associated
/// extended-mesh subpart's material-index pointer (including convex subparts).
pub fn material_index(filter: u32) -> u8 {
    let painted = filter & 0x8000 != 0;
    match filter & 0x1f {
        0x16 => u8::from(!painted),
        2 => 2 + u8::from(!painted),
        _ => 0,
    }
}
#[derive(Debug)]
pub struct PaintState {
    pub painted: Vec<bool>,
}
impl PaintState {
    /// A nonempty saved piece-index array selects a distinct initialization
    /// path: all pieces start painted, then type-zero entries thin their piece.
    /// Type-one entries do NOT undo preceding type-zero entries. Out-of-range
    /// piece IDs and absent/other types do not change any piece.
    pub fn initial(
        piece_count: usize,
        default_painted: bool,
        indices: &[u16],
        types: &[i32],
    ) -> Self {
        let mut painted = vec![
            if indices.is_empty() {
                default_painted
            } else {
                true
            };
            piece_count
        ];
        for (i, &piece) in indices.iter().enumerate() {
            if types.get(i) == Some(&0) {
                if let Some(value) = painted.get_mut(piece as usize) {
                    *value = false;
                }
            }
        }
        Self { painted }
    }
    pub fn load(component: &Component, piece_count: usize) -> Result<Self> {
        let get = |name: &str| {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| &p.value)
        };
        let painted = get("Is Painted")
            .and_then(|v| v.as_bool())
            .context("Missing or invalid jigsaw Is Painted")?;
        let array = |name: &str| -> Result<&[serde_json::Value]> {
            match get(name) {
                Some(v) => v
                    .as_array()
                    .map(Vec::as_slice)
                    .with_context(|| format!("Invalid {name}")),
                None => Ok(&[]),
            }
        };
        let indices = array("ModificationPieceIndices")?
            .iter()
            .map(|v| {
                u16::try_from(v.as_u64().context("Invalid modification piece index")?)
                    .context("Modification piece index exceeds u16")
            })
            .collect::<Result<Vec<_>>>()?;
        let types = array("ModificationSphereTypes")?
            .iter()
            .map(|v| {
                i32::try_from(v.as_i64().context("Invalid modification type")?)
                    .context("Modification type exceeds i32")
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self::initial(piece_count, painted, &indices, &types))
    }
    /// REL 0x37500c counts bit 15 and divides in single precision. Empty nodes
    /// produce NaN in the original; callers must handle that in diagnostic UI.
    pub fn percentage(&self) -> f32 {
        self.painted.iter().filter(|v| **v).count() as f32 / self.painted.len() as f32
    }
    /// REL 0x374548..0x374560 and 0x373d1c..0x373d84. This returns the native
    /// material filter; it does not activate Box3D collision or change a mesh.
    pub fn filter(&self, piece: usize, motion_type: u8) -> Option<u32> {
        self.painted.get(piece).map(|&painted| {
            ((painted as u32) << 15)
                | if matches!(motion_type, 4 | 5) {
                    0x16
                } else {
                    2
                }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_material_selection_uses_only_layer_and_paint_bit() {
        for (filter, index) in [
            (0x8016, 0),
            (0x16, 1),
            (0x8002, 2),
            (2, 3),
            (0x8003, 0),
            (3, 0),
        ] {
            assert_eq!(material_index(filter), index);
            assert_eq!(material_index(filter | 0xffff0000 | 0x7fe0), index);
        }
        for motion in [2, 4, 5] {
            let state = PaintState::initial(2, true, &[1], &[0]);
            assert_eq!(
                material_index(state.filter(0, motion).unwrap()),
                if motion == 2 { 2 } else { 0 }
            );
            assert_eq!(
                material_index(state.filter(1, motion).unwrap()),
                if motion == 2 { 3 } else { 1 }
            );
        }
    }
    #[test]
    fn saved_modifications_override_default_and_are_not_last_write_wins() {
        let state = PaintState::initial(4, false, &[1, 1, 2, 12, 3], &[0, 1, 1, 0]);
        assert_eq!(state.painted, [true, false, true, true]);
        assert_eq!(state.percentage(), 0.75);
        assert_eq!(state.filter(0, 5), Some(0x8016));
        assert_eq!(state.filter(1, 4), Some(0x16));
        assert_eq!(state.filter(0, 2), Some(0x8002));
        assert_eq!(state.filter(1, 2), Some(2));
    }
    #[test]
    #[ignore = "Requires BLOTWORX_TEST_DATA pointing to original game files"]
    fn original_gremlin_village_initial_states() -> Result<()> {
        use epic_mickey_lib_rs::{
            file_manipulator::EndianType, packfile::Packfile, scene_file::SceneFile,
        };
        let root = std::path::PathBuf::from(std::env::var("BLOTWORX_TEST_DATA")?);
        let pak = Packfile::from_binary(std::fs::read(
            root.join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let scene_file = pak
            .files
            .iter()
            .find(|f| {
                f.path
                    .to_ascii_lowercase()
                    .ends_with("/gsa/gremlinvillage_zonea.bin")
            })
            .context("Missing village scene")?;
        let scene = SceneFile::from_binary(&scene_file.data, EndianType::BIG);
        for (suffix, count, modified) in [("Toon01 01", 26, 22), ("Toon02 01", 32, 28)] {
            let entity = scene
                .objects
                .iter()
                .find(|e| e.name.ends_with(suffix))
                .context("Missing original jigsaw entity")?;
            let c = entity
                .components
                .iter()
                .find(|c| c.class_name == "PhysicsJigsawComponent")
                .context("Missing original jigsaw component")?;
            let scope = super::super::jigsaw_overlap::load_key_scope(c)?;
            println!("{suffix}: Allow Nonoverlapping Painting = {scope}");
            let values = |name: &str| -> Result<&Vec<serde_json::Value>> {
                c.properties
                    .iter()
                    .find(|p| p.name == name)
                    .and_then(|p| p.value.as_array())
                    .context("Missing original modification array")
            };
            let indices = values("ModificationPieceIndices")?
                .iter()
                .map(|v| Ok(u16::try_from(v.as_u64().context("Invalid piece")?)?))
                .collect::<Result<Vec<_>>>()?;
            let types = values("ModificationSphereTypes")?
                .iter()
                .map(|v| Ok(i32::try_from(v.as_i64().context("Invalid type")?)?))
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(indices.len(), modified);
            let state = PaintState::initial(count, false, &indices, &types);
            assert_eq!(state.painted.iter().filter(|v| **v).count(), 4);
            assert!(indices.iter().all(|&i| !state.painted[i as usize]));
            println!("{suffix}: {} painted / {} pieces", 4, count);
        }
        Ok(())
    }
    #[test]
    fn empty_modification_list_retains_default_and_empty_node_matches_native_division() {
        assert_eq!(
            PaintState::initial(2, false, &[], &[0]).painted,
            [false, false]
        );
        assert_eq!(PaintState::initial(2, true, &[], &[]).percentage(), 1.);
        assert!(PaintState::initial(0, true, &[], &[]).percentage().is_nan());
        assert_eq!(PaintState::initial(0, true, &[], &[]).filter(0, 5), None);
    }
}
