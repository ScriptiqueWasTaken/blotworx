//! Retail dynamic-body activation policy, not a Box3D simulation substitute.
//! A caller must execute every returned transition side effect. World installs
//! eligible bodies through dynamic_runtime; unsupported systems stay explicit.

/// Source geometry retained for a placed component. Repeated placements share
/// immutable geometry, while entity identity and future runtime bodies differ.
/// Vertices remain body-local. Serialized placement lives in body_transform;
/// it must be composed with the entity pose exactly once during installation.
/// A decode failure stays attached to the component, not an empty mesh.
pub struct Imported {
    pub entity: usize,
    pub component: u32,
    pub asset: String,
    pub geometry: Result<std::sync::Arc<Vec<super::collision::StaticMesh>>, String>,
    pub systems: Result<std::sync::Arc<Vec<super::physics_system::Imported>>, String>,
    pub selected_system: Result<Option<usize>, String>,
    pub initial_velocity: Result<InitialVelocity, String>,
}
/// Owned construction state retained on failure for diagnosis. Query/world
/// installation is subsequent; no static substitutes are installed here.
pub struct Construction {
    /// Committed owner pose at body construction; scripts and attachment
    /// initialization may have changed it since scene deserialization.
    pub entity_pose: super::native_hk_transform::Transform,
    pub layouts: Vec<BodyLayout>,
    pub bodies: Vec<PlacedBody>,
    /// Per-body model-node binding, independent of the component/entity owner.
    pub node_bindings: Vec<Option<NodeBinding>>,
    pub materials: Vec<super::body_material::Coefficients>,
    pub filters: Vec<u32>,
    pub bounds: super::shape_bounds::ShapeGraph,
    pub mover_material_seen: bool,
    pub started: bool,
    pub completed: bool,
}
#[derive(Clone, Copy)]
pub struct NodeBinding {
    pub node: u32,
    pub offset: super::native_hk_transform::Transform,
}
impl Construction {
    pub fn new(
        source: &Imported,
        entity: &super::native_hk_transform::Transform,
    ) -> anyhow::Result<Option<Self>> {
        use anyhow::Context;
        let Some(layouts) = source.selected_layout(entity)? else {
            return Ok(None);
        };
        let layouts = layouts
            .into_iter()
            .map(|layout| layout.context("Sparse body construction is unsupported"))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let parts = source
            .geometry
            .as_ref()
            .map_err(|error| anyhow::anyhow!(error.clone()))?;
        let bounds = parts
            .first()
            .map(|part| part.bounds_graph.as_ref().clone())
            .unwrap_or_default();
        let mut bodies = Vec::new();
        let mut materials = Vec::new();
        let mut filters = Vec::new();
        for layout in &layouts {
            let part = &parts[layout.parts[0]];
            bodies.push(PlacedBody::new(
                part.motion.instantiate()?,
                vec![],
                part.instantiate_auxiliary()?,
            )?);
            materials.push(part.material);
            filters.push(part.body_filter_info);
        }
        let node_bindings = vec![None; bodies.len()];
        Ok(Some(Self {
            entity_pose: *entity,
            layouts,
            bodies,
            node_bindings,
            materials,
            filters,
            bounds,
            mover_material_seen: false,
            started: false,
            completed: false,
        }))
    }
}
#[derive(Default, serde::Serialize)]
pub struct SourceSummary {
    pub components: usize,
    pub decoded: usize,
    pub bodies: usize,
    pub modes: std::collections::BTreeMap<u8, usize>,
    pub systems: usize,
    pub constraints: usize,
    pub actions: usize,
    pub phantoms: usize,
}
impl SourceSummary {
    pub fn new(imported: &[Imported]) -> Self {
        let mut result = Self {
            components: imported.len(),
            ..Self::default()
        };
        for component in imported {
            if let Ok(systems) = &component.systems {
                for source in systems.iter() {
                    result.systems += 1;
                    result.constraints += source.system.constraints.iter().flatten().count();
                    result.actions += source.system.actions.iter().flatten().count();
                    result.phantoms += source.system.phantoms.iter().flatten().count();
                }
            }
            let Ok(parts) = &component.geometry else {
                continue;
            };
            result.decoded += 1;
            let mut bodies = std::collections::BTreeSet::new();
            for part in parts.iter() {
                if bodies.insert(part.body_offset) {
                    result.bodies += 1;
                    *result
                        .modes
                        .entry(part.motion.current.motion_type())
                        .or_default() += 1;
                }
            }
        }
        result
    }
}

/// REL descriptor slots +8/+0xc/+0x10 at 0x2d220: Initial Velocity,
/// Initial Velocity Max, Initial Linear Velocity Min. Sampling occurs inside
/// the loader's body loop, but its setter (0x381944) visits the entire system.
#[derive(Clone, Copy, Debug)]
pub struct InitialVelocity {
    pub minimum: [f32; 3],
    pub maximum: [f32; 3],
    pub minimum_speed: f32,
}
impl InitialVelocity {
    pub fn from_component(
        component: &epic_mickey_lib_rs::scene_file::Component,
    ) -> anyhow::Result<Self> {
        use anyhow::Context;
        let property = |name: &str| component.properties.iter().find(|p| p.name == name);
        let vector = |name: &str| -> anyhow::Result<[f32; 3]> {
            let Some(property) = property(name) else {
                return Ok([0.; 3]);
            };
            let mut result = [0.; 3];
            for (i, axis) in ["x", "y", "z"].into_iter().enumerate() {
                result[i] = property
                    .value
                    .get(axis)
                    .and_then(|v| v.as_f64())
                    .with_context(|| format!("Invalid {name}.{axis}"))?
                    as f32;
            }
            Ok(result)
        };
        let result = Self {
            minimum: vector("Initial Velocity")?,
            maximum: vector("Initial Velocity Max")?,
            minimum_speed: property("Initial Linear Velocity Min")
                .map(|p| {
                    p.value
                        .as_f64()
                        .context("Invalid Initial Linear Velocity Min")
                })
                .transpose()?
                .unwrap_or(0.) as f32,
        };
        result.validate()?;
        Ok(result)
    }
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.minimum
                .iter()
                .chain(&self.maximum)
                .all(|v| v.is_finite())
                && self.minimum_speed.is_finite(),
            "Nonfinite initial rigid-body velocity parameters"
        );
        Ok(())
    }
    /// REL 0x382898..0x382960. Uses caller-owned shared DOL rand state;
    /// zero maximum skips its draw, even if the corresponding minimum differs.
    /// Equality of nonzero bounds still consumes a draw. Does not apply writes
    /// to bodies or skip the native loader's per-body invocation ordering.
    pub fn sample(&self, mut random: impl FnMut() -> u16) -> anyhow::Result<[f32; 4]> {
        self.validate()?;
        let mut velocity = self.minimum;
        for i in 0..3 {
            if self.maximum[i] != 0. {
                let value = random();
                anyhow::ensure!(value <= 32767, "Rigid-body RNG must produce 15-bit samples");
                velocity[i] =
                    (value as f32 / 32767.).mul_add(self.maximum[i] - velocity[i], velocity[i]);
            }
        }
        // 0x805aea20 has the same positive-input estimate/refinement as
        // 0x800071f0. Preserve paired dot order before the square root.
        let squared =
            velocity[2].mul_add(velocity[2], velocity[0] * velocity[0]) + velocity[1] * velocity[1];
        anyhow::ensure!(
            squared.is_finite(),
            "Initial rigid-body velocity length overflow"
        );
        let length = super::cursor::native_sqrt(squared);
        if length < self.minimum_speed {
            if length == 0. {
                velocity = [self.minimum_speed / super::cursor::native_sqrt(3.); 3];
            } else {
                let factor = self.minimum_speed / length;
                velocity = velocity.map(|v| v * factor);
            }
        }
        anyhow::ensure!(
            velocity.iter().all(|v| v.is_finite()),
            "Initial rigid-body velocity overflow"
        );
        Ok([velocity[0], velocity[1], velocity[2], 0.])
    }
}

/// REL 0x381944: the component setter visits the entire system, even when
/// called from inside the loader's per-body initialization. XYZ alone gates
/// the write; activation precedes the virtual setter, including fixed bodies.
pub fn set_system_linear_velocity(
    bodies: &mut [PlacedBody],
    velocity: [f32; 4],
    mut activate: impl FnMut(usize, &PlacedBody) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    for (index, body) in bodies.iter_mut().enumerate() {
        let old = body.linear_velocity();
        // Negated <= also preserves the original unordered comparison branch.
        if !(0..3).all(|axis| (old[axis] - velocity[axis]).abs() <= f32::from_bits(0x3a83126f)) {
            activate(index, body)?;
            body.motion.set_linear_velocity(velocity)?;
        }
    }
    Ok(())
}

/// Execute only the velocity stage of one loader iteration (0x38281c..970).
/// Call at that body's real initialization boundary, not as a second pass:
/// material/mass/shape setup remains interleaved in the original loader.
pub fn initialize_body_velocity(
    bodies: &mut [PlacedBody],
    index: usize,
    parameters: &InitialVelocity,
    random: impl FnMut() -> u16,
    activate: impl FnMut(usize, &PlacedBody) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let body = bodies
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("Invalid initialization body {index}"))?;
    let mode = body.motion.current.motion_type();
    anyhow::ensure!(
        (1..=7).contains(&mode),
        "Invalid initialization motion type {mode}"
    );
    if matches!(mode, 4 | 5) {
        return Ok(());
    }
    let velocity = parameters.sample(random)?;
    set_system_linear_velocity(bodies, velocity, activate)
}

/// Owns initialized motion and the query shapes of one body (not the entity).
/// Construction requires the actual initialized state; it does not invent
/// lifecycle defaults or convert serialized dynamic motion into keyframing.
pub struct PlacedBody {
    pub motion: super::body_motion::BodyMotion,
    pub auxiliary: super::character_runtime_storage::BodyAuxiliary,
    pub shapes: Vec<usize>,
    pub welding: Option<super::shape_welding::Prepared>,
    // Per installed shape: original subpart, authored word offset, triangle count.
    welding_parts: Vec<Option<(usize, usize, usize)>>,
    /// Immutable source mesh index for each installed triangle shape. This
    /// resolves contact vertices without retaining another world-space copy.
    source_parts: Vec<Option<usize>>,
    // Collision::triangles ranges captured when each mesh is appended.
    source_ranges: Vec<Option<std::ops::Range<usize>>>,
    triangle_radii: Vec<Option<f32>>,
}
impl PlacedBody {
    /// Install the recovered dynamic userdata prefix/cache after welding,
    /// before native activation. Owner token is supplied by entity allocation.
    /// Direction and transform bindings are separate required loader stages.
    pub fn initialize_dynamic_user_data(&mut self, entity: Option<usize>) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.auxiliary.user_data.is_none(),
            "Dynamic userdata initialized twice"
        );
        self.auxiliary.user_data = Some(super::collidable_owner::EntityData::dynamic(entity));
        Ok(())
    }
    /// Loader382f44..60, using the retained pre-placement frame rather than
    /// this body's current placed world transform. No second entity transform.
    pub fn bind_dynamic_user_data(
        &mut self,
        point: [f32; 3],
        preplacement: [f32; 16],
    ) -> anyhow::Result<()> {
        let binding = super::collidable_owner::DynamicBinding::new(point, preplacement)?;
        let data = self
            .auxiliary
            .user_data
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Missing dynamic userdata"))?;
        anyhow::ensure!(
            data.activation_cache.is_some() && data.byte_09 == 1,
            "Uninitialized dynamic userdata"
        );
        anyhow::ensure!(
            data.dynamic_binding.is_none(),
            "Dynamic userdata bound twice"
        );
        data.dynamic_binding = Some(binding);
        Ok(())
    }
    /// REL383378/383490: only the activity flag tolerates missing userdata.
    pub fn set_activation_active(&mut self, active: bool) {
        if let Some(data) = &mut self.auxiliary.user_data {
            data.byte_08 = u8::from(active);
        }
    }
    /// REL3834a4..3834c8 stores both complete native vectors before zeroing.
    pub fn cache_activation_velocities(&mut self) -> anyhow::Result<()> {
        let linear = self.linear_velocity().map(f32::to_bits);
        let angular = self.angular_velocity().map(f32::to_bits);
        let data = self
            .auxiliary
            .user_data
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Activation cache requires body user data"))?;
        data.activation_cache = Some(super::collidable_owner::ActivationCache { linear, angular });
        Ok(())
    }
    pub fn cached_activation_velocities(&self) -> anyhow::Result<([f32; 4], [f32; 4])> {
        let cache = self
            .auxiliary
            .user_data
            .as_ref()
            .and_then(|data| data.activation_cache)
            .ok_or_else(|| anyhow::anyhow!("Activation velocity cache is uninitialized"))?;
        Ok((
            cache.linear.map(f32::from_bits),
            cache.angular.map(f32::from_bits),
        ))
    }
    /// Bind the initialized native solver storage to this body's compound
    /// Box3D owner. Only the lifecycle owner may decide dynamic admission.
    pub fn admit_native_mass(&self, world: &mut super::physics::Collision) -> anyhow::Result<()> {
        let (inertia, mass) = self.motion.current.solver_inverse_mass()?;
        let center = self.state().local_center;
        world.set_native_body_mass(
            &self.shapes,
            mass,
            inertia,
            [center[0], center[1], center[2]],
        )
    }
    pub fn new(
        motion: super::body_motion::BodyMotion,
        shapes: Vec<usize>,
        auxiliary: super::character_runtime_storage::BodyAuxiliary,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=7).contains(&motion.current.motion_type()),
            "Invalid placed-body motion type"
        );
        let welding_parts = vec![None; shapes.len()];
        let source_parts = vec![None; shapes.len()];
        let source_ranges = vec![None; shapes.len()];
        let triangle_radii = vec![None; shapes.len()];
        Ok(Self {
            motion,
            auxiliary,
            shapes,
            welding: None,
            welding_parts,
            source_parts,
            source_ranges,
            triangle_radii,
        })
    }
    /// Recover the native triangle from a query-world contact identity. The
    /// returned geometry is body-local; pair it with this body's current state
    /// transform, never the serialized placement or Box3D simplex vertices.
    pub fn contact_triangle(
        &self,
        parts: &[super::collision::StaticMesh],
        shape: usize,
        source_triangle: usize,
    ) -> anyhow::Result<Option<super::collision::TriangleContactShape>> {
        use anyhow::Context;
        let Some(local) = self.local_contact_triangle(shape, source_triangle)? else {
            return Ok(None);
        };
        let index = self
            .shapes
            .iter()
            .position(|&id| id == shape)
            .context("Unknown body shape")?;
        let word = self
            .welding_word(shape, local)?
            .context("Triangle has no prepared welding")?;
        let mode = match self.welding.as_ref().context("Missing body welding")? {
            super::shape_welding::Prepared::Authored(w) => w.mode,
            super::shape_welding::Prepared::TwoSided(_) => 5,
        };
        let source_part = self.source_parts[index].context("Missing source mesh binding")?;
        let part = parts
            .get(source_part)
            .context("Missing source collision mesh")?;
        let face_start = local
            .checked_mul(3)
            .context("Source contact triangle index overflow")?;
        let face = part
            .indices
            .get(face_start..face_start + 3)
            .context("Source contact triangle is outside its mesh")?;
        let mut vertices = [[0.; 4]; 3];
        for corner in 0..3 {
            let position = part
                .vertices
                .get(face[corner] as usize)
                .context("Source contact vertex is outside its mesh")?;
            vertices[corner] = [position[0], position[1], position[2], 0.];
        }
        Ok(Some(super::collision::TriangleContactShape {
            support: super::native_gsk::TriangleSupport {
                vertices,
                extrusion: [0.; 4],
                extrusion_flag: 0,
            },
            radius: self.triangle_radii[index].context("Missing imported triangle radius")?,
            mode,
            word,
        }))
    }
    /// Translate a Collision::contact_sources triangle ID to this shape's
    /// original local triangle. Convex shapes have no triangle identity.
    pub fn local_contact_triangle(
        &self,
        shape: usize,
        source_triangle: usize,
    ) -> anyhow::Result<Option<usize>> {
        use anyhow::Context;
        let index = self
            .shapes
            .iter()
            .position(|&id| id == shape)
            .context("Shape does not belong to body")?;
        let Some(range) = &self.source_ranges[index] else {
            return Ok(None);
        };
        anyhow::ensure!(
            range.contains(&source_triangle),
            "Contact triangle does not belong to shape"
        );
        Ok(Some(source_triangle - range.start))
    }
    pub fn contact_welding_word(
        &self,
        shape: usize,
        source_triangle: usize,
    ) -> anyhow::Result<Option<u16>> {
        match self.local_contact_triangle(shape, source_triangle)? {
            Some(local) => self.welding_word(shape, local),
            None => Ok(None),
        }
    }
    /// Resolve the original shape-local triangle identity, never Box3D's reordered face
    /// index. None denotes a shape without extended-mesh welding metadata.
    pub fn welding_word(
        &self,
        shape: usize,
        source_triangle: usize,
    ) -> anyhow::Result<Option<u16>> {
        use anyhow::Context;
        let index = self
            .shapes
            .iter()
            .position(|&id| id == shape)
            .context("Shape does not belong to body")?;
        let Some((subpart, offset, count)) = self.welding_parts[index] else {
            return Ok(None);
        };
        anyhow::ensure!(
            source_triangle < count,
            "Source welding triangle outside shape"
        );
        let word = match self
            .welding
            .as_ref()
            .context("Body lost prepared welding storage")?
        {
            super::shape_welding::Prepared::Authored(w) => w
                .subpart(offset, count)?
                .context("Prepared authored welding is empty")?[source_triangle],
            super::shape_welding::Prepared::TwoSided(w) => w.word(subpart, source_triangle)?,
        };
        Ok(Some(word))
    }
    pub fn state(&self) -> super::native_motion_step::MotionState {
        self.motion.current.swept_state()
    }
    pub fn linear_velocity(&self) -> [f32; 4] {
        self.motion.current.linear_velocity()
    }
    pub fn angular_velocity(&self) -> [f32; 4] {
        self.motion.current.angular_velocity()
    }
    /// Publish Box3D solver output to the owned record. Swept bookkeeping uses
    /// the recovered native step, while pose/velocities come from Box3D. This
    /// compatibility path is not numerical Havok solver parity.
    pub fn accept_simulation_pose(
        &mut self,
        pose: super::physics::SimulationPose,
        start: f32,
        dt: f32,
    ) -> anyhow::Result<()> {
        let mut state = self.state();
        let mut linear = self.linear_velocity();
        let mut angular = self.angular_velocity();
        linear[..3].copy_from_slice(&pose.linear);
        angular[..3].copy_from_slice(&pose.angular);
        super::native_motion_step::step(
            &mut state,
            &mut linear,
            &mut angular,
            start,
            dt,
            1. / dt,
            [
                super::native_motion_limits::decode(self.motion.current.0[0xbc]),
                super::native_motion_limits::decode(self.motion.current.0[0xbd]),
            ],
        )?;
        state.orientation = pose.orientation;
        super::native_hk_transform::set_rotation(&mut state.transform, pose.orientation);
        state.transform[12..15].copy_from_slice(&pose.position);
        state.center[..3].copy_from_slice(&pose.center);
        self.motion.current.set_swept_state(&state);
        self.motion.set_linear_velocity([
            pose.linear[0],
            pose.linear[1],
            pose.linear[2],
            linear[3],
        ])?;
        self.motion.set_angular_velocity([
            pose.angular[0],
            pose.angular[1],
            pose.angular[2],
            angular[3],
        ])?;
        Ok(())
    }
    #[cfg(test)]
    pub fn test_body(
        state: super::native_motion_step::MotionState,
        linear: [f32; 4],
        angular: [f32; 4],
        limits: [u8; 2],
        shapes: Vec<usize>,
    ) -> Self {
        let mut record = super::native_motion_record::MotionRecord([0; 0x120]);
        record.0[8] = 4;
        record.set_swept_state(&state);
        record.set_linear_velocity(linear);
        record.set_angular_velocity(angular);
        record.0[0xbc..0xbe].copy_from_slice(&limits);
        Self::new(
            super::body_motion::BodyMotion::new(record, 1, None).unwrap(),
            shapes,
            super::character_runtime_storage::BodyAuxiliary {
                properties: Default::default(),
                listeners: Default::default(),
                user_data: None,
            },
        )
        .unwrap()
    }
    /// Apply the recovered target-velocity policy, preserving activation before
    /// each write and linear before angular. The lifecycle owner supplies real
    /// activation; a failure aborts immediately, retaining earlier side effects.
    /// Collision placement changes only when stepping or explicitly warping.
    pub fn update_target(
        &mut self,
        origin: [f32; 4],
        orientation: [f32; 4],
        inverse_delta: f32,
        mut activate: impl FnMut(&Self) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.motion.current.motion_type() == 4,
            "Target update requires keyframed motion"
        );
        let state = self.state();
        let writes = KeyframeState {
            orientation: state.orientation,
            center_of_mass: state.center,
            local_center_of_mass: state.local_center,
            linear_velocity: self.linear_velocity(),
            angular_velocity: self.angular_velocity(),
        }
        .target_writes(origin, orientation, inverse_delta)?;
        if let Some(velocity) = writes.linear {
            activate(self)?;
            self.motion.set_linear_velocity(velocity)?;
        }
        if let Some(velocity) = writes.angular {
            activate(self)?;
            self.motion.set_angular_velocity(velocity)?;
        }
        Ok(())
    }
    /// Step once at the caller's original simulation boundary, then publish
    /// the result to every collision part. This does not apply target writes,
    /// choose a timestep, run character carry, or execute dynamic-body forces.
    pub fn advance_keyframed(
        &mut self,
        world: &mut super::physics::Collision,
        start: f32,
        delta: f32,
        inverse_delta: f32,
    ) -> anyhow::Result<()> {
        let mut next = self.motion.current.clone();
        next.step_keyframed(start, delta, inverse_delta)?;
        self.publish_record(world, next)
    }
    /// Apply the recovered setTransform and publish its pose to the query
    /// shapes without changing velocities, mode or the retained dynamic owner.
    /// This supplies the motion/query part of parent pose propagation; native
    /// activation, world callbacks and deferred admission remain caller-owned.
    pub fn warp(
        &mut self,
        world: &mut super::physics::Collision,
        transform: super::native_hk_transform::Transform,
    ) -> anyhow::Result<()> {
        let mut next = self.motion.current.clone();
        next.set_transform(transform)?;
        self.publish_record(world, next)
    }
    fn publish_record(
        &mut self,
        world: &mut super::physics::Collision,
        next: super::native_motion_record::MotionRecord,
    ) -> anyhow::Result<()> {
        let state = next.swept_state();
        let linear = next.linear_velocity();
        let angular = next.angular_velocity();
        world.set_body_state(
            &self.shapes,
            state.transform[12..15].try_into().unwrap(),
            state.orientation,
            super::physics::BodyMotion {
                center_of_mass: state.center[..3].try_into().unwrap(),
                linear_velocity: linear[..3].try_into().unwrap(),
                angular_velocity: angular[..3].try_into().unwrap(),
                motion_type: next.motion_type(),
            },
        )?;
        self.motion.current = next;
        Ok(())
    }
}

/// Ordered velocity writes requested by REL 0x2432c8. For each Some value,
/// activate the body before its setter; linear precedes angular. None preserves
/// the existing velocity, including its exact bits. This is not a pose teleport.
#[derive(Debug, PartialEq)]
pub struct KeyframeWrites {
    pub linear: Option<[f32; 4]>,
    pub angular: Option<[f32; 4]>,
}
pub struct KeyframeState {
    pub orientation: [f32; 4],
    pub center_of_mass: [f32; 4],
    pub local_center_of_mass: [f32; 4],
    pub linear_velocity: [f32; 4],
    pub angular_velocity: [f32; 4],
}
impl KeyframeState {
    pub fn target_writes(
        &self,
        origin: [f32; 4],
        orientation: [f32; 4],
        inverse_delta: f32,
    ) -> anyhow::Result<KeyframeWrites> {
        anyhow::ensure!(
            self.linear_velocity
                .iter()
                .chain(&self.angular_velocity)
                .all(|v| v.is_finite()),
            "Nonfinite existing keyframe velocity"
        );
        let linear = super::native_hk_transform::keyframe_linear_velocity4(
            origin,
            orientation,
            self.local_center_of_mass,
            self.center_of_mass,
            inverse_delta,
        )?;
        let angular = super::native_hk_transform::keyframe_angular_velocity4(
            orientation,
            self.orientation,
            inverse_delta,
        )?;
        // REL 0x243424..0x2434dc and 0x243764..0x243840: compare XYZ only.
        // If any XYZ lane exceeds the threshold, replace all FOUR stored lanes.
        let changed =
            |old: [f32; 4], new: [f32; 4]| (0..3).any(|i| (old[i] - new[i]).abs() > 0.001);
        Ok(KeyframeWrites {
            linear: changed(self.linear_velocity, linear).then_some(linear),
            angular: changed(self.angular_velocity, angular).then_some(angular),
        })
    }
}

/// One serialized rigid body, potentially split into several collision shapes.
/// Part indices refer to shared Imported.geometry; no vertex data is copied.
#[derive(Clone, Debug)]
pub struct BodyLayout {
    pub body_offset: usize,
    pub parts: Vec<usize>,
    pub initial_transform: super::native_hk_transform::Transform,
    pub local_center_of_mass: [f32; 4],
    pub swept_base_time: f32,
    pub encoded_motion_limits: [u8; 2],
}
impl Imported {
    pub fn selected_layout(
        &self,
        entity: &super::native_hk_transform::Transform,
    ) -> anyhow::Result<Option<Vec<Option<BodyLayout>>>> {
        let selected = *self
            .selected_system
            .as_ref()
            .map_err(|error| anyhow::anyhow!(error.clone()))?;
        selected
            .map(|offset| self.system_layout(offset, entity))
            .transpose()
    }
    /// Select an explicit resource system, then preserve its body-array order
    /// and null slots. Definition order is used only to locate shared geometry.
    pub fn system_layout(
        &self,
        system_offset: usize,
        entity: &super::native_hk_transform::Transform,
    ) -> anyhow::Result<Vec<Option<BodyLayout>>> {
        use anyhow::Context;
        let parts = self
            .geometry
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?;
        let systems = self
            .systems
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?;
        let source = systems
            .iter()
            .find(|system| system.offset == system_offset)
            .context("Unknown resource physics system")?;
        let definitions = layout(parts, entity)?
            .into_iter()
            .map(|body| (body.body_offset, body))
            .collect::<std::collections::BTreeMap<_, _>>();
        source
            .system
            .bodies
            .iter()
            .map(|body| {
                body.map(|offset| {
                    definitions
                        .get(&offset)
                        .cloned()
                        .with_context(|| format!("System body {offset:#x} has no decoded geometry"))
                })
                .transpose()
            })
            .collect()
    }
}
impl BodyLayout {
    /// Attach query geometry to an already constructed owner. Preserve its
    /// motion snapshots, userdata and prepared welding; do not reconstruct it.
    /// Query installation is not native world admission or dynamic simulation.
    /// As elsewhere, discard the query world after an installation failure.
    pub fn attach_constructed_queries(
        &self,
        body: &mut PlacedBody,
        parts: &[super::collision::StaticMesh],
        world: &mut super::physics::Collision,
        owner: usize,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(body.shapes.is_empty(), "Body queries already attached");
        anyhow::ensure!(
            body.state().transform == self.initial_motion_state()?.transform,
            "Constructed query pose mismatch"
        );
        let needs_welding = self
            .parts
            .iter()
            .any(|&index| parts[index].welding.is_some());
        anyhow::ensure!(
            !needs_welding || body.welding.is_some(),
            "Construction welding missing"
        );
        let bindings = self
            .parts
            .iter()
            .map(|&index| {
                let part = &parts[index];
                part.welding.as_ref().map(|_| {
                    (
                        part.subpart_index,
                        part.welding_offset,
                        part.indices.len() / 3,
                    )
                })
            })
            .collect();
        let installed = self.install_query_parts(parts, world, owner)?;
        let (shapes, ranges): (Vec<_>, Vec<_>) = installed.into_iter().unzip();
        world.group_body_shapes(&shapes)?;
        body.shapes = shapes;
        body.source_ranges = ranges;
        body.welding_parts = bindings;
        body.source_parts = self
            .parts
            .iter()
            .map(|&part| (!parts[part].convex).then_some(part))
            .collect();
        body.triangle_radii = self
            .parts
            .iter()
            .map(|&index| {
                let part = &parts[index];
                (!part.convex).then_some(part.triangle_radius)
            })
            .collect();
        Ok(())
    }
    /// Assemble query shapes and welding around caller-initialized motion.
    /// This owns the resources but does not complete dynamic world admission,
    /// activation, force integration, or contact-normal welding.
    /// As with install_query_geometry, discard the construction world on error.
    pub fn place_initialized_query_body(
        &self,
        parts: &[super::collision::StaticMesh],
        motion: super::body_motion::BodyMotion,
        auxiliary: super::character_runtime_storage::BodyAuxiliary,
        world: &mut super::physics::Collision,
        owner: usize,
    ) -> anyhow::Result<PlacedBody> {
        anyhow::ensure!(
            (1..=7).contains(&motion.current.motion_type()),
            "Invalid placed-body motion type"
        );
        let welding = self.prepare_welding(parts)?;
        anyhow::ensure!(
            motion.current.swept_state().transform == self.initial_motion_state()?.transform,
            "Initialized motion and query placement disagree"
        );
        let bindings = self
            .parts
            .iter()
            .map(|&index| {
                let p = &parts[index];
                p.welding
                    .as_ref()
                    .map(|_| (p.subpart_index, p.welding_offset, p.indices.len() / 3))
            })
            .collect();
        let installed = self.install_query_parts(parts, world, owner)?;
        let (shapes, source_ranges): (Vec<_>, Vec<_>) = installed.into_iter().unzip();
        world.group_body_shapes(&shapes)?;
        let mut body = PlacedBody::new(motion, shapes, auxiliary)?;
        body.welding = welding;
        body.welding_parts = bindings;
        body.source_parts = self
            .parts
            .iter()
            .map(|&part| (!parts[part].convex).then_some(part))
            .collect();
        body.source_ranges = source_ranges;
        body.triangle_radii = self
            .parts
            .iter()
            .map(|&index| {
                let part = &parts[index];
                (!part.convex).then_some(part.triangle_radius)
            })
            .collect();
        Ok(body)
    }
    /// REL 2430ec retains populated mode-5 data; otherwise its MOPP helper
    /// generates single-body two-sided welding. Original resources stay shared
    /// and immutable. Direct convex/list bodies have no extended-mesh storage.
    pub fn prepare_welding(
        &self,
        parts: &[super::collision::StaticMesh],
    ) -> anyhow::Result<Option<super::shape_welding::Prepared>> {
        use super::shape_welding::{Prepared, Regenerated};
        use anyhow::{ensure, Context};
        let mut source = None;
        let mut triangles = Vec::new();
        for &index in &self.parts {
            let part = parts
                .get(index)
                .context("Invalid welding preparation part")?;
            ensure!(
                part.body_offset == self.body_offset,
                "Welding preparation owner changed"
            );
            if let Some(w) = &part.welding {
                ensure!(
                    part.mopp.is_some(),
                    "Extended mesh welding has no MOPP root"
                );
                if let Some(prior) = source {
                    ensure!(
                        std::sync::Arc::ptr_eq(prior, w),
                        "Mixed welding storage in body"
                    );
                }
                source = Some(w);
                triangles.push(part);
            }
        }
        let Some(source) = source else {
            return Ok(None);
        };
        if !source.needs_two_sided_generation() {
            return Ok(Some(Prepared::Authored(source.clone())));
        }
        triangles.sort_by_key(|p| p.subpart_index);
        let mut counts = Vec::new();
        let mut destinations = std::collections::BTreeMap::new();
        for (subpart, part) in triangles.iter().enumerate() {
            ensure!(
                part.subpart_index == subpart,
                "Incomplete extended-mesh subpart table"
            );
            ensure!(part.indices.len() % 3 == 0, "Incomplete welding triangle");
            counts.push(part.indices.len() / 3);
            for triangle in 0..part.indices.len() / 3 {
                ensure!(
                    destinations
                        .insert(part.shape_keys.key(Some(triangle))?, (subpart, triangle))
                        .is_none(),
                    "Duplicate welding destination key"
                );
            }
        }
        let mut storage = Regenerated::new(source, &counts)?;
        for (key, word) in self.generate_local_welding(parts)? {
            let &(subpart, triangle) = destinations
                .get(&key)
                .context("Unresolved welding write key")?;
            storage.write(subpart, triangle, word)?;
        }
        Ok(Some(Prepared::TwoSided(storage)))
    }
    /// Single-body identity-collection generation used by the retail loader's
    /// MOPP helper. Requires body_local_meshes, never world-baked geometry.
    /// Returns writes for visited keys without modifying originals. Degenerate
    /// triangle keys are omitted, as in retail mesh iteration; callers must not
    /// overwrite unvisited storage or interpret omission as a generated zero.
    pub fn generate_local_welding(
        &self,
        parts: &[super::collision::StaticMesh],
    ) -> anyhow::Result<Vec<(u32, u16)>> {
        let identity =
            super::native_hk_transform::from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [0.; 3]);
        self.generate_welding(parts, &identity, &[(self, identity)], true)
    }
    /// General ordered-collection welding (retail 80287974..80287de0).
    /// All body indices refer to the same body-local parts table. Transforms
    /// are explicit so runtime identity-space generation and authored asset
    /// placement never silently substitute for one another. Returns writes
    /// only; degenerate source triangles leave storage untouched.
    pub fn generate_welding(
        &self,
        parts: &[super::collision::StaticMesh],
        source_transform: &super::native_hk_transform::Transform,
        collections: &[(&BodyLayout, super::native_hk_transform::Transform)],
        weld_open_edges: bool,
    ) -> anyhow::Result<Vec<(u32, u16)>> {
        use anyhow::Context;
        anyhow::ensure!(
            source_transform.iter().all(|x| x.is_finite()),
            "Nonfinite welding source transform"
        );
        for (body, transform) in collections {
            let first = parts
                .get(*body.parts.first().context("Empty welding collection")?)
                .context("Invalid welding collection part")?;
            anyhow::ensure!(
                first.body_offset == body.body_offset && first.mopp.is_some(),
                "Welding collection has no matching MOPP body"
            );
            anyhow::ensure!(
                transform.iter().all(|x| x.is_finite()),
                "Nonfinite welding collection transform"
            );
        }
        let triangle = |part: usize, index: usize| -> anyhow::Result<[[f32; 4]; 3]> {
            let part = parts.get(part).context("Invalid welding part")?;
            let start = index.checked_mul(3).context("Welding triangle overflow")?;
            let indices = part
                .indices
                .get(start..start.checked_add(3).context("Welding triangle overflow")?)
                .context("Missing welding triangle")?;
            let mut vertices = [[0.; 4]; 3];
            for i in 0..3 {
                vertices[i][..3].copy_from_slice(
                    part.vertices
                        .get(indices[i] as usize)
                        .context("Missing welding vertex")?,
                );
            }
            Ok(vertices)
        };
        let mut result = Vec::new();
        for &index in &self.parts {
            let part = parts.get(index).context("Invalid welding body part")?;
            anyhow::ensure!(
                part.body_offset == self.body_offset,
                "Welding body owner changed"
            );
            if part.convex {
                continue;
            }
            anyhow::ensure!(part.indices.len() % 3 == 0, "Incomplete welding triangle");
            for i in 0..part.indices.len() / 3 {
                let vertices = triangle(index, i)?;
                if super::shape_welding::degenerate_triangle(vertices)? {
                    continue;
                }
                let word =
                    super::shape_welding::generate_triangle(weld_open_edges, |edge, intervals| {
                        for (collection, transform) in collections {
                            let frame = super::shape_welding::CandidateFrame::in_collection(
                                vertices,
                                source_transform,
                                transform,
                                edge,
                            )?;
                            for (candidate, face) in
                                collection.query_local(parts, frame.query_bounds())?
                            {
                                if let Some(face) = face {
                                    frame.accumulate(triangle(candidate, face)?, intervals)?;
                                }
                            }
                        }
                        Ok(())
                    })?;
                result.push((part.shape_keys.key(Some(i))?, word));
            }
        }
        Ok(result)
    }
    /// Bounds are in original body-local coordinates. Preserve MOPP result
    /// order/duplicates and resolve every key; no missing-key fallback.
    pub fn query_local(
        &self,
        parts: &[super::collision::StaticMesh],
        bounds: [[f32; 4]; 2],
    ) -> anyhow::Result<Vec<(usize, Option<usize>)>> {
        use anyhow::Context;
        let first = parts
            .get(*self.parts.first().context("Body has no parts")?)
            .context("Invalid body part")?;
        anyhow::ensure!(
            first.body_offset == self.body_offset,
            "Body query owner changed"
        );
        let code = first
            .mopp
            .as_ref()
            .context("Body has no MOPP query program")?;
        let mut result = Vec::new();
        for key in code.query(bounds)? {
            let mut found = None;
            for &index in &self.parts {
                let part = parts.get(index).context("Invalid body query part")?;
                anyhow::ensure!(
                    part.body_offset == self.body_offset
                        && part
                            .mopp
                            .as_ref()
                            .is_some_and(|other| std::sync::Arc::ptr_eq(code, other)),
                    "Body query resource changed"
                );
                let triangle = match part.shape_keys {
                    super::collision::ShapeKeys::List { child } if child == key => Some(None),
                    super::collision::ShapeKeys::Triangles { bits, subpart } => {
                        anyhow::ensure!((1..=31).contains(&bits), "Invalid query shape-key width");
                        let shift = 32 - bits as u32;
                        if key >> shift == subpart && key & 0x80000000 == 0 {
                            let triangle = (key & ((1u32 << shift) - 1)) as usize;
                            anyhow::ensure!(
                                triangle < part.indices.len() / 3,
                                "MOPP triangle out of range"
                            );
                            Some(Some(triangle))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(triangle) = triangle {
                    anyhow::ensure!(found.is_none(), "Ambiguous MOPP shape key");
                    found = Some((index, triangle));
                }
            }
            result.push(found.with_context(|| format!("Unresolved MOPP key {key:#x}"))?);
        }
        Ok(result)
    }
    /// Own and place the original motion without selecting a different mode
    /// or applying component overrides. The source asset remains immutable.
    pub fn instantiate_motion(
        &self,
        parts: &[super::collision::StaticMesh],
    ) -> anyhow::Result<super::body_motion::BodyMotion> {
        use anyhow::Context;
        let index = *self.parts.first().context("Body has no geometry parts")?;
        let part = parts.get(index).context("Body part index out of range")?;
        anyhow::ensure!(
            part.body_offset == self.body_offset,
            "Body motion owner changed"
        );
        let mut motion = part.motion.instantiate()?;
        motion.current.set_transform(self.initial_transform)?;
        Ok(motion)
    }
    /// Initialize only the swept state touched by the native setTransform.
    /// Authored velocity overrides and final motion-mode selection remain the
    /// lifecycle owner's responsibility; this does not imply motion type 4.
    pub fn initial_motion_state(&self) -> anyhow::Result<super::native_motion_step::MotionState> {
        let mut state = super::native_motion_step::MotionState {
            transform: [0.; 16],
            previous_center: [0., 0., 0., self.swept_base_time],
            center: [0.; 4],
            previous_orientation: [0.; 4],
            orientation: [0.; 4],
            local_center: self.local_center_of_mass,
            delta_angle: [0.; 4],
        };
        state.set_transform(self.initial_transform)?;
        Ok(state)
    }
    /// Install collision/query geometry at the recovered initial body pose.
    /// Returned shape IDs belong only to this body, not every body of `owner`.
    /// This does not install a Havok-equivalent motion integrator or filtering.
    /// On installation failure the caller must discard the construction world.
    pub fn install_query_geometry(
        &self,
        parts: &[super::collision::StaticMesh],
        world: &mut super::physics::Collision,
        owner: usize,
    ) -> anyhow::Result<Vec<usize>> {
        Ok(self
            .install_query_parts(parts, world, owner)?
            .into_iter()
            .map(|(shape, _)| shape)
            .collect())
    }
    fn install_query_parts(
        &self,
        parts: &[super::collision::StaticMesh],
        world: &mut super::physics::Collision,
        owner: usize,
    ) -> anyhow::Result<Vec<(usize, Option<std::ops::Range<usize>>)>> {
        use anyhow::Context;
        let rotation = self.initial_motion_state()?.orientation;
        let position = self.initial_transform[12..15].try_into().unwrap();
        let mut installed = Vec::with_capacity(self.parts.len());
        for &index in &self.parts {
            let part = parts
                .get(index)
                .context("Rigid-body part index out of range")?;
            anyhow::ensure!(
                part.body_offset == self.body_offset,
                "Rigid-body part owner changed"
            );
            let source_start = world.triangle_count();
            let shape = if part.convex {
                world.add_static_convex(&part.vertices, part.triangle_radius, owner)?
            } else {
                anyhow::ensure!(
                    part.indices.len() % 3 == 0,
                    "Incomplete rigid-body triangle"
                );
                let triangles = part
                    .indices
                    .chunks_exact(3)
                    .map(|indices| {
                        let mut p = [[0.; 3]; 3];
                        for i in 0..3 {
                            p[i] = *part
                                .vertices
                                .get(indices[i] as usize)
                                .context("Rigid-body triangle vertex out of range")?;
                        }
                        Ok(super::physics::Triangle { p })
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                world.add_static_triangles(&triangles, part.triangle_radius, owner)?;
                world
                    .owner_shapes(owner)
                    .last()
                    .context("Installed body part has no shape")?
            };
            world.set_shape_transform(shape, position, rotation)?;
            let range = (!part.convex).then_some(source_start..world.triangle_count());
            installed.push((shape, range));
        }
        Ok(installed)
    }
}

/// Prepare entity/body composition and preserve authored body encounter order.
/// Motion-mode overrides and model-node binding are subsequent lifecycle steps;
/// this does not silently turn dynamic bodies into static geometry.
pub fn layout(
    parts: &[super::collision::StaticMesh],
    entity: &super::native_hk_transform::Transform,
) -> anyhow::Result<Vec<BodyLayout>> {
    let mut result: Vec<BodyLayout> = Vec::new();
    let mut by_offset = std::collections::HashMap::new();
    for (index, part) in parts.iter().enumerate() {
        if let Some(&group) = by_offset.get(&part.body_offset) {
            let body: &mut BodyLayout = &mut result[group];
            let first = &parts[body.parts[0]];
            anyhow::ensure!(
                match (&first.mopp, &part.mopp) {
                    (None, None) => true,
                    (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                    _ => false,
                },
                "Inconsistent rigid-body MOPP ownership at {:#x}",
                part.body_offset
            );
            anyhow::ensure!(
                first.motion == part.motion
                    && first.properties == part.properties
                    && first.body_transform == part.body_transform
                    && first.material == part.material
                    && std::sync::Arc::ptr_eq(&first.bounds_graph, &part.bounds_graph)
                    && first.bounds_root == part.bounds_root
                    && first.local_center_of_mass == part.local_center_of_mass
                    && first.swept_base_time == part.swept_base_time
                    && first.encoded_motion_limits == part.encoded_motion_limits
                    && first.motion_type == part.motion_type
                    && first.body_name == part.body_name
                    && first.linear_velocity == part.linear_velocity
                    && first.angular_velocity == part.angular_velocity,
                "Inconsistent rigid-body metadata at {:#x}",
                part.body_offset
            );
            anyhow::ensure!(
                !body
                    .parts
                    .iter()
                    .any(|&i| parts[i].subpart_index == part.subpart_index),
                "Duplicate rigid-body part at {:#x}/{}",
                part.body_offset,
                part.subpart_index
            );
            body.parts.push(index);
        } else {
            by_offset.insert(part.body_offset, result.len());
            result.push(BodyLayout {
                body_offset: part.body_offset,
                parts: vec![index],
                initial_transform: super::native_hk_transform::initial_body(
                    entity,
                    &part.body_transform,
                )?,
                local_center_of_mass: part.local_center_of_mass,
                encoded_motion_limits: part.encoded_motion_limits,
                swept_base_time: part.swept_base_time,
            });
        }
    }
    Ok(result)
}

/// REL 0x3835e4..0x383648. Is Mover overrides the real/painted activation path.
pub fn requests_dynamic(is_mover: bool, is_real: bool, is_painted: bool) -> bool {
    !is_mover && (is_real || is_painted)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// Mark body user data active, set Is Mover false, request Havok motion 1.
    /// If a saved motion exists, then restore cached linear/angular velocities.
    /// Motion 1 is Havok's dynamic request; it must not overwrite the concrete
    /// serialized inertia model with a guessed Box3D equivalent.
    Dynamic { restore_velocities: bool },
    /// Mark user data inactive, save both velocities, zero both velocities,
    /// set Is Mover true, then request Havok motion 4 (keyframed), in that order.
    Keyframed,
    /// Existing keyframed/fixed motion bypasses the conversion and its cache.
    Unchanged,
}

/// REL 0x38330c. Component+0x68 holds the transformation's parent entity,
/// passed through 0x354010 / 0x383160 / 0x37f000.
/// `saved_motion` corresponds to hkpRigidBody::getStoredDynamicMotion,
/// DOL 0x802b7964: only types 4/5 can expose body+0x1f8.
pub fn transition(
    request_dynamic: bool,
    has_parent: bool,
    motion_type: u8,
    saved_motion: bool,
) -> Transition {
    let non_dynamic = matches!(motion_type, 4 | 5);
    if request_dynamic && !has_parent {
        Transition::Dynamic {
            restore_velocities: non_dynamic && saved_motion,
        }
    } else if non_dynamic {
        Transition::Unchanged
    } else {
        Transition::Keyframed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn velocity_body(mode: u8, velocity: [f32; 4]) -> PlacedBody {
        let mut record = super::super::native_motion_record::MotionRecord([0; 0x120]);
        record.0[8] = mode;
        record.set_linear_velocity(velocity);
        PlacedBody::new(
            super::super::body_motion::BodyMotion::new(record, 0, None).unwrap(),
            vec![],
            super::super::character_runtime_storage::BodyAuxiliary {
                properties: Default::default(),
                listeners: Default::default(),
                user_data: None,
            },
        )
        .unwrap()
    }
    #[test]
    fn loader_velocity_stage_samples_per_dynamic_body_but_writes_entire_system() {
        let mut bodies = vec![
            velocity_body(2, [0.; 4]),
            velocity_body(4, [0.; 4]),
            velocity_body(5, [0.; 4]),
            velocity_body(3, [0.; 4]),
        ];
        let parameters = InitialVelocity {
            minimum: [1., 0., 0.],
            maximum: [3., 0., 0.],
            minimum_speed: 0.,
        };
        let mut draws = 0;
        let mut events = Vec::new();
        for index in 0..bodies.len() {
            initialize_body_velocity(
                &mut bodies,
                index,
                &parameters,
                || {
                    draws += 1;
                    if draws == 1 {
                        0
                    } else {
                        32767
                    }
                },
                |body, b| {
                    events.push((body, b.linear_velocity()[0]));
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(draws, 2);
        assert_eq!(
            events,
            [
                (0, 0.),
                (1, 0.),
                (2, 0.),
                (3, 0.),
                (0, 1.),
                (1, 1.),
                (2, 0.),
                (3, 1.)
            ]
        );
        for index in [0, 1, 3] {
            assert_eq!(bodies[index].linear_velocity(), [3., 0., 0., 0.]);
        }
        assert_eq!(bodies[2].linear_velocity(), [0.; 4]);
    }
    #[test]
    fn component_velocity_gate_ignores_w_and_aborts_at_failed_activation() {
        let epsilon = f32::from_bits(0x3a83126f);
        let mut bodies = vec![
            velocity_body(2, [0., 0., 0., 9.]),
            velocity_body(4, [0.; 4]),
        ];
        set_system_linear_velocity(&mut bodies, [epsilon, 0., 0., 0.], |_, _| {
            panic!("Inclusive tolerance")
        })
        .unwrap();
        assert_eq!(bodies[0].linear_velocity(), [0., 0., 0., 9.]);
        let next = f32::from_bits(epsilon.to_bits() + 1);
        let mut events = Vec::new();
        assert!(
            set_system_linear_velocity(&mut bodies, [next, 0., 0., 7.], |index, body| {
                events.push((index, body.linear_velocity()));
                anyhow::ensure!(index != 1, "Activation failure");
                Ok(())
            })
            .is_err()
        );
        assert_eq!(events, [(0, [0., 0., 0., 9.]), (1, [0.; 4])]);
        assert_eq!(bodies[0].linear_velocity(), [next, 0., 0., 7.]);
        assert_eq!(bodies[1].linear_velocity(), [0.; 4]);
        assert!(initialize_body_velocity(
            &mut bodies,
            2,
            &InitialVelocity {
                minimum: [0.; 3],
                maximum: [0.; 3],
                minimum_speed: 0.
            },
            || panic!("Invalid index must not draw"),
            |_, _| panic!("Invalid index must not activate")
        )
        .is_err());
    }
    #[test]
    fn initial_velocity_preserves_axis_draw_gates_and_inverted_ranges() {
        let parameters = InitialVelocity {
            minimum: [5., 9., -2.],
            maximum: [5., 0., -4.],
            minimum_speed: 0.,
        };
        let mut calls = 0;
        assert_eq!(
            parameters
                .sample(|| {
                    calls += 1;
                    32767
                })
                .unwrap(),
            [5., 9., -4., 0.]
        );
        assert_eq!(
            calls, 2,
            "Equal nonzero bounds consume RNG; zero maximum does not"
        );
        assert!(parameters.sample(|| 32768).is_err());
        let mut invalid = parameters;
        invalid.minimum[0] = f32::NAN;
        assert!(invalid
            .sample(|| panic!("Invalid parameters must not consume RNG"))
            .is_err());
    }
    #[test]
    fn initial_velocity_minimum_speed_handles_zero_and_nonzero_directions() {
        let parameters = InitialVelocity {
            minimum: [0.; 3],
            maximum: [0.; 3],
            minimum_speed: 3.,
        };
        let v = parameters
            .sample(|| panic!("Zero maxima must not draw"))
            .unwrap();
        assert_eq!(v[0], v[1]);
        assert_eq!(v[1], v[2]);
        assert_eq!(v[3], 0.);
        assert!((v[0] * 3f32.sqrt() - 3.).abs() < 1e-6);
        let parameters = InitialVelocity {
            minimum: [-1., 0., 0.],
            maximum: [0.; 3],
            minimum_speed: 3.,
        };
        let v = parameters
            .sample(|| panic!("Zero maxima must not draw"))
            .unwrap();
        assert!((v[0] + 3.).abs() < 1e-6);
        assert_eq!(&v[1..], [0.; 3]);
    }
    #[test]
    fn original_four_lane_calculations_and_stored_writes_match() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/hk_keyframe_four_lane.json"
        ))
        .unwrap();
        assert_eq!(fixture["complete"], true);
        assert_eq!(fixture["capture_version"], 2);
        assert_eq!(
            fixture["rel_sha256"],
            "ac25d520f329a3d0507d656f0ab1cf1c34025bb36ff866187ad082ae3e0a4ebf"
        );
        let samples = fixture["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 8);
        let mut differences = Vec::new();
        for (index, sample) in samples.iter().enumerate() {
            let vector = |name: &str| -> [f32; 4] {
                sample[name]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| f32::from_bits(v.as_u64().unwrap() as u32))
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap()
            };
            let inverse = f32::from_bits(sample["inverse_delta_bits"].as_u64().unwrap() as u32);
            let origin = vector("target_origin");
            let target = vector("target");
            let center = vector("current_center");
            let local = vector("local_center");
            let current = vector("current");
            let linear = super::super::native_hk_transform::keyframe_linear_velocity4(
                origin, target, local, center, inverse,
            )
            .unwrap();
            let angular = super::super::native_hk_transform::keyframe_angular_velocity4(
                target, current, inverse,
            )
            .unwrap();
            let mut body = PlacedBody::test_body(
                super::super::native_motion_step::MotionState {
                    transform: [0.; 16],
                    previous_center: center,
                    center,
                    previous_orientation: current,
                    orientation: current,
                    local_center: local,
                    delta_angle: [0.; 4],
                },
                vector("old_linear"),
                vector("old_angular"),
                [127; 2],
                vec![],
            );
            // This harness checks stored writes; activation ordering has its
            // own policy test and is not claimed as an original event trace.
            body.update_target(origin, target, inverse, |_| Ok(()))
                .unwrap();
            for (name, actual) in [
                ("linear", linear),
                ("angular", angular),
                ("result_linear", body.linear_velocity()),
                ("result_angular", body.angular_velocity()),
            ] {
                for (lane, (a, b)) in actual.into_iter().zip(vector(name)).enumerate() {
                    if a.to_bits() != b.to_bits() {
                        differences.push(format!(
                            "sample {index} {name}[{lane}]: {:08x} != {:08x}",
                            a.to_bits(),
                            b.to_bits()
                        ));
                    }
                }
            }
        }
        assert!(differences.is_empty(), "{}", differences.join("\n"));
    }
    #[test]
    fn target_update_activates_before_each_ordered_write() {
        let layout = BodyLayout {
            body_offset: 0,
            parts: vec![],
            initial_transform: super::super::native_hk_transform::from_entity(
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                [0.; 3],
            ),
            local_center_of_mass: [0.; 4],
            swept_base_time: 0.,
            encoded_motion_limits: [127; 2],
        };
        let mut body = PlacedBody::test_body(
            layout.initial_motion_state().unwrap(),
            [0.; 4],
            [0., 0.2, 0., 9.],
            [127; 2],
            vec![],
        );
        let initial_center = body.state().center;
        let mut before = Vec::new();
        body.update_target([1., 0., 0., 7.], [0., 0., 0., 1.], 1., |b| {
            before.push((b.linear_velocity(), b.angular_velocity()));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            before,
            vec![
                ([0.; 4], [0., 0.2, 0., 9.]),
                ([1., 0., 0., 7.], [0., 0.2, 0., 9.])
            ]
        );
        assert_eq!(body.angular_velocity(), [0.; 4]);
        assert_eq!(body.state().center, initial_center);
        body.motion.current.set_angular_velocity([0., 0.2, 0., 9.]);
        let mut count = 0;
        assert!(body
            .update_target([2., 0., 0., 8.], [0., 0., 0., 1.], 1., |_| {
                count += 1;
                anyhow::ensure!(count != 2, "activation failed");
                Ok(())
            })
            .is_err());
        assert_eq!(body.linear_velocity(), [2., 0., 0., 8.]);
        assert_eq!(body.angular_velocity(), [0., 0.2, 0., 9.]);
    }
    #[test]
    fn keyframe_writes_preserve_subthreshold_motion_and_replace_entire_vector() {
        let mut state = KeyframeState {
            orientation: [0., 0., 0., 1.],
            center_of_mass: [0.; 4],
            local_center_of_mass: [0.; 4],
            linear_velocity: [0., 0., 0., 123.],
            angular_velocity: [0.; 4],
        };
        let equal = state
            .target_writes([0.001, 0., 0., 7.], state.orientation, 1.)
            .unwrap();
        assert_eq!(
            equal,
            KeyframeWrites {
                linear: None,
                angular: None
            }
        );
        let above = f32::from_bits(0.001f32.to_bits() + 1);
        let write = state
            .target_writes([above, 0.0001, -0.0001, 7.], state.orientation, 1.)
            .unwrap();
        assert_eq!(write.linear, Some([above, 0.0001, -0.0001, 7.]));
        assert_eq!(write.angular, None);
        state.linear_velocity = [0.0001, 0., 0., 123.];
        assert_eq!(
            state
                .target_writes([0.; 4], state.orientation, 1.)
                .unwrap()
                .linear,
            None
        );
        state.angular_velocity = [0., 0.002, 0., 9.];
        assert_eq!(
            state
                .target_writes([0.; 4], state.orientation, 1.)
                .unwrap()
                .angular,
            Some([0.; 4])
        );
        state.linear_velocity[0] = f32::NAN;
        assert!(state.target_writes([0.; 4], state.orientation, 1.).is_err());
    }
    #[test]
    fn mover_precedes_real_and_painted_state() {
        for real in [false, true] {
            for painted in [false, true] {
                assert!(!requests_dynamic(true, real, painted));
                assert_eq!(requests_dynamic(false, real, painted), real || painted);
            }
        }
    }
    #[test]
    fn serialized_wheel_bodies_request_keyframing() {
        // Original wheel/platform HKX resources serialize motion type 3.
        let request = requests_dynamic(true, true, true);
        assert_eq!(transition(request, false, 3, false), Transition::Keyframed);
        assert_eq!(transition(request, false, 4, true), Transition::Unchanged);
        assert_eq!(transition(request, false, 5, true), Transition::Unchanged);
    }
    #[test]
    fn restore_requires_original_saved_motion_and_unblocked_activation() {
        assert_eq!(
            transition(true, false, 3, true),
            Transition::Dynamic {
                restore_velocities: false
            }
        );
        assert_eq!(
            transition(true, false, 4, false),
            Transition::Dynamic {
                restore_velocities: false
            }
        );
        assert_eq!(
            transition(true, false, 4, true),
            Transition::Dynamic {
                restore_velocities: true
            }
        );
        assert_eq!(
            transition(true, false, 5, true),
            Transition::Dynamic {
                restore_velocities: true
            }
        );
        assert_eq!(transition(true, true, 3, false), Transition::Keyframed);
        assert_eq!(transition(true, true, 4, true), Transition::Unchanged);
    }
}
