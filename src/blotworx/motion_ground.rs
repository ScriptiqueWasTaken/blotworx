//! Motion ground-record publication, REL308b20..308d24. Query/collector and
//! supporting-body ownership are explicit inputs, not guessed contact normals.
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
pub type V = [f32; 4];
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum SupportBody {
    Unknown,
    Absent,
    Body(usize),
}
///23a148 clears the prior supporting body before contact traversal.
///23a528..628 assigns each eligible body in traversal order. Character
/// motion7 does not replace it. Missing contact metadata is unknown, not fixed.
/// Backend manifold order/disabled flags and native listeners remain separate.
pub fn contact_support_body(
    contacts: impl IntoIterator<Item = (usize, Option<u8>)>,
) -> SupportBody {
    let mut selected = SupportBody::Absent;
    for (body, motion_type) in contacts {
        match motion_type {
            Some(7) => {}
            Some(1..=6) => selected = SupportBody::Body(body),
            _ => selected = SupportBody::Unknown,
        }
    }
    selected
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Body {
    Own,
    Rigid(usize),
    Other,
}
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub point: V,
    pub normal: V,
    pub body: Body,
}
#[derive(Clone, Copy, Debug)]
pub struct Input {
    pub position: V,
    pub radius: f32,
    pub maximum: f32,
    pub slope_cosine: f32,
    pub enabled: bool,
    pub support: u32,
    pub supporting_body: SupportBody,
}
#[derive(Clone, Debug, Serialize)]
pub struct State {
    ///304304..318 explicitly constructs all four lanes as zero. The producer
    /// retains this point on no-hit, disabled and supported updates.
    pub last_hit: Option<V>,
    pub last_body: Option<usize>,
    /// Observed query metadata; not additional serialized component fields.
    pub ray: Option<(V, V)>,
    pub normal: Option<V>,
    pub support: u32,
    pub surface_velocity: Option<V>,
    pub distance: Option<f32>,
    pub maximum: f32,
    pub queried: bool,
    pub hit_this_update: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            last_hit: Some([0.; 4]),
            last_body: None,
            ray: None,
            normal: None,
            support: 0,
            surface_velocity: Some([0.; 4]),
            distance: None,
            maximum: 0.,
            queried: false,
            hit_this_update: false,
        }
    }
}
pub trait Query {
    /// Return original collector order (23f1a8); no hit cap or deduplication.
    fn hits(&mut self, start: V, end: V) -> Result<Vec<Hit>>;
    /// None is unresolved adapter metadata, never a zero velocity assumption.
    fn linear_velocity(&mut self, body: usize) -> Result<Option<V>>;
}
impl State {
    pub fn update(&mut self, input: Input, query: &mut impl Query) -> Result<()> {
        ensure!(
            input
                .position
                .iter()
                .chain([input.radius, input.maximum, input.slope_cosine].iter())
                .all(|v| v.is_finite()),
            "Nonfinite motion ground input"
        );
        //29c is written before the enabled branch. Disabled retains other fields.
        self.maximum = input.maximum;
        self.support = input.support;
        self.queried = false;
        self.hit_this_update = false;
        self.ray = None;
        if !input.enabled {
            return Ok(());
        }
        self.surface_velocity = Some([0.; 4]);
        if input.support == 2 {
            self.distance = Some(0.);
            self.surface_velocity = match input.supporting_body {
                SupportBody::Unknown => None,
                SupportBody::Absent => Some([0.; 4]),
                SupportBody::Body(body) => query.linear_velocity(body)?,
            };
            return Ok(());
        }
        let start =
            std::array::from_fn(|i| input.position[i] + if i == 1 { input.radius } else { 0. });
        let end =
            std::array::from_fn(|i| input.position[i] + if i == 1 { -input.maximum } else { 0. });
        self.queried = true;
        self.ray = Some((start, end));
        for hit in query.hits(start, end)? {
            let Body::Rigid(body) = hit.body else {
                continue;
            };
            ensure!(
                hit.normal.iter().chain(&hit.point).all(|v| v.is_finite()),
                "Nonfinite motion ground hit"
            );
            // Native paired dot against world up; strict >, not >= and not
            // a second normalization of the contact normal.
            let upward = hit.normal[2].mul_add(0., hit.normal[0] * 0.) + hit.normal[1];
            if upward <= input.slope_cosine {
                continue;
            }
            self.last_hit = Some(hit.point);
            self.normal = Some(hit.normal);
            self.last_body = Some(body);
            self.hit_this_update = true;
            self.surface_velocity = query.linear_velocity(body)?;
            self.distance = Some(input.position[1] - hit.point[1]);
            return Ok(());
        }
        self.distance = Some(input.maximum);
        Ok(())
    }
}
///2967b4 selects these four configuration handles.37ad6c..adac retains
/// Entity Radius as component+7c even when using an authored convex hull.
pub fn radius(profile: i64, settings: &BTreeMap<String, String>) -> Result<f32> {
    let (key, default) = match profile {
        0 => ("ThinEntityRadius", 0.5),
        1 => ("MediumEntityRadius", 1.),
        2 => ("WideEntityRadius", 1.5),
        3 => ("PlayerEntityRadius", 0.4375),
        _ => anyhow::bail!("Unsupported native Entity Radius profile {profile}"),
    };
    let value = settings
        .get(key)
        .map(|s| s.parse::<f64>().with_context(|| format!("Invalid {key}")))
        .transpose()?
        .map_or(default, |v| v as f32);
    ensure!(value.is_finite(), "Nonfinite {key}");
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Backend {
        hits: Vec<Hit>,
        calls: Vec<&'static str>,
        segment: Option<(V, V)>,
    }
    impl Query for Backend {
        fn hits(&mut self, a: V, b: V) -> Result<Vec<Hit>> {
            self.calls.push("query");
            self.segment = Some((a, b));
            Ok(self.hits.clone())
        }
        fn linear_velocity(&mut self, body: usize) -> Result<Option<V>> {
            self.calls.push("velocity");
            Ok(Some([body as f32, 2., 3., 4.]))
        }
    }
    fn input() -> Input {
        Input {
            position: [2., 3., 4., 0.],
            radius: 0.39,
            maximum: 5.,
            slope_cosine: 0.75,
            enabled: true,
            support: 0,
            supporting_body: SupportBody::Unknown,
        }
    }
    fn hit(y: f32, normal: f32, body: Body) -> Hit {
        Hit {
            point: [2., y, 4., 7.],
            normal: [0., normal, 0., 0.],
            body,
        }
    }
    #[test]
    fn ray_offsets_and_strict_slope_keep_original_order_and_support_body_velocity() -> Result<()> {
        let mut backend = Backend {
            hits: vec![
                hit(2.9, 1., Body::Own),
                hit(2.8, 1., Body::Other),
                hit(2.7, 0.75, Body::Rigid(8)),
                hit(2.5, 0.8, Body::Rigid(9)),
                hit(2., 1., Body::Rigid(10)),
            ],
            ..Default::default()
        };
        let mut state = State::default();
        state.update(input(), &mut backend)?;
        assert_eq!(
            backend.segment,
            Some(([2., 3. + 0.39, 4., 0.], [2., -2., 4., 0.]))
        );
        assert_eq!(state.ray, backend.segment);
        assert_eq!(backend.calls, ["query", "velocity"]);
        assert_eq!(state.last_body, Some(9));
        assert_eq!(state.last_hit, Some([2., 2.5, 4., 7.]));
        assert_eq!(state.distance, Some(0.5));
        assert_eq!(state.surface_velocity, Some([9., 2., 3., 4.]));
        Ok(())
    }
    #[test]
    fn no_hit_supported_and_disabled_paths_preserve_distinct_native_writes() -> Result<()> {
        let mut backend = Backend {
            hits: vec![hit(2.5, 1., Body::Rigid(9))],
            ..Default::default()
        };
        let mut state = State::default();
        state.update(input(), &mut backend)?;
        let point = state.last_hit;
        backend.hits.clear();
        backend.calls.clear();
        state.update(input(), &mut backend)?;
        assert_eq!(state.distance, Some(5.));
        assert_eq!(state.last_hit, point);
        assert_eq!(state.surface_velocity, Some([0.; 4]));
        assert!(!state.hit_this_update);
        backend.calls.clear();
        state.update(
            Input {
                support: 2,
                ..input()
            },
            &mut backend,
        )?;
        assert_eq!(state.distance, Some(0.));
        assert_eq!(state.surface_velocity, None);
        assert!(backend.calls.is_empty());
        state.update(
            Input {
                support: 2,
                supporting_body: SupportBody::Body(11),
                ..input()
            },
            &mut backend,
        )?;
        assert_eq!(state.surface_velocity, Some([11., 2., 3., 4.]));
        assert_eq!(backend.calls, ["velocity"]);
        state.update(
            Input {
                enabled: false,
                maximum: 7.,
                ..input()
            },
            &mut backend,
        )?;
        assert_eq!(state.maximum, 7.);
        assert_eq!(state.distance, Some(0.));
        assert_eq!(state.last_hit, point);
        Ok(())
    }
    #[test]
    fn contact_traversal_keeps_last_eligible_body_and_does_not_select_character_motion() {
        assert_eq!(contact_support_body([]), SupportBody::Absent);
        assert_eq!(
            contact_support_body([(3, Some(5)), (4, Some(4)), (5, Some(7))]),
            SupportBody::Body(4)
        );
        for mode in 1..=6 {
            assert_eq!(
                contact_support_body([(9, Some(mode))]),
                SupportBody::Body(9)
            );
        }
        assert_eq!(
            contact_support_body([(3, Some(5)), (4, None)]),
            SupportBody::Unknown
        );
        assert_eq!(
            contact_support_body([(3, None), (4, Some(5)), (5, Some(7))]),
            SupportBody::Body(4)
        );
    }
    #[test]
    fn native_constructor_zero_point_survives_the_first_no_hit() -> Result<()> {
        let mut state = State::default();
        assert_eq!(state.last_hit, Some([0.; 4]));
        assert_eq!(state.surface_velocity, Some([0.; 4]));
        state.update(input(), &mut Backend::default())?;
        assert_eq!(state.last_hit, Some([0.; 4]));
        assert_eq!(state.distance, Some(5.));
        Ok(())
    }
    #[test]
    fn configured_radius_profiles_keep_source_values_and_reject_unknowns() -> Result<()> {
        let mut settings = BTreeMap::new();
        assert_eq!(radius(3, &settings)?, 0.4375);
        settings.insert("PlayerEntityRadius".into(), "0.39".into());
        assert_eq!(radius(3, &settings)?, 0.39);
        settings.insert("ThinEntityRadius".into(), "-0.5".into());
        assert_eq!(radius(0, &settings)?, -0.5);
        assert!(radius(4, &settings).is_err());
        Ok(())
    }
}
