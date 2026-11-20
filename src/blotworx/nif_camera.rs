//! Authored NIF cameras and compatible-projection native handoffs. Default
//! gameplay controllers, input steering and FOV/clip overrides remain required work.
use super::{assets::World, physics::V};
use anyhow::{ensure, Context, Result};
use serde::Serialize;
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Selection {
    pub entity: usize,
    pub node: u32,
    pub focus: Option<usize>,
    /// Generic GrabCamera uses entity property getters, without requiring a
    /// NiCamera. With no source FOV property, native output retains its lens.
    pub entity_projection: Option<Projection>,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Projection {
    pub frustum: [f32; 6],
    pub orthographic: bool,
    pub field_of_view: f32,
}
#[derive(Clone, Debug, Serialize)]
pub struct Transition {
    pub source: Selection,
    pub clock: super::camera_transition::Clock,
    /// The previous destination becomes this source's destination. Retain its
    /// outgoing controller too: interruption does not freeze the old blend.
    pub source_transition: Option<Box<Transition>>,
    /// Native1d9a60 composes output before1db12c promotes completed children.
    /// Retain that output for the current update even after child promotion.
    pub source_output: Option<Frame>,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Frame {
    pub eye: V,
    pub left: V,
    pub direction: V,
    pub up: V,
    pub frustum: [f32; 6],
    pub orthographic: bool,
    pub native_lens: Option<super::camera_lens::Sample>,
}
impl Frame {
    pub(super) fn from_information(
        information: super::camera_base::Information,
        lens: super::camera_lens::Sample,
    ) -> Result<Self> {
        let matrix = information.transform;
        ensure!(
            matrix.iter().all(|v| v.is_finite()),
            "Nonfinite published camera transform is unsupported"
        );
        let mut frame = Self {
            eye: matrix[12..15].try_into().unwrap(),
            left: matrix[0..3].try_into().unwrap(),
            up: matrix[4..7].try_into().unwrap(),
            direction: matrix[8..11].try_into().unwrap(),
            frustum: [0.; 6],
            orthographic: false,
            native_lens: None,
        };
        frame.set_native_lens(lens)?;
        Ok(frame)
    }
    /// Controller information before application aspect/letterbox publication.
    /// NIF writes transform/raw FOV and retains the caller's focus lanes.
    pub(super) fn information(self, focus: [f32; 4]) -> Result<super::camera_base::Information> {
        let fov = self
            .native_lens
            .context("Raw inspection camera has no native controller FOV")?
            .field_of_view;
        ensure!(
            self.eye
                .iter()
                .chain(&self.left)
                .chain(&self.up)
                .chain(&self.direction)
                .all(|v| v.is_finite())
                && fov.is_finite(),
            "Nonfinite authored camera information is unsupported"
        );
        let rotation = [
            self.left[0],
            self.up[0],
            self.direction[0],
            self.left[1],
            self.up[1],
            self.direction[1],
            self.left[2],
            self.up[2],
            self.direction[2],
        ];
        Ok(super::camera_base::Information {
            transform: super::native_hk_transform::from_entity(rotation, self.eye),
            field_of_view: fov,
            focus,
        })
    }
    fn from_entity(rotation: [f32; 9], position: V, projection: Projection) -> Result<Self> {
        //1e485c reads the unscaled property rotation/position;23bd24 builds
        // the column transform, then8014c0cc round-trips through a normalized
        // quaternion (8014d174 ->8014bdd4), preserving translation.
        let mut matrix = super::native_hk_transform::from_entity(rotation, position);
        let rotation = super::native_hk_transform::rotation_quaternion(&matrix)?;
        super::native_hk_transform::set_rotation(&mut matrix, rotation);
        Ok(Self {
            eye: position,
            left: [matrix[0], matrix[1], matrix[2]],
            up: [matrix[4], matrix[5], matrix[6]],
            direction: [matrix[8], matrix[9], matrix[10]],
            frustum: projection.frustum,
            orthographic: projection.orthographic,
            native_lens: None,
        })
    }
    fn blend(self, destination: Self, weight: f32) -> Result<Self> {
        let lens = match (self.native_lens, destination.native_lens) {
            (Some(source), Some(target)) => Some(source.blend(target, weight)),
            (None, None) => {
                ensure!(
                    self.frustum == destination.frustum
                        && self.orthographic == destination.orthographic,
                    "Raw camera projections cannot be blended without an application lens"
                );
                None
            }
            _ => anyhow::bail!("Cannot mix raw and application-owned camera output"),
        };
        let rotation = |frame: Self| {
            let column0 = frame.left;
            let matrix = [
                column0[0],
                column0[1],
                column0[2],
                0.,
                frame.up[0],
                frame.up[1],
                frame.up[2],
                0.,
                frame.direction[0],
                frame.direction[1],
                frame.direction[2],
                0.,
                0.,
                0.,
                0.,
                0.,
            ];
            super::native_hk_transform::rotation_quaternion_raw(&matrix)
        };
        let q =
            super::camera_transition::rotation(rotation(self)?, rotation(destination)?, weight)?;
        let mut matrix = [0.; 16];
        super::native_hk_transform::set_rotation(&mut matrix, q);
        let mut frame = Self {
            eye: std::array::from_fn(|i| {
                destination.eye[i].mul_add(weight, self.eye[i] * (1. - weight))
            }),
            left: [matrix[0], matrix[1], matrix[2]],
            direction: [matrix[8], matrix[9], matrix[10]],
            up: [matrix[4], matrix[5], matrix[6]],
            ..destination
        };
        if let Some(lens) = lens {
            frame.set_native_lens(lens)?;
        }
        Ok(frame)
    }
    fn set_native_lens(&mut self, lens: super::camera_lens::Sample) -> Result<()> {
        self.frustum = lens.frustum()?;
        self.orthographic = lens.properties.orthographic;
        self.native_lens = Some(lens);
        Ok(())
    }
    pub(super) fn from_transform(
        transform: &epic_mickey_lib_rs::nif_file::NiTransform,
        frustum: [f32; 6],
        orthographic: bool,
    ) -> Self {
        let r = &transform.rotation.rows;
        // EM1 DOL8044d1d4..d1f4 negates column0 for screen right;
        // d218..d22c supplies column2 to clip depth and homogeneous W.
        // This differs from the generic 2.6 SDK's column0 direction.
        Self {
            eye: [
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ],
            left: [r[0][0], r[1][0], r[2][0]],
            direction: [r[0][2], r[1][2], r[2][2]],
            up: [r[0][1], r[1][1], r[2][1]],
            frustum,
            orthographic,
            native_lens: None,
        }
    }
    pub fn view(self) -> [f32; 16] {
        //8044d1d4..254 copies all three axes. Reconstructing left from
        // up/direction, or normalizing via look_at, changes native output.
        let right = self.left.map(|v| -v);
        let dot =
            |axis: V| axis[0].mul_add(self.eye[0], axis[1] * self.eye[1]) + axis[2] * self.eye[2];
        [
            right[0],
            self.up[0],
            -self.direction[0],
            0.,
            right[1],
            self.up[1],
            -self.direction[1],
            0.,
            right[2],
            self.up[2],
            -self.direction[2],
            0.,
            -dot(right),
            -dot(self.up),
            dot(self.direction),
            1.,
        ]
    }
    /// The renderer fills the host viewport; keep the authored frustum and
    /// native axes for Cursor3D rather than inventing a symmetric orbit lens.
    pub fn cursor_camera(self) -> super::cursor::Camera {
        super::cursor::Camera {
            position: self.eye,
            left: self.left,
            up: self.up,
            forward: self.direction,
            viewport: super::cursor::Rect {
                left: 0.,
                right: 1.,
                top: 1.,
                bottom: 0.,
            },
            frustum: super::cursor::Rect {
                left: self.frustum[0],
                right: self.frustum[1],
                top: self.frustum[2],
                bottom: self.frustum[3],
            },
            orthographic: self.orthographic,
        }
    }
    pub fn ear_camera(self) -> super::ears::EarTransform {
        super::ears::EarTransform {
            position: self.eye,
            rotation: std::array::from_fn(|r| [self.left[r], self.up[r], self.direction[r]]),
        }
    }
    /// Preserve the authored camera's asymmetric frustum. Application-level
    /// FOV scaling/viewport adaptation are not silently invented here.
    pub fn projection(self) -> Result<[f32; 16]> {
        super::camera_projection::matrix(self.frustum, self.orthographic)
    }
}
impl World {
    pub(super) fn select_nif_camera(
        &mut self,
        entity: Option<usize>,
        focus: Option<usize>,
        mode: i32,
        duration: f32,
    ) -> Result<()> {
        ensure!(
            duration.is_finite(),
            "Camera handoff duration must be finite"
        );
        let mode = if duration == 0. { 0 } else { mode };
        ensure!(matches!(mode, 0 | 1 | 3), "Camera transition mode {mode} is not connected");
        let Some(entity) = entity else {
            ensure!(
                mode == 0,
                "Native default gameplay camera controller required for timed release"
            );
            self.nif_camera = None;
            self.nif_camera_transition = None;
            self.nif_camera_completed_frame = None;
            return Ok(());
        };
        // Native ccb4 substitutes application+104 (MickeyTracker), rather
        // than looking at Mickey's mesh bound or raw entity position.
        let focus = if focus == Some(self.player_entity) {
            Some(
                self.mickey_tracker
                    .as_ref()
                    .context("MickeyTracker entity is unavailable")?
                    .owner,
            )
        } else {
            focus
        };
        ensure!(
            !self.removed_entities.contains(&entity),
            "NIF camera source was retired"
        );
        let graph = self
            .entity_bound_hierarchy(entity)?
            .context("NIF camera source model unavailable")?;
        // Native1e4a40 visits the root and children in order, overwriting the
        // retained camera for each match. The last match is retained.
        let mut found = None;
        let mut pending: Vec<_> = graph.roots.iter().rev().flatten().copied().collect();
        while let Some(id) = pending.pop() {
            if graph.cameras.contains_key(&id) {
                found = Some(id);
            }
            pending.extend(graph.nodes[&id].children.iter().rev().flatten().copied());
        }
        let node = found.context("Source model has no supported NiCamera")?;
        let destination = Selection {
            entity,
            node,
            focus,
            entity_projection: None,
        };
        let frame = self.sample_nif_camera(destination)?;
        frame.projection()?;
        let transition = if mode != 0 {
            let source = self.nif_camera.context(
                "Native default gameplay camera controller required for timed camera acquisition",
            )?;
            self.active_nif_camera_frame()?
                .context("Outgoing NIF camera output missing")?
                .blend(frame, 0.)?;
            let ease = if mode == 3 { 0.2 } else { 0. };
            let clock = super::camera_transition::Clock::new(duration, ease, ease)?;
            Some(Transition {
                source,
                clock,
                source_transition: self.nif_camera_transition.take().map(Box::new),
                source_output: None,
            })
        } else {
            None
        };
        self.demo_story.owns_grabbed_camera = false;
        self.nif_camera = Some(destination);
        self.nif_camera_transition = transition;
        self.nif_camera_completed_frame = None;
        Ok(())
    }
    /// Lua f7a4 -> cb90 passes use-NIF=false to 1e4888. This is a
    /// transform-property camera, distinct from the NiCamera resource path.
    pub(super) fn select_entity_camera(
        &mut self,
        entity: Option<usize>,
        focus: Option<usize>,
        mode: i32,
        duration: f32,
    ) -> Result<()> {
        ensure!(
            duration.is_finite(),
            "Camera handoff duration must be finite"
        );
        if entity.is_none() && self.demo_story.enabled {
            // Functional demo return to its player camera, matching the demo's
            // ReleaseCamera path. Native timed default-controller blending is
            // still unavailable; do not abort the remaining sequence events.
            self.demo_story.camera_override = None;
            self.clear_grabbed_camera();
            return Ok(());
        }
        let entity = entity.context(
            "GrabCamera default-controller return requires the unfinished native gameplay camera",
        )?;
        self.validate_entity_camera(entity)?;
        let focus = if focus == Some(self.player_entity) {
            Some(
                self.mickey_tracker
                    .as_ref()
                    .context("MickeyTracker entity is unavailable")?
                    .owner,
            )
        } else {
            focus
        };
        let outgoing = self.active_nif_camera_frame()?.or(
            if self.demo_story.enabled && self.nif_camera.is_none() {
                self.published_camera_frame()?
            } else {
                None
            },
        );
        let inherited = outgoing
            .and_then(|frame| frame.native_lens)
            .map_or(self.camera_lens.field_of_view, |lens| lens.field_of_view);
        let application = self.camera_lens.sample(inherited);
        let destination = Selection {
            entity,
            node: 0,
            focus,
            entity_projection: Some(Projection {
                frustum: application.frustum()?,
                orthographic: application.properties.orthographic,
                field_of_view: inherited,
            }),
        };
        let frame = self.sample_nif_camera(destination)?;
        frame.projection()?;
        // cbe4 forces mode0 for zero duration. Only modes1/3 instantiate a
        // blender for non-null sources; other modes cut to the new controller.
        let transition = if duration != 0.
            && matches!(mode, 1 | 3)
            && (!self.demo_story.enabled || outgoing.is_some())
        {
            let source = self
                .nif_camera
                .or_else(|| self.demo_story.enabled.then_some(destination))
                .context("Outgoing camera controller missing")?;
            outgoing
                .context("Native default camera pose is required for timed acquisition")?
                .blend(frame, 0.)?;
            let ease = if mode == 3 { 0.2 } else { 0. };
            Some(Transition {
                source,
                clock: super::camera_transition::Clock::new(duration, ease, ease)?,
                source_transition: self.nif_camera_transition.take().map(Box::new),
                source_output: if self.nif_camera.is_none() {
                    outgoing
                } else {
                    None
                },
            })
        } else {
            None
        };
        self.demo_story.owns_grabbed_camera = false;
        self.nif_camera = Some(destination);
        self.nif_camera_transition = transition;
        self.nif_camera_completed_frame = None;
        Ok(())
    }
    pub(super) fn clear_grabbed_camera(&mut self) {
        self.demo_story.owns_grabbed_camera = false;
        self.nif_camera = None;
        self.nif_camera_transition = None;
        self.nif_camera_completed_frame = None;
    }

    fn validate_entity_camera(&self, entity: usize) -> Result<()> {
        let source = self
            .entities
            .get(entity)
            .context("Entity camera source is not owned")?;
        ensure!(
            !self.removed_entities.contains(&entity),
            "Entity camera source was retired"
        );
        if source.transform_parent_id()?.is_some() {
            ensure!(
                self.transform_hierarchy
                    .get(&entity)
                    .is_some_and(|link| !self.removed_entities.contains(&link.parent)),
                "Entity camera {} has an unresolved transform parent",
                source.name
            );
        }
        ensure!(
            source
                .components
                .iter()
                .filter(|component| component.class_name == "JPSTransformationComponent")
                .count()
                == 1,
            "Entity camera requires one resolved transform component"
        );
        ensure!(
            source
                .components
                .iter()
                .filter(|component| component.class_name == "JPSCameraComponent")
                .count()
                <= 1,
            "Generic camera has multiple unresolved lens components"
        );
        Ok(())
    }
    fn sample_nif_camera_controller(
        &self,
        selection: Selection,
        transition: Option<&Transition>,
    ) -> Result<Frame> {
        let destination = self.sample_nif_camera(selection)?;
        if let Some(transition) = transition {
            let source = match transition.source_output {
                Some(frame) => frame,
                None => self.sample_nif_camera_controller(
                    transition.source,
                    transition.source_transition.as_deref(),
                )?,
            };
            source.blend(destination, transition.clock.weight)
        } else {
            Ok(destination)
        }
    }
    pub(super) fn active_nif_camera_frame(&self) -> Result<Option<Frame>> {
        let Some(selection) = self.nif_camera else {
            return Ok(None);
        };
        if let Some(mut frame) = self.nif_camera_completed_frame {
            // Pose/FOV publication is retained for this update; renderer
            // projection changes are independent (including a paused resize).
            if let Some(lens) = frame.native_lens {
                frame.set_native_lens(self.camera_lens.sample(lens.field_of_view))?;
            }
            return Ok(Some(frame));
        }
        self.sample_nif_camera_controller(selection, self.nif_camera_transition.as_ref())
            .map(Some)
    }
    pub(super) fn advance_nif_camera_transition(&mut self, delta: f32) -> Result<()> {
        // A completed root was promoted after the previous output publication.
        self.nif_camera_completed_frame = None;
        fn advance(world: &World, transition: &mut Transition, delta: f32) -> Result<()> {
            transition.clock.advance(delta)?; // native parent clock first
            transition.source_output = None;
            if let Some(child) = &mut transition.source_transition {
                advance(world, child, delta)?;
                transition.source_output =
                    Some(world.sample_nif_camera_controller(transition.source, Some(child))?);
                if child.clock.complete() {
                    transition.source_transition = None;
                }
            }
            Ok(())
        }
        if let Some(mut transition) = self.nif_camera_transition.take() {
            let result = (|| -> Result<Option<Frame>> {
                advance(self, &mut transition, delta)?;
                if transition.clock.complete() {
                    let destination = self
                        .nif_camera
                        .context("Active blend has no NIF destination")?;
                    Ok(Some(self.sample_nif_camera_controller(
                        destination,
                        Some(&transition),
                    )?))
                } else {
                    Ok(None)
                }
            })();
            match result {
                Ok(Some(frame)) => self.nif_camera_completed_frame = Some(frame),
                Ok(None) => self.nif_camera_transition = Some(transition),
                Err(error) => {
                    self.nif_camera_transition = Some(transition);
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    pub(super) fn sample_nif_camera(&self, selection: Selection) -> Result<Frame> {
        let mut frame = if let Some(projection) = selection.entity_projection {
            self.validate_entity_camera(selection.entity)?;
            let source = &self.entities[selection.entity];
            // The transform hierarchy publishes the composed world pose into
            // these fields whenever the parent moves.
            Frame::from_entity(
                source.runtime_rotation,
                source.transform[12..15].try_into()?,
                projection,
            )?
        } else {
            self.sample_nif_node(selection)?
        };
        let fov = if let Some(projection) = selection.entity_projection {
            self.entities[selection.entity]
                .components
                .iter()
                .find(|component| component.class_name == "JPSCameraComponent")
                .map(|component| {
                    component
                        .properties
                        .iter()
                        .find(|property| property.name == "Field of View")
                        .and_then(|property| property.value.as_f64())
                        .map(|value| value as f32)
                        .context("Generic source camera has no resolved FOV getter")
                })
                .transpose()?
                .unwrap_or(projection.field_of_view)
        } else {
            super::camera_lens::source_fov(frame.frustum[1])?
        };
        frame.set_native_lens(self.camera_lens.sample(fov))?;
        if let Some(focus) = selection.focus {
            if let Some(target) = self.camera_focus_point(focus)? {
                if let Some(basis) = super::camera_focus::orientation(frame.eye, target)? {
                    frame.direction = basis.direction;
                    frame.up = basis.up;
                    frame.left = basis.left;
                }
            }
        }
        Ok(frame)
    }
    fn sample_nif_node(&self, selection: Selection) -> Result<Frame> {
        ensure!(
            !self.removed_entities.contains(&selection.entity),
            "Active NIF camera source was retired"
        );
        let entity = &self.entities[selection.entity];
        let graph = self
            .entity_bound_hierarchy(selection.entity)?
            .context("NIF camera hierarchy missing")?;
        let camera = graph
            .cameras
            .get(&selection.node)
            .context("Selected NiCamera disappeared")?;
        let scale = entity.authored_scale;
        ensure!(
            scale.is_finite() && scale > 0.,
            "Invalid camera entity scale"
        );
        let parent = epic_mickey_lib_rs::nif_file::NiTransform {
            rotation: epic_mickey_lib_rs::nif_file::Matrix3 {
                rows: std::array::from_fn(|r| {
                    std::array::from_fn(|c| entity.transform[c * 4 + r] / scale)
                }),
            },
            translation: epic_mickey_lib_rs::nif_file::Vector3 {
                x: entity.transform[12],
                y: entity.transform[13],
                z: entity.transform[14],
            },
            scale,
        };
        let empty = std::collections::BTreeMap::new();
        let poses = if let Some(actor) = self.actors.get(&selection.entity) {
            ensure!(
                actor.active_id().is_none() || actor.pose.is_some(),
                "Active NIF camera actor pose is unavailable"
            );
            actor
                .pose
                .as_ref()
                .map(|pose| &pose.transforms)
                .unwrap_or(&empty)
        } else if let Some(animation) = self.model_animation.get(&selection.entity) {
            &animation.poses
        } else {
            &empty
        };
        let world = graph.base_world_transforms_with_poses(Some(&parent), poses)?;
        Ok(Frame::from_transform(
            &world[&selection.node],
            camera.frustum,
            camera.orthographic != 0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_letterbox_lua_controls_live_lens_and_preserves_raw_controller_fov() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.camera_lens.set_renderer_aspect(4. / 3.)?;
        world.run_callback_fixture("assert(select('#',SetLetterbox(false))==0); GrabCameraNif(GetEntity('gremlinvillage_zonea.seq_projector 01.ExitProjectorCamera'),nil,1,0)")?;
        let before = world
            .active_nif_camera_frame()?
            .context("Missing original camera")?;
        let raw = before.native_lens.unwrap().field_of_view;
        world.run_callback_fixture("assert(select('#',SetLetterbox('TrUe'))==0)")?;
        let after = world.active_nif_camera_frame()?.unwrap();
        assert!(world.camera_lens.letterbox);
        assert_ne!(after.frustum, before.frustum);
        assert_eq!(
            after.native_lens.unwrap().field_of_view.to_bits(),
            raw.to_bits()
        );
        assert_eq!(
            world.active_nif_camera_frame()?.unwrap().frustum,
            after.frustum
        );
        for (expression, expected) in [
            ("0", false),
            ("'0'", false),
            ("nil", false),
            ("'FALSE'", false),
            ("''", true),
            ("{}", true),
            ("-1", true),
            ("false", false),
            ("true", true),
        ] {
            world.run_callback_fixture(&format!(
                "assert(select('#',SetLetterbox({expression}))==0)"
            ))?;
            assert_eq!(world.camera_lens.letterbox, expected, "{expression}");
        }
        world.camera_lens.set_renderer_aspect(16. / 9.)?;
        world.run_callback_fixture("SetLetterbox(false)")?;
        assert!(
            world.camera_lens.letterbox,
            "Wide aspect must reject clearing an already active mode"
        );
        world.camera_lens.set_renderer_aspect(4. / 3.)?;
        world.run_callback_fixture("SetLetterbox(false); assert(pcall(SetLetterbox)==false); assert(pcall(SetLetterbox,true,false)==false)")?;
        assert!(!world.camera_lens.letterbox);
        assert_eq!(
            world
                .active_nif_camera_frame()?
                .unwrap()
                .native_lens
                .unwrap()
                .field_of_view
                .to_bits(),
            raw.to_bits()
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_jps_camera_uses_live_fov_and_application_clip_planes_during_handoff() -> Result<()>
    {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.camera_lens.set_renderer_aspect(16. / 9.)?;
        assert_eq!(world.camera_lens.properties.near, 0.25);
        assert_eq!(world.camera_lens.properties.far, 400.);
        world.run_callback_fixture("GrabCameraNif(GetEntity('gremlinvillage_zonea.seq_projector 01.ExitProjectorCamera'),nil,1,0)")?;
        let source = world
            .active_nif_camera_frame()?
            .unwrap()
            .native_lens
            .unwrap()
            .field_of_view;
        let name = "gremlinvillage_zonea.CameraControlPoint 12";
        let entity = world
            .scripts
            .resolve_name(name)
            .context("Original JPS camera source missing")?;
        world.run_callback_fixture(&format!("GrabCamera(GetEntity({name:?}),nil,1,2)"))?;
        let selection = world.nif_camera.unwrap();
        assert_eq!(selection.entity, entity);
        let frame = world.sample_nif_camera(selection)?;
        assert_eq!(frame.native_lens.unwrap().field_of_view, 60.);
        assert_eq!(
            frame.frustum[4..],
            [0.25, 400.],
            "Source marker clips must not replace the application's lens"
        );
        world.advance_nif_camera_transition(1.)?;
        let mixed = world.active_nif_camera_frame()?.unwrap();
        assert_eq!(
            mixed.native_lens.unwrap().field_of_view,
            0.5f32.mul_add(60. - source, source)
        );
        let camera = world.entities[entity]
            .components
            .iter_mut()
            .find(|c| c.class_name == "JPSCameraComponent")
            .unwrap();
        camera
            .properties
            .iter_mut()
            .find(|property| property.name == "Field of View")
            .unwrap()
            .value = serde_json::json!(90.);
        let live = world.active_nif_camera_frame()?.unwrap();
        assert_eq!(
            live.native_lens.unwrap().field_of_view,
            0.5f32.mul_add(90. - source, source)
        );
        world.camera_lens.set_renderer_aspect(4. / 3.)?;
        let resized = world.active_nif_camera_frame()?.unwrap();
        assert_eq!(resized.native_lens.unwrap().properties.aspect, 4. / 3.);
        assert_eq!(resized.frustum[4..], [0.25, 400.]);
        assert_ne!(resized.frustum[1], live.frustum[1]);
        world.advance_nif_camera_transition(2.)?;
        assert!(world.nif_camera_completed_frame.is_some());
        let completed = world.active_nif_camera_frame()?.unwrap();
        world.camera_lens.set_renderer_aspect(1.6)?;
        let paused_resize = world.active_nif_camera_frame()?.unwrap();
        assert_eq!(paused_resize.eye, completed.eye);
        assert_eq!(paused_resize.native_lens.unwrap().properties.aspect, 1.6);
        assert_ne!(paused_resize.frustum[1], completed.frustum[1]);
        Ok(())
    }
    #[test]
    fn entity_camera_preserves_position_and_inherits_projection_with_native_rotation_roundtrip(
    ) -> Result<()> {
        let projection = Projection {
            frustum: [-0.5, 1.5, 0.75, -0.25, 1., 100.],
            orthographic: false,
            field_of_view: 60.,
        };
        let frame = Frame::from_entity(
            [0., 0., 1., 0., 1., 0., -1., 0., 0.],
            [3., 4., 5.],
            projection,
        )?;
        assert_eq!(frame.eye, [3., 4., 5.]);
        assert_eq!(frame.frustum, projection.frustum);
        // Component rotation/normalization instruction fixtures test exact bits;
        // this orthogonal smoke case permits1e-6 for its single-precision basis.
        for (actual, expected) in frame.direction.into_iter().zip([1., 0., 0.]) {
            assert!((actual - expected).abs() < 1e-6);
        }
        for (actual, expected) in frame.up.into_iter().zip([0., 1., 0.]) {
            assert!((actual - expected).abs() < 1e-6);
        }
        let view = frame.view();
        let point: [f32; 3] = std::array::from_fn(|r| {
            view[r] * 3. + view[4 + r] * 4. + view[8 + r] * 5. + view[12 + r]
        });
        assert!(point.iter().all(|value| value.abs() < 1e-6));
        frame.projection()?;
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_grab_camera_uses_live_marker_without_requiring_a_model_and_retains_interrupted_sources(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.run_callback_fixture("GrabCameraNif(GetEntity('gremlinvillage_zonea.seq_projector 01.ExitProjectorCamera'),nil,1,0)")?;
        let outgoing = world
            .active_nif_camera_frame()?
            .context("Original projector output missing")?;
        let marker_name = "gremlinvillage_zonea.CheckPointMarker 01";
        let marker = world
            .scripts
            .resolve_name(marker_name)
            .context("Original marker missing")?;
        assert!(
            world.entities[marker].model_asset.is_none(),
            "Fixture requires a source with no camera model"
        );
        let marker_position: [f32; 3] = world.entities[marker].transform[12..15].try_into()?;
        world.run_callback_fixture(&format!(
            "assert(select('#',GrabCamera(GetEntity({marker_name:?}),nil,3,2))==0)"
        ))?;
        let selection = world
            .nif_camera
            .context("Generic source was not selected")?;
        assert_eq!(selection.entity, marker);
        assert!(selection.entity_projection.is_some());
        assert_eq!(world.sample_nif_camera(selection)?.eye, marker_position);
        assert_eq!(
            world.active_nif_camera_frame()?.unwrap().frustum,
            outgoing.frustum
        );
        world.advance_nif_camera_transition(1.)?;
        let moving = [
            marker_position[0] + 5.,
            marker_position[1] + 2.,
            marker_position[2] - 3.,
        ];
        let rotation = world.entities[marker].runtime_rotation;
        world.relocate_entity(marker, moving, rotation)?;
        assert_eq!(
            world.sample_nif_camera(selection)?.eye,
            moving,
            "Source getter must be sampled live"
        );
        world.run_callback_fixture("GrabCamera(GetPlayer(),nil,1,2)")?;
        let transition = world.nif_camera_transition.as_ref().unwrap();
        assert_eq!(transition.source.entity, marker);
        assert!(
            transition.source_transition.is_some(),
            "Interrupted source blend was frozen/discarded"
        );
        world.advance_nif_camera_transition(2.)?;
        assert!(world.nif_camera_transition.is_none());
        assert_eq!(world.nif_camera.unwrap().entity, world.player_entity);
        world.run_callback_fixture(&format!("GrabCamera(GetEntity({marker_name:?}),nil,2,5)"))?;
        assert!(
            world.nif_camera_transition.is_none(),
            "Native non-blend mode must cut regardless of nonzero duration"
        );
        let retained = world.nif_camera.unwrap().entity;
        world.run_callback_fixture("assert(not pcall(GrabCamera,nil,nil,0,0))")?;
        assert_eq!(
            world.nif_camera.unwrap().entity,
            retained,
            "Unimplemented default return must not corrupt the active source"
        );
        Ok(())
    }
    #[test]
    fn camera_pose_blend_moves_eye_and_rotates_without_losing_roll() -> Result<()> {
        let a = Frame {
            eye: [0.; 3],
            left: [1., 0., 0.],
            direction: [0., 0., 1.],
            up: [0., 1., 0.],
            frustum: [-1., 1., 1., -1., 1., 10.],
            orthographic: false,
            native_lens: None,
        };
        let b = Frame {
            eye: [10., 4., -6.],
            left: [0., 0., -1.],
            direction: [1., 0., 0.],
            ..a
        };
        let middle = a.blend(b, 0.5)?;
        assert_eq!(middle.eye, [5., 2., -3.]);
        assert!((middle.direction[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((middle.direction[2] - middle.direction[0]).abs() < 1e-6);
        assert_eq!(middle.up, [0., 1., 0.]);
        let rolled = Frame {
            left: [0., 1., 0.],
            up: [-1., 0., 0.],
            ..a
        };
        let roll = a.blend(rolled, 0.5)?;
        assert!(roll.up[0] < -0.7 && roll.up[1] > 0.7);
        assert!(a
            .blend(
                Frame {
                    frustum: [-2., 2., 1., -1., 1., 10.],
                    ..b
                },
                0.5
            )
            .is_err());
        Ok(())
    }
    #[test]
    fn em1_camera_basis_matches_native_screen_right_up_and_depth() {
        use epic_mickey_lib_rs::nif_file::{Matrix3, NiTransform, Vector3};
        // A rotated camera makes the SDK and retail forward axes distinct.
        let transform = NiTransform {
            rotation: Matrix3 {
                rows: [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]],
            },
            translation: Vector3 {
                x: 3.,
                y: 4.,
                z: 5.,
            },
            scale: 1.,
        };
        let frame = Frame::from_transform(&transform, [-1., 1., 1., -1., 1., 10.], false);
        assert_eq!(frame.direction, [1., 0., 0.]);
        assert_eq!(frame.up, [0., 1., 0.]);
        let view = frame.view();
        let point = |v: [f32; 3]| -> [f32; 3] {
            std::array::from_fn(|r| {
                view[r] * v[0] + view[4 + r] * v[1] + view[8 + r] * v[2] + view[12 + r]
            })
        };
        assert_eq!(point([3., 4., 5.]), [0., 0., 0.]);
        assert_eq!(point([5., 4., 5.]), [0., 0., -2.]);
        // Right = -column0; up = column1, preserving authored roll.
        assert_eq!(point([3., 4., 7.]), [2., 0., 0.]);
        assert_eq!(point([3., 6., 5.]), [0., 2., 0.]);
    }
    #[test]
    fn stream_camera_alias_keeps_original_footer_and_runtime_parent_transform() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{
            NiCameraData, NiNodeData, NiObject, NiTransform, NifFile, Vector3,
        };
        let mut root = NiNodeData::default();
        root.base.transform.scale = 1.;
        root.base.transform.translation.x = 3.;
        root.children = vec![Some(1)];
        let mut camera = NiCameraData::default();
        camera.base.transform.scale = 1.;
        camera.base.transform.translation.y = 5.;
        let nif = NifFile {
            roots: vec![Some(0), Some(1)],
            blocks: vec![NiObject::NiNode(root), NiObject::NiCamera(camera)],
            ..Default::default()
        };
        let graph = super::super::scene_bounds::Hierarchy::load(&nif)?;
        assert_eq!(graph.stream_roots, nif.roots);
        assert_eq!(graph.roots, [Some(0)]);
        let parent = NiTransform {
            translation: Vector3 {
                x: 10.,
                y: 0.,
                z: 0.,
            },
            scale: 2.,
            ..Default::default()
        };
        let previous = std::collections::BTreeMap::from([(
            1,
            super::super::scene_bounds::Sphere {
                center: [0.; 3],
                radius: 0.25,
            },
        )]);
        let bounds = graph.static_world_bounds(Some(&parent), &previous)?;
        assert_eq!(bounds[&1].center, [16., 10., 0.]);
        assert_eq!(bounds[&1].radius, 0.25);
        let mut cycle = nif.clone();
        if let NiObject::NiNode(root) = &mut cycle.blocks[0] {
            root.children.push(Some(0));
        }
        assert!(super::super::scene_bounds::Hierarchy::load(&cycle).is_err());
        Ok(())
    }
    #[test]
    fn asymmetric_frustum_maps_near_corners_and_depth_exactly() -> Result<()> {
        let frame = Frame {
            eye: [0.; 3],
            left: [0., 0., -1.],
            direction: [1., 0., 0.],
            up: [0., 1., 0.],
            frustum: [-0.5, 1.5, 0.75, -0.25, 1., 2.],
            orthographic: false,
            native_lens: None,
        };
        let p = frame.projection()?;
        let project = |v: [f32; 3]| {
            let q: [f32; 4] = std::array::from_fn(|r| {
                p[r] * v[0] + p[4 + r] * v[1] + p[8 + r] * v[2] + p[12 + r]
            });
            [q[0] / q[3], q[1] / q[3], q[2] / q[3]]
        };
        assert_eq!(project([-0.5, -0.25, -1.]), [-1., -1., -1.]);
        assert_eq!(project([1.5, 0.75, -1.]), [1., 1., -1.]);
        assert!(
            project([0., 0., -0.5])[2] < -1.,
            "Geometry before the authored near plane was not clipped"
        );
        assert_eq!(project([0., 0., -2.])[2], 1.);
        let mut bad = frame;
        bad.frustum[5] = bad.frustum[4];
        assert!(bad.projection().is_err());
        Ok(())
    }
    #[test]
    fn aiming_and_ears_use_the_rendered_camera_axes_and_asymmetric_lens() -> Result<()> {
        let frame = Frame {
            eye: [3., 4., 5.],
            left: [0., 0., -1.],
            up: [0., 1., 0.],
            direction: [1., 0., 0.],
            frustum: [-0.5, 1.5, 0.75, -0.25, 1., 3.],
            orthographic: false,
            native_lens: None,
        };
        let ray = frame
            .cursor_camera()
            .window_ray([25, 20], [100, 80])?
            .context("Cursor ray missing")?;
        assert_eq!(ray.origin, frame.eye);
        assert_eq!(ray.direction[2], 0.);
        assert_eq!(ray.direction[1] / ray.direction[0], 0.5);
        let world: [f32; 4] = std::array::from_fn(|i| {
            if i < 3 {
                ray.origin[i] + ray.direction[i] * 5.
            } else {
                1.
            }
        });
        let multiply = |m: [f32; 16], v: [f32; 4]| -> [f32; 4] {
            std::array::from_fn(|r| {
                m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2] + m[12 + r] * v[3]
            })
        };
        let clip = multiply(frame.projection()?, multiply(frame.view(), world));
        // Four single-precision epsilons in NDC accommodate normalize,
        // world-coordinate addition and the separate view/projection products.
        // This is a host consumer-coherence check, not a retail image tolerance.
        assert!((clip[0] / clip[3] + 0.5).abs() <= 4. * f32::EPSILON);
        assert!((clip[1] / clip[3] - 0.5).abs() <= 4. * f32::EPSILON);
        let ears = frame.ear_camera();
        assert_eq!(ears.position, ray.origin);
        assert_eq!(ears.rotation, [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]]);
        let ortho = Frame {
            orthographic: true,
            ..frame
        };
        let ortho_ray = ortho
            .cursor_camera()
            .window_ray([25, 20], [100, 80])?
            .unwrap();
        assert_eq!(ortho_ray.origin, [3., 4.5, 5.]);
        assert_eq!(ortho_ray.direction, [f32::from_bits(0x3f7fffff), 0., 0.]);
        Ok(())
    }
    #[test]
    fn independent_view_axes_keep_native_rounding_at_fractional_and_large_positions() -> Result<()>
    {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_projection.json"
        ))?;
        for case in reference["view_cases"]
            .as_array()
            .context("View cases missing")?
        {
            let vector = |name: &str| -> Result<[f32; 3]> {
                let values = case[name].as_array().context("View vector missing")?;
                ensure!(values.len() == 3, "Invalid view vector");
                Ok(std::array::from_fn(|i| values[i].as_f64().unwrap() as f32))
            };
            let frame = Frame {
                eye: vector("eye")?,
                left: vector("left")?,
                up: vector("up")?,
                direction: vector("direction")?,
                frustum: [-1., 1., 1., -1., 1., 10.],
                orthographic: false,
                native_lens: None,
            };
            let expected: [u32; 16] = std::array::from_fn(|i| {
                case["native_view_matrix_bits"][i].as_u64().unwrap() as u32
            });
            assert_eq!(
                frame.view().map(f32::to_bits),
                expected,
                "Original dot/axis bits changed: {case}"
            );
        }
        Ok(())
    }
    #[test]
    fn view_and_projection_match_original_instruction_reference_cases() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_projection.json"
        ))?;
        assert_eq!(
            reference["source_sha256"],
            "994779fa2e276983efdb50f02c406568f285a82558cb5d54bbdf74651a98a920"
        );
        for case in reference["cases"]
            .as_array()
            .context("Projection cases missing")?
        {
            let vector = |name: &str| -> Result<[f32; 3]> {
                let values = case[name].as_array().context("Projection vector missing")?;
                ensure!(values.len() == 3, "Bad reference vector");
                Ok(std::array::from_fn(|i| values[i].as_f64().unwrap() as f32))
            };
            let frame = Frame {
                eye: vector("eye")?,
                left: vector("left")?,
                direction: vector("direction")?,
                up: vector("up")?,
                frustum: std::array::from_fn(|i| case["frustum"][i].as_f64().unwrap() as f32),
                orthographic: case["orthographic"]
                    .as_bool()
                    .context("Projection kind missing")?,
                native_lens: None,
            };
            let native: [f32; 16] = std::array::from_fn(|i| {
                f32::from_bits(case["native_matrix_bits"][i].as_u64().unwrap() as u32)
            });
            let projection = frame.projection()?;
            let view = frame.view();
            let native_view: [u32; 16] = std::array::from_fn(|i| {
                case["native_view_matrix_bits"][i].as_u64().unwrap() as u32
            });
            assert_eq!(
                view.map(f32::to_bits),
                native_view,
                "Native view axes/translation changed: {case}"
            );
            for point in [[-2., 0., 1.], [0., 0., 1.], [3., -2., 7.], [5., 1., -3.]] {
                let camera: [f32; 4] = std::array::from_fn(|row| {
                    view[row] * point[0]
                        + view[4 + row] * point[1]
                        + view[8 + row] * point[2]
                        + view[12 + row]
                });
                let gl: [f32; 4] = std::array::from_fn(|row| {
                    projection[row] * camera[0]
                        + projection[4 + row] * camera[1]
                        + projection[8 + row] * camera[2]
                        + projection[12 + row] * camera[3]
                });
                let mut original: [f32; 4] = std::array::from_fn(|row| {
                    native[row * 4] * point[0]
                        + native[row * 4 + 1] * point[1]
                        + native[row * 4 + 2] * point[2]
                        + native[row * 4 + 3]
                });
                original[2] = original[2] * 2. - original[3];
                assert_eq!(
                    gl, original,
                    "Native clip mismatch, orthographic={}, eye={:?}, point={point:?}",
                    frame.orthographic, frame.eye
                );
            }
        }
        Ok(())
    }
}
