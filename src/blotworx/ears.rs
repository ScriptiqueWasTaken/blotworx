//! Mickey's recovered camera-dependent ear update and model-space overrides.
//! Live pose delivery is connected; native runtime comparison remains pending.

#[derive(serde::Serialize)]
pub struct Settings {
    pub side_tilt: f32,
    pub front_depth: f32,
    pub back_depth: f32,
    pub side_separation: f32,
    pub right_scale: [f32; 3],
    pub left_scale: [f32; 3],
    pub distance_scale: f32,
    pub yaw_mod: f32,
    pub high_enter: f32,
    pub high_exit: f32,
    pub forward_tilt: f32,
    pub max_head_pitch: f32,
    pub rotation_speed: f32,
    pub pop_angle: f32,
    pub high_depth: f32,
    pub camera_max_pitch: f32,
}
impl Settings {
    /// Config registration defaults from retail REL 0x88dd8..0x89444.
    /// Angular fields retain authored degrees; camera_max_pitch is radians.
    pub fn load(values: &std::collections::BTreeMap<String, String>) -> anyhow::Result<Self> {
        use anyhow::Context;
        let value = |name: &str, default: f32| -> anyhow::Result<f32> {
            let result = values
                .get(name)
                .map(|v| {
                    v.parse::<f32>()
                        .with_context(|| format!("Invalid ear setting {name}: {v}"))
                })
                .transpose()?
                .unwrap_or(default);
            anyhow::ensure!(result.is_finite(), "Non-finite ear setting {name}");
            Ok(result)
        };
        Ok(Self {
            side_tilt: value("EarSideTilt", 0.4)?,
            front_depth: value("EarFrontDepth", 0.2)?,
            back_depth: value("EarBackDepth", -0.2)?,
            side_separation: value("EarSideSeparation", 0.75)?,
            right_scale: [
                value("MickeyEar_Right_Scale_X", 0.85)?,
                value("MickeyEar_Right_Scale_Y", 1.)?,
                value("MickeyEar_Right_Scale_Z", 1.)?,
            ],
            left_scale: [
                value("MickeyEar_Left_Scale_X", 0.85)?,
                value("MickeyEar_Left_Scale_Y", 1.)?,
                value("MickeyEar_Left_Scale_Z", 1.)?,
            ],
            distance_scale: value("EarDistanceScale", 0.9)?,
            yaw_mod: value("EarYawMod", 92.)?,
            high_enter: value("Ears_HighAngle_Enter", 140.)?,
            high_exit: value("Ears_HighAngle_Exit", 130.)?,
            forward_tilt: value("Ears_HighAngle_ForwardTilt", 30.)?,
            max_head_pitch: value("Ears_HighAngle_HeadPitch", 45.)?,
            rotation_speed: value("Ears_HighRotationSpeed", 0.2)?,
            pop_angle: value("Ears_HighRotationPopAngle", 45.)?,
            high_depth: value("Ears_HighDepth", -0.1)?,
            camera_max_pitch: value("FreeCamera_Limits_CameraMaxPitch", 0.7123)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub tilt: f32,
    pub depth: f32,
    pub separation: f32,
}

pub struct CardinalProfile {
    tilt: [f32; 4],
    depth: [f32; 4],
    separation: [f32; 4],
}
impl CardinalProfile {
    /// REL 0x88dd8..0x88ec4 initializes these four-view tables from config.
    pub fn new(side_tilt: f32, front_depth: f32, back_depth: f32, side_separation: f32) -> Self {
        Self {
            tilt: [0., -side_tilt, 0., side_tilt],
            depth: [front_depth, 0., back_depth, 0.],
            separation: [1., side_separation, 1., side_separation],
        }
    }
    /// REL 0x8fca4..0x8fdc8. Input is camera yaw minus head yaw, in radians;
    /// negative angles receive one full turn, matching the atan2 input domain.
    pub fn sample(&self, relative_yaw: f32, head_pitch: f32, high_angle: bool) -> Profile {
        let yaw = if relative_yaw < 0. {
            relative_yaw + std::f32::consts::TAU
        } else {
            relative_yaw
        };
        let quarter = std::f32::consts::FRAC_PI_2;
        let mut result = Profile {
            tilt: 0.,
            depth: 0.,
            separation: 0.,
        };
        for i in 0..4 {
            let start = quarter * i as f32;
            let end = quarter * (i + 1) as f32;
            // The native loop uses strict inequalities, including at cardinals.
            if yaw > start && yaw < end {
                let fraction = (-(quarter)).mul_add(i as f32, yaw) / quarter;
                let complement = 1. - fraction;
                let next = (i + 1) % 4;
                result.tilt = std::f32::consts::PI
                    * complement.mul_add(self.tilt[i], fraction * self.tilt[next]);
                result.depth = complement.mul_add(self.depth[i], fraction * self.depth[next]);
                result.separation =
                    complement.mul_add(self.separation[i], fraction * self.separation[next]);
                break;
            }
        }
        if !high_angle {
            result.tilt -= head_pitch * ((yaw as f64).sin() as f32);
        }
        result
    }
}

#[derive(Default)]
pub struct HighAngle {
    pub active: bool,
    pub transitioning: bool,
}
impl HighAngle {
    /// REL 0x8fbf4..0x8fc7c. Thresholds are supplied in radians after the
    /// native degree conversion. Enter and exit comparisons are asymmetric.
    pub fn update(
        &mut self,
        view_angle: f32,
        head_pitch: f32,
        enter: f32,
        exit: f32,
        max_head_pitch: f32,
    ) {
        if view_angle < enter || head_pitch > max_head_pitch {
            if !self.active {
                self.transitioning = true;
            }
            self.active = true;
        } else if view_angle > exit {
            if self.active {
                self.transitioning = true;
            }
            self.active = false;
        }
    }

    /// REL 0x8fdd8..0x8fe50 and 0x8ff44..0x8ffb8. This is one game
    /// update, not a render-time interpolation: speed is an authored fraction.
    pub fn advance_yaw(&mut self, current: f32, target: f32, speed: f32, pop_angle: f32) -> f32 {
        if !self.transitioning {
            return target;
        }
        let distance = angle_difference(current, target).abs();
        if distance < 0.1 || distance > pop_angle {
            // Native clears the flag but retains this frame's previous yaw.
            // The following update takes the direct-target branch.
            self.transitioning = false;
            current
        } else {
            current + speed.mul_add(angle_difference(target, current), 0.)
        }
    }
}

/// REL 0x147c44. Keep both +/-PI endpoints; rem_euclid would change one.
/// Inputs are bounded atan2 results and the retained native yaw, not arbitrary
/// unbounded user angles.
fn angle_difference(a: f32, b: f32) -> f32 {
    let mut difference = a - b;
    while difference > std::f32::consts::PI {
        difference -= std::f32::consts::TAU;
    }
    while difference < -std::f32::consts::PI {
        difference += std::f32::consts::TAU;
    }
    difference
}

/// REL 0x8fe54..0x8ff3c. The native ordered branches map an unordered
/// quotient (0/0) to zero, rather than propagating it through the blend.
pub fn high_angle_depth(
    profile_depth: f32,
    high_depth: f32,
    view_angle: f32,
    view_up_angle: f32,
    enter_angle: f32,
    camera_max_pitch: f32,
) -> f32 {
    let camera_distance = angle_difference(
        std::f32::consts::FRAC_PI_2 - camera_max_pitch,
        view_up_angle,
    );
    let enter_distance = angle_difference(view_angle, enter_angle);
    let fraction = enter_distance / (camera_distance + enter_distance);
    let fraction = if !(fraction >= 0.) {
        0.
    } else if fraction > 1. {
        1.
    } else {
        fraction
    };
    fraction.mul_add(high_depth - profile_depth, profile_depth)
}

/// Row-major Gamebryo rotation. The native ear routine uses this convention,
/// independently of the renderer's column-major skin matrices.
pub type Rotation = [[f32; 3]; 3];
pub struct EarTransform {
    pub rotation: Rotation,
    pub position: [f32; 3],
}

/// Named bindings are resolved from the imported rig, never fixed bone indices.
pub struct Pose {
    head: usize,
    left: usize,
    right: usize,
    runtime: Runtime,
    dynamic: bool,
    pub overrides: Vec<(usize, [f32; 16])>,
}
impl Pose {
    pub fn new(bones: &[epic_mickey_lib_rs::hkx_file::HkxBone]) -> anyhow::Result<Self> {
        let find = |name: &str| -> anyhow::Result<usize> {
            let matches = bones
                .iter()
                .enumerate()
                .filter(|(_, b)| b.name.eq_ignore_ascii_case(name))
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            anyhow::ensure!(
                matches.len() == 1,
                "Expected one Mickey bone {name}, found {}",
                matches.len()
            );
            Ok(matches[0])
        };
        Ok(Self {
            head: find("skel_head")?,
            left: find("skel_ear_l")?,
            right: find("skel_ear_r")?,
            runtime: Runtime::default(),
            dynamic: true,
            overrides: Vec::with_capacity(2),
        })
    }
    /// REL 0x8d3f8..0x8d418, native events 0x60 and 0x61. Graph event
    /// IDs are resource-local, so the animation adapter resolves their names.
    pub fn event(&mut self, name: &str) -> bool {
        match name {
            "EVENT_Set_Ears_Dynamic" => self.dynamic = true,
            "EVENT_Set_Ears_Static" => {
                self.dynamic = false;
                self.overrides.clear();
            }
            _ => return false,
        }
        true
    }
    pub fn animation_events(
        &mut self,
        graph: &super::behavior::Document,
        events: &[super::state_machine::Event],
    ) -> anyhow::Result<()> {
        use anyhow::Context;
        let strings = graph.single("hkbBehaviorGraphStringData")?;
        let names = strings["eventNames"]
            .as_array()
            .context("Missing animation events")?;
        for event in events {
            if let Ok(index) = usize::try_from(event.id) {
                if let Some(name) = names.get(index).and_then(serde_json::Value::as_str) {
                    self.event(name);
                }
            }
        }
        Ok(())
    }
    pub fn update(
        &mut self,
        settings: &Settings,
        bones: &[epic_mickey_lib_rs::hkx_file::HkxBone],
        actor_matrix: [f32; 16],
        camera: EarTransform,
    ) -> anyhow::Result<()> {
        // REL 0x90dd8 gates the entire ear update. Retain yaw and mode while
        // authored animation owns the ears; do not restart on reactivation.
        if !self.dynamic {
            self.overrides.clear();
            return Ok(());
        }
        let model = super::skin::model_pose(bones, &[])?;
        let (actor, actor_scale) = EarTransform::from_matrix(actor_matrix)?;
        // hkaPose keeps rotation separately from scale. A skinned affine
        // matrix can contain animated anisotropic scale, so it cannot supply
        // the Ni rotation used by the ear routine. Resolve rotation-only
        // locals without changing the original animation or its positions.
        let mut rotation_bones = bones.to_vec();
        for bone in &mut rotation_bones {
            bone.local_scale = [1.; 3];
            bone.local_translation = [0.; 3];
        }
        let rotations = super::skin::model_pose(&rotation_bones, &[])?;
        let transform = |index: usize| -> anyhow::Result<EarTransform> {
            let local = *model
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("Ear rig changed during playback"))?;
            // Preserve the resolved rotation's floating-point values. Native
            // approximate quaternion normalization need not yield perfectly
            // unit matrix columns; do not mistake that for actor scale.
            let orientation =
                std::array::from_fn(|r| std::array::from_fn(|c| rotations[index][c * 4 + r]));
            let translated = rotate(actor.rotation, [local[12], local[13], local[14]]);
            Ok(EarTransform {
                rotation: product(actor.rotation, orientation),
                position: std::array::from_fn(|i| actor.position[i] + actor_scale * translated[i]),
            })
        };
        let result = self.runtime.step(
            settings,
            &transform(self.head)?,
            &camera,
            transform(self.right)?.position,
            &actor,
            actor_scale,
        )?;
        self.overrides.clear();
        self.overrides
            .extend([(self.right, result[0]), (self.left, result[1])]);
        Ok(())
    }
}

pub struct View {
    pub direction: [f32; 3],
    pub relative_yaw: f32,
    pub head_pitch: f32,
    pub head_angle: f32,
    pub up_angle: f32,
}

#[derive(Default)]
pub struct Runtime {
    pub mode: HighAngle,
    pub yaw: f32,
}
impl Runtime {
    /// One original controller update. Constructor 0x87674..0x876d8
    /// initializes the mode, transition flag and retained yaw to zero.
    /// The caller must invoke this on simulation updates, not render frames.
    pub fn step(
        &mut self,
        settings: &Settings,
        head: &EarTransform,
        camera: &EarTransform,
        animated_right_ear: [f32; 3],
        actor: &EarTransform,
        actor_scale: f32,
    ) -> anyhow::Result<[[f32; 16]; 2]> {
        let radians = |degrees: f32| std::f32::consts::PI * degrees / 180.;
        let view = View::from_transforms(head, camera, settings.forward_tilt);
        self.mode.update(
            view.head_angle,
            view.head_pitch,
            radians(settings.high_enter),
            radians(settings.high_exit),
            radians(settings.max_head_pitch),
        );
        let profile = CardinalProfile::new(
            settings.side_tilt,
            settings.front_depth,
            settings.back_depth,
            settings.side_separation,
        )
        .sample(view.relative_yaw, view.head_pitch, self.mode.active);
        let target = if self.mode.active {
            -view.relative_yaw
        } else {
            profile.tilt
        };
        self.yaw = self.mode.advance_yaw(
            self.yaw,
            target,
            settings.rotation_speed,
            radians(settings.pop_angle),
        );
        let depth = if self.mode.active {
            high_angle_depth(
                profile.depth,
                settings.high_depth,
                view.head_angle,
                view.up_angle,
                radians(settings.high_enter),
                settings.camera_max_pitch,
            )
        } else {
            profile.depth
        };
        let delta = std::array::from_fn(|i| head.position[i] - animated_right_ear[i]);
        let radius = (dot(delta, delta) as f64).sqrt() as f32 * settings.distance_scale;
        let ears = world_transforms(
            camera.rotation,
            head.position,
            view.direction,
            radius,
            self.yaw,
            depth,
            profile.separation,
            radians(settings.yaw_mod),
        );
        Ok([
            ears[0].model_matrix(
                actor.rotation,
                actor.position,
                actor_scale,
                settings.right_scale,
            )?,
            ears[1].model_matrix(
                actor.rotation,
                actor.position,
                actor_scale,
                settings.left_scale,
            )?,
        ])
    }
}
impl View {
    /// REL 0x8fa5c..0x8fcb4. Read the actual head columns: its second
    /// column supplies heading, not the engine's global up vector.
    pub fn from_transforms(head: &EarTransform, camera: &EarTransform, forward_tilt: f32) -> Self {
        let heading = [
            head.rotation[0][1],
            head.rotation[1][1],
            head.rotation[2][1],
        ];
        let horizontal =
            (heading[0].mul_add(heading[0], heading[2] * heading[2]) as f64).sqrt() as f32;
        // PPC f1 = horizontal length, f2 = heading.y at the atan2 call.
        let head_pitch =
            std::f32::consts::FRAC_PI_2 - (horizontal as f64).atan2(heading[1] as f64) as f32;
        let right = [
            head.rotation[0][0],
            head.rotation[1][0],
            head.rotation[2][0],
        ];
        let axis = [
            head.rotation[0][2],
            head.rotation[1][2],
            head.rotation[2][2],
        ];
        let angle = std::f32::consts::PI * forward_tilt / 180.;
        let tilted = rotate(rotation_axis(angle, axis), right);
        let offset = std::array::from_fn(|i| camera.position[i] - head.position[i]);
        let length = (dot(offset, offset) as f64).sqrt() as f32;
        let direction = if length > 1e-6 {
            offset.map(|v| v / length)
        } else {
            [0.; 3]
        };
        let camera_yaw =
            (-camera.rotation[0][2] as f64).atan2(-camera.rotation[2][2] as f64) as f32;
        let head_yaw = (heading[0] as f64).atan2(heading[2] as f64) as f32;
        let yaw = camera_yaw - head_yaw;
        Self {
            direction,
            relative_yaw: if yaw < 0. {
                yaw + std::f32::consts::TAU
            } else {
                yaw
            },
            head_pitch,
            head_angle: bounded_acos(dot(tilted, direction)),
            up_angle: bounded_acos(direction[1]),
        }
    }
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[2].mul_add(b[2], a[0].mul_add(b[0], a[1] * b[1]))
}
fn bounded_acos(value: f32) -> f32 {
    if value <= -1. {
        std::f32::consts::PI
    } else if value >= 1. {
        0.
    } else {
        (value as f64).acos() as f32
    }
}

impl EarTransform {
    fn from_matrix(matrix: [f32; 16]) -> anyhow::Result<(Self, f32)> {
        anyhow::ensure!(
            matrix.iter().all(|v| v.is_finite()),
            "Non-finite ear source matrix"
        );
        let scale = (matrix[0] * matrix[0] + matrix[1] * matrix[1] + matrix[2] * matrix[2]).sqrt();
        anyhow::ensure!(scale > 0., "Zero ear source scale");
        let rotation = std::array::from_fn(|r| std::array::from_fn(|c| matrix[c * 4 + r] / scale));
        // NiTransform carries a uniform scale. Reject a different contract
        // instead of silently converting shear or anisotropic actor scaling.
        for col in 0..3 {
            let length = (0..3)
                .map(|r| rotation[r][col] * rotation[r][col])
                .sum::<f32>();
            anyhow::ensure!(
                (length - 1.).abs() < 1e-4,
                "Nonuniform ear source transform"
            );
        }
        Ok((
            Self {
                rotation,
                position: [matrix[12], matrix[13], matrix[14]],
            },
            scale,
        ))
    }
    /// REL 0x908bc..0x90d40 composes inverse actor * ear, converts the
    /// resulting rotation/translation, then replaces scale with the authored
    /// per-ear vector. Translation conversion 0x23b970 performs no unit change.
    /// The renderer consumes the equivalent column-major affine matrix.
    pub fn model_matrix(
        &self,
        actor_rotation: Rotation,
        actor_position: [f32; 3],
        actor_scale: f32,
        ear_scale: [f32; 3],
    ) -> anyhow::Result<[f32; 16]> {
        anyhow::ensure!(
            actor_scale.is_finite() && actor_scale != 0.,
            "Invalid ear actor scale"
        );
        anyhow::ensure!(
            actor_rotation
                .iter()
                .flatten()
                .chain(&actor_position)
                .chain(&ear_scale)
                .chain(self.rotation.iter().flatten())
                .chain(&self.position)
                .all(|v| v.is_finite()),
            "Non-finite ear transform"
        );
        let inverse = transpose(actor_rotation);
        let rotation = product(inverse, self.rotation);
        // Match inverse-transform construction and composition order rather
        // than subtracting world positions before rotation.
        let inverse_scale = 1. / actor_scale;
        let inverse_position = rotate(inverse, actor_position).map(|v| -v * inverse_scale);
        let rotated_position = rotate(inverse, self.position);
        let position = std::array::from_fn::<_, 3, _>(|i| {
            inverse_position[i] + rotated_position[i] * inverse_scale
        });
        Ok(std::array::from_fn(|i| {
            let row = i % 4;
            let col = i / 4;
            if col == 3 {
                if row == 3 {
                    1.
                } else {
                    position[row]
                }
            } else if row == 3 {
                0.
            } else {
                rotation[row][col] * ear_scale[col]
            }
        }))
    }
}

fn product(a: Rotation, b: Rotation) -> Rotation {
    std::array::from_fn(|row| {
        std::array::from_fn(|col| {
            a[row][2].mul_add(
                b[2][col],
                a[row][0].mul_add(b[0][col], a[row][1] * b[1][col]),
            )
        })
    })
}
fn rotate(a: Rotation, b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| a[row][2].mul_add(b[2], a[row][0].mul_add(b[0], a[row][1] * b[1])))
}
fn transpose(a: Rotation) -> Rotation {
    std::array::from_fn(|r| std::array::from_fn(|c| a[c][r]))
}
fn rotation_z(angle: f32) -> Rotation {
    let s = (angle as f64).sin() as f32;
    let c = (angle as f64).cos() as f32;
    [[c, s, 0.], [-s, c, 0.], [0., 0., 1.]]
}
fn rotation_x(angle: f32) -> Rotation {
    let s = (angle as f64).sin() as f32;
    let c = (angle as f64).cos() as f32;
    [[1., 0., 0.], [0., c, s], [0., -s, c]]
}
/// DOL 0x804c4b30: the input axis is already a head rotation column.
fn rotation_axis(angle: f32, [x, y, z]: [f32; 3]) -> Rotation {
    let s = (angle as f64).sin() as f32;
    let c = (angle as f64).cos() as f32;
    let t = 1. - c;
    let xy = t * (x * y);
    let xz = t * (x * z);
    let yz = t * (y * z);
    let sx = x * s;
    let sy = y * s;
    let sz = z * s;
    [
        [(x * x).mul_add(t, c), xy + sz, xz - sy],
        [xy - sz, (y * y).mul_add(t, c), yz + sx],
        [xz + sy, yz - sx, (z * z).mul_add(t, c)],
    ]
}

/// REL 0x8ffc0..0x908b8: world-space ear placement before inverse-actor
/// conversion. Radius is the animated head/right-ear distance times the
/// authored EarDistanceScale. The outputs are right, then left, matching
/// the native temporaries 0x3c4 and 0x390 and final bone destinations.
/// Actor-space conversion and per-ear scale must be applied by the pose owner.
pub fn world_transforms(
    camera: Rotation,
    head_position: [f32; 3],
    view_direction: [f32; 3],
    radius: f32,
    yaw: f32,
    depth: f32,
    separation: f32,
    yaw_mod: f32,
) -> [EarTransform; 2] {
    let spread = separation * -0.6806784272193909_f32; // REL constant 0x4ac834
    let placement = rotation_z(spread);
    let orientation = rotation_z(spread + std::f32::consts::FRAC_PI_2);
    let basis = product(camera, rotation_z(yaw));
    std::array::from_fn(|side| {
        let offset = rotate(
            basis,
            rotate(
                if side == 0 {
                    placement
                } else {
                    transpose(placement)
                },
                [0., radius, 0.],
            ),
        );
        let position = std::array::from_fn(|axis| {
            head_position[axis] + (offset[axis] - view_direction[axis] * depth)
        });
        let rotation = product(
            product(
                basis,
                if side == 0 {
                    orientation
                } else {
                    transpose(orientation)
                },
            ),
            rotation_x(if side == 0 {
                yaw_mod
            } else {
                yaw_mod + std::f32::consts::PI
            }),
        );
        EarTransform { rotation, position }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_retail_ear_updates() {
        for source in [
            include_str!("../../tools/blotworx/fixtures/ear_updates.json"),
            include_str!("../../tools/blotworx/fixtures/ear_camera_updates.json"),
            include_str!("../../tools/blotworx/fixtures/ear_high_updates.json"),
        ] {
            let capture: serde_json::Value = serde_json::from_str(source).unwrap();
            assert_eq!(capture["samples"].as_array().unwrap().len(), 4);
            assert_eq!(
                capture["rel_sha256"],
                "ac25d520f329a3d0507d656f0ab1cf1c34025bb36ff866187ad082ae3e0a4ebf"
            );
            let floats = |v: &serde_json::Value| {
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| f32::from_bits(x.as_u64().unwrap() as u32))
                    .collect::<Vec<_>>()
            };
            let transform = |v: &serde_json::Value| {
                let f = floats(v);
                EarTransform {
                    rotation: std::array::from_fn(|r| std::array::from_fn(|c| f[r * 3 + c])),
                    position: [f[9], f[10], f[11]],
                }
            };
            for (index, sample) in capture["samples"].as_array().unwrap().iter().enumerate() {
                let mut settings = Settings::load(&Default::default()).unwrap();
                let table = floats(&sample["profile_tables"]);
                settings.side_tilt = -table[1];
                settings.front_depth = table[4];
                settings.back_depth = table[6];
                settings.side_separation = table[9];
                settings.right_scale = floats(&sample["right_scale"])[..3].try_into().unwrap();
                settings.left_scale = floats(&sample["left_scale"])[..3].try_into().unwrap();
                settings.distance_scale =
                    f32::from_bits(sample["distance_scale"].as_u64().unwrap() as u32);
                for (name, target) in [
                    ("yaw_mod", &mut settings.yaw_mod),
                    ("enter", &mut settings.high_enter),
                    ("exit", &mut settings.high_exit),
                    ("forward_tilt", &mut settings.forward_tilt),
                    ("head_pitch", &mut settings.max_head_pitch),
                    ("speed", &mut settings.rotation_speed),
                    ("pop_angle", &mut settings.pop_angle),
                    ("high_depth", &mut settings.high_depth),
                ] {
                    *target = f32::from_bits(sample["settings"][name].as_u64().unwrap() as u32);
                }
                // The registry value is required whenever the native high branch executes.
                if sample["after"]["high"].as_u64().unwrap() != 0 {
                    settings.camera_max_pitch = f32::from_bits(
                        sample["settings"]["camera_max_pitch"]
                            .as_u64()
                            .expect("high-angle capture requires camera pitch")
                            as u32,
                    );
                }
                let mut runtime = Runtime {
                    mode: HighAngle {
                        active: sample["before"]["high"].as_u64().unwrap() != 0,
                        transitioning: sample["before"]["transitioning"].as_u64().unwrap() != 0,
                    },
                    yaw: f32::from_bits(sample["before"]["yaw"].as_u64().unwrap() as u32),
                };
                let camera_rotation = floats(&sample["camera_rotation"]);
                let camera = EarTransform {
                    rotation: std::array::from_fn(|r| {
                        std::array::from_fn(|c| camera_rotation[r * 3 + c])
                    }),
                    position: floats(&sample["camera_position"]).try_into().unwrap(),
                };
                let result = runtime
                    .step(
                        &settings,
                        &transform(&sample["head_world"]),
                        &camera,
                        transform(&sample["animated_right_ear_world"]).position,
                        &transform(&sample["actor_world"]),
                        floats(&sample["actor_world"])[12],
                    )
                    .unwrap();
                assert_eq!(
                    runtime.mode.active,
                    sample["after"]["high"].as_u64().unwrap() != 0
                );
                assert_eq!(
                    runtime.mode.transitioning,
                    sample["after"]["transitioning"].as_u64().unwrap() != 0
                );
                let native_yaw = f32::from_bits(sample["after"]["yaw"].as_u64().unwrap() as u32);
                assert!(
                    (runtime.yaw - native_yaw).abs() <= 2e-6,
                    "sample {index}: yaw {} != {native_yaw}",
                    runtime.yaw
                );
                for (side, name) in ["right_model", "left_model"].into_iter().enumerate() {
                    let q = floats(&sample[name]);
                    let bone = epic_mickey_lib_rs::hkx_file::HkxBone {
                        local_translation: q[..3].try_into().unwrap(),
                        local_rotation: q[4..8].try_into().unwrap(),
                        local_scale: q[8..11].try_into().unwrap(),
                        ..Default::default()
                    };
                    let expected = super::super::skin::model_pose(&[bone], &[]).unwrap()[0];
                    for i in 0..16 {
                        let tolerance = if (12..15).contains(&i) { 1e-5 } else { 2e-5 };
                        assert!(
                            (result[side][i] - expected[i]).abs() <= tolerance,
                            "sample {index} {name}[{i}]: {} != {}",
                            result[side][i],
                            expected[i]
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn pose_delivery_accepts_animated_scale_and_approximate_rotation() {
        use epic_mickey_lib_rs::hkx_file::HkxBone;
        let bones = [
            HkxBone {
                name: "skel_head".into(),
                local_translation: [0., 8., 0.],
                local_scale: [1., 0.9, 1.1],
                local_rotation: [0., 0., 0.707, 0.707],
                ..Default::default()
            },
            HkxBone {
                name: "skel_ear_l".into(),
                parent_index: 0,
                local_translation: [1., 1., 0.],
                ..Default::default()
            },
            HkxBone {
                name: "skel_ear_r".into(),
                parent_index: 0,
                local_translation: [-1., 1., 0.],
                ..Default::default()
            },
        ];
        let mut pose = Pose::new(&bones).unwrap();
        pose.update(
            &Settings::load(&Default::default()).unwrap(),
            &bones,
            [
                0.1, 0., 0., 0., 0., 0.1, 0., 0., 0., 0., 0.1, 0., 0., 0., 0., 1.,
            ],
            EarTransform {
                rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                position: [0., 2., 5.],
            },
        )
        .unwrap();
        assert_eq!(pose.overrides.len(), 2);
        assert!(pose
            .overrides
            .iter()
            .all(|(_, m)| m.iter().all(|v| v.is_finite())));
        let before = super::super::skin::model_pose(&bones, &[]).unwrap();
        let after = super::super::skin::model_pose(&bones, &pose.overrides).unwrap();
        assert_eq!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
        assert_ne!(before[2], after[2]);
        let yaw = pose.runtime.yaw;
        let high = pose.runtime.mode.active;
        assert!(pose.event("EVENT_Set_Ears_Static"));
        assert!(pose.overrides.is_empty());
        // Disabled mode must skip transform validation and preserve state.
        pose.update(
            &Settings::load(&Default::default()).unwrap(),
            &bones,
            [f32::NAN; 16],
            EarTransform {
                rotation: [[0.; 3]; 3],
                position: [f32::NAN; 3],
            },
        )
        .unwrap();
        assert_eq!(pose.runtime.yaw, yaw);
        assert_eq!(pose.runtime.mode.active, high);
        assert!(!pose.event("EVENT_Unrelated"));
        assert!(!pose.dynamic);
        assert!(pose.event("EVENT_Set_Ears_Dynamic"));
        assert!(pose.dynamic);
        assert_eq!(pose.runtime.yaw, yaw);
    }
    #[test]
    fn settings_preserve_native_fallbacks_and_authored_overrides() {
        let mut values = std::collections::BTreeMap::new();
        let native = Settings::load(&values).unwrap();
        assert_eq!(
            (native.high_enter, native.high_exit, native.yaw_mod),
            (140., 130., 92.)
        );
        values.insert("Ears_HighAngle_Enter".into(), "45".into());
        values.insert("MickeyEar_Left_Scale_X".into(), "0.8".into());
        let authored = Settings::load(&values).unwrap();
        assert_eq!(authored.high_enter, 45.);
        assert_eq!(authored.left_scale, [0.8, 1., 1.]);
        assert_eq!(authored.right_scale, native.right_scale);
        values.insert("EarFrontDepth".into(), "NaN".into());
        assert!(Settings::load(&values).is_err());
    }
    #[test]
    fn opposing_views_interpolate_authored_tables() {
        let profile = CardinalProfile::new(0.4, 0.1, -0.1, 0.75);
        let front = profile.sample(std::f32::consts::FRAC_PI_4, 0., false);
        assert!((front.tilt + 0.2 * std::f32::consts::PI).abs() < 1e-6);
        assert_eq!(front.depth, 0.05);
        assert_eq!(front.separation, 0.875);
        let reverse = profile.sample(-std::f32::consts::FRAC_PI_4, 0., true);
        assert!((reverse.tilt - 0.2 * std::f32::consts::PI).abs() < 1e-6);
        assert!((reverse.depth - 0.05).abs() < 1e-6);
        assert_eq!(
            profile.sample(0., 0., true),
            Profile {
                tilt: 0.,
                depth: 0.,
                separation: 0.
            }
        );
    }
    #[test]
    fn high_angle_hysteresis_preserves_boundary_and_head_pitch_rules() {
        let mut mode = HighAngle::default();
        mode.update(0.4, 0., 0.5, 0.7, 0.6);
        assert!(mode.active && mode.transitioning);
        mode.transitioning = false;
        mode.update(0.7, 0., 0.5, 0.7, 0.6);
        assert!(mode.active && !mode.transitioning);
        mode.update(0.8, 0., 0.5, 0.7, 0.6);
        assert!(!mode.active && mode.transitioning);
        mode.update(0.8, 0.61, 0.5, 0.7, 0.6);
        assert!(mode.active);
    }
    #[test]
    fn yaw_transition_wraps_and_delays_snap_until_next_update() {
        let mut mode = HighAngle {
            active: true,
            transitioning: true,
        };
        let yaw = mode.advance_yaw(3., -3., 0.3, 1.);
        assert!((yaw - (3. + 0.3 * (std::f32::consts::TAU - 6.))).abs() < 1e-6);
        assert!(mode.transitioning);
        assert_eq!(mode.advance_yaw(0., 2., 0.3, 1.), 0.);
        assert!(!mode.transitioning);
        assert_eq!(mode.advance_yaw(0., 2., 0.3, 1.), 2.);
        mode.transitioning = true;
        assert_eq!(mode.advance_yaw(0., 0.05, 0.3, 1.), 0.);
        assert!(!mode.transitioning);
        mode.transitioning = true;
        assert_eq!(mode.advance_yaw(0., 0.1, 0.5, 1.), 0.05);
        assert!(mode.transitioning);
        assert_eq!(
            angle_difference(std::f32::consts::PI, 0.),
            std::f32::consts::PI
        );
        assert_eq!(
            angle_difference(-std::f32::consts::PI, 0.),
            -std::f32::consts::PI
        );
    }
    #[test]
    fn high_depth_blend_preserves_endpoints_and_unordered_branch() {
        let quarter = std::f32::consts::FRAC_PI_2;
        assert_eq!(high_angle_depth(0.1, -0.1, 0.5, 0., 0.5, quarter), 0.1);
        assert_eq!(high_angle_depth(0.1, -0.1, 0.75, 0., 0.5, quarter), -0.1);
        assert!(high_angle_depth(0.1, -0.1, 0.75, 0., 0.5, quarter - 0.25).abs() < 1e-7);
        assert_eq!(
            high_angle_depth(0.1, -0.1, 0.25, 0., 0.5, quarter - 0.5),
            0.1
        );
    }
    #[test]
    fn ear_world_placement_follows_camera_and_preserves_radius() {
        let identity = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
        let head = [2., 3., 4.];
        let ears = world_transforms(identity, head, [0., 0., 1.], 0.4, 0., 0.1, 1., 1.);
        assert!((ears[0].position[0] + ears[1].position[0] - 2. * head[0]).abs() < 1e-6);
        for ear in &ears {
            let delta: [f32; 3] =
                std::array::from_fn(|i| ear.position[i] - head[i] + if i == 2 { 0.1 } else { 0. });
            assert!((delta.iter().map(|v| v * v).sum::<f32>() - 0.16).abs() < 1e-6);
            for row in ear.rotation {
                assert!((row.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-6);
            }
        }
        let camera = rotation_x(0.7);
        let turned = world_transforms(
            camera,
            rotate(camera, head),
            rotate(camera, [0., 0., 1.]),
            0.4,
            0.,
            0.1,
            1.,
            1.,
        );
        for side in 0..2 {
            let expected = rotate(camera, ears[side].position);
            for axis in 0..3 {
                assert!((expected[axis] - turned[side].position[axis]).abs() < 1e-6);
            }
        }
    }
    #[test]
    fn world_to_model_removes_actor_motion_without_scaling_ear_axes() {
        let actor = rotation_x(0.6);
        let origin = [10., -5., 3.];
        let scale = 0.1;
        let local_position = [2., 3., 4.];
        let local_rotation = rotation_z(-0.4);
        let rotated = rotate(actor, local_position);
        let ear = EarTransform {
            rotation: product(actor, local_rotation),
            position: std::array::from_fn(|i| origin[i] + scale * rotated[i]),
        };
        let result = ear
            .model_matrix(actor, origin, scale, [0.85, 1., 0.7])
            .unwrap();
        for axis in 0..3 {
            assert!((result[12 + axis] - local_position[axis]).abs() < 1e-5);
        }
        for col in 0..3 {
            for row in 0..3 {
                assert!(
                    (result[col * 4 + row] - local_rotation[row][col] * [0.85, 1., 0.7][col]).abs()
                        < 1e-6
                );
            }
        }
        assert!(ear.model_matrix(actor, origin, 0., [1.; 3]).is_err());
    }
    #[test]
    fn view_uses_head_heading_column_and_handles_coincident_camera() {
        let head = EarTransform {
            rotation: [[0., 0., 1.], [1., 0., 0.], [0., 1., 0.]],
            position: [0.; 3],
        };
        let camera = EarTransform {
            rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            position: [0., 2., 0.],
        };
        let view = View::from_transforms(&head, &camera, 0.);
        assert_eq!(view.head_pitch, 0.);
        assert_eq!(view.head_angle, 0.);
        assert_eq!(view.up_angle, 0.);
        assert_eq!(view.relative_yaw, std::f32::consts::PI);
        let coincident = View::from_transforms(
            &head,
            &EarTransform {
                position: head.position,
                ..camera
            },
            0.,
        );
        assert_eq!(coincident.direction, [0.; 3]);
        assert_eq!(coincident.head_angle, std::f32::consts::FRAC_PI_2);
    }
}
