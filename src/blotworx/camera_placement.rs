//! Native placement contact selection e189c..e1bbc and side test e1be4.
//! This feeds the remaining polar placement solver; it is not an orbit camera.
use super::camera_collision::{Backend, CastHit, FILTER, V};
use anyhow::{ensure, Result};
#[derive(Clone, Copy, Debug)]
pub struct Overlap {
    pub body: usize,
    pub key: u32,
}
pub trait PlacementBackend: Backend {
    fn thinned_collides(&mut self) -> Result<bool>; //live property2b0
    /// Actual current-profile tracker -> tracked controller -> transform.
    /// None is a verified absent tracker/controller, not an unavailable binding.
    fn tracked_actor_position(&mut self) -> Result<Option<V>>;
    fn ignore_height(&mut self) -> Result<f32>; //live property788
    ///23e5c8 overlap results, unsorted; retain per-child material identities.
    fn overlap(&mut self, center: V, radius: f32, filter: u32) -> Result<Vec<Overlap>>;
}
fn blocks(hit: Overlap, backend: &mut impl PlacementBackend) -> Result<bool> {
    // Preserve lazy config reads: painted/real materials do not query2b0.
    Ok(backend.material(hit.body, hit.key)? & 0x18000 != 0 || backend.thinned_collides()?)
}
/// Return the complete first accepted contact. Native no-contact returns -1
/// and leaves both caller vectors untouched; Option preserves that distinction.
/// The initial cast uses radius*scale, while the secondary overlap uses the
/// UNSCALED radius and adds it after actorY+IgnoreHeight.
pub fn contact(
    from: V,
    to: V,
    radius: f32,
    scale: f32,
    backend: &mut impl PlacementBackend,
) -> Result<Option<CastHit>> {
    let cast_radius = radius * scale;
    ensure!(
        from.into_iter()
            .chain(to)
            .chain([radius, cast_radius])
            .all(f32::is_finite)
            && radius >= 0.
            && cast_radius >= 0.,
        "Unsupported native camera placement cast"
    );
    for hit in backend.sphere(from, to, cast_radius, FILTER)? {
        if hit.fraction == 0. {
            continue;
        }
        if backend.allow_look_through(hit.body)? {
            continue;
        }
        if !blocks(
            Overlap {
                body: hit.body,
                key: hit.key,
            },
            backend,
        )? {
            continue;
        }
        if let Some(actor) = backend.tracked_actor_position()? {
            let height = hit.point[1] - actor[1];
            if height > 0. && height < backend.ignore_height()? {
                // Native reads IgnoreHeight again after collector construction.
                let mut center = hit.point;
                center[1] = (actor[1] + backend.ignore_height()?) + radius;
                let mut blocked = false;
                for overlapping in backend.overlap(center, radius, FILTER)? {
                    // No fraction/Allow Look Through test and no early break
                    // in this secondary loop. Later material failures matter.
                    if blocks(overlapping, backend)? {
                        blocked = true;
                    }
                }
                if !blocked {
                    continue;
                }
            }
        }
        return Ok(Some(hit));
    }
    Ok(None)
}
/// e1be4: positive Y component of the XZ cross product. No normalization,
/// epsilon, plane fitting or W contribution; preserve the native fused order.
pub fn positive_side(from: V, to: V, point: V) -> bool {
    let ux = to[0] - from[0];
    let uz = to[2] - from[2];
    let vx = point[0] - from[0];
    let vz = point[2] - from[2];
    -ux.mul_add(vz, -(vx * uz)) > 0.
}

/// Persistent fields used by e1c8c. Construction/acquisition of the full
/// derived controller remains the caller's responsibility.
#[derive(Clone, Copy, Debug)]
pub struct YawState {
    pub previous_polar: V,     //360
    pub target: V,             //2d0
    pub last_target: V,        //2e0
    pub acquired_inside: bool, //4d8
}
/// Trig leaves are replaceable only for bounded instruction-reference replay.
/// Production uses the existing ports of the original DOL math routines.
trait Math {
    fn cos(&mut self, value: f64) -> Result<f32>;
    fn sin(&mut self, value: f64) -> Result<f32>;
    fn atan(&mut self, value: f32) -> Result<f32>;
}
struct NativeMath;
impl Math for NativeMath {
    fn cos(&mut self, value: f64) -> Result<f32> {
        Ok(super::native_angle::sin_cos_medium(value)?.1 as f32)
    }
    fn sin(&mut self, value: f64) -> Result<f32> {
        Ok(super::native_angle::sin_cos_medium(value)?.0 as f32)
    }
    fn atan(&mut self, value: f32) -> Result<f32> {
        Ok(super::native_angle::atan(value as f64) as f32)
    }
}
fn polar(value: V, math: &mut impl Math) -> Result<V> {
    let phi = f32::from_bits(0x3fc90fdb) - value[2];
    let sp = math.sin(phi as f64)?;
    let sy = math.sin(value[1] as f64)?;
    let x = sy * (value[0] * sp);
    let y = value[0] * math.cos(phi as f64)?;
    let sp = math.sin(phi as f64)?;
    let cy = math.cos(value[1] as f64)?;
    Ok([x, y, cy * (value[0] * sp), 0.])
}
fn normalize(mut v: V) -> Result<V> {
    let squared = v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    ensure!(
        squared.is_finite(),
        "Native camera placement length overflow"
    );
    let length = if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    };
    if length > f32::from_bits(0x38d1b717) {
        let scale = 1. / length;
        v = v.map(|x| x * scale);
    }
    Ok(v)
}
fn plus(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn minus(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
/// e1c8c..e24a8: native motion gate, acquisition ray, clearance rays and yaw
/// search. Changes only requested yaw and the acquired-inside flag. No guessed
/// iteration cap or substitute orbit distance is introduced.
pub fn adjust_yaw(
    state: &mut YawState,
    requested: &mut V,
    unfiltered_target: V,
    radius: f32,
    backend: &mut impl PlacementBackend,
) -> Result<bool> {
    adjust_yaw_with(
        state,
        requested,
        unfiltered_target,
        radius,
        backend,
        &mut NativeMath,
    )
}
fn adjust_yaw_with(
    state: &mut YawState,
    requested: &mut V,
    unfiltered_target: V,
    radius: f32,
    backend: &mut impl PlacementBackend,
    math: &mut impl Math,
) -> Result<bool> {
    ensure!(
        state
            .previous_polar
            .into_iter()
            .chain(state.target)
            .chain(state.last_target)
            .chain(*requested)
            .chain(unfiltered_target)
            .chain([radius])
            .all(f32::is_finite)
            && radius >= 0.,
        "Unsupported native camera yaw placement input"
    );
    let previous_direction = normalize(polar(state.previous_polar, math)?)?;
    let movement = normalize(minus(state.target, state.last_target))?;
    let dot = movement[2].mul_add(previous_direction[2], movement[0] * previous_direction[0])
        + movement[1] * previous_direction[1];
    if dot > math.cos(0.5)? {
        return Ok(false);
    }
    if backend.thinned_collides()? && !state.acquired_inside {
        let mut to = unfiltered_target;
        to[0] += radius;
        // Raw closest query, before visibility/property/material rejection.
        if backend
            .ray(unfiltered_target, to, FILTER)?
            .first()
            .is_some_and(|h| h.fraction == 0.)
        {
            state.acquired_inside = true;
        }
    }
    let mut initial = *requested;
    initial[0] = state.previous_polar[0];
    let endpoint = plus(polar(initial, math)?, state.target);
    let Some(hit) = contact(unfiltered_target, endpoint, radius, 1., backend)? else {
        return Ok(false);
    };
    let fraction = hit.fraction;
    let side = positive_side(unfiltered_target, endpoint, hit.point);
    if !(fraction > 0. && fraction < 0.75) {
        return Ok(false);
    }
    let half_pi = f32::from_bits(0x3fc90fdb);
    let margin = 0.1 + radius;
    let offset = polar(
        [
            margin,
            f32::from_bits(0x40490fdb) + requested[1],
            half_pi - requested[2],
            0.,
        ],
        math,
    )?;
    if let Some(other) = super::camera_collision::ray(
        minus(unfiltered_target, offset),
        minus(endpoint, offset),
        false,
        backend,
    )? {
        if (fraction - other).abs() < 0.2 {
            return Ok(false);
        }
    }
    let distance = fraction * initial[0];
    let offset = polar([margin, 0., half_pi, 0.], math)?;
    let start = plus(offset, unfiltered_target);
    let mut shorter = *requested;
    shorter[0] = 2.0f32.mul_add(radius, distance);
    let end = plus(start, polar(shorter, math)?);
    if super::camera_collision::ray(start, end, false, backend)?.is_some() {
        return Ok(false);
    }
    let bound = math.atan(0.25 / fraction)?;
    let step = (0.01 * bound) * if side { -1. } else { 1. };
    let mut candidate = *requested;
    loop {
        let next = candidate[1] + step;
        ensure!(
            next.is_finite() && next != candidate[1],
            "Native camera yaw search cannot represent its next step"
        );
        candidate[1] = next;
        let end = plus(polar(candidate, math)?, state.target);
        let next = contact(unfiltered_target, end, radius, 1., backend)?;
        let next_fraction = next.map(|h| h.fraction).unwrap_or(-1.);
        // No-contact retains the previous native output vectors; its side
        // result is irrelevant because the negative-fraction branch accepts.
        let next_side = next
            .map(|h| positive_side(unfiltered_target, end, h.point))
            .unwrap_or(side);
        if next_fraction < 0.
            || next_fraction > 0.75
            || (next_side != side && next_fraction > fraction)
        {
            requested[1] = candidate[1];
            return Ok(true);
        }
        let delta = super::camera_wrapper::angle_difference(requested[1], candidate[1])?;
        if delta.abs() > bound {
            requested[1] = candidate[1];
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::camera_collision::RayHit;
    use super::*;
    struct Host<'a> {
        case: &'a serde_json::Value,
        events: Vec<String>,
        queries: Vec<serde_json::Value>,
    }
    impl Backend for Host<'_> {
        fn ray(&mut self, _: V, _: V, _: u32) -> Result<Vec<RayHit>> {
            anyhow::bail!("Placement contact must not issue a ray")
        }
        fn sphere(&mut self, _: V, _: V, radius: f32, filter: u32) -> Result<Vec<CastHit>> {
            assert_eq!(radius, 0.125);
            assert_eq!(filter, FILTER);
            self.case["hits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| {
                    Ok(CastHit {
                        body: h["body"].as_u64().unwrap() as usize,
                        key: h["key"].as_u64().unwrap() as u32,
                        fraction: h["fraction"].as_f64().unwrap() as f32,
                        point: serde_json::from_value(h["point"].clone())?,
                        normal: serde_json::from_value(h["normal"].clone())?,
                    })
                })
                .collect()
        }
        fn allow_look_through(&mut self, body: usize) -> Result<bool> {
            self.events.push(format!("look:{body}"));
            Ok(body == 21 || body == 40)
        }
        fn material(&mut self, body: usize, key: u32) -> Result<u32> {
            self.events.push(format!("material:{body}:{key}"));
            Ok(match body {
                20 => self.case["word"].as_u64().unwrap() as u32,
                21 | 40 => 0x8002,
                22 | 41 => 0,
                _ => panic!("Unknown fixture body"),
            })
        }
    }
    impl PlacementBackend for Host<'_> {
        fn thinned_collides(&mut self) -> Result<bool> {
            self.events.push("config".into());
            Ok(self.case["thinned"].as_bool().unwrap())
        }
        fn ignore_height(&mut self) -> Result<f32> {
            self.events.push("height".into());
            Ok(1.1)
        }
        fn tracked_actor_position(&mut self) -> Result<Option<V>> {
            self.events.push("tracked".into());
            Ok((self.case["actor"] == "present").then_some([0.; 4]))
        }
        fn overlap(&mut self, center: V, radius: f32, filter: u32) -> Result<Vec<Overlap>> {
            self.events.push("overlap".into());
            self.queries.push(serde_json::json!({"center_bits":center.map(f32::to_bits),"radius_bits":radius.to_bits(),"filter":filter}));
            Ok(self.case["overlap"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| Overlap {
                    body: h["body"].as_u64().unwrap() as usize,
                    key: h["key"].as_u64().unwrap() as u32,
                })
                .collect())
        }
    }
    #[test]
    fn contact_selection_and_nested_query_order_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_placement.json"
        ))?;
        assert_eq!(fixture["cases"].as_array().unwrap().len(), 180);
        for case in fixture["cases"].as_array().unwrap() {
            let mut host = Host {
                case,
                events: Vec::new(),
                queries: Vec::new(),
            };
            let hit = contact([0.; 4], [2., 0., 0., 0.], 0.25, 0.5, &mut host)?;
            assert_eq!(hit.is_some(), case["accepted"].as_bool().unwrap(), "{case}");
            assert_eq!(
                hit.map(|h| h.fraction).unwrap_or(-1.).to_bits(),
                case["fraction_bits"].as_u64().unwrap() as u32,
                "{case}"
            );
            if let Some(hit) = hit {
                assert_eq!(
                    hit.point.map(f32::to_bits),
                    serde_json::from_value::<[u32; 4]>(case["point_bits"].clone())?,
                    "{case}"
                );
                assert_eq!(
                    hit.normal.map(f32::to_bits),
                    serde_json::from_value::<[u32; 4]>(case["normal_bits"].clone())?,
                    "{case}"
                );
            }
            assert_eq!(
                host.events,
                serde_json::from_value::<Vec<String>>(case["events"].clone())?,
                "{case}"
            );
            assert_eq!(
                host.queries,
                case["queries"].as_array().unwrap().clone(),
                "{case}"
            );
        }
        Ok(())
    }
    #[test]
    fn collision_side_predicate_matches_original_paired_arithmetic() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_placement.json"
        ))?;
        for case in fixture["sides"].as_array().unwrap() {
            assert_eq!(
                positive_side(
                    serde_json::from_value(case["start"].clone())?,
                    serde_json::from_value(case["end"].clone())?,
                    serde_json::from_value(case["point"].clone())?
                ),
                case["positive"].as_bool().unwrap(),
                "{case}"
            );
        }
        Ok(())
    }
    struct ReplayMath<'a> {
        calls: &'a [serde_json::Value],
        next: usize,
    }
    impl ReplayMath<'_> {
        fn sample(&mut self, kind: &str, value: f64) -> Result<f32> {
            let call = &self.calls[self.next];
            self.next += 1;
            assert_eq!(call["kind"], kind, "Math call {}", self.next);
            assert_eq!(
                call["input_bits"].as_u64().unwrap(),
                value.to_bits(),
                "Math argument {}",
                self.next
            );
            Ok(f32::from_bits(call["output_bits"].as_u64().unwrap() as u32))
        }
    }
    impl Math for ReplayMath<'_> {
        fn sin(&mut self, v: f64) -> Result<f32> {
            self.sample("sin", v)
        }
        fn cos(&mut self, v: f64) -> Result<f32> {
            self.sample("cos", v)
        }
        fn atan(&mut self, v: f32) -> Result<f32> {
            self.sample("atan", v as f64)
        }
    }
    struct YawHost<'a> {
        case: &'a serde_json::Value,
        events: Vec<serde_json::Value>,
        casts: usize,
        rays: usize,
        raw: bool,
    }
    impl YawHost<'_> {
        fn scenario(&self) -> &str {
            self.case["scenario"].as_str().unwrap()
        }
        fn xyz(v: V) -> [u32; 3] {
            std::array::from_fn(|i| v[i].to_bits())
        }
    }
    impl Backend for YawHost<'_> {
        fn ray(&mut self, from: V, to: V, filter: u32) -> Result<Vec<RayHit>> {
            assert_eq!(filter, FILTER);
            let fraction = if self.raw {
                self.raw = false;
                self.events.push(serde_json::json!({"kind":"closest","start":Self::xyz(from),"end":Self::xyz(to)}));
                match self.scenario() {
                    "inside" => Some(0.),
                    "closest_nonzero" => Some(0.5),
                    _ => None,
                }
            } else {
                self.rays += 1;
                self.events.push(serde_json::json!({"kind":"visibility","start":Self::xyz(from),"end":Self::xyz(to),"bypass":0}));
                if self.rays == 1 && matches!(self.scenario(), "near_ray" | "edge_ray") {
                    Some(if self.scenario() == "near_ray" {
                        0.4
                    } else {
                        0.1
                    })
                } else if self.rays == 2 && self.scenario() == "second_blocked" {
                    Some(0.4)
                } else {
                    None
                }
            };
            Ok(fraction
                .map(|fraction| {
                    vec![RayHit {
                        body: 20,
                        fraction,
                        keys: [u32::MAX; 8],
                    }]
                })
                .unwrap_or_default())
        }
        fn sphere(&mut self, from: V, to: V, radius: f32, filter: u32) -> Result<Vec<CastHit>> {
            assert_eq!(radius, 0.25);
            assert_eq!(filter, FILTER);
            self.casts += 1;
            self.events.push(serde_json::json!({"kind":"contact","start":Self::xyz(from),"end":Self::xyz(to),"scale_bits":1.0f32.to_bits()}));
            let scenario = self.scenario();
            let fraction = if scenario == "no_contact" {
                -1.
            } else if scenario == "boundary" {
                0.75
            } else if self.casts > 1 && scenario == "above_threshold" {
                0.76
            } else if self.casts > 1 && scenario == "side_better" {
                0.6
            } else if self.casts > 1 && scenario == "side_worse" {
                0.3
            } else if self.casts == 1 || scenario == "exhaust" {
                0.4
            } else {
                -1.
            };
            let mut point: V = serde_json::from_value(self.case["point"].clone())?;
            if self.casts > 1 && matches!(scenario, "side_better" | "side_worse") {
                point[0] = -point[0] + 2.;
                point[2] = -point[2] + 6.;
            }
            Ok(if fraction < 0. {
                vec![]
            } else {
                vec![CastHit {
                    body: 20,
                    key: 37,
                    fraction,
                    point,
                    normal: [0., 1., 0., fraction],
                }]
            })
        }
        fn allow_look_through(&mut self, _: usize) -> Result<bool> {
            Ok(false)
        }
        fn material(&mut self, _: usize, _: u32) -> Result<u32> {
            Ok(0x8002)
        }
    }
    impl PlacementBackend for YawHost<'_> {
        fn thinned_collides(&mut self) -> Result<bool> {
            Ok(self.case["thinned"].as_bool().unwrap())
        }
        fn tracked_actor_position(&mut self) -> Result<Option<V>> {
            Ok(None)
        }
        fn ignore_height(&mut self) -> Result<f32> {
            anyhow::bail!("No tracker in yaw fixture")
        }
        fn overlap(&mut self, _: V, _: f32, _: u32) -> Result<Vec<Overlap>> {
            anyhow::bail!("No tracker in yaw fixture")
        }
    }
    #[test]
    fn yaw_search_queries_branches_and_mutations_match_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_yaw.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut state = YawState {
                previous_polar: serde_json::from_value(case["previous"].clone())?,
                target: serde_json::from_value(case["target"].clone())?,
                last_target: serde_json::from_value(case["last"].clone())?,
                acquired_inside: case["inside"].as_bool().unwrap(),
            };
            let mut requested: V = serde_json::from_value(case["requested"].clone())?;
            let mut host = YawHost {
                case,
                events: Vec::new(),
                casts: 0,
                rays: 0,
                raw: case["thinned"] == true && case["inside"] == false,
            };
            let mut math = ReplayMath {
                calls: case["math_calls"].as_array().unwrap(),
                next: 0,
            };
            let result = adjust_yaw_with(
                &mut state,
                &mut requested,
                [1., 2., 3., 0.],
                0.25,
                &mut host,
                &mut math,
            )?;
            assert_eq!(result, case["result"].as_bool().unwrap(), "Case {index}");
            assert_eq!(
                state.acquired_inside,
                case["inside_after"].as_bool().unwrap(),
                "Case {index}"
            );
            assert_eq!(
                requested.map(f32::to_bits),
                serde_json::from_value::<[u32; 4]>(case["requested_bits"].clone())?,
                "Case {index}"
            );
            assert_eq!(
                host.events,
                *case["events"].as_array().unwrap(),
                "Case {index}"
            );
            assert_eq!(math.next, math.calls.len(), "Case {index}");
        }
        Ok(())
    }
    #[test]
    fn original_math_ports_match_supplied_yaw_reference_leaves() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_yaw.json"
        ))?;
        let mut math = NativeMath;
        for case in fixture["cases"].as_array().unwrap() {
            for call in case["math_calls"].as_array().unwrap() {
                let value = f64::from_bits(call["input_bits"].as_u64().unwrap());
                let actual = match call["kind"].as_str().unwrap() {
                    "sin" => math.sin(value)?,
                    "cos" => math.cos(value)?,
                    "atan" => math.atan(value as f32)?,
                    _ => unreachable!(),
                };
                assert_eq!(
                    actual.to_bits(),
                    call["output_bits"].as_u64().unwrap() as u32,
                    "{call}"
                );
            }
        }
        Ok(())
    }
}
