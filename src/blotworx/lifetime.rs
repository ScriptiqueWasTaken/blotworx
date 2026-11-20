//! LifetimeComponent timer contract, REL358f68..359158/35922c..359550.
//! Event/callback dispatch happens before committing the computed subtraction.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::Component;

#[derive(Clone, Debug)]
pub struct State {
    pub timer: super::brush::Lifetime,
    pub pause_during_igc: bool,
    pub pause_all_during_igc: bool,
    pub shrink: bool,
}
pub struct Update {
    pub remaining: f32,
    pub expired: bool,
    pub shrink_ratio: Option<f32>,
}
impl State {
    pub fn from_component(
        component: &Component,
        mut random: impl FnMut() -> Result<i32>,
    ) -> Result<Self> {
        ensure!(
            component.class_name == "LifetimeComponent",
            "Not a LifetimeComponent"
        );
        let field = |name: &str| -> Result<Option<&serde_json::Value>> {
            let mut found = component
                .properties
                .iter()
                .filter(|property| property.name == name);
            let value = found.next().map(|property| &property.value);
            ensure!(found.next().is_none(), "Duplicate lifetime property {name}");
            Ok(value)
        };
        let number = |name| -> Result<f32> {
            let number = field(name)?
                .and_then(|value| value.as_f64())
                .with_context(|| format!("Missing lifetime {name}"))?
                as f32;
            ensure!(number.is_finite(), "Non-finite lifetime {name}");
            Ok(number)
        };
        let boolean = |name| -> Result<bool> {
            match field(name)? {
                Some(value) => value
                    .as_bool()
                    .with_context(|| format!("Invalid lifetime {name}")),
                None => Ok(false), // Original boolean property defaults are zero.
            }
        };
        let min = number("Min Lifetime")?;
        let max = number("Max Lifetime")?;
        let mut remaining = number("Lifetime")?;
        if min > 0. && max >= min {
            // Even equal endpoints consume the original shared RNG stream.
            let sample = random()?;
            ensure!(
                (0..=32767).contains(&sample),
                "Invalid native lifetime RNG sample"
            );
            remaining = (max - min).mul_add(sample as f32 / 32767., min);
        }
        Ok(Self {
            timer: super::brush::Lifetime {
                remaining,
                ..Default::default()
            },
            pause_during_igc: boolean("Pause During IGC")?,
            pause_all_during_igc: boolean("Pause All Components During IGC")?,
            shrink: boolean("Shrink Over Lifetime")?,
        })
    }
    /// Returns an uncommitted result so synchronous listeners observe the old
    /// property. Their other state mutations survive the later timer write.
    pub fn prepare(&self, dt: f32) -> Result<Option<Update>> {
        ensure!(dt.is_finite() && dt >= 0., "Invalid lifetime elapsed time");
        if self.timer.remaining <= 0. || self.timer.paused || self.timer.paused_during_igc {
            return Ok(None);
        }
        let mut timer = self.timer.clone();
        let expired = timer.update(dt);
        Ok(Some(Update {
            remaining: timer.remaining,
            expired,
            shrink_ratio: (self.shrink && !expired).then(|| timer.remaining / self.timer.remaining),
        }))
    }
    pub fn commit(&mut self, update: &Update) {
        self.timer.remaining = update.remaining;
    }
    /// Optional all-component pause write: Some(true) pauses, Some(false)
    /// resumes. Timer flags also change when no entity-wide write is requested.
    pub fn igc_event(&mut self, event: u16, active: bool) -> Option<bool> {
        if !self.pause_during_igc && !self.pause_all_during_igc {
            return None;
        }
        match event {
            0x8b => {
                self.timer.paused = true;
                self.pause_all_during_igc.then_some(true)
            }
            0x8c => {
                let resume = !self.timer.paused_during_igc;
                self.timer.paused = false;
                resume.then_some(false)
            }
            0x85 => {
                let write = if active {
                    self.pause_all_during_igc.then_some(true)
                } else {
                    (!self.timer.paused).then_some(false)
                };
                self.timer.paused_during_igc = active;
                write
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn component() -> Component {
        use epic_mickey_lib_rs::scene_file::{Property, ID};
        Component::new(
            "LifetimeComponent".into(),
            "Life".into(),
            ID::new(1),
            ID::new(0),
            ID::new(0),
            [
                ("Min Lifetime", serde_json::json!(0)),
                ("Max Lifetime", serde_json::json!(0)),
                ("Lifetime", serde_json::json!(0.5)),
            ]
            .into_iter()
            .map(|(name, value)| {
                Property::new(String::new(), name.into(), false, false, false, value)
            })
            .collect(),
        )
    }
    #[test]
    fn expiry_and_callback_commit_order() -> Result<()> {
        let mut state = State::from_component(&component(), || panic!("Inactive RNG range"))?;
        let update = state.prepare(0.25)?.unwrap();
        assert!(!update.expired);
        state.commit(&update);
        let update = state.prepare(0.5)?.unwrap();
        assert!(update.expired);
        assert_eq!(
            state.timer.remaining, 0.25,
            "Listener must see the pre-subtraction value"
        );
        state.timer.remaining = 99.;
        state.timer.paused = true;
        state.commit(&update);
        assert_eq!(state.timer.remaining, -0.25);
        assert!(state.timer.paused);
        assert!(state.prepare(0.5)?.is_none());
        Ok(())
    }
    #[test]
    fn exact_zero_does_not_expire_and_equal_rng_endpoints_consume_sample() -> Result<()> {
        let mut state = State::from_component(&component(), || panic!("Inactive RNG range"))?;
        let update = state.prepare(0.5)?.unwrap();
        assert!(!update.expired);
        state.commit(&update);
        assert!(state.prepare(1.)?.is_none());
        let mut component = component();
        component.properties[0].value = serde_json::json!(2.);
        component.properties[1].value = serde_json::json!(2.);
        let mut calls = 0;
        let state = State::from_component(&component, || {
            calls += 1;
            Ok(123)
        })?;
        assert_eq!(calls, 1);
        assert_eq!(state.timer.remaining, 2.);
        Ok(())
    }
    #[test]
    fn overlapping_igc_flags_do_not_resume_a_still_paused_timer() -> Result<()> {
        let mut state = State::from_component(&component(), || panic!("Inactive RNG range"))?;
        assert_eq!(state.igc_event(0x8b, true), None);
        assert!(!state.timer.paused);
        state.pause_during_igc = true;
        state.pause_all_during_igc = true;
        assert_eq!(state.igc_event(0x8b, true), Some(true));
        assert_eq!(state.igc_event(0x85, true), Some(true));
        assert_eq!(state.igc_event(0x8c, false), None);
        assert!(state.prepare(1.)?.is_none());
        assert_eq!(state.igc_event(0x85, false), Some(false));
        assert!(state.prepare(0.1)?.is_some());
        Ok(())
    }
}
