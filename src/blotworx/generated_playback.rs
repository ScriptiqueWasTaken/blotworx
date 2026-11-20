//! Connect generated effects to their actual child synchronization clocks.
//! DOL801fa29c clip sync producer,8021e934 synchronization,8021ebdc update.
use super::{
    animation_resources::Resources,
    animation_sync::{Point, Sync},
    behavior::Document,
    generator_plan::Prepared,
    transition_effect::{Definition, Operation},
    transition_instances::{Handle, Playback},
};
use anyhow::{ensure, Context, Result};

impl Prepared {
    fn child_sync(
        &self,
        graph: &Document,
        id: &str,
        resources: &mut Resources,
        event_flags: &[u32],
    ) -> Result<Sync> {
        if let Some(authored) = self.state.clips.get(id) {
            let settings = authored.resolve(self.state.words())?;
            let resource = resources.clip_binding(&settings.animation_name,settings.animation_binding_index)?;
            let clock = &self.clips.get(id)?.clock;
            ensure!(
                settings.flags & 4 == 0,
                "Clip synchronization extrapolation is not implemented for {id}"
            );
            let span = (resource.info.duration - (clock.crop_start + clock.crop_end)).max(0.);
            let factor = if span > 0. && settings.enforced_duration > 0. {
                span / settings.enforced_duration
            } else {
                1.
            };
            let mut rate = factor * settings.playback_speed;
            if settings.mode == 3 && clock.ping_pong_backward {
                rate = -rate;
            }
            let mut output = Sync {
                points: vec![],
                frequency: 0.,
                time: 0.,
                field_48: 1.,
                mapped_events: false,
            };
            if rate == 0. {
                return Ok(output);
            }
            ensure!(span > 0., "Degenerate clip sync period for {id}");
            let mut period = span / rate.abs();
            let start = if settings.enforced_duration > 0. {
                settings.start_time / settings.enforced_duration * span
            } else {
                settings.start_time
            };
            let mut time = (clock.local_time - clock.crop_start - start.min(span)) / rate.abs();
            if time < 0. {
                time += period;
            }
            if settings.mode == 3 && settings.flags & 2 == 0 {
                period *= 2.;
            }
            if rate < 0. {
                time = period - time;
            }
            output.frequency = 1. / period;
            output.time = time.min(period);
            // Native synchronization storage has exactly eight marker slots.
            // Only events with synchronization flag bit1 enter this array.
            for trigger in &authored.triggers {
                if trigger.event.id < 0 {
                    continue;
                }
                let flags = *event_flags
                    .get(trigger.event.id as usize)
                    .context("Invalid clip sync event")?;
                if flags & 2 == 0 {
                    continue;
                }
                let time = if trigger.acyclic {
                    trigger.local_time
                } else {
                    let t = if trigger.relative_to_end_of_clip {
                        span + trigger.local_time
                    } else {
                        trigger.local_time
                    };
                    t.clamp(0., span)
                } / settings.playback_speed.abs();
                output.points.push(Point {
                    event: trigger.event.id,
                    time,
                });
                if output.points.len() == 8 {
                    break;
                }
            }
            return Ok(output);
        }
        let node = self
            .plan
            .nodes
            .iter()
            .find(|node| node.object == id && node.effect.is_none())
            .with_context(|| format!("Inactive synchronization child {id}"))?;
        match graph
            .objects
            .get(id)
            .context("Missing synchronization object")?
            .klass
            .as_str()
        {
            "hkbStateMachine" => {
                ensure!(
                    node.children.len() == 1,
                    "Invalid machine synchronization children"
                );
                let child = &self.plan.nodes[node.children[0]];
                ensure!(
                    child.effect.is_none(),
                    "Nested effect synchronization requires implementation"
                );
                self.child_sync(graph, &child.object, resources, event_flags)
            }
            "hkbBlenderGenerator" => {
                let object = &graph.objects[id];
                let flags = object.fields["flags"].as_u64().context("Missing blender flags")?;
                ensure!(flags & 1 == 0,
                    "Synchronizing blender child clocks are not implemented for {id}");
                let master = object.fields["indexOfSyncMasterChild"].as_i64()
                    .context("Missing blender sync master")?;
                ensure!(master >= -1, "Invalid blender sync master for {id}");
                // hkbBlenderGenerator::updateSync: without synchronization or an
                // authored master, output has no period or markers. A looping
                // leaf does not make its containing blender cyclic.
                if master == -1 {
                    return Ok(Sync { points: vec![], frequency: 0., time: 0.,
                        field_48: 1., mapped_events: false });
                }
                let children = object.fields["children"].as_array()
                    .context("Missing blender children")?;
                let reference = children.get(master as usize)
                    .context("Invalid blender sync master")?["ref"].as_str()
                    .context("Missing blender child reference")?;
                let child = graph.objects.get(reference).context("Missing blender child")?
                    .fields["generator"]["ref"].as_str().context("Missing blender generator")?;
                let mut output = self.child_sync(graph, child, resources, event_flags)?;
                // Native updateSync clears the copied event-map flag.
                output.mapped_events = false;
                Ok(output)
            }
            "hkbModifierGenerator" => {
                // cyclic_output also validates modifier synchronization support.
                self.cyclic_output(graph, id)?;
                let child = graph.objects[id].fields["generator"]["ref"]
                    .as_str()
                    .context("Missing modifier child")?;
                self.child_sync(graph, child, resources, event_flags)
            }
            class => anyhow::bail!(
                "Generated transition child synchronization for {class} is not implemented"
            ),
        }
    }
    fn generated_definition(
        &self,
        handle: Handle,
    ) -> Result<Option<(super::transition_effect::Generator, String)>> {
        let instance = self.transitions.get(handle)?;
        match &self.state.transition_effects[&instance.definition] {
            Definition::Generator(d) => Ok(Some((
                d.clone(),
                instance
                    .target
                    .clone()
                    .context("Missing generated target")?,
            ))),
            _ => Ok(None),
        }
    }
    fn generated_operations(
        &mut self,
        graph: &Document,
        d: &super::transition_effect::Generator,
        target: &str,
        resources: &mut Resources,
        operations: Vec<Operation>,
    ) -> Result<()> {
        for operation in operations {
            match operation {
                Operation::TransitionEcho(duration) => {
                    self.echo_subgraph(graph, &d.transition_generator.id, duration, resources)?
                }
                Operation::TargetSelfTransition => {
                    let mode = if d.self_transition_mode == 0 {
                        if self.cyclic_output(graph, target)? {
                            1
                        } else {
                            3
                        }
                    } else {
                        d.self_transition_mode
                    };
                    match mode {
                        1 => {}
                        2 => self.echo_subgraph(graph, target, 0., resources)?,
                        //Generated effect virtual+60 at8021f1f0 returns the
                        //authored blend-out field34, before effective scaling.
                        3 => self.echo_subgraph(graph, target, d.blend_out_duration, resources)?,
                        _ => anyhow::bail!(
                            "Generated target echo policy {mode} requires native echo duration"
                        ),
                    }
                }
                Operation::TargetSeek(time) => {
                    anyhow::bail!("Generated transition target seek {time} is not implemented")
                }
            }
        }
        Ok(())
    }
    pub(super) fn synchronize_generated(
        &mut self,
        graph: &Document,
        handle: Handle,
        resources: &mut Resources,
        event_flags: &[u32],
    ) -> Result<()> {
        let Some((definition, target)) = self.generated_definition(handle)? else {
            return Ok(());
        };
        let middle = self.child_sync(
            graph,
            &definition.transition_generator.id,
            resources,
            event_flags,
        )?;
        let target_sync = self.child_sync(graph, &target, resources, event_flags)?;
        let mut operations = vec![];
        let Some(Playback::Generated(playback)) = &mut self.transitions.get_mut(handle)?.playback
        else {
            anyhow::bail!("Missing generated playback");
        };
        playback.synchronize(&definition, &middle, &target_sync, |op| {
            operations.push(op);
            Ok(())
        })?;
        self.generated_operations(graph, &definition, &target, resources, operations)
    }
    pub(super) fn update_generated(
        &mut self,
        graph: &Document,
        handle: Handle,
        resources: &mut Resources,
        dt: f32,
    ) -> Result<bool> {
        let Some((definition, target)) = self.generated_definition(handle)? else {
            return Ok(false);
        };
        let mut operations = vec![];
        let Some(Playback::Generated(playback)) = &mut self.transitions.get_mut(handle)?.playback
        else {
            anyhow::bail!("Missing generated playback");
        };
        if playback.update(&definition, dt, |op| {
            operations.push(op);
            Ok(())
        })? {
            self.transitions.topology_changed = true;
        }
        self.generated_operations(graph, &definition, &target, resources, operations)?;
        Ok(true)
    }
}
