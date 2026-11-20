//! Automatic spawner budget REL3cb4b0. No catch-up loop or total-batch clamp.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Settings {
    pub number: i32,
    pub max_live: i32,
    pub total: i32,
    pub delay: f32,
    pub start_disabled: bool,
}
impl Settings {
    pub fn load(component: &Component) -> Result<Self> {
        let value = |name| -> Result<Option<&serde_json::Value>> {
            let mut fields = component
                .properties
                .iter()
                .filter(|field| field.name == name);
            let value = fields.next().map(|field| &field.value);
            ensure!(fields.next().is_none(), "Duplicate spawner property {name}");
            Ok(value)
        };
        // Native property-value constructors13b0bc/13b0d8/13b0f0 default to zero.
        let integer = |name| -> Result<i32> {
            value(name)?
                .map(|v| {
                    v.as_i64()
                        .context("Invalid spawner integer")
                        .and_then(|v| i32::try_from(v).map_err(Into::into))
                })
                .transpose()
                .map(Option::unwrap_or_default)
        };
        let delay = value("Spawn Delay")?
            .map(|v| v.as_f64().context("Invalid Spawn Delay"))
            .transpose()?
            .unwrap_or(0.) as f32;
        ensure!(delay.is_finite(), "Nonfinite Spawn Delay");
        Ok(Self {
            number: integer("Number To Spawn")?,
            max_live: integer("Max Live Spawns")?,
            total: integer("Total Spawns")?,
            delay,
            start_disabled: value("TSCStartDisabled")?
                .map(|v| v.as_bool().context("Invalid TSCStartDisabled"))
                .transpose()?
                .unwrap_or(false),
        })
    }
}
fn constructor_disabled(settings: Settings, components: &[Component]) -> Result<bool> {
    let shooter_disabled = components
        .iter()
        .find(|component| component.class_name == "ShooterComponent")
        .and_then(|component| {
            component
                .properties
                .iter()
                .find(|property| property.name == "Shooter Start Disabled")
        })
        .map(|property| {
            property
                .value
                .as_bool()
                .context("Invalid Shooter Start Disabled")
        })
        .transpose()?
        .unwrap_or(false);
    Ok(settings.start_disabled || shooter_disabled)
}
#[derive(Default, Debug, serde::Serialize)]
pub struct State {
    pub countdown: f32,
    pub pending: i32,
    pub blocked: Option<String>,
    pub retirement_pending: bool,
    pub updates: u64,
}
pub struct Budget {
    pub number: i32,
    pub all_gone: bool,
    pub completed: bool,
}
impl State {
    pub fn budget(
        &mut self,
        delta: f32,
        settings: Settings,
        attempts: u32,
        live: &mut Vec<usize>,
        mut alive: impl FnMut(usize) -> bool,
    ) -> Result<Budget> {
        ensure!(
            delta.is_finite() && self.countdown.is_finite(),
            "Invalid spawner clock"
        );
        if delta <= 0. {
            return Ok(Budget {
                number: 0,
                all_gone: false,
                completed: false,
            });
        }
        self.updates = self.updates.wrapping_add(1);
        let mut number = 0;
        if self.countdown <= 0. {
            number = settings.number;
            self.countdown = settings.delay;
        } else {
            self.countdown -= delta;
        }
        let previous = live.len();
        let mut index = live.len();
        while index > 0 {
            index -= 1;
            if !alive(live[index]) {
                live.swap_remove(index);
            }
        }
        let all_gone = previous > 0 && live.is_empty();
        let completed = all_gone && attempts as i32 == settings.total;
        if number > 0 && settings.total != 0 && attempts as i32 >= settings.total {
            number = 0;
        }
        let count = i32::try_from(live.len()).context("Native live-spawn count overflow")?;
        if number > 0 && count > 0 && settings.max_live != 0 {
            if count >= settings.max_live {
                number = 0;
            } else if number.wrapping_add(count) > settings.max_live {
                number = settings.max_live - count;
            }
        }
        Ok(Budget {
            number,
            all_gone,
            completed,
        })
    }
}
impl super::assets::World {
    pub(super) fn initialize_spawners(&mut self, range: std::ops::Range<usize>) -> Result<()> {
        let mut templates = std::collections::BTreeSet::new();
        for owner in range {
            let components = &self.entities[owner].components;
            let mut found = components
                .iter()
                .enumerate()
                .filter(|(_, component)| component.class_name == "TemplateSpawnerComponent");
            let Some((index, component)) = found.next() else {
                continue;
            };
            ensure!(
                found.next().is_none(),
                "Multiple spawner components on {}",
                self.entities[owner].name
            );
            let settings = Settings::load(component)?;
            if let Some(objects) = component
                .properties
                .iter()
                .find(|p| p.name == "Objects To Spawn")
                .and_then(|p| p.value.as_array())
            {
                for id in objects
                    .iter()
                    .filter_map(|v| v.as_u64())
                    .filter(|id| *id != 0)
                {
                    if let Some(index) = self.entities.iter().position(|e| {
                        e.scene == self.entities[owner].scene && u64::from(e.id) == id
                    }) {
                        if index != owner {
                            templates.insert(index);
                        }
                    }
                }
            }
            let disabled = constructor_disabled(settings, components)?;
            let state = self.spawners.entry(owner).or_default();
            // Both authored constructor switches are present in EM1 data:
            // TSCStartDisabled gates the spawner and Shooter Start Disabled
            // gates its shooter. Retain the combined initial state for the
            // scheduler instead of treating enabled shooter-prefabs as
            // uninitialized and permanently blocking their first update.
            state.disabled = Some(disabled);
            self.component_control
                .disabled
                .entry(owner)
                .or_default()
                .insert(index, disabled);
        }
        // Objects To Spawn remain available as factory inputs. Scene loading
        // keeps them hidden and defers OnSpawned until clone construction.
        for entity in templates {
            self.spawner_sources.insert(entity);
            self.set_entity_hidden(entity, true)?;
            // Source objects remain available to the factory, but their
            // gameplay aliases must resolve to live spawned instances. In GV,
            // the original unfreeze callback uses the source's inherited alias.
            self.scripts.suppress_source_groups(entity)?;
        }
        Ok(())
    }
    pub(super) fn advance_spawners(&mut self, delta: f32) -> Result<()> {
        ensure!(delta.is_finite(), "Nonfinite spawner update interval");
        if delta <= 0. {
            return Ok(());
        }
        let owners = self
            .spawners
            .keys()
            .copied()
            .filter(|&owner| self.entity_scene_active(owner))
            .collect::<Vec<_>>();
        for owner in owners {
            if self.removed_entities.contains(&owner)
                || self.spawners[&owner].disabled == Some(true)
                || self.spawners[&owner].automatic.blocked.is_some()
            {
                continue;
            }
            let result = (|| -> Result<()> {
                ensure!(
                    self.spawners[&owner].disabled.is_some(),
                    "Spawner constructor enablement is unavailable"
                );
                let (index, component) = self.entities[owner]
                    .components
                    .iter()
                    .enumerate()
                    .find(|(_, component)| component.class_name == "TemplateSpawnerComponent")
                    .context("Spawner component disappeared")?;
                let settings = Settings::load(component)?;
                let budget = {
                    let state = self.spawners.get_mut(&owner).unwrap();
                    let removed = &self.removed_entities;
                    let length = self.entities.len();
                    // REL3cb638..3cb660 clears a slot when its stale live
                    // handle is removed, even if another clone shares it.
                    let occupied = &mut state.occupied_destinations;
                    state.destinations.retain(|entity, slot| {
                        if *entity < length && !removed.contains(entity) {
                            true
                        } else {
                            *occupied &= !super::spawn_selection::destination_bit(*slot);
                            false
                        }
                    });
                    state.automatic.budget(
                        delta,
                        settings,
                        state.attempts,
                        &mut state.live,
                        |id| id < length && !removed.contains(&id),
                    )?
                };
                if budget.all_gone {
                    self.queue_spawner_status(0x1a, owner, owner)?;
                }
                if budget.completed {
                    self.queue_spawner_status(0x1b, owner, owner)?;
                    let state = self.spawners.get_mut(&owner).unwrap();
                    state.automatic.retirement_pending = true;
                    state.disabled = Some(true);
                    self.component_control
                        .disabled
                        .entry(owner)
                        .or_default()
                        .insert(index, true);
                    if self.demo_story.enabled {
                        // Same deferred gameplay bridge as DestroyEntity. Keep
                        // the owner alive through this update; its queued
                        // command is committed on the next script pass.
                        // The shared event pump admits completion IO before
                        // scheduling retirement; otherwise owner teardown would
                        // unregister its listeners before the status is read.
                        let limitation="Completed spawners queue deferred gameplay retirement. Native completion callbacks, component readiness/event44 and notification dispatch phase parity remain incomplete.";
                        if !self.report.unsupported.iter().any(|entry|entry==limitation) {self.report.unsupported.push(limitation.into());}
                        return Ok(());
                    }
                    anyhow::bail!(
                        "Completed spawner requires deferred owner destruction/component lifecycle"
                    );
                }
                self.spawners.get_mut(&owner).unwrap().automatic.pending = budget.number.max(0);
                while self.spawners[&owner].automatic.pending > 0 {
                    if self.spawners[&owner].disabled == Some(true) {
                        self.spawners.get_mut(&owner).unwrap().automatic.pending = 0;
                        break;
                    }
                    self.force_spawn(owner, 1)?;
                    self.spawners.get_mut(&owner).unwrap().automatic.pending -= 1;
                }
                Ok(())
            })();
            if let Err(error) = result {
                let message = format!(
                    "Automatic spawner {} blocked: {error:#}",
                    self.entities[owner].name
                );
                self.spawners.get_mut(&owner).unwrap().automatic.blocked = Some(message.clone());
                self.report.unsupported.push(message);
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        Settings {
            number: 3,
            max_live: 0,
            total: 0,
            delay: 1.,
            start_disabled: false,
        }
    }
    #[test]
    fn due_batch_resets_delay_and_crossing_zero_waits_for_next_update() -> Result<()> {
        let mut state = State::default();
        let mut live = vec![];
        assert_eq!(
            state.budget(0., settings(), 0, &mut live, |_| true)?.number,
            0
        );
        assert_eq!(
            state
                .budget(0.25, settings(), 0, &mut live, |_| true)?
                .number,
            3
        );
        assert_eq!(state.countdown, 1.);
        assert_eq!(
            state
                .budget(1.25, settings(), 0, &mut live, |_| true)?
                .number,
            0
        );
        assert_eq!(state.countdown, -0.25);
        assert_eq!(
            state.budget(4., settings(), 0, &mut live, |_| true)?.number,
            3
        );
        assert_eq!(state.countdown, 1.);
        Ok(())
    }
    #[test]
    fn pruning_precedes_limits_and_total_does_not_clamp_a_due_batch() -> Result<()> {
        let mut state = State::default();
        let mut live = vec![1, 2, 3, 4];
        let s = Settings {
            max_live: 3,
            total: 2,
            ..settings()
        };
        let b = state.budget(0.1, s, 1, &mut live, |id| id == 2)?;
        assert_eq!(live, [2]);
        assert_eq!(b.number, 2); // Live cap only; one remaining total is not a cap.
        state.countdown = 0.;
        let b = state.budget(0.1, s, 2, &mut live, |_| false)?;
        assert!(b.all_gone && b.completed);
        assert_eq!(b.number, 0);
        Ok(())
    }
    #[test]
    fn shooter_constructor_flags_initialize_the_automatic_spawner_gate() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::ID;
        use epic_mickey_lib_rs::scene_file::{Component, Property};
        let property = |name: &str, value| {
            Property::new("Boolean".into(), name.into(), false, false, false, value)
        };
        let component = |name: &str, property: Property| {
            Component::new(
                name.into(),
                name.into(),
                ID::new(0),
                ID::new(0),
                ID::new(0),
                vec![property],
            )
        };
        let mut spawner = component(
            "TemplateSpawnerComponent",
            property("TSCStartDisabled", serde_json::Value::Bool(false)),
        );
        let mut shooter = component(
            "ShooterComponent",
            property("Shooter Start Disabled", serde_json::Value::Bool(false)),
        );
        let settings = Settings::load(&spawner)?;
        assert!(!constructor_disabled(
            settings,
            &[spawner.clone(), shooter.clone()]
        )?);
        shooter.properties[0].value = serde_json::Value::Bool(true);
        assert!(constructor_disabled(settings, &[spawner.clone(), shooter])?);
        spawner.properties[0].value = serde_json::Value::Bool(true);
        assert!(constructor_disabled(Settings::load(&spawner)?, &[spawner])?);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_projector_automatic_task_spawns_prunes_and_shares_the_event_queue() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .entities
            .iter()
            .position(|entity| entity.name.ends_with(".SpawnerPortalWarpFX"))
            .context("Projector spawner missing")?;
        // Isolate one actual registered component task, not a full-level parity run.
        world.spawners.retain(|id, _| *id == owner);
        let name = world.entities[owner].name.clone();
        world.run_callback_fixture(&format!("local s=GetEntity({name:?}); SetPropertyInt(s,'Number To Spawn',1); SetPropertyInt(s,'Max Live Spawns',1); SetPropertyInt(s,'Total Spawns',0); SetPropertyInt(s,'Index To Spawn',0); SetPropertyFloat(s,'Spawn Delay',1); Enable(s)"))?;
        let before = world.entities.len();
        world.advance_spawners(0.)?;
        assert_eq!(world.entities.len(), before);
        world.advance_spawners(1. / 30.)?;
        ensure!(
            world.spawners[&owner].automatic.blocked.is_none(),
            "{:?}",
            world.spawners[&owner].automatic.blocked
        );
        assert_eq!(world.entities.len(), before + 1);
        assert_eq!(world.spawners[&owner].attempts, 1);
        let child = world.spawners[&owner].live[0];
        world.run_callback_fixture(&format!("local s=GetEntity({name:?}); assert(GetPropertyFloat(s,'Spawn Delay')==1); SetPropertyFloat(s,'Spawn Delay',2); SetPropertyFloat(s,'Spawn Delay',99,1); assert(GetPropertyFloat(s,'Spawn Delay')==2); assert(select('#',GetPropertyFloat(s,'Spawn Delay',1))==0)"))?;
        assert_eq!(
            world.spawners[&owner].automatic.countdown, 1.,
            "Changing delay restarted a running timer"
        );
        world.advance_spawners(1.)?;
        assert_eq!(world.spawners[&owner].automatic.countdown, 0.);
        world.advance_spawners(1. / 30.)?;
        assert_eq!(
            world.spawners[&owner].attempts, 1,
            "Live cap did not suppress a due batch"
        );
        assert_eq!(
            world.spawners[&owner].automatic.countdown, 2.,
            "Due task did not read the updated delay"
        );
        world.retire_entity(child)?;
        world.advance_spawners(0.25)?;
        assert!(world.spawners[&owner].live.is_empty());
        let events = world
            .entity_event_queue
            .entries()
            .iter()
            .filter_map(|entry| match &entry.event {
                super::super::entity_events::Event::Basic(event) if event.source == Some(owner) => {
                    Some((event.event_kind, event.header))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(events, [(0x18, Some(child)), (0x1a, Some(owner))]);
        Ok(())
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate_completed_spawner()->Result<()> {
    use super::action_event_queue::QueuedEvent;
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world=super::assets::load(std::path::Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
    world.enable_demo_story()?;
    world.attach_player_body()?;
    let mut player=world.take_startup_player();
    let owner=world.entities.iter().position(|entity|entity.name.ends_with(".SpawnerPortalWarpFX")).context("Original projector spawner missing")?;
    world.spawners.retain(|entity,_|*entity==owner);
    // Controlled authored IO rows use the same decoder and dispatcher as
    // original scene connections. No game files or prefab definitions change.
    use epic_mickey_lib_rs::scene_file::{ID,Property};
    let rows=[("OnSpawnerSpawned","ProbeSpawned",0.),("OnSpawnerAllSpawned","ProbeExhausted",0.),("OnSpawnerAllDead","ProbeGone",0.),("OnSpawnerAllDeadForever","ProbeComplete",0.1)];
    let fields=[
        ("Output",serde_json::json!(rows.iter().map(|row|row.0).collect::<Vec<_>>())),
        ("Input",serde_json::json!(rows.iter().map(|row|row.1).collect::<Vec<_>>())),
        ("Target",serde_json::json!(vec!["_self";4])),("Parameters",serde_json::json!(vec!["";4])),
        ("DelayWait",serde_json::json!(rows.iter().map(|row|row.2).collect::<Vec<_>>())),
        ("IoFlags",serde_json::json!(vec![0;4])),("TargetGroup",serde_json::json!(vec!["";4])),
    ];
    let io=Component::new("InputOutputComponent".into(),"Probe".into(),ID::new(0),ID::new(0),ID::new(0),fields.into_iter().map(|(name,value)|Property::new(String::new(),name.into(),false,false,false,value)).collect());
    world.retire_deferred_output_events(owner);
    world.outputs.remove_owner(owner);
    world.outputs.bind(owner,&world.entities[owner].name,&[io])?;
    world.bind_deferred_output_events(owner)?;
    world.run_callback_fixture("SpawnerCalls={0,0,0,0}; function ProbeSpawned(...) SpawnerCalls[1]=SpawnerCalls[1]+1 end; function ProbeExhausted(...) SpawnerCalls[2]=SpawnerCalls[2]+1 end; function ProbeGone(...) SpawnerCalls[3]=SpawnerCalls[3]+1 end; function ProbeComplete(...) SpawnerCalls[4]=SpawnerCalls[4]+1 end")?;
    let name=world.entities[owner].name.clone();
    world.run_callback_fixture(&format!("local s=GetEntity({name:?}); SetPropertyInt(s,'Number To Spawn',0); SetPropertyInt(s,'Total Spawns',1); SetPropertyInt(s,'Max Live Spawns',1); SetPropertyInt(s,'Index To Spawn',0); Enable(s)"))?;
    let state=world.spawners.get_mut(&owner).unwrap();state.attempts=0;state.live.clear();
    let children=world.force_spawn(owner,1)?;
    let child=children.first().copied().flatten().context("ForceSpawn returned no child")?;
    ensure!(world.spawners[&owner].attempts==1,"Fixture did not reach its authored spawn budget");
    world.scripts.schedule_retirement(child)?;
    world.advance_player_models(&mut player,super::STEP)?;
    ensure!(world.removed_entities.contains(&child),"Child retirement was not committed");
    ensure!(!world.removed_entities.contains(&owner),"Completed owner was retired synchronously");
    let state=&world.spawners[&owner];
    ensure!(state.automatic.retirement_pending&&state.disabled==Some(true)&&state.automatic.blocked.is_none(),"Completed spawner blocked instead of queuing retirement");
    for kind in [0x1a,0x1b] {
        ensure!(world.entity_event_queue.entries().iter().filter(|entry|entry.event.kind()==kind).count()==1,"Completion status missing or duplicated");
    }
    world.advance_player_models(&mut player,2.*super::STEP)?;
    ensure!(!world.removed_entities.contains(&owner),"Owner retired before its delayed completion IO");
    for tick in 3..=10 {world.advance_player_models(&mut player,tick as f32*super::STEP)?;}
    world.run_callback_fixture("for i=1,4 do assert(SpawnerCalls[i]==1,'Spawner IO missing or duplicated') end")?;
    ensure!(world.deferred_event_runtime.spawner_io_deliveries>=4,"Native spawner IO receiver count differs");
    ensure!(world.removed_entities.contains(&owner)&&!world.spawners.contains_key(&owner),"Deferred completed owner remained live");
    println!("PASS: original projector spawner routes four deferred notifications through authored IO once, delivers delayed completion before retirement, and removes its owner");
    Ok(())
}
