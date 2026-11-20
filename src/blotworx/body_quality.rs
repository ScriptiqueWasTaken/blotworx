//! Collision dispatcher quality-index table+1ab0, DOL80265ee0..802661e4.
//! Body quality installation/overrides remain world-owned, not imported defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub values: [[i8; 10]; 10],
}
/// Rigid-body constructor DOL802b6bf0..802b6c90: cinfo+b7 is signed
/// quality (-1 default); body+e8 actual motion and cinfo+b4 requested motion
/// are distinct. This is not a blanket fixup for every loaded/saved body.
pub fn constructor_quality(provided: u8, actual_motion: u8, requested_motion: u8) -> u8 {
    if provided != 0xff {
        provided
    } else if actual_motion == 5 {
        0
    } else if requested_motion == 4 {
        1
    } else {
        2
    }
}
impl Default for Table {
    fn default() -> Self {
        Self {
            values: [
                [0, 0, 1, 2, 4, 5, 4, 1, 6, 3],
                [0, 0, 1, 1, 3, 3, 3, 1, 3, 3],
                [1, 1, 1, 1, 1, 1, 3, 1, 1, 1],
                [2, 1, 1, 1, 1, 1, 3, 1, 1, 1],
                [4, 3, 1, 1, 1, 3, 3, 1, 3, 3],
                [5, 3, 1, 1, 3, 3, 3, 1, 3, 3],
                [4, 3, 3, 3, 3, 3, 3, 1, 3, 3],
                [1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
                [6, 3, 1, 1, 3, 3, 3, 1, 3, 3],
                [3, 3, 1, 1, 3, 3, 3, 1, 3, 3],
            ],
        }
    }
}
impl Table {
    /// Native addresses use signed type bytes. Invalid entries would index
    /// outside this table; diagnose them rather than clamp to a valid quality.
    pub fn get(&self, types: [i8; 2]) -> anyhow::Result<i8> {
        anyhow::ensure!(
            types.iter().all(|v| (0..10).contains(v)),
            "Collision quality outside native table: {types:?}"
        );
        Ok(self.values[types[0] as usize][types[1] as usize])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constructor_default_quality_preserves_explicit_bytes_and_motion_precedence() {
        for byte in 0..=255u8 {
            if byte != 0xff {
                assert_eq!(constructor_quality(byte, 5, 4), byte);
            }
        }
        assert_eq!(constructor_quality(0xff, 5, 4), 0);
        assert_eq!(constructor_quality(0xff, 5, 3), 0);
        assert_eq!(constructor_quality(0xff, 4, 4), 1);
        assert_eq!(constructor_quality(0xff, 3, 4), 1);
        assert_eq!(constructor_quality(0xff, 4, 3), 2);
        assert_eq!(constructor_quality(0xff, 3, 3), 2);
        assert_eq!(constructor_quality(0xfe, 5, 4), 0xfe);
    }
    #[test]
    fn quality_lookup_preserves_indices_symmetry_and_invalid_type_errors() -> anyhow::Result<()> {
        let table = Table::default();
        for i in 0..10 {
            for j in 0..10 {
                assert_eq!(table.get([i, j])?, table.get([j, i])?);
            }
        }
        assert_eq!(table.get([0, 0])?, 0);
        assert_eq!(table.get([0, 1])?, 0);
        assert_eq!(table.get([0, 8])?, 6);
        assert_eq!(table.get([0, 5])?, 5);
        for i in 0..10 {
            assert_eq!(table.get([7, i])?, 1);
        }
        assert!(table.get([-1, 0]).is_err());
        assert!(table.get([0, 10]).is_err());
        let mut modified = table.clone();
        modified.values[2][3] = -1;
        assert_eq!(modified.get([2, 3])?, -1);
        Ok(())
    }
}
