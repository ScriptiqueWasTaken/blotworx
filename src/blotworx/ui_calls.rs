//! UI movie invocation wrappers REL20b730/20b834/20b938, not save writes.
use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value<'a> {
    String(&'a str),
    Number(f64),
    BooleanByte(u8),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Invocation<'a> {
    pub method: &'a str,
    pub arguments: &'a [Value<'a>],
}
pub trait Movie {
    /// Native manager+c null skips the invocation, as original wrappers do.
    fn available(&mut self) -> Result<bool>;
    /// DOL800f26dc; wrappers ignore the returned movie invocation boolean.
    fn invoke(&mut self, call: Invocation<'_>) -> Result<bool>;
}
pub fn call(method: &str, argument: &str, value: Value<'_>, movie: &mut impl Movie) -> Result<()> {
    invoke(method, &[Value::String(argument), value], movie)
}
/// Deliver queued native two-string messages after movie initialization. A
/// successful prefix is committed; a failed call and its tail remain pending.
/// This boot bridge does not claim the retail UI manager's update scheduling.
pub fn drain_string_messages(
    queue: &mut Vec<(&'static str, &'static str, &'static str)>,
    movie: &mut impl Movie,
) -> Result<u64> {
    if !movie.available()? {
        return Ok(0);
    }
    let pending = std::mem::take(queue);
    let mut pending = pending.into_iter();
    let mut delivered = 0;
    while let Some((method, argument, value)) = pending.next() {
        let outcome = call(method, argument, Value::String(value), movie);
        if let Err(error) = outcome {
            queue.push((method, argument, value));
            queue.extend(pending);
            return Err(error);
        }
        delivered += 1;
    }
    Ok(delivered)
}
///20b594 has zero arguments;20b730/834/938 have two;20cf90 has three.
pub fn invoke(method: &str, arguments: &[Value<'_>], movie: &mut impl Movie) -> Result<()> {
    if movie.available()? {
        movie.invoke(Invocation { method, arguments })?;
    }
    Ok(())
}
///20ccb8 compares the promoted f32 aspect against a DOUBLE1.6. Comparing
///against1.6f32 changes the branch at that f32 value (0x3fcccccd).
pub fn initialization_arguments(aspect: f32) -> Result<[Value<'static>; 3]> {
    anyhow::ensure!(
        aspect.is_finite(),
        "Nonfinite native movie initialization aspect"
    );
    Ok([
        Value::Number(if (aspect as f64) > 1.6 { 853. } else { 640. }),
        Value::Number(480.),
        Value::BooleanByte(0),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_messages_preserve_order_and_commit_only_delivered_prefix() -> Result<()> {
        struct M {
            calls: Vec<&'static str>,
            fail: bool,
        }
        impl Movie for M {
            fn available(&mut self) -> Result<bool> {
                Ok(true)
            }
            fn invoke(&mut self, c: Invocation<'_>) -> Result<bool> {
                assert_eq!(
                    c.arguments,
                    &[Value::String("HudElement"), Value::String("AmmoBars")]
                );
                if self.fail && c.method == "Hide" {
                    anyhow::bail!("Expected interruption");
                }
                self.calls
                    .push(if c.method == "Show" { "Show" } else { "Hide" });
                Ok(false)
            }
        }
        let mut queue = vec![
            ("Show", "HudElement", "AmmoBars"),
            ("Hide", "HudElement", "AmmoBars"),
            ("Show", "HudElement", "AmmoBars"),
        ];
        let mut movie = M {
            calls: vec![],
            fail: true,
        };
        assert!(drain_string_messages(&mut queue, &mut movie).is_err());
        assert_eq!(movie.calls, ["Show"]);
        assert_eq!(queue.len(), 2);
        movie.fail = false;
        assert_eq!(drain_string_messages(&mut queue, &mut movie)?, 2);
        assert_eq!(movie.calls, ["Show", "Hide", "Show"]);
        assert!(queue.is_empty());
        Ok(())
    }
    #[test]
    fn initialization_arguments_match_native_float_double_boundary() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ui_initialization.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let aspect = f32::from_bits(case["aspect_bits"].as_u64().unwrap() as u32);
            let args = initialization_arguments(aspect)?;
            let actual = args.map(|v| match v {
                Value::Number(v) => serde_json::json!({"kind":3,"value":v}),
                Value::BooleanByte(v) => serde_json::json!({"kind":2,"value":v!=0}),
                _ => unreachable!(),
            });
            assert_eq!(serde_json::json!(actual), case["arguments"], "{aspect:?}");
        }
        assert_eq!(initialization_arguments(1.6)?[0], Value::Number(853.));
        assert!(initialization_arguments(f32::NAN).is_err());
        Ok(())
    }
    #[test]
    fn movie_wrappers_keep_typed_arguments_and_ignore_native_false() -> Result<()> {
        struct M {
            available: bool,
            calls: usize,
        }
        impl Movie for M {
            fn available(&mut self) -> Result<bool> {
                Ok(self.available)
            }
            fn invoke(&mut self, c: Invocation<'_>) -> Result<bool> {
                assert_eq!(
                    c,
                    Invocation {
                        method: "UpdateVariable",
                        arguments: &[Value::String("POWERUP"), Value::String("None")]
                    }
                );
                self.calls += 1;
                Ok(false)
            }
        }
        let mut movie = M {
            available: false,
            calls: 0,
        };
        call(
            "UpdateVariable",
            "POWERUP",
            Value::String("None"),
            &mut movie,
        )?;
        assert_eq!(movie.calls, 0);
        movie.available = true;
        call(
            "UpdateVariable",
            "POWERUP",
            Value::String("None"),
            &mut movie,
        )?;
        assert_eq!(movie.calls, 1);
        Ok(())
    }
}
