//! Blender child weights and activation flags, DOL 0x801f63b4..0x801f677c.
//! Pose blending and synchronization-master selection are separate operations.
use anyhow::{ensure, Context, Result};

/// Resolve current bound values during the active-node update pass, not only
/// when topology is built. A changed parameter need not generate an event.
pub fn update_authored(
    graph: &super::behavior::Document,
    state: &super::behavior_state::State,
    id: &str,
    children: &mut Vec<Child>,
) -> Result<bool> {
    let object = graph.objects.get(id).context("Missing active blender")?;
    ensure!(
        object.klass == "hkbBlenderGenerator",
        "Invalid active blender class"
    );
    let fields = &object.fields;
    let weights = fields["children"]
        .as_array()
        .context("Missing blend children")?
        .iter()
        .map(|child| {
            super::generator_plan::bound_float(
                graph,
                state,
                child["ref"].as_str().context("Invalid blend child")?,
                "weight",
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let master = fields["indexOfSyncMasterChild"]
        .as_i64()
        .context("Missing sync master")?;
    ensure!(master >= -1, "Invalid sync master");
    update(
        &Parameters {
            children: &weights,
            parameter: super::generator_plan::bound_float(graph, state, id, "blendParameter")?,
            cyclic_min: fields["minCyclicBlendParameter"]
                .as_f64()
                .context("Missing cyclic minimum")? as f32,
            cyclic_max: fields["maxCyclicBlendParameter"]
                .as_f64()
                .context("Missing cyclic maximum")? as f32,
            flags: fields["flags"]
                .as_u64()
                .context("Missing blender flags")?
                .try_into()?,
            sync_master: (master >= 0).then_some(master as usize),
        },
        false,
        children,
    )
}

pub struct Parameters<'a> {
    /// Weights in direct mode, authored parameter thresholds in parametric mode.
    pub children: &'a [f32],
    pub parameter: f32,
    pub cyclic_min: f32,
    pub cyclic_max: f32,
    pub flags: u16,
    pub sync_master: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Child {
    pub weight: f32,
    pub active: bool,
    pub newly_active: bool,
    pub synchronize: bool,
}

/// Caller-owned storage preserves child activation history without per-frame
/// allocation. Returns the graph-change flag produced by the original update.
pub fn update(
    parameters: &Parameters<'_>,
    activating: bool,
    children: &mut Vec<Child>,
) -> Result<bool> {
    let p = parameters;
    ensure!(
        !p.children.is_empty() && p.children.iter().all(|w| w.is_finite()),
        "Invalid blender children"
    );
    ensure!(
        p.parameter.is_finite() && p.flags & !63 == 0,
        "Invalid blender parameters or flags"
    );
    ensure!(
        p.sync_master.is_none_or(|i| i < p.children.len()),
        "Invalid blender sync master"
    );
    if activating && children.is_empty() {
        children.resize(p.children.len(), Child::default());
    }
    ensure!(
        children.len() == p.children.len(),
        "Blender child topology changed"
    );
    // Native EM2 activate() builds a stable threshold-index permutation.
    // Keep child identity/activation history in authored order.
    let order = if p.flags & 16 != 0 && !p.children.windows(2).all(|w| w[0] <= w[1]) {
        let mut indices = (0..p.children.len()).collect::<Vec<_>>();
        indices.sort_by(|&a, &b| p.children[a].partial_cmp(&p.children[b]).unwrap());
        Some(indices)
    } else {
        None
    };
    let index = |i: usize| order.as_ref().map_or(i, |order| order[i]);
    let threshold = |i: usize| p.children[index(i)];
    let mut upper = 0;
    let mut fraction = 0.;
    if p.flags & 16 != 0 {
        let cyclic = p.flags & 32 != 0;
        let first = threshold(0);
        let last = threshold(p.children.len() - 1);
        let (min, max) = if cyclic {
            ensure!(
                p.cyclic_min.is_finite() && p.cyclic_max.is_finite() && p.cyclic_min < p.cyclic_max,
                "Invalid cyclic blend bounds"
            );
            (p.cyclic_min, p.cyclic_max)
        } else {
            (first, last)
        };
        let parameter = if p.parameter <= min {
            min
        } else if p.parameter >= max {
            if cyclic {
                min
            } else {
                max
            }
        } else {
            p.parameter
        };
        upper = (0..p.children.len())
            .position(|i| parameter < threshold(i))
            .unwrap_or(p.children.len());
        fraction = if upper == 0 && cyclic {
            1. - (first - parameter) / ((first - min) + (max - last))
        } else if upper >= p.children.len() {
            if cyclic {
                upper = 0;
                (parameter - last) / ((first - min) + (max - last))
            } else {
                upper = p.children.len() - 1;
                1.
            }
        } else {
            ensure!(upper > 0, "Invalid parametric blend interval");
            (parameter - threshold(upper - 1)) / (threshold(upper) - threshold(upper - 1))
        };
        ensure!(fraction.is_finite(), "Degenerate parametric blend interval");
    }
    let mut changed = false;
    for (i, child) in children.iter_mut().enumerate() {
        child.weight = if p.flags & 16 != 0 {
            if i == index(upper) {
                fraction
            } else if i
                == index(if upper == 0 {
                    p.children.len() - 1
                } else {
                    upper - 1
                })
            {
                1. - fraction
            } else {
                0.
            }
        } else {
            let weight = p.children[i];
            if p.flags & 4 != 0 {
                (-2_f32).mul_add(weight, 3.) * (weight * weight)
            } else {
                weight
            }
        };
        if p.flags & 8 != 0 {
            child.active = true;
        } else if p.sync_master == Some(i) {
            child.active = true;
            child.newly_active = false;
        } else {
            let was_active = child.active;
            child.active = child.weight != 0.;
            child.newly_active = !activating && !was_active && child.active;
            changed |= was_active != child.active;
        }
        child.synchronize = p.flags & 1 != 0 || p.sync_master == Some(i);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parametric_boundaries_activate_only_contributing_children() -> Result<()> {
        let mut p = Parameters {
            children: &[-45., 0., 45.],
            parameter: 0.,
            cyclic_min: 0.,
            cyclic_max: 1.,
            flags: 17,
            sync_master: None,
        };
        let mut children = vec![];
        assert!(update(&p, true, &mut children)?);
        assert_eq!(
            children
                .iter()
                .map(|c| (c.weight, c.active, c.newly_active))
                .collect::<Vec<_>>(),
            [(0., false, false), (1., true, false), (0., false, false)]
        );
        p.parameter = 22.5;
        assert!(update(&p, false, &mut children)?);
        assert_eq!(
            children.iter().map(|c| c.weight).collect::<Vec<_>>(),
            [0., 0.5, 0.5]
        );
        assert!(children[2].newly_active);
        assert!(children.iter().all(|c| c.synchronize));
        assert!(!update(&p, false, &mut children)?);
        assert!(!children[2].newly_active);
        p.parameter = 100.;
        update(&p, false, &mut children)?;
        assert_eq!(
            children.iter().map(|c| c.weight).collect::<Vec<_>>(),
            [0., 0., 1.]
        );
        Ok(())
    }
    #[test]
    fn cyclic_boundary_and_forced_children_keep_original_selection_rules() -> Result<()> {
        let mut p = Parameters {
            children: &[-90., 0., 90., 180.],
            parameter: -135.,
            cyclic_min: -180.,
            cyclic_max: 180.,
            flags: 49,
            sync_master: None,
        };
        let mut children = vec![];
        update(&p, true, &mut children)?;
        assert_eq!(
            children.iter().map(|c| c.weight).collect::<Vec<_>>(),
            [0.5, 0., 0., 0.5]
        );
        p.parameter = 180.;
        update(&p, false, &mut children)?;
        assert_eq!(
            children.iter().map(|c| c.weight).collect::<Vec<_>>(),
            [0., 0., 0., 1.]
        );
        p.flags |= 8;
        update(&p, false, &mut children)?;
        assert!(children.iter().all(|c| c.active));
        p.flags = 16;
        p.sync_master = Some(0);
        update(&p, true, &mut children)?;
        assert!(children[0].active && children[0].weight == 0. && children[0].synchronize);
        Ok(())
    }
}

#[cfg(test)]
mod threshold_order_tests {
    use super::*;
    #[test]
    fn unsorted_thresholds_keep_authored_child_and_sync_identity() -> Result<()> {
        let mut children = Vec::new();
        let mut parameters = Parameters {
            children: &[1., 0.],
            parameter: 0.25,
            cyclic_min: 0.,
            cyclic_max: 1.,
            flags: 16,
            sync_master: Some(0),
        };
        update(&parameters, true, &mut children)?;
        assert_eq!(
            children.iter().map(|c| c.weight).collect::<Vec<_>>(),
            [0.25, 0.75]
        );
        assert!(children[0].synchronize);
        assert!(!children[1].synchronize);
        parameters.parameter = 0.;
        update(&parameters, false, &mut children)?;
        assert_eq!(
            children.iter().map(|c| c.weight).collect::<Vec<_>>(),
            [0., 1.]
        );
        assert!(children[0].active); // explicit sync master, even at zero weight
        Ok(())
    }
}
