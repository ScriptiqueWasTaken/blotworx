//! EM1 AudioController start/stop and source admission. This is not a mixer.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;

#[derive(Default, Debug, serde::Serialize)]
pub struct Controller {
    pub component: u32,
    pub active: bool,
    pub playing_id: u32,
    pub dynamic: bool,
    pub sources: Vec<Option<usize>>,
    pub positions: Vec<[f32; 4]>,
    pub position_updates: u64,
    pub error: Option<String>,
}
pub trait Host {
    fn event(&mut self) -> Result<String>;
    fn post(&mut self, event: &str) -> Result<u32>;
    fn positions(&mut self) -> Result<Vec<[f32; 4]>>;
    fn publish_positions(&mut self, positions: &[[f32; 4]]) -> Result<()>;
    fn stop(&mut self, playing_id: u32) -> Result<()>;
}
impl Controller {
    /// Initialization2fe3c0 clears active/ID and caches source handles and
    /// dynamic45. Event/condition properties are queried live by the methods.
    pub fn from_component(
        component: &Component,
        mut resolve: impl FnMut(u32) -> Option<usize>,
    ) -> Result<Self> {
        ensure!(
            component.class_name == "AudioControllerComponent",
            "Not an audio controller"
        );
        let sources = property(component, "Audio Source Entities")?
            .as_array()
            .context("Invalid audio source array")?;
        ensure!(
            sources.len() <= u16::MAX as usize,
            "Audio source array exceeds native16-bit storage"
        );
        let sources = sources
            .iter()
            .map(|value| -> Result<_> {
                let id = u32::try_from(value.as_u64().context("Invalid audio source ID")?)?;
                Ok(if id == 0 { None } else { resolve(id) })
            })
            .collect::<Result<_>>()?;
        let authored_dynamic = property(component, "Audio Sources Dynamic")?
            .as_bool()
            .context("Invalid dynamic audio source flag")?;
        let dynamic = if authored_dynamic {
            true
        } else {
            dynamic_sources(
                false,
                i32::try_from(
                    property(component, "Play Condition")?
                        .as_i64()
                        .context("Invalid audio play condition")?,
                )?,
            )
        };
        Ok(Self {
            component: component.link_id.to_u32(),
            sources,
            dynamic,
            ..Default::default()
        })
    }
    ///2fea9c, then2feb90. Empty events preserve the old playing ID and
    /// skip the initial positions call, but the caller still activates44.
    pub fn start(&mut self, host: &mut impl Host) -> Result<()> {
        let event = host.event()?;
        let event = event.split('\0').next().unwrap_or("");
        if !event.is_empty() {
            self.playing_id = host.post(event)?;
            self.update_positions(host)?;
        }
        self.active = true;
        Ok(())
    }
    ///2feb7c, then2feb9c. Stopping does not clear playing ID40.
    pub fn stop(&mut self, host: &mut impl Host) -> Result<()> {
        if self.playing_id != 0 {
            host.stop(self.playing_id)?;
        }
        self.active = false;
        Ok(())
    }
    ///2fe70c..734: dynamic positions update only while active. Effect
    /// ramping later in that update is a separate, still incomplete owner.
    pub fn update(&mut self, host: &mut impl Host) -> Result<()> {
        if self.active && self.dynamic {
            self.update_positions(host)?;
        }
        Ok(())
    }
    fn update_positions(&mut self, host: &mut impl Host) -> Result<()> {
        let positions = host.positions()?;
        ensure!(
            positions.iter().flatten().all(|value| value.is_finite()),
            "Nonfinite audio source position"
        );
        host.publish_positions(&positions)?;
        self.positions = positions;
        self.position_updates += 1;
        Ok(())
    }
}
pub fn property<'a>(component: &'a Component, name: &str) -> Result<&'a serde_json::Value> {
    component
        .properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| &property.value)
        .with_context(|| format!("Audio controller lacks {name}"))
}
///2fec4c..ac: lack of a PhysicsComponent admits the source even for a
/// nonzero/unknown condition. A present physics flag uses exact cases1/2.
pub fn admits(condition: i32, painted: Option<bool>) -> bool {
    condition == 0
        || painted.map_or(true, |painted| match condition {
            1 => painted,
            2 => !painted,
            _ => false,
        })
}
///2fe5b4..5f8. Nonzero paint conditions force updates even for fixed sources.
pub fn dynamic_sources(authored: bool, condition: i32) -> bool {
    authored || condition != 0
}
///31cf74 negates X and uses mode2. Its empty-list path bypasses that
/// conversion and submits one[-10000,-10000,-10000] position (4c6b88).
pub fn backend_positions(positions: &[[f32; 4]]) -> Vec<[f32; 3]> {
    if positions.is_empty() {
        vec![[-10000.; 3]]
    } else {
        positions.iter().map(|v| [-v[0], v[1], v[2]]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_and_position_copy_match_bounded_original_instructions() -> Result<()> {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_controller.json"
        ))?;
        assert_eq!(
            reference["rel_sha256"],
            "ac25d520f329a3d0507d656f0ab1cf1c34025bb36ff866187ad082ae3e0a4ebf"
        );
        for case in reference["admission"]
            .as_array()
            .context("Missing admission cases")?
        {
            assert_eq!(
                admits(
                    case["condition"].as_i64().unwrap() as i32,
                    case["paint"].as_u64().map(|value| value != 0)
                ),
                case["admitted"].as_bool().unwrap(),
                "{case}"
            );
        }
        for case in reference["positions"]
            .as_array()
            .context("Missing position cases")?
        {
            let input = case["input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| std::array::from_fn(|i| value[i].as_f64().unwrap() as f32))
                .collect::<Vec<_>>();
            let expected = case["bits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| std::array::from_fn(|i| value[i].as_u64().unwrap() as u32))
                .collect::<Vec<[u32; 3]>>();
            assert_eq!(
                backend_positions(&input)
                    .into_iter()
                    .map(|value| value.map(f32::to_bits))
                    .collect::<Vec<_>>(),
                expected
            );
        }
        for case in reference["dynamic"]
            .as_array()
            .context("Missing dynamic cases")?
        {
            assert_eq!(
                dynamic_sources(
                    case["authored"].as_bool().unwrap(),
                    case["condition"].as_i64().unwrap() as i32
                ),
                case["dynamic"].as_bool().unwrap()
            );
        }
        let empty: [u32; 3] =
            std::array::from_fn(|i| reference["empty_position_bits"][i].as_u64().unwrap() as u32);
        assert_eq!(backend_positions(&[])[0].map(f32::to_bits), empty);
        Ok(())
    }
    struct Trace {
        event: String,
        id: u32,
        calls: Vec<String>,
        positions: Vec<[f32; 4]>,
    }
    impl Host for Trace {
        fn event(&mut self) -> Result<String> {
            self.calls.push("event".into());
            Ok(self.event.clone())
        }
        fn post(&mut self, event: &str) -> Result<u32> {
            self.calls.push(format!("post {event}"));
            Ok(self.id)
        }
        fn positions(&mut self) -> Result<Vec<[f32; 4]>> {
            self.calls.push("positions".into());
            Ok(self.positions.clone())
        }
        fn publish_positions(&mut self, positions: &[[f32; 4]]) -> Result<()> {
            self.calls.push(format!("publish {}", positions.len()));
            Ok(())
        }
        fn stop(&mut self, id: u32) -> Result<()> {
            self.calls.push(format!("stop {id}"));
            Ok(())
        }
    }
    #[test]
    fn source_conditions_keep_native_missing_physics_and_unknown_enum_behavior() {
        for (condition, expected) in [
            (0, [true, true, true]),
            (1, [true, false, true]),
            (2, [true, true, false]),
            (-1, [true, false, false]),
            (3, [true, false, false]),
        ] {
            assert_eq!(
                [None, Some(false), Some(true)].map(|value| admits(condition, value)),
                expected
            );
        }
        assert_eq!(backend_positions(&[]), vec![[-10000., -10000., -10000.]]);
        assert_eq!(
            backend_positions(&[[1., 2., 3., 0.], [1., 2., 3., 0.]]),
            vec![[-1., 2., 3.], [-1., 2., 3.]]
        );
    }
    #[test]
    fn constructor_caches_handles_in_authored_order_without_deduplicating() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{SceneFile, SceneFileVersion, ID};
        let scene = SceneFile::new(
            vec![],
            vec![],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        let mut component = crate::component_schema::CATALOG.components["AudioControllerComponent"]
            .instantiate("AudioControllerComponent", &scene)?;
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "Audio Source Entities")
            .unwrap()
            .value = serde_json::json!([42, 0, 77, 42]);
        let state = Controller::from_component(&component, |id| (id == 42).then_some(9))?;
        assert_eq!(state.sources, [Some(9), None, None, Some(9)]);
        assert!(!state.active);
        assert_eq!(state.playing_id, 0);
        assert!(state.positions.is_empty());
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "Audio Sources Dynamic")
            .unwrap()
            .value = serde_json::json!(false);
        for condition in [-1, 0, 1, 2, 3] {
            component
                .properties
                .iter_mut()
                .find(|p| p.name == "Play Condition")
                .unwrap()
                .value = serde_json::json!(condition);
            assert_eq!(
                Controller::from_component(&component, |_| None)?.dynamic,
                condition != 0
            );
        }
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "Audio Sources Dynamic")
            .unwrap()
            .value = serde_json::json!(true);
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "Play Condition")
            .unwrap()
            .value = serde_json::json!("not read during dynamic initialization");
        assert!(Controller::from_component(&component, |_| None)?.dynamic);
        component
            .properties
            .iter_mut()
            .find(|p| p.name == "Audio Source Entities")
            .unwrap()
            .value = serde_json::json!(["bad"]);
        assert!(Controller::from_component(&component, |_| None).is_err());
        Ok(())
    }
    #[test]
    fn start_stop_repeat_empty_event_and_failed_post_keep_original_call_order() -> Result<()> {
        let mut state = Controller {
            dynamic: true,
            ..Default::default()
        };
        let mut host = Trace {
            event: "Play_Ambient".into(),
            id: 17,
            calls: vec![],
            positions: vec![[1., 2., 3., 0.]],
        };
        state.start(&mut host)?;
        assert!(state.active);
        assert_eq!(state.playing_id, 17);
        assert_eq!(
            host.calls,
            ["event", "post Play_Ambient", "positions", "publish 1"]
        );
        host.calls.clear();
        state.start(&mut host)?;
        assert_eq!(
            host.calls,
            ["event", "post Play_Ambient", "positions", "publish 1"],
            "Repeated start must not be idempotent"
        );
        host.calls.clear();
        state.stop(&mut host)?;
        assert_eq!(host.calls, ["stop 17"]);
        assert_eq!(state.playing_id, 17);
        assert!(!state.active);
        host.calls.clear();
        state.update(&mut host)?;
        assert!(host.calls.is_empty());
        host.event.clear();
        state.start(&mut host)?;
        assert_eq!(host.calls, ["event"]);
        assert!(state.active);
        assert_eq!(state.playing_id, 17);
        host.calls.clear();
        state.update(&mut host)?;
        assert_eq!(host.calls, ["positions", "publish 1"]);
        state.dynamic = false;
        host.calls.clear();
        state.update(&mut host)?;
        assert!(host.calls.is_empty());
        host.event = "Play_Fail".into();
        host.id = 0;
        state.start(&mut host)?;
        assert!(state.active);
        assert_eq!(state.playing_id, 0);
        assert_eq!(state.positions, host.positions);
        host.calls.clear();
        state.stop(&mut host)?;
        assert!(host.calls.is_empty());
        Ok(())
    }
}
