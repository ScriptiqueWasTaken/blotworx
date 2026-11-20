//! Authored extended-mesh edge welding; separate from triangle collision filters.
use anyhow::{ensure, Context, Result};
#[derive(Debug)]
pub struct Welding {
    pub mode: u8,
    pub words: Vec<u16>,
}
#[derive(Debug)]
pub enum Prepared {
    Authored(std::sync::Arc<Welding>),
    TwoSided(Regenerated),
}

#[derive(Debug, PartialEq)]
pub enum ContactNormal {
    Accept,
    Reject,
    Snapped([f32; 4]),
}

/// DOL 8027edcc..8027ee58. Feature IDs include extruded vertices 3..5;
/// constructor 80280560..74 initializes mapping [0,1,2,0,1,2]. None is the
/// original face-contact early accept. Only three-feature reduction mutates
/// IDs/count; preserve raw IDs for the one/two-feature paths.
pub fn reduce_contact_features(features: &mut [u16; 3], count: &mut u8) -> Result<Option<usize>> {
    ensure!((1..=3).contains(count), "Invalid contact feature count");
    ensure!(
        features[..*count as usize].iter().all(|&id| id < 6),
        "Invalid triangle feature ID"
    );
    const VERTEX: [u16; 6] = [0, 1, 2, 0, 1, 2];
    let mut first = VERTEX[features[0] as usize];
    if *count > 1 {
        let mut second = VERTEX[features[1] as usize];
        if *count == 3 {
            // The retail branch sums RAW feature IDs, not mapped IDs or a set.
            if features.iter().map(|&v| v as u32).sum::<u32>() == 3 {
                return Ok(None);
            }
            if second == first {
                second = VERTEX[features[2] as usize];
            }
            *count = 2;
            features[0] = first;
            features[1] = second;
        }
        if second != VERTEX[first as usize + 1] {
            first = second;
        }
    }
    Ok(Some(first as usize))
}

/// Original table starts in zeroed BSS; world initialization overwrites only
/// entries 0..30. Owned per compatibility world to avoid implicit global state.
pub struct ContactTable {
    entries: [[f32; 6]; 32],
}
impl ContactTable {
    pub fn new(edge_delta: f32, triangle_delta: f32) -> Result<Self> {
        let mut entries = [[0.; 6]; 32];
        initialize_contact_table_native(&mut entries, edge_delta, triangle_delta)?;
        Ok(Self { entries })
    }
    /// DOL config constructor 802e0988/98c; REL 240af8..240c24 retains
    /// these fields before creating the gameplay physics world.
    pub fn retail_world() -> Result<Self> {
        Self::new(f32::from_bits(0x3f0624dd), f32::from_bits(0x3f32b021))
    }
    pub fn edge_entry(&self, word: u16, edge: usize) -> Result<[f32; 6]> {
        ensure!(edge < 3, "Invalid contact welding edge");
        Ok(self.entries[((word >> (edge * 5)) & 31) as usize])
    }
}

/// Two-sided triangle contact path from 8027edcc through 8027f564.
/// separating is the caller's world normal vector, including its fourth lane.
/// Do not assume W is the final surface distance: the closest-point caller
/// at 80236a48 passes rotated normal W, then overwrites W with its separately
/// retained surface distance when publishing the contact at 80236b70.
/// Returns native result codes: 0 reject, 1 modified, 2 accepted unchanged.
/// Input feature IDs must be native triangle IDs, not Box3D feature encoding.
pub fn weld_two_sided_contact(
    vertices: [[f32; 4]; 3],
    transform: &super::native_hk_transform::Transform,
    features: &mut [u16; 3],
    count: &mut u8,
    word: u16,
    separating: &mut [f32; 4],
    table: &ContactTable,
) -> Result<u8> {
    let Some(edge_index) = reduce_contact_features(features, count)? else {
        return Ok(2);
    };
    ensure!(
        transform
            .iter()
            .chain(separating.iter())
            .all(|v| v.is_finite()),
        "Nonfinite contact frame"
    );
    let normal = triangle_normal(vertices)?;
    let edge = normalize4(std::array::from_fn(|i| {
        vertices[(edge_index + 1) % 3][i] - vertices[edge_index][i]
    }))?;
    // 8027eea0..8027ef10 transposes XYZ. The fourth paired-single lane
    // uses m9*x + m8*y + m11*z; preserve the caller's fourth-lane semantics.
    let mut local = [0.; 4];
    for i in 0..3 {
        local[i] = transform[i * 4 + 2].mul_add(
            separating[2],
            transform[i * 4 + 1].mul_add(separating[1], transform[i * 4] * separating[0]),
        );
    }
    local[3] = transform[11].mul_add(
        separating[2],
        transform[8].mul_add(separating[1], transform[9] * separating[0]),
    );
    let projection = normal[2].mul_add(local[2], normal[0] * local[0]) + normal[1] * local[1];
    let distance = separating[3] * projection.abs();
    match weld_two_sided_normal(
        normal,
        edge,
        local,
        table.edge_entry(word, edge_index)?,
        *count,
    )? {
        ContactNormal::Accept => Ok(2),
        ContactNormal::Reject => Ok(0),
        ContactNormal::Snapped(value) => {
            let mut output = super::native_hk_transform::rotate_vector(transform, value);
            output[3] = distance;
            ensure!(
                output.iter().all(|v| v.is_finite()),
                "Contact welding output overflow"
            );
            *separating = output;
            Ok(1)
        }
    }
}

/// Complete one-sided path with the original caller's position argument.
/// The center callback models virtual+4c and is only called for a snapped,
/// non-extruded contact. Preserve caller coordinates even when surprising.
pub fn weld_one_sided_contact(
    vertices: [[f32; 4]; 3],
    transform: &super::native_hk_transform::Transform,
    features: &mut [u16; 3],
    count: &mut u8,
    word: u16,
    separating: &mut [f32; 4],
    table: &ContactTable,
    mode: u8,
    extruded: bool,
    contact_position: [f32; 4],
    opposing_transform: &super::native_hk_transform::Transform,
    opposing_center: impl FnOnce() -> Result<[f32; 4]>,
) -> Result<u8> {
    ensure!(matches!(mode, 0 | 4), "Invalid one-sided contact mode");
    let Some(edge_index) = reduce_contact_features(features, count)? else {
        return Ok(2);
    };
    ensure!(
        transform
            .iter()
            .chain(separating.iter())
            .all(|v| v.is_finite()),
        "Nonfinite one-sided frame"
    );
    let normal = triangle_normal(vertices)?;
    let edge = normalize4(std::array::from_fn(|i| {
        vertices[(edge_index + 1) % 3][i] - vertices[edge_index][i]
    }))?;
    let mut local = [0.; 4];
    for i in 0..3 {
        local[i] = transform[i * 4 + 2].mul_add(
            separating[2],
            transform[i * 4 + 1].mul_add(separating[1], transform[i * 4] * separating[0]),
        );
    }
    local[3] = transform[11].mul_add(
        separating[2],
        transform[8].mul_add(separating[1], transform[9] * separating[0]),
    );
    let projection = normal[2].mul_add(local[2], normal[0] * local[0]) + normal[1] * local[1];
    let mut depth = separating[3] * projection.abs();
    let candidate = match weld_one_sided_candidate(
        normal,
        edge,
        local,
        table.edge_entry(word, edge_index)?,
        mode,
    )? {
        ContactNormal::Accept => return Ok(2),
        ContactNormal::Snapped(v) => v,
        ContactNormal::Reject => unreachable!("angular stage never rejects"),
    };
    let mut world = super::native_hk_transform::rotate_vector(transform, candidate);
    if extruded {
        depth = one_sided_extrusion_depth(
            contact_position,
            transform,
            vertices[0],
            normal,
            mode,
            depth,
        )?;
    } else if !one_sided_position_accepts(
        contact_position,
        opposing_center()?,
        opposing_transform,
        world,
    )? {
        return Ok(0);
    }
    world[3] = depth;
    ensure!(
        world.iter().all(|v| v.is_finite()),
        "One-sided contact output overflow"
    );
    *separating = world;
    Ok(1)
}

/// Closest-point publication policy, DOL 80236900..80236b80. This is not
/// the persistent-manifold path. The supplied normal is already in world
/// space, with the original rotation's fourth lane, not a packed distance.
/// The welding callback must use the native feature IDs and shape ordering.
pub fn weld_closest_contact(
    core_distance: f32,
    radius_a: f32,
    radius_b: f32,
    tolerance: f32,
    mut normal: [f32; 4],
    weld: impl FnOnce(&mut [f32; 4]) -> Result<u8>,
) -> Result<Option<[f32; 4]>> {
    ensure!(
        [core_distance, radius_a, radius_b, tolerance]
            .iter()
            .chain(normal.iter())
            .all(|v| v.is_finite()),
        "Nonfinite closest-contact data"
    );
    ensure!(
        radius_a >= 0. && radius_b >= 0.,
        "Negative closest-contact radius"
    );
    // Two fsubs instructions, not core_distance - (radius_a + radius_b).
    let distance = (core_distance - radius_a) - radius_b;
    ensure!(distance.is_finite(), "Closest-contact distance overflow");
    if distance >= tolerance {
        return Ok(None);
    }
    let original = normal;
    match weld(&mut normal)? {
        0 => return Ok(None),
        1 => {}
        2 => normal = original,
        _ => anyhow::bail!("Invalid native welding result"),
    }
    // The native collector always receives the pre-welding surface distance.
    normal[3] = distance;
    ensure!(
        normal.iter().all(|v| v.is_finite()),
        "Closest-contact normal overflow"
    );
    Ok(Some(normal))
}

#[derive(Debug, PartialEq)]
pub struct ClosestContact {
    pub position: [f32; 4],
    pub separating: [f32; 4],
}

/// Bind recovered GSK B-source IDs to a triangle welding callback. Mode6
/// bypasses all geometry/feature reads in 8027ed98..edc8; mode5 selects the
/// recovered two-sided path at8027f060. Modes0/4 require caller context.
pub struct OneSidedQueryContext<'a> {
    pub contact_position: [f32; 4],
    pub opposing_transform: &'a super::native_hk_transform::Transform,
    pub opposing_center: &'a mut dyn FnMut() -> Result<[f32; 4]>,
    pub extruded: bool,
}
pub fn weld_query_triangle(
    vertices: [[f32; 4]; 3],
    transform: &super::native_hk_transform::Transform,
    features: &super::native_gsk::Features,
    mode: u8,
    word: u16,
    normal: &mut [f32; 4],
    table: &ContactTable,
    one_sided: Option<OneSidedQueryContext<'_>>,
) -> Result<u8> {
    if mode == 6 {
        return Ok(2);
    }
    ensure!(
        matches!(mode, 0 | 4 | 5),
        "Invalid native triangle welding mode {mode}"
    );
    ensure!(
        (1..=3).contains(&features.count_b),
        "Invalid query triangle feature count"
    );
    let mut ids = std::array::from_fn(|i| features.vertices_b[i][3].to_bits() as u16);
    let mut count = features.count_b as u8;
    weld_triangle_ids(
        vertices, transform, &mut ids, &mut count, mode, word, normal, table, one_sided,
    )
}

/// 80236a5c/7c pass the cache's B count and IDs directly to virtual+48.
/// Preserve its in-place feature reduction, including when welding rejects
/// the contact. Do not canonicalize again after the triangle callback.
pub fn weld_cached_triangle(
    vertices: [[f32; 4]; 3],
    transform: &super::native_hk_transform::Transform,
    cache: &mut super::native_gsk::Cache,
    mode: u8,
    word: u16,
    normal: &mut [f32; 4],
    table: &ContactTable,
    one_sided: Option<OneSidedQueryContext<'_>>,
) -> Result<u8> {
    if mode == 6 {
        return Ok(2);
    }
    let (_, b) = cache.features()?;
    let original_count = b.len() / 2;
    let mut ids = [0; 3];
    for (id, bytes) in ids.iter_mut().zip(b.chunks_exact(2)) {
        *id = u16::from_be_bytes(bytes.try_into()?);
    }
    let mut count = original_count as u8;
    let result = weld_triangle_ids(
        vertices, transform, &mut ids, &mut count, mode, word, normal, table, one_sided,
    )?;
    let start = cache.0[8] as usize * 2;
    for (i, id) in ids[..original_count].iter().enumerate() {
        cache.0[start + 2 * i..start + 2 * i + 2].copy_from_slice(&id.to_be_bytes());
    }
    cache.0[9] = count;
    Ok(result)
}

fn weld_triangle_ids(
    vertices: [[f32; 4]; 3],
    transform: &super::native_hk_transform::Transform,
    ids: &mut [u16; 3],
    count: &mut u8,
    mode: u8,
    word: u16,
    normal: &mut [f32; 4],
    table: &ContactTable,
    one_sided: Option<OneSidedQueryContext<'_>>,
) -> Result<u8> {
    if mode == 6 {
        return Ok(2);
    }
    ensure!(
        matches!(mode, 0 | 4 | 5),
        "Invalid native triangle welding mode {mode}"
    );
    if mode == 5 {
        return weld_two_sided_contact(vertices, transform, ids, count, word, normal, table);
    }
    let context = one_sided.context("Missing one-sided query position/shape context")?;
    weld_one_sided_contact(
        vertices,
        transform,
        ids,
        count,
        word,
        normal,
        table,
        mode,
        context.extruded,
        context.contact_position,
        context.opposing_transform,
        context.opposing_center,
    )
}

pub enum ClosestQuery {
    /// Native GSK status4; no valid witness was produced.
    Unresolved,
    /// A valid query was outside tolerance or rejected by welding.
    NoContact,
    Contact(ClosestContact),
}

/// Closest query for an explicitly supplied triangle and its authored/prepared
/// welding word. The caller owns candidate selection and cache initialization.
pub fn query_cached_triangle_contact(
    cache: &mut super::native_gsk::Cache,
    opposing: &mut dyn super::native_gsk::SupportShape,
    triangle: &mut super::native_gsk::TriangleSupport,
    opposing_transform: &super::native_hk_transform::Transform,
    triangle_transform: &super::native_hk_transform::Transform,
    opposing_radius: f32,
    triangle_radius: f32,
    tolerance: f32,
    penetration_enabled: bool,
    difference_scratch: &mut [[f32; 4]; 4],
    initial_retry_padding: Option<f32>,
    welding_enabled: bool,
    mode: u8,
    word: u16,
    table: &ContactTable,
) -> Result<ClosestQuery> {
    use super::native_gsk::{Features, QueryResult};
    let relative = super::native_hk_transform::relative(opposing_transform, triangle_transform);
    let mut features = Features::load(cache, opposing, triangle, &relative)?;
    let mut dirty = 0;
    let core = match features.query_core_with_cache_flag(
        opposing,
        triangle,
        &relative,
        penetration_enabled,
        difference_scratch,
        initial_retry_padding,
        &mut dirty,
    )? {
        QueryResult::Unresolved => return Ok(ClosestQuery::Unresolved),
        QueryResult::Contact(core) => core,
    };
    // 80236a0c checks the caller's welding flag only after the distance gate.
    // 80236a30 publishes dirty features immediately before the callback.
    Ok(
        match closest_contact_from_core(
            core.point_a,
            core.normal,
            core.distance,
            opposing_transform,
            triangle_transform,
            opposing_radius,
            triangle_radius,
            tolerance,
            |point, normal| {
                if !welding_enabled {
                    return Ok(2);
                }
                if dirty != 0 {
                    features.publish_cache(cache)?;
                }
                let mut center = || opposing.center();
                let context = OneSidedQueryContext {
                    contact_position: *point,
                    opposing_transform,
                    opposing_center: &mut center,
                    extruded: triangle.extrusion_flag != 0,
                };
                weld_cached_triangle(
                    triangle.vertices,
                    triangle_transform,
                    cache,
                    mode,
                    word,
                    normal,
                    table,
                    Some(context),
                )
            },
        )? {
            Some(contact) => ClosestQuery::Contact(contact),
            None => ClosestQuery::NoContact,
        },
    )
}

/// Feature-level entry for callers that explicitly manage their own cache.
pub fn query_triangle_contact(
    features: &mut super::native_gsk::Features,
    opposing: &mut dyn super::native_gsk::SupportShape,
    triangle: &mut super::native_gsk::TriangleSupport,
    opposing_transform: &super::native_hk_transform::Transform,
    triangle_transform: &super::native_hk_transform::Transform,
    opposing_radius: f32,
    triangle_radius: f32,
    tolerance: f32,
    penetration_enabled: bool,
    difference_scratch: &mut [[f32; 4]; 4],
    initial_retry_padding: Option<f32>,
    mode: u8,
    word: u16,
    table: &ContactTable,
) -> Result<ClosestQuery> {
    let vertices = triangle.vertices;
    let extruded = triangle.extrusion_flag != 0;
    query_closest_contact(
        features,
        opposing,
        triangle,
        opposing_transform,
        triangle_transform,
        opposing_radius,
        triangle_radius,
        tolerance,
        penetration_enabled,
        difference_scratch,
        initial_retry_padding,
        |features, opposing, point, normal| {
            let mut center = || opposing.center();
            let context = OneSidedQueryContext {
                contact_position: *point,
                opposing_transform,
                opposing_center: &mut center,
                extruded,
            };
            weld_query_triangle(
                vertices,
                triangle_transform,
                features,
                mode,
                word,
                normal,
                table,
                Some(context),
            )
        },
    )
}

/// Connect the recovered core query to 802368ec..80236b80 publication.
/// Features are caller-owned so native source IDs remain available to welding.
/// This does not substitute Box3D simplex IDs or initialize a guessed cache.
pub fn query_closest_contact(
    features: &mut super::native_gsk::Features,
    a: &mut dyn super::native_gsk::SupportShape,
    b: &mut dyn super::native_gsk::SupportShape,
    transform_a: &super::native_hk_transform::Transform,
    transform_b: &super::native_hk_transform::Transform,
    radius_a: f32,
    radius_b: f32,
    tolerance: f32,
    penetration_enabled: bool,
    difference_scratch: &mut [[f32; 4]; 4],
    initial_retry_padding: Option<f32>,
    weld: impl FnOnce(
        &super::native_gsk::Features,
        &mut dyn super::native_gsk::SupportShape,
        &mut [f32; 4],
        &mut [f32; 4],
    ) -> Result<u8>,
) -> Result<ClosestQuery> {
    let relative = super::native_hk_transform::relative(transform_a, transform_b);
    let core = match features.query_core(
        a,
        b,
        &relative,
        penetration_enabled,
        difference_scratch,
        initial_retry_padding,
    )? {
        super::native_gsk::QueryResult::Unresolved => return Ok(ClosestQuery::Unresolved),
        super::native_gsk::QueryResult::Contact(core) => core,
    };
    Ok(
        match closest_contact_from_core(
            core.point_a,
            core.normal,
            core.distance,
            transform_a,
            transform_b,
            radius_a,
            radius_b,
            tolerance,
            |point, normal| weld(features, a, point, normal),
        )? {
            Some(contact) => ClosestQuery::Contact(contact),
            None => ClosestQuery::NoContact,
        },
    )
}

/// DOL 802368ec..80236b80, after GSK has provided its core witness on A.
/// Core point/normal are in A's frame; radii are applied with the original
/// fused operations. Welding sees B-local position and world-space normal.
/// This deliberately retains the B-local round trip used by the collector.
/// It does not derive GSK witnesses or feature IDs from Box3D's simplex.
pub fn closest_contact_from_core(
    point_a: [f32; 4],
    normal_a: [f32; 4],
    core_distance: f32,
    transform_a: &super::native_hk_transform::Transform,
    transform_b: &super::native_hk_transform::Transform,
    radius_a: f32,
    radius_b: f32,
    tolerance: f32,
    weld: impl FnOnce(&mut [f32; 4], &mut [f32; 4]) -> Result<u8>,
) -> Result<Option<ClosestContact>> {
    use super::native_hk_transform::{relative, rotate_vector, transform_point};
    ensure!(
        point_a
            .iter()
            .chain(normal_a.iter())
            .chain(transform_a.iter())
            .chain(transform_b.iter())
            .all(|v| v.is_finite()),
        "Nonfinite closest-contact frame"
    );
    let mut point_b = [0.; 4];
    let normal = rotate_vector(transform_a, normal_a);
    let separating = weld_closest_contact(
        core_distance,
        radius_a,
        radius_b,
        tolerance,
        normal,
        |normal| {
            let distance = (core_distance - radius_a) - radius_b;
            let surface_a: [f32; 4] =
                std::array::from_fn(|i| normal_a[i].mul_add(-radius_a, point_a[i]));
            let surface_b: [f32; 4] =
                std::array::from_fn(|i| normal_a[i].mul_add(-distance, surface_a[i]));
            let b_in_a = relative(transform_a, transform_b);
            let delta: [f32; 4] = std::array::from_fn(|i| surface_b[i] - b_in_a[12 + i]);
            for i in 0..3 {
                point_b[i] = b_in_a[i * 4 + 2].mul_add(
                    delta[2],
                    b_in_a[i * 4 + 1].mul_add(delta[1], b_in_a[i * 4] * delta[0]),
                );
            }
            // 802369c0/9d0/9e0/9f0 preserve the paired shuffle's padded lane.
            point_b[3] =
                b_in_a[11].mul_add(delta[2], b_in_a[8].mul_add(delta[1], b_in_a[9] * delta[0]));
            ensure!(
                point_b.iter().all(|v| v.is_finite()),
                "Closest-contact position overflow"
            );
            weld(&mut point_b, normal)
        },
    )?;
    let Some(separating) = separating else {
        return Ok(None);
    };
    let position = transform_point(transform_b, point_b);
    ensure!(
        position.iter().all(|v| v.is_finite()),
        "Closest-contact world position overflow"
    );
    Ok(Some(ClosestContact {
        position,
        separating,
    }))
}

/// DOL 802887a8 initializes entries 0..30 of runtime table 807139f0.
/// World constructor 802df148 supplies config +8c/+90. Entry 31 is untouched.
/// Trig callback must implement native cos (false, 80023f68) / sin (true,
/// 80024794), including their final f32 rounding; host libm is not assumed equal.
pub fn initialize_contact_table(
    table: &mut [[f32; 6]; 32],
    edge_delta: f32,
    triangle_delta: f32,
    mut trig: impl FnMut(bool, f32) -> Result<f32>,
) -> Result<()> {
    ensure!(
        edge_delta.is_finite() && triangle_delta.is_finite(),
        "Nonfinite welding table settings"
    );
    let pi = f32::from_bits(0x40490fdb);
    let step = f32::from_bits(0x3e567750);
    let mut call = |sine, angle| -> Result<f32> {
        ensure!(f32::is_finite(angle), "Welding table angle overflow");
        let value = trig(sine, angle)?;
        ensure!(value.is_finite(), "Nonfinite native welding trigonometry");
        Ok(value)
    };
    for (i, entry) in table[..31].iter_mut().enumerate() {
        let angle = step.mul_add(i as f32, -pi);
        if angle < 0. {
            entry[0] = 1.;
            entry[1] = 0.;
            let accept = pi - angle.abs();
            let snap = accept - edge_delta;
            entry[2] = call(
                false,
                if snap > triangle_delta {
                    triangle_delta
                } else if snap > 0. {
                    snap
                } else {
                    0.
                },
            )?;
            entry[3] = call(false, if snap > 0. { snap } else { 0. })?;
            entry[4] = call(false, accept)?;
            entry[5] = call(true, accept)?;
        } else {
            entry[0] = call(false, angle)?;
            entry[1] = call(true, angle)?;
            let snap = angle + edge_delta;
            entry[2] = call(false, if snap < pi { snap } else { pi })?;
            let limit = pi - triangle_delta;
            entry[3] = call(
                false,
                if snap < limit {
                    limit
                } else if snap < pi {
                    snap
                } else {
                    pi
                },
            )?;
            entry[4] = -1.;
            entry[5] = 0.;
        }
    }
    Ok(())
}

/// Connect recovered table arithmetic to the ported double trig path, with
/// the original frsp after each call. Unsupported reduction ranges are errors.
pub fn initialize_contact_table_native(
    table: &mut [[f32; 6]; 32],
    edge_delta: f32,
    triangle_delta: f32,
) -> Result<()> {
    initialize_contact_table(table, edge_delta, triangle_delta, |sine, angle| {
        let (s, c) = super::native_angle::sin_cos_medium(angle as f64)?;
        Ok(if sine { s as f32 } else { c as f32 })
    })
}

/// One-sided local stage8027f064..8027f1cc. A snapped candidate still needs
/// the original opposing-shape/position test before it can be published.
pub fn weld_one_sided_candidate(
    normal: [f32; 4],
    edge: [f32; 4],
    collision: [f32; 4],
    entry: [f32; 6],
    mode: u8,
) -> Result<ContactNormal> {
    ensure!(matches!(mode, 0 | 4), "Invalid one-sided welding mode");
    ensure!(
        normal
            .iter()
            .chain(&edge)
            .chain(&collision)
            .chain(&entry)
            .all(|v| v.is_finite()),
        "Nonfinite one-sided welding input"
    );
    let cosine =
        normal[2].mul_add(collision[2], normal[0] * collision[0]) + normal[1] * collision[1];
    ensure!(cosine.is_finite(), "One-sided welding projection overflow");
    let index = mode as usize;
    let needs_snap = if mode == 0 {
        cosine < entry[index]
    } else {
        cosine > entry[index]
    };
    if !needs_snap {
        return Ok(ContactNormal::Accept);
    }
    let tangent = cross4(edge, normal);
    let candidate = normalize4(std::array::from_fn(|i| {
        normal[i] * entry[index] + tangent[i] * entry[index + 1]
    }))?;
    Ok(ContactNormal::Snapped(candidate))
}

/// Non-extruded one-sided gate8027f280..f300. The caller supplies the
/// actual virtual+4c center, not support vertex0, and native contact-position
/// argument. Equality rejects; a strictly negative projection accepts.
pub fn one_sided_position_accepts(
    contact_position: [f32; 4],
    opposing_center: [f32; 4],
    opposing_transform: &super::native_hk_transform::Transform,
    world_candidate: [f32; 4],
) -> Result<bool> {
    ensure!(
        contact_position
            .iter()
            .chain(&opposing_center)
            .chain(opposing_transform)
            .chain(&world_candidate)
            .all(|v| v.is_finite()),
        "Nonfinite one-sided contact position"
    );
    let center = super::native_hk_transform::transform_point(opposing_transform, opposing_center);
    let delta: [f32; 4] = std::array::from_fn(|i| contact_position[i] - center[i]);
    let projection = delta[2].mul_add(world_candidate[2], delta[0] * world_candidate[0])
        + delta[1] * world_candidate[1];
    ensure!(
        projection.is_finite(),
        "One-sided contact projection overflow"
    );
    Ok(projection < 0.)
}

/// Extruded branch8027f1e0..f260: inverse-transform the native position
/// argument, project from triangle vertex0 onto its signed local normal,
/// and retain the smaller depth. Equality preserves the previous depth bits.
pub fn one_sided_extrusion_depth(
    contact_position: [f32; 4],
    triangle_transform: &super::native_hk_transform::Transform,
    vertex0: [f32; 4],
    local_normal: [f32; 4],
    mode: u8,
    previous_depth: f32,
) -> Result<f32> {
    ensure!(
        matches!(mode, 0 | 4),
        "Invalid extruded one-sided welding mode"
    );
    ensure!(
        contact_position
            .iter()
            .chain(triangle_transform)
            .chain(&vertex0)
            .chain(&local_normal)
            .chain(std::iter::once(&previous_depth))
            .all(|v| v.is_finite()),
        "Nonfinite extruded welding input"
    );
    let delta: [f32; 3] = std::array::from_fn(|i| contact_position[i] - triangle_transform[12 + i]);
    let local: [f32; 3] = std::array::from_fn(|i| {
        triangle_transform[i * 4 + 2].mul_add(
            delta[2],
            triangle_transform[i * 4 + 1].mul_add(delta[1], triangle_transform[i * 4] * delta[0]),
        )
    });
    let offset: [f32; 3] = std::array::from_fn(|i| local[i] - vertex0[i]);
    // Original table806b88cc stores +1 for mode0 and -1 for mode4.
    let normal = local_normal.map(|v| v * if mode == 0 { 1. } else { -1. });
    let projected = offset[2].mul_add(normal[2], offset[0] * normal[0]) + offset[1] * normal[1];
    ensure!(
        projected.is_finite(),
        "Extruded welding projection overflow"
    );
    Ok(if projected < previous_depth {
        projected
    } else {
        previous_depth
    })
}

/// Two-sided local-normal stage, DOL 8027f304..8027f55c. The caller supplies
/// the original six-float table entry, normalized local triangle/edge vectors,
/// and feature count AFTER native feature reduction. Face contacts bypass this
/// stage. Table initialization, feature reduction, world conversion and contact
/// distance adjustment remain separate; no guessed trigonometric table here.
pub fn weld_two_sided_normal(
    normal: [f32; 4],
    edge: [f32; 4],
    collision: [f32; 4],
    entry: [f32; 6],
    feature_count: u8,
) -> Result<ContactNormal> {
    ensure!(
        (1..=2).contains(&feature_count),
        "Welding requires a reduced vertex/edge feature"
    );
    ensure!(
        normal
            .iter()
            .chain(&edge)
            .chain(&collision)
            .chain(&entry)
            .all(|x| x.is_finite()),
        "Nonfinite contact welding input"
    );
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let cosine = dot(normal, collision);
    ensure!(cosine.is_finite(), "Contact welding dot overflow");
    // Preserve original ordered >= tests, including exact boundary equality.
    let sector = if cosine >= entry[0] {
        1
    } else if cosine >= entry[2] {
        0
    } else if cosine >= entry[3] {
        2
    } else if cosine >= entry[4] {
        4
    } else {
        3
    };
    if sector == 1 || sector == 3 {
        return Ok(ContactNormal::Accept);
    }
    if sector == 2 || feature_count == 1 {
        return Ok(ContactNormal::Reject);
    }
    let tangent = cross4(edge, normal);
    let snap = normalize4(std::array::from_fn(|i| {
        normal[i] * entry[sector] + tangent[i] * entry[sector + 1]
    }))?;
    let along_edge = dot(edge, collision);
    let along_snap = dot(snap, collision);
    let result = normalize4(std::array::from_fn(|i| {
        snap[i] * along_snap + edge[i] * along_edge
    }))?;
    Ok(ContactNormal::Snapped(result))
}

/// Owned regeneration storage. Native setWeldingInfo rebuilds sequential
/// subpart offsets and resizes without initializing new u16 slots. Represent
/// those unknown slots explicitly rather than reading uninitialized memory
/// or inventing zero welding for triangles the iterator never visits.
#[derive(Debug)]
pub struct Regenerated {
    pub offsets: Vec<usize>,
    words: Vec<Option<u16>>,
    counts: Vec<usize>,
}
impl Regenerated {
    pub fn new(original: &Welding, counts: &[usize]) -> Result<Self> {
        let mut total = 0usize;
        let mut offsets = Vec::with_capacity(counts.len());
        for &count in counts {
            offsets.push(total);
            total = total
                .checked_add(count)
                .context("Welding storage overflow")?;
        }
        let mut words = Vec::new();
        words
            .try_reserve_exact(total)
            .context("Cannot allocate welding storage")?;
        words.extend((0..total).map(|i| original.words.get(i).copied()));
        Ok(Self {
            offsets,
            words,
            counts: counts.to_vec(),
        })
    }
    fn index(&self, subpart: usize, triangle: usize) -> Result<usize> {
        ensure!(
            triangle
                < *self
                    .counts
                    .get(subpart)
                    .context("Invalid welding subpart")?,
            "Welding triangle outside subpart"
        );
        Ok(self.offsets[subpart] + triangle)
    }
    pub fn write(&mut self, subpart: usize, triangle: usize, word: u16) -> Result<()> {
        let index = self.index(subpart, triangle)?;
        self.words[index] = Some(word);
        Ok(())
    }
    pub fn word(&self, subpart: usize, triangle: usize) -> Result<u16> {
        self.words[self.index(subpart, triangle)?]
            .context("Uninitialized welding word for an unvisited triangle")
    }
}

/// Original 802879a4 initializes the word to zero, then 802879e8..87dc4
/// visits edges 0,1,2. Each callback accumulates ALL collections in source
/// order before resolving that edge. Bit 15 consequently remains clear.
pub fn generate_triangle(
    weld_open_edges: bool,
    mut collect: impl FnMut(usize, &mut EdgeIntervals) -> Result<()>,
) -> Result<u16> {
    let mut word = 0;
    for edge in 0..3 {
        let mut intervals = EdgeIntervals::new(weld_open_edges);
        collect(edge, &mut intervals)?;
        word = with_edge(word, edge, angle_code(intervals.resolve()?)?)?;
    }
    Ok(word)
}

fn cross4(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let y = -a[0].mul_add(b[2], -(b[0] * a[2]));
    [
        a[1].mul_add(b[2], -(b[1] * a[2])),
        y,
        -a[1].mul_add(b[0], -(b[1] * a[0])),
        y,
    ]
}
/// Retail mesh getFirstKey/getNextKey (8026fc98/8026fe20) call 8028b334
/// with squared-area threshold 806b90c0. Retain both cross-product checks
/// and the separate rounded Gram determinant: these are not interchangeable.
pub fn degenerate_triangle(v: [[f32; 4]; 3]) -> Result<bool> {
    ensure!(
        v.iter().flatten().all(|x| x.is_finite()),
        "Nonfinite mesh triangle"
    );
    let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn(|i| a[i] - b[i]);
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let ab = sub(v[0], v[1]);
    let ac = sub(v[0], v[2]);
    let ba = sub(v[1], v[0]);
    let bc = sub(v[1], v[2]);
    let n0 = cross4(ab, ac);
    let n1 = cross4(ba, bc);
    let threshold = f32::from_bits(0x33d6bf95);
    let a0 = dot(n0, n0);
    let a1 = dot(n1, n1);
    ensure!(
        a0.is_finite() && a1.is_finite(),
        "Mesh triangle area overflow"
    );
    if a0 < threshold || a1 < threshold {
        return Ok(true);
    }
    let cb = sub(v[2], v[1]);
    let d = dot(cb, ab);
    let determinant = dot(ab, ab) * dot(cb, cb) - d * d;
    ensure!(
        determinant.is_finite(),
        "Mesh triangle determinant overflow"
    );
    Ok(determinant == 0.)
}
/// 801664d4 uses XYZ length and scales all four lanes, including copied W.
fn normalize4(value: [f32; 4]) -> Result<[f32; 4]> {
    ensure!(
        value.iter().all(|v| v.is_finite()),
        "Nonfinite welding frame vector"
    );
    let squared = value[2].mul_add(value[2], value[0] * value[0]) + value[1] * value[1];
    ensure!(squared.is_finite(), "Welding frame length overflow");
    let scale = if squared == 0. {
        0.
    } else {
        super::native_hk_transform::inverse_root(squared)?
    };
    Ok(value.map(|v| v * scale))
}
/// DOL 80288630 uses consecutive edges, retaining the cross product's copied
/// Y lane in W. Transforming this normal is separate from recomputing it.
pub fn triangle_normal(vertices: [[f32; 4]; 3]) -> Result<[f32; 4]> {
    ensure!(
        vertices.iter().flatten().all(|v| v.is_finite()),
        "Nonfinite welding triangle"
    );
    normalize4(cross4(
        std::array::from_fn(|i| vertices[1][i] - vertices[0][i]),
        std::array::from_fn(|i| vertices[2][i] - vertices[1][i]),
    ))
}

/// One edge expressed in one candidate collection's coordinates. The original
/// source triangle stays separate for 8028658c; callers must supply the actual
/// transformed normal, rather than recomputing it after vertex transformation.
pub struct CandidateFrame {
    source: [[f32; 4]; 3],
    transformed: [[f32; 4]; 3],
    normal: [f32; 4],
    direction: [f32; 4],
    edge: usize,
}
impl CandidateFrame {
    /// Query one original MOPP collection and consume keys in returned order.
    /// Child resolution returns None only for non-triangle child shapes,
    /// matching the original type-3 gate; a missing key must be an error.
    pub fn accumulate_mopp(
        &self,
        code: &[u8],
        info: [f32; 4],
        mut triangle: impl FnMut(u32) -> Result<Option<[[f32; 4]; 3]>>,
        intervals: &mut EdgeIntervals,
    ) -> Result<usize> {
        let mut accepted = 0;
        for key in super::mopp_query::query(code, self.query_bounds(), info)? {
            if let Some(vertices) = triangle(key)? {
                if self.accumulate(vertices, intervals)? {
                    accepted += 1;
                }
            }
        }
        Ok(accepted)
    }
    /// Original collection frame conversion, 80287ad0..80287b24.
    pub fn in_collection(
        source: [[f32; 4]; 3],
        source_transform: &super::native_hk_transform::Transform,
        collection_transform: &super::native_hk_transform::Transform,
        edge: usize,
    ) -> Result<Self> {
        use super::native_hk_transform::{relative, rotate_vector, transform_point};
        ensure!(
            source_transform
                .iter()
                .chain(collection_transform)
                .all(|v| v.is_finite()),
            "Nonfinite welding collection transform"
        );
        let transform = relative(collection_transform, source_transform);
        let normal = rotate_vector(&transform, triangle_normal(source)?);
        Self::new(
            source,
            source.map(|v| transform_point(&transform, v)),
            normal,
            edge,
        )
    }
    /// 80287b54..70 builds the query from the two edge endpoints only.
    /// The later clipping margin must NOT expand this broadphase query.
    /// Keep all four lanes and first-operand ties from 80168814.
    pub fn query_bounds(&self) -> [[f32; 4]; 2] {
        let mut bounds = [
            [f32::from_bits(0x7f7fffee); 4],
            [f32::from_bits(0xff7fffee); 4],
        ];
        for point in [
            self.transformed[self.edge],
            self.transformed[(self.edge + 1) % 3],
        ] {
            for i in 0..4 {
                if !(point[i] - bounds[0][i] >= 0.) {
                    bounds[0][i] = point[i];
                }
                if !(bounds[1][i] - point[i] >= 0.) {
                    bounds[1][i] = point[i];
                }
            }
        }
        bounds
    }
    pub fn new(
        source: [[f32; 4]; 3],
        transformed: [[f32; 4]; 3],
        normal: [f32; 4],
        edge: usize,
    ) -> Result<Self> {
        ensure!(edge < 3, "Invalid welding edge");
        ensure!(
            source
                .iter()
                .chain(&transformed)
                .flatten()
                .chain(&normal)
                .all(|v| v.is_finite()),
            "Nonfinite welding source frame"
        );
        let direction = normalize4(std::array::from_fn(|i| {
            transformed[(edge + 1) % 3][i] - transformed[edge][i]
        }))?;
        Ok(Self {
            source,
            transformed,
            normal,
            direction,
            edge,
        })
    }
    /// 80287be4..80287d58: filter, build angle frame, construct planes, clip,
    /// then append the interval. Candidate enumeration/order belongs to caller.
    pub fn accumulate(
        &self,
        candidate: [[f32; 4]; 3],
        intervals: &mut EdgeIntervals,
    ) -> Result<bool> {
        if reject_candidate_geometry(self.source, self.transformed, self.edge, candidate)? {
            return Ok(false);
        }
        let normal = triangle_normal(candidate)?;
        if !candidate_normal_matches(self.direction, normal)? {
            return Ok(false);
        }
        let tangent = normalize4(cross4(self.direction, self.normal))?;
        let angle = signed_angle(self.normal, tangent, normal)?;
        let planes = triangle_planes(candidate, f32::from_bits(0x3d4ccccd))?;
        let Some([start, end]) = clip_edge(
            self.transformed[self.edge],
            self.transformed[(self.edge + 1) % 3],
            &planes,
        )?
        else {
            return Ok(false);
        };
        intervals.push(Interval { start, end, angle })?;
        Ok(true)
    }
}

/// DOL 802885f8: candidate projected onto tangent/reference-normal axes,
/// followed by Havok's float atan2 approximation (8014eba4).
/// The caller constructs and normalizes the frame before entering this helper.
pub fn signed_angle(
    reference_normal: [f32; 4],
    tangent: [f32; 4],
    candidate: [f32; 4],
) -> Result<f32> {
    ensure!(
        [reference_normal, tangent, candidate]
            .iter()
            .all(|v| v[..3].iter().all(|x| x.is_finite())),
        "Nonfinite welding angle frame"
    );
    let dot = |axis: [f32; 4]| {
        candidate[2].mul_add(axis[2], candidate[0] * axis[0]) + candidate[1] * axis[1]
    };
    super::native_angle::havok_atan2(dot(tangent), dot(reference_normal))
}

/// 8028658c compares corresponding vertices (not unordered triangle sets).
/// 80286890 tests independent membership of two points in a triangle, not
/// oriented adjacency. Generator 80287be4..7c74 rejects the source triangle
/// and candidates containing either of the source's OTHER two edges.
pub fn reject_candidate_geometry(
    source: [[f32; 4]; 3],
    transformed: [[f32; 4]; 3],
    edge: usize,
    candidate: [[f32; 4]; 3],
) -> Result<bool> {
    ensure!(edge < 3, "Invalid welding source edge");
    ensure!(
        source
            .iter()
            .chain(&transformed)
            .chain(&candidate)
            .all(|p| p[..3].iter().all(|v| v.is_finite())),
        "Nonfinite welding candidate position"
    );
    let same = |a: [f32; 4], b: [f32; 4]| {
        (0..3).all(|i| (a[i] - b[i]).abs() <= f32::from_bits(0x3a83126f))
    };
    if (0..3).all(|i| same(source[i], candidate[i])) {
        return Ok(true);
    }
    let contains = |point| candidate.iter().any(|&v| same(point, v));
    let opposite = transformed[(edge + 2) % 3];
    Ok(
        contains(opposite)
            && (contains(transformed[(edge + 1) % 3]) || contains(transformed[edge])),
    )
}

/// 80286e7c: normalized edge direction and candidate normal must have an
/// absolute native XYZ dot strictly BELOW 0.05. W is excluded, not normalized.
pub fn candidate_normal_matches(edge: [f32; 4], normal: [f32; 4]) -> Result<bool> {
    ensure!(
        edge[..3].iter().chain(&normal[..3]).all(|v| v.is_finite()),
        "Nonfinite welding candidate normal"
    );
    let dot = edge[2].mul_add(normal[2], edge[0] * normal[0]) + edge[1] * normal[1];
    ensure!(dot.is_finite(), "Welding candidate dot overflow");
    Ok(dot.abs() < f32::from_bits(0x3d4ccccd))
}

/// DOL 80286ec4: two face planes, then one outward plane per triangle edge.
/// The welding caller uses margin bits 3d4ccccd (0.05), not collision radius.
pub fn triangle_planes(vertices: [[f32; 4]; 3], margin: f32) -> Result<[[f32; 4]; 5]> {
    ensure!(
        vertices.iter().flatten().all(|v| v.is_finite()) && margin.is_finite(),
        "Nonfinite welding plane input"
    );
    let difference = |a: [f32; 4], b: [f32; 4]| std::array::from_fn::<_, 3, _>(|i| a[i] - b[i]);
    // Preserve paired msub then negate, rather than swapping subtraction terms.
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1].mul_add(b[2], -(b[1] * a[2])),
            -a[0].mul_add(b[2], -(b[0] * a[2])),
            -a[1].mul_add(b[0], -(b[1] * a[0])),
        ]
    };
    let normalize = |v: [f32; 3]| -> Result<[f32; 3]> {
        let squared = v[2].mul_add(v[2], v[0] * v[0]) + v[1] * v[1];
        ensure!(squared.is_finite(), "Welding normal length overflow");
        // Original zero-length refinement becomes unordered; fsel supplies
        // zero, then multiplies the original lanes (retaining signed zeros).
        let scale = if squared == 0. {
            0.
        } else {
            super::native_hk_transform::inverse_root(squared)?
        };
        Ok(v.map(|x| x * scale))
    };
    let edges = [
        difference(vertices[1], vertices[0]),
        difference(vertices[2], vertices[1]),
        difference(vertices[0], vertices[2]),
    ];
    let normal = normalize(cross(edges[0], edges[1]))?;
    let plane = |n: [f32; 3], point: [f32; 4]| {
        let distance = n[2].mul_add(point[2], n[0] * point[0]) + n[1] * point[1];
        [n[0], n[1], n[2], -margin - distance]
    };
    let result = [
        plane(normal, vertices[0]),
        plane(normal.map(|v| -v), vertices[0]),
        plane(normalize(cross(edges[0], normal))?, vertices[0]),
        plane(normalize(cross(edges[1], normal))?, vertices[1]),
        plane(normalize(cross(edges[2], normal))?, vertices[2]),
    ];
    ensure!(
        result.iter().flatten().all(|v| v.is_finite()),
        "Welding plane overflow"
    );
    Ok(result)
}

/// DOL 8028726c..80287494. Clip an edge against the caller's ordered planes
/// and project its surviving endpoints onto the ORIGINAL edge. The welding
/// caller supplies five planes; plane construction is a separate operation.
pub fn clip_edge(start: [f32; 4], end: [f32; 4], planes: &[[f32; 4]]) -> Result<Option<[f32; 2]>> {
    ensure!(
        start
            .iter()
            .chain(&end)
            .chain(planes.iter().flatten())
            .all(|v| v.is_finite()),
        "Nonfinite welding clipping input"
    );
    let dot = |a: [f32; 4], b: [f32; 4]| a[2].mul_add(b[2], a[0] * b[0]) + a[1] * b[1];
    let (mut first, mut last) = (start, end);
    for &plane in planes {
        let a = dot(plane, first) + plane[3];
        let b = dot(plane, last) + plane[3];
        ensure!(
            a.is_finite() && b.is_finite(),
            "Welding plane distance overflow"
        );
        // Points exactly on the plane are outside for this early rejection.
        if a >= 0. && b >= 0. {
            return Ok(None);
        }
        // Preserve the actual multiplication test: underflow to signed zero
        // does not enter the clipping branch, even with opposing signs.
        if a * b < 0. {
            let fraction = a / (a - b);
            let remaining = 1. - fraction;
            let clipped = std::array::from_fn(|i| fraction.mul_add(last[i], first[i] * remaining));
            ensure!(
                clipped.iter().all(|v| v.is_finite()),
                "Welding intersection overflow"
            );
            if a > 0. {
                first = clipped;
            } else {
                last = clipped;
            }
        }
    }
    let direction = std::array::from_fn(|i| end[i] - start[i]);
    let first = std::array::from_fn(|i| first[i] - start[i]);
    let last = std::array::from_fn(|i| last[i] - start[i]);
    let squared = dot(direction, direction);
    ensure!(
        squared > 0. && squared.is_finite(),
        "Degenerate welding edge projection"
    );
    let result = [
        dot(first, direction) / squared,
        dot(last, direction) / squared,
    ];
    ensure!(
        result.iter().all(|v| v.is_finite()),
        "Nonfinite welding edge projection"
    );
    Ok(Some(result))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub start: f32,
    pub end: f32,
    pub angle: f32,
}
/// Original edge accumulator (80287e30/68, 80287f7c). Candidate intervals
/// remain separate: summing their lengths is NOT geometric interval union.
pub struct EdgeIntervals {
    fallback: f32,
    entries: Vec<Interval>,
}
impl EdgeIntervals {
    pub fn new(weld_open_edges: bool) -> Self {
        Self {
            fallback: if weld_open_edges {
                0.
            } else {
                f32::from_bits(0x40490fdb)
            },
            entries: Vec::new(),
        }
    }
    pub fn push(&mut self, value: Interval) -> Result<()> {
        ensure!(
            [value.start, value.end, value.angle]
                .iter()
                .all(|v| v.is_finite()),
            "Nonfinite welding interval"
        );
        // Original search inserts after the first strictly smaller start;
        // otherwise appends. Do not replace with a conventional sorted insert.
        let index = self
            .entries
            .iter()
            .position(|old| value.start > old.start)
            .map(|i| i + 1)
            .unwrap_or(self.entries.len());
        self.entries.insert(index, value);
        Ok(())
    }
    pub fn resolve(&mut self) -> Result<f32> {
        if self.entries.is_empty() {
            return Ok(self.fallback);
        }
        sort_intervals(&mut self.entries);
        let mut first = 0;
        let mut chosen = f32::from_bits(0x40490fdb);
        let mut found = false;
        for next in 1..=self.entries.len() {
            if next < self.entries.len()
                && (self.entries[first].angle - self.entries[next].angle).abs()
                    < f32::from_bits(0x3a83126f)
            {
                continue;
            }
            let mut length = 0f32;
            for value in &self.entries[first..next] {
                length += value.end - value.start;
            }
            ensure!(length.is_finite(), "Welding interval sum overflow");
            // Native sequential sums, including its eight-element unroll,
            // clamp only after finishing a group, then require >= 0.6.
            if length.clamp(0., 1.) >= f32::from_bits(0x3f19999a) {
                let angle = self.entries[first].angle;
                if chosen >= angle {
                    chosen = angle;
                }
                found = true;
            }
            first = next;
        }
        Ok(if found { chosen } else { 0. })
    }
}

/// 80288384 midpoint-pivot partition, including swaps of equal angles.
/// Explicit range stack preserves left-before-right order without recursion.
fn sort_intervals(values: &mut [Interval]) {
    if values.len() < 2 {
        return;
    }
    let mut pending = vec![(0isize, values.len() as isize - 1)];
    while let Some((left, right)) = pending.pop() {
        let pivot = values[(left + (right - left) / 2) as usize].angle;
        let (mut i, mut j) = (left, right);
        loop {
            while values[i as usize].angle < pivot {
                i += 1;
            }
            while pivot < values[j as usize].angle {
                j -= 1;
            }
            if j >= i {
                if i != j {
                    values.swap(i as usize, j as usize);
                }
                j -= 1;
                i += 1;
            }
            if i > j {
                break;
            }
        }
        if i < right {
            pending.push((i, right));
        }
        if left < j {
            pending.push((left, j));
        }
    }
}

/// DOL 80288730. Thresholds are generated by one fmadds using original
/// constants, not host PI/division. Negative angles round up, positive angles
/// down, with exact threshold equality retained. Above the final threshold
/// returns 31; do not clamp this to the last ordinary angular bucket.
pub fn angle_code(angle: f32) -> Result<u8> {
    ensure!(angle.is_finite(), "Nonfinite welding angle");
    let origin = f32::from_bits(0xc0490fdb);
    let step = f32::from_bits(0x3e567750);
    for code in 0..31u8 {
        let threshold = step.mul_add(code as f32, origin);
        if angle <= threshold {
            return Ok(if angle > 0. && angle != threshold {
                code - 1
            } else {
                code
            });
        }
    }
    Ok(31)
}

/// DOL 80288704: replace one five-bit edge field, preserving other edges and
/// bit 15. The generator visits edges 0,1,2 in triangle vertex order.
pub fn with_edge(word: u16, edge: usize, code: u8) -> Result<u16> {
    ensure!(edge < 3 && code < 32, "Invalid welding edge/code");
    let shift = 5 * edge;
    Ok((word & !(0x1fu16 << shift)) | ((code as u16) << shift))
}
impl Welding {
    /// Original hkpExtendedMeshShape: array +cc, size +d0, mode +d8.
    /// Resolve its pointer only for a populated array, retaining authored bits.
    pub fn decode(
        data: &[u8],
        shape: usize,
        pointer: impl FnOnce(usize) -> Result<usize>,
    ) -> Result<Self> {
        let end = shape.checked_add(0xd9).context("Welding header overflow")?;
        let header = data
            .get(shape..end)
            .context("Truncated mesh welding header")?;
        let count = u32::from_be_bytes(header[0xd0..0xd4].try_into()?) as usize;
        let words = if count == 0 {
            Vec::new()
        } else {
            let start = pointer(shape + 0xcc)?;
            let end = start
                .checked_add(
                    count
                        .checked_mul(2)
                        .context("Welding array size overflow")?,
                )
                .context("Welding array offset overflow")?;
            data.get(start..end)
                .context("Truncated mesh welding words")?
                .chunks_exact(2)
                .map(|v| u16::from_be_bytes(v.try_into().unwrap()))
                .collect()
        };
        Ok(Self {
            mode: header[0xd8],
            words,
        })
    }
    /// REL 2430ec gate, when the root is MOPP and its child is extended mesh.
    /// Returning true requests real generation; it must not create zero data.
    pub fn needs_two_sided_generation(&self) -> bool {
        self.words.is_empty() || self.mode != 5
    }
    /// DOL 8026fa34..6c indexes subpart+4c plus the source triangle index.
    /// Empty storage remains absent until the real welding stage runs.
    pub fn subpart(&self, offset: usize, triangles: usize) -> Result<Option<&[u16]>> {
        if self.words.is_empty() {
            return Ok(None);
        }
        let end = offset
            .checked_add(triangles)
            .context("Welding subpart overflow")?;
        ensure!(
            end <= self.words.len(),
            "Welding subpart exceeds authored array"
        );
        Ok(Some(&self.words[offset..end]))
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn triangle_query_dispatches_welding_without_losing_contact_status() -> Result<()> {
        use crate::blotworx::native_gsk::{Cache, Features, TriangleSupport};
        let identity = crate::blotworx::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let vertices = [[0., 0., 0., 0.], [2., 0., 0., 0.], [0., 2., 0., 0.]];
        let table = ContactTable::retail_world()?;
        for mode in [0, 4, 5, 6] {
            let mut a = TriangleSupport {
                vertices: vertices.map(|mut p| {
                    p[2] = 2.;
                    p
                }),
                extrusion: [0.; 4],
                extrusion_flag: 0,
            };
            let mut b = TriangleSupport {
                vertices,
                extrusion: [0.; 4],
                extrusion_flag: 0,
            };
            let mut cache = Cache([0; 12]);
            cache.initialize_triangle(&mut a, vertices, &identity)?;
            let mut features = Features::load(&cache, &mut a, &mut b, &identity)?;
            let result = query_triangle_contact(
                &mut features,
                &mut a,
                &mut b,
                &identity,
                &identity,
                0.25,
                0.25,
                3.,
                false,
                &mut [[0.; 4]; 4],
                None,
                mode,
                0,
                &table,
            )?;
            assert!(!matches!(result, ClosestQuery::Unresolved));
            if mode == 6 {
                assert!(matches!(result, ClosestQuery::Contact(_)));
            }
            assert!(matches!(
                query_triangle_contact(
                    &mut features,
                    &mut a,
                    &mut b,
                    &identity,
                    &identity,
                    0.25,
                    0.25,
                    0.,
                    false,
                    &mut [[0.; 4]; 4],
                    None,
                    mode,
                    0,
                    &table
                )?,
                ClosestQuery::NoContact
            ));
        }
        Ok(())
    }

    #[test]
    fn welding_reduces_cache_in_place_without_recanonicalizing() -> Result<()> {
        use crate::blotworx::native_gsk::Cache;
        let transform = crate::blotworx::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let vertices = [[0., 0., 0., 0.], [1., 0., 0., 0.], [0., 1., 0., 0.]];
        let table = ContactTable::retail_world()?;
        // Raw B IDs [0,0,1] reduce to [0,1]; canonical sorting would reverse
        // them and lose the actual callback's write order.
        let mut cache = Cache([0, 7, 0, 0, 0, 0, 0, 1, 1, 3, 0xa5, 0x5a]);
        let mut normal = [0., 0., 1., 0.];
        weld_cached_triangle(
            vertices,
            &transform,
            &mut cache,
            5,
            0,
            &mut normal,
            &table,
            None,
        )?;
        assert_eq!(cache.0, [0, 7, 0, 0, 0, 1, 0, 1, 1, 2, 0xa5, 0x5a]);
        let mut invalid = Cache([0xff; 12]);
        let before = invalid.clone();
        assert_eq!(
            weld_cached_triangle(
                vertices,
                &transform,
                &mut invalid,
                6,
                0,
                &mut normal,
                &table,
                None
            )?,
            2
        );
        assert_eq!(invalid, before);
        Ok(())
    }

    #[test]
    fn cached_query_publishes_only_inside_enabled_welding_gate() -> Result<()> {
        use crate::blotworx::native_gsk::{Cache, TriangleSupport};
        let identity = crate::blotworx::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let vertices = [[0., 0., 0., 0.], [2., 0., 0., 0.], [0., 2., 0., 0.]];
        let mut a = TriangleSupport {
            vertices: vertices.map(|mut p| {
                p[2] = 2.;
                p
            }),
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut b = TriangleSupport {
            vertices,
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let table = ContactTable::retail_world()?;
        let mut initial = Cache([0; 12]);
        initial.initialize_convex(&mut a, &mut b, &identity)?;
        initial.update(&[0], &[1])?; // Valid, deliberately nonclosest source pair.
        for (enabled, tolerance, expected_contact) in
            [(false, 3., true), (true, 0., false), (true, 3., true)]
        {
            let mut cache = initial.clone();
            let result = query_cached_triangle_contact(
                &mut cache,
                &mut a,
                &mut b,
                &identity,
                &identity,
                0.25,
                0.25,
                tolerance,
                false,
                &mut [[0.; 4]; 4],
                None,
                enabled,
                6,
                0,
                &table,
            )?;
            assert_eq!(matches!(result, ClosestQuery::Contact(_)), expected_contact);
            if enabled && expected_contact {
                assert_ne!(cache, initial, "Dirty features were not published");
            } else {
                assert_eq!(cache, initial, "Skipped welding rewrote the cache");
            }
        }
        Ok(())
    }

    #[test]
    fn query_adapter_dispatches_both_one_sided_modes() -> Result<()> {
        let transform = crate::blotworx::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let vertices = [[0., 0., 0., 0.], [1., 0., 0., 0.], [0., 1., 0., 0.]];
        let mut features = crate::blotworx::native_gsk::Features {
            count_a: 1,
            count_b: 2,
            max_dimension_a: 15,
            max_dimension_b: 3,
            vertices_a: [[0.; 4]; 4],
            vertices_b: [[0.; 4]; 4],
            vertices_b_in_a: [[99.; 4]; 4],
        };
        features.vertices_b[1][3] = f32::from_bits(0x3f000001);
        let table = ContactTable {
            entries: [[0., 1., 0., 0., 0., -1.]; 32],
        };
        for mode in [0, 4] {
            let sign = if mode == 0 { -1. } else { 1. };
            let mut normal = [0., 0., sign, 7.];
            let mut calls = 0;
            let mut center = || {
                calls += 1;
                Ok([0.; 4])
            };
            let context = OneSidedQueryContext {
                contact_position: [0., -sign, 0., 0.],
                opposing_transform: &transform,
                opposing_center: &mut center,
                extruded: false,
            };
            assert_eq!(
                weld_query_triangle(
                    vertices,
                    &transform,
                    &features,
                    mode,
                    0,
                    &mut normal,
                    &table,
                    Some(context)
                )?,
                1
            );
            assert_eq!(calls, 1);
            assert_eq!(normal[1].is_sign_positive(), mode == 4);
        }
        Ok(())
    }

    #[test]
    fn joined_one_sided_contact_preserves_rejection_and_callback_order() -> Result<()> {
        let transform = crate::blotworx::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let vertices = [[0., 0., 0., 0.], [1., 0., 0., 0.], [0., 1., 0., 0.]];
        let table = ContactTable {
            entries: [[0., 1., 0., 0., 0., -1.]; 32],
        };
        for (position, expected) in [([0., 1., 0., 0.], 1), ([0., 0., 0., 0.], 0)] {
            let mut normal = [0., 0., -1., 7.];
            let original = normal;
            let mut calls = 0;
            let result = weld_one_sided_contact(
                vertices,
                &transform,
                &mut [0, 1, 0],
                &mut 2,
                0,
                &mut normal,
                &table,
                0,
                false,
                position,
                &transform,
                || {
                    calls += 1;
                    Ok([0.; 4])
                },
            )?;
            assert_eq!(result, expected);
            assert_eq!(calls, 1);
            if result == 0 {
                assert_eq!(normal, original);
            } else {
                assert!(normal[1] < 0.);
                assert!(normal[3] > 6.99);
            }
        }
        let mut normal = [0., 0., -1., 7.];
        assert_eq!(
            weld_one_sided_contact(
                vertices,
                &transform,
                &mut [0, 1, 0],
                &mut 2,
                0,
                &mut normal,
                &table,
                0,
                true,
                [0., 0., -2., 0.],
                &transform,
                || panic!("extrusion must skip center callback")
            )?,
            1
        );
        assert!(normal[3] < -1.99);
        Ok(())
    }

    #[test]
    fn extruded_welding_depth_preserves_frame_sign_and_previous_bound() -> Result<()> {
        let transform = crate::blotworx::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        // Local point2,3,4 -> world7,22,34; normal projects localX from vertexX1.
        let point = [7., 22., 34., 99.];
        let vertex = [1., 0., 0., 88.];
        let normal = [1., 0., 0., 77.];
        assert_eq!(
            one_sided_extrusion_depth(point, &transform, vertex, normal, 0, 2.)?,
            1.
        );
        assert_eq!(
            one_sided_extrusion_depth(point, &transform, vertex, normal, 4, 2.)?,
            -1.
        );
        assert_eq!(
            one_sided_extrusion_depth(point, &transform, vertex, normal, 0, 0.5)?,
            0.5
        );
        let zero = [0.; 4];
        let identity = crate::blotworx::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        assert_eq!(
            one_sided_extrusion_depth(zero, &identity, zero, [1., 0., 0., 0.], 0, -0.)?.to_bits(),
            (-0f32).to_bits()
        );
        Ok(())
    }

    #[test]
    fn one_sided_position_gate_uses_transformed_center_and_strict_sign() -> Result<()> {
        let transform = crate::blotworx::native_hk_transform::from_entity(
            [0., -1., 0., 1., 0., 0., 0., 0., 1.],
            [10., 20., 30.],
        );
        let center = [2., 3., 4., 123.]; // Rotated/transformed center is7,22,34.
        let normal = [0., 1., 0., 999.];
        assert!(one_sided_position_accepts(
            [7., 21., 34., -999.],
            center,
            &transform,
            normal
        )?);
        assert!(!one_sided_position_accepts(
            [7., 22., 34., -999.],
            center,
            &transform,
            normal
        )?);
        assert!(!one_sided_position_accepts(
            [7., 23., 34., -999.],
            center,
            &transform,
            normal
        )?);
        assert!(one_sided_position_accepts(
            [7., 23., 34., 0.],
            center,
            &transform,
            [0., -1., 0., 0.]
        )?);
        Ok(())
    }

    #[test]
    fn one_sided_candidate_preserves_strict_boundaries_and_orientation() -> Result<()> {
        let n = [0., 0., 1., 0.];
        let e = [1., 0., 0., 0.];
        let entry = [0., 1., 0., 0., 0., -1.];
        for mode in [0, 4] {
            assert_eq!(
                weld_one_sided_candidate(n, e, [1., 0., 0., 0.], entry, mode)?,
                ContactNormal::Accept
            );
            let sign = if mode == 0 { -1. } else { 1. };
            let ContactNormal::Snapped(candidate) =
                weld_one_sided_candidate(n, e, [0., 0., sign, 0.], entry, mode)?
            else {
                panic!("expected candidate")
            };
            assert_eq!(candidate[0], 0.);
            assert_eq!(candidate[2], 0.);
            assert_eq!(candidate[1].is_sign_positive(), mode == 4);
            assert_eq!(
                weld_one_sided_candidate(n, e, [0., 0., -sign, 0.], entry, mode)?,
                ContactNormal::Accept
            );
        }
        assert!(weld_one_sided_candidate(n, e, n, entry, 5).is_err());
        Ok(())
    }

    #[test]
    fn query_welding_uses_source_ids_and_native_disabled_bypass() -> Result<()> {
        let transform = crate::blotworx::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let vertices = [[0., 0., 0., 0.], [1., 0., 0., 0.], [0., 1., 0., 0.]];
        let mut features = crate::blotworx::native_gsk::Features {
            count_a: 1,
            count_b: 3,
            max_dimension_a: 15,
            max_dimension_b: 3,
            vertices_a: [[0.; 4]; 4],
            vertices_b: [[0.; 4]; 4],
            vertices_b_in_a: [[99.; 4]; 4],
        };
        for i in 0..3 {
            features.vertices_b[i][3] = f32::from_bits(0x3f000000 | i as u32);
        }
        let table = ContactTable::retail_world()?;
        let mut normal = [0., 0., 1., 7.];
        assert_eq!(
            weld_query_triangle(
                vertices,
                &transform,
                &features,
                5,
                0,
                &mut normal,
                &table,
                None
            )?,
            2
        );
        assert_eq!(normal, [0., 0., 1., 7.]);
        features.count_b = 0;
        assert_eq!(
            weld_query_triangle(
                [[f32::NAN; 4]; 3],
                &transform,
                &features,
                6,
                0,
                &mut normal,
                &table,
                None
            )?,
            2
        );
        assert!(weld_query_triangle(
            vertices,
            &transform,
            &features,
            5,
            0,
            &mut normal,
            &table,
            None
        )
        .is_err());
        assert!(weld_query_triangle(
            vertices,
            &transform,
            &features,
            0,
            0,
            &mut normal,
            &table,
            None
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn closest_query_connects_native_features_radii_and_welding() -> Result<()> {
        use crate::blotworx::native_gsk::{Cache, Features, TriangleSupport};
        let identity = crate::blotworx::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let triangle = [[0., 0., 0., 0.], [2., 0., 0., 0.], [0., 2., 0., 0.]];
        let mut a = TriangleSupport {
            vertices: triangle.map(|mut p| {
                p[2] = 2.;
                p
            }),
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut b = TriangleSupport {
            vertices: triangle,
            extrusion: [0.; 4],
            extrusion_flag: 0,
        };
        let mut cache = Cache([0; 12]);
        cache.initialize_convex(&mut a, &mut b, &identity)?;
        let mut features = Features::load(&cache, &mut a, &mut b, &identity)?;
        let mut called = false;
        let result = query_closest_contact(
            &mut features,
            &mut a,
            &mut b,
            &identity,
            &identity,
            0.25,
            0.25,
            3.,
            false,
            &mut [[0.; 4]; 4],
            None,
            |features, opposing, point, normal| {
                assert_eq!(opposing.center()?, [2. / 3., 2. / 3., 2., 0.]);
                called = true;
                assert!((1..=3).contains(&features.count_b));
                assert!(features.vertices_b[..features.count_b]
                    .iter()
                    .all(|p| p[3].to_bits() & 0xffff < 3));
                assert!(point.iter().chain(normal.iter()).all(|v| v.is_finite()));
                Ok(2)
            },
        )?;
        assert!(called);
        let ClosestQuery::Contact(contact) = result else {
            panic!("expected contact")
        };
        assert!(contact.separating[3] > 1.499 && contact.separating[3] <= 1.5);
        assert!(matches!(
            query_closest_contact(
                &mut features,
                &mut a,
                &mut b,
                &identity,
                &identity,
                0.25,
                0.25,
                0.,
                false,
                &mut [[0.; 4]; 4],
                None,
                |_, _, _, _| panic!("outside tolerance must skip welding")
            )?,
            ClosestQuery::NoContact
        ));
        features.count_a = 1;
        features.count_b = 1;
        features.vertices_a[0] = features.vertices_b_in_a[0];
        assert!(matches!(
            query_closest_contact(
                &mut features,
                &mut a,
                &mut b,
                &identity,
                &identity,
                0.25,
                0.25,
                3.,
                false,
                &mut [[0.; 4]; 4],
                None,
                |_, _, _, _| panic!("unresolved must skip welding")
            )?,
            ClosestQuery::Unresolved
        ));
        Ok(())
    }

    #[test]
    fn closest_contact_constructs_b_local_witness_and_publishes_world_position(
    ) -> anyhow::Result<()> {
        use super::super::native_hk_transform::from_entity;
        use super::closest_contact_from_core;
        let a = from_entity([1., 0., 0., 0., 1., 0., 0., 0., 1.], [0.; 3]);
        let b = from_entity([0., -1., 0., 1., 0., 0., 0., 0., 1.], [10., 0., 0.]);
        let contact = closest_contact_from_core(
            [0., 2., 0., 0.],
            [0., 1., 0., 0.],
            2.,
            &a,
            &b,
            0.25,
            0.5,
            2.,
            |point, normal| {
                assert_eq!(&point[..3], &[0.5, 10., 0.]);
                assert_eq!(*normal, [0., 1., 0., 0.]);
                Ok(2)
            },
        )?
        .unwrap();
        assert_eq!(contact.position, [0., 0.5, 0., 0.]);
        assert_eq!(contact.separating, [0., 1., 0., 1.25]);
        let changed = closest_contact_from_core(
            [0., 2., 0., 0.],
            [0., 1., 0., 0.],
            2.,
            &a,
            &b,
            0.25,
            0.5,
            2.,
            |point, normal| {
                *point = [2., 3., 4., 99.];
                *normal = [1., 0., 0., 123.];
                Ok(1)
            },
        )?
        .unwrap();
        assert_eq!(changed.position, [7., 2., 4., 0.]);
        assert_eq!(changed.separating, [1., 0., 0., 1.25]);
        assert!(closest_contact_from_core(
            [0., 2., 0., 0.],
            [0., 1., 0., 0.],
            2.,
            &a,
            &b,
            0.25,
            0.5,
            1.25,
            |_, _| panic!("Tolerance gate must precede welding")
        )?
        .is_none());
        assert!(closest_contact_from_core(
            [0., 2., 0., 0.],
            [0., 1., 0., 0.],
            2.,
            &a,
            &b,
            0.25,
            0.5,
            2.,
            |_, _| Ok(0)
        )?
        .is_none());
        Ok(())
    }
    #[test]
    fn closest_contact_keeps_surface_distance_separate_from_welding_lane() -> anyhow::Result<()> {
        use super::weld_closest_contact;
        let normal = [0., 1., 0., 17.];
        let snapped = weld_closest_contact(0.25, 0.5, 0.125, 0., normal, |n| {
            assert_eq!(*n, normal);
            *n = [0.6, 0.8, 0., 123.];
            Ok(1)
        })?
        .unwrap();
        assert_eq!(snapped, [0.6, 0.8, 0., -0.375]);
        assert_eq!(
            weld_closest_contact(0.25, 0.5, 0.125, 0., normal, |n| {
                *n = [9.; 4];
                Ok(2)
            })?,
            Some([0., 1., 0., -0.375])
        );
        assert!(weld_closest_contact(0.25, 0.5, 0.125, 0., normal, |_| Ok(0))?.is_none());
        assert!(
            weld_closest_contact(0.75, 0.5, 0.125, 0.125, normal, |_| panic!(
                "Tolerance equality must skip welding"
            ))?
            .is_none()
        );
        // Distinguish sequential f32 subtraction from a combined radius.
        let core = 16777216.;
        let contact = weld_closest_contact(core, core, 1., 0., normal, |_| Ok(2))?.unwrap();
        assert_eq!(contact[3], -1.);
        assert_eq!(core - (core + 1.), 0.);
        assert!(weld_closest_contact(0., -1., 0., 0., normal, |_| Ok(2)).is_err());
        Ok(())
    }
    #[test]
    fn two_sided_contact_connects_features_frames_and_distance() -> Result<()> {
        let vertices = [[0., 0., 0., 0.], [1., 0., 0., 0.], [0., 1., 0., 0.]];
        let transform = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [7., 8., 9.],
        );
        let table = ContactTable {
            entries: [[0.8, 0.6, 0.4, -0.4, -0.8, 0.6]; 32],
        };
        let mut separating = [0., -0.8, 0.6, 2.];
        assert_eq!(
            weld_two_sided_contact(
                vertices,
                &transform,
                &mut [0, 1, 99],
                &mut 2,
                0,
                &mut separating,
                &table
            )?,
            1
        );
        for (actual, expected) in separating.into_iter().zip([0., -0.6, 0.8, 1.2]) {
            assert!((actual - expected).abs() < 1e-6);
        }
        let rotated = [
            0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1., 0., 7., 8., 9., 1.,
        ];
        let mut separating = [0.8, 0., 0.6, 2.];
        assert_eq!(
            weld_two_sided_contact(
                vertices,
                &rotated,
                &mut [0, 1, 99],
                &mut 2,
                0,
                &mut separating,
                &table
            )?,
            1
        );
        for (actual, expected) in separating.into_iter().zip([0.6, 0., 0.8, 1.2]) {
            assert!((actual - expected).abs() < 1e-6);
        }
        let mut rejected = [0., 1., 0., 3.];
        let original = rejected;
        assert_eq!(
            weld_two_sided_contact(
                vertices,
                &transform,
                &mut [0, 1, 99],
                &mut 2,
                0,
                &mut rejected,
                &table
            )?,
            0
        );
        assert_eq!(rejected, original);
        let mut face = [0., -0.8, 0.6, 2.];
        let original = face;
        assert_eq!(
            weld_two_sided_contact(
                vertices,
                &transform,
                &mut [0, 1, 2],
                &mut 3,
                0,
                &mut face,
                &table
            )?,
            2
        );
        assert_eq!(face, original);
        Ok(())
    }
    #[test]
    fn contact_feature_reduction_retains_native_raw_ids_and_mutation_rules() -> Result<()> {
        for (pair, edge) in [
            ([0, 1], 0),
            ([1, 0], 0),
            ([1, 2], 1),
            ([2, 1], 1),
            ([2, 0], 2),
            ([0, 2], 2),
            ([3, 5], 2),
        ] {
            let mut ids = [pair[0], pair[1], 99];
            let original = ids;
            let mut count = 2;
            assert_eq!(reduce_contact_features(&mut ids, &mut count)?, Some(edge));
            assert_eq!(ids, original);
            assert_eq!(count, 2);
        }
        for id in 0..6 {
            let mut ids = [id, 99, 99];
            let mut count = 1;
            assert_eq!(
                reduce_contact_features(&mut ids, &mut count)?,
                Some(id as usize % 3)
            );
            assert_eq!(ids, [id, 99, 99]);
        }
        let mut ids = [2, 0, 1];
        let mut count = 3;
        assert_eq!(reduce_contact_features(&mut ids, &mut count)?, None);
        assert_eq!((ids, count), ([2, 0, 1], 3));
        let mut ids = [3, 0, 5];
        let mut count = 3;
        assert_eq!(reduce_contact_features(&mut ids, &mut count)?, Some(2));
        assert_eq!((ids, count), ([0, 2, 5], 2));
        let mut ids = [3, 4, 5];
        let mut count = 3;
        assert_eq!(reduce_contact_features(&mut ids, &mut count)?, Some(0));
        assert_eq!((ids, count), ([0, 1, 5], 2));
        assert!(reduce_contact_features(&mut [6, 0, 0], &mut 1).is_err());
        assert!(reduce_contact_features(&mut [0; 3], &mut 0).is_err());
        Ok(())
    }
    #[test]
    fn retail_contact_table_resolves_each_packed_edge_and_zero_bss_entry() -> Result<()> {
        let table = ContactTable::retail_world()?;
        let mut expected = [[0.; 6]; 32];
        initialize_contact_table_native(
            &mut expected,
            f32::from_bits(0x3f0624dd),
            f32::from_bits(0x3f32b021),
        )?;
        let word = with_edge(with_edge(with_edge(0x8000, 0, 7)?, 1, 20)?, 2, 31)?;
        assert_eq!(table.edge_entry(word, 0)?, expected[7]);
        assert_eq!(table.edge_entry(word, 1)?, expected[20]);
        assert_eq!(table.edge_entry(word, 2)?, [0.; 6]);
        assert!(table.edge_entry(word, 3).is_err());
        Ok(())
    }
    #[test]
    fn native_contact_table_uses_ported_trig_and_preserves_reserved_entry() -> Result<()> {
        let mut table = [[42.; 6]; 32];
        initialize_contact_table_native(&mut table, 0.2, 0.1)?;
        assert_eq!(table[0], [1., 0., 1., 1., 1., 0.]);
        assert_eq!(table[31], [42.; 6]);
        assert!(table[..31]
            .iter()
            .flatten()
            .all(|v| v.is_finite() && v.abs() <= 1.));
        assert_eq!(
            weld_two_sided_normal(
                [0., 1., 0., 0.],
                [0., 0., 1., 0.],
                [0., 1., 0., 0.],
                table[0],
                2
            )?,
            ContactNormal::Accept
        );
        Ok(())
    }
    #[test]
    fn contact_table_initialization_keeps_native_branch_inputs_and_unused_entry() -> Result<()> {
        let mut table = [[42.; 6]; 32];
        let mut calls = Vec::new();
        // Identity callback deliberately exposes arguments rather than claiming
        // host trigonometry is a native-reference result.
        initialize_contact_table(&mut table, 0.2, 0.1, |sine, angle| {
            calls.push((sine, angle));
            Ok(angle)
        })?;
        assert_eq!(table[0], [1., 0., 0., 0., 0., 0.]);
        assert_eq!(table[31], [42.; 6]);
        assert_eq!(calls.len(), 31 * 4);
        assert_eq!(
            &calls[..4],
            &[(false, 0.), (false, 0.), (false, 0.), (true, 0.)]
        );
        let angle = f32::from_bits(0x3e567750).mul_add(20., -f32::from_bits(0x40490fdb));
        assert_eq!(
            table[20],
            [
                angle,
                angle,
                angle + 0.2,
                f32::from_bits(0x40490fdb) - 0.1,
                -1.,
                0.
            ]
        );
        assert!(initialize_contact_table(&mut table, f32::NAN, 0., |_, _| Ok(0.)).is_err());
        assert!(initialize_contact_table(&mut table, 0., 0., |_, _| Ok(f32::NAN)).is_err());
        Ok(())
    }
    #[test]
    fn two_sided_contact_sectors_keep_boundaries_and_vertex_rejection() -> Result<()> {
        let normal = [0., 1., 0., 0.];
        let edge = [0., 0., 1., 0.];
        let entry = [0.8, 0.6, 0.4, -0.4, -0.8, 0.6];
        let classify =
            |y, count| weld_two_sided_normal(normal, edge, [-0.8, y, 0., 0.], entry, count);
        assert_eq!(classify(0.8, 2)?, ContactNormal::Accept);
        assert!(matches!(classify(0.4, 2)?, ContactNormal::Snapped(_)));
        assert_eq!(classify(-0.4, 2)?, ContactNormal::Reject);
        assert!(matches!(classify(-0.8, 2)?, ContactNormal::Snapped(_)));
        assert_eq!(classify(-0.9, 2)?, ContactNormal::Accept);
        assert_eq!(classify(0.6, 1)?, ContactNormal::Reject);
        assert_eq!(classify(-0.6, 1)?, ContactNormal::Reject);
        let ContactNormal::Snapped(value) = classify(0.6, 2)? else {
            panic!("Expected snapping");
        };
        assert!((value[0] + 0.6).abs() < 1e-6 && (value[1] - 0.8).abs() < 1e-6);
        assert_eq!(value[2], 0.);
        assert!(classify(0.6, 3).is_err());
        assert!(classify(f32::NAN, 2).is_err());
        Ok(())
    }
    #[test]
    fn regeneration_preserves_unvisited_storage_without_fabricating_words() -> Result<()> {
        let original = Welding {
            mode: 4,
            words: vec![0xbeef, 0xabcd, 0x1234],
        };
        let mut storage = Regenerated::new(&original, &[2, 2])?;
        assert_eq!(storage.offsets, vec![0, 2]);
        storage.write(0, 0, 0x0123)?;
        assert_eq!(storage.word(0, 0)?, 0x0123);
        assert_eq!(storage.word(0, 1)?, 0xabcd);
        assert_eq!(storage.word(1, 0)?, 0x1234);
        assert!(storage.word(1, 1).is_err());
        storage.write(1, 1, 0)?;
        assert_eq!(storage.word(1, 1)?, 0);
        assert!(storage.write(0, 2, 0).is_err());
        assert!(storage.word(2, 0).is_err());
        assert_eq!(original.words, vec![0xbeef, 0xabcd, 0x1234]);
        assert!(Regenerated::new(&original, &[usize::MAX, 1]).is_err());
        Ok(())
    }
    #[test]
    fn retail_mesh_degenerate_filter() -> Result<()> {
        let a = [0., 0., 0., 0.];
        let b = [1., 0., 0., 0.];
        assert!(degenerate_triangle([a, b, [2., 0., 0., 0.]])?);
        assert!(degenerate_triangle([a, a, b])?);
        assert!(degenerate_triangle([a, b, [0., 0.0001, 0., 0.]])?);
        assert!(!degenerate_triangle([a, b, [0., 0.001, 0., 0.]])?);
        assert!(!degenerate_triangle([a, [0., 1., 0., 0.], b])?);
        assert!(degenerate_triangle([[f32::NAN; 4], a, b]).is_err());
        Ok(())
    }
    use super::*;
    #[test]
    fn query_bounds_use_only_selected_edge_and_preserve_zero_ties() {
        let triangle = [
            [-0., 2., 3., 4.],
            [0., -2., 7., -4.],
            [100., 100., 100., 100.],
        ];
        let frame = CandidateFrame::new(triangle, triangle, [0., 0., 1., 0.], 0).unwrap();
        let bounds = frame.query_bounds();
        assert_eq!(bounds[0], [-0., -2., 3., -4.]);
        assert_eq!(bounds[1], [-0., 2., 7., 4.]);
        assert_eq!(bounds[0][0].to_bits(), (-0f32).to_bits());
        assert_eq!(bounds[1][0].to_bits(), (-0f32).to_bits());
        let last = CandidateFrame::new(triangle, triangle, [0., 0., 1., 0.], 2)
            .unwrap()
            .query_bounds();
        assert_eq!(last[0], [-0., 2., 3., 4.]);
        assert_eq!(last[1], [100.; 4]);
    }
    #[test]
    fn candidate_pipeline_connects_filtering_clipping_angles_and_edge_codes() {
        let source = [[0., 0., 0., 0.], [1., 0., 0., 0.], [0., 1., 0., 0.]];
        let normal = triangle_normal(source).unwrap();
        let frame = CandidateFrame::new(source, source, normal, 0).unwrap();
        let mut intervals = EdgeIntervals::new(true);
        assert!(!frame.accumulate(source, &mut intervals).unwrap());
        assert!(frame
            .accumulate([source[1], source[0], [0., -1., 0., 0.]], &mut intervals)
            .unwrap());
        assert_eq!(angle_code(intervals.resolve().unwrap()).unwrap(), 15);
        let mut convex = EdgeIntervals::new(true);
        assert!(frame
            .accumulate([source[0], source[1], [0., 0., 1., 0.]], &mut convex)
            .unwrap());
        assert_eq!(angle_code(convex.resolve().unwrap()).unwrap(), 22);
        let mut concave = EdgeIntervals::new(true);
        assert!(frame
            .accumulate([source[1], source[0], [0., 0., 1., 0.]], &mut concave)
            .unwrap());
        assert_eq!(angle_code(concave.resolve().unwrap()).unwrap(), 8);
        let before = concave.entries.clone();
        assert!(!frame
            .accumulate(
                source.map(|mut p| {
                    p[2] += 10.;
                    p
                }),
                &mut concave
            )
            .unwrap());
        assert_eq!(
            concave.entries, before,
            "Rejected candidates must not alter accepted intervals"
        );
    }
    #[test]
    fn signed_angle_uses_native_quadrants_and_float_polynomial() {
        let normal = [0., 0., 1., f32::NAN];
        let tangent = [1., 0., 0., 0.];
        assert_eq!(
            signed_angle(normal, tangent, [0., 0., 1., 0.])
                .unwrap()
                .to_bits(),
            0
        );
        assert_eq!(
            signed_angle(normal, tangent, [1., 0., 0., 0.])
                .unwrap()
                .to_bits(),
            0x3fc90fdb
        );
        assert_eq!(
            signed_angle(normal, tangent, [-1., 0., 0., 0.])
                .unwrap()
                .to_bits(),
            0xbfc90fdb
        );
        assert_eq!(
            signed_angle(normal, tangent, [0., 0., -1., 0.])
                .unwrap()
                .to_bits(),
            0x40490fdb
        );
        let angle = signed_angle(normal, tangent, [0.5, 0., 1., 0.]).unwrap();
        assert_ne!(
            angle.to_bits(),
            0.5f32.atan2(1.).to_bits(),
            "Do not replace original approximation with libm"
        );
        assert_eq!(
            signed_angle(normal, tangent, [-0.5, 0., 1., 0.]).unwrap(),
            -angle
        );
        assert_eq!(
            super::super::native_angle::havok_atan2(-0., -0.)
                .unwrap()
                .to_bits(),
            0
        );
        assert!(signed_angle(normal, tangent, [f32::INFINITY, 0., 0., 0.]).is_err());
    }
    #[test]
    fn candidate_rejection_uses_other_edges_xyz_tolerance_and_strict_normal_gate() {
        let source = [[0., 0., 0., 0.], [1., 0., 0., 0.], [0., 1., 0., 0.]];
        let shared_selected = [source[0], source[1], [0., -1., 0., 0.]];
        assert!(!reject_candidate_geometry(source, source, 0, shared_selected).unwrap());
        assert!(reject_candidate_geometry(
            source,
            source,
            0,
            [source[1], source[2], [2., 2., 0., 0.]]
        )
        .unwrap());
        assert!(reject_candidate_geometry(
            source,
            source,
            0,
            [source[2], source[0], [-2., 2., 0., 0.]]
        )
        .unwrap());
        let mut same = source;
        same[0][3] = f32::NAN;
        same[0][2] = f32::from_bits(0x3a83126f);
        let far = source.map(|mut p| {
            p[0] += 10.;
            p
        });
        assert!(
            reject_candidate_geometry(source, far, 0, same).unwrap(),
            "Exact inclusive XYZ threshold ignores W"
        );
        same[0][2] = f32::from_bits(0x3a831270);
        assert!(!reject_candidate_geometry(source, far, 0, same).unwrap());
        let gate = f32::from_bits(0x3d4ccccd);
        assert!(!candidate_normal_matches([1., 0., 0., f32::NAN], [gate, 0., 0., 1.]).unwrap());
        assert!(!candidate_normal_matches([1., 0., 0., 0.], [-gate, 0., 0., 0.]).unwrap());
        assert!(candidate_normal_matches(
            [1., 0., 0., 0.],
            [f32::from_bits(gate.to_bits() - 1), 0., 0., 0.]
        )
        .unwrap());
        assert!(candidate_normal_matches([f32::NAN; 4], [0.; 4]).is_err());
    }
    #[test]
    fn five_planes_keep_face_pair_edge_order_margin_and_winding() {
        let vertices = [[0., 0., 0., 7.], [2., 0., 0., 8.], [0., 2., 0., 9.]];
        let planes = triangle_planes(vertices, 0.05).unwrap();
        // Native reciprocal-root estimate plus one refinement is not exact
        // unit normalization, even for this axis-aligned triangle.
        let unit = f32::from_bits(0x3f7fffff);
        assert_eq!(planes[0], [0., -0., unit, -0.05]);
        assert_eq!(planes[1], [-0., 0., -unit, -0.05]);
        assert_eq!(planes[2], [0., -unit, -0., -0.05]);
        assert_eq!(planes[4], [-unit, -0., -0., -0.05]);
        assert!(planes[3][0] > 0. && planes[3][0] == planes[3][1]);
        assert_eq!(planes[3][3], -0.05 - planes[3][0] * 2.);
        assert_eq!(
            clip_edge([0.25, 0.25, -1., 0.], [0.25, 0.25, 1., 0.], &planes)
                .unwrap()
                .is_some(),
            true
        );
        assert!(clip_edge([3., 3., -1., 0.], [3., 3., 1., 0.], &planes)
            .unwrap()
            .is_none());
        let reversed = triangle_planes([vertices[0], vertices[2], vertices[1]], 0.05).unwrap();
        assert_eq!(reversed[0][2], -planes[0][2]);
        let flat = triangle_planes([[0.; 4]; 3], 0.05).unwrap();
        assert!(flat.iter().all(|p| p[..3] == [0.; 3] && p[3] == -0.05));
        assert!(triangle_planes(vertices, f32::NAN).is_err());
    }
    #[test]
    fn clipping_preserves_original_edge_parameters_and_plane_boundary_rules() {
        let planes = [[1., 0., 0., -1.], [-1., 0., 0., -1.]];
        // Second plane uses rounded 1/3 and 1-(1/3), then fused interpolation.
        // The resulting start projection is one ULP above the ideal quarter.
        assert_eq!(
            clip_edge([-2., 0., 0., 9.], [2., 0., 0., -9.], &planes).unwrap(),
            Some([f32::from_bits(0x3e800001), 0.75])
        );
        assert_eq!(
            clip_edge([2., 0., 0., 0.], [-2., 0., 0., 0.], &planes).unwrap(),
            Some([0.25, 0.75])
        );
        assert_eq!(
            clip_edge([-2., 0., 0., 0.], [-1.5, 0., 0., 0.], &planes).unwrap(),
            None
        );
        let plane = [[1., 0., 0., 0.]];
        assert_eq!(
            clip_edge([0., 0., 0., 0.], [0., 1., 0., 0.], &plane).unwrap(),
            None
        );
        assert_eq!(
            clip_edge([0., 0., 0., 0.], [-1., 0., 0., 0.], &plane).unwrap(),
            Some([0., 1.])
        );
        assert_eq!(
            clip_edge([1e-30, 0., 0., 0.], [-1e-30, 1., 0., 0.], &plane).unwrap(),
            Some([0., 1.]),
            "Signed-product underflow must not become a sign-bit clipping test"
        );
        assert!(clip_edge([-1., 0., 0., 0.], [-1., 0., 0., 0.], &plane).is_err());
        assert!(clip_edge([f32::NAN; 4], [0.; 4], &[]).is_err());
    }
    #[test]
    fn interval_resolution_uses_native_coverage_groups_and_empty_fallback() {
        assert_eq!(
            EdgeIntervals::new(false).resolve().unwrap().to_bits(),
            0x40490fdb
        );
        assert_eq!(EdgeIntervals::new(true).resolve().unwrap(), 0.);
        let mut edge = EdgeIntervals::new(false);
        edge.push(Interval {
            start: 0.,
            end: 0.4,
            angle: -1.,
        })
        .unwrap();
        assert_eq!(
            edge.resolve().unwrap(),
            0.,
            "Nonempty insufficient coverage ignores empty fallback"
        );
        edge.push(Interval {
            start: 0.,
            end: 0.4,
            angle: -1.,
        })
        .unwrap();
        assert_eq!(
            edge.resolve().unwrap(),
            -1.,
            "Overlapping lengths sum; do not replace by union"
        );
        edge.push(Interval {
            start: 0.,
            end: 0.6,
            angle: -2.,
        })
        .unwrap();
        assert_eq!(edge.resolve().unwrap(), -2.);
        let epsilon = f32::from_bits(0x3a83126f);
        let mut exact = EdgeIntervals::new(true);
        for angle in [-epsilon, 0.] {
            exact
                .push(Interval {
                    start: 0.,
                    end: 0.3,
                    angle,
                })
                .unwrap();
        }
        assert_eq!(exact.resolve().unwrap(), 0.);
        let mut near = EdgeIntervals::new(true);
        for angle in [-epsilon, -f32::from_bits(epsilon.to_bits() - 1)] {
            near.push(Interval {
                start: 0.,
                end: 0.3,
                angle,
            })
            .unwrap();
        }
        assert_eq!(near.resolve().unwrap(), -epsilon);
        assert!(near
            .push(Interval {
                start: 0.,
                end: f32::NAN,
                angle: 0.
            })
            .is_err());
        let mut entries = (0..4)
            .map(|i| Interval {
                start: i as f32,
                end: 1.,
                angle: 0.,
            })
            .collect::<Vec<_>>();
        sort_intervals(&mut entries);
        assert_eq!(
            entries.iter().map(|v| v.start).collect::<Vec<_>>(),
            [2., 3., 0., 1.],
            "Native equal-angle swaps affect later accumulation order"
        );
    }
    #[test]
    fn angle_codes_preserve_native_thresholds_and_directional_rounding() {
        for code in 0..31u8 {
            let threshold =
                f32::from_bits(0x3e567750).mul_add(code as f32, f32::from_bits(0xc0490fdb));
            assert_eq!(angle_code(threshold).unwrap(), code);
        }
        for (angle, code) in [
            (-4., 0),
            (-1., 11),
            (-0.1, 15),
            (-0., 15),
            (0., 15),
            (0.1, 15),
            (1., 19),
            (4., 31),
        ] {
            assert_eq!(angle_code(angle).unwrap(), code, "angle {angle}");
        }
        assert!(angle_code(f32::NAN).is_err());
        assert!(angle_code(f32::INFINITY).is_err());
        for edge in 0..3 {
            for code in 0..32 {
                let word = with_edge(0xa55a, edge, code).unwrap();
                assert_eq!((word >> (edge * 5)) & 31, code as u16);
                let untouched = !(31u16 << (edge * 5));
                assert_eq!(word & untouched, 0xa55a & untouched);
            }
        }
        assert!(with_edge(0, 3, 0).is_err());
        assert!(with_edge(0, 0, 32).is_err());
    }
    #[test]
    fn retained_words_offsets_and_generation_gate_match_original_storage() {
        let mut bytes = vec![0; 0x106];
        bytes[0xd8] = 5;
        let empty = Welding::decode(&bytes, 0, |_| {
            panic!("Empty array must not resolve pointer")
        })
        .unwrap();
        assert!(empty.needs_two_sided_generation());
        assert!(empty.subpart(99, 100).unwrap().is_none());
        bytes[0xd0..0xd4].copy_from_slice(&3u32.to_be_bytes());
        bytes[0x100..].copy_from_slice(&[0x12, 0x34, 0xab, 0xcd, 0xff, 0xfe]);
        let mut stored = Welding::decode(&bytes, 0, |at| {
            assert_eq!(at, 0xcc);
            Ok(0x100)
        })
        .unwrap();
        assert!(!stored.needs_two_sided_generation());
        assert_eq!(stored.subpart(1, 2).unwrap().unwrap(), [0xabcd, 0xfffe]);
        assert!(stored.subpart(2, 2).is_err());
        assert!(stored.subpart(usize::MAX, 1).is_err());
        stored.mode = 6;
        assert!(stored.needs_two_sided_generation());
        assert!(Welding::decode(&bytes[..0x105], 0, |_| Ok(0x100)).is_err());
    }
}
