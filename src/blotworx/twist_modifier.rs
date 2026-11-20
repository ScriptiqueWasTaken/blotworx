//! Twist pose operation recovered from DOL 0x80211164. Quaternion arithmetic
//! still needs captured-pose comparison; native transcendental parity is pending.
use super::{behavior::Document, behavior_state::State, variable_bindings::Scalar};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::{ffi_anim::FfiTransform, hkx_file::HkxSkeleton};
fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
pub fn modify(
    graph: &Document,
    state: &State,
    id: &str,
    properties: Option<&[u32]>,
    rig: &HkxSkeleton,
    tracks: &mut Vec<Option<FfiTransform>>,
) -> Result<()> {
    let f = &graph
        .objects
        .get(id)
        .context("Missing twist modifier")?
        .fields;
    if !f["enable"].as_bool().context("Missing twist enable")? {
        return Ok(());
    }
    let index = |member: &str| -> Result<i32> {
        match state.bindings.read(id, member, state.words(), properties)? {
            Some(Scalar::Integer(v)) => Ok(v),
            None => Ok(f[member]
                .as_i64()
                .context("Missing twist bone")?
                .try_into()?),
            _ => anyhow::bail!("Invalid twist bone binding"),
        }
    };
    let start = index("startBoneIndex")?;
    let end = index("endBoneIndex")?;
    if start < 0 || end < 0 {
        return Ok(());
    }
    let (start, end) = (start as usize, end as usize);
    ensure!(
        start < rig.bones.len() && end < rig.bones.len(),
        "Twist bone exceeds skeleton"
    );
    let mut chain = vec![];
    let mut bone = end;
    while bone != start {
        ensure!(!chain.contains(&bone), "Cyclic twist bone hierarchy");
        chain.push(bone);
        let parent = rig.bones[bone].parent_index;
        ensure!(parent >= 0, "Twist start is not an ancestor of its end");
        bone = parent as usize;
        ensure!(bone < rig.bones.len(), "Invalid twist parent");
    }
    chain.push(start);
    chain.reverse();
    let angle = state.real_member(graph, id, "twistAngle")?;
    let method = f["setAngleMethod"]
        .as_u64()
        .context("Missing twist distribution")?;
    ensure!(method <= 1, "Unknown twist distribution");
    let additive = f["isAdditive"].as_bool().context("Missing twist mode")?;
    let axis: Vec<f32> = serde_json::from_value(f["axisOfRotation"].clone())?;
    ensure!(
        axis.len() == 4 && axis.iter().all(|x| x.is_finite()) && angle.is_finite(),
        "Invalid twist axis/angle"
    );
    // Native converts the generator output to a complete local pose first.
    let resolved = super::pose_blend::resolve(rig, tracks)?;
    let mut pose: Vec<_> = resolved
        .iter()
        .map(|b| FfiTransform {
            translation: b.local_translation,
            rotation: b.local_rotation,
            scale: b.local_scale,
        })
        .collect();
    let mut ancestors = vec![];
    let mut parent = rig.bones[start].parent_index;
    while parent >= 0 {
        let i = parent as usize;
        ensure!(
            i < rig.bones.len() && !ancestors.contains(&i),
            "Invalid twist ancestor hierarchy"
        );
        ancestors.push(i);
        parent = rig.bones[i].parent_index;
    }
    let mut accumulated = [0., 0., 0., 1.];
    for &i in ancestors.iter().rev() {
        accumulated = multiply(accumulated, pose[i].rotation);
    }
    let mut per_bone = (angle * 0.01745329238474369_f32) / chain.len() as f32;
    if method == 1 {
        per_bone *= 2. / chain.len() as f32;
    }
    for (n, &i) in chain.iter().enumerate() {
        if method == 1 {
            per_bone /= n as f32 + 0.5;
        }
        let inverse = [
            -accumulated[0],
            -accumulated[1],
            -accumulated[2],
            accumulated[3],
        ];
        let local = multiply(
            multiply(inverse, [axis[0], axis[1], axis[2], 0.]),
            accumulated,
        );
        let length = (local[0] * local[0] + local[1] * local[1] + local[2] * local[2]).sqrt();
        ensure!(length > 0. && length.is_finite(), "Degenerate twist axis");
        let (sin, cos) = (per_bone * 0.5).sin_cos();
        let rotation = [
            local[0] / length * sin,
            local[1] / length * sin,
            local[2] / length * sin,
            cos,
        ];
        pose[i].rotation = if additive {
            multiply(rotation, pose[i].rotation)
        } else {
            rotation
        };
        accumulated = multiply(accumulated, pose[i].rotation);
    }
    *tracks = pose.into_iter().map(Some).collect();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires local original graph for modifier definitions"]
    fn additive_and_replacement_rotate_the_selected_bone() -> Result<()> {
        let mut graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let id = "2:61136";
        let f = &mut graph.objects.get_mut(id).unwrap().fields;
        f["twistAngle"] = serde_json::json!(90.);
        f["axisOfRotation"] = serde_json::json!([0., 0., 1., 0.]);
        let state = State::new(&graph)?;
        let rig = HkxSkeleton {
            name: "single".into(),
            bones: vec![Default::default()],
        };
        let identity = FfiTransform {
            translation: [2., 3., 4.],
            rotation: [0., 0., 0., 1.],
            scale: [1.; 3],
        };
        let mut pose = vec![Some(identity)];
        modify(&graph, &state, id, None, &rig, &mut pose)?;
        let result = pose[0].unwrap();
        assert_eq!(result.translation, identity.translation);
        assert!((result.rotation[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((result.rotation[3] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        graph.objects.get_mut(id).unwrap().fields["isAdditive"] = serde_json::json!(false);
        modify(&graph, &state, id, None, &rig, &mut pose)?;
        for (a, b) in pose[0].unwrap().rotation.iter().zip(result.rotation) {
            assert!((a - b).abs() < 1e-6);
        }
        Ok(())
    }
}
