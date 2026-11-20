//! PCab4100..ab4286 ground-source selection. Subsequent target-height and
//! spring branches remain separate; this does not supply live physics data.
use super::em2_camera_tracking::{on_custom_ground, on_real_ground, GroundMotion, ResetState};
use anyhow::{ensure, Result};
#[derive(Clone, Copy)]
pub(super) struct Motion {
    pub ground: GroundMotion,
    pub surface_velocity: [f32; 3],
    pub maximum_speed: f32,
    pub inactive: bool,
}
pub(super) trait Source {
    fn target_translation(&mut self) -> Result<[f32; 3]>;
    fn referenced_translation(&mut self) -> Result<[f32; 3]>;
    fn motion_ground_point(&mut self) -> Result<[f32; 3]>;
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct Selection {
    pub floor: [f32; 3],
    pub moving_surface: bool,
    pub real_ground: bool,
}
/// held is native fieldc8; it must come from owning-component lifecycle.
/// property_a0 and custom_states are explicitly resolved source properties.
pub(super) fn select(
    tracker: &mut ResetState,
    held: &mut bool,
    motion: Option<Motion>,
    property_a0: bool,
    custom_states: &[i32],
    source: &mut impl Source,
) -> Result<Option<Selection>> {
    let Some(m) = motion else { return Ok(None) };
    ensure!(
        m.surface_velocity.iter().all(|v| v.is_finite()) && m.maximum_speed.is_finite(),
        "Nonfinite PC ground selection input"
    );
    tracker.eligible = false;
    let moving_surface = m.surface_velocity.iter().any(|v| *v != 0.);
    // Native custom-ground test precedes real ground. A matching custom state
    // intentionally prevents real_ground from becoming true in this phase.
    let mut real_ground = false;
    if on_custom_ground(tracker.flags_11c_11f[0], Some(m.ground), custom_states) {
        tracker.eligible = true;
    } else if on_real_ground(Some(m.ground)) {
        tracker.eligible = true;
        real_ground = true;
    }
    let floor = if m.maximum_speed == 0. || m.inactive {
        let floor = source.target_translation()?;
        *held = true;
        floor
    } else if *held {
        let floor = source.target_translation()?;
        *held = false;
        floor
    } else if property_a0 && m.ground.support_type == 1 && !tracker.flags_11c_11f[0] {
        let floor = source.referenced_translation()?;
        tracker.eligible = false;
        floor
    } else {
        source.motion_ground_point()?
    };
    ensure!(
        floor.iter().all(|v| v.is_finite()),
        "Nonfinite PC ground source position"
    );
    Ok(Some(Selection {
        floor,
        moving_surface,
        real_ground,
    }))
}

/// Owning fields87,ac,b0,74. Values must be initialized by the real
/// component lifecycle rather than a synthetic default for this stage.
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct TargetState {
    pub previous_real: bool,
    pub moving_surface: bool,
    pub moving_blend: f32,
    pub fall_offset: f32,
}
/// PCab4286..ab43b5. Returns the explicit field86-clear directive.
pub(super) fn target_height(
    tracker: &mut ResetState,
    state: &mut TargetState,
    selection: Selection,
    motion_state: i32,
    point_y: f32,
    up: f32,
    moving_up: f32,
    precision: super::pc_float::Precision,
) -> Result<bool> {
    ensure!(
        [
            point_y,
            up,
            moving_up,
            state.moving_blend,
            tracker.ground_target,
            selection.floor[1],
            tracker.rotated_offset[1]
        ]
        .iter()
        .all(|v| v.is_finite()),
        "Nonfinite PC ground target input"
    );
    let mut clear_aligned = false;
    if tracker.eligible {
        if selection.moving_surface {
            state.moving_surface = true;
            tracker.ground_target =
                super::pc_float::sum(tracker.rotated_offset[1], selection.floor[1], precision)?;
        } else if selection.real_ground && state.previous_real {
            tracker.ground_target =
                super::pc_float::sum(tracker.rotated_offset[1], selection.floor[1], precision)?;
        } else {
            tracker.ground_target = point_y;
        }
        state.previous_real = selection.real_ground;
        if motion_state != 1 && !tracker.flags_11c_11f[3] {
            tracker.flags_84_85 = [false; 2];
            clear_aligned = true;
        }
        if !tracker.flags_84_85[0] && !tracker.flags_84_85[1] {
            tracker.external_ground[1] = f32::MAX;
        }
    } else if selection.moving_surface && state.moving_surface {
        if tracker.ground_target < selection.floor[1] {
            tracker.ground_target =
                super::pc_float::sum(selection.floor[1], tracker.rotated_offset[1], precision)?;
        }
    } else {
        let upper = super::pc_float::sum(tracker.rotated_offset[1], point_y, precision)?;
        if tracker.ground_target > upper {
            tracker.ground_target = upper;
        }
    }
    if tracker.eligible && !selection.moving_surface {
        state.moving_surface = false;
        state.moving_blend = up;
    } else if state.moving_surface {
        // Native qword1288050 is exactly the widened float0.1 value.
        state.moving_blend =
            super::pc_float::ground_blend(moving_up, state.moving_blend, 0.1, precision)?;
    }
    if !tracker.eligible {
        state.fall_offset = f32::MAX;
    }
    Ok(clear_aligned)
}
