//! Owned motion replacement state. World locks, islands, callbacks and shape
//! refresh remain outside this object; this is not a world request executor.
use super::native_motion_record::MotionRecord;
use anyhow::{ensure, Result};

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Unchanged,
    NoStoredDynamicMotion,
    Keyframed,
    RestoredDynamic,
}

/// Shared once per decoded HKX body, even when it has many shape parts.
#[derive(Debug, PartialEq, Eq)]
pub struct ImportedMotion {
    pub current: MotionRecord,
    pub quality: u8,
    pub saved: Option<MotionRecord>,
}
impl ImportedMotion {
    pub fn validate(&self) -> Result<()> {
        validate(&self.current, self.saved.as_ref())
    }
    pub fn instantiate(&self) -> Result<BodyMotion> {
        // Native clone 802b6f48 builds cinfo via 802b6d98: requested motion
        // comes from body+e8 and quality from body+2a. Construction resolves
        // the -1 quality sentinel before copying the source motion record.
        // Only the fixed/non-fixed distinction matters for default quality;
        // resolving generic motion request 1 cannot change that distinction.
        self.validate()?;
        let motion = self.current.motion_type();
        let quality = super::body_quality::constructor_quality(self.quality, motion, motion);
        BodyMotion::new(self.current.clone(), quality, self.saved.clone())
    }
}

fn validate(current: &MotionRecord, saved: Option<&MotionRecord>) -> Result<()> {
    ensure!(
        (1..=7).contains(&current.motion_type()),
        "Invalid motion type"
    );
    if let Some(saved) = saved {
        ensure!(
            matches!(current.motion_type(), 4 | 5),
            "Dynamic motion cannot own a stored motion"
        );
        ensure!(
            matches!(saved.motion_type(), 1 | 2 | 3 | 6 | 7),
            "Stored motion is not dynamic"
        );
    } else if matches!(current.motion_type(), 4 | 5) {
        ensure!(
            current.0[0x118..0x11c] == [0; 4],
            "Unresolved stored-motion reference"
        );
    }
    Ok(())
}

pub struct BodyMotion {
    pub current: MotionRecord,
    pub quality: u8,
    saved: Option<Box<MotionRecord>>,
}
impl BodyMotion {
    /// The importer resolves the saved-motion reference before constructing
    /// this owner. Raw Wii pointer bytes are never used as native addresses.
    pub fn new(current: MotionRecord, quality: u8, saved: Option<MotionRecord>) -> Result<Self> {
        validate(&current, saved.as_ref())?;
        Ok(Self {
            current,
            quality,
            saved: saved.map(Box::new),
        })
    }
    pub fn has_stored_dynamic(&self) -> bool {
        matches!(self.current.motion_type(), 4 | 5) && self.saved.is_some()
    }
    /// Dispatch against the current mode, including while a conversion is pending.
    /// Fixed motion's original virtual setters (802b928c/802b9290) are no-ops;
    /// the other motion tables use the four-lane copies at 802bc058/802bc06c.
    pub fn set_linear_velocity(&mut self, velocity: [f32; 4]) -> Result<()> {
        ensure!(
            (1..=7).contains(&self.current.motion_type()),
            "Invalid motion type"
        );
        if self.current.motion_type() != 5 {
            self.current.set_linear_velocity(velocity);
        }
        Ok(())
    }
    pub fn set_angular_velocity(&mut self, velocity: [f32; 4]) -> Result<()> {
        ensure!(
            (1..=7).contains(&self.current.motion_type()),
            "Invalid motion type"
        );
        if self.current.motion_type() != 5 {
            self.current.set_angular_velocity(velocity);
        }
        Ok(())
    }
    /// Apply the local record replacement only, after the world owner has
    /// admitted a request. Supports requests 1 and 4, and restoring an owned
    /// character snapshot with request7 (REL37b364/DOL802d7900). Other
    /// conversions fail explicitly until their inertia/fixed paths are ported.
    /// The caller must still execute the original surrounding world effects.
    pub fn replace(&mut self, requested: u8, gravity_factor_word: u32) -> Result<Change> {
        ensure!(
            matches!(requested, 1 | 4 | 7),
            "Motion request {requested} conversion is not implemented"
        );
        let old_type = self.current.motion_type();
        if requested == old_type {
            return Ok(Change::Unchanged);
        }
        if requested == 1 || requested == 7 {
            if requested == 7 {
                ensure!(matches!(old_type,4|5) && self.saved.as_ref().is_some_and(|record|record.motion_type()==7),
                    "Character motion restore requires a retained character snapshot; constructing another inertia model is not implemented");
            }
            if !matches!(old_type, 4 | 5) {
                return Ok(Change::Unchanged);
            }
            let Some(saved) = &self.saved else {
                return Ok(Change::NoStoredDynamicMotion);
            };
            let restored = self.current.restore_dynamic(saved)?;
            self.current = restored.motion;
            self.quality = restored.collision_quality;
            self.saved = None;
            return Ok(Change::RestoredDynamic);
        }
        let mut next = self.current.prepare_keyframed(gravity_factor_word)?;
        if old_type == 5 {
            // Fixed -> keyframed reuses the existing saved motion and quality.
            next.0[0x118..0x11e].copy_from_slice(&self.current.0[0x118..0x11e]);
        } else {
            let mut saved = self.current.clone();
            // The original allocates a 0x120-byte referenced motion snapshot.
            saved.0[4..6].copy_from_slice(&0x120u16.to_be_bytes());
            saved.0[6..8].copy_from_slice(&1u16.to_be_bytes());
            self.saved = Some(Box::new(saved));
            next.0[0x11c..0x11e].copy_from_slice(&(self.quality as i8 as i16).to_be_bytes());
            // Saved ownership is the Box above, not an invented Wii pointer.
        }
        self.current = next;
        self.quality = 1;
        Ok(Change::Keyframed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dynamic() -> MotionRecord {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_motion_transfer.json"
        ))
        .unwrap();
        let sample = &fixture["samples"][1]["source"];
        let bytes: Vec<u8> = sample
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|v| (v.as_u64().unwrap() as u32).to_be_bytes())
            .collect();
        let result = MotionRecord(bytes.try_into().unwrap());
        assert_eq!(result.motion_type(), 2);
        result
    }
    #[test]
    fn imported_clone_resolves_quality_without_mutating_source_or_saved_motion() {
        for (motion, expected) in [(2, 2), (4, 1), (5, 0)] {
            let mut current = dynamic();
            current.0[8] = motion;
            if matches!(motion, 4 | 5) {
                current.0[0x118..0x11c].fill(0);
            }
            let imported = ImportedMotion {
                current: current.clone(),
                quality: 0xff,
                saved: matches!(motion, 4 | 5).then(dynamic),
            };
            let mut first = imported.instantiate().unwrap();
            let second = imported.instantiate().unwrap();
            assert_eq!(first.quality, expected);
            assert_eq!(second.quality, expected);
            assert_eq!(first.current, current);
            assert_eq!(imported.quality, 0xff);
            first.current.0[0x40..0x44].copy_from_slice(&123f32.to_be_bytes());
            assert_eq!(second.current, current);
            assert_eq!(imported.current, current);
            if let Some(saved) = first.saved.as_mut() {
                saved.0[0x40..0x44].copy_from_slice(&456f32.to_be_bytes());
                assert_eq!(second.saved.as_deref(), imported.saved.as_ref());
            }
            for quality in [0, 1, 9, 0xfe] {
                let source = ImportedMotion {
                    current: current.clone(),
                    quality,
                    saved: imported.saved.clone(),
                };
                assert_eq!(source.instantiate().unwrap().quality, quality);
            }
        }
    }
    #[test]
    fn round_trip_keeps_movement_and_one_saved_inertia_model() {
        let original = dynamic();
        let mut body = BodyMotion::new(original.clone(), 0x89, None).unwrap();
        assert_eq!(body.replace(4, 0x3f800000).unwrap(), Change::Keyframed);
        let saved_address = body.saved.as_deref().unwrap() as *const MotionRecord;
        assert_eq!(body.replace(4, 0x3f800000).unwrap(), Change::Unchanged);
        assert_eq!(
            body.saved.as_deref().unwrap() as *const MotionRecord,
            saved_address
        );
        // A later current pose must win over the pre-conversion snapshot.
        body.current.0[0x40..0x44].copy_from_slice(&123f32.to_be_bytes());
        body.current.0[0xd0..0xd4].copy_from_slice(&7f32.to_be_bytes());
        body.quality = 9; // Component override must not erase saved quality.
        assert_eq!(
            body.replace(1, 0x3f800000).unwrap(),
            Change::RestoredDynamic
        );
        assert!(!body.has_stored_dynamic());
        assert_eq!(body.quality, 0x89);
        assert_eq!(body.current.motion_type(), 2);
        assert_eq!(&body.current.0[0xc0..0xd0], &original.0[0xc0..0xd0]);
        assert_eq!(&body.current.0[0x40..0x44], &123f32.to_be_bytes());
        assert_eq!(&body.current.0[0xd0..0xd4], &7f32.to_be_bytes());
    }
    #[test]
    fn authored_keyframes_do_not_invent_dynamic_motion() {
        let record = dynamic().prepare_keyframed(0x3f800000).unwrap();
        let mut body = BodyMotion::new(record, 9, None).unwrap();
        let before = body.current.clone();
        assert_eq!(
            body.replace(1, 0x3f800000).unwrap(),
            Change::NoStoredDynamicMotion
        );
        assert_eq!(body.current, before);
        assert_eq!(body.quality, 9);
        assert!(body.replace(5, 0x3f800000).is_err());
        assert_eq!(body.current, before);
    }
    #[test]
    fn fixed_to_keyframed_retains_saved_owner_and_quality() {
        let saved = dynamic();
        let mut fixed = saved.prepare_keyframed(0x3f800000).unwrap();
        fixed.0[8] = 5;
        fixed.0[0x11c..0x11e].copy_from_slice(&[0, 6]);
        let mut body = BodyMotion::new(fixed, 0, Some(saved)).unwrap();
        let address = body.saved.as_deref().unwrap() as *const MotionRecord;
        assert_eq!(body.replace(4, 0x3f800000).unwrap(), Change::Keyframed);
        assert_eq!(
            body.saved.as_deref().unwrap() as *const MotionRecord,
            address
        );
        assert_eq!(
            body.replace(1, 0x3f800000).unwrap(),
            Change::RestoredDynamic
        );
        assert_eq!(body.quality, 6);
    }
    #[test]
    fn character_restore_keeps_kinematics_and_rejects_unowned_inertia_conversions() {
        let mut record = dynamic();
        record.0[8] = 7;
        let mut body = BodyMotion::new(record.clone(), 8, None).unwrap();
        assert_eq!(body.replace(7, 0x3f800000).unwrap(), Change::Unchanged);
        body.replace(4, 0x3f800000).unwrap();
        body.current.set_linear_velocity([3., 4., 5., 0.]);
        assert_eq!(
            body.replace(7, 0x3f800000).unwrap(),
            Change::RestoredDynamic
        );
        assert_eq!(body.current.motion_type(), 7);
        assert_eq!(body.quality, 8);
        assert_eq!(body.current.linear_velocity(), [3., 4., 5., 0.]);
        assert_eq!(&body.current.0[0xc0..0xd0], &record.0[0xc0..0xd0]);
        let mut other = BodyMotion::new(dynamic(), 2, None).unwrap();
        other.replace(4, 0x3f800000).unwrap();
        let before = other.current.clone();
        assert!(other.replace(7, 0x3f800000).is_err());
        assert_eq!(other.current, before);
    }
    #[test]
    fn velocity_setters_follow_current_mode_and_preserve_saved_motion() {
        let saved = dynamic();
        let mut fixed = saved.prepare_keyframed(0x3f800000).unwrap();
        fixed.0[8] = 5;
        let mut body = BodyMotion::new(fixed, 0, Some(saved.clone())).unwrap();
        let before = body.current.clone();
        let linear = [1., 2., 3., -0.];
        let angular = [4., 5., 6., 7.];
        body.set_linear_velocity(linear).unwrap();
        body.set_angular_velocity(angular).unwrap();
        assert_eq!(body.current, before);
        assert_eq!(body.saved.as_deref(), Some(&saved));
        body.replace(1, 0x3f800000).unwrap();
        body.set_linear_velocity(linear).unwrap();
        body.set_angular_velocity(angular).unwrap();
        assert_eq!(
            body.current.linear_velocity().map(f32::to_bits),
            linear.map(f32::to_bits)
        );
        assert_eq!(body.current.angular_velocity(), angular);
        body.replace(4, 0x3f800000).unwrap();
        let snapshot = body.saved.clone();
        body.set_linear_velocity(angular).unwrap();
        body.set_angular_velocity(linear).unwrap();
        assert_eq!(body.saved, snapshot);
        assert_eq!(body.current.linear_velocity(), angular);
        assert_eq!(
            body.current.angular_velocity().map(f32::to_bits),
            linear.map(f32::to_bits)
        );
    }
}
