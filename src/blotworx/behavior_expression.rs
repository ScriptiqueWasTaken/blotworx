//! Authored expression modifiers. DOL 0x80208cb0 resolves assignment names;
//! 0x80208610 evaluates in array order and maintains two event-history bytes.
//! Generator scheduling and event delivery belong to the graph runtime.
use super::{behavior::Document, expression::Program};
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Authored {
    pub expression: String,
    pub assignment_variable_index: i32,
    pub assignment_event_index: i32,
    pub event_mode: u8,
}

pub struct Statement {
    pub program: Program,
    pub variable: Option<usize>,
    pub event: Option<usize>,
    mode: u8,
    scratch: Vec<f32>,
    sent: bool,
    previous_positive: bool,
    /// Original lookup failures retain the cooked variable index or resolve an
    /// event to -1. Keep them visible; do not invent a replacement target.
    pub unresolved_target: Option<String>,
}

fn index(value: i32, count: usize) -> Result<Option<usize>> {
    ensure!(
        value == -1 || (value >= 0 && (value as usize) < count),
        "Invalid expression assignment index {value}"
    );
    Ok((value >= 0).then_some(value as usize))
}

impl Statement {
    pub fn compile(source: &Authored, variables: &[String], events: &[String]) -> Result<Self> {
        ensure!(
            source.event_mode <= 3,
            "Unsupported expression event mode {}",
            source.event_mode
        );
        // The retail compiler removes tabs/newlines before searching for the
        // literal " if ", then '='. Names are case-sensitive.
        let text = source.expression.replace(['\t', '\n'], "");
        let mut variable = index(source.assignment_variable_index, variables.len())?;
        let mut event = index(source.assignment_event_index, events.len())?;
        let mut unresolved_target = None;
        let expression = if let Some((name, rhs)) = text.split_once(" if ") {
            let name = name.trim_matches(' ');
            event = events.iter().position(|v| v == name);
            if event.is_none() {
                unresolved_target = Some(name.into());
            }
            rhs
        } else if let Some((name, rhs)) = text.split_once('=') {
            let name = name.trim_matches(' ');
            if let Some(i) = variables.iter().position(|v| v == name) {
                variable = Some(i);
            } else {
                unresolved_target = Some(name.into());
            }
            rhs
        } else {
            &text
        };
        let program = Program::compile(expression, variables)
            .with_context(|| format!("Compiling modifier expression {:?}", source.expression))?;
        let scratch = program.scratch();
        Ok(Self {
            program,
            variable,
            event,
            mode: source.event_mode,
            scratch,
            sent: false,
            previous_positive: false,
            unresolved_target,
        })
    }

    pub fn activate(&mut self) {
        self.sent = false;
        self.previous_positive = false;
    }

    pub fn evaluate(&mut self, words: &mut [u32], types: &[u8]) -> Result<Option<usize>> {
        self.evaluate_with_random(words, types, None)
    }
    pub fn evaluate_with_random(
        &mut self,
        words: &mut [u32],
        types: &[u8],
        random: Option<&mut u32>,
    ) -> Result<Option<usize>> {
        let value = self
            .program
            .evaluate_with_random(words, types, &mut self.scratch, random)?;
        if let Some(i) = self.variable {
            let kind = *types.get(i).context("Modifier target out of range")?;
            words[i] = match kind {
                0 => u32::from(value != 0.),
                3 => {
                    // fctiwz truncates toward zero. Invalid/overflow conversion
                    // remains explicit until its Wii result is validated.
                    ensure!(
                        value.is_finite()
                            && (-2147483648.0..2147483648.0).contains(&(value as f64)),
                        "Unverified out-of-range Wii integer expression conversion"
                    );
                    (value as i32) as u32
                }
                4 => value.to_bits(), // no AnimationComponent NaN sanitization here
                _ => bail!("Expression assignment to scalar type {kind} is not implemented"),
            };
        }
        // The original ordered compare branches on <= 0, including its
        // unordered fallthrough; the previous-result byte uses CR_GT instead.
        let passes = !(value <= 0.);
        let emit = self.event.is_some()
            && match self.mode {
                0 => !self.sent && passes,
                1 => passes,
                2 => !self.previous_positive && passes,
                3 => self.sent || passes,
                _ => unreachable!(),
            };
        self.sent |= emit;
        self.previous_positive = value > 0.;
        Ok(if emit { self.event } else { None })
    }
}

pub struct Modifier {
    pub enabled: bool,
    pub statements: Vec<Statement>,
}
impl Modifier {
    pub fn activate(&mut self) {
        for statement in &mut self.statements {
            statement.activate();
        }
    }
    /// Append local graph event IDs in authored order. The caller owns the
    /// queue and its delivery phase; this does not recursively change states.
    pub fn evaluate(
        &mut self,
        words: &mut [u32],
        types: &[u8],
        events: &mut Vec<usize>,
    ) -> Result<()> {
        self.evaluate_with_random(words, types, events, None)
    }
    pub fn evaluate_with_random(
        &mut self,
        words: &mut [u32],
        types: &[u8],
        events: &mut Vec<usize>,
        mut random: Option<&mut u32>,
    ) -> Result<()> {
        if self.enabled {
            for statement in &mut self.statements {
                if let Some(event) =
                    statement.evaluate_with_random(words, types, random.as_deref_mut())?
                {
                    events.push(event);
                }
            }
        }
        Ok(())
    }
}

pub fn compile(graph: &Document) -> Result<BTreeMap<String, Modifier>> {
    let strings = graph.single("hkbBehaviorGraphStringData")?;
    let variables: Vec<String> = serde_json::from_value(strings["variableNames"].clone())?;
    let events: Vec<String> = serde_json::from_value(strings["eventNames"].clone())?;
    let mut result = BTreeMap::new();
    for (id, object) in &graph.objects {
        if object.klass != "hkbEvaluateExpressionModifier" {
            continue;
        }
        let fields = &object.fields;
        let enabled = fields["enable"]
            .as_bool()
            .context("Missing modifier enable flag")?;
        let statements = if fields["expressions"].is_null() {
            vec![]
        } else {
            let array_id = fields["expressions"]["ref"]
                .as_str()
                .context("Missing expression array reference")?;
            let array = graph
                .objects
                .get(array_id)
                .context("Missing expression array")?;
            ensure!(
                array.klass == "hkbExpressionDataArray",
                "Invalid expression array class"
            );
            let authored: Vec<Authored> =
                serde_json::from_value(array.fields["expressionsData"].clone())?;
            authored
                .iter()
                .map(|s| Statement::compile(s, &variables, &events))
                .collect::<Result<Vec<_>>>()?
        };
        result.insert(
            id.clone(),
            Modifier {
                enabled,
                statements,
            },
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blotworx::expression::RetailToken;

    #[test]
    fn compiler_matches_runtime_resolved_modifier_programs() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/behavior_modifiers.json"
        ))?;
        let variables = serde_json::from_value::<Vec<String>>(fixture["variables"].clone())?;
        let events = serde_json::from_value::<Vec<String>>(fixture["events"].clone())?;
        let modifiers = fixture["modifiers"].as_object().unwrap();
        assert_eq!(modifiers.len(), 4);
        for value in modifiers.values() {
            let source: Vec<Authored> = serde_json::from_value(value["source"].clone())?;
            let tokens: Vec<RetailToken> =
                serde_json::from_value(value["compiled"]["tokens"].clone())?;
            let starts: Vec<usize> = serde_json::from_value(value["compiled"]["starts"].clone())?;
            assert_eq!(source.len(), starts.len());
            for (i, source) in source.iter().enumerate() {
                let statement = Statement::compile(source, &variables, &events)?;
                let end = starts.get(i + 1).copied().unwrap_or(tokens.len());
                let reference = Program::from_retail(&tokens[starts[i]..end])?;
                assert_eq!(statement.program.instructions(), reference.instructions());
                assert_eq!(
                    statement.variable,
                    index(
                        value["assignments"][i]["variable"].as_i64().unwrap() as i32,
                        variables.len()
                    )?
                );
                assert_eq!(
                    statement.event,
                    index(
                        value["assignments"][i]["event"].as_i64().unwrap() as i32,
                        events.len()
                    )?
                );
            }
        }
        Ok(())
    }

    fn authored(expression: &str, mode: u8) -> Authored {
        Authored {
            expression: expression.into(),
            assignment_variable_index: -1,
            assignment_event_index: -1,
            event_mode: mode,
        }
    }

    #[test]
    fn event_history_and_activation_follow_recovered_branches() -> Result<()> {
        for (mode, expected) in [
            (0, vec![false, true, false, false, false]),
            (1, vec![false, true, true, false, true]),
            (2, vec![false, true, false, false, true]),
            (3, vec![false, true, true, true, true]),
        ] {
            let mut statement =
                Statement::compile(&authored("go if x", mode), &["x".into()], &["go".into()])?;
            let observed: Vec<bool> = [0f32, 1., 2., -1., 1.]
                .into_iter()
                .map(|x| {
                    statement
                        .evaluate(&mut [x.to_bits()], &[4])
                        .map(|v| v.is_some())
                })
                .collect::<Result<_>>()?;
            assert_eq!(observed, expected, "mode {mode}");
            statement.activate();
            assert_eq!(statement.evaluate(&mut [1f32.to_bits()], &[4])?, Some(0));
        }
        Ok(())
    }

    #[test]
    fn assignments_are_sequential_and_unresolved_targets_remain_visible() -> Result<()> {
        let variables = vec!["speed".into(), "scaled".into()];
        let mut modifier = Modifier {
            enabled: true,
            statements: ["scaled = speed * 10", "run if scaled >= 5"]
                .iter()
                .map(|s| Statement::compile(&authored(s, 1), &variables, &["run".into()]))
                .collect::<Result<_>>()?,
        };
        let mut words = [0.5f32.to_bits(), 0];
        let mut events = vec![];
        modifier.evaluate(&mut words, &[4, 4], &mut events)?;
        assert_eq!(words[1], 5f32.to_bits());
        assert_eq!(events, [0]);
        let mut missing = Statement::compile(&authored("obsolete = 1", 0), &variables, &[])?;
        assert_eq!(missing.unresolved_target.as_deref(), Some("obsolete"));
        assert_eq!(missing.variable, None);
        assert_eq!(missing.evaluate(&mut words, &[4, 4])?, None);
        assert_eq!(words[1], 5f32.to_bits());
        Ok(())
    }
}
