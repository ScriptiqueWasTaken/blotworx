//! Paired-single Qs composition in DOL 0x801e6bc4..0x801e6d48.
use super::actor_bone_query::Pose;
type Pair = [f32; 2];
fn mul(a: Pair, b: Pair) -> Pair {
    [a[0] * b[0], a[1] * b[1]]
}
fn add(a: Pair, b: Pair) -> Pair {
    [a[0] + b[0], a[1] + b[1]]
}
fn madd(a: Pair, b: Pair, c: Pair) -> Pair {
    [a[0].mul_add(b[0], c[0]), a[1].mul_add(b[1], c[1])]
}
fn msub(a: Pair, b: Pair, c: Pair) -> Pair {
    [a[0].mul_add(b[0], -c[0]), a[1].mul_add(b[1], -c[1])]
}
fn neg(a: Pair) -> Pair {
    [-a[0], -a[1]]
}
fn splat(a: f32) -> Pair {
    [a, a]
}
fn load(a: &Pose, i: usize) -> Pair {
    [a[i], a[i + 1]]
}

/// Retains quaternion and four scale lanes separately. The source rotates
/// translation without multiplying it by parent scale; matrix composition
/// would change this behavior. FPSCR/exception equivalence remains unverified.
pub fn compose(parent: Pose, local: Pose) -> Pose {
    let mut out = [0.; 12];
    let mut f4 = load(&local, 0);
    let f0 = splat(local[2]);
    let mut f10 = load(&local, 2);
    let mut f31 = [f4[1], f4[0]];
    let mut f11 = load(&parent, 4);
    let f1 = splat(parent[6]);
    let f13 = f11;
    let f12 = load(&parent, 6);
    f11 = mul(f11, f4);
    let f3_scalar = parent[7];
    let f6 = load(&parent, 0);
    let mut f9 = mul(f4, splat(f13[0]));
    let mut f2 = madd(f12, f10, f11);
    let mut f30 = mul(f4, f1);
    f31 = msub(f13, f31, f9);
    f9 = load(&parent, 2);
    let dot = f2[0] + f11[1]; // ps_sum0 f11,f2,f11,f11
    f30 = msub(f13, f0, f30);
    let factor = f3_scalar.mul_add(f3_scalar, -0.5);
    f31 = [f31[1], f30[0]];
    f4 = mul(f4, splat(factor));
    f2 = mul(f10, splat(factor));
    f31 = neg(f31);
    let f29 = splat(f3_scalar);
    let mut f3 = madd(f13, splat(dot), f4);
    f10 = [f30[1], f31[1]];
    f2 = madd(f12, splat(dot), f2);
    f4 = madd(f10, f29, f3);
    f3 = madd(f31, f29, f2);
    f2 = add(f4, f4);
    f4 = add(f3, f3);
    let translation_xy = add(f6, f2);
    let translation_zw = add(f9, f4);
    out[..2].copy_from_slice(&translation_xy);
    out[2..4].copy_from_slice(&translation_zw);

    let f8 = splat(parent[6]);
    f9 = load(&local, 4);
    f10 = load(&parent, 4);
    let f7 = splat(local[6]);
    let f6 = [f9[1], f9[0]];
    f2 = mul(f9, splat(f10[0]));
    let local_w = local[7];
    let parent_w = parent[7];
    f3 = mul(f9, f8);
    f2 = msub(f10, f6, f2);
    let f11 = load(&local, 6);
    f3 = msub(f10, f7, f3);
    let f6 = load(&parent, 6);
    f2 = [f2[1], f3[0]];
    f2 = neg(f2);
    f3 = [f3[1], f2[1]];
    f2 = madd(f11, splat(parent_w), f2);
    f3 = madd(f9, splat(parent_w), f3);
    f3 = madd(f10, splat(local_w), f3);
    f4 = mul(f10, f9);
    out[4..6].copy_from_slice(&f3);
    f2 = madd(f6, splat(local_w), f2);
    f3 = madd(f6, f11, f4);
    out[6..8].copy_from_slice(&f2);
    let dot = f3[0] + f4[1]; // ps_sum0 f3,f3,f4,f4
    out[7] = parent_w.mul_add(local_w, -dot);
    out[8..10].copy_from_slice(&mul(load(&parent, 8), load(&local, 8)));
    out[10..12].copy_from_slice(&mul(load(&parent, 10), load(&local, 10)));
    out
}

pub struct Native;
impl super::actor_pose_storage::Compose for Native {
    fn compose(&mut self, parent: Pose, local: Pose) -> anyhow::Result<Pose> {
        Ok(compose(parent, local))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actor_pose_compose_keeps_qs_scale_separate_and_rotates_translation() {
        let parent = [10., 20., 30., 0., 0., 0., 1., 0., 2., 3., 4., 5.];
        let local = [1., 2., 3., 0., 0., 0., 0., 1., 6., 7., 8., 9.];
        let out = compose(parent, local);
        assert_eq!(&out[..3], &[9., 18., 33.]);
        assert_eq!(&out[4..8], &[0., 0., 1., 0.]);
        assert_eq!(&out[8..], &[12., 21., 32., 45.]);
        let mut storage = super::super::actor_pose_storage::Storage {
            parents: vec![-1, 0],
            local: vec![parent, local],
            model: vec![[99.; 12]; 2],
            flags: vec![2, 2],
        };
        assert_eq!(storage.resolve(1, &mut Native).unwrap(), out);
        assert_eq!(storage.flags, [0, 0]);
        let identity = [0., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1.];
        // Nonzero scalar quaternion lanes distinguish ps_sum0's sources;
        // summing the two lanes of the first operand breaks identity.
        assert_eq!(compose(identity, identity), identity);
        assert_eq!(compose(identity, local), local);
        assert_eq!(compose(parent, identity), parent);
    }
}
