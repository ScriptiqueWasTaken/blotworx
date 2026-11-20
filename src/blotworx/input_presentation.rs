//! Retail input-manager presentation and concrete CutScene state callbacks.
//! Message execution, cursor ownership and the global event bus are required
//! services. Recording a command is not a successful movie/event invocation.
use anyhow::Result;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub hud: u8,               // input manager +60
    pub locked: u8,            // +88
    pub mode_2d: u8,           // +89
    pub reticles: u8,          // +a8
    pub restore_reticles: u8,  // +a9
    pub restore_hud: u8,       // +aa
    pub modal: u8,             // +ab
    pub request: i32,          // context +8, input manager +4c
    pub cutscene_entries: u32, // separate concrete state object +4
}
impl Default for State {
    fn default() -> Self {
        // 1f5de0..1f5e68 and 1fb738..1fb74c, not guessed UI defaults.
        Self {
            hud: 1,
            locked: 0,
            mode_2d: 0,
            reticles: 0,
            restore_reticles: 1,
            restore_hud: 1,
            modal: 0,
            request: 0xa00f,
            cutscene_entries: 0,
        }
    }
}
pub trait Backend {
    fn flush(&mut self, owner: &mut State) -> Result<()>;
    fn message(&mut self, owner: &mut State, name: &str) -> Result<()>;
    fn cursor(&mut self, owner: &mut State, enabled: bool) -> Result<()>;
    fn menu(&mut self, owner: &mut State, command: &str, menu: &str, reason: &str) -> Result<()>;
    fn global_event(&mut self, owner: &mut State, kind: u16) -> Result<()>;
}
impl State {
    ///1f7878. Even redundant requests execute the native services. Preserve the
    ///original return byte across callbacks, and retain writes on failure.
    pub fn set_hud(&mut self, visible: bool, flush: bool, b: &mut impl Backend) -> Result<u8> {
        let previous = self.hud;
        if self.locked != 0 {
            return Ok(previous);
        }
        self.hud = u8::from(visible);
        if !visible && flush {
            b.flush(self)?;
        }
        // A flush callback can change mode_2d. Sample it AFTER that callback.
        let message = match (visible, self.mode_2d != 0) {
            (true, true) => "ShowHudFor2D",
            (true, false) => "ShowHud",
            (false, true) => "HideHudFor2D",
            (false, false) => "HideHud",
        };
        b.message(self, message)?;
        Ok(previous)
    }
    ///1f8638. A failed message must not proceed to the cursor service.
    pub fn set_reticles(&mut self, visible: bool, b: &mut impl Backend) -> Result<()> {
        self.reticles = u8::from(visible);
        b.message(
            self,
            if visible {
                "ShowReticles"
            } else {
                "HideReticles"
            },
        )?;
        b.cursor(self, visible)
    }
    ///1fb790. The callback may receive a null transition context. The ordinary
    ///manager passes input+44; it is NOT the state manager's phase field.
    pub fn enter_cutscene(&mut self, has_context: bool, b: &mut impl Backend) -> Result<()> {
        if has_context {
            self.request = -1;
        }
        b.menu(self, "Close", "Menu", "Black")?;
        self.cutscene_entries = self.cutscene_entries.wrapping_add(1);
        self.set_hud(false, true, b)?;
        self.set_reticles(false, b)?;
        b.global_event(self, 0x8b)
    }
    ///1fb850. Exit always emits8c, including nested exits and underflow. This
    ///counter is independent of application's RestrictCutSceneInput count.
    pub fn exit_cutscene(&mut self, next: i32, b: &mut impl Backend) -> Result<()> {
        b.global_event(self, 0x8c)?;
        self.cutscene_entries = self.cutscene_entries.wrapping_sub(1);
        if self.cutscene_entries == 0 && self.modal == 0 && next == 0xa00f {
            self.set_hud(self.restore_hud != 0, true, b)?;
            // The HUD callback can change the desired reticle state.
            self.set_reticles(self.restore_reticles != 0, b)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::ensure;
    use serde_json::{json, Value};
    struct Host<'a> {
        case: &'a Value,
        calls: Vec<Value>,
        counts: std::collections::BTreeMap<String, usize>,
    }
    impl Host<'_> {
        fn call(&mut self, owner: &mut State, op: &str, args: Value) -> Result<()> {
            self.calls.push(json!({"op":op,"args":args,"owner":owner}));
            let count = self.counts.entry(op.to_owned()).or_default();
            *count += 1;
            let fail = self.case["fail"].as_str();
            ensure!(
                fail != Some(op) && fail != Some(&format!("{op}:{count}")),
                "Supplied service failure"
            );
            let mutation = &self.case["mutation"];
            if mutation["on"] == op && mutation["occurrence"].as_u64().unwrap_or(1) == *count as u64
            {
                let mut value = serde_json::to_value(&*owner)?;
                for (key, v) in mutation["set"].as_object().into_iter().flatten() {
                    value[key] = v.clone();
                }
                *owner = serde_json::from_value(value)?;
            }
            Ok(())
        }
    }
    impl Backend for Host<'_> {
        fn flush(&mut self, o: &mut State) -> Result<()> {
            self.call(o, "flush", json!(null))
        }
        fn message(&mut self, o: &mut State, n: &str) -> Result<()> {
            self.call(o, "message", json!(n))
        }
        fn cursor(&mut self, o: &mut State, v: bool) -> Result<()> {
            self.call(o, "cursor", json!(v))
        }
        fn menu(&mut self, o: &mut State, c: &str, m: &str, r: &str) -> Result<()> {
            self.call(o, "menu", json!([c, m, r]))
        }
        fn global_event(&mut self, o: &mut State, k: u16) -> Result<()> {
            self.call(o, "event", json!(k))
        }
    }
    #[test]
    fn presentation_and_cutscene_callbacks_match_retail_instructions() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/input_presentation.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let mut state: State = serde_json::from_value(case["initial"].clone())?;
            let mut host = Host {
                case,
                calls: vec![],
                counts: Default::default(),
            };
            let result: Result<Option<u8>> = match case["operation"].as_str().unwrap() {
                "hud" => state
                    .set_hud(
                        case["visible"].as_bool().unwrap(),
                        case["flush"].as_bool().unwrap(),
                        &mut host,
                    )
                    .map(Some),
                "reticles" => state
                    .set_reticles(case["visible"].as_bool().unwrap(), &mut host)
                    .map(|_| None),
                "enter" => state
                    .enter_cutscene(case["context"].as_bool().unwrap(), &mut host)
                    .map(|_| None),
                "exit" => state
                    .exit_cutscene(case["next"].as_i64().unwrap() as i32, &mut host)
                    .map(|_| None),
                _ => unreachable!(),
            };
            assert_eq!(
                result.is_err(),
                !case["stopped"].is_null(),
                "Case{index}: {result:?}"
            );
            if let Ok(result) = result {
                assert_eq!(json!(result), case["returned"], "Case{index}");
            }
            assert_eq!(json!(state), case["output"], "Case{index}");
            assert_eq!(json!(host.calls), case["calls"], "Case{index}");
        }
        Ok(())
    }
}
