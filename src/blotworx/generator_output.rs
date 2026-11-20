//! Transform/attribute output composition for an already selected graph tree.
//! Updates, expression evaluation, events, float tracks and transitions are
//! separate phases. Composition keeps partial bone tracks until the root.
use super::{animation_attributes::Track, behavior::Document, generator_plan::Prepared};
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::ffi_anim::FfiTransform;

#[derive(Clone)]
pub struct Output {
    pub transforms: Vec<Option<FfiTransform>>,
    pub attributes: Track,
}
impl Output {
    /// The verified transform and dense attribute portions of retail output
    /// blending. Motion and float-slot output are not represented here yet.
    pub fn blend(&self, to: &Self, weight: f32) -> Result<Self> {
        Ok(Self {
            transforms: super::pose_blend::tracks(&self.transforms, &to.transforms, weight)?,
            attributes: super::animation_attributes::blend(
                &self.attributes,
                &to.attributes,
                weight,
            )?,
        })
    }
}

/// The caller must explicitly implement every modifier's output operation.
/// Ownership moves upward, avoiding skeleton copies at each wrapper node.
pub fn single_branch(
    graph: &Document,
    prepared: &Prepared,
    clip: &str,
    transforms: Vec<Option<FfiTransform>>,
    modify: impl FnMut(&str, &mut Output) -> Result<()>,
) -> Result<Output> {
    let mut tracks = Some(transforms);
    compose(
        graph,
        prepared,
        0.,
        |id| {
            ensure!(id == clip, "Unexpected clip in single-branch output: {id}");
            tracks
                .take()
                .context("Multiple clip outputs supplied to single-branch composition")
        },
        modify,
    )
}

/// Output phase only: clips have already been advanced by the graph scheduler.
/// `offset` is the original generate-time offset, not a second animation step.
pub fn compose(
    graph: &Document,
    prepared: &Prepared,
    offset: f32,
    mut clip: impl FnMut(&str) -> Result<Vec<Option<FfiTransform>>>,
    mut modify: impl FnMut(&str, &mut Output) -> Result<()>,
) -> Result<Output> {
    let nodes = &prepared.plan.nodes;
    ensure!(!nodes.is_empty(), "Empty generator output tree");
    ensure!(offset.is_finite(), "Invalid generator output time offset");
    let mut uses = vec![0usize; nodes.len()];
    for (parent, node) in nodes.iter().enumerate() {
        for &child in &node.children {
            ensure!(child < parent, "Invalid generator output child order");
            uses[child] += 1;
        }
    }
    let mut outputs: Vec<Option<Output>> = (0..nodes.len()).map(|_| None).collect();
    for (index, node) in nodes.iter().enumerate() {
        let object = graph
            .objects
            .get(&node.object)
            .context("Missing output generator")?;
        let mut inputs = Vec::new();
        for &i in &node.children {
            uses[i] -= 1;
            // Shared generators update/sample once. Copy output only while
            // another consumer still needs it, then transfer final ownership.
            let output = if uses[i] == 0 {
                outputs[i].take()
            } else {
                outputs[i].clone()
            };
            if let Some(output) = output {
                inputs.push((i, output));
            }
        }
        let output = if let Some(handle) = node.effect {
            transition(prepared, handle, offset, &mut inputs)?
        } else {
            match object.klass.as_str() {
                "hkbClipGenerator" => Output {
                    transforms: clip(&node.object)?,
                    attributes: Track {
                        values: prepared.state.attribute_defaults.clone(),
                        on_fraction: 1.,
                    },
                },
                "hkbModifierList"
                | "hkbEventDrivenModifier"
                | "hkbEvaluateExpressionModifier"
                | "hkbAttributeModifier"
                | "hkbTwistModifier"
                | "hkbAttachmentModifier" => continue,
                "hkbBlenderGenerator" => {
                    let children = prepared
                        .blenders
                        .get(&node.object)
                        .context("Missing blender runtime")?;
                    let definitions = object.fields["children"]
                        .as_array()
                        .context("Missing blender definitions")?;
                    ensure!(
                        definitions.len() == children.len(),
                        "Blender topology differs"
                    );
                    ensure!(
                        object.fields["flags"]
                            .as_u64()
                            .context("Missing blender flags")?
                            & 2
                            == 0
                            && object.fields["subtractLastChild"].as_bool() == Some(false),
                        "Additive blender output is not implemented: {}",
                        node.object
                    );
                    ensure!(
                        object.fields["referencePoseWeightThreshold"].as_f64() == Some(0.),
                        "Reference-pose threshold blending is not implemented: {}",
                        node.object
                    );
                    let mut weights = Vec::new();
                    for (definition, child) in
                        definitions.iter().zip(children).filter(|(_, c)| c.active)
                    {
                        let definition = &graph.objects[definition["ref"]
                            .as_str()
                            .context("Invalid blender child")?]
                        .fields;
                        ensure!(
                            definition["boneWeights"].is_null(),
                            "Masked blender output is not implemented: {}",
                            node.object
                        );
                        ensure!(
                            definition["worldFromModelWeight"].as_f64() == Some(1.),
                            "Weighted model motion is not implemented"
                        );
                        let index = weights.len();
                        let (node_index, _) =
                            inputs.get(index).context("Missing blended output")?;
                        ensure!(
                            definition["generator"]["ref"].as_str()
                                == Some(nodes[*node_index].object.as_str()),
                            "Blender contributor order differs"
                        );
                        weights.push(child.weight);
                    }
                    ensure!(
                        inputs.len() == weights.len() && !inputs.is_empty(),
                        "Invalid blender contributor count"
                    );
                    if weights.len() == 1 && weights[0] == 1. {
                        inputs.pop().unwrap().1
                    } else {
                        Output {
                            transforms: super::pose_blend::weighted_tracks(
                                &inputs
                                    .iter()
                                    .zip(&weights)
                                    .map(|((_, o), w)| (o.transforms.as_slice(), *w))
                                    .collect::<Vec<_>>(),
                            )?,
                            attributes: super::animation_attributes::weighted(
                                &inputs
                                    .iter()
                                    .zip(&weights)
                                    .map(|((_, o), w)| (&o.attributes, *w))
                                    .collect::<Vec<_>>(),
                            )?,
                        }
                    }
                }
                "hkbStateMachine" | "hkbModifierGenerator" => {
                    ensure!(
                        inputs.len() == 1,
                        "Generator {} requires one output, found {}",
                        node.object,
                        inputs.len()
                    );
                    let mut output = inputs.pop().unwrap().1;
                    if object.klass == "hkbModifierGenerator" {
                        if let Some(id) = object.fields["modifier"]["ref"].as_str() {
                            modify(id, &mut output)?;
                        }
                    }
                    output
                }
                class => anyhow::bail!("Generator output for {class} is not implemented"),
            }
        };
        outputs[index] = Some(output);
    }
    outputs
        .last_mut()
        .unwrap()
        .take()
        .context("Generator output did not reach root")
}

fn transition(
    prepared: &Prepared,
    handle: super::transition_instances::Handle,
    offset: f32,
    inputs: &mut Vec<(usize, Output)>,
) -> Result<Output> {
    use super::{
        transition_effect::Definition,
        transition_instances::{Binding, Playback},
        transition_timing::BlendPlan,
    };
    let instance = prepared.transitions.get(handle)?;
    let source = instance
        .source
        .as_ref()
        .context("Unbound effect output source")?;
    let target = Binding::Generator(
        instance
            .target
            .clone()
            .context("Unbound effect output target")?,
    );
    let mut take = |binding: &Binding| -> Result<Output> {
        let index = inputs
            .iter()
            .position(|(index, _)| {
                let node = &prepared.plan.nodes[*index];
                match binding {
                    Binding::Effect(h) => node.effect == Some(*h),
                    Binding::Generator(id) => node.effect.is_none() && node.object == *id,
                }
            })
            .context("Missing contributing transition child output")?;
        Ok(inputs.remove(index).1)
    };
    match (
        instance
            .playback
            .as_ref()
            .context("Inactive transition output")?,
        prepared
            .state
            .transition_effects
            .get(&instance.definition)
            .context("Missing transition output definition")?,
    ) {
        (Playback::Blend(p), Definition::Blend(d)) => {
            let to = take(&target)?;
            if source == &target {
                return Ok(to);
            }
            let from = take(source)?;
            p.clock.output(&from, &to, false, offset, d.blend_curve)
        }
        (Playback::Generated(p), Definition::Generator(d)) => {
            // Nonzero predictive offsets for generator effects still require
            // the original generate-time stage rules; do not guess a step.
            ensure!(
                offset == 0.,
                "Generator-transition predictive output is not implemented"
            );
            let middle = take(&Binding::Generator(d.transition_generator.id.clone()))?;
            match p.clock.blend_plan_at_stage(p.stage) {
                BlendPlan::FromToTransition(weight) => take(source)?.blend(&middle, weight),
                BlendPlan::TransitionOnly => Ok(middle),
                BlendPlan::TargetToTransition(weight) => take(&target)?.blend(&middle, weight),
            }
        }
        _ => anyhow::bail!("Transition output definition/playback mismatch"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn interrupted_return_to_idle_shares_and_samples_the_retained_branch_once() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut prepared = super::super::generator_plan::prepare(
            &graph,
            super::super::behavior_state::State::new(&graph)?,
            0,
        )?;
        prepared.plan.pending_generator_activation.clear();
        assert!(prepared.dispatch_machine_event("2:240", 0, None, |_, _| Ok(()))?);
        super::super::generator_plan::rebuild_in_place(&graph, &mut prepared)?;
        prepared.plan.pending_generator_activation.clear();
        let machine = &prepared.state.machines["2:240"];
        let stop = machine.states[machine.state_indices[&5]]
            .transitions
            .iter()
            .find(|t| t.event_id == 1 && t.to_state_id == 0)
            .context("Missing stop transition")?
            .clone();
        prepared.start_transition("2:240", &stop, false, |_, _| Ok(()))?;
        super::super::generator_plan::rebuild_in_place(&graph, &mut prepared)?;
        let idle = prepared
            .plan
            .nodes
            .iter()
            .position(|n| n.object == "2:1728")
            .unwrap();
        assert_eq!(
            prepared
                .plan
                .nodes
                .iter()
                .filter(|n| n.object == "2:1728")
                .count(),
            1
        );
        assert_eq!(
            prepared
                .plan
                .nodes
                .iter()
                .filter(|n| n.children.contains(&idle))
                .count(),
            2
        );
        let mut sampled = std::collections::BTreeMap::new();
        let output = compose(
            &graph,
            &prepared,
            0.,
            |id| {
                *sampled.entry(id.to_owned()).or_insert(0usize) += 1;
                Ok(vec![Some(FfiTransform {
                    translation: [1., 2., 3.],
                    rotation: [0., 0., 0., 1.],
                    scale: [1.; 3],
                })])
            },
            |_, _| Ok(()),
        )?;
        assert_eq!(sampled["2:6480"], 1);
        assert!(sampled.values().all(|count| *count == 1));
        assert_eq!(output.transforms[0].unwrap().translation, [1., 2., 3.]);
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn original_walk_tree_composes_both_branches_into_root_output() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let mut prepared = super::super::generator_plan::prepare(
            &graph,
            super::super::behavior_state::State::new(&graph)?,
            0,
        )?;
        prepared.plan.pending_generator_activation.clear();
        let machine = &prepared.state.machines["2:240"];
        let walk = machine.states[machine.state_indices[&0]]
            .transitions
            .iter()
            .find(|t| t.event_id == 0 && t.to_state_id == 5)
            .context("Missing walk transition")?
            .clone();
        prepared.start_transition("2:240", &walk, false, |_, _| Ok(()))?;
        let (mut prepared, _) = super::super::generator_plan::rebuild(&graph, prepared)?;
        let handle = prepared.transition_stacks["2:240"].records[0].effect;
        let effect = &prepared.state.transition_effects[&walk.transition.as_ref().unwrap().id];
        let (duration, curve) = match effect {
            super::super::transition_effect::Definition::Blend(d) => (d.duration, d.blend_curve),
            _ => anyhow::bail!("Expected original walk blending effect"),
        };
        for fraction in [0., 0.5, 1.] {
            let expected_weight =
                if let Some(super::super::transition_instances::Playback::Blend(p)) =
                    &mut prepared.transitions.get_mut(handle)?.playback
                {
                    p.clock.time = duration * fraction;
                    p.clock.remaining = duration - p.clock.time;
                    p.clock.weight(0., curve)?
                } else {
                    anyhow::bail!("Missing blending runtime");
                };
            let mut sampled = vec![];
            // Deliberately distinct synthetic poses exercise output routing;
            // this is not an original-game pose capture or twist parity test.
            let output = compose(
                &graph,
                &prepared,
                0.,
                |id| {
                    sampled.push(id.to_owned());
                    Ok(vec![Some(FfiTransform {
                        translation: [if id == "2:6480" { 0. } else { 10. }, 0., 0.],
                        rotation: [0., 0., 0., 1.],
                        scale: [1.; 3],
                    })])
                },
                |id, out| {
                    if let Some(modifier) = prepared.state.attribute_modifiers.get(id) {
                        if modifier.enabled {
                            modifier.modify(Some(&mut out.attributes.values), None)?;
                        }
                    }
                    Ok(())
                },
            )?;
            assert!(sampled.iter().any(|id| id == "2:6480"));
            assert!(sampled.iter().any(|id| id != "2:6480"));
            assert_eq!(
                output.transforms[0].unwrap().translation[0],
                10. * expected_weight
            );
            let expected = super::super::animation_attributes::blend(
                &Track {
                    values: prepared.state.attribute_defaults.clone(),
                    on_fraction: 1.,
                },
                &Track {
                    values: vec![2., 5., 0.5],
                    on_fraction: 1.,
                },
                expected_weight,
            )?;
            assert_eq!(output.attributes, expected);
        }
        assert!(compose(
            &graph,
            &prepared,
            0.,
            |_| anyhow::bail!("missing animation"),
            |_, _| Ok(())
        )
        .is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn authored_startup_tree_routes_output_and_requires_modifier_dispatch() -> Result<()> {
        let graph = Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let prepared = super::super::generator_plan::prepare(
            &graph,
            super::super::behavior_state::State::new(&graph)?,
            0,
        )?;
        let mut visited = vec![];
        let output = single_branch(&graph, &prepared, "2:6480", vec![None; 83], |id, output| {
            visited.push(id.to_owned());
            output.attributes.values[0] = 2.;
            Ok(())
        })?;
        assert_eq!(visited, ["2:6240"]);
        assert_eq!(output.transforms.len(), 83);
        assert_eq!(output.attributes.values, [2., 6.43, 0.5]);
        assert!(
            single_branch(&graph, &prepared, "2:6480", vec![], |_, _| anyhow::bail!(
                "unsupported modifier"
            ))
            .is_err()
        );
        assert!(single_branch(&graph, &prepared, "wrong-clip", vec![], |_, _| Ok(())).is_err());
        Ok(())
    }
}
