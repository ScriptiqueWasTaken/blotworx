//! Event-ID PostEvent producer, DOL80368d10. Name conversion, engine/object
//! admission, queue storage and playing-record registration are supplied hosts.
use anyhow::Result;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Context {
    pub object: u32,
    pub playing_id: u32,
    pub words: [u32; 3],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Command<H: Copy> {
    pub event: H,
    pub context: Context,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Registration {
    pub event: u32,
    pub context: Context,
    pub callback: u32,
    pub cookie: u32,
    pub flags: u32,
}
pub trait Control {
    type Event: Copy;
    fn acquire_event(&mut self, id: u32) -> Result<Option<Self::Event>>;
    /// Takes the producer's acquired event reference. Native queue statuses
    /// are preserved; this producer performs no compensating Release call.
    fn enqueue(&mut self, command: Command<Self::Event>) -> Result<u32>;
    fn register_playing(&mut self, registration: Registration) -> Result<u32>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        calls: Vec<Value>,
        references: u32,
    }
    impl Control for Host<'_> {
        type Event = u32;
        fn acquire_event(&mut self, id: u32) -> Result<Option<u32>> {
            self.calls.push(json!({"op":"acquire","event":id}));
            Ok(if self.case["present"].as_bool().unwrap() {
                self.references += 1;
                Some(id)
            } else {
                None
            })
        }
        fn enqueue(&mut self, command: Command<u32>) -> Result<u32> {
            self.calls
                .push(json!({"op":"queue","event":command.event,"context":command.context}));
            Ok(self.case["queue_status"].as_u64().unwrap() as u32)
        }
        fn register_playing(&mut self, registration: Registration) -> Result<u32> {
            self.calls
                .push(json!({"op":"register","registration":registration}));
            Ok(self.case["register_status"].as_u64().unwrap() as u32)
        }
    }
    #[test]
    fn original_event_producer_reference() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_ingress.json"
        ))
        .unwrap();
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let number = |key: &str| case[key].as_u64().unwrap() as u32;
            let mut host = Host {
                case,
                calls: vec![],
                references: 3,
            };
            let mut producer = Producer {
                counter: number("counter"),
            };
            let result = producer
                .post(
                    0x1234abcd,
                    0x12345678,
                    serde_json::from_value(case["words"].clone()).unwrap(),
                    number("flags"),
                    number("callback"),
                    0x87654321,
                    &mut host,
                )
                .unwrap();
            assert_eq!(result, number("api_id"), "case {index}");
            assert_eq!(producer.counter, number("after_counter"), "case {index}");
            assert_eq!(host.references, number("event_references"), "case {index}");
            assert_eq!(json!(host.calls), case["calls"], "case {index}");
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Producer {
    pub counter: u32,
}
impl Producer {
    pub fn post(
        &mut self,
        event: u32,
        object: u32,
        words: Option<[u32; 3]>,
        flags: u32,
        callback: u32,
        cookie: u32,
        host: &mut impl Control,
    ) -> Result<u32> {
        let Some(handle) = host.acquire_event(event)? else {
            return Ok(0);
        };
        // The BSS counter increments before enqueue, including failure. Retail
        // wrapping arithmetic does not skip the zero ID at rollover.
        self.counter = self.counter.wrapping_add(1);
        let context = Context {
            object,
            playing_id: self.counter,
            words: words.unwrap_or([0; 3]),
        };
        if host.enqueue(Command {
            event: handle,
            context,
        })? != 1
        {
            return Ok(0);
        }
        if callback != 0 || flags & 0x10000 != 0 {
            if host.register_playing(Registration {
                event,
                context,
                callback,
                cookie,
                flags,
            })? != 1
            {
                return Ok(0);
            }
        }
        // Registration can fail after a successful enqueue. Returning0 does
        // not prove that no command is queued and must never roll it back.
        Ok(context.playing_id)
    }
}
