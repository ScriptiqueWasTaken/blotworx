//! Voice termination control flow80358340..80358684. Resource implementations,
//! locking, node lists and registered-object reference bookkeeping are supplied.
use super::{audio_playing::Playing, audio_schedule::CompletionHost, audio_voice_init};
use anyhow::Result;
#[derive(Clone, Debug, serde::Serialize)]
pub struct State {
    pub voice: audio_voice_init::State,
    /// Native bytes+da and+d9. The count-decrement guard is da bit3.
    pub count_flags: u8,
    pub control_flags: u8,
    pub rtpc_targets: [Option<u32>; 2],     // +ec/+fc
    pub transition_users: [Option<u32>; 2], // +58/+5c
    pub path_flags: u32,
    pub parameter_array: Option<u32>, // +1c; native twelve-byte records
    pub resource18: Option<u32>,
    pub resource14c: Option<u32>,
}
pub trait Control {
    fn begin(&mut self) -> Result<()>;
    fn decrement_play_count(&mut self, node: u32, object: u32) -> Result<()>;
    fn deactivate_object(&mut self, object: u32) -> Result<()>;
    fn unsubscribe_rtpc(&mut self, target: u32) -> Result<()>;
    /// Execute the recovered binding release. As with that helper, an Err
    /// must still attempt later resource destruction and clear owned handles.
    fn release_parameters(&mut self, voice: u32) -> Result<()>;
    fn clear_node_path(&mut self, node: u32) -> Result<()>;
    fn detach_path(&mut self, path: u32, voice: u32) -> Result<()>;
    fn remove_transition_user(&mut self, transition: u32, voice: u32) -> Result<()>;
    fn release_object(&mut self, object: u32) -> Result<()>;
    fn destroy_position(&mut self, position: u32) -> Result<()>;
    fn free_position(&mut self, position: u32) -> Result<()>;
    fn free_parameter_array(&mut self, array: u32) -> Result<()>;
    fn unlink_voice(&mut self, node: u32, voice: u32) -> Result<()>;
    fn release_media(&mut self, source: u32) -> Result<()>;
    fn release_resource18(&mut self, resource: u32) -> Result<()>;
    fn release_node(&mut self, node: u32) -> Result<()>;
    fn release_resource14c(&mut self, resource: u32) -> Result<()>;
    fn end(&mut self) -> Result<()>;
}
/// The original resource services are void. On a Rust host error, continue
/// releasing later resources in native order and return the first diagnostic.
/// This is not idempotent: the native object is destroyed after termination.
pub fn terminate(
    state: &mut State,
    playing: &mut Playing,
    completion: &mut impl CompletionHost,
    host: &mut impl Control,
) -> Result<()> {
    host.begin()?;
    let mut first = None;
    let mut attempt = |result: Result<()>| {
        if let Err(error) = result {
            if first.is_none() {
                first = Some(error);
            }
        }
    };
    let voice = state.voice.voice;
    let node = state.voice.node;
    let object = state.voice.object;
    if state.count_flags & 8 == 0 {
        state.count_flags |= 8;
        attempt(host.decrement_play_count(node, object));
    }
    attempt(host.deactivate_object(object));
    for target in state.rtpc_targets.into_iter().flatten() {
        attempt(host.unsubscribe_rtpc(target));
    }
    if state.voice.positioning.is_some() {
        attempt(host.unsubscribe_rtpc(voice));
    }
    attempt(host.release_parameters(voice));
    // Recovered8035b3b0 clears the released+ec/+fc handles. Do not leave this
    // lifecycle view pointing at resources that the binding stage destroyed.
    state.rtpc_targets = [None; 2];
    if let Some(path) = state.voice.path {
        if state.path_flags & 2 != 0 {
            attempt(host.clear_node_path(node));
        }
        attempt(host.detach_path(path, voice));
        state.voice.path = None;
        state.voice.path_id = 0;
    }
    for transition in state.transition_users.into_iter().flatten() {
        attempt(host.remove_transition_user(transition, voice));
    }
    state.control_flags &= !0x20;
    if state.voice.playing_id != 0 {
        if let Some(done) = playing.remove_voice(state.voice.playing_id, completion) {
            if let Some(end) = done.end {
                attempt(completion.end_event(end));
            }
        }
    }
    attempt(host.release_object(object));
    if let Some(position) = state.voice.positioning {
        attempt(host.destroy_position(position));
        attempt(host.free_position(position));
        state.voice.positioning = None;
    }
    if let Some(array) = state.parameter_array {
        attempt(host.free_parameter_array(array));
        state.parameter_array = None;
    }
    attempt(host.unlink_voice(node, voice));
    if state.voice.media[0] != 0 {
        attempt(host.release_media(state.voice.source));
    }
    if let Some(resource) = state.resource18 {
        attempt(host.release_resource18(resource));
        state.resource18 = None;
    }
    attempt(host.release_node(node));
    if let Some(resource) = state.resource14c {
        attempt(host.release_resource14c(resource));
        state.resource14c = None;
    }
    attempt(host.end());
    if let Some(error) = first {
        Err(error)
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::{cell::RefCell, rc::Rc};
    struct Host {
        calls: Rc<RefCell<Vec<Value>>>,
        references: u32,
        node_path: [u32; 2],
        node_head: u32,
        bindings: super::super::audio_voice_bindings::State,
        fail: bool,
    }
    impl Host {
        fn log(&mut self, value: Value) -> Result<()> {
            self.calls.borrow_mut().push(value);
            Ok(())
        }
    }
    impl Control for Host {
        fn begin(&mut self) -> Result<()> {
            self.log(json!({"op":"begin"}))
        }
        fn decrement_play_count(&mut self, _: u32, _: u32) -> Result<()> {
            self.log(json!({"op":"decrement"}))
        }
        fn deactivate_object(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"deactivate"}))?;
            anyhow::ensure!(!self.fail, "Injected object service error");
            Ok(())
        }
        fn unsubscribe_rtpc(&mut self, target: u32) -> Result<()> {
            self.log(json!({"op":"unsubscribe","target":target}))
        }
        fn release_parameters(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"parameters"}))?;
            let mut bindings = std::mem::take(&mut self.bindings);
            let result = super::super::audio_voice_bindings::release(&mut bindings, self);
            self.bindings = bindings;
            result
        }
        fn clear_node_path(&mut self, _: u32) -> Result<()> {
            self.node_path = [0, 0];
            self.log(json!({"op":"clear_node_path"}))
        }
        fn detach_path(&mut self, _: u32, _: u32) -> Result<()> {
            self.log(json!({"op":"detach_path"}))
        }
        fn remove_transition_user(&mut self, transition: u32, _: u32) -> Result<()> {
            self.log(json!({"op":"transition","target":transition}))
        }
        fn release_object(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"release_object"}))?;
            self.references = self.references.wrapping_sub(1);
            if self.references == 0 {
                self.log(json!({"op":"destroy_object"}))?;
                self.log(json!({"op":"free_object"}))?;
            }
            Ok(())
        }
        fn destroy_position(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"destroy_position"}))
        }
        fn free_position(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"free_position"}))
        }
        fn free_parameter_array(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"free_array"}))
        }
        fn unlink_voice(&mut self, _: u32, voice: u32) -> Result<()> {
            assert_eq!(self.node_head, voice);
            self.node_head = 0;
            self.log(json!({"op":"unlink"}))
        }
        fn release_media(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"media"}))
        }
        fn release_resource18(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"resource18"}))
        }
        fn release_node(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"release_node"}))
        }
        fn release_resource14c(&mut self, _: u32) -> Result<()> {
            self.log(json!({"op":"resource14c"}))
        }
        fn end(&mut self) -> Result<()> {
            self.log(json!({"op":"end"}))
        }
    }
    impl super::super::audio_voice_bindings::Control for Host {
        fn source(&mut self, _: u32) -> Result<super::super::audio_voice_bindings::Source> {
            anyhow::bail!("Unexpected initialization during termination")
        }
        fn clone_resource(&mut self, _: u32) -> Result<Option<u32>> {
            anyhow::bail!("Unexpected clone during termination")
        }
        fn attach(&mut self, _: u32, _: u32) -> Result<()> {
            anyhow::bail!("Unexpected attach during termination")
        }
        fn descriptor(
            &mut self,
            _: u32,
            _: u32,
        ) -> Result<super::super::audio_voice_bindings::Descriptor> {
            anyhow::bail!("Unexpected descriptor during termination")
        }
        fn node_flag(&mut self, _: u32, _: u32) -> Result<u32> {
            anyhow::bail!("Unexpected flag query during termination")
        }
        fn detach(&mut self, resource: u32) -> Result<()> {
            self.log(json!({"op":"binding_detach","resource":resource}))
        }
        fn destroy(&mut self, resource: u32) -> Result<()> {
            self.log(json!({"op":"binding_destroy","resource":resource}))
        }
    }
    struct Completed {
        calls: Rc<RefCell<Vec<Value>>>,
    }
    impl super::super::audio_playing::CompletionHost for Completed {
        fn remove_source_position(&mut self, _: u32) {
            self.calls
                .borrow_mut()
                .push(json!({"op":"remove_position"}));
        }
    }
    impl CompletionHost for Completed {
        fn end_event(&mut self, _: super::super::audio_playing::EndEvent) -> Result<()> {
            self.calls.borrow_mut().push(json!({"op":"end_event"}));
            Ok(())
        }
    }
    fn setup(case: &Value) -> (State, Playing, Completed, Host) {
        let features = case["features"].as_u64().unwrap();
        let option = |bit: u32, value: u32| {
            if features & (1u64 << bit) != 0 {
                Some(value)
            } else {
                None
            }
        };
        let id = case["playing_id"].as_u64().unwrap() as u32;
        let calls = Rc::new(RefCell::new(Vec::new()));
        let mut completed = Completed {
            calls: calls.clone(),
        };
        let mut playing = Playing::default();
        if id != 0 {
            playing.register(super::super::audio_ingress::Registration {
                event: 17,
                context: super::super::audio_ingress::Context {
                    object: 42,
                    playing_id: id,
                    words: [0; 3],
                },
                callback: 1,
                cookie: 0,
                flags: 0x10001,
            });
            playing.add_voice(id);
            assert!(playing.remove_action(id, &mut completed).is_none());
        }
        let state = State {
            voice: audio_voice_init::State {
                voice: 0x72000000,
                node: 0x72020000,
                source: 0x72030000,
                object: 0x72010000,
                playing_id: id,
                callback_flags: 0x10001,
                positioning: option(2, 0x72040000),
                path: option(3, 0x72050000),
                path_id: 123,
                media: [option(9, 401).unwrap_or(0), 4096],
            },
            count_flags: case["count_flags"].as_u64().unwrap() as u8,
            control_flags: 255,
            rtpc_targets: [option(0, 0x72090000), option(1, 0x720a0000)],
            transition_users: [option(4, 201), option(5, 202)],
            path_flags: case["path_flags"].as_u64().unwrap() as u32,
            parameter_array: option(6, 0x72070000),
            resource18: option(7, 301),
            resource14c: option(8, 302),
        };
        let bindings = super::super::audio_voice_bindings::State {
            node_id: if features & 1 != 0 { 44 } else { u32::MAX },
            source_id: if features & 2 != 0 { 55 } else { u32::MAX },
            node_resource: state.rtpc_targets[0],
            source_resource: state.rtpc_targets[1],
            ..Default::default()
        };
        let host = Host {
            calls,
            references: case["references"].as_u64().unwrap() as u32,
            node_path: [9, 8],
            node_head: state.voice.voice,
            bindings,
            fail: false,
        };
        (state, playing, completed, host)
    }
    #[test]
    fn original_voice_termination_order_flags_resources_and_playing_completion() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_term.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let (mut state, mut playing, mut completed, mut host) = setup(case);
            terminate(&mut state, &mut playing, &mut completed, &mut host)?;
            assert_eq!(
                host.calls.borrow().as_slice(),
                case["calls"].as_array().unwrap().as_slice(),
                "case {index}"
            );
            let output = json!({"count_flags":state.count_flags,"control_flags":state.control_flags,"path":state.voice.path.unwrap_or(0),
                "path_id":state.voice.path_id,"positioning":state.voice.positioning.unwrap_or(0),"array":state.parameter_array.unwrap_or(0),
                "resource18":state.resource18.unwrap_or(0),"resource14c":state.resource14c.unwrap_or(0),"object_references":host.references,
                "node_head":host.node_head,"node_path":host.node_path,"binding_targets":state.rtpc_targets,"binding_ids":[host.bindings.node_id,host.bindings.source_id]});
            assert_eq!(output, case["output"], "case {index}");
            assert!(playing.record(state.voice.playing_id).is_none());
        }
        Ok(())
    }
    #[test]
    fn host_error_does_not_abandon_later_termination_releases_or_unlock() -> Result<()> {
        let case =
            json!({"features":1023,"playing_id":7,"count_flags":0,"path_flags":2,"references":1});
        let (mut state, mut playing, mut completed, mut host) = setup(&case);
        host.fail = true;
        assert!(
            terminate(&mut state, &mut playing, &mut completed, &mut host)
                .unwrap_err()
                .to_string()
                .contains("Injected object")
        );
        assert_eq!(host.calls.borrow().last(), Some(&json!({"op":"end"})));
        assert_eq!(host.references, 0);
        assert_eq!(host.node_head, 0);
        assert!(state.voice.positioning.is_none() && state.voice.path.is_none());
        assert!(
            state.resource14c.is_none()
                && state.resource18.is_none()
                && state.parameter_array.is_none()
        );
        assert!(playing.record(7).is_none());
        Ok(())
    }
}
