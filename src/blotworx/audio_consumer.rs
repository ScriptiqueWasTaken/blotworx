//! Queued PostEvent branch,8036cde8..8036cf50. The acquired event and
//! pending objects belong to the audio command phase, not the game tick.
use super::audio_ingress::{Command, Context};
use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingContext {
    pub playing_id: u32,
    pub words: [u32; 3],
}
pub trait Control {
    type Event: Copy;
    type Object: Copy;
    type Action: Copy;
    type Pending;
    fn acquire_object(&mut self, id: u32) -> Result<Option<Self::Object>>;
    /// Ordered action handles and native kind words. Preserve duplicates.
    fn event_actions(&mut self, event: Self::Event) -> Result<Vec<(Self::Action, u32)>>;
    /// Successful construction retains the supplied object for the pending
    /// action's lifetime. Failed allocation does not retain it. Only these
    /// three context words are initialized by the original consumer; native
    /// padding at pending+24 is not invented as a fourth event parameter.
    fn allocate_pending(
        &mut self,
        action: Self::Action,
        object: Option<Self::Object>,
        context: PendingContext,
    ) -> Result<Option<Self::Pending>>;
    /// Takes ownership, including eventual object release on destruction.
    fn admit_pending(&mut self, pending: Self::Pending) -> Result<()>;
    fn release_object(&mut self, object: Self::Object);
    fn finish_event(&mut self, playing_id: u32) -> Result<()>;
    fn release_event(&mut self, event: Self::Event);
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub admitted: usize,
    pub missing_object: usize,
    pub allocation_failed: usize,
}

pub fn consume<H: Control>(command: Command<H::Event>, host: &mut H) -> Result<Outcome> {
    let Context {
        object: id,
        playing_id,
        words,
    } = command.context;
    let mut object = None;
    let result: Result<Outcome> = (|| {
        object = host.acquire_object(id)?;
        let mut outcome = Outcome::default();
        for (action, kind) in host.event_actions(command.event)? {
            let target = if kind & 1 != 0 {
                let Some(object) = object else {
                    outcome.missing_object += 1;
                    continue;
                };
                Some(object)
            } else {
                None
            };
            if let Some(pending) =
                host.allocate_pending(action, target, PendingContext { playing_id, words })?
            {
                host.admit_pending(pending)?;
                outcome.admitted += 1;
            } else {
                outcome.allocation_failed += 1;
            }
        }
        Ok(outcome)
    })();
    if let Some(object) = object {
        host.release_object(object);
    }
    // Unlike the scheduler's per-action completion call, this original event
    // completion is unconditional, including ID zero and empty events.
    let finished = host.finish_event(playing_id);
    host.release_event(command.event);
    let outcome = result?;
    finished?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        calls: Vec<Value>,
        references: u32,
        allocations: i64,
    }
    impl Control for Host<'_> {
        type Event = u32;
        type Object = u32;
        type Action = usize;
        type Pending = (usize, Option<u32>, PendingContext);
        fn acquire_object(&mut self, _: u32) -> Result<Option<u32>> {
            Ok(if self.case["present"].as_bool().unwrap() {
                self.references += 1;
                Some(1)
            } else {
                None
            })
        }
        fn event_actions(&mut self, _: u32) -> Result<Vec<(usize, u32)>> {
            Ok(self.case["kinds"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, k)| (i, k.as_u64().unwrap() as u32))
                .collect())
        }
        fn allocate_pending(
            &mut self,
            action: usize,
            object: Option<u32>,
            context: PendingContext,
        ) -> Result<Option<Self::Pending>> {
            let success = self.allocations != self.case["fail_at"].as_i64().unwrap();
            self.allocations += 1;
            self.calls.push(json!({"op":"allocate","success":success}));
            Ok(if success {
                if object.is_some() {
                    self.references += 1;
                }
                Some((action, object, context))
            } else {
                None
            })
        }
        fn admit_pending(&mut self, (action, object, context): Self::Pending) -> Result<()> {
            self.calls.push(json!({"op":"admit","action":action,"object":object.unwrap_or(0),"playing_id":context.playing_id,"words":context.words}));
            Ok(())
        }
        fn release_object(&mut self, _: u32) {
            self.references -= 1;
        }
        fn finish_event(&mut self, id: u32) -> Result<()> {
            self.calls
                .push(json!({"op":"finish_event","playing_id":id}));
            Ok(())
        }
        fn release_event(&mut self, _: u32) {
            self.calls.push(json!({"op":"release_event"}));
        }
    }
    #[test]
    fn original_event_consumer_scope_order_context_and_references() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_consumer.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut host = Host {
                case,
                calls: vec![],
                references: 3,
                allocations: 0,
            };
            let outcome = consume(
                Command {
                    event: 5,
                    context: Context {
                        object: 0x12345678,
                        playing_id: case["playing_id"].as_u64().unwrap() as u32,
                        words: [1, 2, 0xabcdef01],
                    },
                },
                &mut host,
            )?;
            assert_eq!(json!(host.calls), case["calls"], "case {case}");
            assert_eq!(json!(host.references), case["object_references"]);
            assert_eq!(
                outcome.admitted,
                host.calls.iter().filter(|v| v["op"] == "admit").count()
            );
            assert_eq!(
                outcome.admitted + outcome.missing_object + outcome.allocation_failed,
                case["kinds"].as_array().unwrap().len()
            );
        }
        Ok(())
    }
}
