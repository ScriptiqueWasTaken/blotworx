//! EM1 character listener REL2396bc..2399a8. Produces the raw factor vector
//! passed to DOL802a2c50, not a post-step impulse or a Box3D approximation.
//! The owner supplies current contact normals, body order and fallback factors.
use anyhow::{ensure, Result};

pub fn mass_factors(
    other_motion: u8,
    character_is_body_a: bool,
    normals: &[[f32; 4]],
    velocity: [f32; 4],
    character_inverse_mass: f32,
    limit: f32,
    fallback: [f32; 4],
) -> Result<Option<[f32; 4]>> {
    ensure!(
        (1..=7).contains(&other_motion),
        "Unresolved contacted motion type"
    );
    ensure!(
        velocity
            .iter()
            .chain(fallback.iter())
            .all(|v| v.is_finite())
            && character_inverse_mass.is_finite()
            && character_inverse_mass >= 0.
            && limit.is_finite()
            && normals.iter().flatten().all(|v| v.is_finite()),
        "Invalid character contact response input"
    );
    // Fixed, keyframed and another character do not receive this modifier.
    if matches!(other_motion, 4 | 5 | 7) {
        return Ok(None);
    }
    if limit <= 0. {
        return Ok(Some(fallback));
    }
    let mut sum = [0f32; 4];
    for normal in normals {
        for i in 0..4 {
            sum[i] += normal[i];
        }
    }
    let sign = if character_is_body_a { -1. } else { 1. };
    for v in &mut sum {
        *v *= sign;
    }
    // DOL802bbfd0 returns reciprocal motion+cc, except zero returns zero.
    let mass = if character_inverse_mass == 0. {
        0.
    } else {
        1. / character_inverse_mass
    };
    let squared = sum[2].mul_add(sum[2], sum[0] * sum[0]) + sum[1] * sum[1];
    let projected = if squared > 0. {
        let dot = sum[2].mul_add(velocity[2], sum[0] * velocity[0]) + sum[1] * velocity[1];
        (mass * dot) / squared
    } else {
        let squared =
            velocity[2].mul_add(velocity[2], velocity[0] * velocity[0]) + velocity[1] * velocity[1];
        let estimate = super::pose_blend::estimate(squared);
        let correction = (-squared).mul_add(estimate * estimate, 1.);
        let refined = correction.mul_add(0.5 * estimate, estimate);
        // Native fsel chooses zero for unordered refinement, and zero input.
        let reciprocal = if refined >= 0. { refined } else { 0. };
        mass * if squared >= 0. {
            squared * reciprocal
        } else {
            0.
        }
    };
    // Native constants REL4bd6f8; XYZ must not be collapsed to one factor.
    let factors = [
        1.,
        1.,
        0.001,
        if projected > limit {
            projected / limit
        } else {
            0.001
        },
    ];
    ensure!(
        factors.iter().all(|v| v.is_finite()),
        "Character contact modifier overflow"
    );
    Ok(Some(factors))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_listener_body_order_and_projection() -> Result<()> {
        // Exact binary arithmetic case: mass100, speed4, summed normal2,
        // projected100*8/4=200 and limit50 gives W4.
        let normals = [[1., 0., 0., 0.], [1., 0., 0., 0.]];
        assert_eq!(
            mass_factors(3, false, &normals, [4., 0., 0., 0.], 0.01, 50., [0.; 4])?,
            Some([1., 1., 0.001, 4.])
        );
        assert_eq!(
            mass_factors(3, true, &normals, [-4., 0., 0., 0.], 0.01, 50., [0.; 4])?,
            Some([1., 1., 0.001, 4.])
        );
        assert_eq!(
            mass_factors(3, true, &normals, [4., 0., 0., 0.], 0.01, 50., [0.; 4])?,
            Some([1., 1., 0.001, 0.001])
        );
        Ok(())
    }
    #[test]
    fn original_listener_exclusions_and_current_fallback() -> Result<()> {
        for mode in [4, 5, 7] {
            assert_eq!(
                mass_factors(mode, false, &[], [0.; 4], 0.01, 50., [2.; 4])?,
                None
            );
        }
        assert_eq!(
            mass_factors(3, false, &[], [0.; 4], 0.01, 0., [2., 3., 4., 5.])?,
            Some([2., 3., 4., 5.])
        );
        assert_eq!(
            mass_factors(3, false, &[], [0.; 4], 0.01, 50., [0.; 4])?,
            Some([1., 1., 0.001, 0.001])
        );
        assert!(mass_factors(3, false, &[], [f32::NAN; 4], 0.01, 50., [0.; 4]).is_err());
        Ok(())
    }
}
