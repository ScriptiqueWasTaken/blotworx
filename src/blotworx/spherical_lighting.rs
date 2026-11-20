//! EM1 SH projection, attenuation and texture evaluation (REL 0x24c424..0x24d54c).
//! Scene registration, raycast visibility and runtime binding remain separate.
use anyhow::{ensure, Context, Result};

pub const SIZE: usize = 16;
pub type Coefficients = [[f32; 3]; 9];

/// A model instance's node identity; equal asset names do not imply ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundOwner {
    pub entity: u64,
    pub node: u32,
}

#[derive(Clone, Debug)]
pub struct SharedProperty {
    pub owner: BoundOwner,
    meshes: std::collections::BTreeSet<(u64, u32)>,
    pixels: Option<std::sync::Arc<Vec<u8>>>,
    coefficients: Option<Coefficients>,
}

/// Tracks SH property identity separately from child components' non-owning
/// pointers. GPU textures and visibility state will belong to the same record.
#[derive(Clone, Default, Debug)]
pub struct PropertyStore {
    pub texture_evaluations: u64,
    pub coefficient_reuses: u64,
    next_id: u64,
    properties: std::collections::BTreeMap<u64, SharedProperty>,
    mesh_properties: std::collections::BTreeMap<(u64, u32), u64>,
}
impl PropertyStore {
    pub fn allocate(&mut self, owner: BoundOwner) -> Result<u64> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .context("SH property identity exhausted")?;
        self.properties.insert(
            id,
            SharedProperty {
                owner,
                meshes: Default::default(),
                pixels: None,
                coefficients: None,
            },
        );
        Ok(id)
    }
    pub fn get(&self, id: u64) -> Result<&SharedProperty> {
        self.properties
            .get(&id)
            .with_context(|| format!("Missing SH property {id}"))
    }
    /// Resolve the owner's current world sphere explicitly. Unchanged pixels
    /// retain their Arc identity, avoiding redundant viewport texture uploads.
    /// Compute before committing so failed visibility/bound input leaves the
    /// last valid frame intact and still returns the error to the caller.
    pub fn update_image<'a>(
        &mut self,
        id: u64,
        bound: super::scene_bounds::Sphere,
        lights: impl IntoIterator<Item = (&'a Light, f32)>,
        view: &[f32; 16],
    ) -> Result<std::sync::Arc<Vec<u8>>> {
        self.get(id)?;
        ensure!(
            bound.center.iter().all(|v| v.is_finite())
                && bound.radius.is_finite()
                && bound.radius >= 0.,
            "Invalid SH owner world bound"
        );
        // Validate/sample every live light before testing reuse. No light,
        // visibility or camera changes can be hidden behind the pixel cache.
        let coefficients = coefficients(lights, bound.center, camera_rotation(view)?)?;
        let property = self.get(id)?;
        if property.coefficients.as_ref().is_some_and(|old| {
            old.iter()
                .flatten()
                .zip(coefficients.iter().flatten())
                .all(|(a, b)| a.to_bits() == b.to_bits())
        }) {
            if let Some(pixels) = &property.pixels {
                let pixels = pixels.clone();
                self.coefficient_reuses += 1;
                return Ok(pixels);
            }
        }
        let bytes = texture(&coefficients)?;
        self.texture_evaluations += 1;
        let property = self
            .properties
            .get_mut(&id)
            .context("SH property disappeared")?;
        property.coefficients = Some(coefficients);
        if let Some(pixels) = &property.pixels {
            if pixels.as_slice() == bytes.as_slice() {
                return Ok(pixels.clone());
            }
        }
        let pixels = std::sync::Arc::new(bytes);
        property.pixels = Some(pixels.clone());
        Ok(pixels)
    }
    /// REL 0x24f428: an already attached property prevents reassignment.
    /// Returns the effective property so callers keep texture ownership aligned.
    pub fn attach(&mut self, mesh: (u64, u32), candidate: u64) -> Result<u64> {
        if let Some(&existing) = self.mesh_properties.get(&mesh) {
            return Ok(existing);
        }
        let property = self
            .properties
            .get_mut(&candidate)
            .with_context(|| format!("Missing SH property {candidate}"))?;
        property.meshes.insert(mesh);
        self.mesh_properties.insert(mesh, candidate);
        Ok(candidate)
    }
    pub fn detach(&mut self, mesh: (u64, u32)) -> Result<Option<u64>> {
        let Some(&id) = self.mesh_properties.get(&mesh) else {
            return Ok(None);
        };
        let property = self
            .properties
            .get_mut(&id)
            .with_context(|| format!("Missing attached SH property {id}"))?;
        ensure!(
            property.meshes.remove(&mesh),
            "SH property attachment mismatch"
        );
        self.mesh_properties.remove(&mesh);
        Ok(Some(id))
    }
    /// REL 0x24f670 / 0x24e448: remove only an unreferenced property.
    /// Callers clear the child component's pointer only when None is returned.
    pub fn collect_unreferenced(&mut self, id: Option<u64>) -> Result<Option<u64>> {
        let Some(id) = id else { return Ok(None) };
        if !self.get(id)?.meshes.is_empty() {
            return Ok(Some(id));
        }
        self.properties.remove(&id);
        Ok(None)
    }
}

/// Entity transform ancestry and an already attached native SH property.
/// IDs are runtime entity/property identities, never prefab identities.
#[derive(Clone, Copy, Debug)]
pub struct OwnerLink {
    pub parent: Option<u64>,
    pub property: Option<u64>,
}

/// REL 0x24e36c..0x24e434. Every non-null ancestor property replaces
/// the candidate; the walk does NOT stop at the closest match. A temporary
/// engine override changes the starting entity, not the selected property.
/// None means the caller must create a property owned by the current model.
pub fn inherited_property(
    entity: u64,
    override_entity: Option<u64>,
    entities: &std::collections::BTreeMap<u64, OwnerLink>,
) -> Result<Option<u64>> {
    let mut cursor = Some(override_entity.unwrap_or(entity));
    let mut seen = std::collections::BTreeSet::new();
    let mut property = None;
    while let Some(id) = cursor {
        ensure!(
            seen.insert(id),
            "Cycle in SH transform ancestry at entity {id}"
        );
        let link = entities
            .get(&id)
            .with_context(|| format!("Missing SH transform ancestor {id}"))?;
        if link.property.is_some() {
            property = link.property;
        }
        cursor = link.parent;
    }
    Ok(property)
}

#[cfg(test)]
mod owner_tests {
    use super::*;
    #[test]
    fn shared_property_survives_other_meshes_and_preserves_existing_attachment() -> Result<()> {
        let mut store = PropertyStore::default();
        let owner = BoundOwner { entity: 4, node: 0 };
        let first = store.allocate(owner)?;
        let second = store.allocate(BoundOwner { entity: 5, node: 0 })?;
        assert_eq!(store.attach((4, 1), first)?, first);
        assert_eq!(store.attach((4, 1), second)?, first);
        assert_eq!(store.attach((5, 2), first)?, first);
        assert_eq!(store.get(first)?.owner, owner);
        assert_eq!(store.collect_unreferenced(Some(second))?, None);
        assert!(store.attach((6, 1), second).is_err());
        assert_eq!(store.detach((4, 1))?, Some(first));
        assert_eq!(store.collect_unreferenced(Some(first))?, Some(first));
        assert_eq!(store.detach((5, 2))?, Some(first));
        assert_eq!(store.collect_unreferenced(Some(first))?, None);
        assert!(store.get(first).is_err());
        assert_eq!(store.detach((5, 2))?, None);
        assert!(store.allocate(owner)? > second);
        Ok(())
    }
    #[test]
    fn shared_image_reuses_unchanged_pixels_and_drops_collected_storage() -> Result<()> {
        use std::sync::Arc;
        let mut store = PropertyStore::default();
        let id = store.allocate(BoundOwner { entity: 1, node: 0 })?;
        let view = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        let bound = super::super::scene_bounds::Sphere {
            center: [0.; 3],
            radius: 1.,
        };
        let first = store.update_image(id, bound, std::iter::empty(), &view)?;
        assert!(first.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        let second = store.update_image(id, bound, std::iter::empty(), &view)?;
        assert!(Arc::ptr_eq(&first, &second));
        let invalid = super::super::scene_bounds::Sphere {
            radius: f32::NAN,
            ..bound
        };
        assert!(store
            .update_image(id, invalid, std::iter::empty(), &view)
            .is_err());
        assert!(Arc::ptr_eq(&first, store.get(id)?.pixels.as_ref().unwrap()));
        let weak = Arc::downgrade(&first);
        drop(first);
        drop(second);
        store.collect_unreferenced(Some(id))?;
        assert!(weak.upgrade().is_none());
        Ok(())
    }
    #[test]
    fn highest_existing_property_wins_and_override_changes_start() -> Result<()> {
        let mut entities = std::collections::BTreeMap::from([
            (
                1,
                OwnerLink {
                    parent: Some(2),
                    property: Some(10),
                },
            ),
            (
                2,
                OwnerLink {
                    parent: Some(3),
                    property: Some(20),
                },
            ),
            (
                3,
                OwnerLink {
                    parent: None,
                    property: Some(30),
                },
            ),
            (
                4,
                OwnerLink {
                    parent: None,
                    property: None,
                },
            ),
        ]);
        assert_eq!(inherited_property(1, None, &entities)?, Some(30));
        entities.get_mut(&3).unwrap().property = None;
        assert_eq!(inherited_property(1, None, &entities)?, Some(20));
        assert_eq!(inherited_property(1, Some(4), &entities)?, None);
        assert!(inherited_property(1, Some(99), &entities).is_err());
        entities.get_mut(&3).unwrap().parent = Some(1);
        assert!(inherited_property(1, None, &entities).is_err());
        Ok(())
    }
}

/// REL 0x24d168..0x24d210 builds columns [-right, up, direction], then
/// DOL 0x804c0384 transposes them. The renderer's column-major OpenGL view
/// has rows [right, up, -direction]; position and projection do not belong here.
pub fn camera_rotation(view: &[f32; 16]) -> Result<[[f32; 3]; 3]> {
    ensure!(
        view.iter().all(|v| v.is_finite()),
        "Nonfinite SH view matrix"
    );
    Ok([
        [-view[0], -view[4], -view[8]],
        [view[1], view[5], view[9]],
        [-view[2], -view[6], -view[10]],
    ])
}

/// Render-ready CPU pixels for an explicitly resolved light/visibility list.
/// Camera movement changes the lighting map even when the object is stationary.
pub fn view_texture<'a>(
    lights: impl IntoIterator<Item = (&'a Light, f32)>,
    position: [f32; 3],
    view: &[f32; 16],
) -> Result<Vec<u8>> {
    texture(&coefficients(lights, position, camera_rotation(view)?)?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum LightKind {
    Ambient,
    Directional,
    Point,
    Spot,
}

/// Fields consumed from the native 0x60-byte SH light record. These are not
/// interchangeable with the editor's approximate GpuLight defaults.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Light {
    pub enabled: bool,         // +4
    pub position: [f32; 3],    // +8
    pub direction: [f32; 3],   // +0x14
    pub kind: LightKind,       // +0x20
    pub dimmer: f32,           // +0x24
    pub ambient: [f32; 3],     // +0x28
    pub diffuse: [f32; 3],     // +0x34
    pub range_start: f32,      // +0x40
    pub range_end: f32,        // +0x44; positive selects linear range
    pub attenuation: [f32; 3], // +0x48; constant, linear, quadratic
    pub spot_outer_cos: f32,   // +0x54
    pub spot_inner_cos: f32,   // +0x58
    pub spot_exponent: f32,    // +0x5c
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct RegisteredLight {
    pub scene: String,
    pub entity_id: u32,
    pub name: String,
    /// Authored request; global SphericalHarmonicLightingRayCast gates it later.
    pub raycast: bool,
    pub light: Light,
}

/// Decode the property reads in REL 0x24d990. Missing required values are
/// diagnosed, never replaced by editor defaults or catalog sample values.
pub fn authored<'a>(
    mut light_property: impl FnMut(&str) -> Option<&'a serde_json::Value>,
    mut baking_property: impl FnMut(&str) -> Option<&'a serde_json::Value>,
    position: [f32; 3],
    rotation: [[f32; 3]; 3],
) -> Result<Option<(Light, bool)>> {
    let flag = |value: Option<&serde_json::Value>, name: &str| -> Result<bool> {
        value
            .and_then(|v| v.as_bool())
            .with_context(|| format!("Missing/invalid SH {name}"))
    };
    if !flag(
        baking_property("GenerateRuntimeLight"),
        "GenerateRuntimeLight",
    )? {
        return Ok(None);
    }
    let subtractive = flag(baking_property("LightIsSubtractive"), "LightIsSubtractive")?;
    let raycast = flag(
        baking_property("RuntimeLightRayCast"),
        "RuntimeLightRayCast",
    )?;
    let number = |value: Option<&serde_json::Value>, name: &str| -> Result<f32> {
        let value = value
            .and_then(|v| v.as_f64())
            .with_context(|| format!("Missing/invalid SH {name}"))? as f32;
        ensure!(value.is_finite(), "Nonfinite SH {name}");
        Ok(value)
    };
    let color = |value: Option<&serde_json::Value>, name: &str| -> Result<[f32; 3]> {
        let value = value.with_context(|| format!("Missing SH {name}"))?;
        let mut color = [
            number(value.get("r"), name)?,
            number(value.get("g"), name)?,
            number(value.get("b"), name)?,
        ];
        for channel in &mut color {
            *channel *= 255.;
            if subtractive {
                *channel *= -1.;
            }
        }
        Ok(color)
    };
    let kind = match light_property("Light Type").and_then(|v| v.as_str()) {
        Some("Ambient") => LightKind::Ambient,
        Some("Directional") => LightKind::Directional,
        Some("Point") => LightKind::Point,
        Some("Spot") => LightKind::Spot,
        value => anyhow::bail!("Missing/unknown SH Light Type: {value:?}"),
    };
    // Original uses the positive third column, without model scale or a
    // normalization pass. Preserve the authored matrix rather than GpuLight's -Z.
    ensure!(
        position
            .iter()
            .chain(rotation.iter().flatten())
            .all(|v| v.is_finite()),
        "Invalid SH light transform"
    );
    let cosine = |degrees: f32| {
        let radians = ((3.1415927410125732_f32 * degrees) / 180.) * 0.5;
        (radians as f64).cos() as f32
    };
    let light = Light {
        enabled: true,
        kind,
        position,
        direction: rotation.map(|row| row[2]),
        dimmer: number(light_property("Dimmer"), "Dimmer")?,
        ambient: color(light_property("Color (Ambient)"), "Color (Ambient)")?,
        diffuse: color(light_property("Color (Diffuse)"), "Color (Diffuse)")?,
        range_start: number(baking_property("Min Falloff Range"), "Min Falloff Range")?,
        range_end: number(baking_property("Max Falloff Range"), "Max Falloff Range")?,
        attenuation: [
            number(
                light_property("Attenuation (Constant)"),
                "Attenuation (Constant)",
            )?,
            number(
                light_property("Attenuation (Linear)"),
                "Attenuation (Linear)",
            )?,
            number(
                light_property("Attenuation (Quadratic)"),
                "Attenuation (Quadratic)",
            )?,
        ],
        spot_inner_cos: cosine(number(
            light_property("Spot Angle (Inner)"),
            "Spot Angle (Inner)",
        )?),
        spot_outer_cos: cosine(number(
            light_property("Spot Angle (Outer)"),
            "Spot Angle (Outer)",
        )?),
        spot_exponent: number(light_property("Spot Exponent"), "Spot Exponent")?,
    };
    Ok(Some((light, raycast)))
}

/// DOL 0x805aea20/0x805ae9b0 use one refinement, unlike the lookup's two.
pub(super) fn vector_length_and_direction(v: [f32; 3]) -> (f32, [f32; 3]) {
    let squared = v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    if squared == 0. {
        return (0., [0.; 3]);
    }
    let estimate = super::pose_blend::estimate_double(squared);
    let square = super::pose_blend::square_estimate(estimate);
    let half = (estimate * 0.5) as f32;
    let inverse = (-square).mul_add(squared, 3.) * half;
    let length = squared * inverse;
    if length <= 1e-6_f32 {
        (0., [0.; 3])
    } else {
        (length, v.map(|x| x * inverse))
    }
}

impl Light {
    /// REL 0x24cc14: distance/cone weight, before optional raycast fading.
    /// The original computes distance even for ambient/directional records.
    fn sample(&self, position: [f32; 3]) -> Result<(f32, [f32; 3])> {
        ensure!(
            self.position
                .iter()
                .chain(&self.direction)
                .chain(&self.ambient)
                .chain(&self.diffuse)
                .chain(&self.attenuation)
                .copied()
                .chain([
                    self.dimmer,
                    self.range_start,
                    self.range_end,
                    self.spot_outer_cos,
                    self.spot_inner_cos,
                    self.spot_exponent
                ])
                .all(f32::is_finite),
            "Nonfinite SH light"
        );
        if !self.enabled || self.dimmer == 0. {
            return Ok((0., [0.; 3]));
        }
        let vector = std::array::from_fn(|i| self.position[i] - position[i]);
        let (distance, direction) = vector_length_and_direction(vector);
        ensure!(
            distance.is_finite() && direction.iter().all(|v| v.is_finite()),
            "SH light distance overflow"
        );
        if distance == 0. && self.kind != LightKind::Directional {
            return Ok((0., direction));
        }
        let mut attenuation = 1.;
        if self.range_end > 0. {
            if distance > self.range_end {
                return Ok((0., direction));
            }
            if distance >= self.range_start {
                attenuation =
                    1. - (distance - self.range_start) / (self.range_end - self.range_start);
            }
        } else if matches!(self.kind, LightKind::Point | LightKind::Spot) {
            let [constant, linear, quadratic] = self.attenuation;
            let denominator = constant + distance.mul_add(quadratic * distance, linear * distance);
            if (denominator as f64) < 1e-6_f64 {
                return Ok((0., direction));
            }
            attenuation = (1. / denominator).clamp(0., 1.);
        }
        let mut cone = 1.;
        if self.kind == LightKind::Spot {
            let opposite = direction.map(|v| -v);
            let cosine = self.direction[0].mul_add(opposite[0], self.direction[1] * opposite[1])
                + self.direction[2] * opposite[2];
            if cosine <= self.spot_inner_cos {
                if cosine < self.spot_outer_cos {
                    return Ok((0., direction));
                }
                let ratio =
                    (cosine - self.spot_outer_cos) / (self.spot_inner_cos - self.spot_outer_cos);
                // Native calls double pow then frsp. Host libm bit parity remains unverified.
                cone = (ratio as f64).powf(self.spot_exponent as f64) as f32;
            }
        }
        let weight = cone * (self.dimmer * attenuation);
        ensure!(
            weight.is_finite(),
            "Invalid SH attenuation or cone interval"
        );
        Ok((
            if weight < 0.003921568859368563_f32 {
                0.
            } else {
                weight
            },
            direction,
        ))
    }
}

/// Irradiance projection, REL 0x24c424. Its band weights differ from the
/// evaluation basis below; using the same basis for both changes the lighting.
fn project([x, y, z]: [f32; 3]) -> [f32; 9] {
    let linear = 0.3257350027561188_f32;
    let quadratic = 0.1365685611963272_f32;
    let negative_z = -0.2731371223926544_f32 * z;
    [
        0.282094806432724_f32,
        linear * y,
        linear * z,
        linear * x,
        quadratic * x.mul_add(y, y * x),
        negative_z * y,
        0.236543670296669_f32.mul_add(z * z, -0.07884789258241653_f32),
        negative_z * x,
        quadratic * x.mul_add(x, -(y * y)),
    ]
}

/// REL 0x24ce44. Camera rotation is native row-major, before the routine's
/// explicit X/Z sign flips. Preserve native light order for float accumulation.
fn accumulate(
    coefficients: &mut Coefficients,
    light: &Light,
    direction: [f32; 3],
    weight: f32,
    camera_rotation: [[f32; 3]; 3],
) {
    let ambient_scale = 3.544907808303833_f32 * weight;
    for channel in 0..3 {
        coefficients[0][channel] =
            light.ambient[channel].mul_add(ambient_scale, coefficients[0][channel]);
    }
    if light.kind == LightKind::Ambient {
        return;
    }
    let direction = if light.kind == LightKind::Directional {
        light.direction.map(|v| -v)
    } else {
        direction
    };
    let rotated = camera_rotation.map(|row| {
        row[2].mul_add(
            direction[2],
            row[0].mul_add(direction[0], row[1] * direction[1]),
        )
    });
    let basis = project([-rotated[0], rotated[1], -rotated[2]]);
    let scale = 2.9567930698394775_f32 * weight;
    let diffuse = light.diffuse.map(|v| v * scale);
    for (coefficient, basis) in coefficients.iter_mut().zip(basis) {
        for channel in 0..3 {
            coefficient[channel] = diffuse[channel].mul_add(basis, coefficient[channel]);
        }
    }
}

/// Build the nine coefficients for one object. Callers must supply the native
/// raycast visibility/fade factor for each light (1 only when unoccluded or the
/// record doesn't request raycasting); this function never invents visibility.
pub fn coefficients<'a>(
    lights: impl IntoIterator<Item = (&'a Light, f32)>,
    position: [f32; 3],
    camera_rotation: [[f32; 3]; 3],
) -> Result<Coefficients> {
    ensure!(
        position
            .iter()
            .chain(camera_rotation.iter().flatten())
            .all(|v| v.is_finite()),
        "Nonfinite SH object/camera transform"
    );
    let mut output = [[0.; 3]; 9];
    for (light, visibility) in lights {
        ensure!(
            visibility.is_finite() && (0. ..=1.).contains(&visibility),
            "Invalid SH visibility"
        );
        let (weight, direction) = light.sample(position)?;
        if weight > 0. {
            accumulate(
                &mut output,
                light,
                direction,
                weight * visibility,
                camera_rotation,
            );
        }
    }
    ensure!(
        output.iter().flatten().all(|v| v.is_finite()),
        "SH coefficient overflow"
    );
    Ok(output)
}

// REL 0x24d858..0x24d8c4: frsqrte narrowed to single, two refinements.
pub(super) fn native_sqrt(value: f32) -> f32 {
    if value < 3.000000085396614e-38_f32 {
        return 0.;
    }
    let mut estimate = super::pose_blend::estimate(value);
    let half = 0.5 * value;
    for _ in 0..2 {
        let product = half * estimate;
        let correction = (-estimate).mul_add(product, 1.5);
        estimate *= correction;
    }
    value * estimate
}

/// Lookup initialization at REL 0x24d7f8..0x24d910. Returned in row-major order;
/// native storage traverses the same directions in GX 4x4 tile order.
pub fn directions() -> [[f32; 3]; SIZE * SIZE] {
    std::array::from_fn(|i| {
        let mut x = 2.0f32.mul_add((i % SIZE) as f32 / 15., -1.);
        let mut y = 2.0f32.mul_add((i / SIZE) as f32 / 15., -1.);
        let length2 = x.mul_add(x, y * y);
        let z = if length2 <= 1. {
            native_sqrt(1. - length2)
        } else {
            let inverse = 1. / native_sqrt(length2);
            x *= inverse;
            y *= inverse;
            0.
        };
        [x, -y, z]
    })
}

fn evaluation_basis([x, y, z]: [f32; 3]) -> [f32; 9] {
    // Constants and operation order from the paired-single evaluation loop.
    let linear = 0.48860251903533936_f32;
    let quadratic = 0.5462742447853088_f32;
    let negative_z = z * -1.0925484895706177_f32;
    [
        0.282094806432724_f32,
        y * linear,
        z * linear,
        x * linear,
        x.mul_add(y, x * y) * quadratic,
        negative_z * y,
        (z * z).mul_add(0.946174681186676_f32, -0.31539157032966614_f32),
        negative_z * x,
        x.mul_add(x, -(y * y)) * quadratic,
    ]
}
fn evaluate_basis(basis: [f32; 9], coefficients: &Coefficients) -> [f32; 3] {
    let mut result = [0.; 3];
    for (basis, coefficient) in basis.into_iter().zip(coefficients) {
        for channel in 0..3 {
            result[channel] = basis.mul_add(coefficient[channel], result[channel]);
        }
    }
    result
}
#[cfg(test)]
fn evaluate(direction: [f32; 3], coefficients: &Coefficients) -> [f32; 3] {
    evaluate_basis(evaluation_basis(direction), coefficients)
}

/// Linear RGBA8 suitable for upload. Native GQR2 is unsigned-byte, scale zero:
/// saturate and truncate; no gamma conversion or added ambient floor.
pub fn texture(coefficients: &Coefficients) -> Result<Vec<u8>> {
    ensure!(
        coefficients.iter().flatten().all(|v| v.is_finite()),
        "Nonfinite SH coefficients"
    );
    let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
    // REL24d7f8 initializes the fixed hemisphere table once. Its evaluation
    // basis is also input-independent. Keep every original single/FMA value;
    // only the coefficient accumulation remains per texture/per frame.
    static BASIS: std::sync::OnceLock<[[f32; 9]; SIZE * SIZE]> = std::sync::OnceLock::new();
    for &basis in BASIS.get_or_init(|| directions().map(evaluation_basis)) {
        let color = evaluate_basis(basis, coefficients);
        ensure!(
            color.iter().all(|v| v.is_finite()),
            "SH evaluation overflow"
        );
        rgba.extend(color.map(|v| v.clamp(0., 255.) as u8));
        rgba.push(255);
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    const IDENTITY: [[f32; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    fn light(kind: LightKind) -> Light {
        Light {
            enabled: true,
            kind,
            position: [0., 0., 10.],
            direction: [0., 0., -1.],
            dimmer: 1.,
            ambient: [0.; 3],
            diffuse: [255.; 3],
            range_start: 0.,
            range_end: 0.,
            attenuation: [1., 0., 0.],
            spot_outer_cos: 0.5,
            spot_inner_cos: 0.9,
            spot_exponent: 2.,
        }
    }
    #[test]
    fn viewport_camera_adapter_preserves_native_basis_and_ignores_translation() -> Result<()> {
        use crate::render_support::viewport3d::look_at_matrix;
        let front = look_at_matrix([0., 0., 10.], [0.; 3], [0., 1., 0.]);
        let side = look_at_matrix([10., 0., 0.], [0.; 3], [0., 1., 0.]);
        assert_eq!(
            camera_rotation(&front)?,
            [[-1., 0., 0.], [0., 1., 0.], [0., 0., -1.]]
        );
        assert_eq!(
            camera_rotation(&side)?,
            [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]]
        );
        let mut translated = front;
        translated[12] = 100.;
        translated[13] = -20.;
        translated[14] = 7.;
        let mut l = light(LightKind::Directional);
        l.direction = [-1., 0., 0.];
        let original = view_texture([(&l, 1.)], [0.; 3], &front)?;
        assert_eq!(original, view_texture([(&l, 1.)], [0.; 3], &translated)?);
        assert_ne!(original, view_texture([(&l, 1.)], [0.; 3], &side)?);
        translated[0] = f32::NAN;
        assert!(view_texture([(&l, 1.)], [0.; 3], &translated).is_err());
        Ok(())
    }
    #[test]
    fn authored_properties_preserve_runtime_gate_sign_and_cone() -> Result<()> {
        let l = serde_json::json!({"Light Type":"Spot","Dimmer":0.5,
            "Color (Ambient)":{"r":0.25,"g":0.,"b":0.},
            "Color (Diffuse)":{"r":1.,"g":0.5,"b":0.},
            "Attenuation (Constant)":1.,"Attenuation (Linear)":0.,"Attenuation (Quadratic)":0.,
            "Spot Angle (Inner)":60.,"Spot Angle (Outer)":120.,"Spot Exponent":1.});
        let mut b = serde_json::json!({"GenerateRuntimeLight":true,"RuntimeLightRayCast":true,
            "LightIsSubtractive":true,"Min Falloff Range":5.,"Max Falloff Range":20.});
        let (decoded, raycast) =
            authored(|n| l.get(n), |n| b.get(n), [1., 2., 3.], IDENTITY)?.unwrap();
        assert!(raycast);
        assert_eq!(decoded.direction, [0., 0., 1.]);
        assert_eq!(decoded.position, [1., 2., 3.]);
        assert_eq!(decoded.diffuse, [-255., -127.5, -0.]);
        assert_eq!((decoded.range_start, decoded.range_end), (5., 20.));
        assert!((decoded.spot_outer_cos - 0.5).abs() < 1e-7);
        assert!((decoded.spot_inner_cos - 0.8660254).abs() < 1e-7);
        b["GenerateRuntimeLight"] = false.into();
        assert!(authored(|_| None, |n| b.get(n), [0.; 3], IDENTITY)?.is_none());
        b["GenerateRuntimeLight"] = true.into();
        assert!(authored(|_| None, |n| b.get(n), [0.; 3], IDENTITY).is_err());
        Ok(())
    }
    #[test]
    fn light_gates_preserve_black_and_visibility() -> Result<()> {
        let zero = [[0.; 3]; 9];
        assert_eq!(coefficients([], [0.; 3], IDENTITY)?, zero);
        let mut l = light(LightKind::Directional);
        assert_eq!(coefficients([(&l, 0.)], [0.; 3], IDENTITY)?, zero);
        l.enabled = false;
        assert_eq!(coefficients([(&l, 1.)], [0.; 3], IDENTITY)?, zero);
        l.enabled = true;
        l.dimmer = 0.003;
        assert_eq!(coefficients([(&l, 1.)], [0.; 3], IDENTITY)?, zero);
        l.dimmer = 1.;
        assert!(coefficients([(&l, f32::NAN)], [0.; 3], IDENTITY).is_err());
        assert!(coefficients([(&l, 1.)], [f32::INFINITY; 3], IDENTITY).is_err());
        Ok(())
    }
    #[test]
    fn camera_rotates_diffuse_but_never_ambient() -> Result<()> {
        let mut l = light(LightKind::Ambient);
        l.ambient = [32., 64., 96.];
        let a = coefficients([(&l, 1.)], [0.; 3], IDENTITY)?;
        assert!(a[1..].iter().flatten().all(|&v| v == 0.));
        let rotated = [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]];
        assert_eq!(a, coefficients([(&l, 1.)], [0.; 3], rotated)?);
        l.kind = LightKind::Directional;
        l.ambient = [0.; 3];
        let c = coefficients([(&l, 1.)], [0.; 3], IDENTITY)?;
        let r = coefficients([(&l, 1.)], [0.; 3], rotated)?;
        assert!(c[2][0] < 0. && c[3][0] == 0.);
        assert_eq!(r[3], c[2]);
        assert_eq!(r[2], [0.; 3]);
        let half = coefficients([(&l, 0.5)], [0.; 3], IDENTITY)?;
        for (full, half) in c.iter().flatten().zip(half.iter().flatten()) {
            assert_eq!(*full * 0.5, *half);
        }
        Ok(())
    }
    #[test]
    fn attenuation_uses_authored_range_or_polynomial_and_cone() -> Result<()> {
        let mut l = light(LightKind::Point);
        l.range_start = 5.;
        l.range_end = 15.;
        assert!((l.sample([0.; 3])?.0 - 0.5).abs() < 1e-6);
        assert_eq!(l.sample([0., 0., 9.])?.0, 1.);
        assert_eq!(l.sample([0., 0., -10.])?.0, 0.);
        // Native drops coincident nondirectional lights, not directional ones.
        assert_eq!(l.sample(l.position)?.0, 0.);
        l.range_end = 0.;
        l.attenuation = [2., 0., 0.];
        assert_eq!(l.sample([0.; 3])?.0, 0.5);
        l.attenuation = [0., 0., 0.];
        assert_eq!(l.sample([0.; 3])?.0, 0.);
        l.attenuation = [1., 0., 0.];
        l.kind = LightKind::Spot;
        assert_eq!(l.sample([0.; 3])?.0, 1.);
        assert_eq!(l.sample([0., 0., 20.])?.0, 0.);
        // On a unit direction, cosine 0.6 lies a quarter of the way through
        // the 0.5..0.9 penumbra; exponent 2 gives 1/16 intensity.
        assert!((l.sample([8., 0., 4.])?.0 - 0.0625).abs() < 1e-6);
        l.kind = LightKind::Directional;
        assert_eq!(l.sample(l.position)?.0, 1.);
        Ok(())
    }
    #[test]
    #[ignore = "writes a synthetic light diagnostic, not original-game parity evidence"]
    fn export_light_visualization() -> Result<()> {
        let mut l = light(LightKind::Directional);
        l.direction = [-0.6, 0., 0.8];
        let camera = [[0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]];
        let frames = [
            coefficients([], [0.; 3], IDENTITY)?,
            coefficients([(&l, 1.)], [0.; 3], IDENTITY)?,
            coefficients([(&l, 1.)], [0.; 3], camera)?,
            coefficients([(&l, 0.5)], [0.; 3], IDENTITY)?,
        ];
        let mut image = image::RgbaImage::new(64, 16);
        for (index, c) in frames.iter().enumerate() {
            let bytes = texture(c)?;
            for y in 0..16 {
                for x in 0..16 {
                    let i = (y * 16 + x) * 4;
                    image.put_pixel(
                        (index * 16 + x) as u32,
                        y as u32,
                        image::Rgba(bytes[i..i + 4].try_into()?),
                    );
                }
            }
        }
        image::imageops::resize(&image, 768, 192, image::imageops::FilterType::Nearest)
            .save("target/sh-light-diagnostic.png")?;
        Ok(())
    }
    #[test]
    fn lookup_is_a_front_hemisphere_with_clamped_outer_ring() {
        let values = directions();
        assert_eq!(values.len(), 256);
        assert!(values
            .iter()
            .all(|v| v[2] >= 0. && (v.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 0.000001));
        assert!(values[0][0] < 0. && values[0][1] > 0.);
        assert_eq!(values[0][2], 0.);
        assert!(values[7 * SIZE + 7][2] > 0.99);
    }
    #[test]
    fn evaluation_retains_native_axis_signs_and_unsigned_quantization() -> Result<()> {
        let mut c = [[0.; 3]; 9];
        assert!(texture(&c)?.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        c[0] = [512., 256., 128.];
        assert!(texture(&c)?
            .chunks_exact(4)
            .all(|p| p == [144, 72, 36, 255]));
        c = [[0.; 3]; 9];
        c[1] = [100.; 3];
        assert_eq!(
            evaluate([0., 1., 0.], &c),
            [0.48860251903533936_f32 * 100.; 3]
        );
        c = [[0.; 3]; 9];
        c[5] = [100.; 3];
        assert!(evaluate([0., 0.5, 0.5], &c)[0] < 0.);
        c[0] = [f32::NAN; 3];
        assert!(texture(&c).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "writes a visual diagnostic, not a captured original-game texture"]
    fn export_basis_visualization() -> Result<()> {
        let mut image = image::RgbaImage::new(48, 48);
        for term in 0..9 {
            let mut c = [[0.; 3]; 9];
            c[0] = [128. / 0.282094806432724_f32; 3];
            c[term][0] += 100.;
            c[term][2] -= 100.;
            let bytes = texture(&c)?;
            for y in 0..16 {
                for x in 0..16 {
                    let i = (y * 16 + x) * 4;
                    image.put_pixel(
                        (term % 3 * 16 + x) as u32,
                        (term / 3 * 16 + y) as u32,
                        image::Rgba(bytes[i..i + 4].try_into()?),
                    );
                }
            }
        }
        image::imageops::resize(&image, 384, 384, image::imageops::FilterType::Nearest)
            .save("target/sh-basis-diagnostic.png")?;
        Ok(())
    }
}

#[cfg(test)]
mod sdk_vector_tests {
    #[test]
    fn retained_double_estimate_matches_original_sdk_values() -> anyhow::Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/sdk_vector.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let vector = std::array::from_fn(|i| case["vector"][i].as_f64().unwrap() as f32);
            let length = f32::from_bits(case["length_bits"].as_u64().unwrap() as u32);
            let (actual, direction) = super::vector_length_and_direction(vector);
            if length <= 1e-6 {
                assert_eq!(actual, 0.);
                assert_eq!(direction, [0.; 3]);
            } else {
                assert_eq!(actual.to_bits(), length.to_bits());
                assert_eq!(
                    direction.map(f32::to_bits),
                    std::array::from_fn(|i| case["normalized_bits"][i].as_u64().unwrap() as u32)
                );
            }
        }
        Ok(())
    }
}
