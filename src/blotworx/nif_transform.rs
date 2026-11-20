//! Authored Gamebryo transform channels. Layout follows NiTransformData and
//! NiTransformInterpolator streaming; runtime scheduling is deliberately separate.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};

#[derive(Clone, Debug, serde::Serialize)]
pub struct Key {
    pub time: f32,
    pub value: Vec<f32>,
    pub tangents: Option<[Vec<f32>; 2]>, // incoming, outgoing
    pub tcb: Option<[f32; 3]>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Track {
    pub kind: u32,
    pub keys: Vec<Key>,
}
impl Track {
    pub fn load_color_data(nif: &NifFile, id: u32) -> Result<Self> {
        let mut reader = Reader::raw(nif, id, "NiColorData")?;
        let track = reader.track(4)?;
        reader.done()?;
        Ok(track)
    }
    pub fn load_float_data(nif: &NifFile, id: u32) -> Result<Self> {
        let mut reader = Reader::raw(nif, id, "NiFloatData")?;
        let track = reader.track(1)?;
        reader.done()?;
        Ok(track)
    }
    /// NiFloatKey::GenInterp / NiInterpScalar supporting-source arithmetic.
    /// Native scheduling supplies the key time; this does not loop or clamp it.
    pub fn sample_scalar(&self, time: f32) -> Result<Option<f32>> {
        Ok(self.sample_vector::<1>(time)?.map(|v| v[0]))
    }
    pub fn sample_quaternion(&self,time:f32)->Result<Option<[f32;4]>> {
        ensure!(time.is_finite(),"Nonfinite quaternion key time");
        ensure!(self.keys.iter().all(|k|k.value.len()==4),"Invalid quaternion key width");
        if self.keys.is_empty() {return Ok(None);}
        if self.keys.len()==1 {return Ok(Some(self.keys[0].value.as_slice().try_into()?));}
        ensure!(matches!(self.kind,1|5),"Quaternion key type {} requires its derived rotation-key evaluator",self.kind);
        let upper=self.keys.iter().enumerate().skip(1).find(|(_,key)|time<=key.time).map(|(i,_)|i).context("Quaternion sample exceeds final key time")?;
        let a=&self.keys[upper-1];let b=&self.keys[upper];
        let t=(time-a.time)/(b.time-a.time);
        let p=a.value.as_slice().try_into()?;let q=b.value.as_slice().try_into()?;
        Ok(Some(if self.kind==5 {if t<1. {p}else{q}}else{super::nif_quaternion::interpolate(p,q,t)?}))
    }
    /// Component-wise scalar/position tracks; quaternion tracks use a separate
    /// evaluator and must never be interpolated as four independent scalars.
    pub fn sample_vector<const N: usize>(&self, time: f32) -> Result<Option<[f32; N]>> {
        ensure!(
            N == 1 || N == 3 || N == 4,
            "Expected scalar, position or color track"
        );
        ensure!(time.is_finite(), "Nonfinite transform sample time");
        if self.keys.is_empty() {
            return Ok(None);
        }
        ensure!(
            self.keys.iter().all(|k| k.value.len() == N),
            "Unexpected transform track width"
        );
        if self.keys.len() == 1 {
            return Ok(Some(self.keys[0].value.as_slice().try_into()?));
        }
        let upper = self
            .keys
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, k)| time <= k.time)
            .map(|(i, _)| i)
            .context("Transform sample exceeds final key time")?;
        let a = &self.keys[upper - 1];
        let b = &self.keys[upper];
        let t = (time - a.time) / (b.time - a.time);
        let mut result = [0.; N];
        for (component, value) in result.iter_mut().enumerate() {
            let p = a.value[component];
            let q = b.value[component];
            *value = match self.kind {
                1 => p * (1. - t) + q * t,
                2 => {
                    let out = *a.tangents.as_ref().context("Missing outgoing tangent")?[1]
                        .get(component)
                        .context("Truncated outgoing tangent")?;
                    let input = *b.tangents.as_ref().context("Missing incoming tangent")?[0]
                        .get(component)
                        .context("Truncated incoming tangent")?;
                    let quadratic = 3. * (q - p) - (input + 2. * out);
                    let cubic = out + input - 2. * (q - p);
                    p + (out + (quadratic + cubic * t) * t) * t
                }
                5 => {
                    if t < 1. {
                        p
                    } else {
                        q
                    }
                }
                other => anyhow::bail!(
                    "Transform scalar key type {other} requires derived tangent evaluation"
                ),
            };
            ensure!(value.is_finite(), "Transform sample overflow");
        }
        Ok(Some(result))
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub enum Rotation {
    Absent,
    Quaternion(Track),
    Euler([Track; 3]),
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Channels {
    pub rotation: Rotation,
    pub translation: Track,
    pub scale: Track,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Interpolator {
    /// Keep native invalid-channel sentinels, not an invented identity pose.
    pub translation: [f32; 3],
    /// Serialized quaternion order is W, X, Y, Z.
    pub rotation: [f32; 4],
    pub scale: f32,
    pub data: Option<u32>,
    pub channels: Option<Channels>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Pose {
    pub translation: Option<[f32; 3]>,
    pub rotation: Option<[f32; 4]>,
    pub scale: Option<f32>,
}
impl Pose {
    /// NiTransformController only replaces valid channels; partial animation
    /// must not reset translation or scale merely because it animates rotation.
    pub fn apply_to(
        &self,
        base: &epic_mickey_lib_rs::nif_file::NiTransform,
    ) -> Result<epic_mickey_lib_rs::nif_file::NiTransform> {
        let mut result = base.clone();
        if let Some([x, y, z]) = self.translation {
            ensure!(
                [x, y, z].into_iter().all(f32::is_finite),
                "Invalid animated translation"
            );
            result.translation = epic_mickey_lib_rs::nif_file::Vector3 { x, y, z };
        }
        if let Some([w, x, y, z]) = self.rotation {
            ensure!(
                [w, x, y, z].into_iter().all(f32::is_finite),
                "Invalid animated quaternion"
            );
            let (xx, yy, zz) = (2. * x * x, 2. * y * y, 2. * z * z);
            let (xy, xz, yz) = (2. * y * x, 2. * z * x, 2. * z * y);
            let (wx, wy, wz) = (2. * x * w, 2. * y * w, 2. * z * w);
            let rows = [
                [1. - (yy + zz), xy - wz, xz + wy],
                [xy + wz, 1. - (xx + zz), yz - wx],
                [xz - wy, yz + wx, 1. - (xx + yy)],
            ];
            ensure!(
                rows.iter().flatten().all(|v| v.is_finite()),
                "Animated rotation overflow"
            );
            result.rotation = epic_mickey_lib_rs::nif_file::Matrix3 { rows };
        }
        if let Some(scale) = self.scale {
            ensure!(scale.is_finite(), "Invalid animated scale");
            result.scale = scale;
        }
        Ok(result)
    }
}
fn euler_quaternion([x, y, z]: [f32; 3]) -> [f32; 4] {
    // NiQuaternion::FromAngleAxesXYZ. Host sin/cos is still a numerical
    // fidelity gap until the original Wii implementation is compared.
    let (sx, cx) = (x * 0.5).sin_cos();
    let (sy, cy) = (y * 0.5).sin_cos();
    let (sz, cz) = (z * 0.5).sin_cos();
    [
        cz * cy * cx + sz * sy * sx,
        cz * cy * sx - sz * sy * cx,
        cz * sy * cx + sz * cy * sx,
        -cz * sy * sx + sz * cy * cx,
    ]
}
/// Per-instance direct-controller state. Manager-owned animation must be
/// dispatched by its sequence manager, never independently advanced here.
pub struct Playback {
    pub block: u32,
    pub flags: u16,
    pub frequency: f32,
    pub phase: f32,
    pub start: f32,
    pub stop: f32,
    pub target: u32,
    interpolator: Option<std::sync::Arc<Interpolator>>,
    clock: super::brush_particles::ControllerTime,
    scaled: Option<f32>,
    application_start: Option<f32>,
}
#[derive(Debug)]
pub enum Update {
    Inactive,
    ManagerControlled,
    Unchanged,
    NoInterpolator,
    NoValue,
    Pose { target: u32, pose: Pose },
}
impl Playback {
    pub fn load(controller: &super::scene_bounds::Controller) -> Result<Self> {
        ensure!(
            controller.class_name == "NiTransformController",
            "Expected transform controller"
        );
        let target = controller
            .target
            .context("Transform controller has no target")?;
        let interpolator = controller
            .transform
            .as_ref()
            .map(|value| {
                value
                    .as_ref()
                    .map(|v| std::sync::Arc::new(v.clone()))
                    .map_err(|e| anyhow::anyhow!("{e}"))
            })
            .transpose()?;
        Ok(Self {
            block: controller.block,
            application_start: None,
            flags: (controller.flags | 0x40) & !0x80,
            frequency: controller.frequency,
            phase: controller.phase,
            start: controller.start,
            stop: controller.stop,
            target,
            interpolator,
            clock: Default::default(),
            scaled: None,
        })
    }
    pub fn start(&mut self) {
        self.start_at(0.);
    }
    pub fn start_at(&mut self, time: f32) {
        self.flags |= 8;
        self.clock.reset();
        if self.flags & 1 != 0 {
            self.application_start = Some(time);
        }
    }
    pub fn stop(&mut self) {
        self.flags &= !8;
        self.clock.reset();
        if self.flags & 1 != 0 {
            self.application_start = None;
        }
    }
    pub fn force_next_update(&mut self) {
        self.flags |= 0x80;
    }
    pub fn update(&mut self, time: f32) -> Result<Update> {
        ensure!(time.is_finite(), "Invalid transform playback time");
        if self.flags & 0x20 != 0 {
            return Ok(Update::ManagerControlled);
        }
        if self.flags & 8 == 0 {
            return Ok(Update::Inactive);
        }
        let force = self.flags & 0x80 != 0;
        if self.clock.last_time() == Some(time) && !force {
            return Ok(Update::Unchanged);
        }
        ensure!(
            self.flags & 0x40 != 0,
            "Externally scaled transform playback requires a supplied time"
        );
        let scaled = self.clock.scale_parameters(
            time,
            self.flags,
            self.frequency,
            self.phase,
            self.start,
            self.stop,
        )?;
        if self.scaled == Some(scaled) && !force {
            return Ok(Update::Unchanged);
        }
        self.scaled = Some(scaled);
        self.flags &= !0x80;
        let Some(interpolator) = &self.interpolator else {
            return Ok(Update::NoInterpolator);
        };
        let pose = interpolator.sample(scaled)?;
        if pose.translation.is_none() && pose.rotation.is_none() && pose.scale.is_none() {
            return Ok(Update::NoValue);
        }
        Ok(Update::Pose {
            target: self.target,
            pose,
        })
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    little: bool,
}
impl<'a> Reader<'a> {
    fn raw(nif: &'a NifFile, id: u32, kind: &str) -> Result<Self> {
        let block = nif
            .blocks
            .get(id as usize)
            .with_context(|| format!("Missing {kind} block {id}"))?;
        let NiObject::Unknown(raw) = block else {
            anyhow::bail!("Expected {kind} at {id}, got {}", block.type_name())
        };
        ensure!(
            raw.type_name == kind,
            "Expected {kind} at {id}, got {}",
            raw.type_name
        );
        Ok(Self {
            bytes: &raw.raw_data,
            at: 0,
            little: nif.source_is_little_endian,
        })
    }
    fn uint(&mut self) -> Result<u32> {
        let bytes = self
            .bytes
            .get(self.at..self.at + 4)
            .context("Truncated transform animation")?
            .try_into()?;
        self.at += 4;
        Ok(if self.little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }
    fn float(&mut self) -> Result<f32> {
        let value = f32::from_bits(self.uint()?);
        ensure!(value.is_finite(), "Nonfinite transform animation value");
        Ok(value)
    }
    fn values(&mut self, count: usize) -> Result<Vec<f32>> {
        (0..count).map(|_| self.float()).collect()
    }
    fn done(&self) -> Result<()> {
        ensure!(
            self.at == self.bytes.len(),
            "Trailing transform animation bytes: {}",
            self.bytes.len() - self.at
        );
        Ok(())
    }
    fn track(&mut self, width: usize) -> Result<Track> {
        let count = self.uint()? as usize;
        if count == 0 {
            return Ok(Track {
                kind: 0,
                keys: Vec::new(),
            });
        }
        let kind = self.uint()?;
        self.keys(count, kind, width, false)
    }
    fn keys(&mut self, count: usize, kind: u32, width: usize, quaternion: bool) -> Result<Track> {
        ensure!(
            matches!(kind, 1 | 2 | 3 | 5),
            "Unsupported transform key type {kind}"
        );
        let tangent_values = if kind == 2 && !quaternion {
            2 * width
        } else {
            0
        };
        let tcb_values = if kind == 3 { 3 } else { 0 };
        let stride = 4 * (1 + width + tangent_values + tcb_values);
        ensure!(
            count <= (self.bytes.len() - self.at) / stride,
            "Truncated transform key array ({count} keys)"
        );
        let mut keys: Vec<Key> = Vec::with_capacity(count);
        for _ in 0..count {
            let time = self.float()?;
            ensure!(
                keys.last().is_none_or(|k| k.time < time),
                "Transform key times must increase"
            );
            let value = self.values(width)?;
            let tangents = if tangent_values != 0 {
                Some([self.values(width)?, self.values(width)?])
            } else {
                None
            };
            let tcb = if tcb_values != 0 {
                Some([self.float()?, self.float()?, self.float()?])
            } else {
                None
            };
            keys.push(Key {
                time,
                value,
                tangents,
                tcb,
            });
        }
        // NiLinRotKey/NiStepRotKey invoke NiRotKey::FillDerivedVals once at
        // load: adjacent hemispheres first, then clamp W. Do not normalize.
        if quaternion && matches!(kind,1|5) {
            for i in 1..keys.len() {
                let p=keys[i-1].value.as_slice().try_into()?;
                let q=keys[i].value.as_slice().try_into()?;
                if super::nif_quaternion::dot(p,q)<0. {for v in &mut keys[i].value {*v=-*v;}}
            }
            for key in &mut keys {key.value[0]=key.value[0].clamp(-1.,1.);}
        }
        Ok(Track { kind, keys })
    }
}
impl Channels {
    pub fn euler_angles(&self, time: f32) -> Result<Option<[f32; 3]>> {
        match &self.rotation {
            Rotation::Absent => Ok(None),
            Rotation::Euler(axes) => Ok(Some([
                axes[0].sample_scalar(time)?.unwrap_or(0.),
                axes[1].sample_scalar(time)?.unwrap_or(0.),
                axes[2].sample_scalar(time)?.unwrap_or(0.),
            ])),
            Rotation::Quaternion(_) => {
                anyhow::bail!("Quaternion rotation requires its rotation-key evaluator")
            }
        }
    }
    pub fn load(nif: &NifFile, id: u32) -> Result<Self> {
        let mut r = Reader::raw(nif, id, "NiTransformData")?;
        let count = r.uint()? as usize;
        let rotation = if count == 0 {
            Rotation::Absent
        } else {
            let kind = r.uint()?;
            if kind == 4 {
                ensure!(count == 1, "Euler rotation requires one container key");
                Rotation::Euler([r.track(1)?, r.track(1)?, r.track(1)?])
            } else {
                Rotation::Quaternion(r.keys(count, kind, 4, true)?)
            }
        };
        let translation = r.track(3)?;
        let scale = r.track(1)?;
        r.done()?;
        Ok(Self {
            rotation,
            translation,
            scale,
        })
    }
}
impl Interpolator {
    pub fn sample(&self, time: f32) -> Result<Pose> {
        self.sample_channels(time, [true; 3], [true; 3])
    }
    /// Evaluators select channels independently; interpolators sample all of
    /// their valid channels. Do not evaluate keys belonging to inactive or
    /// constant evaluator channels.
    pub(super) fn sample_channels(
        &self,
        time: f32,
        active: [bool; 3],
        animated: [bool; 3],
    ) -> Result<Pose> {
        ensure!(time.is_finite(), "Nonfinite transform sample time");
        // NiQuatTransform validity uses translate.x, rotate.x and scale.
        let mut result = Pose {
            translation: (active[0] && self.translation[0] != -f32::MAX)
                .then_some(self.translation),
            rotation: (active[1] && self.rotation[1] != -f32::MAX).then_some(self.rotation),
            scale: (active[2] && self.scale != -f32::MAX).then_some(self.scale),
        };
        if let Some(channels) = &self.channels {
            if active[0] && animated[0] {
                if let Some(value) = channels.translation.sample_vector::<3>(time)? {
                    result.translation = Some(value)
                }
            }
            if active[2] && animated[2] {
                if let Some(value) = channels.scale.sample_scalar(time)? {
                    result.scale = Some(value)
                }
            }
            if active[1] && animated[1] {
                match &channels.rotation {
                    Rotation::Absent => {}
                    Rotation::Euler(_) => {
                        result.rotation = Some(euler_quaternion(
                            channels
                                .euler_angles(time)?
                                .context("Missing Euler angles")?,
                        ));
                    }
                    Rotation::Quaternion(track) => {
                        result.rotation=track.sample_quaternion(time)?;
                    }
                }
            }
        }
        Ok(result)
    }
    pub fn load(nif: &NifFile, id: u32) -> Result<Self> {
        let mut r = Reader::raw(nif, id, "NiTransformInterpolator")?;
        let translation = [r.float()?, r.float()?, r.float()?];
        let rotation = [r.float()?, r.float()?, r.float()?, r.float()?];
        let scale = r.float()?;
        let link = r.uint()?;
        let data = (link != u32::MAX).then_some(link);
        r.done()?;
        let channels = data.map(|id| Channels::load(nif, id)).transpose()?;
        Ok(Self {
            translation,
            rotation,
            scale,
            data,
            channels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use epic_mickey_lib_rs::nif_file::RawBlockData;
    #[test]
    fn direct_playback_honors_lifecycle_duplicates_frequency_and_manager() -> Result<()> {
        let key = |time| Key {
            time,
            value: vec![time; 3],
            tangents: None,
            tcb: None,
        };
        let interpolator = Interpolator {
            translation: [-f32::MAX; 3],
            rotation: [-f32::MAX; 4],
            scale: -f32::MAX,
            data: None,
            channels: Some(Channels {
                rotation: Rotation::Absent,
                translation: Track {
                    kind: 1,
                    keys: vec![key(0.), key(4.)],
                },
                scale: Track {
                    kind: 0,
                    keys: vec![],
                },
            }),
        };
        let c = super::super::scene_bounds::Controller {
            block: 1,
            class_name: "NiTransformController".into(),
            flags: 0xcc,
            frequency: 1.,
            phase: 0.,
            start: 0.,
            stop: 4.,
            target: Some(7),
            transform: Some(Ok(interpolator)),
        };
        let mut playback = Playback::load(&c)?;
        assert_eq!(playback.flags, 0x4c);
        let position = |update| match update {
            Update::Pose { target: 7, pose } => pose.translation.unwrap()[0],
            other => panic!("Expected target pose, got {other:?}"),
        };
        assert_eq!(position(playback.update(1.)?), 1.);
        assert!(matches!(playback.update(1.)?, Update::Unchanged));
        playback.frequency = 2.;
        assert_eq!(position(playback.update(2.)?), 3.);
        playback.stop();
        assert!(matches!(playback.update(3.)?, Update::Inactive));
        playback.start();
        assert_eq!(position(playback.update(2.)?), 4.);
        playback.force_next_update();
        assert_eq!(position(playback.update(2.)?), 4.);
        assert!(matches!(playback.update(3.)?, Update::Unchanged));
        playback.flags |= 0x20;
        assert!(matches!(playback.update(10.)?, Update::ManagerControlled));
        playback.flags &= !0x20;
        playback.flags |= 1;
        playback.phase = 1.;
        playback.start();
        assert_eq!(position(playback.update(20.)?), 1.);
        assert_eq!(position(playback.update(21.)?), 3.);
        assert!(playback.update(f32::NAN).is_err());
        Ok(())
    }
    #[test]
    fn rotation_only_pose_preserves_target_channels_and_composes_xyz() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::{NiNodeData, Vector3};
        let invalid = -f32::MAX;
        let mut i = Interpolator {
            translation: [invalid; 3],
            rotation: [invalid; 4],
            scale: invalid,
            data: None,
            channels: None,
        };
        let mut base = NiNodeData::default().base.transform;
        base.translation = Vector3 {
            x: 7.,
            y: 8.,
            z: 9.,
        };
        base.scale = 0.1;
        let unchanged = i.sample(0.)?.apply_to(&base)?;
        assert_eq!(unchanged.rotation.rows, base.rotation.rows);
        i.rotation =
            euler_quaternion([std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2, 0.]);
        let result = i.sample(0.)?.apply_to(&base)?;
        assert_eq!(
            [
                result.translation.x,
                result.translation.y,
                result.translation.z
            ],
            [7., 8., 9.]
        );
        assert_eq!(result.scale, 0.1);
        let expected = [[0., 1., 0.], [0., 0., -1.], [-1., 0., 0.]];
        for (actual, expected) in result
            .rotation
            .rows
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
        {
            assert!(
                (actual - expected).abs() < 3e-7,
                "Wrong XYZ rotation convention"
            );
        }
        assert!(i.sample(f32::NAN).is_err());
        Ok(())
    }
    #[test]
    fn scalar_sampling_uses_authored_tangents_and_exact_step_boundary() -> Result<()> {
        let key = |time, value, input, output| Key {
            time,
            value: vec![value],
            tangents: Some([vec![input], vec![output]]),
            tcb: None,
        };
        let mut track = Track {
            kind: 2,
            keys: vec![key(2., 0., 0., 2.), key(4., 1., 0., 0.)],
        };
        assert_eq!(track.sample_scalar(3.)?, Some(0.75));
        assert_eq!(track.sample_scalar(2.)?, Some(0.));
        assert_eq!(track.sample_scalar(4.)?, Some(1.));
        assert!(track.sample_scalar(4.1).is_err());
        track.kind = 5;
        assert_eq!(track.sample_scalar(3.999)?, Some(0.));
        assert_eq!(track.sample_scalar(4.)?, Some(1.));
        track.kind = 3;
        assert!(track.sample_scalar(3.).is_err());
        Ok(())
    }
    #[test]
    fn authored_channels_retain_tangents_tcb_and_invalid_pose() -> Result<()> {
        for little in [false, true] {
            let encode = |words: &[u32]| {
                words
                    .iter()
                    .flat_map(|v| {
                        if little {
                            v.to_le_bytes()
                        } else {
                            v.to_be_bytes()
                        }
                    })
                    .collect()
            };
            let f = |v: f32| v.to_bits();
            let interpolator = [
                f(-f32::MAX),
                f(-f32::MAX),
                f(-f32::MAX),
                f(1.),
                0,
                0,
                0,
                f(-f32::MAX),
                1,
            ];
            // Euler X cubic, absent Y, constant Z; TCB position, linear scale.
            let data = [
                1,
                4,
                1,
                2,
                0,
                f(2.),
                f(3.),
                f(4.),
                0,
                1,
                1,
                0,
                f(5.),
                1,
                3,
                0,
                f(1.),
                f(2.),
                f(3.),
                f(0.1),
                f(0.2),
                f(0.3),
                1,
                1,
                0,
                f(2.),
            ];
            let mut nif = NifFile {
                source_is_little_endian: little,
                blocks: vec![
                    NiObject::Unknown(RawBlockData {
                        type_name: "NiTransformInterpolator".into(),
                        raw_data: encode(&interpolator),
                    }),
                    NiObject::Unknown(RawBlockData {
                        type_name: "NiTransformData".into(),
                        raw_data: encode(&data),
                    }),
                ],
                ..Default::default()
            };
            let i = Interpolator::load(&nif, 0)?;
            assert_eq!(i.translation, [-f32::MAX; 3]);
            assert_eq!(i.scale, -f32::MAX);
            let channels = i.channels.unwrap();
            let Rotation::Euler(axes) = channels.rotation else {
                panic!("lost Euler channels")
            };
            assert_eq!(axes[0].keys[0].tangents, Some([vec![3.], vec![4.]]));
            assert!(axes[1].keys.is_empty());
            assert_eq!(axes[2].keys[0].value, [5.]);
            assert_eq!(channels.translation.keys[0].tcb, Some([0.1, 0.2, 0.3]));
            if let NiObject::Unknown(raw) = &mut nif.blocks[1] {
                raw.raw_data.pop();
            }
            assert!(Interpolator::load(&nif, 0).is_err());
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_transform_channels() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let path = std::path::Path::new(&root).join("packfiles/gremlinvillage_zonea.pak");
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(path)?);
        let mut inventory = Vec::new();
        let mut platform_samples = Vec::new();
        let mut pose_samples = 0;
        for file in &pak.files {
            if !file.path.to_ascii_lowercase().ends_with(".nif_wii") {
                continue;
            }
            let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            for (id, block) in nif.blocks.iter().enumerate() {
                if block.type_name() != "NiTransformInterpolator" {
                    continue;
                }
                let value = Interpolator::load(&nif, id as u32)
                    .with_context(|| format!("{} block {id}", file.path))?;
                let mut tracks = Vec::new();
                if let Some(c) = &value.channels {
                    tracks.extend([&c.translation, &c.scale]);
                    match &c.rotation {
                        Rotation::Euler(axes) => tracks.extend(axes),
                        Rotation::Quaternion(t) => tracks.push(t),
                        Rotation::Absent => {}
                    }
                }
                let animated: Vec<_> = tracks.iter().filter(|t| t.keys.len() > 1).collect();
                let start = animated
                    .iter()
                    .map(|t| t.keys[0].time)
                    .reduce(f32::max)
                    .unwrap_or(0.);
                let end = animated
                    .iter()
                    .map(|t| t.keys.last().unwrap().time)
                    .reduce(f32::min)
                    .unwrap_or(start);
                ensure!(
                    start <= end,
                    "Transform channels have no shared sampling range"
                );
                let base = epic_mickey_lib_rs::nif_file::NiNodeData::default()
                    .base
                    .transform;
                for time in [start, start + (end - start) * 0.5, end] {
                    value
                        .sample(time)?
                        .apply_to(&base)
                        .with_context(|| format!("{} block {id} time {time}", file.path))?;
                    pose_samples += 1;
                }
                if file
                    .path
                    .to_ascii_lowercase()
                    .ends_with("/gv_zonea_platform01.nif_wii")
                {
                    let channels = value
                        .channels
                        .as_ref()
                        .context("Missing platform key data")?;
                    let hierarchy = super::super::scene_bounds::Hierarchy::load(&nif)?;
                    let controller = hierarchy
                        .nodes
                        .values()
                        .flat_map(|n| &n.controllers)
                        .find(|c| c.class_name == "NiTransformController")
                        .context("Platform controller missing")?;
                    let target = controller
                        .target
                        .context("Platform controller target missing")?;
                    let mut playback = Playback::load(controller)?;
                    let mut playback_samples = Vec::new();
                    for frame in 0..=360 {
                        let raw_time = frame as f32 / 30.;
                        let Update::Pose {
                            target: updated,
                            pose,
                        } = playback.update(raw_time)?
                        else {
                            anyhow::bail!(
                                "Platform direct playback did not produce a pose at {raw_time}"
                            )
                        };
                        ensure!(updated == target, "Controller target changed");
                        ensure!(
                            matches!(playback.update(raw_time)?, Update::Unchanged),
                            "Repeated presentation advanced platform clock"
                        );
                        playback_samples.push(serde_json::json!({"time":raw_time,"scaled_time":playback.scaled,"pose":pose}));
                    }
                    std::fs::write(
                        "target/platform-playback-samples.json",
                        serde_json::to_vec_pretty(&playback_samples)?,
                    )?;
                    let original = hierarchy.base_world_transforms(None)?;
                    let poses = std::collections::BTreeMap::from([(target, value.sample(1.)?)]);
                    let animated = hierarchy.base_world_transforms_with_poses(None, &poses)?;
                    ensure!(
                        original[&target].rotation.rows != animated[&target].rotation.rows,
                        "Platform target did not animate"
                    );
                    let descendant = *hierarchy.nodes[&target]
                        .children
                        .iter()
                        .flatten()
                        .next()
                        .context("Platform child missing")?;
                    ensure!(
                        original[&descendant].rotation.rows != animated[&descendant].rotation.rows,
                        "Animation did not propagate to platform child"
                    );
                    let (&mesh_id, mesh) = hierarchy
                        .nodes
                        .iter()
                        .find(|(_, n)| n.authored_bound.is_some())
                        .context("Platform mesh bound missing")?;
                    let sphere = mesh.authored_bound.unwrap();
                    for frame in 1..=180 {
                        let time = frame as f32 / 30.;
                        let pose = value.sample(time)?;
                        let transforms = hierarchy.base_world_transforms_with_poses(
                            None,
                            &std::collections::BTreeMap::from([(target, pose)]),
                        )?;
                        let bound = sphere.transformed(&transforms[&mesh_id])?;
                        platform_samples.push(serde_json::json!({"time":time,"angles":channels.euler_angles(time)?,
                            "mesh_block":mesh_id,"model_center":bound.center,"radius":bound.radius}));
                    }
                    let first = &platform_samples[0]["model_center"];
                    ensure!(
                        platform_samples.iter().any(|s| &s["model_center"] != first),
                        "Mesh sample center did not follow platform animation"
                    );
                }
                inventory
                    .push(serde_json::json!({"asset":file.path,"block":id,"interpolator":value}));
            }
        }
        ensure!(!inventory.is_empty(), "No authored transforms inspected");
        ensure!(
            platform_samples.len() == 180,
            "Platform Euler samples missing"
        );
        ensure!(
            platform_samples
                .iter()
                .any(|s| s["angles"][2].as_f64().is_some_and(|v| v.abs() > 0.01)),
            "Platform cubic curve lost its motion between keys"
        );
        std::fs::write(
            "target/platform-transform-samples.json",
            serde_json::to_vec_pretty(&platform_samples)?,
        )?;
        std::fs::write(
            "target/production-transform-channels.json",
            serde_json::to_vec_pretty(&inventory)?,
        )?;
        println!(
            "Decoded {} authored transform interpolators; evaluated {pose_samples} complete poses",
            inventory.len()
        );
        Ok(())
    }
}
