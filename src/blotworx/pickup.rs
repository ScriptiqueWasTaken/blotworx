//! Original PlayerPickupComponent overlap receiver, REL38a23c..38a3f0.
//! Physics event production and pickup command execution are separate systems.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq)]
pub struct BobGlobals {
    pub amplitude: f32,
    pub frequency: f32,
}
impl BobGlobals {
    /// Registration388e84..388ed8, original string defaults0.05 and1.0.
    pub fn load(values: &std::collections::BTreeMap<String, String>) -> Result<Self> {
        let read = |name: &str, default: f32| -> Result<f32> {
            let value = values
                .get(name)
                .map(|v| {
                    v.parse::<f32>()
                        .with_context(|| format!("Invalid {name}: {v}"))
                })
                .transpose()?
                .unwrap_or(default);
            ensure!(value.is_finite(), "Nonfinite pickup global {name}");
            Ok(value)
        };
        Ok(Self {
            amplitude: read("PlayerPickup_BobAmplitude", 0.05)?,
            frequency: read("PlayerPickup_BobFrequency", 1.)?,
        })
    }
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct Settings {
    pub auto_attracts: bool,
    pub radius: f32,
    pub speed: f32,
    pub wait: f32,
    pub delete_parent: bool,
    pub should_bob: bool,
    pub picked_up_sfx: String,
}
impl Settings {
    pub fn from_component(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "PlayerPickupComponent",
            "Expected PlayerPickupComponent"
        );
        let field = |name: &str| -> Result<&serde_json::Value> {
            let mut matches = component.properties.iter().filter(|p| p.name == name);
            let value = &matches
                .next()
                .with_context(|| format!("Pickup {name} missing; native default unverified"))?
                .value;
            ensure!(matches.next().is_none(), "Duplicate pickup property {name}");
            Ok(value)
        };
        let boolean = |name: &str| {
            field(name)?
                .as_bool()
                .with_context(|| format!("Pickup {name} must be boolean"))
        };
        let float = |name: &str| -> Result<f32> {
            let value = field(name)?
                .as_f64()
                .with_context(|| format!("Pickup {name} must be float"))?
                as f32;
            ensure!(value.is_finite(), "Nonfinite pickup {name}");
            Ok(value)
        };
        Ok(Self {
            auto_attracts: boolean("AutoAttracts")?,
            radius: float("AutoAttractRadius")?,
            speed: float("AutoAttractSpeed")?,
            wait: float("AttractWaitTime")?,
            delete_parent: boolean("DeleteParentIfAttracting")?,
            // Constructor38a0e8 invokes2048fc, which stores byte1 at+4.
            // Getter388724 returns that field; omitted ShouldBob defaults true.
            should_bob: if component.properties.iter().any(|p| p.name == "ShouldBob") {
                boolean("ShouldBob")?
            } else {
                true
            },
            picked_up_sfx: field("PickedUpSFX")?
                .as_str()
                .context("PickedUpSFX must be string")?
                .to_owned(),
        })
    }
}

/// Decoded native event 0x34: recipient is the collector, payload is the
/// pickup owner (REL38995c..3899a0). Handles remain entity identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionEvent {
    pub recipient: usize,
    pub pickup: usize,
}

pub trait CollectionCleanup {
    /// Fresh owner handle lookup each time; callback may change component owner.
    fn owner(&mut self) -> Result<Option<usize>>;
    /// Original global event0x25; dispatch may synchronously mutate the receiver.
    fn dispatch(&mut self, event: u16, owner: Option<usize>, receiver: &mut Receiver)
        -> Result<()>;
    /// REL181f28: resolve fresh handle, acquire, enqueue181fb0, release.
    /// Mandatory lifecycle backend; not equivalent to deleting an entity vector.
    fn request_cleanup(&mut self, owner: Option<usize>) -> Result<()>;
}
/// REL38a150..38a238. Event dispatch, collector reset, fresh owner, cleanup.
pub fn finish_collection(
    receiver: &mut Receiver,
    backend: &mut impl CollectionCleanup,
) -> Result<()> {
    let owner = backend.owner()?;
    backend.dispatch(0x25, owner, receiver)?;
    receiver.clear();
    let owner = backend.owner()?;
    backend.request_cleanup(owner)
}
/// REL389e08..389e7c: compare full inactive result, store requested bit0 in
/// component flag bit7; disabling clears collector, enabling zeros duration.
pub fn set_inactive(
    current: u32,
    requested: u32,
    flags: &mut u8,
    gate: &mut CollectionGate,
    receiver: &mut Receiver,
) -> bool {
    if current == requested {
        return false;
    }
    *flags = (*flags & 0x7f) | (((requested & 1) as u8) << 7);
    if requested == 0 {
        gate.duration = 0.;
    } else {
        receiver.clear();
    }
    true
}

/// Native collection preconditions REL389828..38991c. Owner transform/bobbing
/// updates elsewhere in the same component update remain separate.
#[derive(Clone, Debug, Default)]
pub struct CollectionGate {
    pub elapsed: f32,
    pub duration: f32,
    pub delayed: bool,
}
#[derive(Clone, Debug, Default)]
pub struct AttractionState {
    pub active: bool,
    pub wait: f32,
}
impl AttractionState {
    /// REL3899b0: a positive wait skips this update even when it crosses zero.
    pub fn waiting(&mut self, delta: f32) -> bool {
        if self.wait > 0. {
            self.wait -= delta;
            true
        } else {
            false
        }
    }
}
pub trait AttractionStart {
    /// Getter+270 writes directly to wait+64; preserve failed getter destination.
    fn read_wait(&mut self, wait: &mut f32) -> Result<()>;
    fn transform(&mut self) -> Result<Option<usize>>;
    fn delete_parent(&mut self) -> Result<bool>;
    /// Fresh global player and REL389458 health-command query, always evaluated.
    fn health_commands_accept(&mut self) -> Result<bool>;
    fn parent(&mut self, transform: usize) -> Result<Option<usize>>;
    fn detach(&mut self, transform: usize) -> Result<()>;
    /// Acquire parent, enqueue181fb0, release; all lifecycle work required.
    fn cleanup_parent(&mut self, parent: usize) -> Result<()>;
}
/// REL389b4c..389ca8 after should_attract succeeds; movement begins later.
pub fn begin_attraction(
    state: &mut AttractionState,
    gate: &mut CollectionGate,
    backend: &mut impl AttractionStart,
) -> Result<()> {
    state.active = true;
    backend.read_wait(&mut state.wait)?;
    gate.delayed = false;
    let transform = backend.transform()?;
    let delete = backend.delete_parent()?;
    let health = backend.health_commands_accept()?;
    if let Some(transform) = transform {
        if delete || health {
            if let Some(parent) = backend.parent(transform)? {
                backend.detach(transform)?;
                if !health {
                    backend.cleanup_parent(parent)?;
                }
            }
        }
    }
    Ok(())
}
#[derive(Default)]
pub struct UpdateState {
    pub gate: CollectionGate,
    pub attraction: AttractionState,
    pub bob: Bob,
}
pub trait UpdateWorld: AttractionStart + AttractionPose {
    fn inactive(&mut self) -> Result<u32>;
    fn owner_disabled(&mut self) -> Result<u32>;
    fn has_parent(&mut self) -> Result<bool>;
    fn should_bob(&mut self) -> Result<bool>;
    fn should_attract(&mut self) -> Result<bool>;
    fn position(&mut self) -> Result<[f32; 4]>;
    fn owner_delta(&mut self) -> Result<f32>;
    fn collection_event(&mut self) -> Result<Option<CollectionEvent>>;
    fn dispatch_collection(
        &mut self,
        event: CollectionEvent,
        state: &mut UpdateState,
    ) -> Result<()>;
    fn speed(&mut self) -> Result<f32>;
    fn fresh_player_position(&mut self) -> Result<[f32; 4]>;
    /// Direct runtime global values: frequency then amplitude.
    fn bob_parameters(&mut self) -> Result<(f32, f32)>;
}
/// Full REL3897d8 branch order. Actual queries/setters/events remain required.
pub fn update(state: &mut UpdateState, backend: &mut impl UpdateWorld) -> Result<()> {
    if backend.inactive()? != 0 {
        return Ok(());
    }
    if backend.owner_disabled()? as u8 != 0 {
        return Ok(());
    }
    let parent = backend.has_parent()?;
    let should_bob = backend.should_bob()?;
    if parent || state.attraction.active || backend.should_attract()? {
        state.bob.base_height = backend.position()?[1];
    }
    let delta = backend.owner_delta()?;
    if !state.gate.advance(0, 0, delta) {
        return Ok(());
    }
    if let Some(event) = backend.collection_event()? {
        backend.dispatch_collection(event, state)?;
    }
    if state.attraction.active {
        if state.attraction.waiting(delta) {
            return Ok(());
        }
        if backend.should_attract()? {
            let speed = backend.speed()?;
            let current = backend.position()?;
            let target = backend.fresh_player_position()?;
            publish_attraction(attraction_step(current, target, speed, delta)?, backend)?;
        } else {
            state.attraction.active = false;
        }
    } else if backend.should_attract()? {
        begin_attraction(&mut state.attraction, &mut state.gate, backend)?;
    } else if !parent && should_bob {
        let (frequency, amplitude) = backend.bob_parameters()?;
        let height = state.bob.advance_height(delta, frequency, amplitude)?;
        let mut position = backend.position()?;
        position[1] = height;
        backend.set_position(position)?;
    }
    Ok(())
}
impl CollectionGate {
    /// REL38a3f4 sets duration and the delay byte without resetting elapsed.
    pub fn delay(&mut self, duration: f32) {
        self.duration = duration;
        self.delayed = true;
    }
    pub fn advance(
        &mut self,
        component_inactive: u32,
        owner_disabled: u32,
        owner_delta: f32,
    ) -> bool {
        if component_inactive != 0 || owner_disabled as u8 != 0 {
            return false;
        }
        if self.delayed {
            self.elapsed += owner_delta;
            if self.elapsed >= self.duration {
                self.delayed = false;
            }
            // Even the update that clears the delay exits before collection.
            return false;
        }
        true
    }
}
/// Verified unparented/non-attracting bob branch REL389cb0..389d30.
/// Caller must evaluate authored ShouldBob and the native attraction branch.
#[derive(Clone, Debug, Default)]
pub struct Bob {
    pub phase: f32,
    pub base_height: f32,
}
impl Bob {
    pub fn advance_height(&mut self, delta: f32, frequency: f32, amplitude: f32) -> Result<f32> {
        self.phase = delta.mul_add(frequency, self.phase);
        let revolution = f32::from_bits(0x40c90fdb);
        if self.phase > revolution {
            self.phase -= revolution;
        }
        let sine = super::native_angle::sin_cos_medium(self.phase as f64)?.0 as f32;
        let offset = amplitude * sine;
        Ok(self.base_height + offset)
    }
}

/// REL8d7bc..8d820 evaluates every command, retaining any successful result.
/// The empty list also requests clear. Errors denote missing implementation,
/// not the native false result, and must not be converted into success.
pub fn player_commands(
    commands: &[String],
    mut execute: impl FnMut(&str) -> Result<bool>,
) -> Result<bool> {
    let mut clear = commands.is_empty();
    for command in commands {
        if execute(command)? {
            clear = true;
        }
    }
    Ok(clear)
}
/// REL389330/389458 attraction predicate. This is a non-mutating HealthComponent
/// acceptance query, not Mickey's Give parser or the collection executor.
/// Native strtok reads up to three operands; the third does not affect Health.
pub fn attraction_commands(
    commands: &[String],
    mut accepts_health: impl FnMut(i32) -> Result<bool>,
) -> Result<bool> {
    for command in commands {
        let mut words = command
            .split('\0')
            .next()
            .unwrap_or("")
            .split(' ')
            .filter(|w| !w.is_empty());
        if words.next() != Some("Give") || words.next() != Some("Health") {
            continue;
        }
        let Some(amount) = words.next() else { continue };
        let amount = super::ability_ownership::decimal(amount);
        if amount > 0 && accepts_health(amount)? {
            return Ok(true);
        }
    }
    Ok(false)
}
/// REL389668..3896d0: four-lane paired distance, including original reciprocal
/// square-root estimate/refinement rather than host sqrt.
pub fn attraction_distance(a: [f32; 4], b: [f32; 4]) -> Result<f32> {
    let d = std::array::from_fn::<_, 4, _>(|i| a[i] - b[i]);
    let squared = d[2].mul_add(d[2], d[0] * d[0]) + d[3].mul_add(d[3], d[1] * d[1]);
    if squared == 0. {
        return Ok(0.);
    }
    Ok(squared * super::native_hk_transform::inverse_root(squared)?)
}
/// REL389a38..389ac4, including W and separate rounded speed/delta products.
pub fn attraction_step(
    current: [f32; 4],
    target: [f32; 4],
    speed: f32,
    delta: f32,
) -> Result<[f32; 4]> {
    let mut direction = std::array::from_fn::<_, 4, _>(|i| target[i] - current[i]);
    let length = attraction_distance(target, current)?;
    if length > f32::from_bits(0x38d1b717) {
        let reciprocal = 1.0 / length;
        for lane in &mut direction {
            *lane *= reciprocal;
        }
    }
    Ok(std::array::from_fn(|i| {
        let movement = direction[i] * speed;
        let movement = movement * delta;
        current[i] + movement
    }))
}
pub trait AttractionPose {
    fn set_position(&mut self, position: [f32; 4]) -> Result<()>;
    fn has_phantom(&mut self) -> Result<bool>;
    fn update_phantom(&mut self, position: [f32; 4]) -> Result<()>;
    fn has_alternate(&mut self) -> Result<bool>;
    fn update_alternate(&mut self, position: [f32; 4]) -> Result<()>;
}
/// REL389ac8..389b2c; setters and physics updates are required, never no-ops.
pub fn publish_attraction(position: [f32; 4], backend: &mut impl AttractionPose) -> Result<()> {
    backend.set_position(position)?;
    if backend.has_phantom()? {
        backend.update_phantom(position)?;
    } else if backend.has_alternate()? {
        backend.update_alternate(position)?;
    }
    Ok(())
}
/// Required native world queries for REL3895a8. No guessed player-validity,
/// inventory acceptance or distance-only attraction fallback.
pub trait AttractionWorld {
    fn enabled(&mut self, pickup: usize) -> Result<bool>;
    fn player(&mut self) -> Result<Option<usize>>;
    fn valid_player(&mut self, player: Option<usize>) -> Result<bool>;
    fn radius(&mut self, pickup: usize) -> Result<f32>;
    fn position(&mut self, entity: usize) -> Result<[f32; 4]>;
    fn inventory(&mut self, player: usize) -> Result<Option<usize>>;
    fn commands_accept(&mut self, pickup: usize, player: usize) -> Result<bool>;
    /// Build native event0x34 with pickup payload and the freshly read global
    /// player handle. Commands can mutate that handle; don't reuse player().
    fn inventory_preview(&mut self, inventory: usize, pickup: usize) -> Result<bool>;
}
pub fn should_attract(pickup: usize, world: &mut impl AttractionWorld) -> Result<bool> {
    if !world.enabled(pickup)? {
        return Ok(false);
    }
    let player = world.player()?;
    if !world.valid_player(player)? {
        return Ok(false);
    }
    let player = player.context("Native player validity accepted a null player")?;
    let radius = world.radius(pickup)?;
    let pickup_position = world.position(pickup)?;
    let player_position = world.position(player)?;
    if !(attraction_distance(player_position, pickup_position)? <= radius) {
        return Ok(false);
    }
    let Some(inventory) = world.inventory(player)? else {
        return Ok(false);
    };
    let commands = world.commands_accept(pickup, player)?;
    // Original always queries InventoryComponent after the command predicate.
    let preview = world.inventory_preview(inventory, pickup)?;
    Ok(preview || commands)
}

#[derive(Clone, Debug)]
pub struct Receiver {
    pub commands: Vec<String>,
    // Native +0x70 is one entity handle, not a set of overlapping bodies.
    pending: Option<usize>,
}
impl Receiver {
    pub fn from_component(component: &Component) -> Result<Self> {
        ensure!(
            component.class_name == "PlayerPickupComponent",
            "Not a player pickup"
        );
        let commands = component
            .properties
            .iter()
            .find(|p| p.name == "Command")
            .context("Pickup Command missing; constructor property default is unverified")?
            .value
            .as_array()
            .context("Pickup Command must be an array")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .context("Pickup Command entry must be a string")
                    .map(str::to_owned)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            commands,
            pending: None,
        })
    }
    /// REL38a23c resolves the stored handle each time, without clearing stale bits.
    pub fn collector(&self, exists: impl Fn(usize) -> bool) -> Option<usize> {
        self.pending.filter(|&index| exists(index))
    }
    pub fn collection_event(
        &self,
        owner: usize,
        exists: impl Fn(usize) -> bool,
    ) -> Option<CollectionEvent> {
        self.collector(exists).map(|recipient| CollectionEvent {
            recipient,
            pickup: owner,
        })
    }
    /// REL38a2cc: do not replace a live collector; new visitors need InventoryComponent.
    pub fn enter(
        &mut self,
        other: Option<usize>,
        exists: impl Fn(usize) -> bool,
        has_inventory: impl Fn(usize) -> bool,
    ) {
        if self.collector(&exists).is_some() {
            return;
        }
        if let Some(other) = other.filter(|&index| exists(index) && has_inventory(index)) {
            self.pending = Some(other);
        }
    }
    /// REL38a368/38a2b4: an unrelated exit never clears the current collector.
    pub fn exit(&mut self, other: Option<usize>, exists: impl Fn(usize) -> bool) {
        if self.collector(exists).is_some() && self.pending == other {
            self.pending = None;
        }
    }
    /// REL38a2a8, also called when the native component becomes inactive.
    pub fn clear(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn inactive_setter_preserves_unrelated_flags_and_timer_fields() {
        use super::*;
        let mut receiver = Receiver {
            commands: vec![],
            pending: Some(7),
        };
        let mut gate = CollectionGate {
            elapsed: 2.,
            duration: 3.,
            delayed: true,
        };
        let mut flags = 0x55;
        assert!(!set_inactive(0, 0, &mut flags, &mut gate, &mut receiver));
        assert_eq!(receiver.pending, Some(7));
        assert_eq!(gate.duration, 3.);
        assert!(set_inactive(0, 1, &mut flags, &mut gate, &mut receiver));
        assert_eq!(flags, 0xd5);
        assert_eq!(receiver.pending, None);
        receiver.pending = Some(8);
        assert!(set_inactive(1, 0, &mut flags, &mut gate, &mut receiver));
        assert_eq!(flags, 0x55);
        assert_eq!(receiver.pending, Some(8));
        assert_eq!((gate.elapsed, gate.duration, gate.delayed), (2., 0., true));
        // Native stores bit0 but uses full requested value for side effects.
        assert!(set_inactive(0, 2, &mut flags, &mut gate, &mut receiver));
        assert_eq!(flags, 0x55);
        assert_eq!(receiver.pending, None);
    }
    #[test]
    fn bob_globals_keep_registered_defaults_and_negative_overrides() -> anyhow::Result<()> {
        use super::*;
        let mut values = std::collections::BTreeMap::new();
        assert_eq!(
            BobGlobals::load(&values)?,
            BobGlobals {
                amplitude: 0.05,
                frequency: 1.
            }
        );
        values.insert("PlayerPickup_BobAmplitude".into(), "-0.2".into());
        values.insert("PlayerPickup_BobFrequency".into(), "0".into());
        assert_eq!(
            BobGlobals::load(&values)?,
            BobGlobals {
                amplitude: -0.2,
                frequency: 0.
            }
        );
        values.insert("PlayerPickup_BobFrequency".into(), "NaN".into());
        assert!(BobGlobals::load(&values).is_err());
        Ok(())
    }
    #[test]
    fn update_requeries_attraction_and_refreshes_baseline_before_delay() -> anyhow::Result<()> {
        use super::*;
        struct World {
            queries: usize,
            calls: Vec<&'static str>,
        }
        impl AttractionStart for World {
            fn read_wait(&mut self, _: &mut f32) -> Result<()> {
                panic!("No start")
            }
            fn transform(&mut self) -> Result<Option<usize>> {
                panic!("No start")
            }
            fn delete_parent(&mut self) -> Result<bool> {
                panic!("No start")
            }
            fn health_commands_accept(&mut self) -> Result<bool> {
                panic!("No start")
            }
            fn parent(&mut self, _: usize) -> Result<Option<usize>> {
                panic!("No start")
            }
            fn detach(&mut self, _: usize) -> Result<()> {
                panic!("No start")
            }
            fn cleanup_parent(&mut self, _: usize) -> Result<()> {
                panic!("No start")
            }
        }
        impl AttractionPose for World {
            fn set_position(&mut self, _: [f32; 4]) -> Result<()> {
                panic!("No movement")
            }
            fn has_phantom(&mut self) -> Result<bool> {
                panic!("No movement")
            }
            fn update_phantom(&mut self, _: [f32; 4]) -> Result<()> {
                panic!("No movement")
            }
            fn has_alternate(&mut self) -> Result<bool> {
                panic!("No movement")
            }
            fn update_alternate(&mut self, _: [f32; 4]) -> Result<()> {
                panic!("No movement")
            }
        }
        impl UpdateWorld for World {
            fn inactive(&mut self) -> Result<u32> {
                self.calls.push("inactive");
                Ok(0)
            }
            fn owner_disabled(&mut self) -> Result<u32> {
                self.calls.push("disabled");
                Ok(0x100)
            }
            fn has_parent(&mut self) -> Result<bool> {
                self.calls.push("parent");
                Ok(false)
            }
            fn should_bob(&mut self) -> Result<bool> {
                self.calls.push("bob");
                Ok(false)
            }
            fn should_attract(&mut self) -> Result<bool> {
                self.calls.push("query");
                self.queries += 1;
                Ok(self.queries % 2 == 1)
            }
            fn position(&mut self) -> Result<[f32; 4]> {
                self.calls.push("position");
                Ok([1., 4., 3., 0.])
            }
            fn owner_delta(&mut self) -> Result<f32> {
                self.calls.push("delta");
                Ok(0.2)
            }
            fn collection_event(&mut self) -> Result<Option<CollectionEvent>> {
                self.calls.push("collector");
                Ok(None)
            }
            fn dispatch_collection(
                &mut self,
                _: CollectionEvent,
                _: &mut UpdateState,
            ) -> Result<()> {
                panic!("No collector")
            }
            fn speed(&mut self) -> Result<f32> {
                panic!("No movement")
            }
            fn fresh_player_position(&mut self) -> Result<[f32; 4]> {
                panic!("No movement")
            }
            fn bob_parameters(&mut self) -> Result<(f32, f32)> {
                panic!("No bob")
            }
        }
        let mut state = UpdateState::default();
        let mut world = World {
            queries: 0,
            calls: vec![],
        };
        update(&mut state, &mut world)?;
        assert_eq!(state.bob.base_height, 4.);
        assert_eq!(
            world.calls,
            vec![
                "inactive",
                "disabled",
                "parent",
                "bob",
                "query",
                "position",
                "delta",
                "collector",
                "query"
            ]
        );
        world.calls.clear();
        world.queries = 0;
        state.gate.delay(0.1);
        update(&mut state, &mut world)?;
        assert!(!state.gate.delayed);
        assert_eq!(
            world.calls,
            vec!["inactive", "disabled", "parent", "bob", "query", "position", "delta"]
        );
        Ok(())
    }
    #[test]
    fn attraction_start_preserves_health_parent_rule_and_wait_boundary() -> anyhow::Result<()> {
        use super::*;
        struct Start {
            health: bool,
            calls: Vec<&'static str>,
        }
        impl AttractionStart for Start {
            fn read_wait(&mut self, w: &mut f32) -> Result<()> {
                self.calls.push("wait");
                *w = 0.1;
                Ok(())
            }
            fn transform(&mut self) -> Result<Option<usize>> {
                self.calls.push("transform");
                Ok(Some(3))
            }
            fn delete_parent(&mut self) -> Result<bool> {
                self.calls.push("delete?");
                Ok(true)
            }
            fn health_commands_accept(&mut self) -> Result<bool> {
                self.calls.push("health?");
                Ok(self.health)
            }
            fn parent(&mut self, t: usize) -> Result<Option<usize>> {
                assert_eq!(t, 3);
                self.calls.push("parent");
                Ok(Some(4))
            }
            fn detach(&mut self, t: usize) -> Result<()> {
                assert_eq!(t, 3);
                self.calls.push("detach");
                Ok(())
            }
            fn cleanup_parent(&mut self, p: usize) -> Result<()> {
                assert_eq!(p, 4);
                self.calls.push("cleanup");
                Ok(())
            }
        }
        let mut state = AttractionState::default();
        let mut gate = CollectionGate {
            elapsed: 2.,
            duration: 3.,
            delayed: true,
        };
        let mut start = Start {
            health: true,
            calls: vec![],
        };
        begin_attraction(&mut state, &mut gate, &mut start)?;
        assert!(state.active);
        assert!(!gate.delayed);
        assert_eq!(gate.elapsed, 2.);
        assert_eq!(
            start.calls,
            vec![
                "wait",
                "transform",
                "delete?",
                "health?",
                "parent",
                "detach"
            ]
        );
        assert!(state.waiting(0.2));
        assert!(!state.waiting(0.2));
        start.health = false;
        start.calls.clear();
        begin_attraction(&mut state, &mut gate, &mut start)?;
        assert_eq!(start.calls.last(), Some(&"cleanup"));
        state.wait = f32::NAN;
        assert!(!state.waiting(0.2));
        Ok(())
    }
    #[test]
    fn attraction_motion_preserves_threshold_overshoot_and_phantom_precedence() -> anyhow::Result<()>
    {
        use super::*;
        let step = attraction_step([0.; 4], [1., 0., 0., 0.], 2., 1.)?;
        // Original reciprocal-root estimate/refinement retains this one-ULP
        // overshoot; do not substitute ideal unit normalization or tolerances.
        assert_eq!(step, [f32::from_bits(0x40000001), 0., 0., 0.]);
        let tiny = f32::from_bits(0x38d1b717) * 0.5;
        assert_eq!(
            attraction_step([0.; 4], [tiny, 0., 0., 0.], 2., 1.)?,
            [tiny * 2., 0., 0., 0.]
        );
        assert_eq!(attraction_step([0.; 4], [0.; 4], 2., 1.)?, [0.; 4]);
        struct Pose(Vec<&'static str>);
        impl AttractionPose for Pose {
            fn set_position(&mut self, _: [f32; 4]) -> Result<()> {
                self.0.push("set");
                Ok(())
            }
            fn has_phantom(&mut self) -> Result<bool> {
                self.0.push("phantom?");
                Ok(true)
            }
            fn update_phantom(&mut self, _: [f32; 4]) -> Result<()> {
                self.0.push("phantom");
                Ok(())
            }
            fn has_alternate(&mut self) -> Result<bool> {
                panic!("Phantom skips alternate query")
            }
            fn update_alternate(&mut self, _: [f32; 4]) -> Result<()> {
                panic!("Phantom skips alternate update")
            }
        }
        let mut pose = Pose(vec![]);
        publish_attraction(step, &mut pose)?;
        assert_eq!(pose.0, vec!["set", "phantom?", "phantom"]);
        Ok(())
    }
    #[test]
    fn collection_cleanup_sends_event_before_reset_and_rereads_owner() -> anyhow::Result<()> {
        use super::*;
        struct Backend {
            owner: Option<usize>,
            calls: Vec<&'static str>,
        }
        impl CollectionCleanup for Backend {
            fn owner(&mut self) -> Result<Option<usize>> {
                self.calls.push("owner");
                Ok(self.owner)
            }
            fn dispatch(
                &mut self,
                event: u16,
                owner: Option<usize>,
                receiver: &mut Receiver,
            ) -> Result<()> {
                assert_eq!(event, 0x25);
                assert_eq!(owner, Some(3));
                assert_eq!(receiver.pending, Some(7));
                receiver.pending = Some(9);
                self.owner = Some(4);
                self.calls.push("event");
                Ok(())
            }
            fn request_cleanup(&mut self, owner: Option<usize>) -> Result<()> {
                assert_eq!(owner, Some(4));
                self.calls.push("cleanup");
                Ok(())
            }
        }
        let mut receiver = Receiver {
            commands: vec![],
            pending: Some(7),
        };
        let mut backend = Backend {
            owner: Some(3),
            calls: vec![],
        };
        finish_collection(&mut receiver, &mut backend)?;
        assert_eq!(receiver.pending, None);
        assert_eq!(backend.calls, vec!["owner", "event", "owner", "cleanup"]);
        Ok(())
    }
    use super::*;
    #[test]
    fn attraction_checks_distance_then_both_acceptance_paths() -> Result<()> {
        struct Fixture {
            calls: Vec<&'static str>,
            radius: f32,
        }
        impl AttractionWorld for Fixture {
            fn enabled(&mut self, _: usize) -> Result<bool> {
                self.calls.push("enabled");
                Ok(true)
            }
            fn player(&mut self) -> Result<Option<usize>> {
                self.calls.push("player");
                Ok(Some(2))
            }
            fn valid_player(&mut self, _: Option<usize>) -> Result<bool> {
                self.calls.push("valid");
                Ok(true)
            }
            fn radius(&mut self, _: usize) -> Result<f32> {
                self.calls.push("radius");
                Ok(self.radius)
            }
            fn position(&mut self, e: usize) -> Result<[f32; 4]> {
                self.calls.push("position");
                Ok(if e == 2 { [3., 4., 0., 0.] } else { [0.; 4] })
            }
            fn inventory(&mut self, _: usize) -> Result<Option<usize>> {
                self.calls.push("inventory");
                Ok(Some(9))
            }
            fn commands_accept(&mut self, _: usize, _: usize) -> Result<bool> {
                self.calls.push("commands");
                Ok(true)
            }
            fn inventory_preview(&mut self, i: usize, pickup: usize) -> Result<bool> {
                assert_eq!(i, 9);
                assert_eq!(pickup, 1);
                self.calls.push("preview");
                Ok(false)
            }
        }
        let distance = attraction_distance([3., 4., 0., 0.], [0.; 4])?;
        let mut world = Fixture {
            calls: vec![],
            radius: distance,
        };
        assert!(should_attract(1, &mut world)?);
        assert_eq!(
            world.calls,
            vec![
                "enabled",
                "player",
                "valid",
                "radius",
                "position",
                "position",
                "inventory",
                "commands",
                "preview"
            ]
        );
        world.calls.clear();
        world.radius = f32::from_bits(distance.to_bits() - 1);
        assert!(!should_attract(1, &mut world)?);
        assert!(!world.calls.contains(&"inventory"));
        assert_eq!(attraction_distance([0.; 4], [0.; 4])?, 0.);
        assert!(attraction_distance([0., 0., 0., 2.], [0.; 4])? > 1.9);
        Ok(())
    }
    #[test]
    fn attraction_only_queries_positive_health_and_stops_after_acceptance() -> Result<()> {
        let commands = [
            "Give Brush All 1",
            "Give Health 0",
            "Give Health -3",
            "give Health 9",
            "Give Health 2tail ignored",
            "Give Health 5",
            "Give Health 9",
        ]
        .map(str::to_owned);
        let mut queries = Vec::new();
        assert!(attraction_commands(&commands, |amount| {
            queries.push(amount);
            Ok(amount == 5)
        })?);
        assert_eq!(queries, vec![2, 5]);
        assert!(!attraction_commands(&[], |_| panic!("Empty predicate"))?);
        assert!(!attraction_commands(
            &["Give\tHealth 10".into(), "Give Health\0 10".into()],
            |_| panic!("Native token guards")
        )?);
        assert!(
            attraction_commands(&["Give Health 5 extra".into()], |_| anyhow::bail!(
                "Missing HealthComponent query"
            ))
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn native_bob_wraps_once_strictly_and_keeps_negative_phase() -> Result<()> {
        let revolution = f32::from_bits(0x40c90fdb);
        let mut bob = Bob {
            phase: revolution,
            base_height: 2.,
        };
        bob.advance_height(0., 1., 0.05)?;
        assert_eq!(bob.phase.to_bits(), revolution.to_bits());
        bob.phase = 0.;
        bob.advance_height(3., revolution, 0.)?;
        assert!(bob.phase > revolution); // No modulo or repeated subtraction.
        bob.phase = -0.5;
        let height = bob.advance_height(0., 1., 0.05)?;
        assert_eq!(bob.phase, -0.5);
        assert!(height < 2.);
        bob.phase = 0.;
        assert_eq!(bob.advance_height(0., 1., 0.05)?, 2.);
        Ok(())
    }
    #[test]
    fn native_collection_delay_skips_expiry_update_and_keeps_elapsed() {
        let mut gate = CollectionGate::default();
        gate.delay(0.5);
        assert!(!gate.advance(1, 0, 10.));
        assert_eq!(gate.elapsed, 0.);
        assert!(!gate.advance(0, 1, 10.));
        assert_eq!(gate.elapsed, 0.);
        assert!(!gate.advance(0, 0x100, 0.25));
        assert_eq!(gate.elapsed, 0.25);
        assert!(!gate.advance(0, 0, 0.25));
        assert!(!gate.delayed);
        assert!(gate.advance(0, 0, 0.25));
        assert_eq!(gate.elapsed, 0.5);
        gate.delay(0.25);
        assert_eq!(gate.elapsed, 0.5);
        assert!(!gate.advance(0, 0, 0.));
        assert!(!gate.delayed);
        assert!(gate.advance(0, 0, 0.));
        gate.delay(f32::NAN);
        assert!(!gate.advance(0, 0, 1.));
        assert!(gate.delayed);
    }
    #[test]
    fn player_collection_runs_all_commands_and_empty_list_requests_clear() -> Result<()> {
        let commands = vec!["first".into(), "second".into(), "third".into()];
        let mut calls = Vec::new();
        assert!(player_commands(&commands, |command| {
            calls.push(command.to_owned());
            Ok(command == "second")
        })?);
        assert_eq!(calls, commands);
        assert!(!player_commands(&commands, |_| Ok(false))?);
        assert!(player_commands(&[], |_| panic!(
            "Empty list must not execute"
        ))?);
        assert!(player_commands(&commands, |_| anyhow::bail!("Unimplemented receiver")).is_err());
        Ok(())
    }
    #[test]
    fn update_event_routes_to_live_collector_with_pickup_payload() {
        let mut receiver = Receiver {
            commands: vec![],
            pending: None,
        };
        assert_eq!(receiver.collection_event(12, |_| true), None);
        receiver.enter(Some(3), |_| true, |_| true);
        assert_eq!(
            receiver.collection_event(12, |_| true),
            Some(CollectionEvent {
                recipient: 3,
                pickup: 12
            })
        );
        assert_eq!(receiver.collection_event(12, |_| false), None);
    }
    #[test]
    fn native_receiver_retains_first_live_inventory_owner_and_matches_exit() {
        let mut receiver = Receiver {
            commands: vec![],
            pending: None,
        };
        let exists = |index| index < 4;
        let inventory = |index| index == 1 || index == 2;
        receiver.enter(None, exists, inventory);
        receiver.enter(Some(0), exists, inventory);
        assert_eq!(receiver.collector(exists), None);
        receiver.enter(Some(1), exists, inventory);
        receiver.enter(Some(2), exists, inventory);
        receiver.exit(Some(2), exists);
        assert_eq!(receiver.collector(exists), Some(1));
        receiver.exit(Some(1), exists);
        assert_eq!(receiver.collector(exists), None);
        receiver.enter(Some(2), exists, inventory);
        receiver.clear();
        assert_eq!(receiver.collector(exists), None);
    }
    #[test]
    fn stale_handle_is_resolved_without_clearing_and_can_be_replaced() {
        let mut receiver = Receiver {
            commands: vec![],
            pending: Some(7),
        };
        assert_eq!(receiver.collector(|index| index != 7), None);
        receiver.exit(Some(7), |index| index != 7);
        assert_eq!(receiver.pending, Some(7));
        receiver.enter(Some(3), |index| index == 3, |_| true);
        assert_eq!(receiver.collector(|_| true), Some(3));
    }
}
