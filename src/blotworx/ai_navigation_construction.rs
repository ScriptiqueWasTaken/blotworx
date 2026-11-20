//! Concrete construction-stage JPSBaseAIBrain, its base storage and owned
//! JPSBaseAIAction. Service initialization/registration are separate stages.
use super::{
    ai_navigation_action::{Action, Registry},
    ai_navigation_brain::{Brain, Construction},
    ai_navigation_brain_base::State,
};
use anyhow::{ensure, Context, Result};

pub enum ActionFactory {
    BaseAi,
    Unsupported(String),
}
pub struct ActionClass {
    pub metadata: usize,
    pub factory: Option<ActionFactory>,
}
pub struct Layout {
    pub agent_classes: usize,
    pub service_classes: usize,
    pub attributes: Registry,
}

#[cfg(test)]
mod tests {
    use super::super::ai_navigation_action::{Class, Kind};
    use super::*;
    use serde_json::{json, Value};
    #[test]
    fn concrete_navigation_brain_construction_owns_base_action_and_metadata_slots() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_construction.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let count = |key: &str| c[key].as_u64().unwrap() as usize;
            let layout = Layout {
                agent_classes: count("agents"),
                service_classes: count("services"),
                attributes: Registry {
                    count: count("attributes"),
                    heading: Class {
                        index: count("heading_index") as i32,
                        constructor: true,
                    },
                    speed: Class {
                        index: count("speed_index") as i32,
                        constructor: true,
                    },
                },
            };
            let class = ActionClass {
                metadata: 90,
                factory: if c["factory"] == true {
                    Some(ActionFactory::BaseAi)
                } else {
                    None
                },
            };
            let owned = Constructed::new(
                20,
                (c["owner"] == true).then_some(10),
                Some(30),
                &class,
                &layout,
            )?;
            let brain = &owned.brain;
            let base = &owned.base;
            let resources = [
                brain.motion_30,
                brain.agent_48,
                brain.agent_4c,
                brain.agent_50,
                brain.service_54,
                brain.observer_6c,
                brain.observer_70,
                brain.traversal_74,
            ];
            let flags = [
                brain.flag_1c,
                brain.flags_34_35[0],
                brain.flags_34_35[1],
                brain.flags_58_5b[0],
                brain.flags_58_5b[1],
                brain.flags_58_5b[2],
                brain.flags_58_5b[3],
                brain.flag_78,
            ];
            assert_eq!(
                json!({"owner":base.owner.is_some(),"action":base.action.is_some(),"agents":base.agents.len(),"services":base.services.len(),
                "agent_storage":base.agents.capacity()!=0,"service_storage":base.services.capacity()!=0,"owner_40":brain.owner_40.is_some(),
                "word_5c":brain.word_5c as u32,"flag_68":u8::from(brain.flag_68),"initialized_resources":resources.map(|id|u8::from(id.is_some())),"flags":flags.map(u8::from)}),
                expected["state"],
                "state: {c}"
            );
            let (slots, values) = if let Some(action) = &owned.action {
                let heading = action.scalar(Kind::Heading)?;
                let speed = action.scalar(Kind::Speed)?;
                (
                    json!(action
                        .attributes
                        .iter()
                        .map(|a| a.as_ref().map(|a| a.kind))
                        .collect::<Vec<_>>()),
                    json!({"heading_changed":heading.changed,"heading":heading.value_bits,"speed_changed":speed.changed,"speed":speed.value_bits}),
                )
            } else {
                (Value::Null, Value::Null)
            };
            assert_eq!(slots, expected["slots"], "slots: {c}");
            assert_eq!(values, expected["values"], "values: {c}");
            assert_eq!(base.owner, brain.owner_40);
        }
        let layout = Layout {
            agent_classes: 0,
            service_classes: 0,
            attributes: Registry {
                count: 2,
                heading: Class {
                    index: 0,
                    constructor: true,
                },
                speed: Class {
                    index: 1,
                    constructor: true,
                },
            },
        };
        let unsupported = ActionClass {
            metadata: 91,
            factory: Some(ActionFactory::Unsupported("UnimplementedAction".into())),
        };
        assert!(Constructed::new(20, Some(10), Some(30), &unsupported, &layout).is_err());
        Ok(())
    }
}
pub struct Constructed {
    pub brain: Brain,
    pub base: State,
    pub action: Option<Action>,
}
impl Constructed {
    pub fn new(
        identity: usize,
        owner: Option<usize>,
        action_identity: Option<usize>,
        class: &ActionClass,
        layout: &Layout,
    ) -> Result<Self> {
        struct Binding<'a> {
            layout: &'a Layout,
            class: &'a ActionClass,
            action_identity: Option<usize>,
            base: Option<State>,
            action: Option<Action>,
        }
        impl Construction for Binding<'_> {
            fn construct_base(
                &mut self,
                _: usize,
                owner: Option<usize>,
                action_class: usize,
            ) -> Result<()> {
                ensure!(
                    action_class == self.class.metadata,
                    "Navigation brain constructor received different ActionClass metadata"
                );
                ensure!(
                    self.layout.agent_classes <= i32::MAX as usize
                        && self.layout.service_classes <= i32::MAX as usize,
                    "Navigation class registry exceeds its native signed-word domain"
                );
                //458b70 allocates agent slots before service slots and invokes
                //the actual ActionClass constructor afterward. Their live
                //counts start zero; registry counts are allocation capacity.
                let mut agents = Vec::new();
                agents.try_reserve_exact(self.layout.agent_classes)?;
                let mut services = Vec::new();
                services.try_reserve_exact(self.layout.service_classes)?;
                let (action, handle) = match &self.class.factory {
                    None => (None, None),
                    Some(ActionFactory::BaseAi) => (
                        Some(Action::new(&self.layout.attributes)?),
                        Some(self.action_identity.context(
                            "Constructed navigation action needs its allocated identity",
                        )?),
                    ),
                    Some(ActionFactory::Unsupported(name)) => anyhow::bail!(
                        "Navigation ActionClass constructor is not implemented: {name}"
                    ),
                };
                self.base = Some(State {
                    owner,
                    action: handle,
                    services,
                    agents,
                });
                self.action = action;
                Ok(())
            }
        }
        let mut binding = Binding {
            layout,
            class,
            action_identity,
            base: None,
            action: None,
        };
        let brain = Brain::new(identity, owner, class.metadata, &mut binding)?;
        Ok(Self {
            brain,
            base: binding
                .base
                .context("Navigation base constructor did not produce storage")?,
            action: binding.action,
        })
    }
}
