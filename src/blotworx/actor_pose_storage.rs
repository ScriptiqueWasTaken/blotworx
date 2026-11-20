//! Owned hkaPose model-space resolution, DOL 0x801e6a9c.
//! Composition arithmetic is required separately; affine skin matrices are
//! not a replacement for the quaternion/scale records used by attachments.
use super::actor_bone_query::Pose;
use anyhow::{ensure, Context, Result};

pub trait Compose {
    fn compose(&mut self, parent: Pose, local: Pose) -> Result<Pose>;
}
pub struct Storage {
    pub parents: Vec<i16>,
    pub local: Vec<Pose>,
    pub model: Vec<Pose>,
    pub flags: Vec<u32>,
}
impl Storage {
    pub fn resolve(&mut self, target: usize, compose: &mut impl Compose) -> Result<Pose> {
        ensure!(
            target < self.parents.len() && target < self.flags.len(),
            "Bone index outside pose storage"
        );
        let mut current = target;
        let mut visited = std::collections::BTreeSet::new();
        while *self
            .flags
            .get(current)
            .context("Missing ancestor pose flags")?
            & 2
            != 0
        {
            ensure!(visited.insert(current), "Cycle in dirty skeleton parents");
            let parent = *self
                .parents
                .get(current)
                .context("Missing skeleton parent")?;
            if parent == -1 {
                let local = *self.local.get(current).context("Missing root local pose")?;
                *self
                    .model
                    .get_mut(current)
                    .context("Missing root model pose")? = local;
                self.flags[current] &= !2;
                break;
            }
            ensure!(parent >= 0, "Invalid negative skeleton parent");
            self.flags[current] |= 8;
            current = parent as usize;
        }
        // Native code scans ascending indices, including intervening marked
        // bones. It does not recursively compose only a captured ancestor list.
        for bone in current + 1..=target {
            if *self
                .flags
                .get(bone)
                .context("Missing model resolution flags")?
                & 8
                == 0
            {
                continue;
            }
            let parent = *self
                .parents
                .get(bone)
                .context("Missing model resolution parent")?;
            ensure!(parent >= 0, "Marked bone has no parent");
            let parent = *self
                .model
                .get(parent as usize)
                .context("Missing parent model pose")?;
            let local = *self.local.get(bone).context("Missing bone local pose")?;
            let result = compose.compose(parent, local)?;
            *self
                .model
                .get_mut(bone)
                .context("Missing bone model pose")? = result;
            self.flags[bone] &= !(8 | 2);
        }
        self.model
            .get(target)
            .copied()
            .context("Missing requested model pose")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Sum(Vec<(f32, f32)>);
    impl Compose for Sum {
        fn compose(&mut self, p: Pose, l: Pose) -> Result<Pose> {
            self.0.push((p[0], l[0]));
            Ok(std::array::from_fn(|i| p[i] + l[i]))
        }
    }
    #[test]
    fn actor_pose_storage_resolves_marked_chain_and_retains_failure_state() -> Result<()> {
        let mut pose = Storage {
            parents: vec![-1, 0, 0, 1],
            local: vec![[1.; 12], [2.; 12], [3.; 12], [4.; 12]],
            model: vec![[99.; 12]; 4],
            flags: vec![2 | 16, 2 | 16, 2 | 16, 2 | 16],
        };
        let mut compose = Sum(vec![]);
        assert_eq!(pose.resolve(3, &mut compose)?, [7.; 12]);
        assert_eq!(compose.0, [(1., 2.), (3., 4.)]);
        assert_eq!(pose.flags, [16, 16, 18, 16]);
        assert_eq!(pose.model[2], [99.; 12]);
        assert_eq!(pose.resolve(3, &mut compose)?, [7.; 12]);
        assert_eq!(compose.0.len(), 2);
        struct Fail;
        impl Compose for Fail {
            fn compose(&mut self, _: Pose, _: Pose) -> Result<Pose> {
                anyhow::bail!("Composition unavailable")
            }
        }
        pose.flags[3] |= 2;
        assert!(pose.resolve(3, &mut Fail).is_err());
        assert_eq!(pose.flags[3], 16 | 8 | 2);
        assert_eq!(pose.model[3], [7.; 12]);
        Ok(())
    }
}
