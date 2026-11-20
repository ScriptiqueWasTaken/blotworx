//! Real all-hit Box3D adapter for the retail camera visibility predicates.
//! Native narrowphase, collector arrival/ties and nested-key parity remain
//! compatibility work; geometry/filter/property failures propagate explicitly.
use super::{
    camera_collision::{self, Backend, CastHit, RayHit, V},
    physics::{Collision, SphereCandidate},
};
use anyhow::{Context, Result};
pub struct Metadata {
    pub key: u32,
    pub body_filter: u32,
    pub child_filter: Option<u32>,
    pub material: u32,
}
pub trait Policy {
    fn metadata(&mut self, candidate: SphereCandidate) -> Result<Metadata>;
    fn collision_info(&self, query: u32, word: u32) -> u32;
    fn allow_look_through(&mut self, shape: usize) -> Result<bool>;
}
pub struct Adapter<'a, P> {
    pub geometry: &'a Collision,
    pub policy: P,
    materials: std::collections::BTreeMap<(usize, u32), u32>,
}
impl<'a, P: Policy> Adapter<'a, P> {
    pub fn new(geometry: &'a Collision, policy: P) -> Self {
        Self {
            geometry,
            policy,
            materials: Default::default(),
        }
    }
    fn admitted(policy: &P, mask: u32, m: &Metadata) -> bool {
        policy.collision_info(mask, m.body_filter) >> 24 != 0
            && m.child_filter
                .is_none_or(|word| policy.collision_info(mask, word) >> 24 != 0)
    }
}
impl<P: Policy> Backend for Adapter<'_, P> {
    fn ray(&mut self, from: V, to: V, filter: u32) -> Result<Vec<RayHit>> {
        self.materials.clear();
        let mut result = Vec::new();
        let hits = self.geometry.ray_hits(
            from[..3].try_into().unwrap(),
            std::array::from_fn(|i| to[i] - from[i]),
        )?;
        for hit in hits {
            let m = self.policy.metadata(SphereCandidate {
                shape: hit.shape,
                owner: hit.owner,
                source_triangle: hit.source_triangle,
            })?;
            if !Self::admitted(&self.policy, filter, &m) {
                continue;
            }
            self.materials.insert((hit.shape, m.key), m.material);
            let mut keys = [u32::MAX; 8];
            keys[0] = m.key;
            result.try_reserve(1)?;
            result.push(RayHit {
                body: hit.shape,
                fraction: hit.fraction,
                keys,
            });
        }
        super::cast_contacts::sort_fractions(&mut result, |hit| hit.fraction)?;
        Ok(result)
    }
    fn sphere(&mut self, from: V, to: V, radius: f32, filter: u32) -> Result<Vec<CastHit>> {
        self.materials.clear();
        let mut result = Vec::new();
        let policy = &mut self.policy;
        let hits = self.geometry.sphere_hits_filtered(
            from[..3].try_into().unwrap(),
            std::array::from_fn(|i| to[i] - from[i]),
            radius,
            |candidate| {
                let m = policy.metadata(candidate)?;
                Ok(Self::admitted(policy, filter, &m))
            },
        )?;
        for hit in hits {
            let m = self.policy.metadata(SphereCandidate {
                shape: hit.shape,
                owner: hit.owner,
                source_triangle: hit.source_triangle,
            })?;
            self.materials.insert((hit.shape, m.key), m.material);
            result.try_reserve(1)?;
            result.push(CastHit {
                body: hit.shape,
                fraction: hit.fraction,
                key: m.key,
                point: [hit.position[0], hit.position[1], hit.position[2], 0.],
                normal: [hit.normal[0], hit.normal[1], hit.normal[2], hit.fraction],
            });
        }
        super::cast_contacts::sort_fractions(&mut result, |hit| hit.fraction)?;
        Ok(result)
    }
    fn allow_look_through(&mut self, body: usize) -> Result<bool> {
        self.policy.allow_look_through(body)
    }
    fn material(&mut self, body: usize, key: u32) -> Result<u32> {
        self.materials
            .get(&(body, key))
            .copied()
            .context("Camera hit lost native material/key identity")
    }
}
impl<P: Policy> Adapter<'_, P> {
    fn overlaps(
        &mut self,
        center: V,
        radius: f32,
        filter: u32,
    ) -> Result<Vec<super::camera_placement::Overlap>> {
        // Nested overlap must not discard the outer cast's material records.
        // The next outer ray/sphere resets storage, bounding its lifetime.
        let mut result = Vec::new();
        for candidate in self
            .geometry
            .overlap_sphere_contacts(center[..3].try_into().unwrap(), radius)?
        {
            let m = self.policy.metadata(candidate)?;
            if !Self::admitted(&self.policy, filter, &m) {
                continue;
            }
            self.materials.insert((candidate.shape, m.key), m.material);
            result.try_reserve(1)?;
            result.push(super::camera_placement::Overlap {
                body: candidate.shape,
                key: m.key,
            });
        }
        Ok(result)
    }
}
impl super::camera_placement::PlacementBackend for Adapter<'_, WorldPolicy<'_>> {
    fn thinned_collides(&mut self) -> Result<bool> {
        let text = self
            .policy
            .0
            .runtime_settings
            .get("FreeCamera_Thinned_Collides")
            .map(String::as_str)
            .unwrap_or("false");
        match text {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => {
                let value = text
                    .parse::<f64>()
                    .context("Invalid camera Thinned Collides")?;
                anyhow::ensure!(
                    value.is_finite()
                        && value.trunc() >= i32::MIN as f64
                        && value.trunc() <= i32::MAX as f64,
                    "Unsupported camera boolean conversion"
                );
                Ok(value.trunc() as i32 != 0)
            }
        }
    }
    fn ignore_height(&mut self) -> Result<f32> {
        let value = self
            .policy
            .0
            .runtime_settings
            .get("FreeCamera_IgnoreHeight")
            .map(String::as_str)
            .unwrap_or("0.6")
            .parse::<f64>()
            .context("Invalid camera IgnoreHeight")? as f32;
        anyhow::ensure!(value.is_finite(), "Nonfinite camera IgnoreHeight");
        Ok(value)
    }
    fn tracked_actor_position(&mut self) -> Result<Option<V>> {
        let world = self.policy.0;
        let profile = world
            .camera_profiles
            .extras
            .get(usize::try_from(
                world.camera_profiles.controller.selection.current,
            )?)
            .context("Camera placement current profile absent")?;
        let Some(target) = profile
            .target
            .filter(|target| !world.removed_entities.contains(target))
        else {
            return Ok(None);
        };
        let entity = world
            .entities
            .get(target)
            .context("Camera placement target absent")?;
        if !entity
            .components
            .iter()
            .any(|c| c.class_name == "EntityTrackerComponent")
        {
            return Ok(None);
        }
        let tracker = world
            .mickey_tracker
            .as_ref()
            .filter(|tracker| tracker.owner == target)
            .context("Camera placement tracker is not bound")?;
        anyhow::ensure!(
            tracker.error.is_none(),
            "Camera placement tracker update failed"
        );
        if world.removed_entities.contains(&tracker.target) {
            return Ok(None);
        }
        anyhow::ensure!(
            tracker.target == world.player_entity,
            "Camera placement tracked controller binding is not reconstructed for this actor"
        );
        let actor = world
            .entities
            .get(tracker.target)
            .context("Tracked camera actor missing")?;
        Ok(Some([
            actor.transform[12],
            actor.transform[13],
            actor.transform[14],
            0.,
        ]))
    }
    fn overlap(
        &mut self,
        center: V,
        radius: f32,
        filter: u32,
    ) -> Result<Vec<super::camera_placement::Overlap>> {
        self.overlaps(center, radius, filter)
    }
}
impl super::camera_distance::Backend for Adapter<'_, WorldPolicy<'_>> {
    fn boolean(&mut self, offset: usize) -> Result<bool> {
        let (key, default) = match offset {
            0x2b4 => ("FreeCamera_Thinned_UsePushbackorDelay", "true"),
            0x2b8 => ("FreeCamera_Thinned_UseMoveThruDist", "true"),
            0x2c8 => (
                "FreeCamera_Thinned_PushThroughOnlyWhenMovingTowards",
                "true",
            ),
            _ => anyhow::bail!("Unbound native camera distance boolean {offset:x}"),
        };
        let text = self
            .policy
            .0
            .runtime_settings
            .get(key)
            .map(String::as_str)
            .unwrap_or(default);
        match text {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => {
                let number = text.parse::<f64>().context("Invalid distance boolean")?;
                anyhow::ensure!(
                    number.is_finite()
                        && number.trunc() >= i32::MIN as f64
                        && number.trunc() <= i32::MAX as f64,
                    "Unsupported camera distance boolean conversion"
                );
                Ok(number.trunc() as i32 != 0)
            }
        }
    }
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        let (key, default) = match offset {
            0x2bc => ("FreeCamera_Thinned_MoveThruDist", "3.0123"),
            0x3d4 => ("FreeCamera_Spring_Distance_In", "0.1"),
            0x3d8 => ("FreeCamera_Spring_Distance_In_Slow", "0.005f"),
            0x3e8 => ("FreeCamera_Spring_Distance_Out", "0.1"),
            0x670 => ("FreeCamera_LookThruMoveThruSpeed", "0.3"),
            0x674 => ("FreeCamera_LookThruMoveAwaySpeed", "0.1"),
            _ => anyhow::bail!("Unbound native camera distance scalar {offset:x}"),
        };
        let text = self
            .policy
            .0
            .runtime_settings
            .get(key)
            .map(String::as_str)
            .unwrap_or(default);
        // The verified constructor literally registers 0.005f. Its atof
        // conversion consumes the numeric prefix; support this authored suffix.
        let value = text
            .strip_suffix('f')
            .unwrap_or(text)
            .parse::<f64>()
            .context("Invalid camera distance scalar")? as f32;
        anyhow::ensure!(value.is_finite(), "Nonfinite camera distance scalar");
        Ok(value)
    }
    fn look_through_entity(&mut self, shape: usize) -> Result<usize> {
        let owner = self
            .geometry
            .shape_owner(shape)
            .context("Camera fading body has no owner")?;
        let entity = self
            .policy
            .0
            .collision_entity(owner)
            .context("Camera fading body has no entity")?;
        self.policy
            .0
            .entities
            .iter()
            .position(|v| std::ptr::eq(v, entity))
            .context("Camera fading entity is not in World")
    }
}
impl super::assets::World {
    pub(super) fn camera_resolve_lookthrough(
        &self,
        state: &mut [u8],
        entries: &[super::camera_distance::LookThrough],
    ) -> Result<()> {
        super::camera_lookthrough::resolve(
            state,
            entries,
            &mut Adapter::new(&self.collision, WorldPolicy(self)),
        )
    }
    /// Full distance stage with caller-owned initialized native scalar state.
    /// Complete derived-controller construction/update remains a prerequisite.
    pub(super) fn camera_solve_distance(
        &self,
        state: &mut [u8],
        from: V,
        to: V,
        velocity: &mut f32,
        follow: bool,
        fades: &mut Vec<super::camera_distance::LookThrough>,
    ) -> Result<f32> {
        super::camera_distance::solve(
            state,
            from,
            to,
            velocity,
            follow,
            fades,
            &mut Adapter::new(&self.collision, WorldPolicy(self)),
        )
    }
    /// Native polar/yaw stage; full generator supplies initialized persistent
    /// fields and the controller's real radius. It does not infer preview state.
    pub(super) fn camera_adjust_placement_yaw(
        &self,
        state: &mut super::camera_placement::YawState,
        requested: &mut V,
        unfiltered_target: V,
        radius: f32,
    ) -> Result<bool> {
        super::camera_placement::adjust_yaw(
            state,
            requested,
            unfiltered_target,
            radius,
            &mut Adapter::new(&self.collision, WorldPolicy(self)),
        )
    }
    pub(super) fn camera_placement_contact(
        &self,
        from: V,
        to: V,
        radius: f32,
        scale: f32,
    ) -> Result<Option<CastHit>> {
        super::camera_placement::contact(
            from,
            to,
            radius,
            scale,
            &mut Adapter::new(&self.collision, WorldPolicy(self)),
        )
    }
    /// Existing diagnostic orbit placement, now using the native visibility
    /// policy. Its margin/clamp and orbit remain explicitly provisional.
    pub(super) fn camera_preview_distance(
        &self,
        target: [f32; 3],
        direction: [f32; 3],
        maximum: f32,
    ) -> Result<f32> {
        let from = [target[0], target[1], target[2], 0.];
        let to = std::array::from_fn(|i| {
            if i < 3 {
                target[i] + direction[i] * maximum
            } else {
                0.
            }
        });
        let fraction = self
            .collision
            .with_player_queries(|| self.camera_obstruction_ray(from, to, false))?;
        Ok(fraction
            .map(|f| (f * maximum - 0.2).max(0.5))
            .unwrap_or(maximum))
    }
    pub(super) fn camera_obstruction_ray(
        &self,
        from: V,
        to: V,
        bypass_material: bool,
    ) -> Result<Option<f32>> {
        camera_collision::ray(
            from,
            to,
            bypass_material,
            &mut Adapter::new(&self.collision, WorldPolicy(self)),
        )
    }
    pub(super) fn camera_obstruction_sphere(
        &self,
        camera: V,
        target: V,
        radius: f32,
        scale: f32,
        bypass_material: bool,
    ) -> Result<bool> {
        camera_collision::sphere(
            camera,
            target,
            radius,
            scale,
            bypass_material,
            &mut Adapter::new(&self.collision, WorldPolicy(self)),
        )
    }
}
impl super::camera_lookthrough::Backend for Adapter<'_, WorldPolicy<'_>> {
    fn overlap(
        &mut self,
        center: [f32; 3],
        radius: f32,
        filter: u32,
    ) -> Result<Vec<super::camera_placement::Overlap>> {
        self.overlaps([center[0], center[1], center[2], 0.], radius, filter)
    }
    fn allow_look_through(&mut self, body: usize) -> Result<bool> {
        super::camera_collision::Backend::allow_look_through(self, body)
    }
    fn material(&mut self, body: usize, key: u32) -> Result<u32> {
        super::camera_collision::Backend::material(self, body, key)
    }
    fn entity(&mut self, body: usize) -> Result<usize> {
        super::camera_distance::Backend::look_through_entity(self, body)
    }
    fn scalar(&mut self, offset: usize) -> Result<f32> {
        super::camera_distance::Backend::scalar(self, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{physics::Triangle, physics_filter::GameFilter};
    use super::*;
    struct TestPolicy {
        filter: GameFilter,
        through: Vec<usize>,
        erased: Vec<usize>,
        reject: Vec<usize>,
    }
    impl Policy for TestPolicy {
        fn metadata(&mut self, c: SphereCandidate) -> Result<Metadata> {
            let word = if self.erased.contains(&c.owner) {
                2
            } else {
                0x8002
            };
            Ok(Metadata {
                key: c.source_triangle.map(|v| v as u32).unwrap_or(u32::MAX),
                body_filter: if self.reject.contains(&c.owner) {
                    0x8004
                } else {
                    word
                },
                child_filter: None,
                material: word,
            })
        }
        fn collision_info(&self, a: u32, b: u32) -> u32 {
            self.filter.collision_info(a, b)
        }
        fn allow_look_through(&mut self, shape: usize) -> Result<bool> {
            Ok(self.through.contains(&shape))
        }
    }
    impl super::super::camera_placement::PlacementBackend for Adapter<'_, TestPolicy> {
        fn thinned_collides(&mut self) -> Result<bool> {
            Ok(false)
        }
        fn ignore_height(&mut self) -> Result<f32> {
            Ok(1.1)
        }
        fn tracked_actor_position(&mut self) -> Result<Option<V>> {
            Ok(Some([0.; 4]))
        }
        fn overlap(
            &mut self,
            center: V,
            radius: f32,
            filter: u32,
        ) -> Result<Vec<super::super::camera_placement::Overlap>> {
            self.overlaps(center, radius, filter)
        }
    }
    impl super::super::camera_distance::Backend for Adapter<'_, TestPolicy> {
        fn boolean(&mut self, offset: usize) -> Result<bool> {
            match offset {
                0x2b4 | 0x2b8 => Ok(true),
                0x2c8 => Ok(false),
                _ => anyhow::bail!("Unexpected distance boolean"),
            }
        }
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            match offset {
                0x2bc => Ok(0.5),
                0x3d4 | 0x3e8 => Ok(0.1),
                0x3d8 => Ok(0.005),
                _ => anyhow::bail!("Unexpected distance scalar"),
            }
        }
        fn look_through_entity(&mut self, body: usize) -> Result<usize> {
            self.geometry
                .shape_owner(body)
                .context("No test source entity")
        }
    }
    fn distance_fixture_state() -> Vec<u8> {
        // Explicit controlled inputs, not a replacement native constructor.
        let mut s = vec![0; 0x800];
        for (offset, value) in [
            (0x384, 1.),
            (0x444, 1.),
            (0x44c, 1.),
            (0x488, f32::MAX),
            (0x580, 0.35),
        ] {
            s[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        s
    }
    impl super::super::camera_lookthrough::Backend for Adapter<'_, TestPolicy> {
        fn overlap(
            &mut self,
            center: [f32; 3],
            radius: f32,
            filter: u32,
        ) -> Result<Vec<super::super::camera_placement::Overlap>> {
            self.overlaps([center[0], center[1], center[2], 0.], radius, filter)
        }
        fn allow_look_through(&mut self, body: usize) -> Result<bool> {
            Backend::allow_look_through(self, body)
        }
        fn material(&mut self, body: usize, key: u32) -> Result<u32> {
            Backend::material(self, body, key)
        }
        fn entity(&mut self, body: usize) -> Result<usize> {
            self.geometry
                .shape_owner(body)
                .context("No look-through source entity")
        }
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            match offset {
                0x670 => Ok(0.3),
                0x674 => Ok(0.1),
                _ => anyhow::bail!("Unexpected look-through property"),
            }
        }
    }
    #[test]
    fn distance_contacts_feed_the_lookthrough_overlap_and_exit_history() -> Result<()> {
        let wall = Triangle {
            p: [[-5., -3., 4.], [0., 7., 4.], [5., -3., 4.]],
        };
        let mut collision = Collision::with_triangle_owners(vec![wall], vec![0.], vec![10])?;
        let policy = || TestPolicy {
            filter: GameFilter::default(),
            through: vec![0],
            erased: vec![],
            reject: vec![],
        };
        let mut state = distance_fixture_state();
        let mut velocity = 0.;
        let mut entries = Vec::new();
        let fraction = super::super::camera_distance::solve(
            &mut state,
            [0., 2., 0., 0.],
            [0., 2., 4., 0.],
            &mut velocity,
            true,
            &mut entries,
            &mut Adapter::new(&collision, policy()),
        )?;
        assert!(
            !entries.is_empty(),
            "Distance query didn't produce look-through records"
        );
        for (off, value) in [
            (0x350, 4.),
            (0x354, 0.),
            (0x358, 0.),
            (0x2d0, 0.),
            (0x2d4, 2.),
            (0x2d8, 0.),
            (0x5a0, 4.),
            (0x388, 0.2),
            (0x384, fraction),
        ] {
            state[off..off + 4].copy_from_slice(&value.to_be_bytes());
        }
        let read = |s: &[u8], off| f32::from_be_bytes(s[off..off + 4].try_into().unwrap());
        let target = entries
            .iter()
            .find(|e| e.entity == 10)
            .context("Wall record lost its entity")?
            .fraction
            .min(fraction);
        super::super::camera_lookthrough::resolve(
            &mut state,
            &entries,
            &mut Adapter::new(&collision, policy()),
        )?;
        let inside = 0.3f32.mul_add(target - 0.2, 0.2);
        assert_eq!(read(&state, 0x388).to_bits(), inside.to_bits());
        assert_eq!(read(&state, 0x350).to_bits(), (inside * 4.).to_bits());
        assert_eq!(&state[0x38c..0x38e], &[1, 1]);
        collision.set_owner_in_world(10, false)?;
        super::super::camera_lookthrough::resolve(
            &mut state,
            &entries,
            &mut Adapter::new(&collision, policy()),
        )?;
        let outside = 0.3f32.mul_add(fraction - inside, inside);
        assert_eq!(read(&state, 0x388).to_bits(), outside.to_bits());
        assert_eq!(&state[0x38c..0x38e], &[1, 0]);
        super::super::camera_lookthrough::resolve(
            &mut state,
            &[],
            &mut Adapter::new(&collision, policy()),
        )?;
        assert_eq!(read(&state, 0x384).to_bits(), outside.to_bits());
        assert_eq!(&state[0x38c..0x38e], &[0, 0]);
        std::fs::write(
            "target/camera-lookthrough-box3d.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Controlled real Box3D wall: distance records feed the second overlap stage, wall removal and empty-list exit. Not a complete gameplay camera or Havok parity capture.",
            "first_fraction":fraction,"entry_fraction":target,"inside":inside,"after_removal":outside,"radius_after_exit":read(&state,0x350)}))?,
        )?;
        Ok(())
    }
    struct TestOrientation;
    impl super::super::camera_orientation::Backend for TestOrientation {
        fn look_min_pitch(&mut self) -> Result<f32> {
            Ok(-0.8123)
        }
        fn look_max_pitch(&mut self) -> Result<f32> {
            Ok(1.5123)
        }
        fn upside_down(&mut self) -> Result<bool> {
            Ok(false)
        }
    }
    struct TestOutput;
    impl super::super::camera_output::Backend for TestOutput {
        fn target_offset(&mut self) -> Result<Option<[f32; 3]>> {
            Ok(None)
        }
        fn orient(
            &mut self,
            s: &[u8],
            info: &mut super::super::camera_base::Information,
            target: V,
        ) -> Result<()> {
            super::super::camera_orientation::orient(
                s,
                &mut info.transform,
                target,
                &mut TestOrientation,
            )
        }
    }
    struct TestPass<'a>(&'a Collision);
    impl TestPass<'_> {
        fn adapter(&self) -> Adapter<'_, TestPolicy> {
            Adapter::new(
                self.0,
                TestPolicy {
                    filter: GameFilter::default(),
                    through: vec![0],
                    erased: vec![],
                    reject: vec![],
                },
            )
        }
    }
    impl super::super::camera_placement_pass::Backend for TestPass<'_> {
        fn distance(
            &mut self,
            s: &mut [u8],
            from: V,
            to: V,
            velocity: &mut f32,
            entries: &mut Vec<super::super::camera_distance::LookThrough>,
        ) -> Result<f32> {
            super::super::camera_distance::solve(
                s,
                from,
                to,
                velocity,
                true,
                entries,
                &mut self.adapter(),
            )
        }
        fn lookthrough(
            &mut self,
            s: &mut [u8],
            entries: &[super::super::camera_distance::LookThrough],
        ) -> Result<()> {
            super::super::camera_lookthrough::resolve(s, entries, &mut self.adapter())
        }
        fn scalar(&mut self, offset: usize) -> Result<f32> {
            match offset {
                0x2c4 => Ok(1.5123),
                0x3e8 => Ok(0.1),
                _ => anyhow::bail!("Unexpected placement property"),
            }
        }
        fn boolean(&mut self, offset: usize) -> Result<bool> {
            anyhow::ensure!(offset == 0x2c0, "Unexpected placement flag");
            Ok(true)
        }
        fn special_output(
            &mut self,
            s: &mut [u8],
            info: &mut super::super::camera_base::Information,
            target: V,
            entries: &[super::super::camera_distance::LookThrough],
        ) -> Result<()> {
            assert!(entries.is_empty());
            super::super::camera_orientation::orient(
                s,
                &mut info.transform,
                target,
                &mut TestOrientation,
            )
        }
        fn finish(
            &mut self,
            s: &mut [u8],
            info: &mut super::super::camera_base::Information,
            delta: f32,
            w: f32,
            entries: &[super::super::camera_distance::LookThrough],
        ) -> Result<()> {
            assert!(entries.is_empty());
            super::super::camera_output::finish(s, info, delta, w, &mut TestOutput)
        }
    }
    #[test]
    fn placement_pass_composes_distance_overlap_and_real_orientation() -> Result<()> {
        let collision = Collision::with_triangle_owners(
            vec![Triangle {
                p: [[-5., -3., 4.], [0., 7., 4.], [5., -3., 4.]],
            }],
            vec![0.],
            vec![10],
        )?;
        let mut rows = Vec::new();
        for special in [false, true] {
            let mut s = distance_fixture_state();
            for (off, v) in [
                (0x350, 4f32),
                (0x360, 4.),
                (0x5a0, 4.),
                (0x388, 0.2),
                (0x2d4, 2.),
                (0x2f4, 2.),
                (0x304, 2.),
            ] {
                s[off..off + 4].copy_from_slice(&v.to_be_bytes());
            }
            s[0x1bf] = special as u8;
            let initial = [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 2., 4., 6.,
            ];
            let mut info = super::super::camera_base::Information {
                transform: initial,
                field_of_view: 63.,
                focus: [1., 2., 3., -0.],
            };
            let mut entries = Vec::new();
            super::super::camera_placement_pass::update(
                &mut s,
                &mut info,
                [0., 2., 4., 9.],
                super::super::STEP,
                7.,
                &mut entries,
                &mut TestPass(&collision),
            )?;
            assert!(entries.is_empty());
            assert!(info.transform.iter().all(|v| v.is_finite()));
            assert_eq!(info.field_of_view, 63.);
            assert_eq!(
                info.focus.map(f32::to_bits),
                [1., 2., 3., -0.].map(f32::to_bits)
            );
            if special {
                assert_eq!(
                    &info.transform[12..16],
                    &initial[12..16],
                    "Special output must only orient the incoming view"
                );
            } else {
                assert!(
                    info.transform[14] > 0. && info.transform[14] < 4.,
                    "The combined placement ignored look-through distance"
                );
                assert_eq!(info.transform[15], 7., "Caller scratch W was replaced");
                assert_eq!(
                    &s[0x350..0x360],
                    &s[0x360..0x370],
                    "Final placement did not publish polar history"
                );
            }
            rows.push(serde_json::json!({"special":special,"transform":info.transform,"distance":f32::from_be_bytes(s[0x350..0x354].try_into().unwrap())}));
        }
        std::fs::write(
            "target/camera-placement-pass-box3d.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"Controlled real Box3D wall with the production distance, second-overlap, final placement and orientation functions. Supplied fixture/controller inputs; not the full default camera or original-frame parity.","frames":rows}))?,
        )?;
        Ok(())
    }
    #[test]
    fn distance_solver_runs_statefully_against_a_real_wall_and_its_removal() -> Result<()> {
        let wall = Triangle {
            p: [[-5., -3., 2.], [0., 7., 2.], [5., -3., 2.]],
        };
        let mut collision = Collision::with_triangle_owners(vec![wall], vec![0.], vec![10])?;
        let policy = || TestPolicy {
            filter: GameFilter::default(),
            through: vec![],
            erased: vec![],
            reject: vec![],
        };
        let mut state = distance_fixture_state();
        let mut velocity = 0.;
        let mut fades = Vec::new();
        let mut trace = Vec::new();
        for tick in 0..40 {
            if tick == 16 {
                collision.set_owner_in_world(10, false)?;
            }
            let fraction = super::super::camera_distance::solve(
                &mut state,
                [0., 2., 0., 0.],
                [0., 2., 4., 0.],
                &mut velocity,
                true,
                &mut fades,
                &mut Adapter::new(&collision, policy()),
            )?;
            assert!(fraction.is_finite() && fraction > 0. && fraction <= 1.);
            state[0x384..0x388].copy_from_slice(&fraction.to_be_bytes());
            trace.push(fraction);
        }
        assert!(
            trace[15] < 0.6,
            "Wall did not constrain the full distance stage"
        );
        assert!(
            trace[39] > trace[16] && trace[39] > 0.95,
            "Outward spring did not recover after removing the wall"
        );
        assert!(fades.is_empty());
        println!("Box3D distance trace: {}", serde_json::to_string(&trace)?);
        Ok(())
    }
    #[test]
    fn yaw_solver_uses_live_geometry_to_search_past_a_narrow_obstruction() -> Result<()> {
        let wall = Triangle {
            p: [[-0.08, 1.8, 2.], [0., 2.2, 2.], [0.08, 1.8, 2.]],
        };
        let collision = Collision::with_triangle_owners(vec![wall], vec![0.], vec![10])?;
        let policy = TestPolicy {
            filter: GameFilter::default(),
            through: vec![],
            erased: vec![],
            reject: vec![],
        };
        let mut backend = Adapter::new(&collision, policy);
        let mut state = super::super::camera_placement::YawState {
            previous_polar: [4., 0., 0., 0.],
            target: [0., 2., 0., 0.],
            last_target: [0., 2., 0., 0.],
            acquired_inside: false,
        };
        let mut requested = [4., 0., 0., 0.];
        assert!(
            super::super::camera_placement::adjust_yaw(
                &mut state,
                &mut requested,
                [0., 2., 0., 0.],
                0.35,
                &mut backend
            )?,
            "Narrow wall must enter native yaw correction"
        );
        assert!(requested[1].abs() > 0. && requested[1].abs() < 0.8);
        assert_eq!([requested[0], requested[2], requested[3]], [4., 0., 0.]);
        assert!(!state.acquired_inside);
        Ok(())
    }
    #[test]
    fn placement_uses_real_overlap_children_and_retains_outer_cast_materials() -> Result<()> {
        let wall = Triangle {
            p: [[0., 0., -2.], [0., 0.6, 2.], [0., 0.6, -2.]],
        };
        let roof = Triangle {
            p: [[-2., 1.4, -2.], [0., 1.4, 2.], [2., 1.4, -2.]],
        };
        let policy = || TestPolicy {
            filter: GameFilter::default(),
            through: vec![],
            erased: vec![],
            reject: vec![],
        };
        let from = [-2., 0.5, 0., 0.];
        let to = [2., 0.5, 0., 0.];
        let world = Collision::with_triangle_owners(vec![wall.clone()], vec![0.], vec![10])?;
        assert!(
            super::super::camera_placement::contact(
                from,
                to,
                0.25,
                0.5,
                &mut Adapter::new(&world, policy())
            )?
            .is_none(),
            "Low wall should be ignored when its clearance sphere is empty"
        );
        let world = Collision::with_triangle_owners(vec![wall, roof], vec![0.; 2], vec![10, 20])?;
        let mut adapter = Adapter::new(&world, policy());
        let hit = super::super::camera_placement::contact(from, to, 0.25, 0.5, &mut adapter)?
            .context("Roof must retain low wall as an obstruction")?;
        assert!(hit.normal[0] < 0.);
        assert_eq!(hit.normal[3], hit.fraction);
        // A secondary overlap must leave the outer contact material available.
        assert_eq!(adapter.material(hit.body, hit.key)?, 0x8002);
        adapter.policy.erased = vec![20];
        assert!(
            super::super::camera_placement::contact(from, to, 0.25, 0.5, &mut adapter)?.is_none()
        );
        Ok(())
    }
    #[test]
    fn actual_all_hit_geometry_retains_far_occluders_after_near_rejection() -> Result<()> {
        let triangles = [0., 2.]
            .map(|h| Triangle {
                p: [[-2., h, -2.], [0., h, 2.], [2., h, -2.]],
            })
            .to_vec();
        let mut world = Collision::with_triangle_owners(triangles, vec![0.; 2], vec![10, 20])?;
        let near = world
            .owner_shapes(20)
            .next()
            .context("Near shape missing")?;
        // A thinned/visually pass-through surface remains available to the
        // collector, so a farther real occluder cannot be lost by clipping.
        world.set_owner_solid(20, false)?;
        let policy = || TestPolicy {
            filter: GameFilter::default(),
            through: vec![near],
            erased: vec![],
            reject: vec![],
        };
        let from = [0., 4., 0., 0.];
        let to = [0., -2., 0., 0.];
        let mut adapter = Adapter::new(&world, policy());
        let fraction = camera_collision::ray(from, to, false, &mut adapter)?
            .context("Far wall disappeared")?;
        assert!((fraction - 4. / 6.).abs() < 1e-6);
        assert!(!camera_collision::sphere(
            to,
            from,
            0.1,
            1.,
            false,
            &mut adapter
        )?);
        adapter.policy.through.clear();
        adapter.policy.erased = vec![10, 20];
        assert!(camera_collision::ray(from, to, false, &mut adapter)?.is_none());
        assert!(camera_collision::sphere(
            to,
            from,
            0.1,
            1.,
            false,
            &mut adapter
        )?);
        assert!(camera_collision::ray(from, to, true, &mut adapter)?.is_some());
        // The real shared matrix, not material interpretation, controls query
        // admission. Rejected near shapes must not shorten the farther cast.
        adapter.policy.erased.clear();
        adapter.policy.reject = vec![20];
        adapter.policy.filter.matrix.rows[9] &= !(1 << 4);
        assert!(
            (camera_collision::ray(from, to, false, &mut adapter)?.unwrap() - 4. / 6.).abs() < 1e-6
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_gv_queries_resolve_live_geometry_properties_and_jigsaw_materials() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let p = world.report.spawn;
        let above = [p[0], p[1] + 4., p[2], 0.];
        let below = [p[0], p[1] - 4., p[2], 0.];
        assert!(
            world
                .collision
                .ray_hits(above[..3].try_into().unwrap(), [0., -8., 0.])?
                .len()
                > 0,
            "Original spawn floor absent"
        );
        let ray = world.camera_obstruction_ray(above, below, false)?;
        let sphere = world.camera_obstruction_sphere(below, above, 0.1, 1., false)?;
        assert!(ray.is_some(), "Original spawn floor must block the camera");
        assert!(!sphere);
        let current = usize::try_from(world.camera_profiles.controller.selection.current)?;
        let target = world.camera_profiles.extras[current].target;
        let tracked = world.camera_placement_contact(above, below, 0.1, 1.)?;
        println!("Original GV tracked placement contact: {tracked:?}");
        // Native e19dc declines actor clearance when the current profile has
        // no tracker. Make that branch explicit: a nearby floor can otherwise
        // legitimately be ignored by IgnoreHeight when space above is clear.
        world.assign_camera_target(None, false)?;
        let contact = world
            .camera_placement_contact(above, below, 0.1, 1.)?
            .context("Original no-tracker floor contact absent")?;
        assert!(contact.normal[1] > 0.);
        assert!(contact.fraction > 0.);
        let owner = world
            .collision
            .shape_owner(contact.body)
            .context("Placement contact owner absent")?;
        assert!(
            !world
                .player_physics
                .as_ref()
                .is_some_and(|body| body.owner == owner),
            "Floor fixture selected Mickey instead"
        );
        let mut distance_state = distance_fixture_state();
        let mut velocity = 0.;
        let mut fades = Vec::new();
        let fraction = world.camera_solve_distance(
            &mut distance_state,
            above,
            below,
            &mut velocity,
            true,
            &mut fades,
        )?;
        assert!(
            fraction > 0. && fraction < 1.,
            "Original GV floor must shorten the distance stage"
        );
        assert!(velocity <= 0.);
        assert!(velocity.is_finite());
        // Exercise the constructor's literal numeric suffix when the live
        // setting is absent, without mutating the original configuration file.
        let key = "FreeCamera_Spring_Distance_In_Slow";
        let saved = world.runtime_settings.remove(key);
        let slow = super::super::camera_distance::Backend::scalar(
            &mut Adapter::new(&world.collision, WorldPolicy(&world)),
            0x3d8,
        )?;
        assert_eq!(slow.to_bits(), 0.005f32.to_bits());
        if let Some(saved) = saved {
            world.runtime_settings.insert(key.into(), saved);
        }
        world.assign_camera_target(target, false)?;
        // Resolve a real installed piece/triangle, preserving original key
        // identity and querying the mutable palette rather than serialized0.
        let (owner, shape, triangle) = world
            .jigsaw_owners
            .iter()
            .find_map(|(&owner, source)| {
                let instance = &world.jigsaw_instances[source.instance];
                let entity = &world.entities[instance.entity];
                let asset = &world.jigsaws[entity.jigsaw_asset.as_ref()?][instance.node];
                if !entity.jigsaw_states[instance.node].painted[source.piece] {
                    return None;
                }
                let shape = world.collision.owner_shapes(owner).next()?;
                let triangle = (0..world.collision.triangles.len()).find(|&index| {
                    instance
                        .collision
                        .triangle_source(index)
                        .is_some_and(|s| asset.collision.parts[s.subpart].piece == source.piece)
                })?;
                Some((owner, shape, triangle))
            })
            .context("Original painted jigsaw query fixture absent")?;
        let candidate = SphereCandidate {
            owner,
            shape,
            source_triangle: Some(triangle),
        };
        let painted = WorldPolicy(&world).metadata(candidate)?;
        assert_ne!(painted.material & 0x8000, 0);
        assert_ne!(painted.key, u32::MAX);
        world.preview_jigsaw_contact(owner, false)?;
        let thinned = WorldPolicy(&world).metadata(candidate)?;
        assert_eq!(thinned.key, painted.key);
        assert_eq!(thinned.material & 0x8000, 0);
        assert_eq!(thinned.child_filter, Some(thinned.material));
        world.preview_jigsaw_contact(owner, true)?;
        assert_eq!(
            WorldPolicy(&world).metadata(candidate)?.material,
            painted.material
        );
        let source = world.jigsaw_owners[&owner];
        let entity = world.jigsaw_instances[source.instance].entity;
        let component = world.entities[entity]
            .components
            .iter()
            .position(|c| c.class_name == "PhysicsJigsawComponent")
            .context("Original jigsaw property owner absent")?;
        let field = world.entities[entity].components[component]
            .properties
            .iter()
            .position(|p| p.name == "Allow Look Through")
            .context("Original jigsaw look-through property absent")?;
        let initial = world.entities[entity].components[component].properties[field]
            .value
            .clone();
        for value in [true, false] {
            world.entities[entity].components[component].properties[field].value =
                serde_json::json!(value);
            assert_eq!(
                WorldPolicy(&world).allow_look_through(shape)?,
                value,
                "Property must resolve the hit body's live entity"
            );
        }
        world.entities[entity].components[component].properties[field].value = initial;
        Ok(())
    }
}
struct WorldPolicy<'a>(&'a super::assets::World);
impl super::assets::World {
    /// Shared shape provenance, also used by projectile contact admission.
    /// Keep body, collection child and live jigsaw words in one binding path.
    pub(super) fn collision_shape_metadata(&self, candidate: SphereCandidate) -> Result<Metadata> {
        WorldPolicy(self).metadata(candidate)
    }
    pub(super) fn advance_brush_projectiles(&mut self, dt: f32) -> Result<()> {
        let mut projectiles = std::mem::take(&mut self.projectile_preview);
        let result = projectiles.advance_filtered(&self.collision, dt, |source, candidate| {
            let admitted =
                |word| self.dynamic_runtime_filter.collision_info(source, word) >> 24 != 0;
            if !admitted(self.collision.simulation_filter_word(candidate.shape)?) {
                return Ok(false);
            }
            let metadata = self.collision_shape_metadata(candidate)?;
            Ok(metadata.child_filter.is_none_or(admitted))
        });
        self.projectile_preview = projectiles;
        result
    }
}
impl Policy for WorldPolicy<'_> {
    fn collision_info(&self, query: u32, word: u32) -> u32 {
        self.0.dynamic_runtime_filter.collision_info(query, word)
    }
    fn allow_look_through(&mut self, shape: usize) -> Result<bool> {
        let owner = self
            .0
            .collision
            .shape_owner(shape)
            .context("Camera hit has no collision owner")?;
        let entity = self
            .0
            .collision_entity(owner)
            .context("Camera hit user-data entity is not bound")?;
        let mut values = entity
            .components
            .iter()
            .filter(|c| c.class_name.starts_with("Physics"))
            .filter_map(|c| c.properties.iter().find(|p| p.name == "Allow Look Through"));
        let Some(value) = values.next() else {
            anyhow::bail!(
                "Camera body {} has no resolved Allow Look Through property",
                entity.name
            )
        };
        anyhow::ensure!(
            values.next().is_none(),
            "Camera body {} has ambiguous physics component property ownership",
            entity.name
        );
        value
            .value
            .as_bool()
            .context("Invalid Allow Look Through property")
    }
    fn metadata(&mut self, c: SphereCandidate) -> Result<Metadata> {
        let world = self.0;
        let body_filter = world.collision.simulation_filter_word(c.shape)?;
        anyhow::ensure!(
            world.collision.shape_owner(c.shape) == Some(c.owner),
            "Camera candidate owner mismatch"
        );
        if let Some(owner) = world.jigsaw_owners.get(&c.owner) {
            let instance = &world.jigsaw_instances[owner.instance];
            let entity = &world.entities[instance.entity];
            let asset = &world.jigsaws[entity
                .jigsaw_asset
                .as_ref()
                .context("Missing camera jigsaw asset")?][instance.node];
            let key = match c.source_triangle {
                Some(source) => instance.collision.triangle_key(asset, source)?,
                None => instance.collision.convex_key(asset, c.shape)?,
            }
            .context("Camera jigsaw hit has no native shape key")?;
            let painted = *entity.jigsaw_states[instance.node]
                .painted
                .get(owner.piece)
                .context("Camera jigsaw piece state missing")?;
            let material = super::jigsaw_state::RUNTIME_MATERIALS
                [super::jigsaw_state::material_index((body_filter & 31) | ((painted as u32) << 15))
                    as usize];
            return Ok(Metadata {
                key,
                body_filter,
                child_filter: Some(material),
                material,
            });
        }
        if let Some(part) = world.imported_static_part(c.owner) {
            let source = world.imported_static_candidate(c)?;
            let mesh = part
                .native_resource
                .get(part.native_part)
                .context("Camera static body source missing")?;
            let collection = mesh.bounds_graph.query_collection(mesh.bounds_root)?;
            let child = collection.map(|_| source.serialized_child_filter);
            let jigsaw = world.collision_entity(c.owner).is_some_and(|entity| {
                entity
                    .components
                    .iter()
                    .any(|c| c.class_name == "PhysicsJigsawComponent")
            });
            let use_child = source.shape_key != u32::MAX
                && collection.is_some_and(|jigsaw_only| !jigsaw_only || jigsaw);
            return Ok(Metadata {
                key: source.shape_key,
                body_filter,
                child_filter: child,
                material: if use_child {
                    source.serialized_child_filter
                } else {
                    body_filter
                },
            });
        }
        if world
            .player_physics
            .as_ref()
            .is_some_and(|body| body.owner == c.owner)
            || world
                .character_physics
                .values()
                .any(|body| body.as_ref().is_ok_and(|body| body.owner == c.owner))
        {
            anyhow::ensure!(
                c.source_triangle.is_none(),
                "Character camera hit unexpectedly has a triangle key"
            );
            return Ok(Metadata {
                key: u32::MAX,
                body_filter,
                child_filter: None,
                material: body_filter,
            });
        }
        for (&source, &owner) in &world.dynamic_runtime_sources {
            if owner != c.owner {
                continue;
            }
            let construction = world
                .dynamic_constructions
                .get(&source)
                .context("Camera dynamic body construction missing")?;
            let parts = world.dynamic_bodies[source]
                .geometry
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?;
            for (body, layout) in construction.bodies.iter().zip(&construction.layouts) {
                let Some(index) = body.shapes.iter().position(|&shape| shape == c.shape) else {
                    continue;
                };
                let part = parts
                    .get(layout.parts[index])
                    .context("Camera dynamic body part missing")?;
                let triangle = c
                    .source_triangle
                    .map(|source| body.local_contact_triangle(c.shape, source))
                    .transpose()?
                    .flatten();
                let key = part.shape_keys.key(triangle)?;
                let collection = part.bounds_graph.query_collection(part.bounds_root)?;
                let child = collection
                    .map(|_| part.child_filters.at(triangle))
                    .transpose()?;
                let jigsaw = world.entities[world.dynamic_bodies[source].entity]
                    .components
                    .iter()
                    .any(|c| c.class_name == "PhysicsJigsawComponent");
                let use_child =
                    key != u32::MAX && collection.is_some_and(|jigsaw_only| !jigsaw_only || jigsaw);
                return Ok(Metadata {
                    key,
                    body_filter,
                    child_filter: child,
                    material: if use_child {
                        child.context("Camera collection child missing")?
                    } else {
                        body_filter
                    },
                });
            }
            anyhow::bail!("Camera dynamic shape has no original body part");
        }
        anyhow::bail!(
            "Camera query body {} has no original source metadata",
            c.owner
        )
    }
}
