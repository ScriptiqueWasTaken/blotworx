//! EM1 CAkAudioMgr action admission and audio-block ordering.
//! DOL8036def0 divides sample delay by96;8036ca50 dispatches due actions
//! before updating transitions at tick+1. Device clock and event ingress remain
//! separate: callers must supply actual processed audio blocks, not game ticks.
use super::{
    audio_bank::{Action, Bank, Object},
    audio_control::{self, Delivery},
    audio_nodes::Nodes,
    audio_play_action,
};
use anyhow::{Context, Result};
use std::{any::Any, collections::VecDeque, sync::Arc};

pub type Owner = Arc<dyn Any + Send + Sync>;
struct Pending {
    due: u32,
    action: Arc<[u8]>,
    context: audio_play_action::Context,
    _owner: Option<Owner>,
}
#[derive(Debug, PartialEq)]
pub enum Execution {
    Parameter(Delivery),
    Play(u32),
    Stop(u32),
}
/// One shared ordered queue for admitted actions. The sink supplies actual
/// parameter/voice execution and the transition update at the audio-block tick.
pub trait Sink {
    /// Concrete audio-worker bridge; not an extra native game callback.
    fn prepare_parameter(&mut self,_:&Action<'_>,_:u32)->Result<()> {Ok(())}
    fn action_admitted(&mut self, playing_id: u32) -> Result<()>;
    fn action_finished(&mut self, playing_id: u32) -> Result<()>;
    /// Native virtual +0x30 notification at delayed admission and again just
    /// before due execution. Immediate execution does not send it.
    fn play_pending(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
    ) -> Result<()>;
    fn execute(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution>;
    fn execute_retained(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        owner: Option<&Owner>,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution>;
    fn update(&mut self, tick: u32) -> Result<()>;
}
impl Sink for Nodes {
    // This parameter-only host has no playing records. Native missing-record
    // counter operations likewise leave the registry unchanged.
    fn action_admitted(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
    fn action_finished(&mut self, _: u32) -> Result<()> {
        Ok(())
    }
    fn play_pending(&mut self, _: &Action<'_>, _: audio_play_action::Context) -> Result<()> {
        anyhow::bail!("Parameter-only sink cannot notify a pending Play")
    }
    fn execute(
        &mut self,
        action: &Action<'_>,
        _: audio_play_action::Context,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution> {
        // The existing guardian SIS sink remains parameter-only. Play requires
        // an actual node/voice host, never an acknowledged no-op on this sink.
        Ok(Execution::Parameter(audio_control::execute_due(
            action, self, random,
        )?))
    }
    fn update(&mut self, tick: u32) -> Result<()> {
        Nodes::update(self, tick)
    }
    fn execute_retained(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        _: Option<&Owner>,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution> {
        self.execute(action, context, random)
    }
}
/// Completion callbacks run after Playing has removed its record. Device/OS
/// callback synchronization remains the host's responsibility.
pub trait CompletionHost: super::audio_playing::CompletionHost {
    fn end_event(&mut self, event: super::audio_playing::EndEvent) -> Result<()>;
}
/// Adds the original playing-manager accounting around a node execution host.
/// The inner host supplies execution, pending Play notification and updates;
/// this adapter supplies its action lifecycle rather than forwarding it twice.
pub struct Tracked<'a, S, H> {
    pub inner: &'a mut S,
    pub playing: &'a mut super::audio_playing::Playing,
    pub completion: &'a mut H,
}
impl<S: Sink, H: CompletionHost> Sink for Tracked<'_, S, H> {
    fn prepare_parameter(&mut self,action:&Action<'_>,due:u32)->Result<()> {self.inner.prepare_parameter(action,due)}
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
        self.inner.play_pending(action, context)
    }
    fn execute(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution> {
        self.inner.execute(action, context, random)
    }
    fn update(&mut self, tick: u32) -> Result<()> {
        self.inner.update(tick)
    }
    fn execute_retained(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        owner: Option<&Owner>,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution> {
        self.inner.execute_retained(action, context, owner, random)
    }
}
#[derive(Default)]
pub struct Schedule {
    tick: u32,
    pending: VecDeque<Pending>,
}
impl Schedule {
    pub fn tick(&self) -> u32 {
        self.tick
    }
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
    /// Bank event order is preserved. Only recovered global mute actions are
    /// executable here; unsupported actions return errors, never silent skips.
    /// Pending entries retain only the small action payload, not a whole bank.
    pub fn post(
        &mut self,
        bank: &Bank<'_>,
        name: &str,
        nodes: &mut Nodes,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Delivery>> {
        let mut delivered = Vec::new();
        for id in bank.named_event_actions(name)? {
            let object = bank.objects.get(&id).context("Missing event action")?;
            delivered.extend(self.post_action(object.payload, nodes, &mut random)?);
        }
        Ok(delivered)
    }
    pub fn post_action(
        &mut self,
        payload: &[u8],
        nodes: &mut Nodes,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Delivery>> {
        Object { kind: 3, payload }.action()?.global_mute()?;
        self.post_context(
            payload,
            audio_play_action::Context::default(),
            nodes,
            &mut random,
        )?
        .into_iter()
        .map(|result| match result {
            Execution::Parameter(result) => Ok(result),
            _ => anyhow::bail!("Voice action result in parameter-only API"),
        })
        .collect()
    }
    /// General action ingress preserves context through immediate and delayed
    /// execution. Its caller must run on the original audio command phase.
    pub fn post_event(
        &mut self,
        bank: &Bank<'_>,
        name: &str,
        context: audio_play_action::Context,
        sink: &mut impl Sink,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Execution>> {
        let mut delivered = Vec::new();
        for id in bank.named_event_actions(name)? {
            let object = bank.objects.get(&id).context("Missing event action")?;
            delivered.extend(self.post_context(object.payload, context, sink, &mut random)?);
        }
        Ok(delivered)
    }
    pub fn post_context(
        &mut self,
        payload: &[u8],
        context: audio_play_action::Context,
        sink: &mut impl Sink,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Execution>> {
        self.post_inner(payload, None, None, context, sink, &mut random)
    }
    /// The command consumer transfers the pending object's retained owner.
    /// Share action bytes rather than allocating another copy for the queue.
    pub fn post_retained(
        &mut self,
        payload: Arc<[u8]>,
        owner: Option<Owner>,
        context: audio_play_action::Context,
        sink: &mut impl Sink,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Execution>> {
        self.post_inner(
            &payload,
            Some(payload.clone()),
            owner,
            context,
            sink,
            &mut random,
        )
    }
    fn post_inner(
        &mut self,
        payload: &[u8],
        owned: Option<Arc<[u8]>>,
        owner: Option<Owner>,
        mut context: audio_play_action::Context,
        sink: &mut impl Sink,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Execution>> {
        let object = Object { kind: 3, payload };
        let action = object.action()?;
        let mut delivered = Vec::new();
        match action.kind {
            0x4011 => {
                action.play()?;
            }
            0x1011 => {
                action.object_stop()?;
                anyhow::ensure!(
                    action.delay == [0; 3],
                    "Delayed Stop admission requires native pending-stop notification support"
                );
            }
            0x6010 | 0x7010 => {
                action.global_mute()?;
            }
            0xa010|0xb010|0xe010|0xf010=>{action.global_parameter()?;}
            kind => anyhow::bail!("Audio scheduler action {kind:08x} requires its native executor"),
        }
        sink.action_admitted(context.playing_id)?;
        let samples = action.delay_samples(&mut random)?;
        let blocks = samples / 96;
        if matches!(action.kind,0x6010|0x7010|0xa010|0xb010|0xe010|0xf010) {
            if let Err(error)=sink.prepare_parameter(&action,self.tick.wrapping_add(blocks)) {
                if context.playing_id!=0 {sink.action_finished(context.playing_id)?;}
                return Err(error);
            }
        }
        context.sample_offset = samples % 96;
        if blocks == 0 {
            let result = sink.execute_retained(&action, context, owner.as_ref(), &mut random);
            // Native execution status does not bypass completion. Preserve
            // accounting on Rust host errors as well before propagating.
            let finished = if context.playing_id != 0 {
                sink.action_finished(context.playing_id)
            } else {
                Ok(())
            };
            delivered.push(result?);
            finished?;
        } else {
            let pending = Pending {
                due: self.tick.wrapping_add(blocks),
                action: owned.unwrap_or_else(|| Arc::from(object.payload)),
                context,
                _owner: owner,
            };
            // Native unsigned comparison, stable insertion after equal
            // deadlines. Do not replace its wrap behavior with signed time.
            let at = self
                .pending
                .partition_point(|entry| entry.due <= pending.due);
            self.pending.insert(at, pending);
            if action.kind == 0x4011 {
                sink.play_pending(&action, context)?;
            }
        }
        Ok(delivered)
    }
    /// One original audio block: due actions see the old tick, transitions
    /// sample tick+1, then the manager advances. Mute dispatch does not consume
    /// the retained sample remainder; Play uses it on a different path.
    pub fn process_block(
        &mut self,
        nodes: &mut Nodes,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Delivery>> {
        self.process_audio_block(nodes, &mut random)?
            .into_iter()
            .map(|result| match result {
                Execution::Parameter(result) => Ok(result),
                _ => anyhow::bail!("Voice action result in parameter-only API"),
            })
            .collect()
    }
    pub fn process_audio_block(
        &mut self,
        sink: &mut impl Sink,
        mut random: impl FnMut() -> u16,
    ) -> Result<Vec<Execution>> {
        let mut delivered = Vec::new();
        while self
            .pending
            .front()
            .is_some_and(|entry| entry.due <= self.tick)
        {
            let pending = self.pending.pop_front().unwrap();
            let action = Object {
                kind: 3,
                payload: &pending.action,
            }
            .action()?;
            if action.kind == 0x4011 {
                sink.play_pending(&action, pending.context)?;
            }
            let result = sink.execute_retained(
                &action,
                pending.context,
                pending._owner.as_ref(),
                &mut random,
            );
            let finished = if pending.context.playing_id != 0 {
                sink.action_finished(pending.context.playing_id)
            } else {
                Ok(())
            };
            delivered.push(result?);
            finished?;
        }
        let next = self.tick.wrapping_add(1);
        sink.update(next)?;
        self.tick = next;
        Ok(delivered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Host {
        references: i32,
        requests: Vec<audio_play_action::Request>,
        mutes: Vec<(u32, u8, i32)>,
        order: Vec<(u32, u32)>,
        updates: Vec<u32>,
    }
    impl audio_play_action::Control for Host {
        type Handle = u32;
        fn acquire_play_node(&mut self, id: u32) -> Result<Option<u32>> {
            self.references += 1;
            Ok(Some(id))
        }
        fn play_node(
            &mut self,
            _: u32,
            request: &audio_play_action::Request,
            _: &mut dyn FnMut() -> u16,
        ) -> Result<u32> {
            self.requests.push(request.clone());
            Ok(1)
        }
        fn release_play_node(&mut self, _: u32) {
            self.references -= 1;
        }
    }
    impl audio_control::ParameterNodeControl for Host {
        type Handle = u32;
        fn acquire_node(&mut self, id: u32) -> Result<Option<u32>> {
            self.references += 1;
            Ok(Some(id))
        }
        fn mute_global(&mut self, id: u32, curve: u8, transition: i32) -> Result<()> {
            self.mutes.push((id, curve, transition));
            Ok(())
        }
        fn unmute_global(&mut self, id: u32, curve: u8, transition: i32) -> Result<()> {
            self.mutes.push((id, curve, transition));
            Ok(())
        }
        fn release_node(&mut self, _: u32) {
            self.references -= 1;
        }
    }
    impl Sink for Host {
        fn execute_retained(
            &mut self,
            action: &Action<'_>,
            context: audio_play_action::Context,
            _: Option<&Owner>,
            random: &mut dyn FnMut() -> u16,
        ) -> Result<Execution> {
            self.execute(action, context, random)
        }
        fn action_admitted(&mut self, _: u32) -> Result<()> {
            Ok(())
        }
        fn action_finished(&mut self, _: u32) -> Result<()> {
            Ok(())
        }
        fn play_pending(&mut self, _: &Action<'_>, _: audio_play_action::Context) -> Result<()> {
            Ok(())
        }
        fn execute(
            &mut self,
            action: &Action<'_>,
            context: audio_play_action::Context,
            random: &mut dyn FnMut() -> u16,
        ) -> Result<Execution> {
            self.order.push((action.kind, action.target));
            if action.kind == 0x4011 {
                Ok(Execution::Play(audio_play_action::execute_due(
                    action, context, self, random,
                )?))
            } else {
                Ok(Execution::Parameter(audio_control::execute_due(
                    action, self, random,
                )?))
            }
        }
        fn update(&mut self, tick: u32) -> Result<()> {
            self.updates.push(tick);
            Ok(())
        }
    }
    fn encoded(kind: u32, target: u32, delay: [i32; 3], transition: [i32; 3]) -> Vec<u8> {
        let mut parameters = transition
            .into_iter()
            .flat_map(i32::to_be_bytes)
            .collect::<Vec<_>>();
        parameters.push(255);
        parameters.extend([0; 16]);
        parameters.extend(0u32.to_be_bytes());
        if kind == 0x4011 {
            parameters.extend(7u32.to_be_bytes());
        }
        let mut bytes = [
            kind,
            target,
            delay[0] as u32,
            delay[1] as u32,
            delay[2] as u32,
            parameters.len() as u32,
        ]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect::<Vec<_>>();
        bytes.extend(parameters);
        bytes
    }
    #[test]
    fn original_full_admission_lifecycle_order() -> Result<()> {
        use serde_json::{json, Value};
        #[derive(Default)]
        struct Probe {
            calls: Vec<Value>,
        }
        impl Sink for Probe {
            fn execute_retained(
                &mut self,
                action: &Action<'_>,
                context: audio_play_action::Context,
                _: Option<&Owner>,
                random: &mut dyn FnMut() -> u16,
            ) -> Result<Execution> {
                self.execute(action, context, random)
            }
            fn action_admitted(&mut self, id: u32) -> Result<()> {
                self.calls.push(json!({"op":"admit","id":id}));
                Ok(())
            }
            fn action_finished(&mut self, id: u32) -> Result<()> {
                self.calls.push(json!({"op":"finish","id":id}));
                Ok(())
            }
            fn play_pending(
                &mut self,
                _: &Action<'_>,
                _: audio_play_action::Context,
            ) -> Result<()> {
                self.calls.push(json!({"op":"pending_notification"}));
                Ok(())
            }
            fn execute(
                &mut self,
                _: &Action<'_>,
                context: audio_play_action::Context,
                _: &mut dyn FnMut() -> u16,
            ) -> Result<Execution> {
                self.calls
                    .push(json!({"op":"execute","offset":context.sample_offset}));
                Ok(Execution::Play(2))
            }
            fn update(&mut self, _: u32) -> Result<()> {
                Ok(())
            }
        }
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_admission.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut schedule = Schedule {
                tick: case["tick"].as_u64().unwrap() as u32,
                ..Default::default()
            };
            let kind = case["kind"].as_u64().unwrap() as u32;
            let delay = std::array::from_fn(|i| case["delay"][i].as_i64().unwrap() as i32);
            let context = audio_play_action::Context {
                playing_id: case["playing_id"].as_u64().unwrap() as u32,
                ..Default::default()
            };
            let mut probe = Probe::default();
            schedule.post_context(
                &encoded(kind, 7, delay, [0; 3]),
                context,
                &mut probe,
                || panic!("unexpected RNG"),
            )?;
            let expected: Vec<_> = case["calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|call| {
                    matches!(
                        call["op"].as_str().unwrap(),
                        "admit" | "finish" | "execute" | "pending_notification"
                    )
                })
                .cloned()
                .collect();
            assert_eq!(probe.calls, expected, "case {case}");
            assert_eq!(
                schedule.pending(),
                case["queued"].as_u64().unwrap() as usize
            );
            if let Some(pending) = schedule.pending.front() {
                assert_eq!(json!(pending.due), case["due"]);
                assert_eq!(json!(pending.context.sample_offset), case["offset"]);
            }
            if schedule.pending() != 0 {
                probe.calls.clear();
                schedule.tick = case["due"].as_u64().unwrap() as u32;
                schedule.process_audio_block(&mut probe, || panic!())?;
                let expected: Vec<_> = case["due_calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|call| {
                        matches!(
                            call["op"].as_str().unwrap(),
                            "admit" | "finish" | "execute" | "pending_notification"
                        )
                    })
                    .cloned()
                    .collect();
                assert_eq!(probe.calls, expected, "due case {case}");
                assert_eq!(schedule.pending(), 0);
            }
        }
        Ok(())
    }
    #[test]
    fn tracked_actions_hold_completion_until_last_pending_action() -> Result<()> {
        use super::super::{
            audio_ingress::{Context, Registration},
            audio_playing::{EndEvent, Playing},
        };
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
        impl CompletionHost for Completed {
            fn end_event(&mut self, event: EndEvent) -> Result<()> {
                self.ends.push(event);
                Ok(())
            }
        }
        let mut playing = Playing::default();
        let mut completed = Completed::default();
        let mut inner = Host::default();
        let mut schedule = Schedule::default();
        playing.register(Registration {
            event: 5,
            context: Context {
                playing_id: 7,
                ..Default::default()
            },
            callback: 9,
            cookie: 11,
            flags: 0x10001,
        });
        let context = audio_play_action::Context {
            playing_id: 7,
            ..Default::default()
        };
        {
            let mut sink = Tracked {
                inner: &mut inner,
                playing: &mut playing,
                completion: &mut completed,
            };
            schedule.post_context(
                &encoded(0x4011, 1, [0; 3], [0; 3]),
                context,
                &mut sink,
                || panic!(),
            )?;
            schedule.post_context(
                &encoded(0x4011, 2, [8, 0, 0], [0; 3]),
                context,
                &mut sink,
                || panic!(),
            )?;
            assert_eq!(sink.playing.record(7).unwrap().actions, 2);
            // Consumer releases the event's initial action after admission.
            sink.action_finished(7)?;
            for _ in 0..2 {
                schedule.process_audio_block(&mut sink, || panic!())?;
            }
            assert!(sink.completion.ends.is_empty());
            schedule.process_audio_block(&mut sink, || panic!())?;
            assert!(sink.playing.record(7).is_none());
        }
        assert_eq!(completed.positions, [7]);
        assert_eq!(completed.ends.len(), 1);
        assert_eq!(completed.ends[0].playing_id, 7);
        assert_eq!(inner.requests.len(), 2);
        Ok(())
    }
    #[test]
    fn original_delay_admission_preserves_deadline_remainder_rng_and_wrap() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_play_action.json"
        ))?;
        for case in reference["admissions"].as_array().unwrap() {
            let mut schedule = Schedule {
                tick: case["tick"].as_u64().unwrap() as u32,
                ..Default::default()
            };
            let mut host = Host::default();
            let delay = std::array::from_fn(|i| case["delay"][i].as_i64().unwrap() as i32);
            let bytes = encoded(0x4011, 5, delay, [0; 3]);
            let mut random = super::super::game_random::State::default();
            let context = audio_play_action::Context {
                object: 0x1234,
                playing_id: 42,
                sample_offset: 999,
                words: [1, 2, 3, 4],
            };
            let result =
                schedule.post_context(&bytes, context, &mut host, || random.draw() as u16)?;
            if case["blocks"] == 0 {
                assert_eq!(result, vec![Execution::Play(1)]);
                assert_eq!(host.requests.len(), 1);
                assert_eq!(case["remainder"], host.requests[0].context.sample_offset);
                assert_eq!(schedule.pending(), 0);
            } else {
                assert!(result.is_empty() && host.requests.is_empty());
                assert_eq!(schedule.pending(), 1);
                let pending = schedule.pending.front().unwrap();
                assert_eq!(case["due"], pending.due);
                assert_eq!(case["remainder"], pending.context.sample_offset);
                assert_eq!(
                    pending.context,
                    audio_play_action::Context {
                        sample_offset: case["remainder"].as_u64().unwrap() as u32,
                        ..context
                    }
                );
            }
            assert_eq!(case["draws"], random.draws);
            assert_eq!(case["seed"], random.seed);
            assert_eq!(host.references, 0);
        }
        Ok(())
    }
    #[test]
    fn play_and_parameter_actions_share_order_clock_rng_and_sample_context() -> Result<()> {
        let mut schedule = Schedule::default();
        let mut host = Host::default();
        let mut random = super::super::game_random::State::default();
        let context = audio_play_action::Context {
            object: 0x1234,
            playing_id: 77,
            words: [1, 2, 3, 4],
            sample_offset: 999,
        };
        for (kind, target) in [(0x4011, 1), (0x6010, 2), (0x4011, 3)] {
            assert!(schedule
                .post_context(
                    &encoded(kind, target, [8, 0, 0], [15, -7, 11]),
                    context,
                    &mut host,
                    || random.draw() as u16
                )?
                .is_empty());
        }
        assert_eq!(
            random.draws, 0,
            "Transition RNG ran before the delayed action was due"
        );
        assert_eq!(
            schedule.post_context(
                &encoded(0x4011, 4, [2, 0, 0], [0; 3]),
                context,
                &mut host,
                || random.draw() as u16
            )?,
            vec![Execution::Play(1)]
        );
        assert_eq!(host.requests[0].context.sample_offset, 64);
        for _ in 0..2 {
            assert!(schedule
                .process_audio_block(&mut host, || random.draw() as u16)?
                .is_empty());
        }
        assert_eq!(
            schedule.process_audio_block(&mut host, || random.draw() as u16)?,
            vec![
                Execution::Play(1),
                Execution::Parameter(Delivery::Applied),
                Execution::Play(1)
            ]
        );
        assert_eq!(
            host.order,
            [(0x4011, 4), (0x4011, 1), (0x6010, 2), (0x4011, 3)]
        );
        assert_eq!(
            host.requests
                .iter()
                .map(|request| request.transition)
                .collect::<Vec<_>>(),
            [0, 17, 14]
        );
        assert_eq!(host.mutes, [(2, 31, 11)]);
        assert_eq!(host.updates, [1, 2, 3]);
        assert_eq!(random.draws, 3);
        for request in &host.requests {
            assert_eq!(
                request.context,
                audio_play_action::Context {
                    sample_offset: 64,
                    ..context
                }
            );
        }
        assert_eq!(host.references, 0);
        assert_eq!(schedule.pending(), 0);
        Ok(())
    }
    #[test]
    fn delay_uses_loaded_signed_words_then_native_unsigned_division() -> Result<()> {
        let action = super::super::audio_bank::Action {
            kind: 0x6010,
            target: 1,
            delay: [2000, 0, 0],
            subsection_size: 0,
            parameters: &[],
        };
        assert_eq!(
            action.delay_samples(|| panic!("Equal range consumed RNG"))?,
            64000
        );
        let negative = super::super::audio_bank::Action {
            delay: [-1, 0, 0],
            ..action
        };
        assert_eq!(negative.delay_samples(|| panic!())?, u32::MAX - 31);
        assert_eq!(64000u32 / 96, 666);
        assert_eq!(64000u32 % 96, 64);
        let randomized = super::super::audio_bank::Action {
            delay: [10, -2, 2],
            ..action
        };
        assert_eq!(randomized.delay_samples(|| 0)?, 256);
        assert_eq!(randomized.delay_samples(|| 32767)?, 384);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_guardian_events_wait_for_native_block_deadlines_then_fade() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(std::path::Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = Bank::parse(&bytes)?;
        let mut nodes = Nodes::new()?;
        nodes.register_bank(&bank)?;
        let mut schedule = Schedule::default();
        assert_eq!(
            schedule.post(&bank, "Mute_Guardians", &mut nodes, || panic!())?,
            vec![Delivery::Applied, Delivery::Applied]
        );
        let targets = bank
            .named_event_actions("Mute_Guardians")?
            .iter()
            .map(|id| bank.objects[id].action().map(|action| action.target))
            .collect::<Result<Vec<_>>>()?;
        for _ in 0..167 {
            assert!(schedule.process_block(&mut nodes, || panic!())?.is_empty());
        }
        for &target in &targets {
            assert_eq!(nodes.byte(target), Some(0));
        }
        assert!(schedule
            .post(&bank, "Unmute_Guardians", &mut nodes, || panic!())?
            .is_empty());
        assert_eq!(schedule.pending(), 2);
        assert!(schedule
            .pending
            .iter()
            .all(|entry| entry.due == 833 && entry.context.sample_offset == 64));
        for _ in 0..666 {
            assert!(schedule.process_block(&mut nodes, || panic!())?.is_empty());
        }
        assert_eq!(schedule.tick(), 833);
        for &target in &targets {
            assert_eq!(nodes.byte(target), Some(0));
        }
        assert_eq!(
            schedule.process_block(&mut nodes, || panic!())?,
            vec![Delivery::Applied, Delivery::Applied]
        );
        assert_eq!(schedule.pending(), 0);
        for _ in 0..166 {
            schedule.process_block(&mut nodes, || panic!())?;
        }
        assert_eq!(schedule.tick(), 1000);
        for target in targets {
            assert_eq!(nodes.byte(target), Some(254));
            assert_eq!(nodes.references(target), Some(0));
        }
        Ok(())
    }
}
