//! Entity-manager spawn override, REL1837a4..1838d0; MICK20304/20254.
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Transform {
    pub rotation: [f32; 9],
    pub position: [f32; 3],
    pub scale: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct State {
    pub override_transform: Option<Transform>,
}
impl State {
    /// Native183834 copies rotation/position and substitutes constant4bcf88.
    /// This does not teleport the player or change any marker/component.
    pub fn set(&mut self, rotation: [f32; 9], position: [f32; 3]) {
        self.override_transform = Some(Transform {
            rotation,
            position,
            scale: f32::from_bits(0x3dcccccd),
        });
    }
    pub fn clear(&mut self) {
        self.override_transform = None;
    }
    /// MICK20304 uses a stored override when present, otherwise the live player.
    pub fn selected(&self, live: Transform) -> Transform {
        self.override_transform.unwrap_or(live)
    }
    /// Save20254 reads all three fields before invoking183834. Consequently a
    /// stored non-default scale is read, then replaced with native Mickey scale.
    pub fn restore(&mut self, payload: &[u8]) -> Result<bool> {
        let Some(saved) = saved_transform(payload)? else {
            return Ok(false);
        };
        self.set(saved.rotation, saved.position);
        Ok(true)
    }
}
pub fn append_records(output: &mut Vec<u8>, transform: Transform) -> Result<()> {
    fn bytes(values: &[f32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_bits().to_be_bytes())
            .collect()
    }
    super::save_game::append_record(output, "PlayerSpawnLoc", &bytes(&transform.position), 4)?;
    super::save_game::append_record(output, "PlayerSpawnRot", &bytes(&transform.rotation), 4)?;
    super::save_game::append_record(output, "PlayerSpawnScale", &bytes(&[transform.scale]), 4)?;
    Ok(())
}
impl super::assets::World {
    ///20304 bypasses the player getters when1837b0 supplies an override.
    pub fn player_spawn_save_transform(&self) -> Result<Transform> {
        if let Some(transform) = self.player_spawn.override_transform {
            return Ok(transform);
        }
        let owner = self
            .entities
            .get(self.player_entity)
            .ok_or_else(|| anyhow::anyhow!("Live Mickey transform is missing"))?;
        ensure!(
            owner
                .components
                .iter()
                .filter(|component| component.class_name == "JPSTransformationComponent")
                .count()
                == 1,
            "Live Mickey transform component selection is missing or ambiguous"
        );
        ensure!(
            owner.transform_parent_id()?.is_none(),
            "Parented player spawn transform getters require native composition integration"
        );
        // Current transform-owner position/unscaled rotation, not report.spawn,
        // authored_transform or an interpolated/skinned presentation matrix.
        // The scale field has no gameplay mutation receiver yet; it retains its
        // initialized value. Never infer it by dividing a render basis.
        Ok(Transform {
            position: owner.transform[12..15].try_into()?,
            rotation: owner.runtime_rotation,
            scale: owner.authored_scale,
        })
    }
}
/// Read the spawn fields from the complete MICK record stream, retaining its
/// other fields. Missing fields leave the entity-manager override unchanged.
pub fn saved_transform(payload: &[u8]) -> Result<Option<Transform>> {
    let records = super::save_game::records(payload)?;
    fn floats<const N: usize>(
        records: &[super::save_game::Record<'_>],
        name: &str,
    ) -> Result<Option<[f32; N]>> {
        let mut matching = records.iter().filter(|record| record.name == name);
        let Some(record) = matching.next() else {
            return Ok(None);
        };
        ensure!(
            matching.next().is_none(),
            "Duplicate player spawn record {name}"
        );
        ensure!(
            record.data.len() == N * 4,
            "Invalid player spawn record width {name}"
        );
        Ok(Some(std::array::from_fn(|index| {
            f32::from_bits(u32::from_be_bytes(
                record.data[index * 4..index * 4 + 4].try_into().unwrap(),
            ))
        })))
    }
    let Some(position) = floats(&records, "PLAYERSPAWNLOC")? else {
        return Ok(None);
    };
    let Some(rotation) = floats(&records, "PLAYERSPAWNROT")? else {
        return Ok(None);
    };
    let Some([scale]) = floats(&records, "PLAYERSPAWNSCALE")? else {
        return Ok(None);
    };
    Ok(Some(Transform {
        rotation,
        position,
        scale,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transform() -> Transform {
        Transform {
            rotation: [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            position: [3., -4., 5.],
            scale: 2.,
        }
    }
    #[test]
    fn manager_override_copies_unscaled_basis_and_forces_mickey_scale() -> Result<()> {
        let live = transform();
        let mut state = State::default();
        assert_eq!(state.selected(live), live);
        state.set(live.rotation, live.position);
        let saved = state.selected(live);
        assert_eq!(saved.rotation, live.rotation);
        assert_eq!(saved.position, live.position);
        assert_eq!(saved.scale.to_bits(), 0x3dcccccd);
        state.clear();
        assert_eq!(state.selected(live), live);
        Ok(())
    }
    #[test]
    fn mick_records_preserve_float_bits_and_restore_through_native_setter() -> Result<()> {
        let mut source = transform();
        source.position[1] = -0.;
        let mut bytes = Vec::new();
        append_records(&mut bytes, source)?;
        let decoded = saved_transform(&bytes)?.unwrap();
        assert_eq!(decoded.position[1].to_bits(), 0x80000000);
        assert_eq!(decoded.scale, 2.);
        let mut state = State::default();
        assert!(state.restore(&bytes)?);
        assert_eq!(
            state.override_transform.unwrap().scale.to_bits(),
            0x3dcccccd
        );
        let previous = state;
        assert!(!state.restore(&[])?);
        assert_eq!(state, previous);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 save at BLOTWORX_TEST_SAVE"]
    fn original_mick_spawn_records_roundtrip_without_changing_other_player_data() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_SAVE"))?;
        let bytes = std::fs::read(path)?;
        let archive = super::super::save_game::Archive::parse(&bytes)?;
        let section = archive.section(*b"MICK")?;
        ensure!(section.version == 3, "Unsupported MICK version");
        let source = saved_transform(&section.payload)?
            .ok_or_else(|| anyhow::anyhow!("Original spawn fields missing"))?;
        let mut encoded = Vec::new();
        append_records(&mut encoded, source)?;
        let original = super::super::save_game::records(&section.payload)?;
        let rewritten = super::super::save_game::records(&encoded)?;
        for (left, right) in original.iter().take(3).zip(&rewritten) {
            assert_eq!(left.name, right.name);
            assert_eq!(left.data, right.data);
        }
        let mut state = State::default();
        assert!(state.restore(&section.payload)?);
        assert_eq!(state.override_transform.unwrap().position, source.position);
        Ok(())
    }
}
