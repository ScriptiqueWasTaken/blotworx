//! Retail camera distance solver dceb8..ddc0c. Caller supplies actual initialized
//! controller scalar storage; this is not a fabricated controller constructor.
use super::camera_collision::{FILTER, V};
use super::camera_placement::PlacementBackend;
use anyhow::{ensure, Result};

/// Properties and source-body identities remain owned by their live runtime
/// objects. Native pointer slots are never serialized into this Rust storage.
pub trait Backend: PlacementBackend {
    fn boolean(&mut self, offset: usize) -> Result<bool>;
    fn scalar(&mut self, offset: usize) -> Result<f32>;
    fn look_through_entity(&mut self, body: usize) -> Result<usize>;
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookThrough {
    pub fraction: f32,
    pub entity: usize,
}
fn f(s: &[u8], off: usize) -> f32 {
    f32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn w(s: &[u8], off: usize) -> u32 {
    u32::from_be_bytes(s[off..off + 4].try_into().unwrap())
}
fn put(s: &mut [u8], off: usize, v: f32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn word(s: &mut [u8], off: usize, v: u32) {
    s[off..off + 4].copy_from_slice(&v.to_be_bytes());
}
fn set_vector(s: &mut [u8], off: usize, v: V) {
    for i in 0..4 {
        put(s, off + i * 4, v[i]);
    }
}
fn length(v: V) -> Result<f32> {
    let squared = v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
    ensure!(
        squared.is_finite(),
        "Native camera distance length overflow"
    );
    Ok(if squared == 0. {
        0.
    } else {
        squared * super::native_hk_transform::inverse_root(squared)?
    })
}
fn normalized(mut v: V) -> Result<V> {
    let len = length(v)?;
    if len > f32::from_bits(0x38d1b717) {
        let scale = 1. / len;
        v = v.map(|x| x * scale);
    }
    Ok(v)
}
fn bool_property(b: &mut impl Backend, off: usize) -> Result<bool> {
    if off == 0x2b0 {
        b.thinned_collides()
    } else {
        b.boolean(off)
    }
}
fn scalar(b: &mut impl Backend, off: usize) -> Result<f32> {
    let value = b.scalar(off)?;
    ensure!(
        value.is_finite(),
        "Nonfinite native camera scalar property {off:x}"
    );
    Ok(value)
}
fn thin(b: &mut impl Backend, body: usize, key: u32) -> Result<bool> {
    Ok(b.material(body, key)? & 0x18000 == 0)
}
fn blocks(b: &mut impl Backend, body: usize, key: u32) -> Result<bool> {
    Ok(!thin(b, body, key)? || bool_property(b, 0x2b0)?)
}
fn clear_above_actor(
    hit: super::camera_collision::CastHit,
    radius: f32,
    b: &mut impl Backend,
) -> Result<bool> {
    let Some(actor) = b.tracked_actor_position()? else {
        return Ok(false);
    };
    let height = hit.point[1] - actor[1];
    if !(height > 0. && height < b.ignore_height()?) {
        return Ok(false);
    }
    let mut center = hit.point;
    center[1] = radius + (actor[1] + b.ignore_height()?);
    let mut obstructed = false;
    for hit in b.overlap(center, radius, FILTER)? {
        if blocks(b, hit.body, hit.key)? {
            obstructed = true;
        }
    }
    Ok(!obstructed)
}
/// Return native distance fraction. Velocity is the caller's mutable370 field;
/// partial scalar/flag/fade writes survive errors as they do at query boundaries.
/// No distance clamp, spring delta scaling or arbitrary contact cap is added.
pub fn solve(
    s: &mut [u8],
    from: V,
    to: V,
    velocity: &mut f32,
    follow: bool,
    fades: &mut Vec<LookThrough>,
    b: &mut impl Backend,
) -> Result<f32> {
    ensure!(
        s.len() >= 0x7f4,
        "Native camera distance owner is too short"
    );
    ensure!(
        from[..3].iter().chain(&to[..3]).all(|v| v.is_finite()),
        "Unsupported native camera distance positions"
    );
    let mut direction: V = std::array::from_fn(|i| to[i] - from[i]);
    let full = length(direction)?;
    let mut distance = full;
    let previous = full * f(s, 0x384);
    // Native returns before touching radius, velocity or configuration. Even
    // uninitialized/NaN values in those unused fields must not prevent it.
    if full == 0. {
        return Ok(0.1);
    }
    ensure!(
        [*velocity, f(s, 0x580), f(s, 0x384)]
            .into_iter()
            .all(f32::is_finite)
            && f(s, 0x580) >= 0.,
        "Unsupported native camera distance scalar state"
    );
    let inverse = 1. / full;
    direction = direction.map(|x| x * inverse);
    let moving = if bool_property(b, 0x2c8)? {
        let movement = normalized(std::array::from_fn(|i| {
            f(s, 0x2d0 + i * 4) - f(s, 0x2e0 + i * 4)
        }))?;
        let dot = movement[2].mul_add(direction[2], movement[0] * direction[0])
            + movement[1] * direction[1];
        dot > super::native_angle::sin_cos_medium(0.5)?.1 as f32
    } else {
        true
    };
    let radius = f(s, 0x580);
    direction = direction.map(|x| x * radius);
    let short: V = std::array::from_fn(|i| from[i] + direction[i]);
    let closest = b.ray(from, short, FILTER)?.first().copied();
    let mut has_contact = closest.is_some();
    if !bool_property(b, 0x2b0)? {
        has_contact = false;
    }
    let initially_inside = has_contact && closest.is_some_and(|h| h.fraction == 0.);
    if initially_inside {
        s[0x4d8] = 1;
        put(s, 0x4dc, 0.);
    }
    if has_contact && !initially_inside {
        if !bool_property(b, 0x2b4)? {
            distance = radius * closest.unwrap().fraction;
            if follow {
                if w(s, 0x4b0) == 0 && s[0x4ac] == 0 && s[0xdd] == 0 && w(s, 0x294) == w(s, 0x298) {
                    word(s, 0x4b0, 2);
                }
                s[0x4ac] = 1;
            }
        }
    } else {
        s[0x4ac] = 0;
        let hits = b.sphere(from, to, radius, FILTER)?;
        has_contact = !hits.is_empty();
        if !has_contact {
            s[0x4d8] = 0;
        }
        let mut thinned_seen = false;
        for hit in hits {
            if hit.fraction == 0. {
                continue;
            }
            if b.allow_look_through(hit.body)? {
                let entity = b.look_through_entity(hit.body)?;
                fades.try_reserve(1)?;
                fades.push(LookThrough {
                    fraction: hit.fraction,
                    entity,
                });
                continue;
            }
            if thin(b, hit.body, hit.key)? {
                if !bool_property(b, 0x2b0)? {
                    continue;
                }
                set_vector(s, 0x4e0, hit.point);
                if thinned_seen {
                    continue;
                }
                thinned_seen = true;
                if bool_property(b, 0x2b4)? {
                    if s[0x4d8] != 0 {
                        if bool_property(b, 0x2b8)? && full * hit.fraction > 0.5 + scalar(b, 0x2bc)?
                        {
                            s[0x4f1] = 0;
                            word(s, 0x4b0, 2);
                        }
                        continue;
                    }
                    if moving && bool_property(b, 0x2b8)? && full * hit.fraction < scalar(b, 0x2bc)?
                    {
                        s[0x4f1] = 1;
                        s[0x4d8] = 1;
                        put(s, 0x4dc, 0.);
                        continue;
                    }
                    if s[0x4f1] != 0 {
                        if !bool_property(b, 0x2b8)? {
                            continue;
                        }
                        if full * hit.fraction <= 0.5 + scalar(b, 0x2bc)? {
                            continue;
                        }
                        s[0x4f1] = 0;
                        s[0x4d8] = 1;
                        put(s, 0x4dc, 0.);
                    }
                }
            }
            if clear_above_actor(hit, radius, b)? {
                continue;
            }
            distance = full * hit.fraction;
            break;
        }
    }
    let delta = distance - previous;
    if has_contact && s[0x1be] != 0 && delta < -(0.25 * previous) && s[0x7f2] == 0 {
        s[0x1bf] = 1;
        return Ok(f(s, 0x384));
    }
    if delta > 0. {
        if f(s, 0x444) == 0.
            && f(s, 0x44c) > 0.
            && f(s, 0x198) == 0.
            && s[0xdd] == 0
            && s[0x464] == 0
            && f(s, 0x488) == f32::MAX
            && w(s, 0x634) & 0x10 == 0
        {
            *velocity = 0.;
        } else {
            let coefficient = scalar(b, 0x3e8)?;
            super::camera_attributes::coefficient(s, 0x3dc, coefficient);
            put(s, 0x3e4, *velocity);
            let next = if delta.abs() < f32::from_bits(0x38d1b717) {
                0.
            } else {
                *velocity + f(s, 0x3dc).mul_add(delta, -(f(s, 0x3e0) * *velocity))
            };
            put(s, 0x3e4, next);
            *velocity = next;
        }
    } else {
        let coefficient = scalar(b, if has_contact { 0x3d4 } else { 0x3d8 })?;
        super::camera_attributes::coefficient(s, 0x3c8, coefficient);
        put(s, 0x3d0, *velocity);
        let next = if delta.abs() < f32::from_bits(0x38d1b717) {
            0.
        } else {
            *velocity + f(s, 0x3c8).mul_add(delta, -(f(s, 0x3cc) * *velocity))
        };
        put(s, 0x3d0, next);
        *velocity = next;
    }
    if -*velocity >= previous {
        *velocity = 0.;
    }
    distance = previous + *velocity;
    let mut fraction = distance / full;
    if has_contact && w(s, 0x4b0) == 0 {
        direction = normalized(direction)?;
        let end: V = std::array::from_fn(|i| direction[i].mul_add(distance + radius, from[i]));
        let mut thinned_seen = false;
        for hit in b.ray(from, end, FILTER)? {
            if hit.fraction < radius {
                continue;
            }
            if b.allow_look_through(hit.body)? {
                continue;
            }
            let mut key = u32::MAX;
            for next in hit.keys {
                if next == u32::MAX {
                    break;
                }
                key = next;
            }
            if thin(b, hit.body, key)? {
                if !bool_property(b, 0x2b0)? {
                    continue;
                }
                if thinned_seen {
                    continue;
                }
                thinned_seen = true;
                if bool_property(b, 0x2b4)? {
                    if s[0x4d8] != 0 {
                        s[0x4f1] = 0;
                        continue;
                    }
                    if moving
                        && bool_property(b, 0x2b8)?
                        && distance * hit.fraction < scalar(b, 0x2bc)?
                    {
                        s[0x4f1] = 1;
                        continue;
                    }
                    if s[0x4f1] != 0 {
                        if !bool_property(b, 0x2b8)? {
                            continue;
                        }
                        if distance * hit.fraction <= 0.5 + scalar(b, 0x2bc)? {
                            continue;
                        }
                        s[0x4f1] = 0;
                        s[0x4d8] = 1;
                        put(s, 0x4dc, 0.);
                    }
                }
            }
            distance *= hit.fraction;
            fraction = distance / full;
            break;
        }
    }
    Ok(fraction)
}
#[cfg(test)]
mod tests {
    use super::super::camera_collision::{Backend as QueryBackend, CastHit, RayHit};
    use super::super::camera_placement::Overlap;
    use super::*;
    struct Host<'a> {
        case: &'a serde_json::Value,
        events: Vec<serde_json::Value>,
        rays: usize,
    }
    impl Host<'_> {
        fn event(&mut self, s: String) {
            self.events.push(serde_json::Value::String(s));
        }
        fn xyz(v: V) -> [u32; 3] {
            std::array::from_fn(|i| v[i].to_bits())
        }
        fn property(&self, off: usize) -> &serde_json::Value {
            &self.case["properties"][off.to_string()]
        }
    }
    impl QueryBackend for Host<'_> {
        fn ray(&mut self, start: V, end: V, filter: u32) -> Result<Vec<RayHit>> {
            self.rays += 1;
            assert_eq!(filter, FILTER);
            self.events.push(serde_json::json!({"kind":if self.rays==1 {"closest"}else{"proximity"},"start":Self::xyz(start),"end":Self::xyz(end),"filter":filter}));
            if self.rays == 1 {
                return Ok(self.case["closest"]
                    .as_f64()
                    .map(|v| {
                        vec![RayHit {
                            body: 20,
                            fraction: v as f32,
                            keys: [u32::MAX; 8],
                        }]
                    })
                    .unwrap_or_default());
            }
            self.case["rays"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| {
                    Ok(RayHit {
                        body: h["body"].as_u64().unwrap() as usize,
                        fraction: h["fraction"].as_f64().unwrap() as f32,
                        keys: serde_json::from_value(h["keys"].clone())?,
                    })
                })
                .collect()
        }
        fn sphere(&mut self, start: V, end: V, radius: f32, filter: u32) -> Result<Vec<CastHit>> {
            self.events.push(serde_json::json!({"kind":"sphere","start":Self::xyz(start),"end":Self::xyz(end),"radius_bits":radius.to_bits(),"filter":filter}));
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
            self.event(format!("look:{body}"));
            Ok(self.case["through"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_u64() == Some(body as u64)))
        }
        fn material(&mut self, body: usize, key: u32) -> Result<u32> {
            self.event(format!("material:{body}:{key}"));
            Ok(self.case["materials"][body.to_string()].as_u64().unwrap() as u32)
        }
    }
    impl PlacementBackend for Host<'_> {
        fn thinned_collides(&mut self) -> Result<bool> {
            self.boolean(0x2b0)
        }
        fn tracked_actor_position(&mut self) -> Result<Option<V>> {
            self.event("tracked".into());
            Ok(self.case["actor"]
                .as_bool()
                .unwrap()
                .then_some([1., 0., 3., 0.]))
        }
        fn ignore_height(&mut self) -> Result<f32> {
            self.scalar(0x788)
        }
        fn overlap(&mut self, start: V, radius: f32, filter: u32) -> Result<Vec<Overlap>> {
            self.events.push(serde_json::json!({"kind":"overlap","start":Self::xyz(start),"end":null,"radius_bits":radius.to_bits(),"filter":filter}));
            Ok(self.case["overlaps"]
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
    impl Backend for Host<'_> {
        fn boolean(&mut self, off: usize) -> Result<bool> {
            self.event(format!("bool:{off:x}"));
            Ok(self.property(off).as_bool().unwrap())
        }
        fn scalar(&mut self, off: usize) -> Result<f32> {
            self.event(format!("float:{off:x}"));
            Ok(self.property(off).as_f64().unwrap() as f32)
        }
        fn look_through_entity(&mut self, body: usize) -> Result<usize> {
            self.event(format!("entity:{body}"));
            Ok(body + 1000)
        }
    }
    #[test]
    fn distance_control_flow_queries_springs_and_mutations_match_original_instructions(
    ) -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_distance.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut record = vec![0x33; 0x800];
            for (off, value) in case["fields"].as_object().unwrap() {
                word(&mut record, off.parse()?, value.as_u64().unwrap() as u32);
            }
            for (off, value) in case["words"].as_object().unwrap() {
                word(&mut record, off.parse()?, value.as_u64().unwrap() as u32);
            }
            for (off, value) in case["flags"].as_object().unwrap() {
                record[off.parse::<usize>()?] = value.as_u64().unwrap() as u8;
            }
            let mut expected = record.clone();
            for (off, value) in case["changes"].as_object().unwrap() {
                expected[off.parse::<usize>()?] = value.as_u64().unwrap() as u8;
            }
            let start = serde_json::from_value(case["start"].clone())?;
            let end = serde_json::from_value(case["end"].clone())?;
            let mut velocity =
                f32::from_bits(case["initial_velocity_bits"].as_u64().unwrap() as u32);
            let mut fades = Vec::new();
            let mut host = Host {
                case,
                events: Vec::new(),
                rays: 0,
            };
            let result = solve(
                &mut record,
                start,
                end,
                &mut velocity,
                case["follow"].as_bool().unwrap(),
                &mut fades,
                &mut host,
            )?;
            assert_eq!(
                result.to_bits(),
                case["result_bits"].as_u64().unwrap() as u32,
                "Result case {index}: {}",
                case["scenario"]
            );
            assert_eq!(
                velocity.to_bits(),
                case["velocity_bits"].as_u64().unwrap() as u32,
                "Velocity case {index}"
            );
            assert_eq!(record, expected, "State case {index}");
            assert_eq!(
                host.events,
                *case["events"].as_array().unwrap(),
                "Query/property order case {index}"
            );
            let fades: Vec<_> = fades
                .iter()
                .map(
                    |v| serde_json::json!({"fraction_bits":v.fraction.to_bits(),"entity":v.entity}),
                )
                .collect();
            assert_eq!(
                fades,
                *case["fades"].as_array().unwrap(),
                "Fade list case {index}"
            );
        }
        Ok(())
    }
}
