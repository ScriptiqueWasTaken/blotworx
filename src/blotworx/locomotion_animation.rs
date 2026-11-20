//! Retail ground locomotion -> animation inputs (REL 0x307310..0x307624).
//! Call with the two latest ground history speeds, not raw stick magnitude.
use anyhow::{ensure, Context, Result};

/// Resolve engine variable/event names once, outside the simulation loop.
/// A registered variable absent from this particular graph is intentionally
/// unbound, matching the retail setter's negative-index path at 0x2f353c.
pub struct Bindings {
    variables: [Option<usize>; 4],
    events: [Option<usize>; 4],
    word_count: usize,
}
impl Bindings {
    pub fn new(graph: &super::behavior::Document) -> Result<Self> {
        let strings = graph.single("hkbBehaviorGraphStringData")?;
        let data = graph.single("hkbBehaviorGraphData")?;
        let variables = strings["variableNames"]
            .as_array()
            .context("Missing variable names")?;
        let events = strings["eventNames"]
            .as_array()
            .context("Missing event names")?;
        let infos = data["variableInfos"]
            .as_array()
            .context("Missing variable types")?;
        ensure!(
            infos.len() == variables.len(),
            "Variable type/name count differs"
        );
        let find = |list: &[serde_json::Value], name: &str| -> Result<Option<usize>> {
            let mut found = list
                .iter()
                .enumerate()
                .filter(|(_, v)| v.as_str() == Some(name));
            let index = found.next().map(|(i, _)| i);
            ensure!(found.next().is_none(), "Ambiguous animation binding {name}");
            Ok(index)
        };
        let mut binding = Self {
            variables: [None; 4],
            events: [None; 4],
            word_count: variables.len(),
        };
        for (i, name) in [
            "VAR_Normalized_Forward_Speed",
            "VAR_Forward_Speed",
            "VAR_Scaled_Run_Speed",
            "VAR_Scaled_Walk_Speed",
        ]
        .into_iter()
        .enumerate()
        {
            let index = find(variables, name)?;
            if let Some(index) = index {
                ensure!(
                    infos[index]["type"].as_i64() == Some(4),
                    "Animation input {name} is not a real variable"
                );
            }
            binding.variables[i] = index;
        }
        for (i, event) in [
            Event::StartWalk,
            Event::StopWalk,
            Event::StartRun,
            Event::StopRun,
        ]
        .into_iter()
        .enumerate()
        {
            binding.events[i] = find(events, event.name())?;
        }
        Ok(binding)
    }
    /// Writes only owned locomotion variables and returns a graph-local event.
    /// The graph runtime must dispatch that event; this does not execute nodes.
    pub fn apply(&self, output: &Output, words: &mut [u32]) -> Result<Option<usize>> {
        ensure!(
            words.len() == self.word_count,
            "Wrong behavior variable storage"
        );
        for (index, value) in self.variables.iter().zip([
            output.normalized_forward_speed,
            output.forward_speed,
            output.scaled_run_speed,
            output.scaled_walk_speed,
        ]) {
            if let Some(index) = index {
                words[*index] = value.to_bits();
            }
        }
        Ok(output.event.and_then(|event| {
            self.events[match event {
                Event::StartWalk => 0,
                Event::StopWalk => 1,
                Event::StartRun => 2,
                Event::StopRun => 3,
            }]
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Event {
    StartWalk,
    StopWalk,
    StartRun,
    StopRun,
}
impl Event {
    pub fn name(self) -> &'static str {
        match self {
            Self::StartWalk => "EVENT_Start_Walking_On_Ground",
            Self::StopWalk => "EVENT_Stop_Walking_On_Ground",
            Self::StartRun => "EVENT_Start_Running_On_Ground",
            Self::StopRun => "EVENT_Stop_Running_On_Ground",
        }
    }
}
#[derive(Debug, serde::Serialize)]
pub struct Output {
    pub event: Option<Event>,
    pub normalized_forward_speed: f32,
    pub forward_speed: f32,
    pub scaled_run_speed: f32,
    pub scaled_walk_speed: f32,
}

/// Mickey's virtual gate at 0x97f40 retains the existing animation variables
/// while motion+0x3a4 is nonzero. The reset branch at 0x3095f8 takes precedence.
pub fn update(
    previous: f32,
    speed: f32,
    maximum: f32,
    animated_run: f32,
    animated_walk: f32,
    mickey_override: u32,
    reset: bool,
) -> Result<Option<Output>> {
    if reset {
        return Ok(Some(Output {
            event: Some(Event::StopWalk),
            normalized_forward_speed: 0.,
            forward_speed: 0.,
            scaled_run_speed: 0.,
            scaled_walk_speed: 0.,
        }));
    }
    if mickey_override != 0 {
        return Ok(None);
    }
    ground(previous, speed, maximum, animated_run, animated_walk).map(Some)
}

pub fn ground(
    previous: f32,
    speed: f32,
    maximum: f32,
    animated_run: f32,
    animated_walk: f32,
) -> Result<Output> {
    ensure!(
        [previous, speed, maximum, animated_run, animated_walk]
            .iter()
            .all(|v| v.is_finite()),
        "Non-finite locomotion animation input"
    );
    // Keep the original divide/multiply sequence; simplifying to maximum
    // changes the threshold rounding when the authored run speed differs.
    let ratio = if animated_run > 0. {
        maximum / animated_run
    } else {
        0.
    };
    let run_threshold = animated_run * ratio - 0.1;
    let event = if previous < 0.13 && speed > 0.13 {
        Some(if speed < run_threshold {
            Event::StartWalk
        } else {
            Event::StartRun
        })
    } else if previous > 0.05 && speed < 0.05 {
        Some(if previous < run_threshold {
            Event::StopWalk
        } else {
            Event::StopRun
        })
    } else {
        None
    };
    Ok(Output {
        event,
        normalized_forward_speed: if maximum > 0. {
            (speed / maximum).min(0.99)
        } else {
            0.
        },
        forward_speed: speed,
        scaled_run_speed: if animated_run > 0. {
            speed / animated_run
        } else {
            0.
        },
        scaled_walk_speed: if animated_walk > 0. {
            speed / animated_walk
        } else {
            0.
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires original locally extracted Mickey behavior"]
    fn retail_bindings_preserve_other_variables_and_remap_events() -> Result<()> {
        let graph = super::super::behavior::Document::load(&std::fs::read(
            "target/managed-player-assets/MickeyBehavior.hkx_wii",
        )?)?;
        let bindings = Bindings::new(&graph)?;
        let mut words = vec![0x12345678; 54];
        let out = ground(0., 5., 5., 5., 2.)?;
        assert_eq!(bindings.apply(&out, &mut words)?, Some(6));
        assert_eq!(words[0], 2.5f32.to_bits());
        assert_eq!(words[1], 1f32.to_bits());
        assert_eq!(words[2], 0.99f32.to_bits());
        assert!(words[3..].iter().all(|&w| w == 0x12345678));
        assert!(bindings.apply(&out, &mut words[..53]).is_err());
        Ok(())
    }
    #[test]
    fn strict_start_stop_thresholds_and_no_repeated_events() {
        let eval = |old, speed| ground(old, speed, 5., 6.43, 0.63).unwrap().event;
        assert_eq!(eval(0., 0.13), None);
        assert_eq!(eval(0.13, 0.14), None);
        assert_eq!(eval(0.12, 0.14), Some(Event::StartWalk));
        assert_eq!(eval(0., 5.), Some(Event::StartRun));
        assert_eq!(eval(4., 5.), None);
        assert_eq!(eval(0.05, 0.), None);
        assert_eq!(eval(1., 0.05), None);
        assert_eq!(eval(1., 0.04), Some(Event::StopWalk));
        assert_eq!(eval(5., 0.), Some(Event::StopRun));
    }
    #[test]
    fn independent_authored_animation_speeds_and_zero_denominators() {
        let out = ground(0., 5., 5., 6.43, 0.63).unwrap();
        assert_eq!(out.normalized_forward_speed, 0.99);
        assert_eq!(out.scaled_run_speed, 5. / 6.43);
        assert_eq!(out.scaled_walk_speed, 5. / 0.63);
        let out = ground(0., 1., 0., 0., 0.).unwrap();
        assert_eq!(
            (
                out.normalized_forward_speed,
                out.scaled_run_speed,
                out.scaled_walk_speed
            ),
            (0., 0., 0.)
        );
        assert!(ground(0., f32::NAN, 5., 6.43, 0.63).is_err());
    }
    #[test]
    fn retail_variable_observations() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ground_animation_inputs.json"
        ))
        .unwrap();
        assert!(fixture["unexplained_snapshots"]
            .as_array()
            .unwrap()
            .is_empty());
        for sample in fixture["samples"].as_array().unwrap() {
            let f = |name: &str| sample[name].as_f64().unwrap() as f32;
            let out = ground(
                f("previous_speed"),
                f("speed"),
                5.,
                f("animated_run"),
                f("animated_walk"),
            )
            .unwrap();
            for (name, actual) in [
                ("normalized", out.normalized_forward_speed),
                ("scaled_run", out.scaled_run_speed),
                ("scaled_walk", out.scaled_walk_speed),
            ] {
                let expected = f(name);
                assert!(
                    (actual - expected).abs() <= 1e-6,
                    "{name}: {actual} vs {expected}"
                );
            }
        }
    }
    #[test]
    fn override_preserves_values_but_explicit_reset_takes_precedence() {
        assert!(update(4., 3., 5., 5., 2., 1, false).unwrap().is_none());
        let reset = update(4., 3., 5., 5., 2., 1, true).unwrap().unwrap();
        assert_eq!(reset.event, Some(Event::StopWalk));
        assert_eq!(reset.normalized_forward_speed, 0.);
        assert_eq!(reset.scaled_run_speed, 0.);
        assert_eq!(reset.scaled_walk_speed, 0.);
        assert_eq!(reset.forward_speed, 0.);
    }
}
