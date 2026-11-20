//! Application information and base/wrapper state. Authored controllers publish
//! here; the derived gameplay generator/physics backend remain unconnected.
use super::{
    camera_base::{CachedInformation, Fov, Information, ProfileState},
    camera_catalog::Catalog,
    camera_wrapper::Gameplay,
};
use anyhow::{ensure, Context, Result};
// Private controller-owner namespace, never scene entity indices.
const BASE_CONTROLLER: usize = 0;
pub struct Runtime {
    pub profiles: Vec<ProfileState>,
    pub fov: Fov,
    pub wrapper: Gameplay,
    pub application: CachedInformation,
    pub input: super::camera_generator_input::State,
    pub derived: Option<super::camera_constructor::Owner>,
    pub publications: u64,
    pub last_authored: Option<super::nif_camera::Selection>,
}
impl Runtime {
    pub fn new(
        catalog: &Catalog,
        settings: &std::collections::BTreeMap<String, String>,
        unscaled_counter: u64,
    ) -> Result<Self> {
        ensure!(
            catalog.extras.len() == catalog.controller.library.profiles.len(),
            "Camera profile storage/metadata mismatch"
        );
        let profiles = catalog
            .extras
            .iter()
            .map(|profile| ProfileState::new(profile.kind))
            .collect();
        let mut wrapper = Gameplay::default();
        // Application a3dc..a3ec: acquired application+7c child, duration=-1,
        // no supplied CameraInformation; request the first cache refresh.
        wrapper.wrapper.initialize(Some(BASE_CONTROLLER), -1., None);
        Ok(Self {
            profiles,
            fov: Fov::default(),
            wrapper,
            input: Default::default(),
            derived: Some(super::camera_constructor::Owner::new(
                settings,
                catalog,
                unscaled_counter,
            )?),
            application: CachedInformation {
                transform: None,
                field_of_view: 0.,
                focus: [0.; 4],
            },
            publications: 0,
            last_authored: None,
        })
    }
    pub fn owns_base(&self, handle: usize) -> bool {
        handle == BASE_CONTROLLER
    }
    pub fn output(&self) -> Result<Information> {
        Ok(Information {
            transform: self
                .application
                .transform
                .context("Application camera has not published its first frame")?,
            field_of_view: self.application.field_of_view,
            focus: self.application.focus,
        })
    }
    pub fn publish_authored(
        &mut self,
        frame: super::nif_camera::Frame,
        source: super::nif_camera::Selection,
    ) -> Result<()> {
        let information = frame.information(self.application.focus)?;
        self.application = information.into();
        self.last_authored = Some(source);
        self.publications = self.publications.wrapping_add(1);
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"base_profiles":self.profiles.len(),
            "published_profile_transforms":self.profiles.iter().filter(|p|p.cached.transform.is_some()).count(),
            "profile_kinds":self.profiles.iter().map(|p|p.kind).collect::<Vec<_>>(),
            "wrapper":{"has_owned_base":self.wrapper.wrapper.child.is_some_and(|h|self.owns_base(h)),
                "duration":self.wrapper.wrapper.duration,"elapsed":self.wrapper.wrapper.elapsed,
                "refresh":self.wrapper.wrapper.refresh,"has_cached_information":self.wrapper.wrapper.cached.is_some()},
            "fov":{"current":self.fov.current,"target":self.fov.target,"stiffness":self.fov.stiffness,
                "damping":self.fov.damping,"velocity":self.fov.velocity},
            "has_published_application_output":self.output().is_ok(),"application_publications":self.publications,
            "last_authored":self.last_authored,
            "application_information":self.output().ok().map(|i|serde_json::json!({"transform":i.transform,
                "raw_field_of_view":i.field_of_view,"focus_bits":i.focus.map(f32::to_bits)})),
            "derived_controller":self.derived.as_ref().map(|owner|owner.summary()),
            "generator_input":{"previous":self.input.previous,"ramp":self.input.ramp,
                "impulse":self.input.impulse,"filtered":self.input.filtered},"generator_connected":false})
    }
}
impl super::assets::World {
    /// The last simulation publication is shared by drawing and input. Live
    /// renderer/lens properties still apply independently (e.g. paused resize).
    pub(super) fn published_camera_frame(&self) -> Result<Option<super::nif_camera::Frame>> {
        // Keep the application record for a future default-controller handoff,
        // but do not present a released/retired authored owner as a live view.
        // The default generator is still explicitly unavailable.
        if self.nif_camera.is_none() {
            let frame = match self.side_scroller_frame()? {
                Some(frame) => Some(frame),
                None => self.short_spline_camera_frame()?,
            };
            return Ok(frame.map(|frame| self.script_feedback.apply(frame)));
        }
        if self.camera_pipeline.application.transform.is_none() {
            return Ok(None);
        }
        let information = self.camera_pipeline.output()?;
        super::nif_camera::Frame::from_information(
            information,
            self.camera_lens.sample(information.field_of_view),
        )
        .and_then(|mut frame| {
            if !self.cutscene_input.restricted && !self.side_scroller.enabled {
                // The active gameplay camera can still come from a level
                // control point. Use the same material/owner obstruction
                // policy as the free camera before publishing it to aim and rendering.
                let target = super::physics::add(self.script_player.position, [0., 1., 0.]);
                let offset = super::physics::sub(frame.eye, target);
                let distance = super::physics::length(offset);
                if distance > f32::EPSILON {
                    let direction = offset.map(|lane| lane / distance);
                    let clear = self.camera_preview_distance(target, direction, distance)?;
                    if clear < distance {
                        frame.eye = std::array::from_fn(|i| target[i] + direction[i] * clear);
                    }
                }
            }
            Ok(Some(self.script_feedback.apply(frame)))
        })
    }

    fn short_spline_camera_frame(&self) -> Result<Option<super::nif_camera::Frame>> {
        use super::physics::{add, cross, dot, length, mul, sub};

        let Some(owner) = self.demo_story.camera_override else { return Ok(None) };
        if self.removed_entities.contains(&owner) || !self.entity_scene_active(owner) {
            return Ok(None);
        }
        let Some(settings) = self.entities[owner].components.iter()
            .find(|component| component.class_name == "CameraOverrideComponent")
        else { return Ok(None) };
        let property = |name: &str| settings.properties.iter()
            .find(|property| property.name == name).map(|property| &property.value);
        // Straight, instantaneous two-knot rails such as the trebuchet view.
        // Longer paths and time-dependent camera modes need their own generator.
        if property("SplineTransitionTime").and_then(|value| value.as_f64()) != Some(0.) {
            return Ok(None);
        }
        let scene = &self.entities[owner].scene;
        let resolve = |id: u64| self.entities.iter().enumerate().find_map(|(index, entity)| {
            (&entity.scene == scene && u64::from(entity.id) == id && id != 0
                && !self.removed_entities.contains(&index)).then_some(entity)
        });
        let next = |component: &epic_mickey_lib_rs::scene_file::Component| {
            component.properties.iter().find(|property| property.name == "NextStopEntity")
                .and_then(|property| property.value.as_u64()).filter(|&id| id != 0)
        };
        let straight = |component: &epic_mickey_lib_rs::scene_file::Component| {
            component.class_name == "SplineKnotComponent"
                && component.properties.iter().all(|property| match property.name.as_str() {
                    "IncomingHandle" | "OutgoingHandle" =>
                        crate::render_support::scene::parse_point3(&property.value) == Some([0.; 3]),
                    "BranchPathEntity" => property.value.as_u64() == Some(0),
                    "BranchPathTarget" | "NextStopTarget" => property.value.as_str() == Some(""),
                    _ => true,
                })
        };
        let Some(first) = property("CameraSplineKnot").and_then(|value| value.as_u64())
            .and_then(resolve)
        else { return Ok(None) };
        let Some(first_knot) = first.components.iter()
            .find(|component| straight(component))
        else { return Ok(None) };
        let Some(last) = next(first_knot).and_then(resolve) else { return Ok(None) };
        if !last.components.iter().any(|component| straight(component) && next(component).is_none()) {
            return Ok(None);
        }
        let a = first.transform[12..15].try_into().unwrap();
        let b = last.transform[12..15].try_into().unwrap();
        let position = self.entities[self.player_entity].transform[12..15].try_into().unwrap();
        let target = add(position, [0., 1., 0.]);
        let edge = sub(b, a);
        let squared = dot(edge, edge);
        let phase = if squared > f32::EPSILON { (dot(sub(target, a), edge) / squared).clamp(0., 1.) } else { 0. };
        let eye = add(a, mul(edge, phase));
        let direction = sub(target, eye);
        let horizontal = cross([0., 1., 0.], direction);
        if length(horizontal) <= f32::EPSILON {
            return Ok(None);
        }
        let direction = mul(direction, length(direction).recip());
        let left = mul(horizontal, length(horizontal).recip());
        let up = cross(direction, left);
        let rotation = std::array::from_fn(|i| [left, up, direction][i / 3][i % 3]);
        let fov = self.camera_lens.field_of_view;
        super::nif_camera::Frame::from_information(
            Information {
                transform: super::native_hk_transform::from_entity(rotation, eye),
                field_of_view: fov,
                focus: [target[0], target[1], target[2], 0.],
            },
            self.camera_lens.sample(fov),
        ).map(Some)
    }
    /// Publish after model/controller and blend updates. Inspection cameras
    /// are presentation-only and never become the application source.
    pub(super) fn publish_authored_camera(&mut self) -> Result<()> {
        if let Some(frame) = self.active_nif_camera_frame()? {
            let source = self
                .nif_camera
                .context("Authored camera output has no source owner")?;
            self.camera_pipeline.publish_authored(frame, source)?;
            self.camera_lens.field_of_view = frame
                .native_lens
                .context("Authored camera native lens missing")?
                .field_of_view;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_keeps_raw_fov_focus_bits_and_rejects_invalid_frames() -> Result<()> {
        let focus = [10., 20., 30., f32::from_bits(0x7fc12345)];
        let mut state = Runtime {
            profiles: Vec::new(),
            fov: Fov::default(),
            wrapper: Gameplay::default(),
            application: CachedInformation {
                transform: None,
                field_of_view: 0.,
                focus,
            },
            input: Default::default(),
            derived: None,
            publications: 0,
            last_authored: None,
        };
        let lens = super::super::camera_lens::Sample {
            field_of_view: 60.,
            letterbox: true,
            properties: super::super::camera_lens::Properties {
                renderer_aspect: 4. / 3.,
                aspect: 4. / 3.,
                near: 0.25,
                far: 400.,
                width: 0.,
                orthographic: false,
            },
        };
        let frame = super::super::nif_camera::Frame {
            eye: [3., 4., 5.],
            left: [1., 0., -0.],
            up: [0., 1., 0.],
            direction: [0., 0., 1.],
            frustum: lens.frustum()?,
            orthographic: false,
            native_lens: Some(lens),
        };
        let source = super::super::nif_camera::Selection {
            entity: 9,
            node: 7,
            focus: None,
            entity_projection: None,
        };
        state.publish_authored(frame, source)?;
        let output = state.output()?;
        assert_eq!(output.field_of_view, 60.);
        assert_ne!(output.field_of_view, lens.published_field_of_view()?);
        assert_eq!(output.focus.map(f32::to_bits), focus.map(f32::to_bits));
        assert_eq!(
            output.transform.map(f32::to_bits),
            [1., 0., -0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 3., 4., 5., 0.].map(f32::to_bits)
        );
        let roundtrip = super::super::nif_camera::Frame::from_information(output, lens)?;
        assert_eq!(roundtrip.eye.map(f32::to_bits), frame.eye.map(f32::to_bits));
        assert_eq!(
            roundtrip.left.map(f32::to_bits),
            frame.left.map(f32::to_bits)
        );
        assert_eq!(
            roundtrip.view().map(f32::to_bits),
            frame.view().map(f32::to_bits)
        );
        assert_eq!(
            roundtrip.projection()?.map(f32::to_bits),
            frame.projection()?.map(f32::to_bits)
        );
        assert_eq!(state.publications, 1);
        assert_eq!(state.last_authored.unwrap().entity, 9);
        assert_eq!(state.fov, Fov::default());
        assert!(state.wrapper.wrapper.cached.is_none());
        assert!(state
            .publish_authored(
                super::super::nif_camera::Frame {
                    native_lens: None,
                    ..frame
                },
                source
            )
            .is_err());
        assert!(state
            .publish_authored(
                super::super::nif_camera::Frame {
                    eye: [f32::NAN, 0., 0.],
                    ..frame
                },
                source
            )
            .is_err());
        assert_eq!(state.publications, 1);
        assert_eq!(
            state.output()?.transform.map(f32::to_bits),
            output.transform.map(f32::to_bits)
        );
        assert_eq!(
            state.output()?.focus.map(f32::to_bits),
            focus.map(f32::to_bits)
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_authored_camera_updates_publish_into_application_information() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.camera_lens.set_renderer_aspect(4. / 3.)?;
        let focus = [1., 2., 3., f32::from_bits(0x80000000)];
        world.camera_pipeline.application.focus = focus;
        world.run_callback_fixture("GrabCameraNif(GetEntity('gremlinvillage_zonea.seq_projector 01.ExitProjectorCamera'),nil,1,0)")?;
        assert_eq!(
            world.camera_pipeline.publications, 0,
            "Selection is not application update/publication"
        );
        // Let the authored entrance establish its initial camera before this
        // controlled handoff; selecting during startup races its own GrabCamera.
        for tick in 1..=6 {
            world
                .advance_models_with_delta(tick as f32 * super::super::STEP, super::super::STEP)?;
        }
        fn check(world: &super::super::assets::World, focus: [f32; 4]) -> Result<[u32; 16]> {
            let frame = world
                .active_nif_camera_frame()?
                .context("Original authored camera output missing")?;
            let information = world.camera_pipeline.output()?;
            assert_eq!(
                information.transform[12..15]
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                frame.eye.map(f32::to_bits)
            );
            for (start, axis) in [(0, frame.left), (4, frame.up), (8, frame.direction)] {
                assert_eq!(
                    information.transform[start..start + 3]
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                    axis.map(f32::to_bits)
                );
                assert_eq!(information.transform[start + 3].to_bits(), 0);
            }
            assert_eq!(information.transform[15].to_bits(), 0);
            assert_eq!(
                information.field_of_view.to_bits(),
                frame.native_lens.unwrap().field_of_view.to_bits()
            );
            assert_eq!(information.focus.map(f32::to_bits), focus.map(f32::to_bits));
            assert_eq!(
                world.camera_pipeline.last_authored.unwrap().entity,
                world.nif_camera.unwrap().entity
            );
            assert_eq!(world.camera_pipeline.fov, Fov::default());
            assert!(world
                .camera_pipeline
                .profiles
                .iter()
                .all(|p| p.cached.transform.is_none()));
            assert!(world.camera_pipeline.wrapper.wrapper.refresh);
            assert!(world.camera_pipeline.wrapper.wrapper.cached.is_none());
            Ok(information.transform.map(f32::to_bits))
        }
        let before = check(&world, focus)?;
        let count = world.camera_pipeline.publications;
        let published = world
            .published_camera_frame()?
            .context("Published view absent")?;
        let original = world.active_nif_camera_frame()?.unwrap();
        assert_eq!(
            published.view().map(f32::to_bits),
            original.view().map(f32::to_bits)
        );
        assert_eq!(
            published.projection()?.map(f32::to_bits),
            original.projection()?.map(f32::to_bits)
        );
        for _ in 0..3 {
            world.active_nif_camera_frame()?;
        }
        assert_eq!(
            world.camera_pipeline.publications, count,
            "Presentation reads must not advance the camera owner"
        );
        let target = world
            .scripts
            .resolve_name("gremlinvillage_zonea.CameraControlPoint 12")
            .context("Original JPS camera absent")?;
        let entity = &world.entities[target];
        let mut position: [f32; 3] = entity.transform[12..15].try_into().unwrap();
        position[0] += 3.;
        let rotation = entity.runtime_rotation;
        world.relocate_entity(target, position, rotation)?;
        world.run_callback_fixture(
            "GrabCamera(GetEntity('gremlinvillage_zonea.CameraControlPoint 12'),nil,3,0.5)",
        )?;
        assert_eq!(
            world.camera_pipeline.output()?.transform.map(f32::to_bits),
            before
        );
        assert_eq!(
            world
                .published_camera_frame()?
                .unwrap()
                .view()
                .map(f32::to_bits),
            published.view().map(f32::to_bits),
            "Acquiring a controller must not resample the view before the next publication"
        );
        for _ in 0..5 {
            world.advance_models_with_delta(
                world.animation_time + super::super::STEP,
                super::super::STEP,
            )?;
            assert_eq!(
                world.nif_camera.unwrap().entity,
                target,
                "The controlled handoff was replaced at {}",
                world.animation_time
            );
            check(&world, focus)?;
        }
        assert_ne!(
            world.camera_pipeline.output()?.transform.map(f32::to_bits),
            before,
            "The live blend must publish new camera information"
        );
        assert_eq!(world.camera_pipeline.publications, count + 5);
        let before_resize = world.published_camera_frame()?.unwrap();
        let publications = world.camera_pipeline.publications;
        world.camera_lens.set_renderer_aspect(16. / 9.)?;
        let after_resize = world.published_camera_frame()?.unwrap();
        assert_eq!(
            before_resize.view().map(f32::to_bits),
            after_resize.view().map(f32::to_bits)
        );
        assert_ne!(
            before_resize.projection()?.map(f32::to_bits),
            after_resize.projection()?.map(f32::to_bits)
        );
        assert_eq!(
            world.camera_pipeline.publications, publications,
            "Paused resize must not advance or resample the controller"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn released_authored_camera_cannot_freeze_the_live_view() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.run_callback_fixture("GrabCameraNif(GetEntity('gremlinvillage_zonea.seq_projector 01.ExitProjectorCamera'),nil,1,0)")?;
        for tick in 1..=6 {
            world
                .advance_models_with_delta(tick as f32 * super::super::STEP, super::super::STEP)?;
        }
        assert!(world.published_camera_frame()?.is_some());
        let output = world.camera_pipeline.output()?;
        let count = world.camera_pipeline.publications;
        world.select_nif_camera(None, None, 1, 0.)?;
        assert!(
            world.published_camera_frame()?.is_none(),
            "Released camera must not remain the render/input owner"
        );
        assert_eq!(
            world.camera_pipeline.output()?,
            output,
            "Handoff must retain the last application information"
        );
        assert_eq!(world.camera_pipeline.publications, count);
        world.publish_authored_camera()?;
        assert!(world.published_camera_frame()?.is_none());
        assert_eq!(world.camera_pipeline.publications, count);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_world_owns_native_base_wrapper_state_without_inventing_output() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let state = &world.camera_pipeline;
        assert_eq!(state.profiles.len(), 13);
        for (profile, source) in state.profiles.iter().zip(&world.camera_profiles.extras) {
            assert_eq!(profile.kind, source.kind);
            assert_eq!(profile.motion, [0.; 3]);
            assert!(profile.cached.transform.is_none());
            assert_eq!(profile.cached.field_of_view, 0.);
            assert_eq!(profile.cached.focus, [0.; 4]);
            assert!(profile.control_reference.is_none());
        }
        assert_eq!(state.fov, Fov::default());
        assert!(state
            .wrapper
            .wrapper
            .child
            .is_some_and(|h| state.owns_base(h)));
        assert_eq!(state.wrapper.wrapper.duration, -1.);
        assert_eq!(state.wrapper.wrapper.elapsed, 0.);
        assert!(state.wrapper.wrapper.refresh);
        assert!(state.wrapper.wrapper.cached.is_none());
        assert!(state.output().is_err());
        assert!(state.wrapper.angles.placement.is_none());
        assert!(state.wrapper.angles.look.is_none());
        assert_eq!(state.summary()["published_profile_transforms"], 0);
        Ok(())
    }
}
