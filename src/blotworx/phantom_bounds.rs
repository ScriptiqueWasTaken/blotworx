//! EM1 size-based phantom bounds, REL37788c..3779fc.
//! This computes the retail AABB; it does not implement overlap dispatch.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct SizeVolume {
    pub size: [f32; 3],
}
impl SizeVolume {
    pub fn from_component(component: &Component) -> Result<Option<Self>> {
        ensure!(
            component.class_name == "PhysicsPhantomComponent",
            "Not a physics phantom"
        );
        // REL377df0..377e58 requires both valid resource strings before trying
        // the resource branch. A lone path takes the size fallback directly.
        let mut paths = [""; 2];
        for (index, name) in ["Phantom NIF File", "Phantom HKX File"]
            .into_iter()
            .enumerate()
        {
            let mut properties = component.properties.iter().filter(|p| p.name == name);
            if let Some(property) = properties.next() {
                ensure!(properties.next().is_none(), "Duplicate {name}");
                paths[index] = property
                    .value
                    .as_str()
                    .with_context(|| format!("Invalid {name}"))?;
                ensure!(
                    !paths[index].contains('\0'),
                    "Embedded NUL in {name} requires native string import semantics"
                );
            }
        }
        // Do not trim authored strings: whitespace isn't an empty interned name.
        ensure!(
            paths.iter().any(|p| p.is_empty()),
            "Resource phantom NIF={} HKX={} requires its native shape/loading path",
            paths[0],
            paths[1]
        );
        let volume = Self::from_size_property(component)?;
        // REL3781dc..378238 checks a native sentinel whose value is unverified.
        ensure!(volume.is_some(), "Zero-size phantom sentinel is unverified; fallback activation requires native initialization evidence");
        Ok(volume)
    }
    /// Box-shaped rigid body sensors expose the same size field, but are
    /// queried by trigger overlap without registering an hkpAabbPhantom.
    pub(super) fn from_size_property(component: &Component) -> Result<Option<Self>> {
        let value = &component
            .properties
            .iter()
            .find(|p| p.name == "Phantom Size")
            .context("Phantom size missing; constructor default has not been verified")?
            .value;
        let mut size = [0.; 3];
        for (i, key) in ["x", "y", "z"].into_iter().enumerate() {
            size[i] = value
                .get(key)
                .and_then(|v| v.as_f64())
                .with_context(|| format!("Invalid phantom size {key}"))?
                as f32;
        }
        ensure!(
            size.iter().all(|v| v.is_finite()),
            "Invalid phantom dimensions"
        );
        // A zero box has no volume for the rigid-body preview query.
        Ok((size != [0.; 3]).then_some(Self { size }))
    }
    pub fn bounds(&self, position: [f32; 3], rotation: [f32; 9]) -> Result<Bounds> {
        ensure!(
            position
                .iter()
                .chain(&rotation)
                .chain(&self.size)
                .all(|v| v.is_finite()),
            "Invalid phantom owner pose"
        );
        let mut half = [0.; 3];
        for row in 0..3 {
            let mut positive = 0_f32;
            let mut negative = 0_f32;
            for col in 0..3 {
                let term = rotation[row * 3 + col] * self.size[col];
                if term > 0. {
                    positive += term;
                } else {
                    negative += term;
                }
            }
            // Retail uses max(positive,-negative), NOT sum(abs(terms)).
            // Keep separate f32 mul/add operations, matching fmuls/fadds.
            half[row] = if positive > -negative {
                positive
            } else {
                -negative
            };
            half[row] *= 0.5; // REL4fa094 is exactly 0.5.
        }
        let bounds = Bounds {
            min: std::array::from_fn(|i| position[i] - half[i]),
            max: std::array::from_fn(|i| position[i] + half[i]),
        };
        ensure!(
            bounds.min.iter().chain(&bounds.max).all(|v| v.is_finite()),
            "Phantom bounds overflow"
        );
        Ok(bounds)
    }
    /// REL37788c initializes all four half-extent lanes to zero;23b970 sets
    /// position W to zero. Both padded result lanes are therefore zero.
    pub fn padded_bounds(
        &self,
        position: [f32; 3],
        rotation: [f32; 9],
    ) -> Result<super::phantom_update::Aabb> {
        let bounds = self.bounds(position, rotation)?;
        Ok([
            bounds.min[0].to_bits(),
            bounds.min[1].to_bits(),
            bounds.min[2].to_bits(),
            0,
            bounds.max[0].to_bits(),
            bounds.max[1].to_bits(),
            bounds.max[2].to_bits(),
            0,
        ])
    }
}
const TRIGGER_PLAYER_RADIUS: f32 = 0.3;
const TRIGGER_PLAYER_HEIGHT: f32 = 1.1;

unsafe extern "C" {
    fn bw_trigger_capsule_overlaps_convex(
        vertices: *const [f32; 3],
        count: i32,
        hull_radius: f32,
        start: *const f32,
        end: *const f32,
        capsule_radius: f32,
    ) -> i32;
}

/// Placement of a trigger volume defined by rigid bodies in a cooked Havok file.
pub(super) struct RigidTriggerHull {
    parts: std::sync::Arc<Vec<super::collision::StaticMesh>>,
    placement: [f32; 16],
    bounds: Vec<Bounds>,
}
impl RigidTriggerHull {
    pub(super) fn new(
        parts: std::sync::Arc<Vec<super::collision::StaticMesh>>,
        placement: [f32; 16],
    ) -> Result<Self> {
        ensure!(parts.iter().all(|part| part.convex), "Non-convex trigger hull is unsupported");
        let bounds = Self::placed_bounds(&parts, &placement)?;
        Ok(Self { parts, placement, bounds })
    }

    fn placed_bounds(
        parts: &[super::collision::StaticMesh],
        placement: &[f32; 16],
    ) -> Result<Vec<Bounds>> {
        let mut bodies = std::collections::BTreeSet::new();
        let mut bounds = Vec::new();
        for part in parts.iter().filter(|part| bodies.insert(part.body_offset)) {
            let pose = super::native_hk_transform::compose(placement, &part.body_transform);
            let [min, max] = (*part.bounds_graph).clone().aabb(part.bounds_root, &pose, 0.)?;
            bounds.push(Bounds { min: min[..3].try_into()?, max: max[..3].try_into()? });
        }
        ensure!(!bounds.is_empty(), "Trigger hull has no rigid bodies");
        Ok(bounds)
    }

    #[cfg(debug_assertions)]
    pub(super) fn interior_player_position(&self) -> [f32; 3] {
        let part = &self.parts[0];
        let pose = super::native_hk_transform::compose(&self.placement, &part.body_transform);
        let local = std::array::from_fn(|axis| {
            part.vertices.iter().map(|point| point[axis]).sum::<f32>() / part.vertices.len() as f32
        });
        let mut center = super::physics::point(&pose, local);
        center[1] -= TRIGGER_PLAYER_HEIGHT * 0.5;
        center
    }

    pub(super) fn overlaps_player(
        &self,
        placement: [f32; 16],
        position: [f32; 3],
    ) -> Result<bool> {
        let moved;
        let bounds = if placement == self.placement {
            &self.bounds
        } else {
            moved = Self::placed_bounds(&self.parts, &placement)?;
            &moved
        };
        if !bounds.iter().any(|bounds| bounds.overlaps_player(position)) {
            return Ok(false);
        }
        for part in self.parts.iter() {
            let pose = super::native_hk_transform::compose(&placement, &part.body_transform);
            let local_endpoint = |height: f32| {
                let delta = [
                    position[0] - pose[12],
                    position[1] + height - pose[13],
                    position[2] - pose[14],
                ];
                std::array::from_fn::<_, 3, _>(|axis| {
                    let start = axis * 4;
                    (0..3).map(|i| pose[start + i] * delta[i]).sum::<f32>()
                })
            };
            let lower = local_endpoint(TRIGGER_PLAYER_RADIUS);
            let upper = local_endpoint(TRIGGER_PLAYER_HEIGHT - TRIGGER_PLAYER_RADIUS);
            let result = unsafe {
                bw_trigger_capsule_overlaps_convex(
                    part.vertices.as_ptr(), part.vertices.len().try_into()?,
                    part.triangle_radius, lower.as_ptr(), upper.as_ptr(), TRIGGER_PLAYER_RADIUS,
                )
            };
            ensure!(result >= 0, "Trigger hull exceeds the collision query vertex limit");
            if result != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl Bounds {
    pub(super) fn overlaps_player(&self, position: [f32; 3]) -> bool {
        (0..3).all(|i| {
            position[i] + if i == 1 { TRIGGER_PLAYER_HEIGHT } else { TRIGGER_PLAYER_RADIUS } >= self.min[i]
                && position[i] - if i == 1 { 0. } else { TRIGGER_PLAYER_RADIUS } <= self.max[i]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn size_fallback_accepts_single_resource_path_without_trimming_names() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{Property, ID};
        let property = |name: &str, value| {
            Property::new(String::new(), name.into(), false, false, false, value)
        };
        let component = |nif: &str, hkx: &str| {
            Component::new(
                "PhysicsPhantomComponent".into(),
                String::new(),
                ID::new(0),
                ID::new(0),
                ID::new(0),
                vec![
                    property("Phantom NIF File", serde_json::json!(nif)),
                    property("Phantom HKX File", serde_json::json!(hkx)),
                    property("Phantom Size", serde_json::json!({"x":2.,"y":4.,"z":6.})),
                ],
            )
        };
        for (nif, hkx) in [
            ("", ""),
            ("models/trigger.nif", ""),
            ("", "physics/trigger.hkx"),
            (" ", ""),
        ] {
            assert_eq!(
                SizeVolume::from_component(&component(nif, hkx))?
                    .unwrap()
                    .size,
                [2., 4., 6.]
            );
        }
        assert!(SizeVolume::from_component(&component(
            "models/trigger.nif",
            "physics/trigger.hkx"
        ))
        .is_err());
        assert!(SizeVolume::from_component(&component(" ", "physics/trigger.hkx")).is_err());
        assert!(SizeVolume::from_component(&component("models/trigger.nif", " ")).is_err());
        assert!(SizeVolume::from_component(&component("\0", "")).is_err());
        let mut duplicated = component("", "");
        duplicated
            .properties
            .push(property("Phantom NIF File", serde_json::json!("")));
        assert!(SizeVolume::from_component(&duplicated).is_err());
        Ok(())
    }
    #[test]
    fn native_size_filter_preserves_all_layer_bits() {
        assert_eq!(SIZE_PHANTOM_MEMORY, 224);
        assert_eq!(size_filter_info(12), 0x1800c);
        assert_eq!(size_filter_info(0x602003), 0x61a003);
        assert_eq!(size_filter_info(-1), u32::MAX);
    }
    #[test]
    fn retail_phantom_bounds_preserve_sign_partition_instead_of_absolute_sum() -> Result<()> {
        let volume = SizeVolume { size: [2., 4., 6.] };
        let identity = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
        assert_eq!(
            volume.bounds([10., 20., 30.], identity)?,
            Bounds {
                min: [9., 18., 27.],
                max: [11., 22., 33.]
            }
        );
        let rotated = [0.6, -0.8, 0., 0.8, 0.6, 0., 0., 0., 1.];
        let bounds = volume.bounds([0.; 3], rotated)?;
        assert_eq!(bounds.max[0], 1.6); // max(1.2,3.2)*.5, not 2.2.
        assert_eq!(bounds.max[1], 2.);
        assert_eq!(bounds.max[2], 3.);
        assert!(volume.bounds([f32::NAN, 0., 0.], identity).is_err());
        Ok(())
    }
}
/// REL37839c..3783c0, size-based hkpAabbPhantom creation.
pub const SIZE_PHANTOM_MEMORY: u16 = 0xe0;
pub fn size_filter_info(layer: i32) -> u32 {
    (layer as u32) | 0x18000
}
pub fn authored_size_filter(
    component: &epic_mickey_lib_rs::scene_file::Component,
) -> anyhow::Result<u32> {
    use anyhow::{ensure, Context};
    ensure!(
        component.class_name == "PhysicsPhantomComponent",
        "Expected PhysicsPhantomComponent"
    );
    let mut properties = component
        .properties
        .iter()
        .filter(|p| p.name == "Collision Layer");
    let property = properties
        .next()
        .context("Collision Layer missing; native default unverified")?;
    ensure!(properties.next().is_none(), "Duplicate Collision Layer");
    let layer = property
        .value
        .as_i64()
        .context("Collision Layer must be integer")?;
    Ok(size_filter_info(
        i32::try_from(layer).context("Collision Layer outside signed32 range")?,
    ))
}
