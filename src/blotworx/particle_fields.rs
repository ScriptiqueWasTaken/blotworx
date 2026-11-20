//! EM1 particle fields. Serialized field/controller ownership is distinct
//! from the prepared kernel input. Unsupported force modes remain errors.
use super::{raw, ControllerTime, RateTrack};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};
type V = [f32; 3];
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    Drag,
    Air,
    Radial,
    Turbulence,
    Vortex,
    Gravity,
}
#[derive(Clone)]
pub(super) struct Force {
    pub name: String,
    pub kind: Kind,
    pub magnitude: f32,
    pub world_direction: bool,
    active: bool,
    node: Option<u32>,
    attenuation: f32,
    maximum: Option<f32>,
    radial_type: f32,
    subtype: u32,
    directional: bool,
    direction: V,
    frame: [f32; 16],
    frequency_update_time: f32,
    last_update_time: Option<f32>,
    air_friction: f32,
    inherited_velocity: f32,
    inherit_rotation: bool,
    enable_spread: bool,
    spread: f32,
    last_air_position: Option<V>,
    last_air_update: Option<f32>,
    pub controls: Vec<Magnitude>,
}
#[derive(Clone)]
pub(super) struct Magnitude {
    pub block: u32,
    pub name: String,
    flags: u16,
    frequency: f32,
    phase: f32,
    start: f32,
    stop: f32,
    clock: ControllerTime,
    rate: RateTrack,
    scaled: Option<f32>,
    application_start: Option<f32>,
}
impl Magnitude {
    pub fn load(
        nif: &NifFile,
        controller: &super::super::scene_bounds::Controller,
        system: u32,
    ) -> Result<Self> {
        ensure!(
            controller.class_name == "NiPSFieldMagnitudeCtlr",
            "Unsupported particle property controller {}",
            controller.class_name
        );
        let mut r = raw(nif, controller.block, "NiPSFieldMagnitudeCtlr")?;
        r.uint()?;
        let flags = (r.short()? | 0x40) & !0x80;
        let frequency = r.float()?;
        let phase = r.float()?;
        let start = r.float()?;
        let stop = r.float()?;
        ensure!(
            r.uint()? == system,
            "Field controller targets another particle system"
        );
        let interpolator = r.uint()?;
        let name = nif
            .string_table
            .get(r.uint()? as usize)
            .context("Missing field-controller force name")?
            .clone();
        r.done()?;
        Ok(Self {
            block: controller.block,
            application_start: None,
            name,
            flags,
            frequency,
            phase,
            start,
            stop,
            clock: ControllerTime::default(),
            rate: RateTrack::load(nif, interpolator)?,
            scaled: None,
        })
    }
    pub fn reset(&mut self) {
        self.clock.reset();
        self.scaled = None;
    }
    pub fn prepare_effect_template(&mut self) {
        self.flags = super::super::nif_runtime::effect_template_flags(self.flags);
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
    fn update(&mut self, time: f32) -> Result<Option<f32>> {
        if self.flags & 8 == 0 {
            return Ok(None);
        }
        ensure!(
            self.flags & 0x20 == 0,
            "Manager-controlled field magnitude requires sequence-manager dispatch"
        );
        if self.clock.last_time() == Some(time) && self.flags & 0x80 == 0 {
            return Ok(None);
        }
        let scaled = self.clock.scale_parameters(
            time,
            self.flags,
            self.frequency,
            self.phase,
            self.start,
            self.stop,
        )?;
        if self.scaled == Some(scaled) && self.flags & 0x80 == 0 {
            return Ok(None);
        }
        let value = self.rate.sample(scaled)?;
        ensure!(value.is_finite(), "Nonfinite animated field magnitude");
        self.scaled = Some(scaled);
        self.flags &= !0x80;
        Ok(Some(value))
    }
}
impl Force {
    pub fn publish_model_transform(
        &mut self,
        world: &std::collections::BTreeMap<u32, epic_mickey_lib_rs::nif_file::NiTransform>,
        inverse_system: Option<&[f32; 16]>,
    ) -> Result<()> {
        if let Some(node) = self.node {
            self.frame = super::super::nif_runtime::matrix(
                world.get(&node).context("Particle field pose missing")?,
            );
            if let Some(inverse) = inverse_system {
                self.frame = super::super::skin::matrix_product(inverse, &self.frame);
            }
        }
        Ok(())
    }
    pub fn load(nif: &NifFile, index: u32, inverse_system: Option<&[f32; 16]>) -> Result<Self> {
        Self::load_inner(nif, index, inverse_system, false)
    }
    pub fn load_animated(nif: &NifFile, index: u32) -> Result<Self> {
        Self::load_inner(nif, index, None, true)
    }
    fn load_inner(nif: &NifFile, index: u32, inverse_system: Option<&[f32; 16]>, live_pose: bool) -> Result<Self> {
        let class = nif
            .blocks
            .get(index as usize)
            .context("Missing particle force")?
            .type_name();
        let kind = match class {
            "NiPSDragFieldForce" => Kind::Drag,
            "NiPSAirFieldForce" => Kind::Air,
            "NiPSRadialFieldForce" => Kind::Radial,
            "NiPSTurbulenceFieldForce" => Kind::Turbulence,
            "NiPSVortexFieldForce" => Kind::Vortex,
            "NiPSGravityFieldForce" => Kind::Gravity,
            _ => anyhow::bail!("Particle force {class} is not implemented"),
        };
        let mut r = raw(nif, index, class)?;
        let name = nif
            .string_table
            .get(r.uint()? as usize)
            .context("Missing particle force name")?
            .clone();
        let code = r.uint()?;
        ensure!(
            code == match kind {
                Kind::Drag => 3,
                Kind::Air => 2,
                Kind::Radial => 5,
                Kind::Vortex => 7,
                Kind::Turbulence => 6,
                Kind::Gravity => 4,
            },
            "Particle force type disagrees with {class}"
        );
        let active = r.byte()? != 0;
        let node = r.uint()?;
        let magnitude = r.float()?;
        let attenuation = r.float()?;
        let use_max = r.byte()? != 0;
        let max = r.float()?;
        let mut radial_type = 0.;
        let mut subtype = 0;
        let mut directional = false;
        let mut direction = [0.; 3];
        let mut world_direction = false;
        let mut frequency_update_time = f32::INFINITY;
        let mut air_friction = 0.;
        let mut inherited_velocity = 0.;
        let mut inherit_rotation = false;
        let mut enable_spread = false;
        let mut spread = 0.;
        match kind {
            Kind::Drag => {
                directional = r.byte()? != 0;
                direction = [r.float()?, r.float()?, r.float()?];
            }
            Kind::Radial => {
                //804e9aac clamps type into[0,1], then loads the EM1 word+34
                //for user>=14. This word is missing from the supplied SDK.
                radial_type = r.float()?.clamp(0., 1.);
                if nif.user_version >= 14 {
                    subtype = r.uint()?;
                }
            }
            Kind::Vortex => direction = [r.float()?, r.float()?, r.float()?],
            Kind::Turbulence => {
                let frequency = r.float()?;
                ensure!(frequency.is_finite(), "Invalid turbulence update frequency");
                frequency_update_time = 1.0 / frequency.max(0.0001);
            }
            Kind::Air => {
                direction = [r.float()?, r.float()?, r.float()?];
                air_friction = r.float()?;
                inherited_velocity = r.float()?;
                inherit_rotation = r.byte()? != 0;
                enable_spread = r.byte()? != 0;
                spread = r.float()?;
            }
            Kind::Gravity => {
                // DOL804dac78: user>=6 revision precedes direction; revision1
                // appends Using World Direction (not part of the SDK payload).
                let revision = if nif.user_version >= 6 { r.short()? } else { 0 };
                direction = [r.float()?, r.float()?, r.float()?];
                if revision == 1 {
                    world_direction = r.byte()? != 0;
                }
            }
        }
        r.done()?;
        let node = (node != u32::MAX).then_some(node);
        let mut frame = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        if let Some(node) = node {
            // A field's animated ancestry must be supplied by model playback,
            // never silently frozen at the bind pose.
            let mut current = Some(node as usize);
            let mut seen = std::collections::BTreeSet::new();
            while let Some(index) = current {
                ensure!(seen.insert(index), "Cyclic particle field ancestry");
                let av = nif
                    .blocks
                    .get(index)
                    .and_then(NiObject::as_av_object)
                    .context("Field node is not a transform")?;
                let controls =
                    super::super::scene_bounds::controllers(nif, av.base.controller_link)?;
                ensure!(
                    live_pose || !controls
                        .iter()
                        .any(|c| c.class_name == "NiTransformController"),
                    "Animated particle field transform requires live model pose publication"
                );
                let mut parents = nif
                    .blocks
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.children().contains(&Some(index as u32)))
                    .map(|(i, _)| i);
                current = parents.next();
                ensure!(parents.next().is_none(), "Multiple particle field parents");
            }
            frame = super::super::brush_render::node_transform(nif, node as usize)?;
            if let Some(inverse) = inverse_system {
                frame = super::super::skin::matrix_product(inverse, &frame);
            }
        }
        Ok(Self {
            name,
            kind,
            magnitude,
            world_direction,
            active,
            node,
            attenuation,
            maximum: use_max.then_some(max),
            radial_type,
            subtype,
            directional,
            direction,
            frame,
            frequency_update_time,
            last_update_time: None,
            air_friction,
            inherited_velocity,
            inherit_rotation,
            enable_spread,
            spread,
            last_air_position: None,
            last_air_update: None,
            controls: Vec::new(),
        })
    }
    pub fn reset(&mut self) {
        self.last_update_time = None;
        self.last_air_position = None;
        self.last_air_update = None;
        for control in &mut self.controls {
            control.reset();
        }
    }
    pub fn prepare(&mut self, time: f32, owner: Option<&[f32; 16]>) -> Result<Option<Prepared>> {
        for control in &mut self.controls {
            if let Some(value) = control.update(time)? {
                self.magnitude = value;
            }
        }
        // NiPSTurbulenceFieldForce::Update throttles field evaluation to its
        // authored frequency. A skipped update contributes no force kernel.
        if self.kind == Kind::Turbulence {
            if let Some(last) = self.last_update_time {
                if time >= last && time < last + self.frequency_update_time {
                    return Ok(None);
                }
            }
            self.last_update_time = Some(time);
        }
        if !self.active
            || self.node.is_none()
            || (self.magnitude == 0. && (self.kind != Kind::Air || self.inherited_velocity == 0.))
        {
            return Ok(None);
        }
        ensure!(
            self.kind != Kind::Radial || self.subtype == 0,
            "EM1 radial field subtype {} requires its native kernel",
            self.subtype
        );
        let frame = owner.map_or(self.frame, |owner| {
            super::super::skin::matrix_product(owner, &self.frame)
        });
        let position: V = frame[12..15].try_into().unwrap();
        let direction = if self.kind == Kind::Air && !self.inherit_rotation {
            normalize(self.direction)?
        } else if self.world_direction {
            [0., -1., 0.]
        } else if matches!(self.kind, Kind::Air | Kind::Vortex | Kind::Gravity) || (self.kind==Kind::Drag && self.directional) {
            if self.direction == [0.; 3] {
                ensure!(self.kind!=Kind::Drag,"Zero authored directional drag axis requires retained native axis initialization");
                return Ok(None);
            }
            let rotated = std::array::from_fn(|row| {
                frame[8 + row].mul_add(
                    self.direction[2],
                    frame[4 + row].mul_add(self.direction[1], frame[row] * self.direction[0]),
                )
            });
            normalize(rotated)?
        } else {
            [0.; 3]
        };
        let direction = if self.kind == Kind::Air {
            let inherited = match (self.last_air_position, self.last_air_update) {
                (Some(previous), Some(previous_time)) if time > previous_time => {
                    let elapsed = time - previous_time;
                    std::array::from_fn(|i| {
                        (position[i] - previous[i]) * self.inherited_velocity / elapsed
                    })
                }
                _ => [0.; 3],
            };
            self.last_air_position = Some(position);
            self.last_air_update = Some(time);
            std::array::from_fn(|i| direction[i] * self.magnitude + inherited[i])
        } else {
            direction
        };
        Ok(Some(Prepared {
            kind: self.kind,
            magnitude: self.magnitude,
            position,
            direction,
            attenuation: self.attenuation,
            maximum: self.maximum,
            radial_type: self.radial_type,
            air_friction: self.air_friction,
            air_spread_cos: if self.enable_spread {
                (std::f32::consts::FRAC_PI_2 * self.spread).cos()
            } else {
                0.
            },
            air_spread: self.enable_spread,
            drag_directional: self.kind==Kind::Drag && self.directional,
        }))
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Prepared {
    pub kind: Kind,
    pub magnitude: f32,
    pub position: V,
    pub direction: V,
    pub attenuation: f32,
    pub maximum: Option<f32>,
    pub radial_type: f32,
    pub air_friction: f32,
    pub air_spread_cos: f32,
    pub air_spread: bool,
    pub drag_directional: bool,
}
impl Prepared {
    ///804fa408 drag,804fa9bc radial,804fb02c vortex. Prepared fields only;
    ///the simulator owns position/age integration and last-update publication.
    pub fn apply(&self, position: V, velocity: V, current: f32, last: f32) -> Result<V> {
        let mut random = crate::blotworx::particle_random::Stream::Preview { seed: 0x6d2b79f5 };
        self.apply_with_random(position, velocity, current, last, &mut random)
    }
    pub fn apply_with_random(
        &self,
        position: V,
        velocity: V,
        current: f32,
        last: f32,
        random: &mut crate::blotworx::particle_random::Stream<'_>,
    ) -> Result<V> {
        ensure!(
            position
                .into_iter()
                .chain(velocity)
                .chain(self.position)
                .chain(self.direction)
                .chain([self.magnitude, current, last])
                .all(f32::is_finite),
            "Nonfinite particle force input"
        );
        let delta = current - last;
        let result = match self.kind {
            // Gamebryo NiPSForceHelpers::ApplyGravityFieldForce, constant
            // unattenuated branch. EM1 arithmetic parity remains unvalidated.
            Kind::Gravity => {
                let distance = position
                    .iter()
                    .zip(self.position)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f32>();
                if self.maximum.is_some_and(|limit| distance > limit * limit) {
                    velocity
                } else {
                    let attenuation = if self.attenuation == 0. || distance == 0. {
                        1.
                    } else {
                        distance.powf(self.attenuation * 0.5).max(1.)
                    };
                    let magnitude = self.magnitude / attenuation;
                    std::array::from_fn(|i| velocity[i] + (self.direction[i] * magnitude) * delta)
                }
            }
            Kind::Drag => drag_velocity(position,velocity,self.position,self.magnitude,
                self.attenuation,self.maximum,delta,self.drag_directional.then_some(self.direction))?,
            Kind::Air => {
                let offset: V = std::array::from_fn(|i| position[i] - self.position[i]);
                if self
                    .maximum
                    .is_some_and(|limit| squared(offset) > limit * limit)
                {
                    velocity
                } else {
                    let mut air_velocity = self.direction;
                    if self.air_spread {
                        let distance = length(offset)?;
                        if distance == 0. {
                            return Ok(velocity);
                        }
                        let toward = offset.map(|component| component / distance);
                        if self.direction[0].mul_add(toward[0], self.direction[1] * toward[1])
                            + self.direction[2] * toward[2]
                            < self.air_spread_cos
                        {
                            return Ok(velocity);
                        }
                        let air_speed = length(self.direction)?;
                        air_velocity = toward.map(|component| component * air_speed);
                    }
                    let relative: V = std::array::from_fn(|i| velocity[i] - air_velocity[i]);
                    let dot = air_velocity[0].mul_add(
                        relative[0],
                        air_velocity[1].mul_add(relative[1], air_velocity[2] * relative[2]),
                    );
                    if dot < 0. {
                        let mut factor = delta;
                        if let Some(maximum) = self.maximum {
                            if self.attenuation != 0. && maximum != 0. {
                                factor *= (1. - length(offset)? / maximum)
                                    .max(0.)
                                    .powf(self.attenuation);
                            }
                        }
                        std::array::from_fn(|i| {
                            velocity[i] + air_velocity[i] * self.air_friction * factor
                        })
                    } else {
                        velocity
                    }
                }
            }
            Kind::Radial => {
                let diff: V = std::array::from_fn(|i| position[i] - self.position[i]);
                let mut factor = self.magnitude * delta;
                //804fa998 skips the squared-distance/pow path when neither
                //a cutoff nor effective attenuation is requested.
                if self.maximum.is_some() || (self.attenuation != 0. && self.radial_type != 0.) {
                    let distance = squared(diff);
                    if self.maximum.is_some_and(|limit| distance > limit * limit) {
                        return Ok(velocity);
                    }
                    if self.attenuation != 0. && distance != 0. {
                        //804faafc narrows the native double pow before the
                        //radial-type multiply and lower-bound comparison.
                        let power =
                            super::super::native_power::field(distance, self.attenuation * 0.5)?;
                        let denominator = power * self.radial_type;
                        factor /= if denominator < 1. { 1. } else { denominator };
                    }
                }
                std::array::from_fn(|i| velocity[i] + diff[i] * factor)
            }
            Kind::Vortex => {
                let diff: V = std::array::from_fn(|i| position[i] - self.position[i]);
                //804fb128: the general path copies velocity before testing the
                //native (not squared) distance. Its zero/distance rejections
                //must preserve even signed-zero velocity bits.
                let distance = if self.attenuation != 0. || self.maximum.is_some() {
                    let distance = length(diff)?;
                    if distance == 0. || self.maximum.is_some_and(|limit| distance > limit) {
                        return Ok(velocity);
                    }
                    Some(distance)
                } else {
                    None
                };
                let dot = self.direction[0].mul_add(diff[0], self.direction[1] * diff[1])
                    + self.direction[2] * diff[2];
                let perpendicular: V = std::array::from_fn(|i| diff[i] - self.direction[i] * dot);
                let tangent = if perpendicular == [0.; 3] {
                    perpendicular
                } else {
                    let a = self.direction;
                    let b = perpendicular;
                    let cross = [
                        a[1].mul_add(b[2], -(b[1] * a[2])),
                        -a[0].mul_add(b[2], -(b[0] * a[2])),
                        -a[1].mul_add(b[0], -(b[1] * a[0])),
                    ];
                    if length(cross)? > f32::from_bits(0x358637bd) {
                        normalize(cross)?
                    } else {
                        [0.; 3]
                    }
                };
                //804fb244 calls pow(distance,attenuation) in double, then
                //804fbdb8 narrows it before fdivs. Unlike gravity/radial this
                //vortex branch does not clamp the denominator to one.
                let factor = self.magnitude * delta;
                let factor = if self.attenuation == 0. {
                    factor
                } else {
                    factor
                        / super::super::native_power::field(
                            distance.expect("Vortex attenuation distance"),
                            self.attenuation,
                        )?
                };
                std::array::from_fn(|i| velocity[i] + tangent[i] * factor)
            }
            Kind::Turbulence => {
                let diff: V = std::array::from_fn(|i| position[i] - self.position[i]);
                let distance = if self.attenuation != 0. || self.maximum.is_some() {
                    Some(length(diff)?)
                } else {
                    None
                };
                if self
                    .maximum
                    .is_some_and(|limit| distance.is_some_and(|value| value > limit))
                {
                    velocity
                } else {
                    let attenuation =
                        distance.map_or(1., |distance| 1. + distance * self.attenuation);
                    let scale = self.magnitude / attenuation;
                    let turbulence = [
                        random.unit() * 2. - 1.,
                        random.unit() * 2. - 1.,
                        random.unit() * 2. - 1.,
                    ];
                    std::array::from_fn(|i| velocity[i] + turbulence[i] * scale)
                }
            }
        };
        ensure!(
            result.into_iter().all(f32::is_finite),
            "Particle force overflow"
        );
        Ok(result)
    }
}
/// Wii804fa2f8..804fa628, including the directional projection branch. The EM1 general
/// denominator is distance*attenuation (or1 at zero distance), unlike the SDK.
fn drag_velocity(position:V,velocity:V,field:V,magnitude:f32,attenuation:f32,
    maximum:Option<f32>,delta:f32,axis:Option<V>)->Result<V> {
    let reduction=magnitude*delta;
    if attenuation==0. && axis.is_none() {
        if let Some(maximum)=maximum {
            if reduction==0. {return Ok(velocity)}
            let offset=std::array::from_fn(|i|position[i]-field[i]);
            if !(squared(offset)<=maximum*maximum) {return Ok(velocity)}
        }
        return Ok(if reduction<1. {let factor=1.-reduction;velocity.map(|v|v*factor)} else {[0.;3]});
    }
    let offset=std::array::from_fn(|i|position[i]-field[i]);
    let distance=length(offset)?;
    if maximum.is_some_and(|maximum|!(distance<=maximum)) {return Ok(velocity)}
    let denominator=if distance==0. {1.} else {distance*attenuation};
    let projection=if let Some(axis)=axis {
        let direction=if length(velocity)?>f32::from_bits(0x358637bd) {normalize(velocity)?} else {[0.;3]};
        direction[0].mul_add(axis[0],direction[1]*axis[1])+direction[2]*axis[2]
    } else {1.};
    let reduction=(projection*reduction)/denominator;
    Ok(if reduction<1. {velocity.map(|v|v-(reduction*v))} else {[0.;3]})
}
fn squared(v: V) -> f32 {
    v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1]
}
fn inverse_root(value: f32) -> Result<f32> {
    ensure!(
        value.is_finite() && value > 0.,
        "Invalid field normalization length"
    );
    let estimate = super::super::pose_blend::estimate_double(value);
    // Gekko fmuls rounds its C operand to25 significant bits. The estimate
    // remains double precision on the A side until this first single result.
    let bits = estimate.to_bits();
    let rounded = f64::from_bits(((bits >> 28) + ((bits >> 27) & 1)) << 28);
    let square = (estimate * rounded) as f32;
    let half = (estimate * 0.5) as f32;
    Ok((-square.mul_add(value, -3.)) * half)
}
fn normalize(v: V) -> Result<V> {
    let inverse = inverse_root(squared(v))?;
    Ok(v.map(|v| v * inverse))
}
fn length(v: V) -> Result<f32> {
    let value = squared(v);
    Ok(if value == 0. {
        0.
    } else {
        value * inverse_root(value)?
    })
}

#[cfg(any(test, debug_assertions))]
pub(super) fn validate_radial_instructions() -> Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/radial_field.json"
    ))?;
    let cases = fixture["cases"].as_array().unwrap();
    ensure!(cases.len() == 594, "Radial source coverage changed");
    for case in cases {
        let vector = |name: &str| std::array::from_fn(|i| case[name][i].as_f64().unwrap() as f32);
        let scalar = |name: &str| case[name].as_f64().unwrap() as f32;
        let force = Prepared {
            kind: Kind::Radial,
            magnitude: scalar("magnitude"),
            position: vector("field_position"),
            direction: [0.; 3],
            attenuation: scalar("attenuation"),
            maximum: case["maximum"].as_f64().map(|v| v as f32),
            radial_type: scalar("radial_type"),
            air_friction: 0.,
            air_spread_cos: 0.,
            air_spread: false, drag_directional:false,
        };
        let result = force.apply(
            vector("position"),
            vector("velocity"),
            scalar("time"),
            scalar("last"),
        )?;
        let expected: [u32; 3] = std::array::from_fn(|i| case["bits"][i].as_u64().unwrap() as u32);
        ensure!(
            result.map(f32::to_bits) == expected,
            "Radial source bits differ: got {:?}, expected {:?}; {case}",
            result.map(f32::to_bits),
            expected
        );
    }
    println!("PASS: 594 original radial field cases, including splash attenuation, distance gates and native power");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn turbulence_force_uses_authored_cadence_and_distance_falloff() -> Result<()> {
        use super::*;
        let mut force = Force {
            name: "turbulence".into(),
            kind: Kind::Turbulence,
            magnitude: 2.,
            world_direction: false,
            active: true,
            node: None,
            attenuation: 0.5,
            maximum: Some(10.),
            radial_type: 0.,
            subtype: 0,
            directional: false,
            direction: [0.; 3],
            frame: [
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            frequency_update_time: 0.25,
            last_update_time: None,
            air_friction: 0.,
            inherited_velocity: 0.,
            inherit_rotation: false,
            enable_spread: false,
            spread: 0.,
            last_air_position: None,
            last_air_update: None,
            controls: Vec::new(),
        };
        assert!(force.prepare(1., None)?.is_none());
        assert!(force.prepare(1.24, None)?.is_none());
        assert!(force.prepare(1.25, None)?.is_none());
        assert_eq!(force.last_update_time, Some(1.25));
        // An actual field owner must pass preparation, not just a manually
        // constructed Prepared kernel. Retail FireFly_03 uses attenuation.
        force.node = Some(0);
        force.last_update_time = None;
        let prepared = force.prepare(1., None)?.context("Turbulence field rejected")?;
        let mut random = crate::blotworx::particle_random::Stream::Preview { seed: 1 };
        let result = prepared.apply_with_random([3., 0., 0.], [1., 2., 3.], 1., 0., &mut random)?;
        let factor = 2. / (1. + 3. * 0.5);
        assert!(result
            .into_iter()
            .zip([1., 2., 3.])
            .all(|(actual, base)| (actual - base).abs() <= factor));
        let mut random = crate::blotworx::particle_random::Stream::Preview { seed: 1 };
        assert_eq!(
            prepared.apply_with_random([11., 0., 0.], [1., 2., 3.], 1., 0., &mut random)?,
            [1., 2., 3.]
        );
        Ok(())
    }

    #[test]
    fn air_field_accelerates_only_up_to_its_flow_speed_and_honors_range() -> Result<()> {
        let force = Prepared {
            kind: Kind::Air,
            magnitude: 10.,
            position: [0.; 3],
            direction: [10., 0., 0.],
            attenuation: 0.,
            maximum: Some(2.),
            radial_type: 0.,
            air_friction: 1.,
            air_spread_cos: 0.,
            air_spread: false, drag_directional:false,
        };
        assert_eq!(force.apply([0.; 3], [0.; 3], 1., 0.)?, [10., 0., 0.]);
        assert_eq!(force.apply([0.; 3], [20., 0., 0.], 1., 0.)?, [20., 0., 0.]);
        assert_eq!(force.apply([3., 0., 0.], [0.; 3], 1., 0.)?, [0.; 3]);
        Ok(())
    }

    #[test]
    fn vortex_attenuation_and_distance_gates_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/vortex_field.json"
        ))?;
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 594);
        for case in cases {
            let vector =
                |name: &str| std::array::from_fn(|i| case[name][i].as_f64().unwrap() as f32);
            let scalar = |name: &str| case[name].as_f64().unwrap() as f32;
            let force = Prepared {
                kind: Kind::Vortex,
                magnitude: scalar("magnitude"),
                position: vector("field_position"),
                direction: vector("direction"),
                attenuation: scalar("attenuation"),
                maximum: case["maximum"].as_f64().map(|v| v as f32),
                radial_type: 0.,
                air_friction: 0.,
                air_spread_cos: 0.,
                air_spread: false, drag_directional:false,
            };
            let result = force.apply(
                vector("position"),
                vector("velocity"),
                scalar("time"),
                scalar("last"),
            )?;
            let expected: [u32; 3] =
                std::array::from_fn(|i| case["bits"][i].as_u64().unwrap() as u32);
            assert_eq!(result.map(f32::to_bits), expected, "{case}");
        }
        Ok(())
    }
    use super::*;
    #[test]
    fn gravity_field_preserves_velocity_and_integrates_signed_magnitude() -> Result<()> {
        let prepared = Prepared {
            kind: Kind::Gravity,
            magnitude: -50.,
            position: [0.; 3],
            direction: [0., -1., 0.],
            attenuation: 0.,
            maximum: None,
            radial_type: 0.,
            air_friction: 0.,
            air_spread_cos: 0.,
            air_spread: false, drag_directional:false,
        };
        assert_eq!(
            prepared.apply([0.; 3], [2., 3., 4.], 0.5, 0.25)?,
            [2., 15.5, 4.]
        );
        assert_eq!(
            prepared.apply([0.; 3], [2., 3., 4.], 0.5, 0.5)?,
            [2., 3., 4.]
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gremlin_fields_bind_and_animate_in_authored_order() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let files = crate::pack_io::read_filtered(
            std::fs::read(root.join("packfiles/gremlinvillage_zonea.pak"))?,
            |_, path| {
                [
                    "Effects/_Shared/fx_gremlin_warp.nif_wii",
                    "Effects/_Shared/fx_gremlin_warp_in.nif_wii",
                ]
                .iter()
                .any(|wanted| path.eq_ignore_ascii_case(wanted))
            },
        )?
        .pak
        .files;
        let mut kinds = Vec::new();
        let mut animated = 0;
        for file in files.into_iter().filter(|file| !file.data.is_empty()) {
            let nif = NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
            for (index, block) in nif.blocks.iter().enumerate() {
                let NiObject::NiPSParticleSystem(system) = block else {
                    continue;
                };
                let plan = super::super::SimulatorPlan::load(
                    &nif,
                    system
                        .simulator_link
                        .context("Missing original simulator")?,
                )?;
                let mut forces = Vec::new();
                for step in plan.steps {
                    if nif.blocks[step as usize].type_name() != "NiPSSimulatorForcesStep" {
                        continue;
                    }
                    let mut r = raw(&nif, step, "NiPSSimulatorForcesStep")?;
                    for _ in 0..r.uint()? {
                        forces.push(Force::load(&nif, r.uint()?, None)?);
                    }
                    r.done()?;
                }
                for controller in super::super::super::scene_bounds::controllers(
                    &nif,
                    system.base.base.base.base.controller_link,
                )? {
                    if controller.class_name != "NiPSFieldMagnitudeCtlr" {
                        continue;
                    }
                    let control = Magnitude::load(&nif, &controller, index as u32)?;
                    let field = forces
                        .iter_mut()
                        .find(|field| field.name == control.name)
                        .context("Original field controller lost its force")?;
                    assert_eq!(field.kind, Kind::Vortex);
                    assert_eq!(control.rate.sample(control.start)?, 0.);
                    assert_eq!(control.rate.sample(control.stop)?, -80.);
                    field.controls.push(control);
                    animated += 1;
                }
                for mut field in forces {
                    kinds.push(field.kind);
                    if field.controls.is_empty() {
                        let prepared = field
                            .prepare(0., None)?
                            .context("Original constant field was disabled")?;
                        assert_eq!(
                            prepared.magnitude,
                            if field.kind == Kind::Radial { -6. } else { 5. }
                        );
                    } else {
                        // All five body-part systems share this authored curve:
                        // zero initially, a negative vortex later, looping at stop.
                        let start = field.controls[0].start;
                        let stop = field.controls[0].stop;
                        assert!(field.prepare(start, None)?.is_none());
                        let later = field
                            .prepare(1., None)?
                            .context("Animated vortex never activated")?;
                        assert!(later.magnitude < 0. && later.magnitude > -80.);
                        assert!(later.direction.into_iter().all(f32::is_finite));
                        assert!(
                            field.prepare(stop, None)?.is_none(),
                            "Loop endpoint did not return to the authored zero"
                        );
                        field.reset();
                        assert!(field.prepare(start, None)?.is_none());
                        // Registered-template preparation and the actual base
                        // Start time must let this same field play long after
                        // application time zero without rebasing its caller.
                        let mut clone = field.clone();
                        for control in &mut clone.controls {
                            control.prepare_effect_template();
                            assert_eq!(control.flags & 9, 1);
                        }
                        assert!(clone.prepare(0., None)?.is_none());
                        let mut baseline = clone.clone();
                        for control in &mut baseline.controls {
                            control.start_at(0.);
                        }
                        for control in &mut clone.controls {
                            control.start_at(12.25);
                            assert_eq!(control.application_start, Some(12.25));
                        }
                        // This source has a nonzero low key and LOOP mode:
                        // phase0 wraps near its end. Preserve that authored
                        // behavior instead of requiring an initially off field.
                        for time in [0., 0.25, 0.5, 1.] {
                            let shifted = clone.prepare(12.25 + time, None)?.map(|p| p.magnitude);
                            let expected = baseline.prepare(time, None)?.map(|p| p.magnitude);
                            assert_eq!(shifted, expected);
                        }
                    }
                }
            }
        }
        assert_eq!(animated, 5);
        assert_eq!(
            kinds.iter().filter(|&&kind| kind == Kind::Vortex).count(),
            5
        );
        assert_eq!(
            kinds.iter().filter(|&&kind| kind == Kind::Radial).count(),
            1
        );
        assert_eq!(kinds.iter().filter(|&&kind| kind == Kind::Drag).count(), 1);
        Ok(())
    }
    #[test]
    fn prepared_forces_match_original_kernel_and_vector_helper_instructions() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/particle_force.json"
        ))?;
        let cases = data["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 300);
        for c in cases {
            let vector =
                |name: &str| -> V { std::array::from_fn(|i| c[name][i].as_f64().unwrap() as f32) };
            let kind = match c["kind"].as_str().unwrap() {
                "drag" => Kind::Drag,
                "radial" => Kind::Radial,
                "vortex" => Kind::Vortex,
                _ => unreachable!(),
            };
            let prepared = Prepared {
                kind,
                magnitude: c["magnitude"].as_f64().unwrap() as f32,
                position: [0.; 3],
                direction: vector("direction"),
                attenuation: 0.,
                maximum: None,
                radial_type: 0.,
                air_friction: 0.,
                air_spread_cos: 0.,
                air_spread: false, drag_directional:false,
            };
            let got = prepared.apply(
                vector("position"),
                vector("velocity"),
                c["time"].as_f64().unwrap() as f32,
                c["last"].as_f64().unwrap() as f32,
            )?;
            let expected =
                std::array::from_fn::<_, 3, _>(|i| c["bits"][i].as_u64().unwrap() as u32);
            assert_eq!(got.map(f32::to_bits), expected, "{c}");
        }
        Ok(())
    }
}
