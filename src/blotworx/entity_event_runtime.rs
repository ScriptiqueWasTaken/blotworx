//! Live deferred pump using the recovered queue/ordinary dispatch protocols.
//! Only constructed receivers run; absent subscriptions are reported explicitly.
use super::{action_event_queue::{Host as QueueHost,QueuedEvent,Entry},action_event_dispatch::{EventRegistry,Host as EventHost},entity_events::{Event,Queue}};
use anyhow::Result;
#[derive(Default,serde::Serialize)]
pub struct Runtime {pub ordinary_processed:u64,pub reached_position_deliveries:u64,pub passes:u64,pub processed:u64,pub unbound_kinds:std::collections::BTreeMap<u16,u64>,#[serde(skip)] io_listeners:std::collections::BTreeMap<u32,usize>,pub spawner_io_deliveries:u64,pub relay_io_deliveries:u64,pub use_io_deliveries:u64,pub spline_io_deliveries:u64,pub spawn_io_deliveries:u64}
impl Runtime {
    /// Ordinary REL1f4110 output-event delivery. This does not enqueue an event
    /// or run the deferred pump; IO rows retain their own authored delays.
    pub(super) fn dispatch_ordinary_output(
        &mut self, registry: &mut EventRegistry<Event>, event: super::entity_events::Basic,
        outputs: &mut super::actor_io::Outputs, entities: &[super::assets::RuntimeEntity],
        resources: &super::scene_resources::State, time: f32,
        brains:&mut std::collections::BTreeMap<usize,super::ai_behavior_events::Brain>,
        arms:&std::collections::BTreeMap<usize,super::robot_arm::Runtime>,actions:&super::cutscene_actions::State,
    ) -> Result<()> {
        struct OutputReceivers<'a> {
            listeners: &'a std::collections::BTreeMap<u32,usize>,
            outputs: &'a mut super::actor_io::Outputs,
            entities: &'a [super::assets::RuntimeEntity],
            resources: &'a super::scene_resources::State,
            time: f32, reached: &'a mut u64,
            brains:&'a mut std::collections::BTreeMap<usize,super::ai_behavior_events::Brain>,
            arms:&'a std::collections::BTreeMap<usize,super::robot_arm::Runtime>,actions:&'a super::cutscene_actions::State,
        }
        impl EventHost<Event> for OutputReceivers<'_> {
            fn receive(&mut self, _: &mut EventRegistry<Event>, listener: u32, event: &Event) -> Result<()> {
                use anyhow::Context as _;
                if !self.listeners.contains_key(&listener) {
                    return receive_arm_position(self.actions,listener,event,self.brains,self.arms);
                }
                let owner = *self.listeners.get(&listener).context("Ordinary output event has an unimplemented receiver")?;
                let Event::Basic(event) = event else {anyhow::bail!("Ordinary output requires a basic event")};
                let name = deferred_output(event.event_kind).context("Unmapped ordinary output event")?;
                if self.entities.get(owner).is_some_and(|e| self.resources.is_active(&e.scene)) {
                    self.outputs.emit_optional(owner, name, event.header, self.time);
                    if event.event_kind == 0x0d {*self.reached = self.reached.wrapping_add(1);}
                }
                Ok(())
            }
        }
        registry.dispatch(&Event::Basic(event), None, &mut OutputReceivers {
            listeners: &self.io_listeners, outputs, entities, resources, time,
            reached: &mut self.reached_position_deliveries,brains,arms,actions,
        })?;
        self.ordinary_processed = self.ordinary_processed.wrapping_add(1);
        Ok(())
    }
}
struct Receivers<'a> {
    actions:&'a mut super::cutscene_actions::State,
    brains:&'a mut std::collections::BTreeMap<usize,super::ai_behavior_events::Brain>,
    arms:&'a std::collections::BTreeMap<usize,super::robot_arm::Runtime>,
    camera:super::camera_feedback_world::Bindings<'a>,
    io_listeners:&'a std::collections::BTreeMap<u32,usize>,
    outputs:&'a mut super::actor_io::Outputs,
    entities:&'a [super::assets::RuntimeEntity],
    removed:&'a std::collections::HashSet<usize>,
    resources:&'a super::scene_resources::State,
    time:f32,
    io_deliveries:&'a mut u64,
    relay_deliveries:&'a mut u64,
    use_deliveries:&'a mut u64,
    spline_deliveries:&'a mut u64,
    spawn_deliveries:&'a mut u64,
}
impl Receivers<'_> {
    fn receive_with_patrol(&mut self,registry:&mut EventRegistry<Event>,listener:u32,event:&Event,
        notify:&mut dyn FnMut(super::entity_events::Basic)->Result<()>)->Result<()> {
        if let Some(&owner)=self.io_listeners.get(&listener) {
            if let Event::Basic(event)=event {
                if self.entities.get(owner).is_some_and(|entity|matches!(event.event_kind, 0x3d | 0x3e) || self.resources.is_active(&entity.scene)) {
                    if let Some(name)=deferred_output(event.event_kind) {
                        self.outputs.emit_optional(owner,name,event.header,self.time);
                        if matches!(event.event_kind, 0x3d | 0x3e) {*self.spawn_deliveries=self.spawn_deliveries.wrapping_add(1);} else if matches!(event.event_kind,0x1c..=0x1e) {*self.spline_deliveries=self.spline_deliveries.wrapping_add(1);} else if event.event_kind==0x0e {*self.use_deliveries=self.use_deliveries.wrapping_add(1);} else if event.event_kind==0x4e {*self.relay_deliveries=self.relay_deliveries.wrapping_add(1);} else {*self.io_deliveries=self.io_deliveries.wrapping_add(1);}
                    }
                }
            }
            return Ok(());
        }
        if event.kind()==0x0d {return receive_arm_position(self.actions,listener,event,self.brains,self.arms);}
        if event.kind()==9 && listener==super::camera_feedback_world::LISTENER {self.camera.receive(registry,listener,event)}
        else {
            let entities=self.entities;
            let removed=self.removed;
            self.actions.receive_deferred_with_patrol(listener,event,self.brains,&mut |reference| {
                if removed.contains(&reference) {return Ok(None);}
                Ok(entities.get(reference).map(|entity|super::native_hk_transform::entity_position_vector(
                    entity.transform[12..15].try_into().unwrap()).map(f32::to_bits)))
            },notify)
        }
    }
}
struct QueuedReceivers<'a,'b> {inner:&'a mut Receivers<'b>,queue:&'a mut Queue}
impl EventHost<Event> for QueuedReceivers<'_,'_> {
    fn receive(&mut self,registry:&mut EventRegistry<Event>,listener:u32,event:&Event)->Result<()> {
        let queue=&mut self.queue;
        self.inner.receive_with_patrol(registry,listener,event,&mut |event| {
            queue.enqueue(&Event::Basic(event),None)?;Ok(())
        })
    }
}
struct Pump<'a> {registry:&'a mut EventRegistry<Event>,receivers:Receivers<'a>,unbound:&'a mut std::collections::BTreeMap<u16,u64>,processed:&'a mut u64,diagnostics:&'a mut Vec<String>}
impl QueueHost<Event> for Pump<'_> {
    fn listener_registered(&mut self,listener:usize,kind:u16)->Result<bool> {Ok(self.registry.registered_node_for(kind,u32::try_from(listener)?))}
    fn deliver(&mut self,queue:&mut Queue,entry:&Entry<Event>)->Result<()> {
        let kind=entry.event.kind();
        if !self.registry.has_nodes_for(kind) {
            let count=self.unbound.entry(kind).or_default();
            if *count==0 {self.diagnostics.push(format!("Deferred event kind{kind:#x} had no registered subscribers; native subscription coverage remains unverified."));}
            *count=count.wrapping_add(1);
        }
        self.registry.dispatch(&entry.event,entry.listener.map(u32::try_from).transpose()?,&mut QueuedReceivers {inner:&mut self.receivers,queue})?;
        *self.processed=self.processed.wrapping_add(1);
        Ok(())
    }
}
const OUTPUT_EVENT_KINDS: [u16; 15] = [
    0x0d, 0x0e, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x3d, 0x3e, 0x44, 0x4e, 0x8d, 0x8e,
];

impl super::assets::World {
    pub(super) fn bind_deferred_output_events(&mut self,owner:usize)->Result<()> {
        let kinds = OUTPUT_EVENT_KINDS.into_iter()
            .filter(|kind| self.outputs.has_connections(owner, deferred_output(*kind).unwrap()))
            .collect::<Vec<_>>();
        if kinds.is_empty() {return Ok(())}
        let existing=self.deferred_event_runtime.io_listeners.iter()
            .find_map(|(&listener,&value)|(value==owner).then_some(listener));
        let listener=if let Some(listener)=existing {listener} else {
            let listener=self.cutscene_actions.allocate_listener()?;
            self.deferred_event_runtime.io_listeners.insert(listener,owner);
            listener
        };
        // A newly bound IO row may add an event kind to an existing owner.
        // Reuse its identity without replacing earlier subscriptions or
        // perturbing epochs for kinds that are already registered.
        for kind in kinds {
            if !self.entity_event_registry.registered_node_for(kind,listener) {
                self.entity_event_registry.register_for(kind,listener,Some(owner))?;
            }
        }
        Ok(())
    }
    pub(super) fn retire_deferred_output_events(&mut self,owner:usize) {
        let registry=&mut self.entity_event_registry;
        self.deferred_event_runtime.io_listeners.retain(|&listener,value| {
            if *value!=owner {return true}
            for kind in OUTPUT_EVENT_KINDS {registry.deactivate_for(kind,listener);}
            false
        });
    }
    pub(super) fn process_deferred_entity_events(&mut self)->Result<()> {
        let io_before=(self.deferred_event_runtime.spawner_io_deliveries,self.deferred_event_runtime.relay_io_deliveries,self.deferred_event_runtime.use_io_deliveries,self.deferred_event_runtime.spline_io_deliveries,self.deferred_event_runtime.spawn_io_deliveries);
        self.cutscene_actions.last_receivers.clear();
        let result=self.entity_event_queue.process(&mut Pump {
            registry:&mut self.entity_event_registry,
            receivers:Receivers {actions:&mut self.cutscene_actions,brains:&mut self.ai_brains,arms:&self.robot_arm_controls,
                io_listeners:&self.deferred_event_runtime.io_listeners,outputs:&mut self.outputs,entities:&self.entities,removed:&self.removed_entities,resources:&self.scene_resources,time:self.animation_time,io_deliveries:&mut self.deferred_event_runtime.spawner_io_deliveries,relay_deliveries:&mut self.deferred_event_runtime.relay_io_deliveries,use_deliveries:&mut self.deferred_event_runtime.use_io_deliveries,spline_deliveries:&mut self.deferred_event_runtime.spline_io_deliveries,spawn_deliveries:&mut self.deferred_event_runtime.spawn_io_deliveries,
                camera:super::camera_feedback_world::Bindings {profiles:&self.camera_profiles,tracker:self.mickey_tracker.as_mut(),settings:&self.runtime_settings,state:&mut self.camera_feedback}},
            unbound:&mut self.deferred_event_runtime.unbound_kinds,processed:&mut self.deferred_event_runtime.processed,diagnostics:&mut self.report.unsupported,
        });
        self.deferred_event_runtime.passes=self.deferred_event_runtime.passes.wrapping_add(1);
        result?;
        if (self.deferred_event_runtime.spawner_io_deliveries,self.deferred_event_runtime.relay_io_deliveries,self.deferred_event_runtime.use_io_deliveries,self.deferred_event_runtime.spline_io_deliveries,self.deferred_event_runtime.spawn_io_deliveries)!=io_before {self.dispatch_actor_inputs(self.animation_time);}
        let owners=self.spawners.iter().filter_map(|(&owner,state)|state.automatic.retirement_pending.then_some(owner)).collect::<Vec<_>>();
        for owner in owners {
            if self.outputs.has_pending_for_owner(owner) {continue;}
            self.scripts.schedule_retirement(owner)?;
            self.spawners.get_mut(&owner).unwrap().automatic.retirement_pending=false;
        }
        Ok(())
    }
}

/// Retail Lua event constants270e04..270fb4. InputOutputComponent selects
/// these numeric event kinds from its authored Output names.
fn deferred_output(kind:u16)->Option<&'static str> {
    //273eb0..273f94 registers the exact Patrol output names and IDs.
    match kind {0x0d=>Some("OnReachedPosition"),0x0e=>Some("OnUse"),0x1c=>Some("OnPass"),0x1d=>Some("OnHold"),0x1e=>Some("OnHoldEnd"),0x18=>Some("OnSpawnerSpawned"),0x19=>Some("OnSpawnerAllSpawned"),0x1a=>Some("OnSpawnerAllDead"),0x1b=>Some("OnSpawnerAllDeadForever"),0x3d=>Some("OnSpawned"),0x3e=>Some("OnSpawnedDynamically"),0x44=>Some("OnEntityRemovedFromWorld"),0x4e=>Some("OnRelayTrigger"),0x8d=>Some("OnReachedPatrolNode"),0x8e=>Some("OnLeftPatrolNode"),_=>None}
}

///5d020: an own-source reached-position event releases only phase4's timer.
fn receive_arm_position(actions:&super::cutscene_actions::State,listener:u32,event:&Event,
    brains:&mut std::collections::BTreeMap<usize,super::ai_behavior_events::Brain>,
    arms:&std::collections::BTreeMap<usize,super::robot_arm::Runtime>)->Result<()> {
    use anyhow::Context as _;
    let owner=actions.listener_owner(listener).context("Ordinary AI event receiver not constructed")?;
    let Event::Basic(event)=event else {anyhow::bail!("Arm position receiver requires basic payload")};
    anyhow::ensure!(event.event_kind==0x0d,"Unexpected ordinary arm event");
    if event.source!=Some(owner) {return Ok(())}
    if arms.get(&owner).is_some_and(|arm|arm.phase==4) {
        let brain=brains.get_mut(&owner).context("Arm position receiver missing brain")?;
        if let Some(slot)=brain.selector.current {
            if brain.behaviors.get(slot).is_some_and(|name|name=="RobotArmGeneral") {
                brain.behavior_tasks.get_mut(&slot).context("Arm position task missing")?.timer=0.;
            }
        }
    }
    Ok(())
}
