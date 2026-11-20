//! PC ef61a0 CameraOverrideComponent::ClearAll dispatch protocol.
//! World, ability, tracking and camera-pass receivers are mandatory. Keeping
//! this ordering separate does not claim their rendering/lifecycle is complete.
use anyhow::{ensure, Result};

pub(super) struct Request {
    pub target: mlua::Value,
    pub duration: f32,
}
impl Request {
    /// f17640 pops from the end, preserving the native -1 default. The
    /// similarly named dataset-reset API has a different default (0.5).
    pub fn parse(lua: &mlua::Lua, args: mlua::MultiValue) -> mlua::Result<Self> {
        let mut args = args.into_vec();
        let count = args.len();
        let duration = if count > 1 {
            lua.coerce_number(args.pop().unwrap())?.unwrap_or(0.) as f32
        } else { -1. };
        if !duration.is_finite() {
            return Err(mlua::Error::external("Nonfinite camera-clear duration is unsupported"));
        }
        let target = if count > 0 { args.pop().unwrap() } else { mlua::Value::Nil };
        Ok(Self { target, duration })
    }
}

#[derive(Debug, serde::Serialize)]
pub(super) struct Pending {
    pub slots: Vec<usize>,
    pub duration: f32,
    pub next_slot: usize,
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub(super) struct Input {
    pub duration: f32,
    pub player: bool,
    pub tracker: bool,
    pub hud2d: bool,
    pub faded: bool,
    pub desired_cursor: bool,
    pub look_ahead: bool,
    pub ledge_check: bool,
    pub auto_level: bool,
    pub default_fov: f32,
}

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq)]
pub(super) enum Operation {
    Hint,
    HintForce,
    HintRange,
    HintSoft,
    FocusTarget,
    FocusSpeed,
    SoftPaintingIgnore(bool),
    RestoreAbilities,
    DesiredCursor(bool),
    Hud2D(bool),
    Cursor(bool),
    Constraint2D(bool),
    GuardianVisible(bool),
    LookAhead(bool),
    LedgeValues,
    LedgeCheck(bool),
    DeactivateMode(u32),
    AutoLevel(bool),
    GroundPlane(bool),
    ExternalGround,
    ReturnToGame(f32),
    CurrentFov(f32),
    FovTransition(f32, f32),
    OverShoulder(bool),
    SoftIgnoresWrapping(bool),
}

impl Operation {
    /// Compact source-call evidence. These words describe values, not native
    /// pointers or executable calls. Boolean padding bytes are not semantic.
    pub fn native_call(self) -> (&'static str, Vec<u32>) {
        use Operation::*;
        let max = f32::MAX.to_bits();
        match self {
            Hint => ("Hint", vec![max; 5]),
            HintForce => ("HintForce", vec![max; 5]),
            HintRange => ("HintRange", vec![0; 3]),
            HintSoft => ("HintSoft", vec![0; 10]),
            FocusTarget => ("FocusTarget", vec![0, 0]),
            FocusSpeed => ("FocusSpeed", vec![max, 0]),
            SoftPaintingIgnore(v) => ("SoftPaintingIgnore", vec![v.into()]),
            RestoreAbilities => ("RestoreAbilities", vec![]),
            DesiredCursor(v) => ("DesiredCursor", vec![v.into()]),
            Hud2D(v) => ("Hud2D", vec![v.into()]),
            Cursor(v) => ("Cursor", vec![v.into()]),
            Constraint2D(v) => ("Constraint2D", vec![v.into()]),
            GuardianVisible(v) => ("GuardianVisible", vec![v.into()]),
            LookAhead(v) => ("LookAhead", vec![v.into(), max, max]),
            LedgeValues => ("LedgeValues", vec![max, max]),
            LedgeCheck(v) => ("LedgeCheck", vec![v.into()]),
            DeactivateMode(mask) => ("DeactivateMode", vec![mask]),
            AutoLevel(v) => ("AutoLevel", vec![v.into(), 0]),
            GroundPlane(v) => ("GroundPlane", vec![v.into()]),
            ExternalGround => ("ExternalGround", vec![max, 0]),
            ReturnToGame(duration) => ("ReturnToGame", vec![duration.to_bits()]),
            CurrentFov(fov) => ("CurrentFov", vec![fov.to_bits()]),
            FovTransition(fov, duration) => ("FovTransition", vec![fov.to_bits(), duration.to_bits()]),
            OverShoulder(v) => ("OverShoulder", vec![v.into()]),
            SoftIgnoresWrapping(v) => ("SoftIgnoresWrapping", vec![v.into()]),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub(super) struct Plan {
    operations: Vec<Operation>,
    cursor: usize,
}
impl Plan {
    pub fn new(input: Input) -> Result<Self> {
        ensure!(input.duration.is_finite() && input.default_fov.is_finite(), "Nonfinite camera reset input");
        use Operation::*;
        let mut operations = vec![Hint];
        if input.duration == 0. { operations.push(HintForce); }
        operations.extend([HintRange, HintSoft, FocusTarget, FocusSpeed, SoftPaintingIgnore(false), RestoreAbilities]);
        if input.player {
            if input.hud2d { operations.extend([DesiredCursor(true), Hud2D(false)]); }
            if !input.faded { operations.push(Cursor(input.hud2d || input.desired_cursor)); }
        }
        operations.extend([Constraint2D(false), GuardianVisible(true), LookAhead(input.look_ahead), LedgeValues]);
        if input.player { operations.push(LedgeCheck(input.ledge_check)); }
        operations.extend([DeactivateMode(1), AutoLevel(input.auto_level)]);
        if input.tracker { operations.extend([GroundPlane(true), ExternalGround]); }
        operations.extend([
            DeactivateMode(0x20),
            ReturnToGame(if input.duration < 0. { 0. } else { input.duration }),
            CurrentFov(input.default_fov),
            FovTransition(input.default_fov, 0.01),
            OverShoulder(true),
            SoftPaintingIgnore(false),
            SoftIgnoresWrapping(false),
        ]);
        Ok(Self { operations, cursor: 0 })
    }
    pub fn next(&self) -> Option<Operation> { self.operations.get(self.cursor).copied() }
    pub fn drain(&mut self, mut receive: impl FnMut(Operation) -> Result<()>) -> Result<()> {
        while let Some(operation) = self.next() {
            // Do not retire an operation or run later services after failure.
            receive(operation)?;
            self.cursor += 1;
        }
        Ok(())
    }
}
