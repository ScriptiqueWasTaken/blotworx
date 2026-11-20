//! Playing-manager records,8035ba30/8035bd00/8035c640..8035c96c.
//! Source-position cleanup precedes removal; end callbacks follow removal and
//! run outside the caller's registry lock, matching8035bd00.
use super::audio_ingress::Registration;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Record {
    pub voices: u32,
    pub actions: u32,
    pub event: u32,
    pub object: u32,
    pub words: [u32; 3],
    pub callback: u32,
    pub cookie: u32,
    pub flags: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    #[derive(Default)]
    struct Host {
        effects: Vec<Value>,
    }
    impl CompletionHost for Host {
        fn remove_source_position(&mut self, id: u32) {
            self.effects
                .push(json!({"op":"source_position","playing_id":id}));
        }
    }
    #[test]
    fn original_playing_manager_reference() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_playing.json"
        ))
        .unwrap();
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut playing = Playing::default();
            for step in case["steps"].as_array().unwrap() {
                let operation = &step["operation"];
                let id = operation["id"].as_u64().unwrap_or(0) as u32;
                let mut host = Host::default();
                let mut flags = None;
                let completion = match operation["op"].as_str().unwrap() {
                    "register" => {
                        playing.register(
                            serde_json::from_value(operation["registration"].clone()).unwrap(),
                        );
                        None
                    }
                    "add_voice" => {
                        flags = playing.add_voice(id);
                        None
                    }
                    "add_action" => {
                        playing.add_action(id);
                        None
                    }
                    "remove_voice" => playing.remove_voice(id, &mut host),
                    "remove_action" => playing.remove_action(id, &mut host),
                    "cancel" => {
                        playing.cancel_cookie(id);
                        None
                    }
                    other => panic!("unknown reference operation {other}"),
                };
                if let Some(completion) = completion {
                    assert!(playing.record(completion.playing_id).is_none());
                    host.effects
                        .push(json!({"op":"free","playing_id":completion.playing_id}));
                    if let Some(end) = completion.end {
                        let mut event = serde_json::to_value(end).unwrap();
                        event["op"] = json!("end");
                        host.effects.push(event);
                    }
                }
                assert_eq!(
                    json!(playing.records),
                    step["records"],
                    "case {index}, operation {operation}"
                );
                assert_eq!(
                    json!(host.effects),
                    step["effects"],
                    "case {index}, operation {operation}"
                );
                assert_eq!(
                    json!(flags),
                    step["voice_flags"],
                    "case {index}, operation {operation}"
                );
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct EndEvent {
    pub callback: u32,
    pub cookie: u32,
    pub object: u32,
    pub playing_id: u32,
    pub event: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Completion {
    pub playing_id: u32,
    pub source_position_removed: bool,
    pub end: Option<EndEvent>,
}
pub trait CompletionHost {
    fn remove_source_position(&mut self, playing_id: u32);
}
#[derive(Default)]
pub struct Playing {
    records: BTreeMap<u32, Record>,
}
impl Playing {
    /// Rust allocation is supplied here; native allocator failure is handled
    /// by the producer host's registration result, not synthesized as success.
    pub fn register(&mut self, registration: Registration) {
        let Registration {
            event,
            context,
            callback,
            cookie,
            flags,
        } = registration;
        self.records.insert(
            context.playing_id,
            Record {
                voices: 0,
                actions: 1,
                event,
                object: context.object,
                words: context.words,
                callback,
                cookie,
                flags: if callback == 0 {
                    flags & 0xffff0004
                } else {
                    flags
                },
            },
        );
    }
    pub fn record(&self, id: u32) -> Option<&Record> {
        self.records.get(&id)
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn cancel_cookie(&mut self, cookie: u32) {
        for record in self
            .records
            .values_mut()
            .filter(|record| record.cookie == cookie)
        {
            record.callback = 0;
            record.flags &= 0xffff0000;
        }
    }
    ///8035c640 always returns status1. Its flags out-parameter is written only
    /// when the record exists, represented here by Some(flags).
    pub fn add_voice(&mut self, id: u32) -> Option<u32> {
        let record = self.records.get_mut(&id)?;
        record.voices = record.voices.wrapping_add(1);
        Some(record.flags)
    }
    pub fn add_action(&mut self, id: u32) {
        //8035c7e0 explicitly skips0; registration/removal/voice paths do not.
        if id != 0 {
            if let Some(record) = self.records.get_mut(&id) {
                record.actions = record.actions.wrapping_add(1);
            }
        }
    }
    pub fn remove_voice(
        &mut self,
        id: u32,
        host: &mut (impl CompletionHost + ?Sized),
    ) -> Option<Completion> {
        let record = self.records.get_mut(&id)?;
        record.voices = record.voices.wrapping_sub(1);
        self.finish(id, host)
    }
    pub fn remove_action(
        &mut self,
        id: u32,
        host: &mut (impl CompletionHost + ?Sized),
    ) -> Option<Completion> {
        let record = self.records.get_mut(&id)?;
        record.actions = record.actions.wrapping_sub(1);
        self.finish(id, host)
    }
    fn finish(&mut self, id: u32, host: &mut (impl CompletionHost + ?Sized)) -> Option<Completion> {
        let record = self.records.get(&id)?;
        if record.voices != 0 || record.actions != 0 {
            return None;
        }
        let source_position_removed = record.flags & 0x10000 != 0;
        if source_position_removed {
            host.remove_source_position(id);
        }
        let record = self.records.remove(&id).unwrap();
        Some(Completion {
            playing_id: id,
            source_position_removed,
            end: if record.flags & 1 != 0 {
                Some(EndEvent {
                    callback: record.callback,
                    cookie: record.cookie,
                    object: record.object,
                    playing_id: id,
                    event: record.event,
                })
            } else {
                None
            },
        })
    }
}
