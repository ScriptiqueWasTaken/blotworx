//! ScriptedMove's ordinary action receiver, REL1b8d20..1b8e7c.
//! Nested movement/turn task construction and behavior selection are separate.
use super::action_event_queue::ActionEvent;
use anyhow::Result;

#[derive(Debug, PartialEq)]
pub enum TaskRequest {
    Move {
        destination: [u32; 4],
        radius: u32,
        speed: u32,
        extra: u32,
    },
    Turn {
        destination: [u32; 4],
    },
}
/// The behavior's own update emits an ordinary event, after child-task waiting
/// in198b3c. Hosts must preserve that delivery and its null header.
pub trait CompletionHost {
    fn reached_position(&mut self, owner:usize)->Result<()>;
}
impl CompletionHost for super::assets::World {
    fn reached_position(&mut self,owner:usize)->Result<()> {
        self.deferred_event_runtime.dispatch_ordinary_output(
            &mut self.entity_event_registry,
            super::entity_events::Basic {event_kind:0x0d,source:Some(owner),header:None},
            &mut self.outputs,&self.entities,&self.scene_resources,self.animation_time,
            &mut self.ai_brains,&self.robot_arm_controls,&self.cutscene_actions,
        )
    }
}
pub trait TaskHost: super::ai_task_runtime::Host {
    fn reset_navigation(&mut self, owner: usize) -> Result<()>;
    fn create_task(&mut self, request: TaskRequest) -> Result<usize>;
}

#[derive(Default, Debug, serde::Serialize)]
pub struct ScriptedMove {
    pub pending: bool,
    pub action: Option<u32>,
    pub radius_bits: Option<u32>,
    // Native missing-reference delivery retains the previous destination.
    // None exposes an uninitialized destination rather than inventing origin.
    pub destination_bits: Option<[u32; 4]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_npc_waited_task_completion_reaches_ordinary_world_io()->Result<()> {
        use anyhow::Context as _;
        use super::super::{ai_task_runtime::{Context,Host,State},assets::World};
        use epic_mickey_lib_rs::scene_file::{Component,Property,ID};
        struct Completion<'a> {world:&'a mut World,done:bool,kind:u32,calls:Vec<&'static str>}
        impl Host for Completion<'_> {
            fn kind(&mut self,_:usize)->Result<u32>{self.calls.push("kind");Ok(self.kind)}
            fn activate(&mut self,_:&mut State,_:usize,_:usize)->Result<()>{unreachable!()}
            fn stop(&mut self,_:&mut State,_:usize,_:usize)->Result<()>{self.calls.push("stop");Ok(())}
            fn destroy(&mut self,_:&mut State,_:usize)->Result<()>{self.calls.push("destroy");Ok(())}
            fn update_task(&mut self,_:&mut State,_:usize,_:Context)->Result<u32>{self.calls.push("tick");Ok(u32::from(self.done))}
            fn child_completed(&mut self,_:&mut State,_:Context)->Result<()>{self.calls.push("completed");Ok(())}
            fn update_behavior(&mut self,_:&mut State,c:Context)->Result<u32>{
                self.calls.push("behavior");ScriptedMove::default().update_behavior(c.owner,self.world)
            }
        }
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
        let owner=world.scripts.resolve_name("gremlinvillage_zonea.Gremlin_Variant4 01").context("Original Gremlin absent")?;
        // The child callback here is a supplied task boundary. The real shared
        // lifecycle, World registry and reflected IO receiver run unchanged.
        let io=Component::new("InputOutputComponent".into(),"completion fixture".into(),
            ID{id:0},ID{id:0},ID{id:0},[
                ("Output",serde_json::json!(["OnReachedPosition"])),
                ("Input",serde_json::json!(["SetPropertyFloat"])),
                ("Target",serde_json::json!(["_self"])),
                ("Parameters",serde_json::json!(["UsableRadius,7.5"])),
                ("DelayWait",serde_json::json!([0.])),("IoFlags",serde_json::json!([0])),
            ].into_iter().map(|(name,value)|Property::new(String::new(),name.into(),false,false,false,value)).collect());
        world.outputs=super::super::actor_io::Outputs::default();
        world.outputs.bind(owner,&world.entities[owner].name,&[io])?;
        world.bind_deferred_output_events(owner)?;
        world.bind_deferred_output_events(owner)?;
        for kind in [1,2] {
            let before=world.deferred_event_runtime.reached_position_deliveries;
            let queued=world.entity_event_queue.entries().len();
            let mut state=State{child:Some(1),mode:1,..State::default()};
            let mut host=Completion{world:&mut world,done:false,kind,calls:vec![]};
            let context=Context{owner,delta:super::super::STEP};
            assert_eq!(state.update(context,&mut host)?,1);
            assert_eq!(host.calls,["tick"]);
            assert_eq!(host.world.deferred_event_runtime.reached_position_deliveries,before);
            host.done=true;
            assert_eq!(state.update(context,&mut host)?,2);
            assert_eq!(host.calls,["tick","tick","kind","completed","stop","destroy","behavior"]);
            assert_eq!(state.child,None);assert_eq!(state.previous_kind,kind);
            assert_eq!(host.world.deferred_event_runtime.reached_position_deliveries,before+1);
            assert_eq!(host.world.entity_event_queue.entries().len(),queued,"Completion was deferred instead of ordinary");
            host.world.dispatch_actor_inputs(host.world.animation_time);
            let radius=host.world.entities[owner].components.iter().find(|c|c.class_name=="UsableComponent")
                .and_then(|c|c.properties.iter().find(|p|p.name=="UsableRadius")).context("Original usable radius missing")?;
            assert_eq!(radius.value.as_f64(),Some(7.5));
        }
        Ok(())
    }
    #[test]
    fn received_move_and_turn_requests_install_independent_waiting_tasks() -> Result<()> {
        use super::super::ai_task_runtime::{Context, Host, State};
        #[derive(Default)]
        struct Factory {
            calls: Vec<&'static str>,
            request: Option<TaskRequest>,
        }
        impl Host for Factory {
            fn kind(&mut self, _: usize) -> Result<u32> {
                unreachable!()
            }
            fn activate(&mut self, state: &mut State, task: usize, owner: usize) -> Result<()> {
                assert_eq!((state.child, state.mode, task, owner), (Some(2), 0, 2, 7));
                self.calls.push("activate");
                Ok(())
            }
            fn stop(&mut self, _: &mut State, _: usize, _: usize) -> Result<()> {
                unreachable!()
            }
            fn destroy(&mut self, _: &mut State, _: usize) -> Result<()> {
                unreachable!()
            }
            fn update_task(&mut self, _: &mut State, _: usize, _: Context) -> Result<u32> {
                unreachable!()
            }
            fn child_completed(&mut self, _: &mut State, _: Context) -> Result<()> {
                unreachable!()
            }
            fn update_behavior(&mut self, _: &mut State, _: Context) -> Result<u32> {
                unreachable!()
            }
        }
        impl TaskHost for Factory {
            fn reset_navigation(&mut self, owner: usize) -> Result<()> {
                assert_eq!(owner, 7);
                self.calls.push("reset");
                Ok(())
            }
            fn create_task(&mut self, request: TaskRequest) -> Result<usize> {
                self.calls.push("create");
                self.request = Some(request);
                Ok(2)
            }
        }
        for action in 0..4 {
            let destination = [2f32.to_bits(), 3f32.to_bits(), 4f32.to_bits(), 0];
            let mut movement = ScriptedMove::default();
            movement.receive(
                &ActionEvent {
                    source: Some(7),
                    header: None,
                    action,
                    vector_bits: destination,
                    reference: Some(42),
                },
                &mut |_| Ok(Some(destination)),
            )?;
            let mut runtime = State::default();
            let mut factory = Factory::default();
            movement.enter(&mut runtime, 7, &mut factory)?;
            assert!(!movement.pending);
            assert_eq!((runtime.child, runtime.mode), (Some(2), 1));
            if action < 2 {
                assert_eq!(factory.calls, ["reset", "create", "activate"]);
                assert_eq!(
                    factory.request,
                    Some(TaskRequest::Move {
                        destination,
                        radius: if action == 0 {
                            0.5f32.to_bits()
                        } else {
                            1.2f32.to_bits()
                        },
                        speed: (-1f32).to_bits(),
                        extra: 0
                    })
                );
            } else {
                assert_eq!(factory.calls, ["create", "activate"]);
                assert_eq!(factory.request, Some(TaskRequest::Turn { destination }));
            }
        }
        Ok(())
    }
    #[test]
    fn original_instruction_fixtures_match_receiver_storage_and_return() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_scripted_move.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let words = |field: &str| -> [u32; 4] {
                std::array::from_fn(|i| case[field][i].as_u64().unwrap() as u32)
            };
            let mut state = ScriptedMove {
                pending: case["initial_pending"].as_bool().unwrap(),
                action: Some(7),
                radius_bits: Some(0x40000000),
                destination_bits: Some(words("before")),
            };
            let event = ActionEvent {
                source: Some(7),
                header: None,
                action: case["action"].as_u64().unwrap() as u32,
                vector_bits: words("vector"),
                reference: case["reference"].as_u64().map(|v| v as usize),
            };
            let returned = state.receive(&event, &mut |_| {
                Ok(case["resolved_position"]
                    .as_array()
                    .map(|_| words("resolved_position")))
            })?;
            assert_eq!(returned, case["returned"].as_bool().unwrap(), "{case}");
            assert_eq!(state.pending, case["pending"].as_bool().unwrap(), "{case}");
            assert_eq!(
                state.action,
                Some(case["stored_action"].as_u64().unwrap() as u32),
                "{case}"
            );
            assert_eq!(
                state.radius_bits,
                Some(case["radius"].as_u64().unwrap() as u32),
                "{case}"
            );
            assert_eq!(state.destination_bits, Some(words("destination")), "{case}");
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_npc_deferred_action_keeps_destination_and_resolves_position_at_delivery(
    ) -> Result<()> {
        use anyhow::Context;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .scripts
            .resolve_name("gremlinvillage_zonea.Gremlin_Variant4 01")
            .context("Original gremlin absent")?;
        anyhow::ensure!(
            !world.ai_brains[&owner].scripted_move.is_empty(),
            "Original ScriptedMove absent"
        );
        let target = world.player_entity;
        world.entity_event_queue.enqueue(
            &ActionEvent {
                source: Some(owner),
                header: None,
                action: 1,
                vector_bits: [99f32.to_bits(); 4],
                reference: Some(target),
            }
            .into(),
            None,
        )?;
        world.entities[target].transform[12..15].copy_from_slice(&[2., 3., 4.]);
        world.process_deferred_entity_events()?;
        for state in world.ai_brains[&owner].scripted_move.values() {
            assert!(state.pending);
            assert_eq!(
                state.destination_bits,
                Some([2f32.to_bits(), 3f32.to_bits(), 4f32.to_bits(), 0])
            );
            assert_eq!(state.action, Some(1));
        }
        assert!(world.ai_brains[&owner].reevaluation_requested);
        // Receipt is not execution: no replacement navigation was fabricated.
        assert!(world.ai_brains[&owner].selector.current.is_none());
        Ok(())
    }
    #[test]
    fn raw_vector_and_entity_requests_preserve_native_pending_and_retention() -> Result<()> {
        let event = |action, reference| ActionEvent {
            source: Some(7),
            header: None,
            action,
            vector_bits: [0x80000000, 0x7fc12345, 0x3f800000, 0xdeadbeef],
            reference,
        };
        let mut state = ScriptedMove::default();
        assert!(!state.receive(&event(11, None), &mut |_| unreachable!())?);
        for action in [0, 2] {
            assert!(state.receive(&event(action, None), &mut |_| unreachable!())?);
            assert_eq!(
                state.destination_bits,
                Some(event(action, None).vector_bits)
            );
            assert_eq!(state.radius_bits, Some(0x3f000000));
        }
        let position = [2f32.to_bits(), 3f32.to_bits(), 4f32.to_bits(), 0];
        assert!(state.receive(&event(1, Some(42)), &mut |id| {
            assert_eq!(id, 42);
            Ok(Some(position))
        })?);
        assert_eq!(state.destination_bits, Some(position));
        assert_eq!(state.radius_bits, Some(0x3f99999a));
        assert!(state.receive(&event(3, None), &mut |_| unreachable!())?);
        assert_eq!(state.destination_bits, Some(position));
        assert!(state.receive(&event(3, Some(43)), &mut |_| Ok(None))?);
        assert_eq!(state.destination_bits, Some(position));
        assert!(state.receive(&event(12, None), &mut |_| unreachable!())?);
        assert_eq!(state.action, Some(3));
        Ok(())
    }
}
impl ScriptedMove {
    /// REL1b8fc0..1b904c. The shared task runtime owns waiting; this update
    /// broadcasts completion even for a failed child, then returns native2.
    pub fn update_behavior(&self,owner:usize,host:&mut impl CompletionHost)->Result<u32> {
        host.reached_position(owner)?;
        Ok(2)
    }
    /// REL1b8e88: consume pending before any controller/factory callback.
    pub fn enter(
        &mut self,
        runtime: &mut super::ai_task_runtime::State,
        owner: usize,
        host: &mut impl TaskHost,
    ) -> Result<()> {
        use anyhow::Context as _;
        self.pending = false;
        let request = match self.action {
            Some(0 | 1) => {
                host.reset_navigation(owner)?;
                TaskRequest::Move {
                    destination: self
                        .destination_bits
                        .context("ScriptedMove destination was never initialized")?,
                    radius: self
                        .radius_bits
                        .context("ScriptedMove radius was never initialized")?,
                    speed: (-1f32).to_bits(),
                    extra: 0,
                }
            }
            Some(2 | 3) => TaskRequest::Turn {
                destination: self
                    .destination_bits
                    .context("ScriptedMove turn destination was never initialized")?,
            },
            _ => return Ok(()),
        };
        let task = host.create_task(request)?;
        runtime.install(task, owner, true, host)
    }
    pub fn receive(
        &mut self,
        event: &ActionEvent,
        resolve: &mut dyn FnMut(usize) -> Result<Option<[u32; 4]>>,
    ) -> Result<bool> {
        match event.action {
            0 | 2 => {
                self.action = Some(event.action);
                self.pending = true;
                self.radius_bits = Some(0x3f000000); // REL4bcff0: 0.5
                self.destination_bits = Some(event.vector_bits);
            }
            1 | 3 => {
                self.action = Some(event.action);
                self.pending = true;
                self.radius_bits = Some(0x3f99999a); // REL4bd0b8: 1.2
                if let Some(reference) = event.reference {
                    if let Some(position) = resolve(reference)? {
                        self.destination_bits = Some(position);
                    }
                }
            }
            _ => {}
        }
        // This receiver returns its pending byte even for unrelated actions.
        Ok(self.pending)
    }
}
