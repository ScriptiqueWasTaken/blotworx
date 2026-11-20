//! PC97e480 listener routing and fe7a10 MainGameState event field.
//! The explicit playtest owns main/cutscene states. This is not the global
//! EventManager subscription registry or the other game-state implementations.
use anyhow::{ensure,Result};
#[derive(Default,serde::Serialize)]
pub(super) struct State {
    pub main_flag_0c:bool,
    pub deliveries:u64,
    pub current_handler_calls:u64,
    pub main_handler_calls:u64,
}
impl State {
    pub fn receive(&mut self,current:u32,event:u16)->Result<()> {
        ensure!(matches!(current,0xa017|3),"PC GameState event receiver for state {current:#x} is not connected");
        // These operations need real additional owners/payloads. Never
        // reinterpret them as the MainGameState no-effect/default branch.
        ensure!(!matches!(event,0x2b|0x41|0x8b|0x8019),"PC GameState event {event:#x} needs pause/registration/movie/level-transition receivers");
        // Native calls the current state first. Cutscene's actual handler is
        // ret4; selected events also reach MainGameState, unless it is current.
        self.current_handler_calls+=1;
        if current==0xa017 || matches!(event,0x10|0x3c|0x3d) {
            self.main_handler_calls+=1;
            match event {
                0x3c=>self.main_flag_0c=true,
                0x10|0x3d=>self.main_flag_0c=false,
                _=>{},
            }
        }
        self.deliveries+=1;Ok(())
    }
    pub fn summary(&self)->serde_json::Value {
        serde_json::json!({"state":self,"global_subscription_registry_connected":false,
            "scope":"Original main/cutscene GameState listener routing and MainGameState flag; other states, pause/movie/transition receivers and global subscription traversal remain incomplete"})
    }
}
