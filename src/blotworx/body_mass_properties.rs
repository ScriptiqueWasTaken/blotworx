//! Loader mass/inertia stage, separate from serialized motion layout.
//! Original reciprocal/clamp/write order is observable; dividing inverse
//! inertia directly by a mass ratio is not equivalent.
use anyhow::{ensure, Result};
#[derive(Clone, Copy)]
pub enum CharacterZero {
    /// Existing Wii compatibility behavior, retained independently of PC.
    Reciprocal,
    /// Havok 2012 character getInertiaLocal's explicit zero case (0x5f7ffff0).
    Havok2012,
}
pub enum Change {
    Unchanged,
    Publish(f32),
    Updated([f32; 4]),
}
pub fn mass(inverse: f32) -> f32 {
    if inverse == 0. {
        0.
    } else {
        1. / inverse
    }
}
pub fn initialize(
    mode: u8,
    inverse: [f32; 4],
    requested: f32,
    character_zero: CharacterZero,
) -> Result<Change> {
    ensure!(requested.is_finite(), "Nonfinite mass override");
    ensure!((1..=7).contains(&mode), "Invalid motion type");
    if requested <= 0. {
        let mass = mass(inverse[3]);
        ensure!(mass.is_finite(), "Nonfinite stored mass");
        return Ok(if mass > 0. {
            Change::Publish(mass)
        } else {
            Change::Unchanged
        });
    }
    if matches!(mode, 4 | 5) {
        return Ok(Change::Unchanged);
    }
    ensure!(
        matches!(mode, 2 | 3 | 6 | 7),
        "Unresolved dynamic inertia model {mode}"
    );
    let old_mass = mass(inverse[3]);
    ensure!(
        old_mass > 0. && old_mass.is_finite() && inverse.iter().all(|v| v.is_finite()),
        "Invalid stored mass/inertia for override"
    );
    let ratio = requested / old_mass;
    let epsilon = f32::from_bits(0x34000000);
    let diagonal = match mode {
        3 | 6 => [inverse[0], inverse[1], inverse[2]].map(|v| 1. / v.max(epsilon)),
        2 => [1. / inverse[0].max(epsilon); 3],
        7 => {
            [if inverse[0] == 0. && matches!(character_zero, CharacterZero::Havok2012) {
                f32::from_bits(0x5f7ffff0)
            } else {
                1. / inverse[0]
            }; 3]
        }
        _ => unreachable!(),
    }
    .map(|v| v * ratio);
    let result = if matches!(mode, 3 | 6) {
        diagonal.map(|v| 1. / v)
    } else {
        let largest = diagonal[0].max(diagonal[1]).max(diagonal[2]);
        [if largest > 0. { 1. / largest } else { 0. }; 3]
    };
    let inverse_mass = 1. / requested;
    ensure!(
        result.iter().all(|v| v.is_finite()) && inverse_mass.is_finite(),
        "Mass override produces nonfinite inverse inertia"
    );
    Ok(Change::Updated([
        result[0],
        result[1],
        result[2],
        inverse_mass,
    ]))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mass_stage_retains_publication_and_fixed_body_behavior() -> Result<()> {
        assert!(matches!(
            initialize(3, [2., 4., 8., 0.5], 0., CharacterZero::Havok2012)?,
            Change::Publish(2.)
        ));
        for mode in [4, 5] {
            assert!(matches!(
                initialize(mode, [0.; 4], 4., CharacterZero::Havok2012)?,
                Change::Unchanged
            ));
        }
        let Change::Updated(values) =
            initialize(3, [2., 4., 8., 0.5], 4., CharacterZero::Havok2012)?
        else {
            panic!()
        };
        assert_eq!(values, [1., 2., 4., 0.25]);
        Ok(())
    }
    #[test]
    fn character_zero_uses_its_version_specific_getter() -> Result<()> {
        let Change::Updated(pc) = initialize(7, [0., 0., 0., 1.], 1., CharacterZero::Havok2012)?
        else {
            panic!()
        };
        let Change::Updated(old) = initialize(7, [0., 0., 0., 1.], 1., CharacterZero::Reciprocal)?
        else {
            panic!()
        };
        assert_eq!(old, [0., 0., 0., 1.]);
        assert_eq!(&pc[..3], &[1. / f32::from_bits(0x5f7ffff0); 3]);
        assert!(initialize(1, [1.; 4], 1., CharacterZero::Havok2012).is_err());
        assert!(initialize(3, [1.; 4], f32::NAN, CharacterZero::Havok2012).is_err());
        Ok(())
    }
}
