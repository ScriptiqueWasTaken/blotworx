//! EM1 CSG visual transition scheduling: REL 0x24b650, 0x24ba3c,
//! 0x24bf44, 0x24c050. Rendering callbacks remain an explicit integration boundary.
use anyhow::{ensure, Result};
/// REL 0x16ca1c..0x16caf8 lazily registers a GLOBAL runtime variable,
/// not a per-mesh extra-data value. Registry range is the full finite f32 range.
pub fn time_scale(settings: &std::collections::BTreeMap<String, String>) -> Result<f32> {
    use anyhow::Context;
    let value = settings
        .get("ThinEffectTimeScale")
        .map(|value| value.parse::<f32>().context("Invalid ThinEffectTimeScale"))
        .transpose()?
        .unwrap_or(0.6666666865);
    ensure!(value.is_finite(), "Nonfinite ThinEffectTimeScale");
    Ok(value)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub node: u64,
    pub piece: i32,
}
#[derive(Clone, Copy, Debug)]
struct Entry {
    target: Target,
    owner: u32,
    speed: f32,
    // Fallback-only targets have no shader scalar to report an endpoint.
    painted: bool,
}
#[derive(Default)]
pub struct Queue {
    entries: Vec<Entry>,
}
/// Implementations must advance actual eligible mesh material effects, in
/// primary-then-secondary mesh order. Native completion uses the LAST eligible
/// mesh result, not an all/any reduction; painted is shared output across calls.
pub trait Host {
    fn owner_exists(&self, owner: u32) -> bool;
    fn prepare(&mut self, target: Target, painted: bool) -> Result<()>;
    fn set_thinned(&mut self, target: Target, thinned: bool) -> Result<()>;
    fn advance(&mut self, target: Target, step: f32, painted: &mut bool) -> Result<bool>;
}
impl Queue {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn contains(&self,target:Target)->bool {self.entries.iter().any(|entry|entry.target==target)}
    /// REL24c050: remove only the exact node/key pair, in reverse order.
    pub fn cancel(&mut self,target:Target) {
        for index in (0..self.entries.len()).rev() {
            if self.entries[index].target==target {self.entries.swap_remove(index);}
        }
    }
    /// REL 0x24ba3c reverses existing matching transitions BEFORE effect setup.
    /// The requested direction does not overwrite the running shader scalar.
    pub fn request(
        &mut self,
        target: Target,
        owner: u32,
        painted: bool,
        force: bool,
        host: &mut impl Host,
    ) -> Result<()> {
        let mut found = false;
        for e in self.entries.iter_mut().rev().filter(|e| e.target == target) {
            e.speed *= -1.;
            e.painted = !e.painted;
            found = true;
        }
        if found {
            return Ok(());
        }
        host.prepare(target, painted)?;
        self.enqueue(target, owner, painted, force, host)
    }
    fn enqueue(
        &mut self,
        target: Target,
        owner: u32,
        painted: bool,
        force: bool,
        host: &mut impl Host,
    ) -> Result<()> {
        // Native removal is reverse traversal with swap removal.
        for i in (0..self.entries.len()).rev() {
            if self.entries[i].target == target {
                self.entries.swap_remove(i);
            }
        }
        if target.piece != -1 && !painted {
            host.set_thinned(target, true)?;
        }
        // This 256-entry bound is the original fixed queue, not an import cap.
        // Overflow completes the new effect synchronously; it is never dropped.
        if force || self.entries.len() >= 256 {
            let mut ignored = painted;
            host.advance(target, 10., &mut ignored)?;
            if target.piece != -1 && painted {
                host.set_thinned(target, false)?;
            }
        } else {
            self.entries.push(Entry {
                target,
                owner,
                speed: 1.,
                painted,
            });
        }
        Ok(())
    }
    pub fn update(&mut self, dt: f32, host: &mut impl Host) -> Result<()> {
        ensure!(
            dt.is_finite() && dt >= 0.,
            "Invalid jigsaw transition timestep"
        );
        for i in (0..self.entries.len()).rev() {
            let e = self.entries[i];
            if !host.owner_exists(e.owner) {
                self.entries.swap_remove(i);
                continue;
            }
            let mut painted = e.painted;
            if host.advance(e.target, dt * e.speed, &mut painted)? {
                if e.target.piece != -1 && painted {
                    host.set_thinned(e.target, false)?;
                }
                self.entries.swap_remove(i);
            }
        }
        Ok(())
    }
}
#[derive(Debug, PartialEq)]
pub struct EffectStep {
    pub value: f32,
    pub completion: Option<bool>,
}
/// REL 0x16c988..0x16cbdc, after material eligibility/property lookup.
/// Completion bool is painted; strict crossing of endpoints is intentional.
/// ThinEffectTimeScale applies to both branches (retail fallback 2/3).
pub fn advance_effect(value: f32, step: f32, time_scale: f32) -> Result<EffectStep> {
    ensure!(
        value.is_finite() && step.is_finite() && time_scale.is_finite(),
        "Nonfinite jigsaw effect value"
    );
    let next = value + step * time_scale;
    let (value, completion) = if value <= 1. {
        if next > 1. {
            (1., Some(false))
        } else if next < 0. {
            (0., Some(true))
        } else {
            (next, None)
        }
    } else if next > 3. {
        (3., Some(true))
    } else if next < 2. {
        (2., Some(false))
    } else {
        (next, None)
    };
    Ok(EffectStep { value, completion })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        events: Vec<String>,
        dead: bool,
        done: bool,
        painted: bool,
    }
    impl Host for Fake {
        fn owner_exists(&self, _: u32) -> bool {
            !self.dead
        }
        fn prepare(&mut self, t: Target, p: bool) -> Result<()> {
            self.events.push(format!("prepare {} {p}", t.piece));
            Ok(())
        }
        fn set_thinned(&mut self, t: Target, p: bool) -> Result<()> {
            self.events.push(format!("thin {} {p}", t.piece));
            Ok(())
        }
        fn advance(&mut self, t: Target, dt: f32, p: &mut bool) -> Result<bool> {
            self.events.push(format!("advance {} {dt}", t.piece));
            *p = self.painted;
            Ok(self.done)
        }
    }
    fn target(piece: i32) -> Target {
        Target { node: 7, piece }
    }
    #[test]
    fn runtime_time_scale_obeys_command_precedence_and_native_range() -> Result<()> {
        let mut values =
            crate::render_support::game_config::runtime_settings("ThinEffectTimeScale = 0.5", false);
        values.extend(crate::render_support::game_config::runtime_settings(
            "-Set ThinEffectTimeScale=2",
            true,
        ));
        assert_eq!(time_scale(&values)?, 2.);
        for value in ["0", "-1"] {
            values.insert("ThinEffectTimeScale".into(), value.into());
            assert_eq!(time_scale(&values)?, value.parse::<f32>()?);
        }
        values.insert("ThinEffectTimeScale".into(), "NaN".into());
        assert!(time_scale(&values).is_err());
        assert_eq!(time_scale(&Default::default())?, 0.6666666865);
        Ok(())
    }
    #[test]
    fn reversal_preserves_running_effect_and_owner_loss_does_not_finish_it() -> Result<()> {
        let mut q = Queue::default();
        let mut h = Fake::default();
        q.request(target(3), 1, false, false, &mut h)?;
        q.request(target(3), 1, true, false, &mut h)?;
        q.update(0.25, &mut h)?;
        assert_eq!(
            h.events,
            ["prepare 3 false", "thin 3 true", "advance 3 -0.25"]
        );
        h.dead = true;
        q.update(0.25, &mut h)?;
        assert_eq!(q.len(), 0);
        assert_eq!(h.events.len(), 3);
        Ok(())
    }
    #[test]
    fn reverse_update_order_and_completion_state() -> Result<()> {
        let mut q = Queue::default();
        let mut h = Fake::default();
        for i in 0..3 {
            q.request(target(i), 1, true, false, &mut h)?;
        }
        h.events.clear();
        h.done = true;
        h.painted = true;
        q.update(0.5, &mut h)?;
        assert_eq!(
            h.events,
            [
                "advance 2 0.5",
                "thin 2 false",
                "advance 1 0.5",
                "thin 1 false",
                "advance 0 0.5",
                "thin 0 false"
            ]
        );
        assert_eq!(q.len(), 0);
        Ok(())
    }
    #[test]
    fn full_native_queue_completes_new_effect_instead_of_dropping() -> Result<()> {
        let mut q = Queue::default();
        let mut h = Fake::default();
        for i in 0..256 {
            q.request(target(i), 1, true, false, &mut h)?;
        }
        h.events.clear();
        q.request(target(256), 1, true, false, &mut h)?;
        assert_eq!(q.len(), 256);
        assert_eq!(
            h.events,
            ["prepare 256 true", "advance 256 10", "thin 256 false"]
        );
        Ok(())
    }
    #[test]
    fn effect_endpoint_crossing_and_reversal_are_asymmetric() -> Result<()> {
        assert_eq!(
            advance_effect(0.5, 0.5, 1.)?,
            EffectStep {
                value: 1.,
                completion: None
            }
        );
        assert_eq!(advance_effect(1., 0.1, 1.)?.completion, Some(false));
        assert_eq!(advance_effect(0., -0.1, 1.)?.completion, Some(true));
        assert_eq!(
            advance_effect(2.5, 0.5, 1.)?,
            EffectStep {
                value: 3.,
                completion: None
            }
        );
        assert_eq!(advance_effect(3., 0.1, 1.)?.completion, Some(true));
        assert_eq!(advance_effect(2., -0.1, 1.)?.completion, Some(false));
        Ok(())
    }
}
