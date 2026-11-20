//! Connect due actions to the recovered Play/parameter-node dispatchers.
//! The concrete node/container/voice/mixer host still supplies engine services.
use super::{
    audio_bank::Action,
    audio_control::{self, ParameterNodeControl},
    audio_events::{NodeHost, RegisteredObject},
    audio_listener::Listeners,
    audio_play_action::{self, Request},
    audio_playing::Playing,
    audio_schedule::{CompletionHost, Execution},
};
use anyhow::Result;
use std::sync::Arc;

pub trait Host: ParameterNodeControl {
    type PlayHandle: Copy;
    fn switch_changed(&mut self, _: Arc<RegisteredObject>, _: u32, _: u32) -> Result<()> {
        anyhow::bail!("Voice host has not implemented continuous switch notifications")
    }
    fn acquire_play_node(&mut self, id: u32) -> Result<Option<Self::PlayHandle>>;
    /// A voice created here must retain its owner. Listener state is borrowed
    /// for this execution; updates receive current state again each block.
    fn play_node(
        &mut self,
        node: Self::PlayHandle,
        request: &Request,
        owner: Option<Arc<RegisteredObject>>,
        listeners: &Listeners,
        playing: &mut Playing,
        completion: &mut dyn CompletionHost,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<u32>;
    fn release_play_node(&mut self, node: Self::PlayHandle);
    /// The backend owns native node/voice update ordering; this bridge does not
    /// invent an ordering between mixer services or advance a game-frame clock.
    fn update(
        &mut self,
        tick: u32,
        listeners: &Listeners,
        playing: &mut Playing,
        completion: &mut dyn CompletionHost,
    ) -> Result<()>;
}
pub struct Executor<H> {
    pub host: H,
}
struct Play<'a, H> {
    host: &'a mut H,
    owner: Option<Arc<RegisteredObject>>,
    listeners: &'a Listeners,
    playing: &'a mut Playing,
    completion: &'a mut dyn CompletionHost,
}
impl<H: Host> audio_play_action::Control for Play<'_, H> {
    type Handle = H::PlayHandle;
    fn acquire_play_node(&mut self, id: u32) -> Result<Option<Self::Handle>> {
        self.host.acquire_play_node(id)
    }
    fn play_node(
        &mut self,
        node: Self::Handle,
        request: &Request,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<u32> {
        self.host.play_node(
            node,
            request,
            self.owner.clone(),
            self.listeners,
            self.playing,
            self.completion,
            random,
        )
    }
    fn release_play_node(&mut self, node: Self::Handle) {
        self.host.release_play_node(node);
    }
}
impl<H: Host> NodeHost for Executor<H> {
    fn switch_changed(
        &mut self,
        object: Arc<RegisteredObject>,
        group: u32,
        value: u32,
    ) -> Result<()> {
        self.host.switch_changed(object, group, value)
    }
    fn execute(
        &mut self,
        action: &Action<'_>,
        context: audio_play_action::Context,
        owner: Option<Arc<RegisteredObject>>,
        listeners: &Listeners,
        playing: &mut Playing,
        completion: &mut dyn CompletionHost,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<Execution> {
        if action.kind == 0x4011 {
            Ok(Execution::Play(audio_play_action::execute_due(
                action,
                context,
                &mut Play {
                    host: &mut self.host,
                    owner,
                    listeners,
                    playing,
                    completion,
                },
                random,
            )?))
        } else {
            // Unsupported parameter/action kinds remain explicit parser errors.
            Ok(Execution::Parameter(audio_control::execute_due(
                action,
                &mut self.host,
                random,
            )?))
        }
    }
    fn update(
        &mut self,
        tick: u32,
        listeners: &Listeners,
        playing: &mut Playing,
        completion: &mut dyn CompletionHost,
    ) -> Result<()> {
        self.host.update(tick, listeners, playing, completion)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Completed;
    impl super::super::audio_playing::CompletionHost for Completed {
        fn remove_source_position(&mut self, _: u32) {}
    }
    impl CompletionHost for Completed {
        fn end_event(&mut self, _: super::super::audio_playing::EndEvent) -> Result<()> {
            Ok(())
        }
    }
    struct Backend {
        present: bool,
        status: u32,
        references: u32,
        calls: Vec<serde_json::Value>,
        owners: Vec<Option<Arc<RegisteredObject>>>,
        fail: bool,
    }
    impl ParameterNodeControl for Backend {
        type Handle = u32;
        fn acquire_node(&mut self, _: u32) -> Result<Option<u32>> {
            anyhow::bail!("Unexpected parameter-node acquisition")
        }
        fn mute_global(&mut self, _: u32, _: u8, _: i32) -> Result<()> {
            unreachable!()
        }
        fn unmute_global(&mut self, _: u32, _: u8, _: i32) -> Result<()> {
            unreachable!()
        }
        fn release_node(&mut self, _: u32) {
            unreachable!()
        }
    }
    impl Host for Backend {
        type PlayHandle = u32;
        fn switch_changed(
            &mut self,
            object: Arc<RegisteredObject>,
            group: u32,
            value: u32,
        ) -> Result<()> {
            assert_eq!(object.switch_value(group)?, Some(value));
            self.calls
                .push(serde_json::json!({"kind":"switch","group":group,"value":value}));
            self.owners.push(Some(object));
            Ok(())
        }
        fn acquire_play_node(&mut self, id: u32) -> Result<Option<u32>> {
            Ok(if self.present {
                self.references += 1;
                Some(id)
            } else {
                None
            })
        }
        fn play_node(
            &mut self,
            node: u32,
            request: &Request,
            owner: Option<Arc<RegisteredObject>>,
            listeners: &Listeners,
            _: &mut Playing,
            _: &mut dyn CompletionHost,
            _: &mut dyn FnMut() -> u16,
        ) -> Result<u32> {
            assert_eq!(node, request.target);
            self.owners.push(owner);
            assert_eq!(listeners.records()[0].pose.position, [3., 4., 5.]);
            self.calls.push(serde_json::json!({"kind":"play","target":request.target,"request_kind":request.kind,"transition":request.transition,
                "curve":request.curve,"context":request.context,"native_zero_words":request.native_zero_words,"native_flags":request.native_flags}));
            anyhow::ensure!(!self.fail, "Injected voice-host failure");
            Ok(self.status)
        }
        fn release_play_node(&mut self, _: u32) {
            self.references -= 1;
            self.calls.push(serde_json::json!({"kind":"release"}));
        }
        fn update(
            &mut self,
            _: u32,
            _: &Listeners,
            _: &mut Playing,
            _: &mut dyn CompletionHost,
        ) -> Result<()> {
            Ok(())
        }
    }
    #[test]
    fn queued_switch_notifications_reach_concrete_execution_host_with_retained_owner() -> Result<()>
    {
        let mut events = super::super::audio_events::Events::default();
        let owner = events.register_object(42)?;
        events.enqueue_switch(42, 12, 99);
        let mut executor = Executor {
            host: Backend {
                present: true,
                status: 1,
                references: 3,
                calls: Vec::new(),
                owners: Vec::new(),
                fail: false,
            },
        };
        let batch = events.process_commands(&mut executor, &mut Completed, || {
            panic!("Switch notification consumed RNG")
        })?;
        assert_eq!(batch.switches.len(), 1);
        assert_eq!(
            executor.host.calls,
            vec![serde_json::json!({"kind":"switch","group":12,"value":99})]
        );
        assert!(Arc::ptr_eq(
            executor.host.owners[0].as_ref().unwrap(),
            &owner
        ));
        Ok(())
    }
    #[test]
    fn original_play_requests_cross_shared_execution_bridge_without_losing_context_or_references(
    ) -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_play_action.json"
        ))?;
        let mut listeners = Listeners::default();
        listeners.set_pose(
            0,
            super::super::audio_listener::Pose {
                forward: [0., 0., 1.],
                up: [0., 1., 0.],
                position: [3., 4., 5.],
            },
        )?;
        let mut playing = Playing::default();
        let mut completed = Completed;
        for case in fixture["cases"].as_array().unwrap() {
            let encoded = case["encoded"].as_str().unwrap();
            let bytes = (0..encoded.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let action = super::super::audio_bank::Object {
                kind: 3,
                payload: &bytes,
            }
            .action()?;
            let mut executor = Executor {
                host: Backend {
                    present: case["present"].as_bool().unwrap(),
                    status: case["status"].as_u64().unwrap() as u32,
                    references: 3,
                    calls: Vec::new(),
                    owners: Vec::new(),
                    fail: false,
                },
            };
            let mut random = super::super::game_random::State::default();
            let context = serde_json::from_value(case["context"].clone())?;
            let result = executor.execute(
                &action,
                context,
                None,
                &listeners,
                &mut playing,
                &mut completed,
                &mut || random.draw() as u16,
            )?;
            assert!(
                matches!(result,Execution::Play(status) if status as u64==case["status"].as_u64().unwrap())
            );
            assert_eq!(serde_json::json!(executor.host.calls), case["calls"]);
            assert_eq!(random.draws, case["draws"].as_u64().unwrap());
            assert_eq!(random.seed as u64, case["seed"].as_u64().unwrap());
            assert_eq!(executor.host.references, 3);
            if executor.host.present {
                executor.host.fail = true;
                assert!(executor
                    .execute(
                        &action,
                        context,
                        None,
                        &listeners,
                        &mut playing,
                        &mut completed,
                        &mut || 0
                    )
                    .is_err());
                assert_eq!(executor.host.references, 3);
            }
        }
        Ok(())
    }
    #[test]
    fn queued_play_reaches_backend_with_retained_owner_current_listeners_and_original_request(
    ) -> Result<()> {
        use super::super::{
            audio_bank::{Bank, Object},
            audio_events::Events,
        };
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_play_action.json"
        ))?;
        let encoded = fixture["cases"].as_array().unwrap()[0]["encoded"]
            .as_str()
            .unwrap();
        let bytes = (0..encoded.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let order = [1u32, 10]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>();
        let bank = Bank {
            id: 7,
            language: 0,
            feedback: false,
            media: std::collections::BTreeMap::new(),
            objects: std::collections::BTreeMap::from([
                (
                    10,
                    Object {
                        kind: 3,
                        payload: &bytes,
                    },
                ),
                (
                    20,
                    Object {
                        kind: 4,
                        payload: &order,
                    },
                ),
            ]),
        };
        let mut events = Events::default();
        events.register_bank(&bank)?;
        let old = events.register_object(42)?;
        events.enqueue_listener_pose(
            0,
            super::super::audio_listener::Pose {
                forward: [0., 0., 1.],
                up: [0., 1., 0.],
                position: [3., 4., 5.],
            },
        );
        let mut executor = Executor {
            host: Backend {
                present: true,
                status: 1,
                references: 3,
                calls: Vec::new(),
                owners: Vec::new(),
                fail: false,
            },
        };
        events.post(20, 42, Some([1, 2, 3]), 0, 0, 0)?;
        let consumed = events.process_commands(&mut executor, &mut Completed, || 0)?;
        assert!(matches!(
            consumed.events[0].executions[0],
            Execution::Play(1)
        ));
        assert!(Arc::ptr_eq(executor.host.owners[0].as_ref().unwrap(), &old));
        let call = &executor.host.calls[0];
        assert_eq!(call["context"]["object"], old.handle);
        assert_eq!(call["context"]["words"], serde_json::json!([1, 2, 3, 0]));
        events.unregister_object(42);
        let replacement = events.register_object(42)?;
        events.post(20, 42, None, 0, 0, 0)?;
        events.process_commands(&mut executor, &mut Completed, || 0)?;
        assert!(Arc::ptr_eq(executor.host.owners[0].as_ref().unwrap(), &old));
        assert!(Arc::ptr_eq(
            executor.host.owners[1].as_ref().unwrap(),
            &replacement
        ));
        assert_eq!(executor.host.references, 3);
        Ok(())
    }
}
