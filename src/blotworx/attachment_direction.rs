//! Directional attachment basis REL35524c..355360; only XYZ survive23b884.
use super::{actor_bone_query::Pose, native_matrix::Matrix};
use anyhow::Result;
/// `up` is the current source global at REL11140, not an invented Y axis.
/// Pose acquisition/second-bone selection and NiNode fallback are separate.
pub fn basis(bone: Pose, other: Pose, up: [f32; 3]) -> Result<Matrix> {
    let delta: [f32; 3] = std::array::from_fn(|i| bone[i] - other[i]);
    let length = delta[2].mul_add(delta[2], delta[0] * delta[0]) + delta[1] * delta[1];
    let factor = super::native_hk_transform::inverse_root(length)?;
    let d = delta.map(|v| v * factor);
    // Source paired shuffles: first column d cross up. Preserve the fused
    // subtract and final negation boundaries, rather than reorder products.
    let dz = d[2];
    let first_z = -(d[1].mul_add(up[0], -(up[1] * d[0])));
    let first_y = -(d[0].mul_add(up[2], -(up[0] * dz)));
    let first_x = d[1].mul_add(up[2], -(up[1] * dz));
    // Second column first cross d, without a second normalization pass.
    let second_z = -(first_y.mul_add(d[0], -(d[1] * first_x)));
    let second_y = -(first_x.mul_add(dz, -(d[0] * first_z)));
    let second_x = first_y.mul_add(dz, -(d[1] * first_z));
    //23b884 maps the three padded columns into row-major NiMatrix3.
    Ok([
        first_x, second_x, d[0], first_y, second_y, d[1], first_z, second_z, d[2],
    ])
}
pub fn apply(
    bone: Pose,
    other: Pose,
    up: [f32; 3],
    transform: &mut super::native_transform_record::Transform,
) -> Result<()> {
    transform[..9].copy_from_slice(&basis(bone, other, up)?);
    Ok(())
}
/// Actor branch354fe0..355004, then35524c. The query's false result still
/// supplies the source default pose; do not turn it into a skipped update.
pub fn actor<G: FnMut() -> Result<usize>>(
    bone: Pose,
    index: i32,
    query: &mut super::actor_bone_query::Owned<'_, G>,
    mut up: impl FnMut() -> Result<[f32; 3]>,
    transform: &mut super::native_transform_record::Transform,
) -> Result<()> {
    let parent = query.parent_index(index)?;
    let mut other = [0.; 12];
    let _ = super::actor_bone_query::query(query, parent, false, &mut other)?;
    apply(bone, other, up()?, transform)
}
/// Non-actor branch35502c..355248: parent-space inverse times the NiNode
/// parent's current world transform, then23bb8c's unnormalized Qs conversion.
pub fn node_pose(
    parent: super::native_transform_record::Transform,
    node_parent: super::native_transform_record::Transform,
) -> Result<Pose> {
    let inverse = super::native_transform_record::inverse(&parent);
    let (position, rotation) = super::native_transform_record::world_pose(
        &inverse,
        node_parent[9..12].try_into().unwrap(),
        &node_parent[..9].try_into().unwrap(),
    );
    let padded = super::native_hk_transform::from_entity(rotation, position);
    let quaternion = super::native_hk_transform::rotation_quaternion_raw(&padded)?;
    let scale = inverse[12] * node_parent[12];
    Ok([
        position[0],
        position[1],
        position[2],
        0.,
        quaternion[0],
        quaternion[1],
        quaternion[2],
        quaternion[3],
        scale,
        scale,
        scale,
        0.,
    ])
}
#[cfg(test)]
mod node_tests {
    use super::*;
    #[test]
    fn attachment_direction_node_pose_retains_relative_scale_and_zero_padding() -> Result<()> {
        let parent = [1., 0., 0., 0., 1., 0., 0., 0., 1., 10., 20., 30., 2.];
        let node = [1., 0., 0., 0., 1., 0., 0., 0., 1., 12., 24., 36., 6.];
        let result = node_pose(parent, node)?;
        assert_eq!(&result[..4], &[1., 2., 3., 0.]);
        assert!(result[4..7].iter().all(|v| v.abs() < 0.000001));
        assert!((result[7] - 1.).abs() < 0.000001);
        assert_eq!(&result[8..], &[3., 3., 3., 0.]);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attachment_direction_actor_uses_fresh_parent_and_pose_queries() -> Result<()> {
        let identity = [0., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1.];
        let mut bone = identity;
        bone[2] = 2.;
        let skeletons = std::collections::BTreeMap::from([(
            10,
            super::super::actor_bone_query::Skeleton {
                bone_count: 2,
                parents: vec![-1, 0],
                reference: vec![identity, bone],
            },
        )]);
        let mut pose = super::super::actor_pose_storage::Storage {
            parents: vec![-1, 0],
            local: vec![identity, bone],
            model: vec![identity, bone],
            flags: vec![0, 0],
        };
        let calls = std::cell::Cell::new(0);
        let mut query = super::super::actor_bone_query::Owned {
            pose: &mut pose,
            skeletons: &skeletons,
            skeleton_getter: || {
                calls.set(calls.get() + 1);
                Ok(10)
            },
        };
        let mut transform = [99.; 13];
        actor(
            bone,
            1,
            &mut query,
            || {
                assert_eq!(calls.get(), 2);
                Ok([0., 1., 0.])
            },
            &mut transform,
        )?;
        assert!((transform[0] + 1.).abs() < 0.00001);
        assert_eq!(&transform[9..], &[99.; 4]);
        assert_eq!(query.parent_index(-7)?, -7);
        assert_eq!(calls.get(), 2);
        // Root parent -1 goes through the default-pose query with no getter.
        actor(
            bone,
            0,
            &mut query,
            || {
                assert_eq!(calls.get(), 3);
                Ok([0., 1., 0.])
            },
            &mut transform,
        )?;
        assert_eq!(calls.get(), 3);
        assert_eq!(query.pose.model, [identity, bone]);
        Ok(())
    }
    #[test]
    fn attachment_direction_keeps_source_basis_magnitude_and_pose_fields() -> Result<()> {
        let mut bone = [0.; 12];
        bone[2] = 2.;
        let other = [0.; 12];
        let matrix = basis(bone, other, [0., 2., 0.])?;
        assert!((matrix[0] + 2.).abs() < 0.00001);
        assert!((matrix[4] - 2.).abs() < 0.00001);
        assert!((matrix[8] - 1.).abs() < 0.00001);
        assert!(matrix
            .iter()
            .enumerate()
            .filter(|(i, _)| ![0, 4, 8].contains(i))
            .all(|(_, v)| v.abs() < 0.00001));
        let mut transform = [99.; 13];
        apply(bone, other, [0., 2., 0.], &mut transform)?;
        assert_eq!(&transform[9..], &[99.; 4]);
        assert!(apply(other, other, [0., 1., 0.], &mut transform).is_err());
        assert_eq!(&transform[..9], &matrix);
        Ok(())
    }
}
