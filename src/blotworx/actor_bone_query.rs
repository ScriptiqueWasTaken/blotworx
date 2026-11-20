//! Retail actor bone query, REL 0x2f2b5c. Qs records retain all four lanes.
use anyhow::{ensure, Context, Result};

pub type Pose = [f32; 12];
pub trait Backend {
    /// The skeleton getter is called again for reference-pose queries.
    fn skeleton(&mut self) -> Result<usize>;
    fn bone_count(&mut self, skeleton: usize) -> Result<i32>;
    fn flags(&mut self, bone: usize) -> Result<u32>;
    fn model_pose(&mut self, bone: usize) -> Result<Pose>;
    fn resolve_model_pose(&mut self, bone: usize) -> Result<Pose>;
    fn reference_pose(&mut self, skeleton: usize, bone: usize) -> Result<Pose>;
}

pub(super) const DEFAULT: Pose = [0., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1.];

pub struct Skeleton {
    pub bone_count: i32,
    pub parents: Vec<i16>,
    pub reference: Vec<Pose>,
}
/// The actor's animation-interface getter remains mandatory. It may select a
/// different skeleton on the second call; do not cache it across a query.
pub struct Owned<'a, G: FnMut() -> Result<usize>> {
    pub skeleton_getter: G,
    pub skeletons: &'a std::collections::BTreeMap<usize, Skeleton>,
    pub pose: &'a mut super::actor_pose_storage::Storage,
}
impl<G: FnMut() -> Result<usize>> Owned<'_, G> {
    /// REL2f2a24: negatives pass through; otherwise fresh interface getter
    /// followed by signed16 skeleton parent, without a bone-count query.
    pub fn parent_index(&mut self, bone: i32) -> Result<i32> {
        if bone < 0 {
            return Ok(bone);
        }
        let skeleton = (self.skeleton_getter)()?;
        Ok(i32::from(
            *self
                .skeletons
                .get(&skeleton)
                .context("Missing parent-index skeleton")?
                .parents
                .get(bone as usize)
                .context("Missing skeleton parent index")?,
        ))
    }
}
impl<G: FnMut() -> Result<usize>> Backend for Owned<'_, G> {
    fn skeleton(&mut self) -> Result<usize> {
        (self.skeleton_getter)()
    }
    fn bone_count(&mut self, s: usize) -> Result<i32> {
        Ok(self
            .skeletons
            .get(&s)
            .context("Missing actor query skeleton")?
            .bone_count)
    }
    fn flags(&mut self, b: usize) -> Result<u32> {
        self.pose
            .flags
            .get(b)
            .copied()
            .context("Missing actor bone flags")
    }
    fn model_pose(&mut self, b: usize) -> Result<Pose> {
        self.pose
            .model
            .get(b)
            .copied()
            .context("Missing actor model pose")
    }
    fn resolve_model_pose(&mut self, b: usize) -> Result<Pose> {
        self.pose.resolve(b, &mut super::actor_pose_compose::Native)
    }
    fn reference_pose(&mut self, s: usize, b: usize) -> Result<Pose> {
        self.skeletons
            .get(&s)
            .context("Missing actor reference skeleton")?
            .reference
            .get(b)
            .copied()
            .context("Missing actor reference pose")
    }
}

pub fn query(
    backend: &mut impl Backend,
    bone: i32,
    reference: bool,
    output: &mut Pose,
) -> Result<bool> {
    if bone < 0 {
        *output = DEFAULT;
        return Ok(false);
    }
    let skeleton = backend.skeleton()?;
    if bone >= backend.bone_count(skeleton)? {
        *output = DEFAULT;
        return Ok(false);
    }
    let bone = bone as usize;
    if reference {
        let skeleton = backend.skeleton()?;
        *output = backend.reference_pose(skeleton, bone)?;
        return Ok(true);
    }
    *output = if backend.flags(bone)? & 2 == 0 {
        backend.model_pose(bone)?
    } else {
        backend.resolve_model_pose(bone)?
    };
    let [x, y, z, w] = output[4..8].try_into().unwrap();
    let length: f32 = z.mul_add(z, x * x) + w.mul_add(w, y * y);
    // Special frsqrte/FPSCR cases still need a hardware reference; retain
    // the already-published pose and report them rather than invent a pose.
    ensure!(
        length.is_finite() && length > 0.,
        "Unsupported degenerate actor bone quaternion"
    );
    let estimate = super::pose_blend::estimate(length);
    let correction = -(length.mul_add(estimate * estimate, -1.));
    let factor = correction.mul_add(0.5 * estimate, estimate);
    let factor = if factor >= 0. { factor } else { 0. };
    for lane in &mut output[4..8] {
        *lane *= factor;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actor_bone_query_owned_resolves_live_pose_without_changing_reference() -> Result<()> {
        let identity = DEFAULT;
        let mut root = identity;
        root[..3].copy_from_slice(&[10., 20., 30.]);
        let mut child = identity;
        child[..3].copy_from_slice(&[1., 2., 3.]);
        let mut pose = super::super::actor_pose_storage::Storage {
            parents: vec![-1, 0],
            local: vec![root, child],
            model: vec![[99.; 12]; 2],
            flags: vec![2, 2],
        };
        let mut reference = identity;
        reference[0] = 123.;
        reference[7] = 2.;
        let skeletons = std::collections::BTreeMap::from([
            (
                10,
                Skeleton {
                    bone_count: 2,
                    parents: vec![-1, 0],
                    reference: vec![identity, identity],
                },
            ),
            (
                20,
                Skeleton {
                    bone_count: 2,
                    parents: vec![-1, 0],
                    reference: vec![identity, reference],
                },
            ),
        ]);
        let mut calls = 0;
        let mut owned = Owned {
            pose: &mut pose,
            skeletons: &skeletons,
            skeleton_getter: || {
                calls += 1;
                Ok(if calls == 3 { 20 } else { 10 })
            },
        };
        let mut output = [0.; 12];
        assert!(query(&mut owned, 1, false, &mut output)?);
        assert_eq!(&output[..3], &[11., 22., 33.]);
        assert_eq!(owned.pose.flags, [0, 0]);
        assert_eq!(owned.pose.local, [root, child]);
        let model = owned.pose.model.clone();
        assert!(query(&mut owned, 1, true, &mut output)?);
        assert_eq!(output, reference); // second getter selects skeleton20
        assert_eq!(owned.pose.model, model);
        assert_eq!(owned.skeletons[&20].reference[1], reference);
        Ok(())
    }
    struct Source {
        calls: Vec<&'static str>,
        dirty: bool,
    }
    impl Backend for Source {
        fn skeleton(&mut self) -> Result<usize> {
            self.calls.push("skeleton");
            Ok(self.calls.len())
        }
        fn bone_count(&mut self, _: usize) -> Result<i32> {
            self.calls.push("count");
            Ok(1)
        }
        fn flags(&mut self, _: usize) -> Result<u32> {
            self.calls.push("flags");
            Ok(if self.dirty { 2 } else { 0 })
        }
        fn model_pose(&mut self, _: usize) -> Result<Pose> {
            self.calls.push("model");
            Ok([2.; 12])
        }
        fn resolve_model_pose(&mut self, _: usize) -> Result<Pose> {
            self.calls.push("resolve");
            Ok([4.; 12])
        }
        fn reference_pose(&mut self, _: usize, _: usize) -> Result<Pose> {
            self.calls.push("reference");
            Ok([8.; 12])
        }
    }
    #[test]
    fn actor_bone_query_preserves_live_reference_and_invalid_paths() -> Result<()> {
        let mut source = Source {
            calls: vec![],
            dirty: true,
        };
        let mut output = [9.; 12];
        assert!(!query(&mut source, -1, false, &mut output)?);
        assert_eq!(output, DEFAULT);
        assert!(source.calls.is_empty());
        assert!(!query(&mut source, 1, false, &mut output)?);
        assert_eq!(source.calls, ["skeleton", "count"]);
        source.calls.clear();
        assert!(query(&mut source, 0, false, &mut output)?);
        assert_eq!(source.calls, ["skeleton", "count", "flags", "resolve"]);
        assert_eq!(&output[..4], &[4.; 4]);
        assert_eq!(&output[8..], &[4.; 4]);
        assert!(output[4..8].iter().all(|v| (*v - 0.5).abs() < 0.000001));
        source.calls.clear();
        assert!(query(&mut source, 0, true, &mut output)?);
        assert_eq!(source.calls, ["skeleton", "count", "skeleton", "reference"]);
        assert_eq!(output, [8.; 12]);
        source.calls.clear();
        source.dirty = false;
        query(&mut source, 0, false, &mut output)?;
        assert_eq!(source.calls, ["skeleton", "count", "flags", "model"]);
        Ok(())
    }
}
