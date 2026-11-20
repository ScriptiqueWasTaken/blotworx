//! Ordinary sound admission,80360150..80360630. Source/voice implementations,
//! node counters, spatial distance, pools and device output are supplied hosts.
use super::audio_play_action::Request;
use anyhow::{ensure, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Pool {
    Default,
    LowerEngine,
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Radius {
    pub found: bool,
    pub value: f32,
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Priority {
    pub current: f32,
    pub base: f32,
    pub distance_offset: f32,
}
/// Spatial reads are lazy: absent radius must not query either value, and a
/// zero distance offset must not query positions/listeners.
pub trait Spatial {
    fn object_scale(&mut self) -> Result<f32>;
    fn distance(&mut self) -> Result<f32>;
}
pub fn spatial_priority(
    base: f32,
    offset: f32,
    mut radius: Radius,
    spatial: &mut impl Spatial,
) -> Result<(Priority, Radius)> {
    let mut priority = Priority {
        current: base,
        base,
        distance_offset: offset,
    };
    if radius.found {
        radius.value *= spatial.object_scale()?;
        if offset != 0.0 {
            let distance = spatial.distance()?;
            let adjusted = if distance < radius.value {
                base + (distance / radius.value) * offset
            } else {
                base + offset
            };
            // Explicit operations preserve native NaN comparison behavior.
            priority.current = if adjusted < 0.0 {
                0.0
            } else if adjusted > 100.0 {
                100.0
            } else {
                adjusted
            };
        }
    }
    Ok((priority, radius))
}
pub trait Control {
    type Voice: Copy;
    fn priority(&mut self, node: u32, object: u32) -> Result<[f32; 2]>;
    fn radius(&mut self, node: u32) -> Result<Radius>;
    fn object_scale(&mut self, object: u32) -> Result<f32>;
    fn distance(&mut self, object: u32) -> Result<f32>;
    fn pool_threshold(&mut self, pool: Pool) -> Result<f32>;
    /// Native stats words: total at+0, used at+4. A zero total skips division.
    fn pool_stats(&mut self, pool: Pool) -> Result<(u32, u32)>;
    fn evict(&mut self, priority: f32) -> Result<bool>;
    fn increment_play_count(&mut self, node: u32, object: u32, priority: f32) -> Result<bool>;
    fn decrement_play_count(&mut self, node: u32, object: u32) -> Result<()>;
    /// Factory belongs to the request's originating node, and receives the
    /// leaf node, its source, the complete request and computed priority.
    fn create_voice(
        &mut self,
        node: u32,
        source: u32,
        request: &Request,
        priority: Priority,
    ) -> Result<Option<Self::Voice>>;
    /// Ordinary request kind0 supplies two zero initialization words.
    fn initialize_voice(&mut self, voice: Self::Voice, words: [u32; 2]) -> Result<u32>;
    fn configure_voice(
        &mut self,
        voice: Self::Voice,
        radius: f32,
        sample_offset: u32,
    ) -> Result<()>;
    fn publish_voice(&mut self, voice: Self::Voice) -> Result<()>;
    /// Exceptional Rust host failure after publication must unlink the voice
    /// before destruction. An Err from publish itself must leave it unpublished.
    fn unpublish_voice(&mut self, voice: Self::Voice) -> Result<()>;
    fn prepare_voice(&mut self, voice: Self::Voice) -> Result<()>;
    fn start_voice(
        &mut self,
        voice: Self::Voice,
        duration: i32,
        curve: u8,
        paused: bool,
    ) -> Result<()>;
    fn terminate_voice(&mut self, voice: Self::Voice, failed: bool) -> Result<()>;
    fn destroy_voice(&mut self, voice: Self::Voice) -> Result<()>;
    fn free_voice(&mut self, voice: Self::Voice) -> Result<()>;
    fn monitor(&mut self, node: u32, code: u32, request: &Request) -> Result<()>;
}
struct HostSpatial<'a, H> {
    host: &'a mut H,
    object: u32,
}
impl<H: Control> Spatial for HostSpatial<'_, H> {
    fn object_scale(&mut self) -> Result<f32> {
        self.host.object_scale(self.object)
    }
    fn distance(&mut self) -> Result<f32> {
        self.host.distance(self.object)
    }
}
fn cleanup<H: Control>(host: &mut H, voice: H::Voice, published: bool) -> Result<()> {
    // Attempt every release even when one supplied service reports an error.
    let results = [
        if published {
            host.unpublish_voice(voice)
        } else {
            Ok(())
        },
        host.terminate_voice(voice, true),
        host.destroy_voice(voice),
        host.free_voice(voice),
    ];
    let mut first = None;
    for result in results {
        if let Err(error) = result {
            if first.is_none() {
                first = Some(error);
            }
        }
    }
    if let Some(error) = first {
        Err(error)
    } else {
        Ok(())
    }
}
fn with_cleanup(error: anyhow::Error, cleaned: Result<()>) -> anyhow::Error {
    match cleaned {
        Ok(()) => error,
        Err(cleanup) => error.context(format!("Sound cleanup also failed: {cleanup:#}")),
    }
}
fn pressure(host: &mut impl Control, pool: Pool) -> Result<bool> {
    let threshold = host.pool_threshold(pool)?;
    if threshold < 1.0 {
        let (total, used) = host.pool_stats(pool)?;
        // Native converts both unsigned words exactly to double, then fdivs
        // rounds the ratio to single before comparing with the f32 threshold.
        if total != 0 {
            return Ok(((used as f64 / total as f64) as f32) > threshold);
        }
    }
    Ok(false)
}
pub fn play(node: u32, source: u32, request: &Request, host: &mut impl Control) -> Result<u32> {
    ensure!(
        request.kind == 0,
        "Continuous/continuation sound admission requires its original initialization context"
    );
    let object = request.context.object;
    let [base, offset] = host.priority(node, object)?;
    let radius = host.radius(node)?;
    let (priority, radius) =
        spatial_priority(base, offset, radius, &mut HostSpatial { host, object })?;
    let notification = request.native_flags[1] != 0;
    if (pressure(host, Pool::Default)? || pressure(host, Pool::LowerEngine)?)
        && !host.evict(priority.current)?
    {
        host.monitor(node, if notification { 0x20 } else { 0x22 }, request)?;
        return Ok(3);
    }
    if !host.increment_play_count(node, object, priority.current)? {
        let monitored = host.monitor(node, if notification { 0x1f } else { 0x21 }, request);
        // Partial ancestor counts can have changed even when admission fails.
        let decremented = host.decrement_play_count(node, object);
        monitored?;
        decremented?;
        return Ok(3);
    }
    let voice = match host.create_voice(node, source, request, priority) {
        Ok(voice) => voice,
        Err(error) => return Err(with_cleanup(error, host.decrement_play_count(node, object))),
    };
    let released = if let Some(voice) = voice {
        let mut published = false;
        let initialized: Result<bool> = (|| {
            if host.initialize_voice(voice, [0; 2])? != 1 {
                return Ok(false);
            }
            host.configure_voice(voice, radius.value, request.context.sample_offset)?;
            host.publish_voice(voice)?;
            published = true;
            host.prepare_voice(voice)?;
            host.start_voice(
                voice,
                request.transition,
                request.curve,
                request.native_zero_words[2] == 1,
            )?;
            Ok(true)
        })();
        match initialized {
            Ok(true) => return Ok(1),
            Ok(false) => cleanup(host, voice, false),
            Err(error) => return Err(with_cleanup(error, cleanup(host, voice, published))),
        }
    } else {
        host.decrement_play_count(node, object)
    };
    let monitored = host.monitor(node, if notification { 0x1d } else { 0x1e }, request);
    released?;
    monitored?;
    Ok(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        calls: Vec<Value>,
    }
    impl Host<'_> {
        fn value(&self, i: usize) -> f32 {
            self.case["spatial"][i].as_f64().unwrap() as f32
        }
        fn policy(&self, i: usize) -> bool {
            self.case["policy"][i].as_bool().unwrap()
        }
        fn pool(pool: Pool) -> usize {
            match pool {
                Pool::Default => 0,
                Pool::LowerEngine => 1,
            }
        }
        fn failure(&self, op: &str) -> Result<()> {
            ensure!(
                !self.case["failures"]
                    .as_array()
                    .is_some_and(|failures| failures.iter().any(|failure| failure == op)),
                "Supplied {op} failure"
            );
            Ok(())
        }
    }
    impl Control for Host<'_> {
        type Voice = u32;
        fn priority(&mut self, node: u32, object: u32) -> Result<[f32; 2]> {
            assert_eq!((node, object), (17, 42));
            self.calls.push(json!({"op":"priority"}));
            Ok([self.value(0), self.value(1)])
        }
        fn radius(&mut self, _: u32) -> Result<Radius> {
            self.calls.push(json!({"op":"radius"}));
            Ok(Radius {
                found: self.case["spatial"][2].as_bool().unwrap(),
                value: self.value(3),
            })
        }
        fn object_scale(&mut self, _: u32) -> Result<f32> {
            Ok(self.value(4))
        }
        fn distance(&mut self, _: u32) -> Result<f32> {
            self.calls.push(json!({"op":"distance"}));
            Ok(self.value(5))
        }
        fn pool_threshold(&mut self, pool: Pool) -> Result<f32> {
            Ok(self.case["pools"][Self::pool(pool)][0].as_f64().unwrap() as f32)
        }
        fn pool_stats(&mut self, pool: Pool) -> Result<(u32, u32)> {
            let index = Self::pool(pool);
            self.calls.push(json!({"op":"pool_stats","pool":index}));
            Ok((
                self.case["pools"][index][1].as_u64().unwrap() as u32,
                self.case["pools"][index][2].as_u64().unwrap() as u32,
            ))
        }
        fn evict(&mut self, priority: f32) -> Result<bool> {
            self.calls
                .push(json!({"op":"evict","priority":priority.to_bits()}));
            Ok(self.policy(0))
        }
        fn increment_play_count(&mut self, _: u32, _: u32, priority: f32) -> Result<bool> {
            self.calls
                .push(json!({"op":"increment","priority":priority.to_bits()}));
            Ok(self.policy(1))
        }
        fn decrement_play_count(&mut self, _: u32, _: u32) -> Result<()> {
            self.calls.push(json!({"op":"decrement"}));
            Ok(())
        }
        fn create_voice(
            &mut self,
            node: u32,
            source: u32,
            request: &Request,
            priority: Priority,
        ) -> Result<Option<u32>> {
            assert_eq!((node, source, request.target), (17, 88, 17));
            self.calls.push(json!({"op":"create","priority":[priority.current.to_bits(),priority.base.to_bits(),priority.distance_offset.to_bits()]}));
            self.failure("create")?;
            Ok(if self.policy(2) { Some(1) } else { None })
        }
        fn initialize_voice(&mut self, _: u32, words: [u32; 2]) -> Result<u32> {
            self.calls.push(json!({"op":"initialize","words":words}));
            self.failure("initialize")?;
            Ok(self.case["policy"][3].as_u64().unwrap() as u32)
        }
        fn configure_voice(&mut self, _: u32, radius: f32, sample_offset: u32) -> Result<()> {
            self.calls.push(
                json!({"op":"configure","radius":radius.to_bits(),"sample_offset":sample_offset}),
            );
            self.failure("configure")
        }
        fn publish_voice(&mut self, _: u32) -> Result<()> {
            self.calls.push(json!({"op":"publish"}));
            self.failure("publish")
        }
        fn unpublish_voice(&mut self, _: u32) -> Result<()> {
            self.calls.push(json!({"op":"unpublish"}));
            Ok(())
        }
        fn prepare_voice(&mut self, _: u32) -> Result<()> {
            self.calls.push(json!({"op":"prepare"}));
            self.failure("prepare")
        }
        fn start_voice(&mut self, _: u32, duration: i32, curve: u8, paused: bool) -> Result<()> {
            self.calls
                .push(json!({"op":"start","duration":duration,"curve":curve,"paused":paused}));
            self.failure("start")
        }
        fn terminate_voice(&mut self, _: u32, failed: bool) -> Result<()> {
            self.calls.push(json!({"op":"terminate","failed":failed}));
            self.failure("terminate")
        }
        fn destroy_voice(&mut self, _: u32) -> Result<()> {
            self.calls.push(json!({"op":"destroy"}));
            self.failure("destroy")
        }
        fn free_voice(&mut self, _: u32) -> Result<()> {
            self.calls.push(json!({"op":"free"}));
            self.failure("free")
        }
        fn monitor(&mut self, node: u32, code: u32, request: &Request) -> Result<()> {
            assert_eq!((node, request.context.object), (17, 42));
            self.calls.push(json!({"op":"monitor","code":code}));
            Ok(())
        }
    }
    #[test]
    fn original_sound_admission_priority_pressure_lifecycle_and_status() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_sound.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut host = Host {
                case,
                calls: vec![],
            };
            let request = Request {
                target: 17,
                context: super::super::audio_play_action::Context {
                    object: 42,
                    playing_id: 7,
                    words: [1, 2, 3, 4],
                    sample_offset: case["sample_offset"].as_u64().unwrap() as u32,
                },
                transition: 12,
                curve: 7,
                kind: 0,
                native_flags: [0, case["notification"].as_u64().unwrap() as u8],
                native_zero_words: [0, 0, case["paused"].as_u64().unwrap() as u32, 0, 0],
            };
            let status = play(17, 88, &request, &mut host)?;
            assert_eq!(json!(status), case["status"], "case {index}");
            assert_eq!(json!(host.calls), case["calls"], "case {index}");
        }
        for case in fixture["notifications"].as_array().unwrap() {
            assert_eq!(
                json!(super::super::audio_play_action::pending_notification()),
                case["after"]
            );
        }
        Ok(())
    }
    #[test]
    fn rust_host_errors_release_reserved_counts_and_created_voices() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_sound.json"
        ))?;
        let template = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["policy"] == json!([true, true, true, 1]) && case["pools"][0][0] == 1)
            .unwrap();
        let request = Request {
            target: 17,
            context: super::super::audio_play_action::Context {
                object: 42,
                ..Default::default()
            },
            transition: 12,
            curve: 7,
            kind: 0,
            native_flags: [0, 1],
            native_zero_words: [0; 5],
        };
        for operation in [
            "create",
            "initialize",
            "configure",
            "publish",
            "prepare",
            "start",
        ] {
            let mut case = template.clone();
            case["failures"] = json!([operation]);
            let mut host = Host {
                case: &case,
                calls: vec![],
            };
            let error = play(17, 88, &request, &mut host).unwrap_err();
            assert!(format!("{error:#}").contains(operation));
            let operations: Vec<_> = host
                .calls
                .iter()
                .map(|call| call["op"].as_str().unwrap())
                .collect();
            if operation == "create" {
                assert_eq!(operations.last(), Some(&"decrement"));
            } else {
                assert!(operations.ends_with(&["terminate", "destroy", "free"]));
                assert_eq!(
                    operations.contains(&"unpublish"),
                    matches!(operation, "prepare" | "start")
                );
            }
        }
        let mut case = template.clone();
        case["failures"] = json!(["initialize", "terminate", "destroy", "free"]);
        let mut host = Host {
            case: &case,
            calls: vec![],
        };
        let error = play(17, 88, &request, &mut host).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("initialize") && message.contains("terminate"));
        assert_eq!(host.calls.last().unwrap()["op"], "free");
        Ok(())
    }
}
