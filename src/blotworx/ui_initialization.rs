//! Retail20a90c HUD_INITIALIZED callback. Native player getters and movie
//! invocation are mandatory services; errors retain preceding flag/UI writes.
use super::ui_calls::{self, Movie, Value};
use anyhow::Result;

#[derive(Default)]
pub struct State {
    initialized: std::sync::atomic::AtomicU8,
}
#[derive(Clone, Copy, Debug)]
pub enum Component {
    Health,
    Inventory,
}
pub trait Backend: Movie {
    type Entity: Copy;
    type Owner: Copy;
    fn player(&mut self) -> Result<Option<Self::Entity>>;
    fn component(&mut self, player: Self::Entity, kind: Component) -> Result<Option<Self::Owner>>;
    fn integer(&mut self, owner: Self::Owner, name: &str) -> Result<i32>;
    fn float(&mut self, owner: Self::Owner, name: &str) -> Result<f32>;
}
impl State {
    pub fn initialized(&self) -> u8 {
        self.initialized.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn command(&self, command: &str, backend: &mut impl Backend) -> Result<bool> {
        if command.split('\0').next() != Some("HUD_INITIALIZED") {
            return Ok(false);
        }
        self.initialized
            .store(1, std::sync::atomic::Ordering::SeqCst);
        let Some(player) = backend.player()? else {
            return Ok(true);
        };
        if let Some(owner) = backend.component(player, Component::Health)? {
            for name in ["Lives", "MaxHealth", "Health"] {
                let value = backend.integer(owner, name)? as f64;
                update(name, value, backend)?;
            }
        }
        if let Some(owner) = backend.component(player, Component::Inventory)? {
            for name in ["PaintMax", "Paint", "ThinnerMax", "Thinner", "ETicket"] {
                let value = backend.float(owner, name)? as f64;
                update(name, value, backend)?;
            }
        }
        Ok(true)
    }
}
fn update(name: &str, value: f64, movie: &mut impl Movie) -> Result<()> {
    ui_calls::invoke(
        "Update",
        &[
            Value::String("UPDATE"),
            Value::String("Player01"),
            Value::String(name),
            Value::Number(value),
        ],
        movie,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_player_initialization_order_and_values() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ui_player_initialization.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            struct B<'a> {
                state: &'a State,
                case: &'a serde_json::Value,
                calls: Vec<String>,
                updates: Vec<serde_json::Value>,
            }
            impl Movie for B<'_> {
                fn available(&mut self) -> Result<bool> {
                    Ok(true)
                }
                fn invoke(&mut self, call: ui_calls::Invocation<'_>) -> Result<bool> {
                    assert_eq!(call.method, "Update");
                    let [Value::String(action), Value::String(entity), Value::String(property), Value::Number(value)] =
                        call.arguments
                    else {
                        panic!("Wrong native message types")
                    };
                    self.calls.push(format!("update {property}"));
                    self.updates.push(serde_json::json!({"action":action,"entity":entity,"property":property,"number_bits":value.to_bits()}));
                    Ok(false)
                }
            }
            impl Backend for B<'_> {
                type Entity = ();
                type Owner = Component;
                fn player(&mut self) -> Result<Option<()>> {
                    assert_eq!(self.state.initialized(), 1);
                    if self.case["player"] == true {
                        self.calls.push("player".into());
                        Ok(Some(()))
                    } else {
                        Ok(None)
                    }
                }
                fn component(&mut self, _: (), kind: Component) -> Result<Option<Component>> {
                    let (name, key) = match kind {
                        Component::Health => ("Health", "health"),
                        Component::Inventory => ("Inventory", "inventory"),
                    };
                    self.calls.push(format!("component {name}"));
                    Ok((self.case[key] == true).then_some(kind))
                }
                fn integer(&mut self, _: Component, name: &str) -> Result<i32> {
                    self.calls.push(format!("get {name}"));
                    let i = ["Lives", "MaxHealth", "Health"]
                        .iter()
                        .position(|v| *v == name)
                        .unwrap();
                    Ok(self.case["integers"][i].as_i64().unwrap() as i32)
                }
                fn float(&mut self, _: Component, name: &str) -> Result<f32> {
                    self.calls.push(format!("get {name}"));
                    let i = ["PaintMax", "Paint", "ThinnerMax", "Thinner", "ETicket"]
                        .iter()
                        .position(|v| *v == name)
                        .unwrap();
                    Ok(f32::from_bits(
                        self.case["float_bits"][i].as_u64().unwrap() as u32
                    ))
                }
            }
            let state = State::default();
            let mut backend = B {
                state: &state,
                case,
                calls: Vec::new(),
                updates: Vec::new(),
            };
            state.command(case["command"].as_str().unwrap(), &mut backend)?;
            assert_eq!(
                state.initialized() as u64,
                case["initialized"].as_u64().unwrap()
            );
            assert_eq!(serde_json::json!(backend.calls), case["calls"]);
            assert_eq!(serde_json::json!(backend.updates), case["updates"]);
        }
        Ok(())
    }
}
