//! Shared event command runtime. The caller supplies native node execution,
//! completion services, shared RNG and processed audio blocks. This module
//! does not derive an audio clock from frame time or substitute a mono preview.
use super::{
    audio_bank::{Action, Bank, Object},
    audio_consumer::{self, PendingContext},
    audio_ingress::{self, Command, Registration},
    audio_play_action,
    audio_playing::Playing,
    audio_schedule::{self, Execution, Owner, Schedule, Sink},
};
use anyhow::{ensure, Context as _, Result};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, RwLock},
};

struct Event {
    actions: Arc<[u32]>,
    references: u32,
}
enum Queued {
    Event(Command<u32>),
    Position {
        object: u32,
        position: super::audio_distance::Position,
        relative: i32,
    },
    Positions {
        object: u32,
        positions: Vec<super::audio_distance::Position>,
        mode: u32,
    },
    ListenerMask {
        object: u32,
        mask: u32,
    },
    ListenerPose {
        index: u32,
        pose: super::audio_listener::Pose,
    },
    ListenerSettings {
        index: u32,
        enabled: u8,
        gains: Option<[f32; 4]>,
    },
    RadiusScale {
        object: u32,
        scale: f32,
    },
    Switch {
        object: u32,
        group: u32,
        value: u32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpatialResult {
    pub object: u32,
    pub command: u16,
    pub status: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListenerResult {
    pub index: u32,
    pub command: u16,
    pub status: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchResult {
    pub object: u32,
    pub group: u32,
    pub value: u32,
    pub status: u32,
}
#[derive(serde::Serialize)]
pub struct SwitchSnapshot {
    pub object: u32,
    pub handle: u32,
    pub values: BTreeMap<u32, u32>,
}
#[derive(Default, Debug)]
pub struct CommandBatch {
    pub events: Vec<Consumed>,
    pub spatial: Vec<SpatialResult>,
    pub listeners: Vec<ListenerResult>,
    pub switches: Vec<SwitchResult>,
}
/// The identity is separate from the authored game-object ID. Re-registering
/// an ID after unregistering it must not change an older pending action's owner.
pub struct RegisteredObject {
    pub id: u32,
    pub handle: u32,
    emitter: RwLock<super::audio_distance::Emitter>,
    switches: RwLock<BTreeMap<u32, u32>>,
}
impl RegisteredObject {
    pub fn switch_value(&self, group: u32) -> Result<Option<u32>> {
        Ok(self
            .switches
            .read()
            .map_err(|_| anyhow::anyhow!("Audio switch state lock poisoned"))?
            .get(&group)
            .copied())
    }
    fn apply_switch(&self, group: u32, value: u32) -> Result<()> {
        self.switches
            .write()
            .map_err(|_| anyhow::anyhow!("Audio switch state lock poisoned"))?
            .insert(group, value);
        Ok(())
    }
    /// Apply on the audio command-consumer thread, not at game API submission.
    /// Voices and pending actions retain this identity and see later updates.
    pub fn apply_positions(
        &self,
        positions: &[super::audio_distance::Position],
        mode: u32,
        relative: i32,
    ) -> Result<()> {
        self.emitter
            .write()
            .map_err(|_| anyhow::anyhow!("Audio emitter state lock poisoned"))?
            .set_positions(positions, mode, relative)
    }
    pub fn apply_listener_mask(&self, mask: u8) -> Result<()> {
        self.emitter
            .write()
            .map_err(|_| anyhow::anyhow!("Audio emitter state lock poisoned"))?
            .set_listener_mask(mask);
        Ok(())
    }
    pub fn apply_radius_scale(&self, scale: f32) -> Result<()> {
        self.emitter
            .write()
            .map_err(|_| anyhow::anyhow!("Audio emitter state lock poisoned"))?
            .set_radius_scale(scale);
        Ok(())
    }
    pub fn minimum_distance(
        &self,
        listeners: &[super::audio_distance::Listener; 8],
    ) -> Result<f32> {
        self.emitter
            .read()
            .map_err(|_| anyhow::anyhow!("Audio emitter state lock poisoned"))?
            .minimum_distance(listeners)
    }
    /// Use the retained identity's live state, never an authored-ID lookup that
    /// could redirect an old voice to a newly registered object.
    pub fn admission_priority(
        &self,
        listeners: &[super::audio_distance::Listener; 8],
        base: f32,
        offset: f32,
        radius: super::audio_sound::Radius,
    ) -> Result<(super::audio_sound::Priority, super::audio_sound::Radius)> {
        let emitter = self
            .emitter
            .read()
            .map_err(|_| anyhow::anyhow!("Audio emitter state lock poisoned"))?;
        super::audio_sound::spatial_priority(
            base,
            offset,
            radius,
            &mut EmitterSpatial {
                emitter: &emitter,
                listeners,
            },
        )
    }
    pub fn emitter_snapshot(&self) -> Result<super::audio_distance::Emitter> {
        Ok(self
            .emitter
            .read()
            .map_err(|_| anyhow::anyhow!("Audio emitter state lock poisoned"))?
            .clone())
    }
}
struct EmitterSpatial<'a> {
    emitter: &'a super::audio_distance::Emitter,
    listeners: &'a [super::audio_distance::Listener; 8],
}
impl super::audio_sound::Spatial for EmitterSpatial<'_> {
    fn object_scale(&mut self) -> Result<f32> {
        Ok(self.emitter.radius_scale())
    }
    fn distance(&mut self) -> Result<f32> {
        self.emitter.minimum_distance(self.listeners)
    }
}
/// Node execution receives the shared playing registry, so nested voices can
/// register before action/event completion. A voice must retain its owner for
/// its entire lifetime, including after the object is unregistered.
pub trait NodeHost {
    fn prepare_parameter(&mut self,_:&Action<'_>,_:u32)->Result<()> {Ok(())}
    /// Called after command4 resolves and retains the current object. Hosts
    /// owning continuous switch subscriptions must deliver their notifications.
    /// Reject by default rather than silently dropping those notifications.
    fn switch_changed(&mut self, _: Arc<RegisteredObject>, _: u32, _: u32) -> Result<()> {
        anyhow::bail!(
            "Continuous switch notification delivery is not implemented by this audio host"
        )
    }
    fn execute(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        owner: Option<Arc<RegisteredObject>>,
        listeners: &super::audio_listener::Listeners,
        playing: &mut Playing,
        completion: &mut dyn audio_schedule::CompletionHost,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution>;
    fn play_pending(&mut self, action: &Action<'_>, _: audio_play_action::Context) -> Result<()> {
        ensure!(
            action.kind == 0x4011,
            "Non-Play pending notification requires its native method"
        );
        let _notification = audio_play_action::pending_notification();
        Ok(())
    }
    fn update(
        &mut self,
        tick: u32,
        listeners: &super::audio_listener::Listeners,
        playing: &mut Playing,
        completion: &mut dyn audio_schedule::CompletionHost,
    ) -> Result<()>;
}
impl NodeHost for super::audio_nodes::Nodes {
    fn execute(
        &mut self,
        action: &Action<'_>,
        _: audio_play_action::Context,
        _: Option<Arc<RegisteredObject>>,
        _: &super::audio_listener::Listeners,
        _: &mut Playing,
        _: &mut dyn audio_schedule::CompletionHost,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution> {
        Ok(Execution::Parameter(super::audio_control::execute_due(
            action, self, random,
        )?))
    }
    fn update(
        &mut self,
        tick: u32,
        _: &super::audio_listener::Listeners,
        _: &mut Playing,
        _: &mut dyn audio_schedule::CompletionHost,
    ) -> Result<()> {
        super::audio_nodes::Nodes::update(self, tick)
    }
}
struct Driven<'a, N, C> {
    nodes: &'a mut N,
    listeners: &'a super::audio_listener::Listeners,
    playing: &'a mut Playing,
    completion: &'a mut C,
}
impl<N: NodeHost, C: audio_schedule::CompletionHost> Sink for Driven<'_, N, C> {
    fn prepare_parameter(&mut self,action:&Action<'_>,due:u32)->Result<()> {self.nodes.prepare_parameter(action,due)}
    fn action_admitted(&mut self, id: u32) -> Result<()> {
        self.playing.add_action(id);
        Ok(())
    }
    fn action_finished(&mut self, id: u32) -> Result<()> {
        if let Some(done) = self.playing.remove_action(id, self.completion) {
            if let Some(end) = done.end {
                self.completion.end_event(end)?;
            }
        }
        Ok(())
    }
    fn play_pending(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
    ) -> Result<()> {
        self.nodes.play_pending(action, context)
    }
    fn execute(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution> {
        self.nodes.execute(
            action,
            context,
            None,
            self.listeners,
            self.playing,
            self.completion,
            random,
        )
    }
    fn execute_retained(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        owner: Option<&Owner>,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution> {
        let owner = owner
            .map(|owner| {
                owner
                    .clone()
                    .downcast::<RegisteredObject>()
                    .map_err(|_| anyhow::anyhow!("Wrong audio pending owner type"))
            })
            .transpose()?;
        self.nodes.execute(
            action,
            context,
            owner,
            self.listeners,
            self.playing,
            self.completion,
            random,
        )
    }
    fn update(&mut self, tick: u32) -> Result<()> {
        self.nodes
            .update(tick, self.listeners, self.playing, self.completion)
    }
}
#[derive(Default)]
pub struct Events {
    producer: audio_ingress::Producer,
    events: BTreeMap<u32, Event>,
    actions: BTreeMap<u32, Arc<[u8]>>,
    objects: BTreeMap<u32, Arc<RegisteredObject>>,
    next_handle: u32,
    commands: VecDeque<Queued>,
    pub playing: Playing,
    pub listeners: super::audio_listener::Listeners,
    schedule: Schedule,
}
#[derive(Debug)]
pub struct Consumed {
    pub event: u32,
    pub outcome: audio_consumer::Outcome,
    pub executions: Vec<Execution>,
}
impl Events {
    /// Validate the whole registration before modifying the index. Store only
    /// event/action definitions; the voice host owns sources, media and buses.
    pub fn register_bank(&mut self, bank: &Bank<'_>) -> Result<()> {
        let mut actions = Vec::new();
        let mut events = Vec::new();
        for (&id, object) in &bank.objects {
            if object.kind == 3 {
                // DOL80347eb4..80347f7c retains an already initialized action.
                // Shared Play IDs legitimately carry different bank-routing
                // words in different banks. Every entry here is fully read;
                // the native uninitialized-Play retry path is not represented.
                if self.actions.contains_key(&id) {
                    continue;
                }
                object.action()?;
                actions.push((id, Arc::<[u8]>::from(object.payload)));
            } else if object.kind == 4 {
                let ordered = bank.event_actions(id)?;
                ensure!(
                    self.events
                        .get(&id)
                        .is_none_or(|old| old.actions.as_ref() == ordered.as_slice()),
                    "Conflicting loaded audio event {id:08x}"
                );
                events.push((id, Arc::<[u32]>::from(ordered)));
            }
        }
        for (id, payload) in actions {
            self.actions.entry(id).or_insert(payload);
        }
        for (id, actions) in events {
            self.events.entry(id).or_insert(Event {
                actions,
                references: 1,
            });
        }
        Ok(())
    }
    pub fn register_object(&mut self, id: u32) -> Result<Arc<RegisteredObject>> {
        if let Some(object) = self.objects.get(&id) {
            return Ok(object.clone());
        }
        self.next_handle = self
            .next_handle
            .checked_add(1)
            .context("Audio object handle space exhausted")?;
        let object = Arc::new(RegisteredObject {
            id,
            handle: self.next_handle,
            emitter: RwLock::default(),
            switches: RwLock::default(),
        });
        self.objects.insert(id, object.clone());
        Ok(object)
    }
    pub fn unregister_object(&mut self, id: u32) {
        self.objects.remove(&id);
    }
    pub fn registered_object(&self, id: u32) -> Option<Arc<RegisteredObject>> {
        self.objects.get(&id).cloned()
    }
    pub fn switch_snapshots(&self) -> Result<Vec<SwitchSnapshot>> {
        self.objects
            .values()
            .map(|object| {
                Ok(SwitchSnapshot {
                    object: object.id,
                    handle: object.handle,
                    values: object
                        .switches
                        .read()
                        .map_err(|_| anyhow::anyhow!("Audio switch state lock poisoned"))?
                        .clone(),
                })
            })
            .collect()
    }
    /// Already-formed native command8. Public game API validation and coordinate
    /// conversion precede this boundary and are not supplied by this method.
    pub fn enqueue_position(
        &mut self,
        object: u32,
        position: super::audio_distance::Position,
        relative: i32,
    ) {
        self.commands.push_back(Queued::Position {
            object,
            position,
            relative,
        });
    }
    /// Native command33 has a halfword count and forces relative listener -1.
    pub fn enqueue_positions(
        &mut self,
        object: u32,
        positions: Vec<super::audio_distance::Position>,
        mode: u32,
    ) -> Result<()> {
        ensure!(
            positions.len() <= u16::MAX as usize,
            "Audio command position count exceeds native halfword"
        );
        self.commands.push_back(Queued::Positions {
            object,
            positions,
            mode,
        });
        Ok(())
    }
    pub fn enqueue_listener_mask(&mut self, object: u32, mask: u32) {
        self.commands
            .push_back(Queued::ListenerMask { object, mask });
    }
    pub fn enqueue_listener_pose(&mut self, index: u32, pose: super::audio_listener::Pose) {
        self.commands
            .push_back(Queued::ListenerPose { index, pose });
    }
    pub fn enqueue_listener_settings(&mut self, index: u32, enabled: u8, gains: Option<[f32; 4]>) {
        self.commands.push_back(Queued::ListenerSettings {
            index,
            enabled,
            gains,
        });
    }
    pub fn enqueue_radius_scale(&mut self, object: u32, scale: f32) {
        self.commands
            .push_back(Queued::RadiusScale { object, scale });
    }
    pub fn enqueue_switch(&mut self, object: u32, group: u32, value: u32) {
        self.commands.push_back(Queued::Switch {
            object,
            group,
            value,
        });
    }
    pub fn queued(&self) -> usize {
        self.commands.len()
    }
    pub fn pending(&self) -> usize {
        self.schedule.pending()
    }
    pub fn tick(&self) -> u32 {
        self.schedule.tick()
    }
    pub fn post_counter(&self) -> u32 {
        self.producer.counter
    }
    pub fn action_definition(&self, id: u32) -> Option<Object<'_>> {
        self.actions
            .get(&id)
            .map(|payload| Object { kind: 3, payload })
    }
    pub fn event_references(&self, id: u32) -> Option<u32> {
        self.events.get(&id).map(|event| event.references)
    }
    pub fn post(
        &mut self,
        event: u32,
        object: u32,
        words: Option<[u32; 3]>,
        flags: u32,
        callback: u32,
        cookie: u32,
    ) -> Result<u32> {
        let mut host = Posting {
            events: &mut self.events,
            commands: &mut self.commands,
            playing: &mut self.playing,
        };
        self.producer
            .post(event, object, words, flags, callback, cookie, &mut host)
    }
    /// Run the command phase before processing the device's audio blocks.
    /// A native status is returned inside Execution; unsupported Rust host
    /// operations remain errors, with event references/completion cleaned up.
    pub fn process_commands(
        &mut self,
        sink: &mut impl NodeHost,
        completion: &mut impl audio_schedule::CompletionHost,
        mut random: impl FnMut() -> u16,
    ) -> Result<CommandBatch> {
        let mut results = CommandBatch::default();
        while let Some(command) = self.commands.pop_front() {
            let command = match command {
                Queued::Event(command) => command,
                Queued::Switch {
                    object,
                    group,
                    value,
                } => {
                    let status = if let Some(owner) = self.objects.get(&object).cloned() {
                        owner.apply_switch(group, value)?;
                        sink.switch_changed(owner, group, value)?;
                        1
                    } else {
                        2
                    };
                    results.switches.push(SwitchResult {
                        object,
                        group,
                        value,
                        status,
                    });
                    continue;
                }
                Queued::Position {
                    object,
                    position,
                    relative,
                } => {
                    let status = if let Some(owner) = self.objects.get(&object) {
                        owner.apply_positions(&[position], 0, relative)?;
                        1
                    } else {
                        2
                    };
                    results.spatial.push(SpatialResult {
                        object,
                        command: 8,
                        status,
                    });
                    continue;
                }
                Queued::Positions {
                    object,
                    positions,
                    mode,
                } => {
                    let status = if let Some(owner) = self.objects.get(&object) {
                        owner.apply_positions(&positions, mode, -1)?;
                        1
                    } else {
                        2
                    };
                    results.spatial.push(SpatialResult {
                        object,
                        command: 33,
                        status,
                    });
                    continue;
                }
                Queued::ListenerMask { object, mask } => {
                    // Native8035eb10 rejects ID0 even when it is registered.
                    let status = if object == 0 {
                        2
                    } else if let Some(owner) = self.objects.get(&object) {
                        owner.apply_listener_mask(mask as u8)?;
                        1
                    } else {
                        2
                    };
                    results.spatial.push(SpatialResult {
                        object,
                        command: 9,
                        status,
                    });
                    continue;
                }
                Queued::ListenerPose { index, pose } => {
                    let status = self.listeners.set_pose(index, pose)?;
                    results.listeners.push(ListenerResult {
                        index,
                        command: 11,
                        status,
                    });
                    continue;
                }
                Queued::ListenerSettings {
                    index,
                    enabled,
                    gains,
                } => {
                    let status = self
                        .listeners
                        .set_spatialization(index, enabled as u32, gains);
                    results.listeners.push(ListenerResult {
                        index,
                        command: 12,
                        status,
                    });
                    continue;
                }
                Queued::RadiusScale { object, scale } => {
                    let status = if let Some(owner) = self.objects.get(&object) {
                        owner.apply_radius_scale(scale)?;
                        1
                    } else {
                        2
                    };
                    results.spatial.push(SpatialResult {
                        object,
                        command: 32,
                        status,
                    });
                    continue;
                }
            };
            let mut executions = Vec::new();
            let mut tracked = Driven {
                nodes: sink,
                listeners: &self.listeners,
                playing: &mut self.playing,
                completion,
            };
            let mut host = Consuming {
                events: &mut self.events,
                actions: &self.actions,
                objects: &self.objects,
                acquired: None,
                schedule: &mut self.schedule,
                sink: &mut tracked,
                random: &mut random,
                executions: &mut executions,
            };
            let outcome = audio_consumer::consume(command, &mut host)?;
            results.events.push(Consumed {
                event: command.event,
                outcome,
                executions,
            });
        }
        Ok(results)
    }
    pub fn process_block(
        &mut self,
        sink: &mut impl NodeHost,
        completion: &mut impl audio_schedule::CompletionHost,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Execution>> {
        self.schedule.process_audio_block(
            &mut Driven {
                nodes: sink,
                listeners: &self.listeners,
                playing: &mut self.playing,
                completion,
            },
            &mut random,
        )
    }
}
struct Posting<'a> {
    events: &'a mut BTreeMap<u32, Event>,
    commands: &'a mut VecDeque<Queued>,
    playing: &'a mut Playing,
}
impl audio_ingress::Control for Posting<'_> {
    type Event = u32;
    fn acquire_event(&mut self, id: u32) -> Result<Option<u32>> {
        Ok(self.events.get_mut(&id).map(|event| {
            event.references = event.references.wrapping_add(1);
            id
        }))
    }
    fn enqueue(&mut self, command: Command<u32>) -> Result<u32> {
        self.commands.push_back(Queued::Event(command));
        Ok(1)
    }
    fn register_playing(&mut self, registration: Registration) -> Result<u32> {
        self.playing.register(registration);
        Ok(1)
    }
}
struct Pending {
    payload: Arc<[u8]>,
    owner: Option<Arc<RegisteredObject>>,
    context: PendingContext,
}
struct Consuming<'a, S: Sink> {
    events: &'a mut BTreeMap<u32, Event>,
    actions: &'a BTreeMap<u32, Arc<[u8]>>,
    objects: &'a BTreeMap<u32, Arc<RegisteredObject>>,
    acquired: Option<Arc<RegisteredObject>>,
    schedule: &'a mut Schedule,
    sink: &'a mut S,
    random: &'a mut dyn FnMut() -> u16,
    executions: &'a mut Vec<Execution>,
}
impl<S: Sink> audio_consumer::Control for Consuming<'_, S> {
    type Event = u32;
    type Object = u32;
    type Action = u32;
    type Pending = Pending;
    fn acquire_object(&mut self, id: u32) -> Result<Option<u32>> {
        self.acquired = self.objects.get(&id).cloned();
        Ok(self.acquired.as_ref().map(|object| object.handle))
    }
    fn event_actions(&mut self, id: u32) -> Result<Vec<(u32, u32)>> {
        self.events
            .get(&id)
            .context("Queued audio event was removed")?
            .actions
            .iter()
            .map(|&id| {
                let payload = self
                    .actions
                    .get(&id)
                    .with_context(|| format!("Missing loaded audio action {id:08x}"))?;
                Ok((id, Object { kind: 3, payload }.action()?.kind))
            })
            .collect()
    }
    fn allocate_pending(
        &mut self,
        id: u32,
        object: Option<u32>,
        context: PendingContext,
    ) -> Result<Option<Pending>> {
        let owner = if let Some(handle) = object {
            let acquired = self
                .acquired
                .as_ref()
                .context("Pending object lost its acquired reference")?;
            ensure!(
                acquired.handle == handle,
                "Pending object identity mismatch"
            );
            Some(acquired.clone())
        } else {
            None
        };
        Ok(Some(Pending {
            payload: self
                .actions
                .get(&id)
                .context("Pending action missing")?
                .clone(),
            owner,
            context,
        }))
    }
    fn admit_pending(&mut self, pending: Pending) -> Result<()> {
        let context = audio_play_action::Context {
            object: pending.owner.as_ref().map_or(0, |object| object.handle),
            playing_id: pending.context.playing_id,
            sample_offset: 0,
            words: [
                pending.context.words[0],
                pending.context.words[1],
                pending.context.words[2],
                0,
            ],
        };
        // The fourth slot is native padding, not another authored parameter.
        // Keep Rust padding initialized without claiming a native value for it.
        let owner = pending.owner.map(|object| object as Owner);
        self.executions.extend(self.schedule.post_retained(
            pending.payload,
            owner,
            context,
            self.sink,
            &mut self.random,
        )?);
        Ok(())
    }
    fn release_object(&mut self, _: u32) {
        self.acquired.take();
    }
    fn finish_event(&mut self, id: u32) -> Result<()> {
        self.sink.action_finished(id)
    }
    fn release_event(&mut self, id: u32) {
        let event = self.events.get_mut(&id).expect("Released missing event");
        event.references = event.references.wrapping_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{audio_bank::event_id, audio_playing::EndEvent};
    use super::*;
    #[derive(Default)]
    struct Completed {
        positions: Vec<u32>,
        ends: Vec<EndEvent>,
    }
    impl super::super::audio_playing::CompletionHost for Completed {
        fn remove_source_position(&mut self, id: u32) {
            self.positions.push(id);
        }
    }
    impl audio_schedule::CompletionHost for Completed {
        fn end_event(&mut self, event: EndEvent) -> Result<()> {
            self.ends.push(event);
            Ok(())
        }
    }
    struct SwitchHost {
        changed: Vec<(u32, u32, u32)>,
    }
    impl NodeHost for SwitchHost {
        fn switch_changed(
            &mut self,
            object: Arc<RegisteredObject>,
            group: u32,
            value: u32,
        ) -> Result<()> {
            assert_eq!(object.switch_value(group)?, Some(value));
            self.changed.push((object.handle, group, value));
            Ok(())
        }
        fn execute(
            &mut self,
            _: &Action<'_>,
            _: audio_play_action::Context,
            _: Option<Arc<RegisteredObject>>,
            _: &super::super::audio_listener::Listeners,
            _: &mut Playing,
            _: &mut dyn audio_schedule::CompletionHost,
            _: &mut dyn FnMut() -> u16,
        ) -> Result<Execution> {
            anyhow::bail!("Unexpected event")
        }
        fn update(
            &mut self,
            _: u32,
            _: &super::super::audio_listener::Listeners,
            _: &mut Playing,
            _: &mut dyn audio_schedule::CompletionHost,
        ) -> Result<()> {
            anyhow::bail!("Unexpected block")
        }
    }
    #[test]
    fn queued_switch_values_keep_object_scope_and_registration_identity() -> Result<()> {
        let mut events = Events::default();
        let old = events.register_object(7)?;
        let other = events.register_object(9)?;
        events.enqueue_switch(7, 12, 100);
        events.enqueue_switch(9, 12, 200);
        events.enqueue_switch(99, 12, 300);
        assert_eq!(old.switch_value(12)?, None);
        let mut host = SwitchHost {
            changed: Vec::new(),
        };
        let batch = events.process_commands(&mut host, &mut Completed::default(), || {
            panic!("Switch command consumed RNG")
        })?;
        assert_eq!(
            batch
                .switches
                .iter()
                .map(|entry| entry.status)
                .collect::<Vec<_>>(),
            [1, 1, 2]
        );
        assert_eq!(old.switch_value(12)?, Some(100));
        assert_eq!(other.switch_value(12)?, Some(200));
        events.unregister_object(7);
        let replacement = events.register_object(7)?;
        assert_ne!(old.handle, replacement.handle);
        events.enqueue_switch(7, 12, 400);
        events.enqueue_switch(7, 12, 400);
        events.process_commands(&mut host, &mut Completed::default(), || panic!())?;
        assert_eq!(old.switch_value(12)?, Some(100));
        assert_eq!(replacement.switch_value(12)?, Some(400));
        assert_eq!(
            host.changed,
            [
                (old.handle, 12, 100),
                (other.handle, 12, 200),
                (replacement.handle, 12, 400),
                (replacement.handle, 12, 400)
            ]
        );
        Ok(())
    }
    #[test]
    fn admission_priority_uses_queued_live_scale_positions_and_retained_identity() -> Result<()> {
        use super::super::{
            audio_distance::{Listener, Position},
            audio_sound::Radius,
        };
        let mut events = Events::default();
        let mut nodes = super::super::audio_nodes::Nodes::new()?;
        let mut completed = Completed::default();
        let old = events.register_object(42)?;
        let position = Position {
            position: [3., 4., 0.],
            orientation: [0., 0., 1.],
        };
        events.enqueue_position(42, position, -1);
        events.enqueue_radius_scale(42, 2.);
        assert_eq!(old.emitter_snapshot()?.radius_scale(), 1.);
        let result = events.process_commands(&mut nodes, &mut completed, || panic!())?;
        assert_eq!(
            result.spatial[1],
            SpatialResult {
                object: 42,
                command: 32,
                status: 1
            }
        );
        let listeners = events.listeners.distance_records();
        let (priority, radius) = old.admission_priority(
            &listeners,
            60.,
            -40.,
            Radius {
                found: true,
                value: 10.,
            },
        )?;
        assert_eq!(radius.value, 20.);
        assert_eq!(priority.current, 50.);
        events.unregister_object(42);
        let replacement = events.register_object(42)?;
        replacement.apply_positions(&[position], 0, -1)?;
        assert_eq!(
            replacement
                .admission_priority(
                    &listeners,
                    60.,
                    -40.,
                    Radius {
                        found: true,
                        value: 10.
                    }
                )?
                .0
                .current,
            40.
        );
        assert_eq!(
            old.admission_priority(
                &listeners,
                60.,
                -40.,
                Radius {
                    found: true,
                    value: 10.
                }
            )?
            .0
            .current,
            50.
        );
        let invalid = [Listener {
            position: [f32::NAN; 3],
            scale: f32::NAN,
        }; 8];
        // These branches must not evaluate invalid, unused listener geometry.
        assert_eq!(
            old.admission_priority(
                &invalid,
                150.,
                -40.,
                Radius {
                    found: false,
                    value: 7.
                }
            )?
            .0
            .current,
            150.
        );
        let (priority, radius) = old.admission_priority(
            &invalid,
            150.,
            0.,
            Radius {
                found: true,
                value: 7.,
            },
        )?;
        assert_eq!(priority.current, 150.);
        assert_eq!(radius.value, 14.);
        assert!(old
            .admission_priority(
                &invalid,
                60.,
                -40.,
                Radius {
                    found: true,
                    value: 7.
                }
            )
            .is_err());
        Ok(())
    }
    #[test]
    fn queued_listener_pose_and_settings_apply_in_order_and_report_invalid_index() -> Result<()> {
        let mut events = Events::default();
        let mut nodes = super::super::audio_nodes::Nodes::new()?;
        let mut completed = Completed::default();
        let pose = super::super::audio_listener::Pose {
            forward: [0., 0., 1.],
            up: [0., 1., 0.],
            position: [3., 4., 5.],
        };
        events.enqueue_listener_pose(0, pose);
        events.enqueue_listener_settings(0, 0, Some([-6., -3., 0., 1.]));
        events.enqueue_listener_settings(0, 255, None);
        events.enqueue_listener_pose(8, pose);
        assert_eq!(events.listeners.records()[0].pose.position, [0.; 3]);
        let result = events.process_commands(&mut nodes, &mut completed, || panic!())?;
        assert_eq!(
            result.listeners,
            [
                ListenerResult {
                    index: 0,
                    command: 11,
                    status: 1
                },
                ListenerResult {
                    index: 0,
                    command: 12,
                    status: 1
                },
                ListenerResult {
                    index: 0,
                    command: 12,
                    status: 1
                },
                ListenerResult {
                    index: 8,
                    command: 11,
                    status: 31
                }
            ]
        );
        assert_eq!(events.listeners.records()[0].pose.position, pose.position);
        assert_eq!(events.listeners.records()[0].spatialization, 255);
        assert_eq!(
            events.listeners.records()[0].channel_gains,
            [-6., -3., 0., 1.]
        );
        let emitter = events.register_object(42)?;
        emitter.apply_positions(
            &[super::super::audio_distance::Position {
                position: pose.position,
                orientation: [0., 0., 1.],
            }],
            0,
            -1,
        )?;
        assert_eq!(
            emitter.minimum_distance(&events.listeners.distance_records())?,
            0.
        );
        Ok(())
    }
    #[test]
    fn original_spatial_commands_lookup_status_fields_and_deferred_updates() -> Result<()> {
        use super::super::audio_distance::Position;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_spatial_commands.json"
        ))?;
        let p = Position {
            position: [3., 4., 0.],
            orientation: [0., 0., 1.],
        };
        let q = Position {
            position: [-7., 8., 9.],
            orientation: [1., 0., 0.],
        };
        let mut nodes = super::super::audio_nodes::Nodes::new()?;
        let mut completed = Completed::default();
        for case in fixture["cases"].as_array().unwrap() {
            let mut events = Events::default();
            let object = case["object"].as_u64().unwrap() as u32;
            let kind = case["kind"].as_u64().unwrap() as u16;
            let value = case["value"].as_u64().unwrap_or(0) as u32;
            // Submit first: lookup must occur at consumption, not submission.
            match kind {
                8 => events.enqueue_position(object, p, value as i32),
                33 => events.enqueue_positions(object, vec![p, q], value)?,
                9 => events.enqueue_listener_mask(object, value),
                32 => events.enqueue_radius_scale(object, case["value"].as_f64().unwrap() as f32),
                _ => unreachable!(),
            }
            let owner = if case["present"] == true {
                Some(events.register_object(object)?)
            } else {
                None
            };
            let result = events.process_commands(&mut nodes, &mut completed, || {
                panic!("Spatial command drew RNG")
            })?;
            assert!(result.events.is_empty());
            assert_eq!(
                result.spatial,
                [SpatialResult {
                    object,
                    command: kind,
                    status: case["status"].as_u64().unwrap() as u32
                }]
            );
            if let Some(owner) = owner {
                let state = owner.emitter_snapshot()?;
                assert_eq!(state.listener_mask() as u64, case["mask"].as_u64().unwrap());
                assert_eq!(state.mode() as u64, case["mode"].as_u64().unwrap());
                assert_eq!(
                    state.relative_listener() as i64,
                    case["relative"].as_i64().unwrap()
                );
                assert_eq!(
                    state.radius_scale().to_bits() as u64,
                    case["radius_scale_bits"].as_u64().unwrap()
                );
                let expected: Vec<[f32; 6]> = serde_json::from_value(case["positions"].clone())?;
                assert_eq!(state.positions().len(), expected.len());
                for (actual, expected) in state.positions().iter().zip(expected) {
                    assert_eq!(actual.position, [expected[0], expected[1], expected[2]]);
                    assert_eq!(actual.orientation, [expected[3], expected[4], expected[5]]);
                }
            }
            assert_eq!(events.queued(), 0);
        }
        let mut events = Events::default();
        let old = events.register_object(42)?;
        events.enqueue_position(42, p, -1);
        events.unregister_object(42);
        let new = events.register_object(42)?;
        events.process_commands(&mut nodes, &mut completed, || panic!())?;
        assert!(old.emitter_snapshot()?.positions().is_empty());
        assert_eq!(new.emitter_snapshot()?.positions(), &[p]);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_bank_corpus_registers_event_and_action_definitions() -> Result<()> {
        fn collect(dir: &std::path::Path, paths: &mut Vec<std::path::PathBuf>) -> Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    collect(&path, paths)?;
                } else if path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("bnk"))
                {
                    paths.push(path);
                }
            }
            Ok(())
        }
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut paths = Vec::new();
        collect(&std::path::Path::new(&root).join("Audio/Wii"), &mut paths)?;
        assert!(!paths.is_empty());
        for path in paths {
            let bytes = std::fs::read(&path)?;
            let bank = Bank::parse(&bytes)?;
            let mut events = Events::default();
            events
                .register_bank(&bank)
                .with_context(|| format!("Register {}", path.display()))?;
            events.register_bank(&bank)?;
            assert_eq!(
                events.events.len(),
                bank.objects
                    .values()
                    .filter(|object| object.kind == 4)
                    .count()
            );
            assert_eq!(
                events.actions.len(),
                bank.objects
                    .values()
                    .filter(|object| object.kind == 3)
                    .count()
            );
            for event in events.events.values() {
                assert_eq!(event.references, 1);
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_guardians_flow_through_producer_consumer_scheduler_and_playing() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(std::path::Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        let mut nodes = super::super::audio_nodes::Nodes::new()?;
        nodes.register_bank(&bank)?;
        let mut events = Events::default();
        events.register_bank(&bank)?;
        let mut completed = Completed::default();
        let mute = event_id("Mute_Guardians")?;
        let unmute = event_id("Unmute_Guardians")?;
        let targets = bank
            .event_actions(mute)?
            .iter()
            .map(|id| bank.objects[id].action().map(|action| action.target))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(events.post(0xdeadbeef, 42, None, 0, 0, 0)?, 0);
        assert_eq!(events.post(mute, 42, Some([1, 2, 3]), 0x10001, 9, 11)?, 1);
        assert_eq!(events.queued(), 1);
        assert_eq!(events.event_references(mute), Some(2));
        assert_eq!(events.playing.record(1).unwrap().actions, 1);
        assert!(completed.ends.is_empty());
        let delivered =
            events.process_commands(&mut nodes, &mut completed, || panic!("Unexpected RNG"))?;
        assert_eq!(delivered.events.len(), 1);
        assert_eq!(delivered.events[0].outcome.admitted, 2);
        assert_eq!(delivered.events[0].executions.len(), 2);
        assert_eq!(events.event_references(mute), Some(1));
        assert!(events.playing.record(1).is_none());
        assert_eq!(completed.positions, [1]);
        assert_eq!(completed.ends.len(), 1);
        for _ in 0..167 {
            events.process_block(&mut nodes, &mut completed, || panic!())?;
        }
        for &id in &targets {
            assert_eq!(nodes.byte(id), Some(0));
        }
        assert_eq!(events.post(unmute, 42, None, 1, 9, 12)?, 2);
        let delivered = events.process_commands(&mut nodes, &mut completed, || panic!())?;
        assert!(delivered.events[0].executions.is_empty());
        assert_eq!(events.pending(), 2);
        assert_eq!(events.playing.record(2).unwrap().actions, 2);
        for _ in 0..666 {
            events.process_block(&mut nodes, &mut completed, || panic!())?;
        }
        assert_eq!(completed.ends.len(), 1);
        assert_eq!(events.tick(), 833);
        assert_eq!(
            events
                .process_block(&mut nodes, &mut completed, || panic!())?
                .len(),
            2
        );
        assert_eq!(events.pending(), 0);
        assert!(events.playing.record(2).is_none());
        assert_eq!(completed.ends.len(), 2);
        assert_eq!(completed.ends[1].playing_id, 2);
        Ok(())
    }
    #[test]
    fn queued_mixed_actions_keep_old_emitter_and_voices_alive_until_completion() -> Result<()> {
        fn encoded(target: u32, delay: i32) -> Vec<u8> {
            let mut parameters = vec![0; 12];
            parameters.push(0);
            parameters.extend([0; 16]);
            parameters.extend(0u32.to_be_bytes());
            parameters.extend(7u32.to_be_bytes());
            let mut bytes = [0x4011, target, delay as u32, 0, 0, parameters.len() as u32]
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect::<Vec<_>>();
            bytes.extend(parameters);
            bytes
        }
        struct Voice {
            owner: Option<Arc<RegisteredObject>>,
            playing_id: u32,
        }
        #[derive(Default)]
        struct Voices {
            voices: Vec<Voice>,
            contexts: Vec<audio_play_action::Context>,
            positions: Vec<Vec<super::super::audio_distance::Position>>,
            priorities: Vec<f32>,
            updated_listeners: Vec<[f32; 3]>,
            notifications: usize,
        }
        impl NodeHost for Voices {
            fn execute(
                &mut self,
                action: &Action<'_>,
                context: audio_play_action::Context,
                owner: Option<Arc<RegisteredObject>>,
                listeners: &super::super::audio_listener::Listeners,
                playing: &mut Playing,
                _: &mut dyn audio_schedule::CompletionHost,
                _: &mut dyn FnMut() -> u16,
            ) -> Result<Execution> {
                action.play()?;
                assert_eq!(playing.add_voice(context.playing_id), Some(0x10001));
                assert_eq!(context.object, owner.as_ref().unwrap().handle);
                self.positions.push(
                    owner
                        .as_ref()
                        .unwrap()
                        .emitter_snapshot()?
                        .positions()
                        .to_vec(),
                );
                self.priorities.push(
                    owner
                        .as_ref()
                        .unwrap()
                        .admission_priority(
                            &listeners.distance_records(),
                            60.,
                            -40.,
                            super::super::audio_sound::Radius {
                                found: true,
                                value: 10.,
                            },
                        )?
                        .0
                        .current,
                );
                self.contexts.push(context);
                self.voices.push(Voice {
                    owner,
                    playing_id: context.playing_id,
                });
                Ok(Execution::Play(1))
            }
            fn play_pending(
                &mut self,
                _: &Action<'_>,
                _: audio_play_action::Context,
            ) -> Result<()> {
                self.notifications += 1;
                Ok(())
            }
            fn update(
                &mut self,
                _: u32,
                listeners: &super::super::audio_listener::Listeners,
                _: &mut Playing,
                _: &mut dyn audio_schedule::CompletionHost,
            ) -> Result<()> {
                self.updated_listeners
                    .push(listeners.records()[0].pose.position);
                Ok(())
            }
        }
        let immediate = encoded(17, 0);
        let delayed = encoded(18, 8);
        let order = [3u32, 10, 20, 20]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        let bank = Bank {
            id: 7,
            language: 0,
            feedback: false,
            media: BTreeMap::new(),
            objects: BTreeMap::from([
                (
                    10,
                    Object {
                        kind: 3,
                        payload: &immediate,
                    },
                ),
                (
                    20,
                    Object {
                        kind: 3,
                        payload: &delayed,
                    },
                ),
                (
                    30,
                    Object {
                        kind: 4,
                        payload: &order,
                    },
                ),
            ]),
        };
        let mut events = Events::default();
        events.register_bank(&bank)?;
        let original = events.register_object(42)?;
        let listeners = [super::super::audio_distance::Listener {
            position: [0.; 3],
            scale: 1.,
        }; 8];
        let position = super::super::audio_distance::Position {
            position: [3., 4., 0.],
            orientation: [0., 0., 1.],
        };
        events.enqueue_position(42, position, -1);
        assert!(original.emitter_snapshot()?.positions().is_empty());
        let weak = Arc::downgrade(&original);
        let original_handle = original.handle;
        drop(original);
        let mut nodes = Voices::default();
        let mut completed = Completed::default();
        let id = events.post(30, 42, Some([1, 2, 3]), 0x10001, 9, 11)?;
        assert!(nodes.voices.is_empty());
        let moved = super::super::audio_distance::Position {
            position: [0.; 3],
            ..position
        };
        events.enqueue_position(42, moved, -1);
        events.enqueue_listener_pose(
            0,
            super::super::audio_listener::Pose {
                forward: [0., 0., 1.],
                up: [0., 1., 0.],
                position: [0., 0., 20.],
            },
        );
        let delivered = events.process_commands(&mut nodes, &mut completed, || panic!())?;
        assert_eq!(delivered.events[0].outcome.admitted, 3);
        assert_eq!(nodes.voices.len(), 1);
        assert_eq!(events.pending(), 2);
        assert_eq!(
            delivered.spatial,
            [SpatialResult {
                object: 42,
                command: 8,
                status: 1
            }; 2]
        );
        assert_eq!(nodes.positions[0], [position]);
        // Updating a registered identity is visible to its already active voice
        // as well as the pending actions. It does not copy spatial state.
        assert_eq!(
            nodes.voices[0]
                .owner
                .as_ref()
                .unwrap()
                .minimum_distance(&listeners)?,
            0.
        );
        events.unregister_object(42);
        let replacement = events.register_object(42)?;
        assert_ne!(replacement.handle, original_handle);
        replacement.apply_positions(&[position], 0, -1)?;
        assert!(replacement.minimum_distance(&listeners)? > 0.);
        for _ in 0..3 {
            events.process_block(&mut nodes, &mut completed, || panic!())?;
        }
        assert_eq!(nodes.notifications, 4);
        assert_eq!(nodes.voices.len(), 3);
        assert_eq!(events.pending(), 0);
        assert_eq!(&nodes.positions[1..], &[vec![moved], vec![moved]]);
        assert_eq!(nodes.priorities, [40., 20., 20.]);
        assert_eq!(nodes.updated_listeners, [[0., 0., 20.]; 3]);
        assert_eq!(events.playing.record(id).unwrap().actions, 0);
        assert_eq!(events.playing.record(id).unwrap().voices, 3);
        assert!(completed.ends.is_empty());
        assert!(weak.upgrade().is_some());
        for context in &nodes.contexts {
            assert_eq!(context.object, original_handle);
            assert_eq!(&context.words[..3], &[1, 2, 3]);
        }
        while let Some(voice) = nodes.voices.pop() {
            assert_eq!(voice.owner.as_ref().unwrap().handle, original_handle);
            assert_eq!(
                voice.owner.as_ref().unwrap().minimum_distance(&listeners)?,
                0.
            );
            if let Some(done) = events
                .playing
                .remove_voice(voice.playing_id, &mut completed)
            {
                assert!(events.playing.record(id).is_none());
                if let Some(end) = done.end {
                    audio_schedule::CompletionHost::end_event(&mut completed, end)?;
                }
            }
        }
        assert!(weak.upgrade().is_none());
        assert_eq!(completed.positions, [id]);
        assert_eq!(completed.ends.len(), 1);
        assert_eq!(events.event_references(30), Some(1));
        Ok(())
    }
}
