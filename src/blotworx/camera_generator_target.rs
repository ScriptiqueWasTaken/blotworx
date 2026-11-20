//! Gameplay generator target acquisition dcddc..dceb4 and path samples
//! df028..df1a0. These positions are distinct from the final rendered camera.
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
pub type V = [f32; 4];

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Parameters {
    pub height_offset: f32,
    pub maximum_breadcrumbs: i32,
    pub breadcrumb_distance: f32,
}
impl Default for Parameters {
    // d1d74..d1d88 and d2054, before configuration registration/copy.
    fn default() -> Self {
        Self {
            height_offset: 0.,
            maximum_breadcrumbs: 15,
            breadcrumb_distance: 0.,
        }
    }
}
impl Parameters {
    pub fn load(settings: &BTreeMap<String, String>) -> Result<Self> {
        let float = |key: &str, default: &str| -> Result<f32> {
            let value = settings
                .get(key)
                .map(String::as_str)
                .unwrap_or(default)
                .parse::<f64>()
                .with_context(|| format!("Invalid {key}"))? as f32;
            ensure!(
                value.is_finite(),
                "Nonfinite camera generator property {key}"
            );
            Ok(value)
        };
        // d3aa8/3ab4, d3fd0/3fdc and d3ff8/4004 copy values, not pointers.
        let maximum_breadcrumbs = settings
            .get("FreeCamera_Pathing_MaxBreadcrumbs")
            .map(String::as_str)
            .unwrap_or("15")
            .parse::<i32>()
            .context("Camera MaxBreadcrumbs requires a native signed integer")?;
        Ok(Self {
            height_offset: float("FreeCamera_HeightOffset_CheckTarget", "0.7123")?,
            maximum_breadcrumbs,
            breadcrumb_distance: float("FreeCamera_Pathing_BreadCrumbDistance", "0.35123")?,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Frame {
    pub published: V,
    pub adjusted: V,
    pub unfiltered: V,
}
pub trait Source {
    fn position(&mut self) -> Result<V>;
    fn tracker(&mut self) -> Result<Option<usize>>;
    fn ground_offset(&mut self, tracker: usize) -> Result<f32>;
    ///3199b4 declines without changing its destination when TargetToTrack is
    /// absent. Return None for that actual branch, not for an unknown runtime.
    fn unfiltered_point(&mut self, tracker: usize) -> Result<Option<V>>;
}
#[derive(Debug, Default, Serialize)]
pub struct Targets {
    pub parameters: Parameters,
    pub frame: Option<Frame>,
    pub breadcrumbs: VecDeque<V>,
    pub updates: u64,
    pub error: Option<String>,
    pub source: Option<usize>,
    pub acquired_position: bool,
}
impl Targets {
    pub fn load(settings: &BTreeMap<String, String>) -> Result<Self> {
        Ok(Self {
            parameters: Parameters::load(settings)?,
            ..Default::default()
        })
    }
    pub fn acquire(&mut self, source: &mut impl Source) -> Result<()> {
        self.acquired_position = false;
        let position = source.position()?;
        ensure!(
            position.iter().all(|v| v.is_finite()),
            "Nonfinite gameplay camera target"
        );
        self.frame = Some(Frame {
            published: position,
            adjusted: position,
            unfiltered: position,
        });
        self.acquired_position = true;
        let frame = self.frame.as_mut().unwrap();
        if let Some(tracker) = source.tracker()? {
            frame.adjusted[1] += source.ground_offset(tracker)?;
            if let Some(point) = source.unfiltered_point(tracker)? {
                frame.unfiltered = point;
            }
        } else {
            frame.adjusted[1] += self.parameters.height_offset;
        }
        Ok(())
    }
    pub fn record(&mut self) -> Result<bool> {
        let position = self
            .frame
            .context("Camera path has no acquired target")?
            .adjusted;
        if let Some(last) = self.breadcrumbs.back() {
            let delta: [f32; 3] = std::array::from_fn(|i| position[i] - last[i]);
            let squared = delta[2].mul_add(delta[2], delta[0] * delta[0]) + delta[1] * delta[1];
            let threshold =
                self.parameters.breadcrumb_distance * self.parameters.breadcrumb_distance;
            if !(squared > threshold) {
                return Ok(false);
            }
        }
        self.breadcrumbs.try_reserve(1)?;
        self.breadcrumbs.push_back(position);
        let count = i32::try_from(self.breadcrumbs.len())
            .context("Native camera path signed count overflow")?;
        // Native removes ONE oldest node after admission, even if an existing
        // list already exceeds a newly supplied maximum. Nonpositive maxima
        // admit and immediately remove a sample, still returning true.
        if count > self.parameters.maximum_breadcrumbs {
            self.breadcrumbs.pop_front();
        }
        Ok(true)
    }
}

impl super::assets::World {
    pub(super) fn update_gameplay_camera_targets(&mut self, player: &super::physics::Player) {
        // Same active-default prefix gate as the existing feedback receiver.
        // This is not an implementation of the missing camera generator body.
        if self.nif_camera.is_some() {
            return;
        }
        let mut targets = std::mem::take(&mut self.gameplay_camera_targets);
        let target = usize::try_from(self.camera_profiles.controller.selection.current)
            .ok()
            .and_then(|i| self.camera_profiles.extras.get(i))
            .and_then(|profile| profile.target);
        targets.source = target;
        let result = targets.acquire(&mut WorldSource {
            world: self,
            target,
            motion_state: player.motion_state(),
        });
        // dcddc publishes300, copies2f0/310, then adjusts them in order. A
        // failing later getter retains those writes in the persistent owner.
        if targets.acquired_position {
            if let Some(frame) = targets.frame {
                self.mirror_camera_target_acquisition(frame);
            }
        }
        let result = result.and_then(|_| {
            self.update_camera_tracker_gate(); // d97bc..d97e0, after acquisition
            self.begin_owned_camera_target_tick()?; // d97e4..d97fc
            targets.record()?;
            targets.updates = targets.updates.wrapping_add(1);
            Ok(())
        });
        match result {
            Ok(()) => targets.error = None,
            Err(error) => {
                let message = format!("Gameplay camera target: {error:#}");
                if !self.report.unsupported.contains(&message) {
                    self.report.unsupported.push(message.clone());
                }
                targets.error = Some(message);
            }
        }
        self.gameplay_camera_targets = targets;
    }
}
struct WorldSource<'a> {
    world: &'a super::assets::World,
    target: Option<usize>,
    motion_state: i32,
}
impl WorldSource<'_> {
    fn owner(&self) -> Result<usize> {
        self.target
            .filter(|target| {
                *target < self.world.entities.len() && !self.world.removed_entities.contains(target)
            })
            .context("Native gameplay camera target is absent or retired")
    }
    fn runtime(&self, tracker: usize) -> Result<&super::entity_tracker::Runtime> {
        let runtime = self
            .world
            .mickey_tracker
            .as_ref()
            .filter(|runtime| runtime.owner == tracker)
            .context("EntityTrackerComponent runtime binding is absent")?;
        ensure!(
            runtime.error.is_none(),
            "Tracker update is unavailable: {}",
            runtime.error.as_deref().unwrap_or("")
        );
        Ok(runtime)
    }
}
impl Source for WorldSource<'_> {
    fn position(&mut self) -> Result<V> {
        let owner = self.owner()?;
        let entity = &self.world.entities[owner];
        ensure!(
            entity
                .components
                .iter()
                .any(|component| component.class_name == "JPSTransformationComponent"),
            "Camera target has no resolved transformation component"
        );
        if self
            .world
            .mickey_tracker
            .as_ref()
            .is_some_and(|runtime| runtime.owner == owner)
        {
            self.runtime(owner)?;
        }
        Ok(super::native_hk_transform::entity_position_vector(
            entity.transform[12..15].try_into().unwrap(),
        ))
    }
    fn tracker(&mut self) -> Result<Option<usize>> {
        let owner = self.owner()?;
        if self.world.entities[owner]
            .components
            .iter()
            .any(|component| component.class_name == "EntityTrackerComponent")
        {
            self.runtime(owner)?;
            Ok(Some(owner))
        } else {
            Ok(None)
        }
    }
    fn ground_offset(&mut self, tracker: usize) -> Result<f32> {
        Ok(self.runtime(tracker)?.state.ground_offset)
    }
    fn unfiltered_point(&mut self, tracker: usize) -> Result<Option<V>> {
        let runtime = self.runtime(tracker)?;
        let Some(target) = self
            .world
            .entities
            .get(runtime.target)
            .filter(|_| !self.world.removed_entities.contains(&runtime.target))
        else {
            return Ok(None);
        };
        let state = if runtime.target == self.world.player_entity {
            Some(self.motion_state)
        } else {
            ensure!(
                !target
                    .components
                    .iter()
                    .any(|component| component.class_name.contains("MotionController")),
                "Tracker target motion-state getter is not connected for {}",
                target.name
            );
            None
        };
        let (point, _) = runtime.parameters.target_point(
            target.transform[12..15].try_into().unwrap(),
            target.runtime_rotation,
            state,
        )?;
        Ok(Some(super::native_hk_transform::entity_position_vector(
            point,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Input {
        calls: Vec<&'static str>,
        tracker: bool,
        point: Option<V>,
        fail: bool,
        missing_position: bool,
    }
    impl Source for Input {
        fn position(&mut self) -> Result<V> {
            self.calls.push("position");
            ensure!(!self.missing_position, "Expected position failure");
            Ok([2., 3., 4., 0.])
        }
        fn tracker(&mut self) -> Result<Option<usize>> {
            self.calls.push("tracker");
            Ok(self.tracker.then_some(7))
        }
        fn ground_offset(&mut self, t: usize) -> Result<f32> {
            assert_eq!(t, 7);
            self.calls.push("height");
            Ok(0.75)
        }
        fn unfiltered_point(&mut self, t: usize) -> Result<Option<V>> {
            assert_eq!(t, 7);
            self.calls.push("unfiltered");
            if self.fail {
                anyhow::bail!("Expected target read failure");
            }
            Ok(self.point)
        }
    }
    #[test]
    fn target_views_and_partial_failure_preserve_original_publication_order() -> Result<()> {
        let mut targets = Targets::default();
        targets.parameters.height_offset = 0.7123;
        let mut input = Input {
            calls: Vec::new(),
            tracker: false,
            point: None,
            fail: false,
            missing_position: false,
        };
        targets.acquire(&mut input)?;
        assert_eq!(input.calls, ["position", "tracker"]);
        assert_eq!(targets.frame.unwrap().adjusted, [2., 3. + 0.7123, 4., 0.]);
        input.calls.clear();
        input.tracker = true;
        input.point = Some([9., 10., 11., 0.]);
        targets.acquire(&mut input)?;
        assert_eq!(input.calls, ["position", "tracker", "height", "unfiltered"]);
        assert_eq!(
            targets.frame,
            Some(Frame {
                published: [2., 3., 4., 0.],
                adjusted: [2., 3.75, 4., 0.],
                unfiltered: [9., 10., 11., 0.]
            })
        );
        input.point = None;
        targets.acquire(&mut input)?;
        assert_eq!(targets.frame.unwrap().unfiltered, [2., 3., 4., 0.]);
        input.fail = true;
        assert!(targets.acquire(&mut input).is_err());
        assert_eq!(targets.frame.unwrap().adjusted, [2., 3.75, 4., 0.]);
        assert!(targets.acquired_position);
        let old = targets.frame;
        input.missing_position = true;
        assert!(targets.acquire(&mut input).is_err());
        assert_eq!(targets.frame, old);
        assert!(
            !targets.acquired_position,
            "Failed position lookup must not republish a stale frame into the owner"
        );
        Ok(())
    }
    #[test]
    fn path_admission_uses_strict_xyz_distance_and_evicts_only_one() -> Result<()> {
        let mut targets = Targets::default();
        targets.parameters.breadcrumb_distance = 1.;
        targets.parameters.maximum_breadcrumbs = 2;
        let set = |targets: &mut Targets, point: V| {
            targets.frame = Some(Frame {
                published: point,
                adjusted: point,
                unfiltered: point,
            });
        };
        set(&mut targets, [0.; 4]);
        assert!(targets.record()?);
        set(&mut targets, [1., 0., 0., 50.]);
        assert!(!targets.record()?, "Equality and W must not admit a sample");
        set(&mut targets, [2., 0., 0., 0.]);
        assert!(targets.record()?);
        set(&mut targets, [4., 0., 0., 0.]);
        assert!(targets.record()?);
        assert_eq!(
            targets.breadcrumbs,
            VecDeque::from([[2., 0., 0., 0.], [4., 0., 0., 0.]])
        );
        targets.parameters.maximum_breadcrumbs = 0;
        set(&mut targets, [6., 0., 0., 0.]);
        assert!(targets.record()?);
        assert_eq!(targets.breadcrumbs.len(), 2);
        targets.breadcrumbs.clear();
        assert!(targets.record()?);
        assert!(targets.breadcrumbs.is_empty());
        targets.parameters.maximum_breadcrumbs = -1;
        assert!(targets.record()?);
        assert!(targets.breadcrumbs.is_empty());
        Ok(())
    }
}
