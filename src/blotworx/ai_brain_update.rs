//! Outer AI tick, retail REL198e18..19904c. This composes the existing common
//! task update and selection services; it does not invent concrete behaviors.
use super::{
    ai_selection::{Lifecycle, Selector},
    ai_task_runtime::Context,
    entity_events::BehaviorFinished,
};
use anyhow::{Context as _, Result};

/// Every operation requires a real receiver. Callbacks may replace the current
/// behavior, child or ordered list, so reads after callbacks must remain fresh.
pub trait Host {
    fn update_allowed(
        &mut self,
        state: &mut Selector,
        behavior: usize,
        owner: usize,
    ) -> Result<bool>;
    /// Invoke common198b3c, including the current behavior's owned child task.
    fn update_current(
        &mut self,
        state: &mut Selector,
        behavior: usize,
        context: Context,
    ) -> Result<u32>;
    fn child(&mut self, behavior: usize) -> Result<Option<usize>>;
    fn stop_child(
        &mut self,
        state: &mut Selector,
        behavior: usize,
        child: usize,
        owner: usize,
    ) -> Result<()>;
    fn destroy_child(&mut self, state: &mut Selector, behavior: usize, child: usize) -> Result<()>;
    fn clear_child(&mut self, behavior: usize) -> Result<()>;
    fn exit(&mut self, state: &mut Selector, behavior: usize, context: Context) -> Result<()>;
    fn owner_entity(&mut self, owner: usize) -> Result<Option<usize>>;
    fn name(&mut self, behavior: usize) -> Result<String>;
    fn enqueue_finished(&mut self, state: &mut Selector, event: BehaviorFinished) -> Result<()>;
    fn selection_requested(&mut self, owner: usize) -> Result<bool>;
    /// Invoke19916c, with its original admission, interval/RNG and callbacks.
    fn select(
        &mut self,
        state: &mut Selector,
        lifecycle: &mut Lifecycle,
        owner: usize,
    ) -> Result<()>;
    fn background_active(&mut self, behavior: usize) -> Result<bool>;
    fn background_update(
        &mut self,
        state: &mut Selector,
        behavior: usize,
        context: Context,
    ) -> Result<()>;
}

pub fn advance(state:&mut Selector,lifecycle:&mut Lifecycle,context:Context,host:&mut impl Host)->Result<()> {
    advance_live(context,&mut Borrowed{state,lifecycle,host})
}

pub trait LiveHost {
    fn selector(&self)->Result<&Selector>;
    fn selector_mut(&mut self)->Result<&mut Selector>;
    fn lifecycle(&self)->Result<&Lifecycle>;
    fn lifecycle_mut(&mut self)->Result<&mut Lifecycle>;
    fn update_allowed(&mut self,behavior:usize,owner:usize)->Result<bool>;
    fn update_current(&mut self,behavior:usize,context:Context)->Result<u32>;
    fn child(&mut self,behavior:usize)->Result<Option<usize>>;
    fn stop_child(&mut self,behavior:usize,child:usize,owner:usize)->Result<()>;
    fn destroy_child(&mut self,behavior:usize,child:usize)->Result<()>;
    fn clear_child(&mut self,behavior:usize)->Result<()>;
    fn exit(&mut self,behavior:usize,context:Context)->Result<()>;
    fn owner_entity(&mut self,owner:usize)->Result<Option<usize>>;
    fn name(&mut self,behavior:usize)->Result<String>;
    fn enqueue_finished(&mut self,event:BehaviorFinished)->Result<()>;
    fn selection_requested(&mut self,owner:usize)->Result<bool>;
    fn select(&mut self,owner:usize)->Result<()>;
    fn background_active(&mut self,behavior:usize)->Result<bool>;
    fn background_update(&mut self,behavior:usize,context:Context)->Result<()>;
}
struct Borrowed<'a,H>{state:&'a mut Selector,lifecycle:&'a mut Lifecycle,host:&'a mut H}
impl<H:Host> LiveHost for Borrowed<'_,H>{
    fn selector(&self)->Result<&Selector>{Ok(self.state)}
    fn selector_mut(&mut self)->Result<&mut Selector>{Ok(self.state)}
    fn lifecycle(&self)->Result<&Lifecycle>{Ok(self.lifecycle)}
    fn lifecycle_mut(&mut self)->Result<&mut Lifecycle>{Ok(self.lifecycle)}
    fn update_allowed(&mut self,b:usize,o:usize)->Result<bool>{self.host.update_allowed(self.state,b,o)}
    fn update_current(&mut self,b:usize,c:Context)->Result<u32>{self.host.update_current(self.state,b,c)}
    fn child(&mut self,b:usize)->Result<Option<usize>>{self.host.child(b)}
    fn stop_child(&mut self,b:usize,t:usize,o:usize)->Result<()>{self.host.stop_child(self.state,b,t,o)}
    fn destroy_child(&mut self,b:usize,t:usize)->Result<()>{self.host.destroy_child(self.state,b,t)}
    fn clear_child(&mut self,b:usize)->Result<()>{self.host.clear_child(b)}
    fn exit(&mut self,b:usize,c:Context)->Result<()>{self.host.exit(self.state,b,c)}
    fn owner_entity(&mut self,o:usize)->Result<Option<usize>>{self.host.owner_entity(o)}
    fn name(&mut self,b:usize)->Result<String>{self.host.name(b)}
    fn enqueue_finished(&mut self,e:BehaviorFinished)->Result<()>{self.host.enqueue_finished(self.state,e)}
    fn selection_requested(&mut self,o:usize)->Result<bool>{self.host.selection_requested(o)}
    fn select(&mut self,o:usize)->Result<()>{self.host.select(self.state,self.lifecycle,o)}
    fn background_active(&mut self,b:usize)->Result<bool>{self.host.background_active(b)}
    fn background_update(&mut self,b:usize,c:Context)->Result<()>{self.host.background_update(self.state,b,c)}
}
/// The same source update against live World-owned fields. Every borrow ends
/// before an event/controller callback can access or reenter that World.
pub fn advance_live(context:Context,h:&mut impl LiveHost)->Result<()> {
    if let Some(current) = h.selector()?.current {
        let result = if h.update_allowed(current, context.owner)? {
            h.selector_mut()?.updating = true;
            let update = (|| {
                let current = h.selector()?.current
                    .context("AI update callback cleared its current behavior")?;
                h.update_current(current, context)
            })();
            // Rust errors are not native return codes. Release the transient
            // flag on failure, but preserve all preceding receiver side effects.
            h.selector_mut()?.updating = false;
            update?
        } else {
            2
        };
        if result == 0 || result == 2 {
            let current = h.selector_mut()?.current.take()
                .context("Finished AI update has no current behavior")?;
            if let Some(child) = h.child(current)? {
                h.stop_child(current, child, context.owner)?;
                if let Some(child) = h.child(current)? {
                    h.destroy_child(current, child)?;
                }
                h.clear_child(current)?;
            }
            h.exit(current, context)?;
            let source = h.owner_entity(context.owner)?;
            let name = h.name(current)?;
            h.enqueue_finished(
                BehaviorFinished {
                    source,
                    header: None,
                    name,
                },
            )?;
        }
    }
    h.lifecycle_mut()?.interval -= context.delta;
    // Preserve short circuiting and unordered floating comparison semantics.
    if h.lifecycle()?.interval <= 0.
        || h.selector()?.current.is_none()
        || h.selection_requested(context.owner)?
    {
        h.select(context.owner)?;
    }
    let mut at = 0;
    while let Some(&behavior) = h.selector()?.ordered.get(at) {
        if h.background_active(behavior)? {
            h.background_update(behavior, context)?;
        }
        at += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct ReferenceHost<'a> {
        input: &'a Value,
        trace: Vec<Value>,
        children: [Option<usize>; 3],
        background_changed: bool,
        completion_snapshot: (Option<usize>, bool),
    }
    impl ReferenceHost<'_> {
        fn record(&mut self, name: &str, receiver: Option<usize>, state: &Selector) {
            self.trace
                .push(json!([name, receiver, state.current, state.updating]));
        }
        fn mutate(&self, name: &str, state: &mut Selector) {
            if self.input["replace_current_at"].as_str() == Some(name) {
                state.current = Some(2);
            }
        }
        fn context(&self, context: Context) {
            assert_eq!(context.owner, 19);
            assert_eq!(context.delta.to_bits(), 0.1f32.to_bits());
        }
    }
    impl Host for ReferenceHost<'_> {
        fn update_allowed(&mut self, s: &mut Selector, b: usize, owner: usize) -> Result<bool> {
            assert_eq!(owner, 19);
            self.record("allowed", Some(b), s);
            self.mutate("allowed", s);
            Ok(self.input["allowed"].as_bool().unwrap())
        }
        fn update_current(&mut self, s: &mut Selector, b: usize, c: Context) -> Result<u32> {
            self.context(c);
            self.record("update", Some(b), s);
            self.mutate("update", s);
            Ok(self.input["result"].as_u64().unwrap() as u32)
        }
        fn child(&mut self, b: usize) -> Result<Option<usize>> {
            Ok(self.children[b - 1])
        }
        fn stop_child(
            &mut self,
            s: &mut Selector,
            b: usize,
            child: usize,
            owner: usize,
        ) -> Result<()> {
            assert_eq!(owner, 19);
            self.record("stop", Some(child), s);
            if self.input["replace_child"] == true {
                self.children[b - 1] = Some(4);
            }
            if self.input["clear_child"] == true {
                self.children[b - 1] = None;
            }
            Ok(())
        }
        fn destroy_child(&mut self, s: &mut Selector, _: usize, child: usize) -> Result<()> {
            self.record("destroy", Some(child), s);
            Ok(())
        }
        fn clear_child(&mut self, b: usize) -> Result<()> {
            self.children[b - 1] = None;
            Ok(())
        }
        fn exit(&mut self, s: &mut Selector, b: usize, c: Context) -> Result<()> {
            self.context(c);
            self.record("exit", Some(b), s);
            self.mutate("exit", s);
            self.completion_snapshot = (s.current, s.updating);
            Ok(())
        }
        fn owner_entity(&mut self, owner: usize) -> Result<Option<usize>> {
            assert_eq!(owner, 19);
            let (current, updating) = self.completion_snapshot;
            self.trace.push(json!(["owner", null, current, updating]));
            Ok(self.input["source"].as_u64().map(|v| v as usize))
        }
        fn name(&mut self, b: usize) -> Result<String> {
            let (current, updating) = self.completion_snapshot;
            self.trace.push(json!(["name", b, current, updating]));
            Ok(format!("Behavior{b}"))
        }
        fn enqueue_finished(&mut self, s: &mut Selector, event: BehaviorFinished) -> Result<()> {
            assert_eq!(
                event.source,
                self.input["source"].as_u64().map(|v| v as usize)
            );
            assert_eq!(event.header, None);
            let b = event
                .name
                .strip_prefix("Behavior")
                .unwrap()
                .parse::<usize>()?;
            self.record("enqueue", Some(b), s);
            self.mutate("enqueue", s);
            Ok(())
        }
        fn selection_requested(&mut self, owner: usize) -> Result<bool> {
            assert_eq!(owner, 19);
            Ok(self.input["force"].as_bool().unwrap())
        }
        fn select(&mut self, s: &mut Selector, l: &mut Lifecycle, owner: usize) -> Result<()> {
            assert_eq!(owner, 19);
            self.record("select", None, s);
            l.interval = 0.3;
            s.current = Some(2);
            if self.input["selection_reorders"] == true {
                s.ordered = vec![2, 1];
            }
            Ok(())
        }
        fn background_active(&mut self, b: usize) -> Result<bool> {
            Ok(b != 2)
        }
        fn background_update(&mut self, s: &mut Selector, b: usize, c: Context) -> Result<()> {
            self.context(c);
            self.record("background", Some(b), s);
            if self.input["append_background"] == true && !self.background_changed {
                self.background_changed = true;
                s.ordered.push(3);
            }
            Ok(())
        }
    }
    #[test]
    fn outer_tick_matches_original_instructions_and_callback_mutations() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_brain_update.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input = &case["input"];
            let mut state = Selector::new(vec![1, 2]);
            state.current = input["current"].as_u64().map(|v| v as usize);
            let mut lifecycle = Lifecycle::default();
            lifecycle.interval = f32::from_bits(input["timer"].as_u64().unwrap() as u32);
            let mut host = ReferenceHost {
                input,
                trace: vec![],
                background_changed: false,
                completion_snapshot: (None, false),
                children: [input["child"].as_u64().map(|v| v as usize), None, None],
            };
            advance(
                &mut state,
                &mut lifecycle,
                Context {
                    owner: 19,
                    delta: 0.1,
                },
                &mut host,
            )?;
            assert_eq!(json!(host.trace), case["trace"], "{input}");
            assert_eq!(json!(state.current), case["current"], "{input}");
            assert_eq!(json!(host.children), case["children"], "{input}");
            assert_eq!(json!(state.ordered), case["ordered"], "{input}");
            assert_eq!(json!(state.updating), case["updating"], "{input}");
            assert_eq!(
                lifecycle.interval.to_bits(),
                case["timer"].as_u64().unwrap() as u32,
                "{input}"
            );
        }
        Ok(())
    }
}
