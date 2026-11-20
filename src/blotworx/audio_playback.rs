//! Native event ingress/consumer connected to immediate direct DSP playback.
//! Non-continuous containers retain authored selection/switch history.
//! Spatialization, buses and full voice management remain separate work.
use super::{
    audio_bank::{Bank, NodePrefix, SoundTail},
    audio_dsp::Media,
};
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeMap, path::Path, sync::Arc};

#[derive(Clone)]
pub struct Clip {
    pub samples: Arc<[i16]>,
    pub rate: u32,
    pub sound: u32,
    pub gain_db: f32,
    pub channel_gains: [f32; 4],
    /// Authored per-object sound cap. Zero is native unlimited admission.
    pub maximum_instances: u16,
    pub lineage: Arc<[u32]>,
    pub pitch_ratio: f32,
    pub low_pass: f32,
    pub pitch_cents:f32,
    pub random_ranges:Arc<[[[f32;2];4]]>,
    pub spatial:Option<Arc<super::audio_spatial::Definition>>,
    pub admission:Option<Arc<super::audio_limits::Definition>>,
    pub layout:Option<super::audio_loop::Layout>,
    pub signal:super::audio_signal::Signal,pub source_passes:u16,
}
type Owner = Arc<super::audio_events::RegisteredObject>;
struct ActiveVoice {
    sound: u32,
    lineage: Arc<[u32]>,
    owner: Option<Owner>,
    playing_id: u32,
    device: super::audio_output::Voice,
    spatial:Option<ActiveSpatial>,
    admission:Option<Arc<super::audio_limits::Definition>>,
    stopping:bool,
}
struct ActiveSpatial {definition:Arc<super::audio_spatial::Definition>,frozen:Option<super::audio_distance::Emitter>,channels:[u32;4],low_pass:u32}
#[derive(Clone)]
enum ImmediateAction {
    Play { target: u32, node: Arc<super::audio_graph::Node> },
    Stop { sound: u32, transition: i32 },
    Mute,
}
struct PendingPlan {
    actions: Arc<[ImmediateAction]>,
    event: String,
    owner: usize,
    reported_failure:std::sync::atomic::AtomicBool,
}
fn owner_key(owner: &Option<Owner>) -> Option<usize> {
    owner.as_ref().map(|owner| owner.handle as usize)
}
#[derive(Debug, serde::Serialize)]
pub struct RejectedPost {
    pub event: String,
    pub owner: usize,
    pub reason: String,
    pub attempts: u64,
}
pub struct Playback {
    objects: super::audio_events::Events,
    pub switch_control_requests: u64,
    pub switch_controls_consumed: u64,
    output: super::audio_output::Output,
    bytes: Arc<[u8]>,
    voices: BTreeMap<u64, ActiveVoice>,
    next: u32,
    next_voice: u64,
    plans: BTreeMap<u32, Result<Arc<[ImmediateAction]>, String>>,
    pending_plans: BTreeMap<u32, PendingPlan>,
    banks: BTreeMap<u32, Arc<[u8]>>,
    event_banks: BTreeMap<u32, u32>,
    pub submitted: u64,
    pub completed: u64,
    pub stop_actions: u64,
    pub stopped_voices: u64,
    pub game_post_requests: u64,
    pub voice_limit_rejections: u64,
    pub stopped_before_render: u64,
    pub last_consumer_event_count: usize,
    pub rejected_posts: BTreeMap<(usize, String), RejectedPost>,
    pub rejected_post_count: u64,
    pub spatial_post_rejections: u64,
    pub spatial_position_requests: u64,
    pub parameter_controls_prepared: u64,
    containers: super::audio_graph::Runtime,
    source_nodes: super::audio_graph::Cache,
    audition_random: super::game_random::State,
    spatial_tables:super::audio_transition::Tables,
    source_positions:BTreeMap<usize,(u32,super::audio_distance::Position)>,
    listener_pose:Option<super::audio_listener::Pose>,
    pub spatial_updates:u64,
    media_root:Option<std::path::PathBuf>,
}
impl Playback {
    pub fn set_media_root(&mut self,root:std::path::PathBuf){self.media_root=Some(root);self.plans.clear();self.source_nodes.clear();}
    pub fn audio_tick(&self)->u32 {self.output.tick()}
    pub fn pending_audio_actions(&self)->usize {self.objects.pending()}
    pub fn load(bytes: Arc<[u8]>) -> Result<Self> {
        Bank::parse(&bytes)?;
        let mut playback = Self {
            objects: Default::default(),
            switch_control_requests: 0,
            switch_controls_consumed: 0,
            output: super::audio_output::Output::default(),
            bytes: bytes.clone(),
            banks: BTreeMap::new(),
            event_banks: BTreeMap::new(),
            voices: BTreeMap::new(),
            next: 0,
            next_voice: 0,
            plans: BTreeMap::new(),
            pending_plans: BTreeMap::new(),
            submitted: 0,
            completed: 0,
            stop_actions: 0,
            stopped_voices: 0,
            game_post_requests: 0,
            voice_limit_rejections: 0,
            stopped_before_render: 0,
            last_consumer_event_count: 0,
            rejected_posts: BTreeMap::new(),
            rejected_post_count: 0,
            spatial_post_rejections: 0,
            spatial_position_requests: 0,
            parameter_controls_prepared: 0,
            containers:Default::default(),source_nodes:Default::default(),audition_random:Default::default(),
            spatial_tables:super::audio_transition::Tables::retail()?,source_positions:Default::default(),listener_pose:None,spatial_updates:0,media_root:None,
        };
        playback.register_bank(bytes)?;
        Ok(playback)
    }
    /// Wwise IDs refer across loaded banks. Keep original bytes shared and
    /// register only banks explicitly requested by the level/part descriptors.
    pub fn register_bank(&mut self, bytes: Arc<[u8]>) -> Result<()> {
        let bank = Bank::parse(&bytes)?;
        let id = bank.id;
        if let Some(existing) = self.banks.get(&id) {
            ensure!(
                existing.as_ref() == bytes.as_ref(),
                "Different audio banks share ID {id:08x}"
            );
            return Ok(());
        }
        let mut event_updates = Vec::new();
        for (&event, object) in &bank.objects {
            if object.kind == 4 {
                if let Some(previous) = self.event_banks.get(&event) {
                    let previous = Bank::parse(&self.banks[previous])?;
                    let other = &previous.objects[&event];
                    ensure!(
                        object.payload == other.payload,
                        "Conflicting loaded audio event {event:08x}"
                    );
                } else {
                    event_updates.push(event);
                }
            }
        }
        self.objects.register_bank(&bank)?;
        self.output.define_mute_nodes(bank.objects.iter().filter_map(|(&id,object)|(object.kind==7).then_some(id)))?;
        self.output.define_sis_nodes(bank.objects.iter().filter(|(_,o)|matches!(o.kind,2|5|6|7|9)).map(|(&id,o)| {
            let definition=(||->Result<super::audio_sis::Definition>{let n=NodePrefix::parse(if o.kind==2{o.source()?.node_parameters}else{o.payload})?;let f=super::audio_parameters::Fixed::load(&n.ranges)?;Ok(super::audio_sis::Definition{id,base:[f.volume as f32/10.,f.filter as f32]})})();
            (id,definition.map_err(|e|format!("{e:#}")))
        }))?;
        for event in event_updates {
            self.event_banks.insert(event, id);
        }
        self.banks.insert(id, bytes);
        // A newly loaded bank can supply cross-bank actions/media which were
        // missing before. Cache immutable decisions only for this bank set.
        self.plans.clear();
        self.source_nodes.clear();
        Ok(())
    }
    fn bank_for_event(&self, event: u32) -> Result<Bank<'_>> {
        let id = self
            .event_banks
            .get(&event)
            .context("Audio event absent from loaded banks")?;
        let mut bank = Bank::parse(&self.banks[id])?;
        for (&other, bytes) in &self.banks {
            if other != *id {
                let loaded = Bank::parse(bytes)?;
                ensure!(
                    loaded.feedback == bank.feedback,
                    "Mixed feedback audio bank layouts require per-node ownership"
                );
                for (id, object) in loaded.objects {
                    bank.objects.entry(id).or_insert(object);
                }
                for (id, media) in loaded.media {
                    bank.media.entry(id).or_insert(media);
                }
            }
        }
        Ok(bank)
    }
    pub fn registered_banks(&self) -> usize {
        self.banks.len()
    }
    pub fn position_owners(&self)->Vec<usize>{self.voices.values().filter(|voice|voice.spatial.as_ref().is_some_and(|s|s.definition.dynamic)).filter_map(|voice|voice.owner.as_ref().map(|owner|owner.id as usize)).collect::<std::collections::BTreeSet<_>>().into_iter().collect()}
    pub fn set_game_position(&mut self,owner:usize,position:[f32;3],orientation:[f32;3])->Result<()> {
        ensure!(position.iter().chain(orientation.iter()).all(|v|v.is_finite()),"Nonfinite game audio pose");
        let object=u32::try_from(owner).context("Audio owner exceeds native ID range")?;let registered=self.objects.register_object(object)?;
        let pose=super::audio_distance::Position{position:[-position[0],position[1],position[2]],orientation:[-orientation[0],orientation[1],orientation[2]]};
        if self.source_positions.get(&owner).is_some_and(|(handle,last)|*handle==registered.handle && *last==pose) {return Ok(());}
        self.source_positions.insert(owner,(registered.handle,pose));self.objects.enqueue_position(object,pose,-1);self.spatial_position_requests+=1;Ok(())
    }
    pub fn set_listener_from_game(&mut self,pose:super::audio_listener::Pose)->Result<()> {
        ensure!(pose.position.iter().chain(pose.forward.iter()).chain(pose.up.iter()).all(|v|v.is_finite()),"Nonfinite game audio listener");
        let same=self.listener_pose.is_some_and(|last|last.position.map(f32::to_bits)==pose.position.map(f32::to_bits) && last.forward.map(f32::to_bits)==pose.forward.map(f32::to_bits) && last.up.map(f32::to_bits)==pose.up.map(f32::to_bits));
        if !same {self.objects.enqueue_listener_pose(0,pose);self.listener_pose=Some(pose);}Ok(())
    }
    pub fn event_summary(&self) -> serde_json::Value {
        serde_json::json!({"post_counter":self.objects.post_counter(),"queued_commands":self.objects.queued(),
            "active_voices":self.voices.len(),"playing_records":self.objects.playing.len(),
            "voice_limit_rejections":self.voice_limit_rejections,"stopped_before_render":self.stopped_before_render,
            "last_consumer_event_count":self.last_consumer_event_count,"command_render_barrier_connected":true,"native_block_clock_connected":false,
            "container_selections":self.containers.selections,"spatial_updates":self.spatial_updates,
            "active_spatial_voices":self.voices.values().filter(|voice|voice.spatial.is_some()).count()})
    }
    /// REL320484 consumes entity/group/value and returns no Lua values.
    /// Queue command4 on this entity's registered audio identity. Group values
    /// must not be stored in the global state-group map.
    pub fn set_switch_from_game(&mut self, owner: usize, group: &str, value: &str) -> Result<u32> {
        let object = u32::try_from(owner).context("Audio owner exceeds object-ID range")?;
        let group = super::audio_bank::event_id(group)?;
        let value = super::audio_bank::event_id(value)?;
        if group == 0 || value == 0 {
            return Ok(15);
        }
        self.objects.register_object(object)?;
        self.objects.enqueue_switch(object, group, value);
        self.switch_control_requests += 1;
        Ok(1)
    }
    pub fn object_switch_value(&self, owner: usize, group: &str) -> Result<Option<u32>> {
        let Some(object) = u32::try_from(owner)
            .ok()
            .and_then(|owner| self.objects.registered_object(owner))
        else {
            return Ok(None);
        };
        object.switch_value(super::audio_bank::event_id(group)?)
    }
    pub fn switch_summary(&self) -> Result<serde_json::Value> {
        Ok(
            serde_json::json!({"requests":self.switch_control_requests,"consumed":self.switch_controls_consumed,
            "objects":self.objects.switch_snapshots()?,"continuous_voice_notifications_connected":false}),
        )
    }
    pub fn poll(&mut self) -> Result<()> {
        // Standalone auditions own a persistent reference seed. Gameplay
        // supplies the World's shared C RNG through poll_with_random instead.
        let mut random=self.audition_random;
        let result=self.poll_with_random(&mut ||random.draw() as u16);self.audition_random=random;result
    }
    pub fn poll_with_random(&mut self,random:&mut dyn FnMut()->u16)->Result<()> {
        self.containers.collect_retired();
        let _phase = self.output.begin_commands()?;
        let mut completion = ControlCompletion::default();
        let finished = self
            .voices
            .iter()
            .filter(|(_, voice)| voice.device.done())
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for id in finished {
            let voice = self.voices.remove(&id).unwrap();
            if voice.stopping {self.stopped_voices+=1;}else {self.completed+=1;}
            if let Some(done) = self
                .objects
                .playing
                .remove_voice(voice.playing_id, &mut completion)
            {
                if let Some(end) = done.end {
                    super::audio_schedule::CompletionHost::end_event(&mut completion, end)?;
                }
            }
        }
        let mut host = ImmediateHost {
            output: &mut self.output,
            voices: &mut self.voices,
            next_voice: &mut self.next_voice,
            pending: &self.pending_plans,
            submitted: &mut self.submitted,
            stop_actions: &mut self.stop_actions,
            stopped: &mut self.stopped_voices,
            stopped_before_render: &mut self.stopped_before_render,
            limited: &mut self.voice_limit_rejections,
            parameter_controls_prepared: &mut self.parameter_controls_prepared,
            containers:&mut self.containers,rejected_posts:&mut self.rejected_posts,rejected_post_count:&mut self.rejected_post_count,
            spatial_tables:&self.spatial_tables,
        };
        // The worker owns audible fade updates. Drain matching manager ticks
        // here for pending-action lifetime/completion accounting, never use
        // gameplay time to advance an audio fade.
        while self.objects.tick()!=host.output.tick() {
            self.objects.process_block(&mut host,&mut completion,&mut *random)?;
        }
        let batch = self
            .objects
            .process_commands(&mut host, &mut completion, &mut *random)?;
        self.pending_plans.clear();
        if !batch.events.is_empty() {
            self.last_consumer_event_count = batch.events.len();
        }
        self.switch_controls_consumed += batch.switches.len() as u64;
        for voice in self.voices.values_mut() {
            if let (Some(spatial),Some(owner))=(&mut voice.spatial,&voice.owner) {
                let emitter=if let Some(frozen)=&spatial.frozen {frozen.clone()}else {owner.emitter_snapshot()?};
                let channels=spatial.definition.channels(&emitter,&self.objects.listeners,&self.spatial_tables)?;
                let bits=channels.map(f32::to_bits);
                if bits!=spatial.channels {self.output.spatial(&voice.device,channels)?;spatial.channels=bits;self.spatial_updates+=1;}
                let low_pass=spatial.definition.low_pass(&emitter,&self.objects.listeners,&self.spatial_tables)?;
                if low_pass.to_bits()!=spatial.low_pass {self.output.low_pass(&voice.device,low_pass)?;spatial.low_pass=low_pass.to_bits();self.spatial_updates+=1;}
            }
        }
        ensure!(
            !completion.unexpected,
            "Immediate audio path received an unsupported source-position callback"
        );
        Ok(())
    }
    pub fn post(&mut self, event: &str) -> Result<u32> {
        self.post_with_owner(event, None)
    }
    pub fn post_on(&mut self, event: &str, owner: usize) -> Result<u32> {
        self.post_with_owner(event, Some(owner))
    }
    /// A mono device preview cannot stand in for Wwise multi-source playback.
    /// Preserve native AK_INVALID_PLAYING_ID and an explicit rejection record
    /// until spatial voice/container/bus ownership is actually implemented.
    pub fn post_spatial_from_game(&mut self, event: &str, owner: usize) -> u32 {
        let rejected=self.rejected_posts.entry((owner,event.to_owned())).or_insert_with(||RejectedPost {
            event:event.into(),owner,reason:"Multi-position playback requires native object registration, spatial voices, containers and buses; the preview backend cannot render this event".into(),attempts:0,
        });
        rejected.attempts += 1;
        self.rejected_post_count += 1;
        self.spatial_post_rejections += 1;
        0
    }
    /// REL31fdd8..31fdfc returns the posting result to Lua. Backend failure is
    /// AK_INVALID_PLAYING_ID (0), not a thrown Lua exception. Aggregate failed
    /// preview attempts; do not claim they were played or replay them later.
    pub fn post_from_game(&mut self, event: &str, owner: usize) -> u32 {
        self.game_post_requests += 1;
        match self.post_on(event, owner) {
            Ok(id) => id,
            Err(error) => {
                let rejected = self
                    .rejected_posts
                    .entry((owner, event.to_owned()))
                    .or_insert_with(|| RejectedPost {
                        event: event.into(),
                        owner,
                        reason: format!("{error:#}"),
                        attempts: 0,
                    });
                rejected.attempts += 1;
                self.rejected_post_count += 1;
                0
            }
        }
    }
    fn post_with_owner(&mut self, event: &str, owner: Option<usize>) -> Result<u32> {
        // Posting enqueues only. Consuming here splits one game-frame command
        // batch into individual renders and races a subsequent Stop.
        self.output.check()?;
        let id = super::audio_bank::event_id(event)?;
        if !self.plans.contains_key(&id) {
            let mut sources=std::mem::take(&mut self.source_nodes);
            let plan=(||->Result<Arc<[ImmediateAction]>> {
                let bank=self.bank_for_event(id)?;let mut actions=Vec::new();
                // Repeated actions are meaningful: retain the authored order.
                for action_id in bank.event_actions(id)? {
                    let action=self.objects.action_definition(action_id).context("Missing registered event action")?.action()?;
                    ensure!(action.delay==[0;3] || (matches!(action.kind,0x6010|0x7010|0xa010|0xb010|0xe010|0xf010) && action.delay[1]==action.delay[2]),"Delayed/randomized audio action is not admitted");
                    if action.kind==0x4011 {
                        let play=action.play()?;
                        ensure!(play.transition==[0;3] && play.exceptions.is_empty() && self.banks.contains_key(&play.bank),
                            "Direct sound has unsupported transition, exception or bank routing");
                        let node=super::audio_graph::prepare_with_root(&bank,&self.banks,*self.event_banks.get(&id).unwrap(),action.target,&mut sources,self.media_root.as_deref())?;
                        actions.push(ImmediateAction::Play{target:action.target,node});
                    }else if matches!(action.kind,0xa010|0xb010|0xe010|0xf010) {
                        let p=action.global_parameter()?;
                        ensure!(p.mode<=2 && p.curve<=8 && p.exceptions.is_empty() && p.transition[1]==p.transition[2] && p.value[1]==p.value[2],"Randomized/exception SIS parameter is not connected");
                        self.output.has_sis_node(action.target)?;
                        if let Some(o)=bank.objects.get(&action.target) {ensure!(matches!(o.kind,2|5|6|7|9),"SIS parameter target class {} is not connected",o.kind);}
                        actions.push(ImmediateAction::Mute);
                    }else if matches!(action.kind,0x6010|0x7010) {
                        let mute=action.global_mute()?;
                        ensure!(mute.exceptions.is_empty() && mute.transition[1]==mute.transition[2],"Randomized/exception mute is not connected");
                        if let Some(node)=bank.objects.get(&action.target) {ensure!(node.kind==7,"Global mute target class {} is not implemented",node.kind);ensure!(mute.curve<=8,"Unimplemented mute curve");}
                        actions.push(ImmediateAction::Mute);
                    }else {
                        let stop=action.object_stop().with_context(||format!("Audio event {event}: full action/container/spatial playback is not connected"))?;
                        ensure!(stop.curve<=8 && stop.exceptions.is_empty() && stop.transition[1]==stop.transition[2],"Randomized/exception stop scheduling is not connected");
                        // A stop targets its authored node and every active
                        // descendant, not a decoded first child. Stopping an
                        // inaudible/unsupported 3D node must not decode media.
                        let sound=action.target;
                        actions.push(ImmediateAction::Stop{sound,transition:stop.transition_time(||unreachable!("Equal transition modifiers do not consume RNG"))?});
                    }
                }
                Ok(actions.into())
            })().map_err(|error|format!("{error:#}"));
            self.source_nodes=sources;
            self.plans.insert(id, plan);
        }
        let plan = self.plans[&id]
            .as_ref()
            .map_err(|error| anyhow::anyhow!(error.clone()))?
            .clone();
        let object = owner
            .map(u32::try_from)
            .transpose()
            .context("Audio owner exceeds object-ID range")?;
        if let Some(object) = object {
            self.objects.register_object(object)?;
        }
        // No callback/position flags are invented. The native producer decides
        // whether a playing-manager record is required. Voice rejection later
        // must not retroactively replace this accepted event ID with zero.
        let playing_id = self
            .objects
            .post(id, object.unwrap_or(u32::MAX), None, 0, 0, 0)?;
        self.next = self.objects.post_counter();
        self.pending_plans.insert(
            playing_id,
            PendingPlan {
                actions: plan,
                event:event.into(),owner:owner.unwrap_or(u32::MAX as usize),
                reported_failure:std::sync::atomic::AtomicBool::new(false),
            },
        );
        Ok(playing_id)
    }
    pub fn write_clip_wav(&self, event: &str, path: &Path) -> Result<()> {
        let bank = Bank::parse(&self.bytes)?;
        let ids = bank.named_event_actions(event)?;
        ensure!(ids.len() == 1, "Audio capture requires one action");
        let clip = resolve_action_source(&bank, bank.objects[&ids[0]].action()?.target)?;
        ensure!(clip.pitch_ratio==1.,"WAV audition export does not yet apply voice pitch");
        ensure!(clip.low_pass==0.,"WAV audition export does not yet apply voice filtering");
        ensure!(clip.random_ranges.iter().flatten().flatten().all(|v|*v==0.),"WAV audition export does not yet apply randomized voice parameters");
        ensure!(clip.spatial.is_none(),"WAV audition export requires a spatial listener/source context");
        ensure!(clip.layout.is_none_or(|layout|layout.passes==1),"Looped WAV audition export requires a chosen duration");
        let samples = scaled(&clip)?;
        let size = u32::try_from(samples.len().checked_mul(2).context("PCM size overflow")?)?;
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(
            &size
                .checked_add(36)
                .context("WAV size overflow")?
                .to_le_bytes(),
        );
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&clip.rate.to_le_bytes());
        wav.extend_from_slice(&(clip.rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&size.to_le_bytes());
        for sample in samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, wav)?;
        Ok(())
    }
}
/// Concrete PCM host for the currently admitted immediate direct-source scope.
/// The event consumer supplies retained object identities and playing IDs;
/// the Play dispatcher supplies the native request. Unsupported containers,
/// processing blocks and continuous switches are still admission errors.
struct ImmediateHost<'a> {
    output: &'a mut super::audio_output::Output,
    voices: &'a mut BTreeMap<u64, ActiveVoice>,
    next_voice: &'a mut u64,
    pending: &'a BTreeMap<u32, PendingPlan>,
    submitted: &'a mut u64,
    stop_actions: &'a mut u64,
    stopped: &'a mut u64,
    stopped_before_render: &'a mut u64,
    limited: &'a mut u64,
    parameter_controls_prepared: &'a mut u64,
    containers:&'a mut super::audio_graph::Runtime,
    rejected_posts:&'a mut BTreeMap<(usize,String),RejectedPost>,
    rejected_post_count:&'a mut u64,
    spatial_tables:&'a super::audio_transition::Tables,
}
struct DirectPlay<'a, 'b> {
    host: &'a mut ImmediateHost<'b>,
    owner: Option<Owner>,
    plan: Arc<[ImmediateAction]>,
    playing_id:u32,
    listeners:&'a super::audio_listener::Listeners,
    playing:&'a mut super::audio_playing::Playing,
    completion:&'a mut dyn super::audio_schedule::CompletionHost,
}
impl super::audio_play_action::Control for DirectPlay<'_, '_> {
    type Handle = u32;
    fn acquire_play_node(&mut self, id: u32) -> Result<Option<u32>> {
        Ok(self
            .plan
            .iter()
            .any(|action| matches!(action,ImmediateAction::Play{target,..} if *target==id))
            .then_some(id))
    }
    fn play_node(
        &mut self,
        node: u32,
        request: &super::audio_play_action::Request,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<u32> {
        ensure!(
            request.kind == 0 && request.transition == 0 && request.context.sample_offset == 0,
            "Unsupported direct voice request"
        );
        let graph = self
            .plan
            .iter()
            .find_map(|action| match action {
                ImmediateAction::Play { target, node:graph } if *target == node => Some(graph.clone()),
                _ => None,
            })
            .context("Prepared sound missing")?;
        let mut containers=std::mem::take(self.host.containers);
        if let Some(owner)=&self.owner {containers.retain_owner(owner);}
        let result=containers.play(&graph,request,owner_key(&self.owner),random,self);
        *self.host.containers=containers;result
    }
    fn release_play_node(&mut self, _: u32) {}
}
impl super::audio_graph::Sink for DirectPlay<'_, '_> {
    fn switch_value(&mut self,group:u32)->Result<Option<u32>> {
        self.owner.as_ref().map(|owner|owner.switch_value(group)).unwrap_or(Ok(None))
    }
    fn reject(&mut self,error:&str) {
        // A valid posted event can fail later at its selected voice. Preserve
        // the accepted playing ID and diagnose this exact event/owner.
        if let Some(pending)=self.host.pending.get(&self.playing_id) {
            if pending.reported_failure.swap(true,std::sync::atomic::Ordering::Relaxed) {return;}
            let rejected=self.host.rejected_posts.entry((pending.owner,pending.event.clone())).or_insert_with(||RejectedPost{event:pending.event.clone(),owner:pending.owner,reason:error.into(),attempts:0});
            rejected.reason=error.into();rejected.attempts+=1;*self.host.rejected_post_count+=1;
        }
    }
    fn sound(&mut self,clip:Arc<Clip>,request:&super::audio_play_action::Request,random:&mut dyn FnMut()->u16)->Result<u32> {
        if let Some(definition)=&clip.admission {
            let limit=definition.limit;let key=owner_key(&self.owner);
            let count=self.host.voices.values().filter(|v|owner_key(&v.owner)==key && v.admission.as_ref().is_some_and(|a|a.limit.node==limit.node)).count();
            if limit.maximum!=0 && count>=usize::from(limit.maximum) {
                let owner=self.owner.as_ref().context("Sound admission lost its registered object")?.emitter_snapshot()?;
                let incoming=definition.priority(&owner,self.listeners)?.current;
                let mut candidates=Vec::new();
                for (&id,voice) in self.host.voices.iter() {
                    if !voice.stopping && owner_key(&voice.owner)==key && voice.lineage.contains(&limit.node) {
                        let admission=voice.admission.as_ref().context("Limited voice has no priority definition")?;
                        let emitter=voice.owner.as_ref().context("Limited voice lost its registered object")?.emitter_snapshot()?;
                        candidates.push((id,admission.priority(&emitter,self.listeners)?.current));
                    }
                }
                let Some(id)=super::audio_limits::victim(incoming,limit.kill_newest,candidates) else {*self.host.limited+=1;return Ok(3)};
                let voice=self.host.voices.remove(&id).unwrap();*self.host.stopped+=1;
                if voice.device.rendered_frames()==0 {*self.host.stopped_before_render+=1;}
                if let Some(done)=self.playing.remove_voice(voice.playing_id,self.completion) {if let Some(end)=done.end {self.completion.end_event(end)?;}}
            }
        }else if clip.maximum_instances!=0 && self.host.voices.values().filter(|voice| {
            same_voice_scope(
                voice.sound,
                owner_key(&voice.owner),
                clip.sound,
                owner_key(&self.owner),
            )
        }).count()>=usize::from(clip.maximum_instances) {
            *self.host.limited += 1;
            return Ok(3);
        }
        let offsets=super::audio_parameters::random_offsets(&clip.random_ranges,random)?;
        if offsets[1]!=0. {super::audio_graph::Sink::reject(self,"Randomized LFE playback is not connected");return Ok(2);}
        let clip=if offsets.iter().all(|v|*v==0.) {clip}else {
            let mut instance=(*clip).clone();instance.gain_db+=offsets[0];instance.pitch_cents+=offsets[2];instance.low_pass+=offsets[3];
            instance.pitch_ratio=super::audio_parameters::pitch_ratio(instance.pitch_cents)?;Arc::new(instance)
        };
        let clip=if let Some(layout)=clip.signal.silence_layout(clip.rate,clip.source_passes,random)? {let mut instance=(*clip).clone();instance.layout=Some(layout);Arc::new(instance)}else {clip};
        let id = self
            .host
            .next_voice
            .checked_add(1)
            .context("Audio voice identity exhausted")?;
        let spatial=if let Some(definition)=&clip.spatial {
            let emitter=self.owner.as_ref().context("Spatial sound lost its retained owner")?.emitter_snapshot()?;
            let channels=match definition.channels(&emitter,self.listeners,self.host.spatial_tables) {Ok(channels)=>channels,Err(error)=>{super::audio_graph::Sink::reject(self,&format!("{error:#}"));return Ok(2);}};
            let low_pass=definition.low_pass(&emitter,self.listeners,self.host.spatial_tables)?;
            Some((channels,ActiveSpatial{definition:definition.clone(),frozen:(!definition.dynamic).then_some(emitter),channels:channels.map(f32::to_bits),low_pass:low_pass.to_bits()}))
        }else {None};
        let device = self.host.output.start_spatial_filtered(&clip,spatial.as_ref().map(|(channels,_)|*channels),spatial.as_ref().map_or(0.,|(_,s)|f32::from_bits(s.low_pass)))?;
        *self.host.next_voice = id;
        self.host.voices.insert(
            id,
            ActiveVoice {
                sound: clip.sound,
                lineage: clip.lineage.clone(),
                owner: self.owner.clone(),
                playing_id: request.context.playing_id,
                device,
                spatial:spatial.map(|(_,state)|state),
                admission:clip.admission.clone(),
                stopping:false,
            },
        );
        self.playing.add_voice(request.context.playing_id);
        *self.host.submitted += 1;
        Ok(1)
    }
}
impl super::audio_events::NodeHost for ImmediateHost<'_> {
    fn prepare_parameter(&mut self,action:&super::audio_bank::Action<'_>,due:u32)->Result<()> {
        if matches!(action.kind,0xa010|0xb010|0xe010|0xf010) {
            let p=action.global_parameter()?;let known=self.output.has_sis_node(action.target)?;
            if !known && action.delay_samples(||unreachable!("No random SIS delay"))?==0 {return Ok(());}
            if known {*self.parameter_controls_prepared+=1;}
            let resetting=matches!(action.kind,0xb010|0xf010);
            let duration=p.transition_time(||unreachable!("No random SIS duration"))?;
            return self.output.sis(super::audio_sis::Command{node:action.target,property:super::audio_sis::Property::from_action(action.kind)?,mode:if resetting {0}else {p.mode},value:if resetting {0.}else {p.randomized_value(||unreachable!("No random SIS value"))?},curve:p.curve,duration,due});
        }
        let mute=action.global_mute()?;
        if !self.output.has_mute_node(action.target) && action.delay_samples(||unreachable!("No random mute delay"))?==0 {return Ok(());}
        if self.output.has_mute_node(action.target) {*self.parameter_controls_prepared+=1;}
        self.output.mute(super::audio_mute_mix::Command{node:action.target,muting:action.kind==0x6010,curve:mute.curve,duration:mute.transition_time(||unreachable!("No random mute transition"))?,due})
    }
    fn switch_changed(&mut self, _: Owner, _: u32, _: u32) -> Result<()> {
        Ok(())
    } // No admitted continuous voices yet.
    fn execute(
        &mut self,
        action: &super::audio_bank::Action<'_>,
        context: super::audio_play_action::Context,
        owner: Option<Owner>,
        listeners: &super::audio_listener::Listeners,
        playing: &mut super::audio_playing::Playing,
        completion: &mut dyn super::audio_schedule::CompletionHost,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<super::audio_schedule::Execution> {
        use super::audio_schedule::Execution;
        if matches!(action.kind,0xa010|0xb010|0xe010|0xf010) {
            return Ok(Execution::Parameter(if self.output.has_sis_node(action.target)? {super::audio_control::Delivery::Applied}else {super::audio_control::Delivery::MissingTarget}));
        }else if matches!(action.kind,0x6010|0x7010) {
            // Audible execution was prepared for its exact worker deadline.
            // This return completes the existing native manager accounting.
            Ok(Execution::Parameter(if self.output.has_mute_node(action.target){super::audio_control::Delivery::Applied}else{super::audio_control::Delivery::MissingTarget}))
        }else if action.kind == 0x4011 {
            let plan = self
                .pending
                .get(&context.playing_id)
                .context("Missing queued audio plan")?
                .actions
                .clone();
            let result = super::audio_play_action::execute_due(
                action,
                context,
                &mut DirectPlay {
                    host: self,
                    owner,
                    plan,
                    playing_id:context.playing_id,
                    listeners,
                    playing,completion,
                },
                random,
            )?;
            Ok(Execution::Play(result))
        } else {
            let stop = action.object_stop()?;
            let transition = stop.transition_time(random)?;
            let key = owner_key(&owner);
            let matches = self
                .voices
                .iter()
                .filter(|(_, voice)| {
                    voice.lineage.contains(&action.target) && stop_matches(owner_key(&voice.owner), key)
                })
                .map(|(&id, _)| id)
                .collect::<Vec<_>>();
            for id in matches {
                if transition!=0 {let voice=self.voices.get_mut(&id).unwrap();self.output.stop(&voice.device,stop.curve,transition)?;voice.stopping=true;continue;}
                let voice = self.voices.remove(&id).unwrap();
                *self.stopped += 1;
                if voice.device.rendered_frames() == 0 {
                    *self.stopped_before_render += 1;
                }
                if let Some(done) = playing.remove_voice(voice.playing_id, completion) {
                    if let Some(end) = done.end {
                        completion.end_event(end)?;
                    }
                }
            }
            *self.stop_actions += 1;
            Ok(Execution::Stop(1))
        }
    }
    fn update(
        &mut self,
        _: u32,
        _: &super::audio_listener::Listeners,
        _: &mut super::audio_playing::Playing,
        _: &mut dyn super::audio_schedule::CompletionHost,
    ) -> Result<()> {
        Ok(()) // Audible transitions are updated by the audio worker.
    }
}
#[derive(Default)]
struct ControlCompletion {
    unexpected: bool,
}
impl super::audio_playing::CompletionHost for ControlCompletion {
    fn remove_source_position(&mut self, _: u32) {
        self.unexpected = true;
    }
}
impl super::audio_schedule::CompletionHost for ControlCompletion {
    fn end_event(&mut self, _: super::audio_playing::EndEvent) -> Result<()> {
        anyhow::bail!("Immediate PCM host received an unsupported event callback")
    }
}
/// Native voice traversal uses a null object filter as a wildcard, rather
/// than limiting the request to voices posted without an owner.
fn stop_matches(voice_owner: Option<usize>, requested: Option<usize>) -> bool {
    requested.is_none() || voice_owner == requested
}
fn same_voice_scope(
    sound: u32,
    owner: Option<usize>,
    requested_sound: u32,
    requested_owner: Option<usize>,
) -> bool {
    sound == requested_sound && owner == requested_owner
}
fn resolve_direct(bank: &Bank<'_>, sound: u32) -> Result<Clip> {
    let (mut clip,bytes)=prepare_direct(bank,sound)?;
    clip.samples=Media::parse(bytes)?.decode()?.into();Ok(clip)
}
pub(super) fn prepare_direct<'a>(bank:&Bank<'a>,sound:u32)->Result<(Clip,&'a [u8])> {
    let source=bank.objects.get(&sound).context("Missing sound")?.source()?;
    ensure!(source.plugin==0x50001 && source.stream_type==0 && source.flags==0,"Unsupported direct DSP source");
    let (mut clip,tail)=prepare_parameters(bank,sound)?;
    let bytes=*bank.media.get(&source.media_id).context("Missing embedded sound media")?;let media=Media::parse(bytes)?;
    ensure!(media.sample_rate==clip.rate,"Sound sample rate disagrees with media");
    clip.layout=Some(super::audio_loop::Layout::new(media.sample_count as usize,media.loop_points,tail.loops[0] as u16)?);Ok((clip,bytes))
}
pub(super) fn prepare_unbacked(bank:&Bank<'_>,sound:u32,root:Option<&Path>)->Result<Clip> {
    let source=bank.objects.get(&sound).context("Missing sound")?.source()?;ensure!(source.flags==0,"Unsupported source flags");
    let (mut clip,_)=prepare_parameters(bank,sound)?;
    clip.signal=if source.plugin==0x650002 && source.stream_type==0 {
        let bytes=source.plugin_parameters.context("Silence initialization missing")?;ensure!(bytes.len()==12,"Invalid silence parameter size");
        let p=std::array::from_fn(|i|f32::from_bits(u32::from_be_bytes(bytes[i*4..i*4+4].try_into().unwrap())));ensure!(p.iter().all(|v|v.is_finite()),"Nonfinite silence parameters");super::audio_signal::Signal::Silence(p)
    }else if source.plugin==0x50001 && source.stream_type==1 {
        ensure!(bank.language==0,"Localized external audio resolution is not connected");
        super::audio_signal::Signal::FilePending(root.context("External audio root is not configured")?.join(format!("{}.wav",source.file_id)))
    }else {anyhow::bail!("Unsupported source plugin {:08x}/stream{}",source.plugin,source.stream_type)};
    Ok(clip)
}
fn prepare_parameters(bank:&Bank<'_>,sound:u32)->Result<(Clip,SoundTail)> {
    let parameters = bank.parameter_chain(sound)?;
    // Full virtual/bus policy remains separate. Preserve authored settings;
    // ordinary per-object limits and stealing run during voice admission.
    let _policy =
        super::audio_bank::positioning::virtual_policy(parameters.iter().map(|node| &node.voices))?;
    let position = parameters
        .iter()
        .find(|node| node.position != super::audio_bank::positioning::Positioning::Inherited)
        .context("Audio root's default positioning needs its native constructor path")?;
    let spatial=if let super::audio_bank::positioning::Positioning::Local{mode:mode@super::audio_bank::positioning::Mode::ThreeD{..},..}=&position.position {Some(Arc::new(super::audio_spatial::Definition::from_mode(bank,mode)?))}else {None};
    let source = bank
        .objects
        .get(&sound)
        .context("Missing sound node")?
        .source()?;
    let node = NodePrefix::parse(source.node_parameters)?;
    let (_, _, tail) = node.positioning_voice_settings()?;
    let admission=Arc::new(super::audio_limits::Definition::from_bank(bank,sound)?);
    let tail = SoundTail::parse(tail, bank.feedback)?;
    ensure!(
        tail.loops[0]>=0 && tail.loops[1..]==[0,0]
            && tail.states.is_empty()
            && tail.rtpc.is_empty()
            && tail.feedback.is_none(),
        "Unsupported direct sound loop/state/RTPC/feedback"
    );
    let mut gain_db = 0.;
    let mut pitch = 0.;
    let mut low_pass = 0.;
    let mut random_ranges=Vec::new();
    let mut parent = node.parent;
    let mut visited = std::collections::BTreeSet::new();
    let mut accumulate = |node: &NodePrefix<'_>| -> Result<()> {
        ensure!(
            node.state_group == 0,
            "State-dependent sound parameters are not connected"
        );
        ensure!(node.ranges.iter().flatten().all(|v|v.is_finite()),"Nonfinite sound parameters");
        let fixed=super::audio_parameters::Fixed::load(&node.ranges)?;
        ensure!(fixed.lfe==0,"LFE playback is not connected");
        gain_db += fixed.volume as f32 / 10.;
        pitch += fixed.pitch as f32;
        low_pass += fixed.filter as f32;
        random_ranges.push(node.ranges.map(|v|[v[1],v[2]]));
        Ok(())
    };
    accumulate(&node)?;
    while parent != 0 {
        ensure!(visited.insert(parent), "Audio parent cycle");
        let object = bank.objects.get(&parent).context("Missing sound parent")?;
        ensure!(
            matches!(object.kind, 5 | 6 | 7 | 9),
            "Unsupported sound parent kind {}",
            object.kind
        );
        let node = NodePrefix::parse(object.payload)?;
        let (_, _, remaining) = node.positioning_voice_settings()?;
        let (tail, _) = super::audio_bank::NodeTail::parse(remaining, bank.feedback)?;
        ensure!(
            tail.states.is_empty() && tail.rtpc.is_empty() && tail.feedback.is_none(),
            "Parent state/RTPC/feedback playback is not connected"
        );
        accumulate(&node)?;
        parent = node.parent;
    }
    let super::audio_bank::positioning::Positioning::Local { pan, .. } = &position.position else {
        unreachable!()
    };
    let matrix = super::audio_panner::from_authored(false, 1, *pan)?;
    Ok((Clip {
        samples: Arc::from([]),
        rate: source.sample_rate,
        sound,
        gain_db,
        channel_gains: matrix[0],
        maximum_instances: if admission.limit.node==sound {admission.limit.maximum}else {0},
        lineage: parameters.iter().map(|node|node.id).collect::<Vec<_>>().into(),
        pitch_ratio: super::audio_parameters::pitch_ratio(pitch)?,
        low_pass,
        pitch_cents:pitch,random_ranges:random_ranges.into(),spatial,admission:Some(admission),layout:None,signal:super::audio_signal::Signal::Pcm,source_passes:tail.loops[0] as u16,
    },tail))
}

/// Static WAV/audition helper only. Actual gameplay uses audio_graph's
/// command-consumer selection and never substitutes a playable sibling.
fn resolve_action_source(bank: &Bank<'_>, root: u32) -> Result<Clip> {
    fn visit(
        bank: &Bank<'_>,
        id: u32,
        seen: &mut std::collections::BTreeSet<u32>,
        depth: usize,
    ) -> Result<Clip> {
        anyhow::ensure!(depth <= 32, "Audio container nesting exceeds 32 nodes");
        anyhow::ensure!(seen.insert(id), "Audio container cycle at {id:08x}");
        let object = bank
            .objects
            .get(&id)
            .with_context(|| format!("Missing audio child {id:08x}"))?;
        if object.kind == 2 {
            return resolve_direct(bank, id);
        }
        let children = match object.kind {
            5 => {
                let container = object.random_sequence(bank.feedback)?;
                let mut ids = container
                    .playlist
                    .iter()
                    .filter(|item| item.weight != 0)
                    .map(|item| item.sound)
                    .collect::<Vec<_>>();
                ids.extend(container.children);
                ids
            }
            6 => {
                let container = object.switch_container(bank.feedback)?;
                let groups = container.runtime_groups();
                let preferred = groups
                    .iter()
                    .find(|group| group.id == container.default)
                    .into_iter()
                    .flat_map(|group| group.nodes.iter().copied());
                preferred.chain(container.children).collect()
            }
            7 => object.actor_mixer(bank.feedback)?.children,
            kind => anyhow::bail!("Audio Play target {id:08x} has unsupported node class {kind}"),
        };
        let mut failures = Vec::new();
        for child in children {
            let mut branch = seen.clone();
            match visit(bank, child, &mut branch, depth + 1) {
                Ok(clip) => return Ok(clip),
                Err(error) => failures.push(format!("{child:08x}: {error:#}")),
            }
        }
        anyhow::bail!(
            "Audio container {id:08x} has no playable sound: {}",
            failures.join("; ")
        )
    }
    visit(bank, root, &mut std::collections::BTreeSet::new(), 0)
}
fn scaled(clip: &Clip) -> Result<Vec<i16>> {
    // Pre-pan mono export uses the same output gain conversion as playback.
    // Host PCM quantization remains unverified against the original AX mixer.
    let gain = super::audio_panner::output_gain(clip.gain_db)?;
    Ok(clip
        .samples
        .iter()
        .take(clip.layout.map_or(clip.samples.len(),|layout|layout.samples))
        .map(|&sample| {
            (sample as f32 * gain)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16
        })
        .collect())
}

#[cfg(debug_assertions)]
pub(super) fn validate_mutes()->Result<()> {
    super::audio_mute_mix::validate()?;
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let bytes=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
    let mut playback=Playback::load(bytes.into())?;
    playback.post_on("Mute_Mickey",7)?;playback.post_on("Unmute_Mickey",7)?;playback.poll()?;
    ensure!(playback.objects.pending()==1,"Original delayed unmute was not retained");
    let before=playback.objects.tick();
    std::thread::sleep(std::time::Duration::from_millis(30));
    ensure!(playback.output.tick()>before,"Mute audio clock stopped without gameplay polling/voices");
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(4);
    while (playback.output.tick()<833 || playback.objects.pending()!=0) && std::time::Instant::now()<deadline {
        playback.poll()?;std::thread::sleep(std::time::Duration::from_millis(5));
    }
    playback.poll()?;
    ensure!(playback.objects.pending()==0 && playback.output.tick()>=833,"Original delayed unmute/fade never completed");
    ensure!(playback.submitted==0 && playback.rejected_post_count==0,"Mute event fabricated a sound voice or was rejected");
    println!("PASS: original Mute_Mickey/Unmute_Mickey actions run on worker clock without voices; delayed action drained; tick={}",playback.output.tick());Ok(())
}
#[cfg(all(debug_assertions,feature="blotworx-ui-render"))]
pub(super) fn validate_animation_audio()->Result<()> {
    super::audio_parameters::validate()?;
    super::controller_feedback::validate_payloads()?;
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut graph=super::behavior::Document::default();
    graph.objects.insert("sound".into(),super::behavior::Object{klass:"hkbNamedStringEventPayload".into(),fields:serde_json::json!({"name":"SoundFX","data":"Play_sfx_mickey_run"})});
    graph.objects.insert("list".into(),super::behavior::Object{klass:"hkbEventPayloadList".into(),fields:serde_json::json!({"payloads":[{"ref":"sound"}]})});
    let reference=super::state_machine::Reference{id:"list".into()};
    let source=super::controller_feedback::audio_source(&graph,Some(&reference))?.context("Named audio source not selected")?;
    let bytes=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
    let mut playback=Playback::load(bytes.into())?;
    let id=playback.post_from_game(source,0);
    ensure!(id!=0,"Original named animation sound rejected: {:?}",playback.rejected_posts.values().collect::<Vec<_>>());
    ensure!(playback.post_from_game(source,0)!=0,"Second unlimited sound was not admitted");
    let plan=playback.pending_plans[&id].actions.clone();
    playback.poll()?;
    ensure!(playback.submitted==2 && playback.voice_limit_rejections==0,"Native zero-limit sounds must overlap on the same owner");
    fn find(node:&super::audio_graph::Node,sound:u32)->Option<&super::audio_graph::Sound> {
        use super::audio_graph::Node;
        match node {Node::Sound(source) if source.metadata.sound==sound=>Some(source),Node::Selector{children,..}|Node::Switch{children,..}=>children.values().find_map(|node|find(node,sound)),_=>None}
    }
    let sound=playback.voices.values().next().context("No selected animation sound")?.sound;
    let original=plan.iter().find_map(|action|match action {ImmediateAction::Play{node,..}=>find(node,sound),_=>None}).context("Selected sound not in prepared graph")?.decode()?;
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);
    let mut rendered=0;
    while std::time::Instant::now()<deadline {
        rendered=playback.voices.values().map(|voice|voice.device.rendered_frames()).sum::<u64>();
        if rendered>0 {break;}
        playback.output.check()?;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    ensure!(rendered>0,"Named animation audio never reached host mixing");
    // Explicit endpoint fixture: apply a recovered -1200-cent ratio to shared
    // original PCM. This is not an authored-event or native-SRC parity claim.
    let mut pitched=(*original).clone();pitched.pitch_ratio=super::audio_parameters::pitch_ratio(-1200.)?;
    let pitched_voice=playback.output.start(&pitched)?;
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);
    while pitched_voice.rendered_frames()==0 && std::time::Instant::now()<deadline {
        playback.output.check()?;std::thread::sleep(std::time::Duration::from_millis(10));
    }
    ensure!(pitched_voice.rendered_frames()>0,"Pitched PCM fixture did not reach host output");
    println!("PASS: pitched endpoint fixture shares original PCM and mixes at recovered ratio {}",pitched.pitch_ratio);
    graph.objects.insert("facing".into(),super::behavior::Object{klass:"hkbNamedRealEventPayload".into(),fields:serde_json::json!({"name":"Facing_X","data":1.})});
    graph.objects.get_mut("list").unwrap().fields=serde_json::json!({"payloads":[{"ref":"sound"},{"ref":"facing"}]});
    ensure!(super::controller_feedback::audio_source(&graph,Some(&reference)).is_err(),"Unimplemented facing metadata silently dropped");
    println!("PASS: original run sound selected from named list, admitted and mixed by host endpoint ({rendered} frames); unsupported spatial facing diagnosed");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original bank and live Windows audio endpoint"]
    fn original_device_mickey_mutes_advance_without_gameplay_polling()->Result<()> {validate_mutes()}
    #[test]
    #[ignore = "requires original bank"]
    fn original_mickey_mute_event_parameters()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;let bank=Bank::parse(&bytes)?;
        for event in ["Mute_Mickey","Unmute_Mickey"] {
            for id in bank.named_event_actions(event)? {
                let action=bank.objects[&id].action()?;
                let mute=action.global_mute()?;
                println!("{event} kind={:x} target={:x} class={:?} transition={:?} curve={} delay={:?}",action.kind,action.target,bank.objects.get(&action.target).map(|object|object.kind),mute.transition,mute.curve,action.delay);
            }
        }
        Ok(())
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "requires original bank and live audio device"]
    fn original_device_parent_stop_matches_descendants_and_preserves_other_owner()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;let bank=Bank::parse(&bytes)?;
        let action=bank.named_event_actions(super::super::brush_runtime::DRY_SOUND_EVENT)?[0];let clip=resolve_direct(&bank,bank.objects[&action].action()?.target)?;
        let parent=*clip.lineage.get(1).context("Original sound has no parent")?;
        let name="Blotworx_ParentStop";let mut fixture=composed_bank(&bytes,name,&[0x7df00101],0)?;
        let view=Bank::parse(&fixture)?;let offset=view.objects[&0x7df00101].payload.as_ptr() as usize-fixture.as_ptr() as usize;drop(view);
        fixture[offset+4..offset+8].copy_from_slice(&parent.to_be_bytes());let mut playback=Playback::load(fixture.into())?;
        playback.post_on(super::super::brush_runtime::DRY_SOUND_EVENT,7)?;playback.post_on(super::super::brush_runtime::DRY_SOUND_EVENT,9)?;playback.post_on(name,7)?;playback.poll()?;
        ensure!(playback.submitted==2 && playback.stopped_voices==1 && playback.voices.len()==1,"Parent stop failed descendant/owner matching");
        assert_eq!(playback.voices.values().next().unwrap().owner.as_ref().unwrap().id,9);Ok(())
    }
    #[test]
    #[ignore = "requires original game bank"]
    fn original_walk_admission_parameters() -> Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank=Bank::parse(&bytes)?;
        for sound in [0x1c04709c,0x3d60048d] {
            println!("Original walk {sound:08x}: {:?}",bank.parameter_chain(sound)?);
            for entry in bank.parameter_chain(sound)? {
                let object=&bank.objects[&entry.id];
                let node=NodePrefix::parse(if object.kind==2 {object.source()?.node_parameters}else{object.payload})?;
                println!("parameters {:08x}: {:?}",entry.id,node.ranges);
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn owned_playback_switch_controls_preserve_distinct_entities_and_casefolded_group_ids(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let mut playback = Playback::load(bytes.into())?;
        assert_eq!(playback.set_switch_from_game(0, "Surface", "Wood")?, 1);
        assert_eq!(playback.set_switch_from_game(7, "SURFACE", "Metal")?, 1);
        assert_eq!(playback.object_switch_value(0, "surface")?, None);
        playback.poll()?;
        assert_eq!(
            playback.object_switch_value(0, "surface")?,
            Some(super::super::audio_bank::event_id("wood")?)
        );
        assert_eq!(
            playback.object_switch_value(7, "surface")?,
            Some(super::super::audio_bank::event_id("metal")?)
        );
        assert_eq!(playback.switch_controls_consumed, 2);
        assert_eq!(playback.submitted, 0);
        assert_eq!(playback.registered_banks(), 1);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 audio banks"]
    fn mickey_run_and_walk_events_resolve_container_targets_to_playable_sources() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        for event in ["Play_sfx_mickey_run", "Play_sfx_mickey_walk"] {
            let actions = bank.named_event_actions(event)?;
            ensure!(actions.len() == 1, "Unexpected {event} action count");
            let target = bank.objects[&actions[0]].action()?.target;
            let clip = resolve_action_source(&bank, target)
                .with_context(|| format!("Resolving {event} target {target:08x}"))?;
            ensure!(
                !clip.samples.is_empty() && clip.rate > 0,
                "Resolved an empty {event} source"
            );
            eprintln!(
                "{event}: action target {target:08x} -> source {:08x}, {} samples @ {} Hz",
                clip.sound,
                clip.samples.len(),
                clip.rate
            );
        }
        Ok(())
    }
    #[test]
    fn voice_limits_use_exact_object_keys_from_original_instructions() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_scope.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let owner = case["owner"].as_u64().unwrap() as usize;
            let before = case["before"].as_array().unwrap();
            let after = case["after"].as_array().unwrap();
            let mut selected = 0;
            for (before, after) in before.iter().zip(after) {
                let key = before[0].as_u64().unwrap() as usize;
                let matches = same_voice_scope(12, Some(key), 12, Some(owner));
                selected += usize::from(matches);
                assert_eq!(
                    after[1].as_u64().unwrap(),
                    before[1].as_u64().unwrap() + u64::from(matches)
                );
                assert_eq!(before[2], after[2]);
            }
            assert_eq!(selected, usize::from(case["existing"].as_bool().unwrap()));
        }
        assert!(!same_voice_scope(12, Some(7), 13, Some(7)));
        assert!(
            !same_voice_scope(12, None, 12, Some(0)),
            "Unowned posts must not alias entity zero"
        );
        assert!(same_voice_scope(12, None, 12, None));
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_device_distinct_owners_have_independent_dry_voices() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let mut playback = Playback::load(bytes.into())?;
        let first = playback.post_on(super::super::brush_runtime::DRY_SOUND_EVENT, 7)?;
        assert!(first > 0);
        let limited = playback.post_on(super::super::brush_runtime::DRY_SOUND_EVENT, 7)?;
        assert!(
            limited > first,
            "Accepted event IDs survive voice rejection"
        );
        let second = playback.post_on(super::super::brush_runtime::DRY_SOUND_EVENT, 9)?;
        assert!(second > limited);
        assert_eq!(playback.submitted, 0);
        assert_eq!(playback.objects.queued(), 3);
        playback.poll()?;
        assert_eq!(playback.last_consumer_event_count, 3);
        assert_eq!(playback.voices.len(), 2);
        assert_eq!(playback.voice_limit_rejections, 1);
        let a = playback
            .voices
            .values()
            .find(|voice| voice.playing_id == first)
            .unwrap();
        let b = playback
            .voices
            .values()
            .find(|voice| voice.playing_id == second)
            .unwrap();
        assert_eq!(a.owner.as_ref().map(|owner| owner.id), Some(7));
        assert_eq!(b.owner.as_ref().map(|owner| owner.id), Some(9));
        assert_eq!(a.sound, b.sound);
        assert_eq!(playback.submitted, 2);
        Ok(())
    }
    /// Compose retail action payloads in memory. Original bank files remain
    /// immutable; this is an integration scenario, not a retail event claim.
    fn composed_bank(
        bytes: &[u8],
        name: &str,
        order: &[u32],
        stop_duration: i32,
    ) -> Result<Vec<u8>> {
        let bank = Bank::parse(bytes)?;
        let dry = bank.named_event_actions(super::super::brush_runtime::DRY_SOUND_EVENT)?[0];
        let mut stop = bank.objects[&bank.named_event_actions("Stop_sfx_projector_portal")?[0]]
            .payload
            .to_vec();
        stop[4..8].copy_from_slice(&bank.objects[&dry].action()?.target.to_be_bytes());
        stop[24..28].copy_from_slice(&stop_duration.to_be_bytes());
        let mut event = (order.len() as u32).to_be_bytes().to_vec();
        for id in order {
            event.extend(id.to_be_bytes());
        }
        let added = [
            (0x7df00101, 3u32, stop),
            (super::super::audio_bank::event_id(name)?, 4, event),
        ];
        let mut hirc = ((bank.objects.len() + added.len()) as u32)
            .to_be_bytes()
            .to_vec();
        for (&id, object) in &bank.objects {
            hirc.extend(object.kind.to_be_bytes());
            hirc.extend(((object.payload.len() + 4) as u32).to_be_bytes());
            hirc.extend(id.to_be_bytes());
            hirc.extend(object.payload);
        }
        for (id, kind, payload) in added {
            ensure!(!bank.objects.contains_key(&id), "Test ID collision");
            hirc.extend(kind.to_be_bytes());
            hirc.extend(((payload.len() + 4) as u32).to_be_bytes());
            hirc.extend(id.to_be_bytes());
            hirc.extend(payload);
        }
        let mut output = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let size = u32::from_be_bytes(bytes[at + 4..at + 8].try_into()?) as usize;
            if &bytes[at..at + 4] == b"HIRC" {
                output.extend(b"HIRC");
                output.extend((hirc.len() as u32).to_be_bytes());
                output.extend(&hirc);
            } else {
                output.extend(&bytes[at..at + 8 + size]);
            }
            at += 8 + size;
        }
        Ok(output)
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_device_ordered_event_repeats_and_play_stop_play_reach_the_shared_output(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        let dry = bank.named_event_actions(super::super::brush_runtime::DRY_SOUND_EVENT)?[0];
        for (name, order, submitted, stopped, limited) in [
            ("Blotworx_Test_Repeat", vec![dry, dry], 1, 0, 1),
            (
                "Blotworx_Test_PlayStopPlay",
                vec![dry, 0x7df00101, dry],
                2,
                1,
                0,
            ),
        ] {
            let mut playback = Playback::load(composed_bank(&bytes, name, &order, 0)?.into())?;
            let id = playback.post_on(name, 7)?;
            assert!(id > 0);
            assert_eq!(
                playback.submitted, 0,
                "Posting must queue, not start the OS voice inline"
            );
            assert_eq!(playback.objects.queued(), 1);
            playback.poll()?;
            assert_eq!(playback.submitted, submitted);
            assert_eq!(playback.stopped_voices, stopped);
            assert_eq!(playback.voice_limit_rejections, limited);
            assert_eq!(playback.voices.len(), 1);
            assert_eq!(playback.voices.values().next().unwrap().playing_id, id);
            assert_eq!(
                playback
                    .objects
                    .event_references(super::super::audio_bank::event_id(name)?),
                Some(1)
            );
            assert!(playback.pending_plans.is_empty());
        }
        Ok(())
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "requires owned EM1 bank files and live audio device"]
    fn original_device_composite_play_then_fade_retires_after_audio_completion()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let bytes=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank=Bank::parse(&bytes)?;let dry=bank.named_event_actions(super::super::brush_runtime::DRY_SOUND_EVENT)?[0];let name="Blotworx_Test_PlayFade";
        let mut playback=Playback::load(composed_bank(&bytes,name,&[dry,0x7df00101],500)?.into())?;
        assert!(playback.post_on(name,7)?>0);playback.poll()?;
        ensure!(playback.submitted==1 && playback.voices.values().all(|v|v.stopping),"Composite fade was not queued on its new voice");
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
        while !playback.voices.is_empty() && std::time::Instant::now()<deadline {std::thread::sleep(std::time::Duration::from_millis(5));playback.poll()?;}
        ensure!(playback.stopped_voices==1 && playback.completed==0 && playback.objects.playing.len()==0,"Faded composite leaked playing state");Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_device_separate_play_stop_posts_share_one_render_barrier() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let name = "Blotworx_Test_SeparateStop";
        let mut playback = Playback::load(composed_bank(&bytes, name, &[0x7df00101], 0)?.into())?;
        let a = playback.post_on(super::super::brush_runtime::DRY_SOUND_EVENT, 7)?;
        let b = playback.post_on(name, 7)?;
        assert!(b > a && a > 0);
        assert_eq!(playback.submitted, 0);
        assert_eq!(playback.objects.queued(), 2);
        playback.poll()?;
        assert_eq!(playback.last_consumer_event_count, 2);
        assert_eq!(playback.submitted, 1);
        assert_eq!(playback.stopped_voices, 1);
        assert_eq!(playback.stopped_before_render, 1);
        assert!(playback.voices.is_empty());
        Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 bank files; must not open an output device"]
    fn authored_fade_admission_preserves_earlier_queued_plays()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let bytes=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let name="Blotworx_Test_QueuedFade";let mut playback=Playback::load(composed_bank(&bytes,name,&[0x7df00101],500)?.into())?;
        assert!(playback.post_on(super::super::brush_runtime::DRY_SOUND_EVENT,7)?>0);
        assert!(playback.post_on(name,7)?>0);assert_eq!(playback.submitted,0);assert_eq!(playback.objects.queued(),2);assert!(playback.rejected_posts.is_empty());Ok(())
    }
    #[test]
    fn native_stop_owner_filter_distinguishes_wildcard_and_entity_zero() {
        assert!(stop_matches(Some(7), None));
        assert!(stop_matches(None, None));
        assert!(stop_matches(Some(0), Some(0)));
        assert!(!stop_matches(Some(7), Some(0)));
        assert!(!stop_matches(None, Some(0)));
        assert!(!stop_matches(Some(7), Some(9)));
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_device_external_2d_sound_streams_and_finishes()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let root=Path::new(&root).join("Audio/Wii");
        let mut playback=Playback::load(std::fs::read(root.join("Global.bnk"))?.into())?;playback.set_media_root(root);
        playback.post_on("Set_Volume_Enter_2D",7)?;playback.poll()?;
        ensure!(playback.submitted==1 && playback.rejected_posts.is_empty(),"External event rejected: {:?}",playback.rejected_posts);
        let (&id,voice)=playback.voices.iter().next().context("External source not retained")?;assert_eq!(voice.sound,0x39cc38a8);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(7);let mut rendered=0;
        while playback.completed==0 && std::time::Instant::now()<deadline {if let Some(v)=playback.voices.get(&id){rendered=rendered.max(v.device.rendered_frames());}std::thread::sleep(std::time::Duration::from_millis(5));playback.poll()?;}
        ensure!(rendered>0 && playback.completed==1 && playback.voices.is_empty(),"External source failed audible output/completion");
        ensure!(playback.pending_audio_actions()==0,"Delayed parameter action failed while streaming");Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_device_door_mix_volume_filter_and_idle_worker_transition()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let root=Path::new(&root).join("Audio/Wii");
        let mut playback=Playback::load(std::fs::read(root.join("Global.bnk"))?.into())?;
        for name in ["LVL_FX_GremlinVillage.bnk","LVL_FX_GV_Slalom.bnk"] {playback.register_bank(std::fs::read(root.join(name))?.into())?;}
        playback.post_on("SetMix_sfx_GV_Slalom_Env_Door_Closed",7)?;playback.post_on("SetMix_sfx_GV_Slalom_Env_Door_Open",7)?;playback.poll()?;
        ensure!(playback.rejected_posts.is_empty() && playback.parameter_controls_prepared>=4,"Original door mix rejected: {:?}",playback.rejected_posts);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(4);
        while playback.output.tick()<334 && std::time::Instant::now()<deadline {std::thread::sleep(std::time::Duration::from_millis(5));playback.poll()?;}
        ensure!(playback.output.tick()>=334 && playback.voices.is_empty(),"Parameter-only fade failed to advance the audio clock");
        ensure!(playback.objects.playing.len()==0 && playback.pending_audio_actions()==0,"Door mix retained completed actions");Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_device_steam_loop_survives_its_first_pass_and_stops()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let root=Path::new(&root).join("Audio/Wii");
        let mut playback=Playback::load(std::fs::read(root.join("Global.bnk"))?.into())?;
        playback.register_bank(std::fs::read(root.join("LVL_FX_GremlinVillage.bnk"))?.into())?;
        playback.set_listener_from_game(super::super::audio_spatial::game_pose([0.;3],[0.;3],[0.,0.,1.],[0.,1.,0.],0.)?)?;
        playback.set_game_position(7,[1.,0.,1.],[0.,0.,1.])?;playback.post_on("Play_sfx_GV_Steam",7).context("Play steam")?;playback.poll()?;
        ensure!(playback.submitted>=1 && playback.rejected_posts.is_empty(),"Loop rejected: {:?}",playback.rejected_posts);
        let (&id,voice)=playback.voices.iter().find(|(_,v)|v.sound==0x15d89853).context("Missing steam loop")?;assert_eq!(voice.sound,0x15d89853);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
        while playback.voices.get(&id).is_some_and(|v|v.device.rendered_frames()<90_000) && std::time::Instant::now()<deadline {std::thread::sleep(std::time::Duration::from_millis(5));playback.poll()?;}
        ensure!(playback.voices.get(&id).is_some_and(|v|v.device.rendered_frames()>=90_000 && !v.device.done()),"Loop ended at its original buffer boundary");
        playback.post_on("Stop_sfx_GV_Steam",7).context("Stop steam")?;playback.poll()?;
        ensure!(playback.rejected_posts.is_empty(),"Original loop stop rejected: {:?}",playback.rejected_posts);
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
        while !playback.voices.is_empty() && std::time::Instant::now()<deadline {std::thread::sleep(std::time::Duration::from_millis(5));playback.poll()?;}
        ensure!(playback.voices.is_empty() && playback.objects.playing.len()==0,"Steam loop stop leaked voice/playing records");Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_device_steam_uses_shared_inherited_limit_and_live_distance_filter()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let root=Path::new(&root).join("Audio/Wii");
        let mut playback=Playback::load(std::fs::read(root.join("Global.bnk"))?.into())?;
        playback.register_bank(std::fs::read(root.join("LVL_FX_GremlinVillage.bnk"))?.into())?;
        playback.set_listener_from_game(super::super::audio_spatial::game_pose([0.;3],[0.;3],[0.,0.,1.],[0.,1.,0.],0.)?)?;
        playback.set_game_position(7,[1.,0.,1.],[0.,0.,1.])?;
        for _ in 0..4 {playback.post_on("Play_sfx_GV_Steam_Start",7)?;playback.poll()?;}
        ensure!(playback.submitted==4 && playback.rejected_posts.is_empty(),"Steam rejected: {:?}",playback.rejected_posts);
        ensure!(playback.voices.len()==3 && playback.stopped_voices==1,"Inherited steam limit did not evict the oldest equal-priority voice");
        ensure!(!playback.voices.contains_key(&1),"Oldest steam voice survived equal-priority eviction");
        let (&id,voice)=playback.voices.iter().next().context("Missing steam voice")?;
        let limit=voice.admission.as_ref().unwrap().limit;assert_eq!(limit.node,0x2984fc82);assert_eq!(limit.maximum,3);
        let first=f32::from_bits(voice.spatial.as_ref().context("Missing steam spatial definition")?.low_pass);
        playback.set_game_position(7,[20.,0.,1.],[0.,0.,1.])?;playback.poll()?;
        let voice=playback.voices.get(&id).context("Steam restarted during movement")?;
        ensure!(f32::from_bits(voice.spatial.as_ref().unwrap().low_pass)>first,"Distance did not increase authored low-pass");
        std::thread::sleep(std::time::Duration::from_millis(80));
        ensure!(voice.device.rendered_frames()>0,"Steam did not reach PCM output");
        playback.set_game_position(9,[1.,0.,1.],[0.,0.,1.])?;playback.post_on("Play_sfx_GV_Steam_Start",9)?;playback.poll()?;
        ensure!(playback.submitted==5 && playback.voices.len()==4,"Independent emitter inherited a different object's count");
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
        while !playback.voices.is_empty() && std::time::Instant::now()<deadline {std::thread::sleep(std::time::Duration::from_millis(5));playback.poll()?;}
        ensure!(playback.completed==4 && playback.stopped_voices==1 && playback.objects.playing.len()==0,"Stolen and completed steam events leaked playing records");
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_steam_authored_distance_filter_is_retained()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes=std::fs::read(Path::new(&root).join("Audio/Wii/LVL_FX_GremlinVillage.bnk"))?;
        let bank=Bank::parse(&bytes)?;
        let mode=super::super::audio_bank::positioning::Mode::ThreeD{kind:3,attenuation:758161254,spatialized:true,source:super::super::audio_bank::positioning::SpatialSource::Game{dynamic:true}};
        let definition=super::super::audio_spatial::Definition::from_mode(&bank,&mode)?;
        let listeners=super::super::audio_listener::Listeners::default();let tables=super::super::audio_transition::Tables::retail()?;
        let mut emitter=super::super::audio_distance::Emitter::default();
        for (distance,expected) in [(0.,0.),(15.45,29.),(30.,100.)] {
            emitter.set_positions(&[super::super::audio_distance::Position{position:[distance,0.,0.],orientation:[0.,0.,1.]}],0,-1)?;
            assert!((definition.low_pass(&emitter,&listeners,&tables)?-expected).abs()<0.0001);
        }
        emitter.set_radius_scale(2.);
        assert!((definition.low_pass(&emitter,&listeners,&tables)?-28.15534).abs()<0.01);
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires owned files and a live Windows audio output device"]
    fn original_device_authored_walk_filter_reaches_pcm_output()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes:Arc<[u8]>=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?.into();
        let mut playback=Playback::load(bytes)?;
        playback.post_on("Play_sfx_mickey_walk",7)?;playback.poll()?;
        ensure!(playback.submitted==1,"Authored walk event did not admit a voice");
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
        while playback.completed==0 && std::time::Instant::now()<deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));playback.poll()?;
        }
        ensure!(playback.completed==1 && playback.rejected_posts.is_empty(),"Filtered walk did not complete device playback");Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore="requires owned files and live Windows audio output"]
    fn original_device_randomized_running_sounds_retain_unlimited_overlap()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes:Arc<[u8]>=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?.into();
        let mut playback=Playback::load(bytes)?;let mut random=super::super::game_random::State::default();
        playback.post_on("Play_sfx_mickey_run",0)?;playback.post_on("Play_sfx_mickey_run",0)?;playback.poll_with_random(&mut ||random.draw() as u16)?;
        ensure!(playback.submitted==2 && playback.voice_limit_rejections==0 && playback.rejected_posts.is_empty(),"Original running variants failed: {:?}",playback.rejected_posts);
        ensure!(random.draws>2,"Running voices skipped native random modulation");Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore="requires owned files and live Windows audio output"]
    fn original_device_gremlin_spatial_voice_moves_between_speaker_sides()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let root=Path::new(&root).join("Audio/Wii");
        let mut playback=Playback::load(std::fs::read(root.join("Global.bnk"))?.into())?;playback.register_bank(std::fs::read(root.join("Gremlin.bnk"))?.into())?;
        playback.set_listener_from_game(super::super::audio_spatial::game_pose([0.;3],[0.,0.,-5.],[0.,0.,1.],[0.,1.,0.],0.)?)?;
        playback.set_game_position(7,[3.,0.,1.],[0.,0.,1.])?;playback.post_on("Play_sfx_Gremlin_mvmt_GrabWrench",7)?;playback.poll()?;
        ensure!(playback.submitted==1 && playback.rejected_posts.is_empty(),"Original gremlin spatial voice rejected: {:?}",playback.rejected_posts);
        let first=playback.voices.values().next().context("Spatial voice not retained")?.spatial.as_ref().context("Spatial definition discarded")?.channels.map(f32::from_bits);
        let left=super::super::audio_spatial::stereo(0.,first)?;ensure!(left[0]>left[1],"Source did not pan to game-space right's mirrored speaker side");
        playback.set_game_position(7,[-3.,0.,1.],[0.,0.,1.])?;playback.poll()?;
        let voice=playback.voices.values().next().context("Moving spatial voice vanished")?;let second=voice.spatial.as_ref().unwrap().channels.map(f32::from_bits);let right=super::super::audio_spatial::stereo(0.,second)?;
        ensure!(right[1]>right[0] && playback.spatial_updates>0,"Moving source did not update panning");
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
        while voice.device.rendered_frames()==0 && std::time::Instant::now()<deadline {std::thread::sleep(std::time::Duration::from_millis(5));playback.output.check()?;}
        ensure!(voice.device.rendered_frames()>0,"Spatial PCM did not reach the host mixer");Ok(())
    }
    #[test]
    #[ignore="requires owned bank; synthetic repeated unsupported-source fixture"]
    fn repeated_unsupported_children_count_one_failed_post_for_each_owner()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let original=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank=Bank::parse(&original)?;let dry=bank.named_event_actions(super::super::brush_runtime::DRY_SOUND_EVENT)?[0];
        let mut bytes=composed_bank(&original,"Blotworx_UnsupportedTwice",&[dry,dry],0)?;
        let changed=Bank::parse(&bytes)?;let source=changed.objects[&changed.objects[&dry].action()?.target].payload;let offset=source.as_ptr() as usize-bytes.as_ptr() as usize;drop(changed);
        bytes[offset..offset+4].copy_from_slice(&0u32.to_be_bytes());
        let mut playback=Playback::load(bytes.into())?;playback.post_on("Blotworx_UnsupportedTwice",7)?;playback.post_on("Blotworx_UnsupportedTwice",9)?;playback.poll()?;
        assert_eq!(playback.submitted,0);assert_eq!(playback.rejected_post_count,2);assert_eq!(playback.rejected_posts.len(),2);assert!(playback.rejected_posts.values().all(|record|record.attempts==1));Ok(())
    }
    #[cfg(windows)]
    #[test]
    #[ignore="requires owned files and live Windows audio output"]
    fn original_device_surface_switches_are_applied_between_queued_footsteps()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes:Arc<[u8]>=std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?.into();
        let bank=Bank::parse(&bytes)?;let action=bank.named_event_actions("Play_sfx_mickey_walk")?[0];let container=bank.objects[&bank.objects[&action].action()?.target].switch_container(bank.feedback)?;
        let group=container.group;
        let memberships=["wood","metal"].map(|name|container.groups.iter().find(|g|Some(g.id)==super::super::audio_bank::event_id(name).ok()).map(|g|g.nodes.clone()).expect("Missing original surface package"));
        let mut playback=Playback::load(bytes)?;let mut random=super::super::game_random::State::default();
        // The authored group is8d9da40b, not the generic test name "surface".
        // Submit that exact native ID through the same queued switch command.
        playback.objects.register_object(7)?;
        playback.objects.enqueue_switch(7,group,super::super::audio_bank::event_id("wood")?);playback.post_on("Play_sfx_mickey_walk",7)?;
        playback.objects.enqueue_switch(7,group,super::super::audio_bank::event_id("metal")?);playback.post_on("Play_sfx_mickey_walk",7)?;
        playback.poll_with_random(&mut ||random.draw() as u16)?;
        ensure!(playback.submitted==2 && playback.rejected_posts.is_empty(),"Queued original surface footsteps rejected: {:?}",playback.rejected_posts);
        let voices=playback.voices.values().collect::<Vec<_>>();
        for i in 0..2 {ensure!(memberships[i].iter().any(|node|voices[i].lineage.contains(node)),"Footstep{i} used a different authored surface package");}
        ensure!(random.draws>=2,"Playback bypassed supplied C RNG");Ok(())
    }
    #[test]
    #[ignore = "requires owned EM1 bank files"]
    fn co_loaded_gv_banks_retain_first_initialized_action_routing() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let root = Path::new(&root).join("Audio/Wii");
        let a = Arc::<[u8]>::from(std::fs::read(root.join("LVL_FX_GV_Slalom.bnk"))?);
        let b = Arc::<[u8]>::from(std::fs::read(root.join("LVL_FX_GV_STB_Airlock.bnk"))?);
        let id = 0x0025f106;
        assert_ne!(
            Bank::parse(&a)?.objects[&id].payload,
            Bank::parse(&b)?.objects[&id].payload
        );
        for (first, second) in [(&a, &b), (&b, &a)] {
            let mut playback = Playback::load(first.clone())?;
            playback.register_bank(second.clone())?;
            let expected = Bank::parse(first)?;
            assert_eq!(
                playback.objects.action_definition(id).unwrap().payload,
                expected.objects[&id].payload
            );
            assert_eq!(playback.registered_banks(), 2);
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_projector_object_stop_dispatches_without_fabricating_a_voice() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        let ids = bank.named_event_actions("Stop_sfx_projector_portal")?;
        assert_eq!(ids, [0x27660b6e]);
        let action = bank.objects[&ids[0]].action()?;
        assert_eq!(action.target, 0x3af50a33);
        assert_eq!(action.delay, [0; 3]);
        assert_eq!(bank.objects[&action.target].kind, 2);
        let stop = action.object_stop()?;
        assert_eq!(stop.transition, [500, 0, 0]);
        assert_eq!(stop.curve, 4);
        assert!(stop.exceptions.is_empty());
        let mut playback = Playback::load(bytes.into())?;
        assert!(playback.post_on("Stop_sfx_projector_portal", 7)? > 0);
        assert!(playback.post_on("Stop_sfx_projector_portal", 9)? > 0);
        playback.poll()?;
        assert_eq!(
            playback.plans.len(),
            1,
            "Repeated stops must reuse immutable action metadata"
        );
        assert_eq!(playback.stop_actions, 2);
        assert_eq!(playback.stopped_voices, 0);
        assert_eq!(playback.submitted, 0);
        assert!(playback.voices.is_empty());
        assert!(
            playback.post_on("Play_sfx_projector_portal", 7).is_err(),
            "Unsupported Play must not masquerade as Stop"
        );
        for _ in 0..3 {
            assert!(playback.post_on("Play_sfx_projector_portal", 9).is_err());
        }
        assert_eq!(
            playback.plans.len(),
            2,
            "Repeated immutable Play failures must reuse parsed bank decisions"
        );
        let mut another = playback.bytes.to_vec();
        assert_eq!(&another[..4], b"BKHD");
        let next_bank = Bank::parse(&another)?.id.wrapping_add(1);
        another[12..16].copy_from_slice(&next_bank.to_be_bytes());
        playback.register_bank(another.into())?;
        assert!(
            playback.plans.is_empty(),
            "New bank did not invalidate cross-bank plans"
        );
        assert!(playback.post_on("Play_sfx_projector_portal", 7).is_err());
        assert_eq!(playback.plans.len(), 1);
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_multi_action_parameter_event_is_admitted_and_reuses_metadata() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        let ids = bank.named_event_actions("Set_Volume_Enter_2D")?;
        assert_eq!(
            ids,
            [0x006daa2b, 0x16b40ca3, 0x14b691d8, 0x22661065, 0x1ddad33a]
        );
        let set = bank.objects[&ids[0]].action()?.global_volume()?;
        assert_eq!(set.mode, 2);
        assert_eq!(set.value, [-12., 0., 0.]);
        assert_eq!(set.transition, [0; 3]);
        assert_eq!(
            set.randomized_value(|| panic!("Equal modifiers consumed RNG"))?,
            -12.
        );
        let reset = bank.objects[&ids[1]].action()?.global_volume()?;
        assert_eq!(reset.mode, 1);
        assert_eq!(reset.transition, [4000, 0, 0]);
        let mut playback = Playback::load(bytes.into())?;
        for _ in 0..3 {assert!(playback.post_on("Set_Volume_Enter_2D",7)?>0);}
        assert!(playback.rejected_posts.is_empty());assert_eq!(playback.objects.queued(),3);assert_eq!(playback.plans.len(),1);
        assert!(playback.plans[&super::super::audio_bank::event_id("Set_Volume_Enter_2D")?].is_ok());
        assert_eq!(playback.submitted,0);assert_eq!(playback.next,3);
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_dry_voice_resolves_dsp_and_parent_gain() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        let clip = resolve_direct(&bank, 0x09075f8e)?;
        let chain = bank.parameter_chain(clip.sound)?;
        assert!(matches!(
            chain.last().unwrap().position,
            super::super::audio_bank::positioning::Positioning::Local {
                center: 0,
                pan: [0., 100.],
                mode: super::super::audio_bank::positioning::Mode::TwoD { panner: false }
            }
        ));
        assert_eq!(clip.sound, 0x09075f8e);
        assert_eq!(clip.rate, 16000);
        assert_eq!(clip.samples.len(), 10416);
        assert_eq!(clip.gain_db, -10.);
        let expected = super::super::audio_panner::from_authored(false, 1, [0., 100.])?;
        assert_eq!(
            clip.channel_gains.map(f32::to_bits),
            expected[0].map(f32::to_bits)
        );
        assert!(clip.channel_gains[0] > 0. && clip.channel_gains[0] < 1.);
        assert!(scaled(&clip)?.iter().any(|sample| *sample != 0));
        if let Some(path) = std::env::var_os("BLOTWORX_AUDIO_CAPTURE") {
            Playback::load(bytes.into())?.write_clip_wav(
                super::super::brush_runtime::DRY_SOUND_EVENT,
                Path::new(&path),
            )?;
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn parent_rtpc_must_not_be_silently_discarded_by_direct_preview() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let mut bank = Bank::parse(&bytes)?;
        assert!(!bank.feedback);
        let sound = 0x09075f8e;
        let source = bank.objects[&sound].source()?;
        let parent = NodePrefix::parse(source.node_parameters)?.parent;
        let original = bank.objects[&parent].payload;
        let prefix = NodePrefix::parse(original)?;
        let (_, _, remaining) = prefix.positioning_voice_settings()?;
        let offset = original.len() - remaining.len();
        let (tail, suffix) = super::super::audio_bank::NodeTail::parse(remaining, false)?;
        assert!(tail.states.is_empty() && tail.rtpc.is_empty());
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_containers.json"
        ))?;
        let encoded = fixture["rtpc"][0]["encoded"].as_str().unwrap();
        let mut changed = original[..offset].to_vec();
        changed.extend([tail.state_sync, 0, 0]);
        changed.extend(
            (0..encoded.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap()),
        );
        changed.extend(suffix);
        bank.objects.insert(
            parent,
            super::super::audio_bank::Object {
                kind: 7,
                payload: &changed,
            },
        );
        let error = resolve_direct(&bank, sound)
            .err()
            .context("Preview silently ignored the parent's runtime curve")?;
        assert!(error.to_string().contains("Parent state/RTPC/feedback"));
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_empty_banks_register_without_inventing_events_or_media() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        for name in [
            "animatronicHook.bnk",
            "lonesomeGhostTall.bnk",
            "LVL_MS_Museum.bnk",
        ] {
            let bytes = std::fs::read(Path::new(&root).join("Audio/Wii").join(name))?;
            let bank = Bank::parse(&bytes)?;
            assert!(bank.objects.is_empty() && bank.media.is_empty());
            let mut playback = Playback::load(bytes.into())?;
            assert_eq!(playback.registered_banks(), 1);
            assert_eq!(playback.post_from_game("MissingInEmptyBank", 7), 0);
            assert_eq!(playback.submitted, 0);
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn original_gremlin_inherits_spatial_and_virtual_settings_from_its_mixer() -> Result<()> {
        use super::super::audio_bank::positioning::{
            virtual_policy, Mode, Positioning, SpatialSource,
        };
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(Path::new(&root).join("Audio/Wii/Gremlin.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        let ids = bank.named_event_actions("Play_sfx_Gremlin_mvmt_GrabWrench")?;
        assert_eq!(ids.len(), 1);
        let target = bank.objects[&ids[0]].action()?.target;
        let chain = bank.parameter_chain(target)?;
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].position, Positioning::Inherited);
        assert_eq!(chain[0].voices.below_threshold, 0);
        assert!(matches!(
            chain[1].position,
            Positioning::Local {
                center: 0,
                pan: [0., 100.],
                mode: Mode::ThreeD {
                    kind: 3,
                    attenuation: 334631128,
                    spatialized: true,
                    source: SpatialSource::Game { dynamic: true }
                }
            }
        ));
        assert!(!chain[1].voices.override_virtual);
        assert_eq!(
            virtual_policy(chain.iter().map(|n| &n.voices))?.below_threshold,
            1,
            "Root uses its own policy even when override-parent is false"
        );
        let clip=resolve_direct(&bank,target)?;
        ensure!(clip.spatial.as_ref().is_some_and(|definition|definition.dynamic),"Gremlin lost inherited dynamic spatial settings");
        ensure!(super::super::audio_output::Output::default().start(&clip).is_err(),"Spatial sound silently rendered as unpositioned2D");
        Ok(())
    }
}
