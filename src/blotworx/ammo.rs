//! AmmoRechargeComponent arithmetic recovered from retail REL 0x2e4890.
//! Inventory notifications, recharge ticking and ownership are not wired yet.
use anyhow::{ensure, Context, Result};
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Waits {
    pub recharge: f32,
    pub penalty_threshold: f32,
    pub penalty: f32,
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct ChannelSettings {
    pub cost: f32,
    pub jump_cost: f32,
    pub recharge_rate: f32,
    pub recharge_max: f32,
    pub waits: Waits,
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Settings {
    pub channels: [ChannelSettings; 2],
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Policy {
    pub infinite: bool,
    pub shared: bool,
}
impl Policy {
    /// REL 0x2e40b0..0x2e4114: defaults are "0" and "true".
    /// Infinite ammo is additionally restricted to a player-owned component.
    pub fn load(
        values: &std::collections::BTreeMap<String, String>,
        is_player: bool,
    ) -> Result<Self> {
        let flag = |key: &str, default: bool| -> Result<bool> {
            match values.get(key).map(|v| v.trim().to_ascii_lowercase()) {
                None => Ok(default),
                Some(v) if v == "true" || v == "1" => Ok(true),
                Some(v) if v == "false" || v == "0" => Ok(false),
                Some(v) => anyhow::bail!("Invalid ammo setting {key}: {v}"),
            }
        };
        Ok(Self {
            infinite: is_player && flag("PlayerInfiniteAmmo", false)?,
            shared: flag("PlayerAmmoOneType", true)?,
        })
    }
}
/// Initial authored inventory, before save restoration and game startup events.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Inventory {
    pub amounts: [f32; 2],
    pub maxima: [Option<f32>; 2],
}
/// Runtime resource owner. Startup restoration and notification dispatch are
/// explicit callers; construction never fabricates a full ammo bar.
pub struct State {
    pub inventory: Option<Inventory>,
    pub settings: Settings,
    pub policy: Policy,
    pub waits: [f32; 2],
    pub timers: RechargeTimers,
}
impl State {
    pub fn new(inventory: Option<Inventory>, settings: Settings, policy: Policy) -> Self {
        Self {
            inventory,
            settings,
            policy,
            waits: [0.; 2],
            timers: Default::default(),
        }
    }
    pub fn available(&self, channel: usize) -> f32 {
        available(
            self.inventory.as_ref().map(|i| i.amounts),
            channel != 0,
            self.policy.shared,
            self.policy.infinite,
        )
    }
    /// Commit the bounded property before notifying its owner. Notifications
    /// are required and synchronous, including writes whose value is unchanged.
    pub fn change(
        &mut self,
        channel: usize,
        delta: f32,
        notify: &mut impl FnMut(usize, f32, f32) -> Result<()>,
    ) -> Result<f32> {
        let channel = if self.policy.shared { 0 } else { channel };
        let Some(inventory) = &mut self.inventory else {
            return Ok(0.);
        };
        let old = inventory.amounts[channel];
        let result = adjust(
            old,
            inventory.maxima[channel],
            delta,
            self.settings.channels[channel].waits,
        );
        if let Some(wait) = result.wait {
            self.waits[channel] = wait;
        }
        let value = inventory.bounded_value(channel, result.balance);
        inventory.amounts[channel] = value;
        notify(channel, old, value)?;
        Ok(result.returned_delta)
    }
    pub fn tick(
        &mut self,
        clock: &super::game_clock::Clock,
        hardware_dt: f32,
        notify: &mut impl FnMut(usize, f32, f32) -> Result<()>,
    ) -> Result<()> {
        let settings = self.settings;
        recharge_tick(
            &settings,
            hardware_dt,
            &mut RuntimeHost {
                state: self,
                clock,
                notify,
            },
        )
    }
}
struct RuntimeHost<'a, F> {
    state: &'a mut State,
    clock: &'a super::game_clock::Clock,
    notify: &'a mut F,
}
impl<F: FnMut(usize, f32, f32) -> Result<()>> RechargeHost for RuntimeHost<'_, F> {
    fn wait(&self, c: usize) -> f32 {
        self.state.waits[c]
    }
    fn set_wait(&mut self, c: usize, value: f32) {
        self.state.waits[c] = value;
    }
    fn inventory_maximum(&mut self, c: usize) -> Result<Option<f32>> {
        Ok(self.state.inventory.as_ref().and_then(|i| i.maxima[c]))
    }
    fn available(&mut self, c: usize) -> Result<f32> {
        Ok(self.state.available(c))
    }
    fn timer_running(&self, c: usize) -> bool {
        self.state.timers.running(c)
    }
    fn timer_elapsed(&mut self, c: usize) -> Result<f32> {
        Ok(self.state.timers.elapsed(c, self.clock))
    }
    fn reset_timer(&mut self, c: usize) -> Result<()> {
        self.state.timers.reset(c, self.clock);
        Ok(())
    }
    fn add(&mut self, c: usize, amount: f32) -> Result<()> {
        self.state.change(c, amount, self.notify)?;
        Ok(())
    }
    fn clear_timer(&mut self, c: usize) -> Result<()> {
        self.state.timers.clear(c);
        Ok(())
    }
}
impl Inventory {
    pub fn load(component: &epic_mickey_lib_rs::scene_file::Component) -> Result<Self> {
        ensure!(
            component.class_name == "InventoryComponent",
            "Expected InventoryComponent"
        );
        let read = |name: &str| -> Result<Option<f32>> {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| {
                    let value = p
                        .value
                        .as_f64()
                        .with_context(|| format!("Invalid inventory {name}"))?
                        as f32;
                    ensure!(value.is_finite(), "Non-finite inventory {name}");
                    Ok(value)
                })
                .transpose()
        };
        Ok(Self {
            amounts: [
                read("Paint")?.context("Missing inventory Paint")?,
                read("Thinner")?.context("Missing inventory Thinner")?,
            ],
            maxima: [read("PaintMax")?, read("ThinnerMax")?],
        })
    }
    /// Arithmetic portion of REL 0x344a20..0x344a50, before the property setter.
    /// Caller still owns notifications; this does not silently dispatch them.
    pub fn bounded_value(&self, channel: usize, requested: f32) -> f32 {
        if self.maxima[channel].is_some_and(|maximum| requested > maximum) {
            self.maxima[channel].unwrap()
        } else if requested < 0. {
            0.
        } else {
            requested
        }
    }
    pub fn available(&self, thinner: bool, policy: Policy) -> f32 {
        available(Some(self.amounts), thinner, policy.shared, policy.infinite)
    }
}
impl Settings {
    pub fn load(component: &epic_mickey_lib_rs::scene_file::Component) -> Result<Self> {
        ensure!(
            component.class_name == "AmmoRechargeComponent",
            "Expected AmmoRechargeComponent"
        );
        let channel = |prefix: &str| -> Result<ChannelSettings> {
            let value = |suffix: &str| -> Result<f32> {
                let name = format!("{prefix}{suffix}");
                let v = component
                    .properties
                    .iter()
                    .find(|p| p.name == name)
                    .and_then(|p| p.value.as_f64())
                    .with_context(|| format!("Missing or invalid ammo property {name}"))?
                    as f32;
                ensure!(v.is_finite(), "Non-finite ammo property {name}");
                Ok(v)
            };
            Ok(ChannelSettings {
                cost: value("Cost")?,
                jump_cost: value("JumpCost")?,
                recharge_rate: value("RechargeRate")?,
                recharge_max: value("RechargeMax")?,
                waits: Waits {
                    recharge: value("RechargeWait")?,
                    penalty_threshold: value("PenaltyAmount")?,
                    penalty: value("PenaltyWait")?,
                },
            })
        };
        Ok(Self {
            channels: [channel("Paint")?, channel("Thinner")?],
        })
    }
}
pub trait RechargeHost {
    fn wait(&self, channel: usize) -> f32;
    fn set_wait(&mut self, channel: usize, value: f32);
    fn inventory_maximum(&mut self, channel: usize) -> Result<Option<f32>>;
    fn available(&mut self, channel: usize) -> Result<f32>;
    fn timer_running(&self, channel: usize) -> bool;
    fn timer_elapsed(&mut self, channel: usize) -> Result<f32>;
    fn reset_timer(&mut self, channel: usize) -> Result<()>;
    fn add(&mut self, channel: usize, amount: f32) -> Result<()>;
    fn clear_timer(&mut self, channel: usize) -> Result<()>;
}
/// Recharge uses the unscaled game timestamp (+28), unlike movement action
/// timers (+30). Both stop with the game clock; neither reads host wall time.
#[derive(Default, Debug)]
pub struct RechargeTimers {
    stamps: [super::locomotion_actions::ClockStamp; 2],
}
impl RechargeTimers {
    pub fn running(&self, channel: usize) -> bool {
        self.stamps[channel].is_running()
    }
    /// Zero-offset reset, REL 0x14bb9c. Tick zero is represented by one.
    pub fn reset(&mut self, channel: usize, clock: &super::game_clock::Clock) {
        self.stamps[channel].reset(clock.unscaled.ticks);
    }
    pub fn elapsed(&self, channel: usize, clock: &super::game_clock::Clock) -> f32 {
        clock
            .rate
            .seconds(self.stamps[channel].elapsed_ticks(clock.unscaled.ticks))
    }
    pub fn clear(&mut self, channel: usize) {
        self.stamps[channel].clear();
    }
}
/// REL 0x2e42f8..0x2e4504. Host add must use the inventory adjustment path,
/// not clamp to recharge_max: this threshold gates recharge, not its result.
/// Initial elapsed comes from clock +48 (hardware frame delta), not the
/// scaled controller step. Scheduling determines whether this component runs.
pub fn recharge_tick(
    settings: &Settings,
    hardware_dt: f32,
    host: &mut impl RechargeHost,
) -> Result<()> {
    let mut elapsed = hardware_dt;
    for channel in 0..2 {
        let wait = host.wait(channel);
        if !(wait <= 0.) {
            host.set_wait(channel, wait - elapsed);
            continue;
        }
        let config = settings.channels[channel];
        let mut ceiling = config.recharge_max;
        if let Some(maximum) = host.inventory_maximum(channel)? {
            if ceiling > maximum {
                ceiling = maximum;
            }
        }
        if !(host.available(channel)? >= ceiling) {
            // f29 is intentionally shared across both loop iterations in the
            // original. A paint timer read also affects this tick's thinner wait.
            if host.timer_running(channel) {
                elapsed = host.timer_elapsed(channel)?;
            }
            if !host.timer_running(channel) || !(elapsed <= 0.5) {
                host.reset_timer(channel)?;
                host.add(channel, config.recharge_rate * elapsed)?;
            }
        }
        if host.available(channel)? >= ceiling {
            host.clear_timer(channel)?;
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adjustment {
    pub balance: f32,
    pub returned_delta: f32,
    pub wait: Option<f32>,
}
/// REL 0x2e4a90..0x2e4b2c, before the inventory setter's notifications.
/// Keep the original debit-to-zero return (-0), rather than replacing it
/// with the mathematical difference; callers can observe this return value.
pub fn adjust(balance: f32, maximum: Option<f32>, delta: f32, waits: Waits) -> Adjustment {
    let mut returned_delta = delta;
    let mut next = 0.;
    let wait;
    // Negated ordered comparisons preserve PPC fcmpu's unordered branches.
    if !(delta <= 0.) {
        if let Some(maximum) = maximum {
            let headroom = maximum - balance;
            if delta >= headroom {
                next = maximum;
                returned_delta = headroom;
            } else {
                next = balance + delta;
            }
        } else {
            next = balance + delta;
        }
        wait = None;
    } else {
        if !(-delta >= balance) {
            next = balance + delta;
        } else {
            returned_delta = -next;
        }
        wait = Some(if !(next >= waits.penalty_threshold) {
            waits.penalty
        } else {
            waits.recharge
        });
    }
    Adjustment {
        balance: next,
        returned_delta,
        wait,
    }
}

/// REL 0x2e47e4: byte +31 overrides availability to 100. The shared-ammo
/// property selects paint even when thinner was requested. Missing inventory
/// returns zero. This is query behavior, not an infinite-ammo debit policy.
pub fn available(inventory: Option<[f32; 2]>, thinner: bool, shared: bool, flag_31: bool) -> f32 {
    if flag_31 {
        100.
    } else {
        inventory
            .map(|v| v[usize::from(thinner && !shared)])
            .unwrap_or(0.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn waits() -> Waits {
        Waits {
            recharge: 0.75,
            penalty_threshold: 1.,
            penalty: 2.,
        }
    }
    #[test]
    fn recharge_timer_uses_unscaled_game_ticks_and_freezes_when_paused() {
        use super::super::game_clock::{Clock, Rate, Step};
        let mut clock = Clock::new(Rate::from_bus_clock(4000).unwrap(), 100);
        let mut timers = RechargeTimers::default();
        assert!(!timers.running(0));
        timers.reset(0, &clock);
        clock.sample(Step::Realtime, 700, 0.25);
        assert_eq!(timers.elapsed(0, &clock), 0.6);
        assert_eq!(clock.rate.seconds((clock.scaled_ticks - 100) as i64), 0.15);
        clock.sample(Step::Frozen, 5700, 1.);
        assert_eq!(timers.elapsed(0, &clock), 0.6);
        clock.sample(Step::Realtime, 5800, 1.);
        assert_eq!(timers.elapsed(0, &clock), 0.7);
        timers.clear(0);
        assert!(!timers.running(0));
        let zero = Clock::new(Rate::from_bus_clock(4000).unwrap(), 0);
        timers.reset(1, &zero);
        assert!(timers.running(1));
        assert_eq!(timers.elapsed(1, &zero), -0.001);
    }
    #[test]
    fn policy_defaults_and_inventory_setter_bounds_follow_retail() {
        let mut config = std::collections::BTreeMap::new();
        let policy = Policy::load(&config, true).unwrap();
        assert!(policy.shared);
        assert!(!policy.infinite);
        config.insert("PlayerInfiniteAmmo".into(), "true".into());
        assert!(Policy::load(&config, true).unwrap().infinite);
        assert!(!Policy::load(&config, false).unwrap().infinite);
        let inventory = Inventory {
            amounts: [3., 7.],
            maxima: [Some(30.), None],
        };
        assert_eq!(inventory.available(true, policy), 3.);
        assert_eq!(inventory.bounded_value(0, 35.), 30.);
        assert_eq!(inventory.bounded_value(1, 35.), 35.);
        assert_eq!(inventory.bounded_value(0, -1.), 0.);
        assert!(inventory.bounded_value(0, f32::NAN).is_nan());
        assert!(inventory.bounded_value(1, f32::NAN).is_nan());
    }
    struct Host {
        balances: [f32; 2],
        timers: [Option<f32>; 2],
        adds: Vec<(usize, f32)>,
        waits: [f32; 2],
    }
    impl RechargeHost for Host {
        fn wait(&self, c: usize) -> f32 {
            self.waits[c]
        }
        fn set_wait(&mut self, c: usize, v: f32) {
            self.waits[c] = v;
        }
        fn inventory_maximum(&mut self, _: usize) -> Result<Option<f32>> {
            Ok(Some(30.))
        }
        fn available(&mut self, c: usize) -> Result<f32> {
            Ok(self.balances[c])
        }
        fn timer_running(&self, c: usize) -> bool {
            self.timers[c].is_some()
        }
        fn timer_elapsed(&mut self, c: usize) -> Result<f32> {
            Ok(self.timers[c].unwrap())
        }
        fn reset_timer(&mut self, c: usize) -> Result<()> {
            self.timers[c] = Some(0.);
            Ok(())
        }
        fn add(&mut self, c: usize, n: f32) -> Result<()> {
            self.adds.push((c, n));
            self.balances[c] += n;
            Ok(())
        }
        fn clear_timer(&mut self, c: usize) -> Result<()> {
            self.timers[c] = None;
            Ok(())
        }
    }
    fn settings() -> Settings {
        Settings {
            channels: [ChannelSettings {
                cost: 1.,
                jump_cost: 1.,
                recharge_rate: 2.,
                recharge_max: 10.,
                waits: waits(),
            }; 2],
        }
    }
    #[test]
    fn recharge_batches_after_half_second_and_preserves_shared_elapsed() {
        let mut host = Host {
            balances: [9.5, 0.],
            timers: [Some(0.5), None],
            adds: vec![],
            waits: [0., 1.],
        };
        recharge_tick(&settings(), 1. / 30., &mut host).unwrap();
        assert!(host.adds.is_empty());
        assert_eq!(host.waits[1], 0.5);
        host.timers[0] = Some(0.6);
        recharge_tick(&settings(), 1. / 30., &mut host).unwrap();
        assert_eq!(host.adds, [(0, 1.2)]);
        assert_eq!(host.balances[0], 10.7); // Recharge ceiling is not a clamp.
        assert_eq!(host.timers[0], None);
        assert_eq!(host.waits[1], 0.5 - 0.6);
    }
    #[test]
    fn crossing_wait_zero_does_not_recharge_until_next_tick() {
        let mut host = Host {
            balances: [0., 30.],
            timers: [None, None],
            adds: vec![],
            waits: [0.01, 0.],
        };
        recharge_tick(&settings(), 1. / 30., &mut host).unwrap();
        assert!(host.adds.is_empty());
        assert!(host.waits[0] < 0.);
        recharge_tick(&settings(), 1. / 30., &mut host).unwrap();
        assert_eq!(host.adds, [(0, 2. * (1f32 / 30.))]);
        assert_eq!(host.timers[0], Some(0.));
    }
    #[test]
    fn runtime_owner_shares_debit_recharge_and_notifies_every_write() {
        use super::super::game_clock::{Clock, Rate, Step};
        let mut state = State::new(
            Some(Inventory {
                amounts: [2., 7.],
                maxima: [Some(30.); 2],
            }),
            settings(),
            Policy {
                shared: true,
                infinite: false,
            },
        );
        let mut notifications = Vec::new();
        let mut notify = |c, old, new| {
            notifications.push((c, old, new));
            Ok(())
        };
        state.change(1, -1., &mut notify).unwrap();
        assert_eq!(state.available(1), 1.);
        assert_eq!(state.waits, [0.75, 0.]);
        let mut clock = Clock::new(Rate::from_bus_clock(4000).unwrap(), 100);
        state.tick(&clock, 0.25, &mut notify).unwrap();
        assert_eq!(state.inventory.as_ref().unwrap().amounts, [1.5, 7.]);
        assert!(state.timers.running(1));
        clock.sample(Step::Realtime, 700, 0.5);
        state.tick(&clock, 0.25, &mut notify).unwrap();
        assert_eq!(state.inventory.as_ref().unwrap().amounts, [2.7, 7.]);
        assert_eq!(notifications, [(0, 2., 1.), (0, 1., 1.5), (0, 1.5, 2.7)]);
    }
    #[test]
    fn debits_choose_wait_from_result_and_preserve_zero_return() {
        assert_eq!(
            adjust(2., Some(30.), -1., waits()),
            Adjustment {
                balance: 1.,
                returned_delta: -1.,
                wait: Some(0.75)
            }
        );
        assert_eq!(adjust(1., Some(30.), -0.5, waits()).wait, Some(2.));
        for debit in [-1., -2.] {
            let result = adjust(1., Some(30.), debit, waits());
            assert_eq!(result.balance, 0.);
            assert_eq!(result.returned_delta.to_bits(), (-0f32).to_bits());
            assert_eq!(result.wait, Some(2.));
        }
        assert_eq!(adjust(1., Some(30.), 0., waits()).wait, Some(0.75));
    }
    #[test]
    fn gain_uses_optional_inventory_limit_without_resetting_wait() {
        assert_eq!(
            adjust(29., Some(30.), 5., waits()),
            Adjustment {
                balance: 30.,
                returned_delta: 1.,
                wait: None
            }
        );
        assert_eq!(
            adjust(29., None, 5., waits()),
            Adjustment {
                balance: 34.,
                returned_delta: 5.,
                wait: None
            }
        );
        assert_eq!(adjust(31., Some(30.), 1., waits()).returned_delta, -1.);
    }
    #[test]
    fn availability_preserves_shared_and_override_policies() {
        assert_eq!(available(Some([3., 7.]), true, false, false), 7.);
        assert_eq!(available(Some([3., 7.]), true, true, false), 3.);
        assert_eq!(available(None, true, false, false), 0.);
        assert_eq!(available(None, true, false, true), 100.);
    }
    #[test]
    fn unordered_comparisons_follow_original_branches() {
        let gain = adjust(1., Some(30.), f32::NAN, waits());
        assert!(gain.balance.is_nan());
        assert_eq!(gain.wait, None);
        let debit = adjust(f32::NAN, Some(30.), -1., waits());
        assert!(debit.balance.is_nan());
        assert_eq!(debit.wait, Some(2.));
    }
}
