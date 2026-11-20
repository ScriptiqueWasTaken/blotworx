//! Native common camera reset70864..70bbc. Every external effect is required;
//! this does not replace the default-controller handoff with a timeout or cut.
use super::camera_attributes::Setter;
use anyhow::Result;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Camera {
    Attribute(Setter, [f32; 5], u8),
    ClearPrimaryOverride,
    ClearControl(u32),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Player {
    NamedActions,
    SketchActions,
    RotationOverride,
    Ledge(bool),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Read89,
    WriteA9(bool),
    Write89(bool),
    ReadAB,
    ReadA9,
    Present(bool),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lens {
    Current,
    Default,
}
pub trait Backend {
    fn camera(&mut self, command: Camera) -> Result<()>;
    fn player(&mut self) -> Result<Option<usize>>;
    fn player_effect(&mut self, player: Option<usize>, effect: Player) -> Result<()>;
    fn input(&mut self, command: Input) -> Result<bool>;
    fn guardian_visible(&mut self, visible: bool) -> Result<()>;
    fn setting(&mut self, key: &str) -> Result<bool>;
    fn target(&mut self) -> Result<Option<usize>>;
    fn ground_plane(&mut self, target: usize, enabled: bool) -> Result<()>;
    fn handoff(&mut self, duration: f32) -> Result<bool>;
    fn manager_fov(&mut self, which: Lens) -> Result<f32>;
    fn set_manager_fov(&mut self, value: f32) -> Result<()>;
    fn controller_fov(&mut self, value: f32, coefficient: f32) -> Result<()>;
    fn enable_update(&mut self, enabled: bool) -> Result<()>;
}
fn attribute(b: &mut impl Backend, setter: Setter, values: [f32; 5], selector: u8) -> Result<()> {
    b.camera(Camera::Attribute(setter, values, selector))
}
pub fn reset(duration: f32, b: &mut impl Backend) -> Result<()> {
    prefix(duration, |command| b.camera(command))?;
    let player = b.player()?;
    if player.is_some() {
        b.player_effect(player, Player::NamedActions)?;
        if b.input(Input::Read89)? {
            b.input(Input::WriteA9(true))?;
            b.input(Input::Write89(false))?;
        }
        if !b.input(Input::ReadAB)? {
            let visible = b.input(Input::ReadA9)?;
            b.input(Input::Present(visible))?;
        }
        b.player_effect(player, Player::SketchActions)?;
        b.player_effect(player, Player::RotationOverride)?;
    }
    b.guardian_visible(true)?;
    let active = b.setting("FreeCamera_ScreenLookAhead_Active")?;
    attribute(b, Setter::Lookahead, [f32::MAX; 5], active as u8)?;
    attribute(b, Setter::Ledge, [f32::MAX; 5], 0)?;
    let active = b.setting("FreeCamera_LedgeCheck_Active")?;
    // Native makes this call even if the earlier player lookup returned null.
    // Backends must diagnose that invalid receiver, not invent a primary actor.
    b.player_effect(player, Player::Ledge(active))?;
    b.camera(Camera::ClearControl(1))?;
    let active = b.setting("FreeCamera_AutoCenter_Active")?;
    attribute(b, Setter::AutoCenter, [f32::MAX; 5], active as u8)?;
    let active = b.setting("FreeCamera_AutoLevel_Active")?;
    attribute(b, Setter::AutoLevel, [f32::MAX; 5], active as u8)?;
    let active = b.setting("FreeCamera_JumpPitch_Active")?;
    attribute(b, Setter::JumpPitch, [f32::MAX; 5], active as u8)?;
    if let Some(target) = b.target()? {
        b.ground_plane(target, true)?;
    }
    if b.handoff(duration)? {
        let fov = b.manager_fov(Lens::Current)?;
        b.controller_fov(fov, 0.)?;
    }
    let fov = b.manager_fov(Lens::Default)?;
    b.set_manager_fov(fov)?;
    // Native reads Default twice; setting Current is an intervening effect.
    let fov = b.manager_fov(Lens::Default)?;
    b.controller_fov(fov, f32::from_bits(0x3c23d70a))?;
    b.enable_update(true)
}
pub fn prefix(duration: f32, mut apply: impl FnMut(Camera) -> Result<()>) -> Result<()> {
    anyhow::ensure!(
        duration.is_finite(),
        "Nonfinite native camera clear duration"
    );
    apply(Camera::Attribute(Setter::Overrides, [f32::MAX; 5], 0))?;
    if duration == 0. {
        apply(Camera::Attribute(Setter::Springs, [f32::MAX; 5], 0))?;
    }
    apply(Camera::Attribute(Setter::Offsets, [0.; 5], 0))?;
    apply(Camera::ClearPrimaryOverride)?;
    apply(Camera::Attribute(Setter::TargetScalar, [f32::MAX; 5], 1))
}
impl super::assets::World {
    ///929b4: lookup in primary/secondary arrays. Jump is genuinely absent in
    ///the reconstructed registered secondary list, not a synonym for Spin.
    pub(super) fn restore_named_camera_actions(&mut self) -> Result<()> {
        self.set_named_camera_actions_disabled(false)
    }
    pub(super) fn set_named_camera_actions_disabled(&mut self, disabled:bool) -> Result<()> {
        for (group, name) in [
            (super::ability_registry::PRIMARY.as_slice(), "Blast"),
            (super::ability_registry::SECONDARY.as_slice(), "Brush"),
            (super::ability_registry::SECONDARY.as_slice(), "Jump"),
        ] {
            if !group.contains(&name) {
                continue;
            }
            let state = self
                .player_abilities
                .get_mut(name)
                .ok_or_else(|| anyhow::anyhow!("Registered camera action {name} missing"))?;
            let edge = state.set_disabled(disabled);
            if name == "Blast" {
                if let Some(visible) = edge {
                    self.ability_hud_requests.push((
                        if visible { "Show" } else { "Hide" },
                        "HudElement",
                        "AmmoBars",
                    ));
                }
            }
        }
        Ok(())
    }
    ///92ad4 visits every tertiary/sketch slot in order, including duplicates.
    pub(super) fn restore_camera_sketch_actions(&mut self) {
        for action in &mut self.player_ability_registry.sketches {
            action.ownership.set_disabled(false);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        events: Vec<Value>,
        counts: std::collections::BTreeMap<String, usize>,
        input: [bool; 4],
    }
    impl Host<'_> {
        fn event(&mut self, value: Value) -> Result<()> {
            let name = value["op"].as_str().unwrap().to_owned();
            self.events.push(value);
            let count = self.counts.entry(name.clone()).or_default();
            *count += 1;
            let fail = self.case["fail"].as_str();
            anyhow::ensure!(
                fail != Some(&name) && fail != Some(&format!("{name}:{count}")),
                "Supplied service failure"
            );
            Ok(())
        }
    }
    impl Backend for Host<'_> {
        fn camera(&mut self, command: Camera) -> Result<()> {
            self.event(match command {
                Camera::ClearPrimaryOverride=>json!({"op":"camera:ClearPrimaryOverride"}),
                Camera::ClearControl(mask)=>json!({"op":"camera:ClearControl","mask":mask}),
                Camera::Attribute(setter,values,selector)=>{
                    let n=match setter {Setter::Overrides|Setter::Springs=>5,Setter::Offsets=>3,Setter::Lookahead|Setter::Ledge=>2,
                        Setter::AutoCenter|Setter::TargetScalar=>1,Setter::AutoLevel|Setter::JumpPitch=>0};
                    json!({"op":format!("camera:{setter:?}"),"values_bits":values[..n].iter().map(|v|v.to_bits()).collect::<Vec<_>>(),"selector":selector})
                }
            })
        }
        fn player(&mut self) -> Result<Option<usize>> {
            self.event(json!({"op":"player"}))?;
            Ok(self.case["player"].as_bool().unwrap().then_some(7))
        }
        fn player_effect(&mut self, player: Option<usize>, effect: Player) -> Result<()> {
            self.event(match effect {
                Player::Ledge(value) => json!({"op":"player:Ledge","value":value}),
                _ => json!({"op":format!("player:{effect:?}")}),
            })?;
            anyhow::ensure!(
                player == Some(7),
                "Native unconditional player receiver absent"
            );
            Ok(())
        }
        fn input(&mut self, command: Input) -> Result<bool> {
            let (name, value) = match command {
                Input::WriteA9(v) => ("WriteA9", Some((1, v))),
                Input::Write89(v) => ("Write89", Some((0, v))),
                Input::Present(v) => ("Present", Some((3, v))),
                Input::Read89 => ("Read89", None),
                Input::ReadAB => ("ReadAB", None),
                Input::ReadA9 => ("ReadA9", None),
            };
            self.event(if let Some((_, v)) = value {
                json!({"op":format!("input:{name}"),"value":v})
            } else {
                json!({"op":format!("input:{name}")})
            })?;
            if let Some((i, v)) = value {
                self.input[i] = v;
                Ok(v)
            } else {
                Ok(self.input[match command {
                    Input::Read89 => 0,
                    Input::ReadA9 => 1,
                    Input::ReadAB => 2,
                    _ => unreachable!(),
                }])
            }
        }
        fn guardian_visible(&mut self, visible: bool) -> Result<()> {
            self.event(json!({"op":"guardian","value":visible}))
        }
        fn setting(&mut self, key: &str) -> Result<bool> {
            self.event(json!({"op":"setting","key":key}))?;
            let keys = [
                "FreeCamera_ScreenLookAhead_Active",
                "FreeCamera_LedgeCheck_Active",
                "FreeCamera_AutoCenter_Active",
                "FreeCamera_AutoLevel_Active",
                "FreeCamera_JumpPitch_Active",
            ];
            Ok(self.case["mask"].as_u64().unwrap()
                & (1 << keys.iter().position(|k| *k == key).unwrap())
                != 0)
        }
        fn target(&mut self) -> Result<Option<usize>> {
            self.event(json!({"op":"target"}))?;
            Ok((self.case["target"] == "tracker").then_some(9))
        }
        fn ground_plane(&mut self, target: usize, enabled: bool) -> Result<()> {
            assert_eq!(target, 9);
            self.event(json!({"op":"ground_plane","value":enabled}))
        }
        fn handoff(&mut self, duration: f32) -> Result<bool> {
            self.event(json!({"op":"handoff","duration_bits":duration.to_bits()}))?;
            Ok(self.case["changed"].as_bool().unwrap())
        }
        fn manager_fov(&mut self, which: Lens) -> Result<f32> {
            self.event(json!({"op":format!("manager:{which:?}")}))?;
            Ok(if which == Lens::Current { 53.25 } else { 90. })
        }
        fn set_manager_fov(&mut self, value: f32) -> Result<()> {
            self.event(json!({"op":"set_manager_fov","value_bits":value.to_bits()}))
        }
        fn controller_fov(&mut self, value: f32, coefficient: f32) -> Result<()> {
            self.event(json!({"op":"controller_fov","value_bits":value.to_bits(),"coefficient_bits":coefficient.to_bits()}))
        }
        fn enable_update(&mut self, enabled: bool) -> Result<()> {
            self.event(json!({"op":"enable_update","value":enabled}))
        }
    }
    #[test]
    fn reset_service_order_arguments_and_failure_prefixes_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_clear.json"
        ))?;
        for (i, c) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut host = Host {
                case: c,
                events: vec![],
                counts: Default::default(),
                input: [
                    c["flags"][0].as_bool().unwrap(),
                    c["flags"][1].as_bool().unwrap(),
                    c["flags"][2].as_bool().unwrap(),
                    false,
                ],
            };
            let result = reset(c["duration"].as_f64().unwrap() as f32, &mut host);
            assert_eq!(
                result.is_err(),
                !c["stopped"].is_null(),
                "Case{i}: {result:?}"
            );
            assert_eq!(json!(host.events), c["events"], "Case{i}");
            assert_eq!(
                json!({"89":host.input[0],"a9":host.input[1],"ab":host.input[2],"a8":host.input[3]}),
                c["input_after"],
                "Case{i}"
            );
        }
        Ok(())
    }
    #[test]
    fn fov_snap_spring_and_equal_cache_match_original_instructions() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/camera_clear.json"
        ))?;
        for c in fixture["fov"].as_array().unwrap() {
            let values: [f32; 5] = serde_json::from_value(c["values"].clone())?;
            let [current, target, stiffness, damping, velocity] = values;
            let mut fov = super::super::camera_base::Fov {
                current,
                target,
                stiffness,
                damping,
                velocity,
            };
            fov.set(
                c["value"].as_f64().unwrap() as f32,
                c["coefficient"].as_f64().unwrap() as f32,
            )?;
            assert_eq!(
                json!([
                    fov.current,
                    fov.target,
                    fov.stiffness,
                    fov.damping,
                    fov.velocity
                ]
                .map(f32::to_bits)),
                c["output_bits"],
                "{c}"
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_world_reset_services_preserve_ownership_and_camera_publication() -> Result<()> {
        use anyhow::Context;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let player = world.player_entity;
        let publication = world.camera_pipeline.application;
        let count = world.camera_pipeline.publications;
        for name in ["Blast", "Brush", "Spin"] {
            let ability = world.player_abilities.get_mut(name).unwrap();
            ability.give("All", 3);
            ability.disabled = true;
        }
        for sketch in &mut world.player_ability_registry.sketches {
            sketch.ownership.disabled = true;
        }
        let requests = world.ability_hud_requests.len();
        world.restore_named_camera_actions()?;
        world.restore_camera_sketch_actions();
        for name in ["Blast", "Brush"] {
            let state = &world.player_abilities[name];
            assert_eq!(state.owned, [true, true]);
            assert_eq!(state.upgrades, [3, 3]);
            assert!(!state.disabled);
        }
        assert!(
            world.player_abilities["Spin"].disabled,
            "Named reset must not clear unrelated secondary actions"
        );
        assert_eq!(world.ability_hud_requests.len(), requests + 1);
        assert_eq!(
            world.ability_hud_requests.last(),
            Some(&("Show", "HudElement", "AmmoBars"))
        );
        assert!(world
            .player_ability_registry
            .sketches
            .iter()
            .all(|s| !s.ownership.disabled));
        world.set_guardian_visible(false)?;
        assert!(!world.guardians.visible);
        world.set_guardian_visible(true)?;
        assert!(world.guardians.visible);
        let tracker = world.mickey_tracker.as_mut().context("Tracker missing")?;
        tracker.parameters.ground_on = false;
        world.restore_camera_tracker_ground_plane()?;
        assert!(world.mickey_tracker.as_ref().unwrap().parameters.ground_on);
        world.mickey_tracker.as_mut().unwrap().parameters.ground_on = false;
        world.camera_profiles.assign_target(Some(player), false)?;
        world.restore_camera_tracker_ground_plane()?;
        assert!(
            !world.mickey_tracker.as_ref().unwrap().parameters.ground_on,
            "Direct non-tracker target must not change the global tracker"
        );
        world
            .camera_pipeline
            .derived
            .as_mut()
            .unwrap()
            .apply_clear_command(Camera::Attribute(Setter::Springs, [0.25; 5], 0))?;
        world.clear_camera_scalar_prefix(0.5)?;
        assert_eq!(
            world
                .camera_pipeline
                .derived
                .as_ref()
                .unwrap()
                .scalar(0x5f0)?,
            0.25,
            "Positive duration preserves the existing spring"
        );
        world.clear_camera_scalar_prefix(0.)?;
        let owner = world.camera_pipeline.derived.as_ref().unwrap();
        for off in [0x5c0, 0x5c4, 0x5c8, 0x5d8, 0x5d4, 0x214] {
            assert_eq!(owner.scalar(off)?, f32::MAX);
        }
        for off in [0x5e0, 0x5e4, 0x5e8] {
            assert_eq!(owner.scalar(off)?.to_bits(), 0);
        }
        assert_eq!(owner.scalar(0x5f0)?, f32::MAX);
        assert!(owner.scalar(0x5f4)?.is_finite());
        assert!(
            owner.scalar(0x430).is_err(),
            "An untouched sentinel cache must not be fabricated"
        );
        assert_eq!(world.camera_pipeline.application, publication);
        assert_eq!(world.camera_pipeline.publications, count);
        Ok(())
    }
}
