//! Shared behavior/child-task ownership and update, REL198a54/198b3c.
//! Hosts must supply actual task and behavior callbacks; none default to no-op.
use anyhow::{Context as _, Result};

#[derive(Clone, Copy)]
pub struct Context {
    pub owner: usize,
    pub delta: f32,
}

#[derive(Default, Debug, serde::Serialize)]
pub struct State {
    pub child: Option<usize>,
    pub previous_kind: u32,
    pub mode: u32,
    pub timer: f32,
}

pub trait Host {
    fn kind(&mut self, task: usize) -> Result<u32>;
    fn activate(&mut self, state: &mut State, task: usize, owner: usize) -> Result<()>;
    fn stop(&mut self, state: &mut State, task: usize, owner: usize) -> Result<()>;
    fn destroy(&mut self, state: &mut State, task: usize) -> Result<()>;
    fn update_task(&mut self, state: &mut State, task: usize, context: Context) -> Result<u32>;
    fn child_completed(&mut self, state: &mut State, context: Context) -> Result<()>;
    fn update_behavior(&mut self, state: &mut State, context: Context) -> Result<u32>;
}

/// Canonical-state interface for World callbacks. The state stays in its
/// owning brain while animation activation/feedback may synchronously reenter.
pub trait LiveHost {
    fn child(&self) -> Result<Option<usize>>;
    fn set_child(&mut self, child: Option<usize>) -> Result<()>;
    fn set_previous_kind(&mut self, kind: u32) -> Result<()>;
    fn mode(&self) -> Result<u32>;
    fn set_mode(&mut self, mode: u32) -> Result<()>;
    fn timer(&self) -> Result<f32>;
    fn set_timer(&mut self, timer: f32) -> Result<()>;
    fn kind(&mut self, child: usize) -> Result<u32>;
    fn stop(&mut self, child: usize, owner: usize) -> Result<()>;
    fn destroy(&mut self, child: usize) -> Result<()>;
    fn update_task(&mut self, child: usize, context: Context) -> Result<u32>;
    fn child_completed(&mut self, context: Context) -> Result<()>;
    fn update_behavior(&mut self, context: Context) -> Result<u32>;
}

/// Complete198b3c common update, with fresh owner reads after each callback.
pub fn update_live(context: Context, host: &mut impl LiveHost) -> Result<u32> {
    let mut child_result = 1;
    if let Some(child) = host.child()? {
        child_result = host.update_task(child, context)?;
        if child_result == 1 {
            let current = host.child()?.context("Completed native task lost its current owner")?;
            let kind = host.kind(current)?;
            host.set_previous_kind(kind)?;
            host.child_completed(context)?;
            let child = host.child()?.context("Native task stop has no current child")?;
            host.stop(child, context.owner)?;
            if let Some(child) = host.child()? { host.destroy(child)?; }
            host.set_child(None)?;
        }
    }
    match host.mode()? {
        1 if child_result != 1 => return Ok(1),
        1 => host.set_mode(0)?,
        0 => {},
        _ => return Ok(2),
    }
    let timer = host.timer()?;
    if timer <= 0. { host.update_behavior(context) } else {
        host.set_timer(timer - context.delta)?;
        Ok(1)
    }
}

impl State {
    /// Fresh child reads after callbacks intentionally retain native reentrancy.
    fn stop_and_destroy(&mut self, owner: usize, host: &mut impl Host) -> Result<()> {
        let child = self
            .child
            .context("Native task stop has no current child")?;
        host.stop(self, child, owner)?;
        if let Some(child) = self.child {
            host.destroy(self, child)?;
        }
        self.child = None;
        Ok(())
    }

    pub fn install(
        &mut self,
        child: usize,
        owner: usize,
        wait: bool,
        host: &mut impl Host,
    ) -> Result<()> {
        if let Some(old) = self.child {
            self.previous_kind = host.kind(old)?;
            self.stop_and_destroy(owner, host)?;
        }
        self.child = Some(child);
        host.activate(self, child, owner)?;
        // False preserves the existing mode, rather than resetting it.
        if wait {
            self.mode = 1;
        }
        Ok(())
    }

    pub fn update(&mut self, context: Context, host: &mut impl Host) -> Result<u32> {
        struct Binding<'a,H> { state: &'a mut State, host: &'a mut H }
        impl<H:Host> LiveHost for Binding<'_,H> {
            fn child(&self)->Result<Option<usize>> {Ok(self.state.child)}
            fn set_child(&mut self,v:Option<usize>)->Result<()> {self.state.child=v;Ok(())}
            fn set_previous_kind(&mut self,v:u32)->Result<()> {self.state.previous_kind=v;Ok(())}
            fn mode(&self)->Result<u32> {Ok(self.state.mode)}
            fn set_mode(&mut self,v:u32)->Result<()> {self.state.mode=v;Ok(())}
            fn timer(&self)->Result<f32> {Ok(self.state.timer)}
            fn set_timer(&mut self,v:f32)->Result<()> {self.state.timer=v;Ok(())}
            fn kind(&mut self,c:usize)->Result<u32> {self.host.kind(c)}
            fn stop(&mut self,c:usize,o:usize)->Result<()> {self.host.stop(self.state,c,o)}
            fn destroy(&mut self,c:usize)->Result<()> {self.host.destroy(self.state,c)}
            fn update_task(&mut self,c:usize,x:Context)->Result<u32> {self.host.update_task(self.state,c,x)}
            fn child_completed(&mut self,x:Context)->Result<()> {self.host.child_completed(self.state,x)}
            fn update_behavior(&mut self,x:Context)->Result<u32> {self.host.update_behavior(self.state,x)}
        }
        update_live(context,&mut Binding {state:self,host})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ReferenceHost<'a> {
        input: &'a serde_json::Value,
        trace: Vec<serde_json::Value>,
    }
    impl ReferenceHost<'_> {
        fn call(&mut self, name: &str, task: Option<usize>, state: &mut State) {
            self.trace.push(match task {
                Some(id) => serde_json::json!([name, id]),
                None => serde_json::json!([name]),
            });
            if self.input["replace_at"].as_str() == Some(name) {
                state.child = Some(3);
            }
            if name == "activate" {
                if let Some(mode) = self.input["activate_mode"].as_u64() {
                    state.mode = mode as u32;
                }
            }
            if name == "completed" {
                if let Some(mode) = self.input["completed_mode"].as_u64() {
                    state.mode = mode as u32;
                }
            }
        }
        fn context(&self, context: Context) {
            assert_eq!(context.owner, 7);
            assert_eq!(context.delta.to_bits(), 0.1f32.to_bits());
        }
    }
    impl Host for ReferenceHost<'_> {
        fn kind(&mut self, task: usize) -> Result<u32> {
            self.trace.push(serde_json::json!(["kind", task]));
            Ok(task as u32 * 100)
        }
        fn activate(&mut self, s: &mut State, t: usize, o: usize) -> Result<()> {
            assert_eq!(o, 7);
            self.call("activate", Some(t), s);
            Ok(())
        }
        fn stop(&mut self, s: &mut State, t: usize, o: usize) -> Result<()> {
            assert_eq!(o, 7);
            self.call("stop", Some(t), s);
            Ok(())
        }
        fn destroy(&mut self, s: &mut State, t: usize) -> Result<()> {
            self.call("destroy", Some(t), s);
            Ok(())
        }
        fn update_task(&mut self, s: &mut State, t: usize, c: Context) -> Result<u32> {
            self.context(c);
            self.call("tick", Some(t), s);
            Ok(self.input["result"].as_u64().unwrap() as u32)
        }
        fn child_completed(&mut self, s: &mut State, c: Context) -> Result<()> {
            self.context(c);
            self.call("completed", None, s);
            Ok(())
        }
        fn update_behavior(&mut self, s: &mut State, c: Context) -> Result<u32> {
            self.context(c);
            self.call("behavior", None, s);
            Ok(6)
        }
    }
    #[test]
    fn install_and_update_match_original_instructions_including_reentrant_child_changes(
    ) -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_task_runtime.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input = &case["input"];
            let child = input["child"].as_u64().unwrap();
            let mut state = State {
                child: (child != 0).then_some(child as usize),
                previous_kind: 17,
                mode: input["mode"].as_u64().unwrap() as u32,
                timer: f32::from_bits(input["timer"].as_u64().unwrap() as u32),
            };
            let mut host = ReferenceHost {
                input,
                trace: vec![],
            };
            let returned = if input["op"] == "install" {
                state.install(2, 7, input["wait"].as_bool().unwrap(), &mut host)?;
                None
            } else {
                Some(state.update(
                    Context {
                        owner: 7,
                        delta: 0.1,
                    },
                    &mut host,
                )?)
            };
            assert_eq!(serde_json::to_value(&host.trace)?, case["trace"], "{input}");
            assert_eq!(
                state.child,
                case["child"].as_u64().map(|v| v as usize),
                "{input}"
            );
            assert_eq!(
                state.previous_kind,
                case["previous_kind"].as_u64().unwrap() as u32,
                "{input}"
            );
            assert_eq!(state.mode, case["mode"].as_u64().unwrap() as u32, "{input}");
            assert_eq!(
                state.timer.to_bits(),
                case["timer"].as_u64().unwrap() as u32,
                "{input}"
            );
            assert_eq!(
                returned,
                case["returned"].as_u64().map(|v| v as u32),
                "{input}"
            );
        }
        Ok(())
    }
}
