//! Ordinary non-continuous switch Play,8032f730..8032fb8c.
//! Group-value resolution and retained child voices belong to the audio host.
//! Do not flatten a switch package: every admitted child is dispatched.
use super::{
    audio_bank::container::{Switch, SwitchGroup, SwitchParameter},
    audio_play_action::Request,
};
use anyhow::{ensure, Result};
use std::collections::BTreeMap;

pub struct Definition {
    pub default: u32,
    pub continuous: bool,
    pub groups: Vec<SwitchGroup>,
    pub parameters: Vec<SwitchParameter>,
}
impl Definition {
    pub fn from_container(container: &Switch<'_>) -> Self {
        Self {
            default: container.default,
            continuous: container.continuous,
            groups: container.runtime_groups(),
            parameters: container.runtime_parameters(),
        }
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, serde::Serialize)]
pub struct History {
    pub group: u32,
    pub count: u32,
}
#[derive(Default)]
pub struct Runtime {
    history: BTreeMap<(u32, u32), History>,
}
pub trait Control {
    type Node: Copy;
    fn acquire(&mut self, id: u32) -> Result<Option<Self::Node>>;
    /// Preserve the originating request, including its container target and
    /// sample offset. The leaf is the acquired node, not a rewritten request.
    fn play(&mut self, node: Self::Node, request: &Request) -> Result<u32>;
    fn release(&mut self, node: Self::Node);
}
impl Runtime {
    pub fn history(&self, container: u32, object: u32) -> History {
        self.history
            .get(&(container, object))
            .copied()
            .unwrap_or_default()
    }
    /// Call on final registered-object retirement, after retained users release
    /// it. A new registration must use its own identity even for the same ID.
    pub fn retire_object(&mut self, object: u32) {
        self.history.retain(|&(_, owner), _| owner != object);
    }
    pub fn begin(
        &mut self,
        container: u32,
        definition: &Definition,
        value: u32,
        request: &Request,
    ) -> Result<Option<Vec<u32>>> {
        ensure!(request.kind==0 && !definition.continuous,"Continuous/continuation switch playback requires retained requests and switch notifications");
        let package = definition
            .groups
            .iter()
            .find(|group| group.id == value)
            .or_else(|| {
                definition
                    .groups
                    .iter()
                    .find(|group| group.id == definition.default)
            });
        let Some(package) = package else { return Ok(None) };
        let history = self
            .history
            .entry((container, request.context.object))
            .or_default();
        if history.group == package.id {
            history.count = history.count.wrapping_add(1);
        } else {
            *history = History {
                group: package.id,
                count: 1,
            };
        }
        let first = history.count == 1;
        Ok(Some(package.nodes.iter().copied().filter(|id| first || !definition.parameters.iter().find(|parameter|parameter.node==*id).is_some_and(|parameter|parameter.first_only)).collect()))
    }
    pub fn play(
        &mut self,container:u32,definition:&Definition,value:u32,request:&Request,host:&mut impl Control,
    )->Result<u32> {
        let Some(nodes)=self.begin(container,definition,value,request)? else {return Ok(2)};
        let mut successes = 0;
        let mut status = 2;
        for id in nodes {
            if request.native_zero_words[4] != 0 && successes != 0 {
                continue;
            }
            let Some(node) = host.acquire(id)? else {
                continue;
            };
            let result = host.play(node, request);
            host.release(node);
            status = result?;
            if status == 1 {
                successes += 1;
            }
        }
        // The original returns the last attempted child's status, including a
        // failure after a previous success. Missing/skipped children retain it.
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Host<'a> {
        step: &'a serde_json::Value,
        calls: Vec<serde_json::Value>,
        request: Request,
        fail: bool,
    }
    impl Control for Host<'_> {
        type Node = u32;
        fn acquire(&mut self, id: u32) -> Result<Option<u32>> {
            self.calls.push(serde_json::json!(["acquire", id]));
            Ok(self.step["loaded"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| *value == id)
                .then_some(id))
        }
        fn play(&mut self, id: u32, request: &Request) -> Result<u32> {
            assert_eq!(request, &self.request);
            self.calls.push(serde_json::json!(["play", id]));
            ensure!(!self.fail, "Injected child failure");
            Ok(self.step["statuses"][id.to_string()].as_u64().unwrap() as u32)
        }
        fn release(&mut self, id: u32) {
            self.calls.push(serde_json::json!(["release", id]));
        }
    }
    #[test]
    fn original_switch_play_package_fallback_first_only_status_and_owner_history() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_switch_play.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let definition = Definition {
                default: 9,
                continuous: false,
                groups: vec![
                    SwitchGroup {
                        id: 7,
                        nodes: vec![100, 101],
                    },
                    SwitchGroup {
                        id: 9,
                        nodes: vec![102],
                    },
                    SwitchGroup {
                        id: 11,
                        nodes: Vec::new(),
                    },
                ],
                parameters: vec![SwitchParameter {
                    node: 100,
                    first_only: case["first_only"].as_bool().unwrap(),
                    continue_playback: false,
                    on_switch: 0,
                    fade_in: 0,
                    fade_out: 0,
                }],
            };
            let mut runtime = Runtime::default();
            for step in case["steps"].as_array().unwrap() {
                let object = step["owner"].as_u64().unwrap() as u32;
                let request = Request {
                    target: 777,
                    context: super::super::audio_play_action::Context {
                        object,
                        playing_id: 42,
                        sample_offset: 13,
                        words: [1, 2, 3, 4],
                    },
                    transition: 17,
                    curve: 4,
                    kind: 0,
                    native_zero_words: [
                        0,
                        0,
                        0,
                        0,
                        u32::from(case["one_success"].as_bool().unwrap()),
                    ],
                    native_flags: [0, 1],
                };
                let mut host = Host {
                    step,
                    calls: Vec::new(),
                    request: request.clone(),
                    fail: false,
                };
                assert_eq!(
                    runtime.play(
                        777,
                        &definition,
                        step["value"].as_u64().unwrap() as u32,
                        &request,
                        &mut host
                    )?,
                    step["status"].as_u64().unwrap() as u32
                );
                assert_eq!(serde_json::json!(host.calls), step["calls"]);
                assert_eq!(
                    serde_json::json!(runtime.history(777, object)),
                    step["history"]
                );
            }
        }
        Ok(())
    }
    #[test]
    fn child_host_failure_releases_node_and_keeps_native_history() -> Result<()> {
        let definition = Definition {
            default: 7,
            continuous: false,
            groups: vec![SwitchGroup {
                id: 7,
                nodes: vec![100],
            }],
            parameters: Vec::new(),
        };
        let step = serde_json::json!({"loaded":[100],"statuses":{"100":1}});
        let request = Request {
            target: 777,
            context: Default::default(),
            transition: 0,
            curve: 0,
            kind: 0,
            native_zero_words: [0; 5],
            native_flags: [0, 1],
        };
        let mut host = Host {
            step: &step,
            calls: Vec::new(),
            request: request.clone(),
            fail: true,
        };
        let mut runtime = Runtime::default();
        assert!(runtime
            .play(777, &definition, 7, &request, &mut host)
            .is_err());
        assert_eq!(
            host.calls.last().unwrap(),
            &serde_json::json!(["release", 100])
        );
        assert_eq!(runtime.history(777, 0), History { group: 7, count: 1 });
        runtime.retire_object(0);
        assert_eq!(runtime.history(777, 0), History::default());
        Ok(())
    }
}
