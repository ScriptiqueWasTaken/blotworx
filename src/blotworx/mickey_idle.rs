//! Mickey controller idle scheduling, EM1 REL8bdd0..8bf08,8bfec..8c0bc.
//! The normal/moving FSM selector is30e37c;337870 resets on entry to state0.
//! Threat collection1e0c and its target/team lifecycle are not reconstructed;
//! absence is explicit, not a fabricated zero-combat-intensity result.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub combat: [f32; 2],
    pub fidget: [f32; 2],
    pub combat_threshold: f32,
}
impl Settings {
    pub fn load(values: &BTreeMap<String, String>) -> Result<Self> {
        let read = |key: &str, default: f32| -> Result<f32> {
            let value = values
                .get(key)
                .map(|v| v.trim().parse::<f32>())
                .transpose()
                .with_context(|| format!("Invalid {key}"))?
                .unwrap_or(default);
            ensure!(value.is_finite(), "Non-finite {key}");
            Ok(value)
        };
        // Defaults are the strings passed to14a0d8 by895b0..89858.
        Ok(Self {
            combat: [
                read("Mickey_CombatIdleMinTime", 3.)?,
                read("Mickey_CombatIdleMaxTime", 5.)?,
            ],
            fidget: [
                read("Mickey_FidgetMinTime", 4.)?,
                read("Mickey_FidgetMaxTime", 15.)?,
            ],
            combat_threshold: read("Mickey_CombatIntensityForCombatIdle", 0.01)?,
        })
    }
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct State {
    pub controller_state: i32,
    pub combat_remaining: f32,
    pub fidget_remaining: f32,
    pub stay_combat: bool,
    pub resets: u64,
    pub fidgets: u64,
    pub combat_exits: u64,
    pub combat_query_missing: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            controller_state: 0,
            combat_remaining: 0.,
            fidget_remaining: 0.,
            stay_combat: false,
            resets: 0,
            fidgets: 0,
            combat_exits: 0,
            combat_query_missing: false,
        }
    }
}
fn interval(range: [f32; 2], random: &mut super::game_random::State) -> f32 {
    // DOL80017940; xoris/lfd conversion is exact for15-bit integers, followed
    // by fdivs32767 then fmadds. Retain even reversed/equal authored ranges.
    let fraction = random.draw() as f32 / 32767.;
    (range[1] - range[0]).mul_add(fraction, range[0])
}
impl State {
    /// Spawn/reset3068b4 passes idle=false, reaching8bfec directly. Skid/turn
    /// motion+213 also reaches this reset each update306680..3066a8.
    pub fn reset(&mut self, settings: &Settings, random: &mut super::game_random::State) {
        self.combat_remaining = interval(settings.combat, random);
        self.fidget_remaining = interval(settings.fidget, random);
        self.resets += 1;
    }
    pub fn advance(
        &mut self,
        motion_state: i32,
        ground_speed: f32,
        reverse_motion: bool,
        dt: f32,
        combat_intensity: Option<f32>,
        settings: &Settings,
        random: &mut super::game_random::State,
        mut send: impl FnMut(&'static str) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            dt.is_finite() && dt >= 0. && ground_speed.is_finite(),
            "Invalid Mickey idle update"
        );
        if reverse_motion {
            self.reset(settings, random);
        }
        //30e37c: state0 iff motion+170==0 and latest history+10 is not >0.
        // This is neither raw stick activity nor world-space support velocity.
        let next = i32::from(motion_state != 0 || ground_speed > 0.);
        if next != self.controller_state {
            self.controller_state = next;
            if next == 0 {
                self.reset(settings, random);
            }
        }
        self.combat_query_missing = false;
        if self.controller_state != 0 {
            return Ok(());
        }
        self.combat_remaining -= dt;
        self.fidget_remaining -= dt;
        if self.combat_remaining <= 0. {
            if let Some(intensity) = combat_intensity {
                ensure!(intensity.is_finite(), "Non-finite combat intensity");
                if intensity < settings.combat_threshold && !self.stay_combat {
                    send("EVENT_Stop_Combat_Idle")?;
                    self.combat_exits += 1;
                    self.combat_remaining = interval(settings.combat, random);
                }
            } else {
                self.combat_query_missing = true;
            }
        }
        // Independently evaluated even while combat prevents the first event.
        if self.fidget_remaining <= 0. {
            send("EVENT_Start_Idle_Fidget")?;
            self.fidgets += 1;
            self.fidget_remaining = interval(settings.fidget, random);
        }
        Ok(())
    }
}

#[cfg(debug_assertions)]
pub fn validate_original_fidgets() -> Result<()> {
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world = super::assets::load(
        std::path::Path::new(&root),
        "Levels/GV_ZoneA_Start.level",
        &egui::Context::default(),
    )?;
    let mut preview = super::authored_preview::Preview::new(&mut world)?;
    let mut player = super::physics::Player::new(world.report.spawn, world.report.motion.clone());
    player.grounded = true;
    player.ground_vectors = Some(super::physics::GroundVectors {
        movement: [0.; 3],
        desired: [0.; 3],
    });
    let original_pose = preview.pose.clone();
    let mut clips = std::collections::BTreeSet::new();
    let mut changed = false;
    let mut events = Vec::new();
    for tick in 1..=1200 {
        world.player_controller_tick(&mut preview, &mut player)?;
        if preview.idle.fidgets > 0 {
            changed |= preview.pose.iter().zip(&original_pose).any(|(a, b)| {
                a.local_translation != b.local_translation || a.local_rotation != b.local_rotation
            });
            for node in &world.player_graph.plan.nodes {
                if let Some(clip) = world.player_graph.state.clips.get(&node.object) {
                    clips.insert(clip.settings.animation_name.clone());
                }
            }
        }
        for event in &preview.delivered_events {
            let names = world.player_behavior.single("hkbBehaviorGraphStringData")?["eventNames"]
                .as_array()
                .context("Missing events")?;
            if names.get(event.id as usize).and_then(|v| v.as_str())
                == Some("EVENT_Start_Idle_Fidget")
            {
                events.push(tick);
            }
        }
    }
    ensure!(
        preview.idle.fidgets >= 2,
        "Stationary controller did not schedule original fidgets"
    );
    ensure!(
        events.len() == preview.idle.fidgets as usize,
        "Synchronous idle commands disappeared from the controller tick's delivery history"
    );
    ensure!(
        changed,
        "Fidget scheduling did not change the original skeleton pose"
    );
    ensure!(
        clips.iter().any(
            |name| ["brushToss", "gunslinger", "_hop", "lookaround", "robotArm"]
                .iter()
                .any(|suffix| name.contains(suffix))
        ),
        "Original idle graph did not select a fidget clip: {clips:?}"
    );
    let before = preview.idle.fidgets;
    let timer = preview.idle.fidget_remaining;
    player.ground_vectors = Some(super::physics::GroundVectors {
        movement: [1., 0., 0.],
        desired: [1., 0., 0.],
    });
    for _ in 0..120 {
        world.player_controller_tick(&mut preview, &mut player)?;
    }
    ensure!(
        preview.idle.fidgets == before && preview.idle.fidget_remaining == timer,
        "Movement did not freeze native idle scheduling"
    );
    std::fs::write(
        "target/mickey-idle-gameplay-validation.json",
        serde_json::to_vec_pretty(&serde_json::json!({
        "stationary_ticks":1200,"moving_ticks":120,"events":events,"fidget_count":before,"clips":clips,
        "pose_changed":changed,"movement_timer_frozen":true,"combat_threat_connected":false,
        "scope":"Original-asset controller/graph fixture; isolated motion-state inputs, not an original-runtime parity test or playthrough."}))?,
    )?;
    println!("PASS: native Mickey fidget scheduling selects authored clips and changes original skeleton; moving state freezes timers");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_idle_gate_resets_on_stationary_entry_and_freezes_while_moving() -> Result<()> {
        let settings = Settings::load(&Default::default())?;
        let mut random = super::super::game_random::State::default();
        let mut state = State::default();
        state.reset(&settings, &mut random);
        let first = (state.combat_remaining, state.fidget_remaining);
        assert_eq!(random.draws, 2);
        state.advance(0, 0.1, false, 1., Some(0.), &settings, &mut random, |_| {
            panic!("moving idle")
        })?;
        assert_eq!((state.combat_remaining, state.fidget_remaining), first);
        assert_eq!(random.draws, 2);
        state.advance(0, 0., false, 0., Some(1.), &settings, &mut random, |_| {
            panic!("early idle")
        })?;
        assert_eq!(state.controller_state, 0);
        assert_eq!(random.draws, 4);
        assert_eq!(state.resets, 2);
        let idle = state.fidget_remaining;
        state.advance(2, 0., false, 1., Some(0.), &settings, &mut random, |_| {
            panic!("air idle")
        })?;
        assert_eq!(state.fidget_remaining, idle);
        state.advance(0, 0., false, 0., Some(1.), &settings, &mut random, |_| {
            panic!("reentry idle")
        })?;
        assert_eq!(state.resets, 3);
        assert_eq!(random.draws, 6);
        Ok(())
    }
    #[test]
    fn native_independent_expiry_uses_ordered_events_and_one_draw_per_reset() -> Result<()> {
        let settings = Settings {
            combat: [1., 1.],
            fidget: [2., 2.],
            combat_threshold: 0.01,
        };
        let mut state = State::default();
        let mut random = super::super::game_random::State::default();
        let mut events = vec![];
        state.advance(0, 0., false, 0., Some(0.), &settings, &mut random, |e| {
            events.push(e);
            Ok(())
        })?;
        assert_eq!(
            events,
            ["EVENT_Stop_Combat_Idle", "EVENT_Start_Idle_Fidget"]
        );
        assert_eq!(random.draws, 2);
        events.clear();
        state.advance(0, 0., false, 3., Some(1.), &settings, &mut random, |e| {
            events.push(e);
            Ok(())
        })?;
        assert_eq!(events, ["EVENT_Start_Idle_Fidget"]);
        assert_eq!(state.combat_remaining, -2.);
        assert_eq!(state.fidget_remaining, 2.);
        assert_eq!(random.draws, 3);
        events.clear();
        state.stay_combat = true;
        state.advance(0, 0., false, 0., Some(0.), &settings, &mut random, |e| {
            events.push(e);
            Ok(())
        })?;
        assert!(events.is_empty());
        assert_eq!(random.draws, 3);
        Ok(())
    }
    #[test]
    fn unavailable_threat_is_reported_without_blocking_fidgets() -> Result<()> {
        let settings = Settings::load(&Default::default())?;
        let mut state = State::default();
        let mut random = super::super::game_random::State::default();
        let mut events = vec![];
        state.advance(0, 0., false, 0., None, &settings, &mut random, |e| {
            events.push(e);
            Ok(())
        })?;
        assert!(state.combat_query_missing);
        assert_eq!(events, ["EVENT_Start_Idle_Fidget"]);
        assert_eq!(random.draws, 1);
        Ok(())
    }
}

impl super::assets::World {
    /// REL91d58 ->90df4: explicit owner, Boolean value, no return values.
    pub(super) fn set_stay_combat_idle_lua(&mut self,lua:&mlua::Lua,mut args:mlua::MultiValue)->mlua::Result<mlua::MultiValue> {
        if args.len()!=2 {self.report.warnings.push("SetStayInCombatIdle: expected owner and Boolean; preserving native stack order".into());}
        let value=super::actor_scripts::native_boolean(lua,args.pop_back().unwrap_or(mlua::Value::Nil))?;
        let Some(owner)=self.scripts.resolve_native_entity_argument(lua,args.pop_back().unwrap_or(mlua::Value::Nil))?.filter(|i|!self.removed_entities.contains(i)) else {return Ok(mlua::MultiValue::new())};
        if !self.entities[owner].components.iter().any(|c|c.class_name=="MickeyPlayerControllerComponent") {return Ok(mlua::MultiValue::new())}
        if owner!=self.player_entity {return Err(mlua::Error::external("SetStayInCombatIdle: secondary Mickey controller runtime is not bound"))}
        self.player_stay_combat_idle=value;
        Ok(mlua::MultiValue::new())
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate_idle_command()->Result<()> {
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world=super::assets::load(std::path::Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
    world.run_callback_fixture("assert(select('#',SetStayInCombatIdle(GetPlayer(),true))==0)")?;
    ensure!(world.player_stay_combat_idle,"Script did not retain combat idle");
    world.run_callback_fixture("SetStayInCombatIdle(nil,false); SetStayInCombatIdle(GetEntity('gremlinvillage_zonea.SteamTrigger 09'),false)")?;
    ensure!(world.player_stay_combat_idle,"Missing/noncontroller target changed primary state");
    let mut preview=super::authored_preview::Preview::new(&mut world)?;
    let mut player=super::physics::Player::new(world.report.spawn,world.report.motion.clone());
    world.player_controller_tick(&mut preview,&mut player)?;
    ensure!(preview.idle.stay_combat,"Authored gameplay controller did not consume live script state");
    world.run_callback_fixture("SetStayInCombatIdle(GetPlayer(),false)")?;
    world.player_controller_tick(&mut preview,&mut player)?;
    ensure!(!preview.idle.stay_combat,"Combat idle disable did not reach gameplay controller");
    let since=world.report.unsupported.len();
    world.screen_fade.out=true;
    world.cutscene_input.restricted=true;
    world.report.unsupported.push("Authored IO fixture: OnMapLoad -> Missing (_self): Missing authored script callback Missing: error converting Lua nil to function".into());
    ensure!(!world.recover_failed_preview_script(since),"Unexecuted callback cancelled another sequence");
    ensure!(world.screen_fade.out && world.cutscene_input.restricted,"Missing callback changed fade/input state");
    world.report.unsupported.push("Game Lua: fixture: runtime error: interrupted callback".into());
    ensure!(world.recover_failed_preview_script(since),"Existing explicit exploration recovery was lost");
    println!("PASS: retail combat-idle command explicit owner, null/noncontroller admission, no results and authored playback integration");
    Ok(())
}
