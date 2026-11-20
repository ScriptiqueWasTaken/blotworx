//! Published motion inputs: native registrations2ebbe0/2ebe70/2ebf60,
//! setters30916c/308a48/307854. Full context/action ordering remains pending.
use anyhow::{ensure, Context, Result};
pub struct Bindings {
    supported: Option<usize>,
    motion: Option<usize>,
    vertical: Option<usize>,
}
impl Bindings {
    pub fn load(graph: &super::behavior::Document) -> Result<Self> {
        let strings = graph.single("hkbBehaviorGraphStringData")?;
        let names = strings["variableNames"]
            .as_array()
            .context("Missing controller animation variables")?;
        let data = graph.single("hkbBehaviorGraphData")?;
        let infos = data["variableInfos"]
            .as_array()
            .context("Missing controller animation types")?;
        let find = |name: &str, kind: u64| -> Result<Option<usize>> {
            let matches: Vec<_> = names
                .iter()
                .enumerate()
                .filter(|(_, value)| value.as_str() == Some(name))
                .collect();
            ensure!(
                matches.len() <= 1,
                "Ambiguous controller animation variable {name}"
            );
            let Some((index, _)) = matches.first() else {
                return Ok(None);
            };
            ensure!(
                infos.get(*index).and_then(|info| info["type"].as_u64()) == Some(kind),
                "Incorrect controller animation type for {name}"
            );
            Ok(Some(*index))
        };
        Ok(Self {
            supported: find("VAR_SupportedState", 3)?,
            motion: find("VAR_MotionState", 3)?,
            vertical: find("VAR_Y_Velocity", 4)?,
        })
    }
    pub fn apply(
        &self,
        player: &super::physics::Player,
        state: &mut super::behavior_state::State,
    ) -> Result<()> {
        if let (Some(index), Some(support)) = (self.supported, player.support_state) {
            state.set_integer(index, support as i32)?;
        }
        if let Some(index) = self.motion {
            state.set_integer(index, player.motion_state())?;
        }
        if let Some(index) = self.vertical {
            state.set_real(index, player.animation_velocity[1])?;
        }
        Ok(())
    }
}
