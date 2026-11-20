//! Retail visibility predicates e0ddc/e0f6c. Query generation, sorting and
//! collidable metadata are mandatory backend operations, not guessed defaults.
use anyhow::{ensure, Result};
pub type V = [f32; 4];
pub const FILTER: u32 = 0x0001_8009;
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub body: usize,
    pub fraction: f32,
    pub keys: [u32; 8],
}
#[derive(Clone, Copy, Debug)]
pub struct CastHit {
    pub body: usize,
    pub fraction: f32,
    pub key: u32,
    pub point: V,
    pub normal: V,
}
pub trait Backend {
    /// Native collector-sorted results, preserving duplicates/equal ordering.
    fn ray(&mut self, from: V, to: V, filter: u32) -> Result<Vec<RayHit>>;
    fn sphere(&mut self, from: V, to: V, radius: f32, filter: u32) -> Result<Vec<CastHit>>;
    fn allow_look_through(&mut self, body: usize) -> Result<bool>;
    ///24317c, including container-key selection and live jigsaw materials.
    fn material(&mut self, body: usize, key: u32) -> Result<u32>;
}
/// None means clear. Some(fraction) means blocked and is the ONLY branch that
/// writes the original optional fraction destination. Zero-fraction hits skip
/// both property and material reads. The bypass skips material, never the
/// Allow Look Through property. A zero material word legitimately passes.
pub fn ray(
    from: V,
    to: V,
    bypass_material: bool,
    backend: &mut impl Backend,
) -> Result<Option<f32>> {
    ensure!(
        from.into_iter().chain(to).all(f32::is_finite),
        "Nonfinite camera visibility ray"
    );
    for hit in backend.ray(from, to, FILTER)? {
        if hit.fraction == 0. {
            continue;
        }
        if backend.allow_look_through(hit.body)? {
            continue;
        }
        if !bypass_material {
            let mut key = u32::MAX;
            for next in hit.keys {
                if next == u32::MAX {
                    break;
                }
                key = next;
            }
            if backend.material(hit.body, key)? & 0x18000 == 0 {
                continue;
            }
        }
        return Ok(Some(hit.fraction));
    }
    Ok(None)
}
fn squared(from: V, to: V) -> f32 {
    let d: [f32; 3] = std::array::from_fn(|i| from[i] - to[i]);
    d[2].mul_add(d[2], d[0] * d[0]) + d[1] * d[1]
}
/// The native cast starts at target and moves toward camera. Besides fraction,
/// it checks actual contact-point XYZ distance against the full segment; a
/// sphere contact beyond the camera endpoint is ignored. W is not distance.
/// radius is the controller's +580 coefficient times the supplied scale.
pub fn sphere(
    camera: V,
    target: V,
    radius: f32,
    scale: f32,
    bypass_material: bool,
    backend: &mut impl Backend,
) -> Result<bool> {
    let radius = radius * scale;
    ensure!(
        camera
            .into_iter()
            .chain(target)
            .chain([radius])
            .all(f32::is_finite)
            && radius >= 0.,
        "Unsupported camera visibility sphere input"
    );
    for hit in backend.sphere(target, camera, radius, FILTER)? {
        if hit.fraction == 0. {
            continue;
        }
        if squared(target, hit.point) > squared(target, camera) {
            continue;
        }
        if backend.allow_look_through(hit.body)? {
            continue;
        }
        if !bypass_material && backend.material(hit.body, hit.key)? & 0x18000 == 0 {
            continue;
        }
        return Ok(false);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Host {
        ray: Vec<RayHit>,
        cast: Vec<CastHit>,
        through: Vec<usize>,
        word: u32,
        calls: Vec<(usize, u32)>,
        seen: Vec<usize>,
    }
    impl Backend for Host {
        fn ray(&mut self, _: V, _: V, filter: u32) -> Result<Vec<RayHit>> {
            assert_eq!(filter, FILTER);
            Ok(self.ray.clone())
        }
        fn sphere(&mut self, from: V, to: V, radius: f32, filter: u32) -> Result<Vec<CastHit>> {
            assert_eq!(from, [0., 0., 0., 71.]);
            assert_eq!(to, [2., 0., 0., -31.]);
            assert_eq!(radius, 0.25);
            assert_eq!(filter, FILTER);
            Ok(self.cast.clone())
        }
        fn allow_look_through(&mut self, body: usize) -> Result<bool> {
            self.seen.push(body);
            Ok(self.through.contains(&body))
        }
        fn material(&mut self, body: usize, key: u32) -> Result<u32> {
            self.calls.push((body, key));
            Ok(self.word)
        }
    }
    #[test]
    fn visibility_loops_match_original_instruction_cases() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_visibility.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut host = Host {
                ray: Vec::new(),
                cast: Vec::new(),
                through: serde_json::from_value(case["through"].clone())?,
                word: case["word"].as_u64().unwrap() as u32,
                calls: Vec::new(),
                seen: Vec::new(),
            };
            let bypass = case["bypass"].as_bool().unwrap();
            let result = if case["kind"] == "ray" {
                for hit in case["hits"].as_array().unwrap() {
                    host.ray.push(RayHit {
                        body: hit["body"].as_u64().unwrap() as usize,
                        fraction: hit["fraction"].as_f64().unwrap() as f32,
                        keys: serde_json::from_value(hit["keys"].clone())?,
                    });
                }
                let fraction = ray([0.; 4], [2., 0., 0., 0.], bypass, &mut host)?;
                assert_eq!(
                    fraction.map(f32::to_bits),
                    case["blocking_fraction_bits"].as_u64().map(|v| v as u32),
                    "{case}"
                );
                fraction.is_none()
            } else {
                for hit in case["hits"].as_array().unwrap() {
                    host.cast.push(CastHit {
                        body: hit["body"].as_u64().unwrap() as usize,
                        fraction: hit["fraction"].as_f64().unwrap() as f32,
                        key: hit["key"].as_u64().unwrap() as u32,
                        point: serde_json::from_value(hit["point"].clone())?,
                        normal: [0.; 4],
                    });
                }
                sphere(
                    [2., 0., 0., -31.],
                    [0., 0., 0., 71.],
                    0.5,
                    0.5,
                    bypass,
                    &mut host,
                )?
            };
            assert_eq!(result, case["clear"].as_bool().unwrap(), "{case}");
            assert_eq!(
                host.seen,
                serde_json::from_value::<Vec<usize>>(case["lookups"].clone())?,
                "{case}"
            );
            assert_eq!(
                host.calls,
                serde_json::from_value::<Vec<(usize, u32)>>(case["materials"].clone())?,
                "{case}"
            );
        }
        Ok(())
    }
}
