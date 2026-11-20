//! Concrete JPSBaseAIAction18ce28/18cef0 and its scalar request attributes.
//! Registry indices are supplied by actual metadata registration, never fixed
//! to an assumed global order. Entity/brain binding remains a required host.
use anyhow::{ensure, Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Kind {
    Heading,
    Speed,
    Steering,
    Acceleration,
    TorsoRotate,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Scalar {
    pub kind: Kind,
    pub changed: u8,
    pub value_bits: u32,
    #[serde(skip_serializing_if="Option::is_none")]
    pub secondary_bits: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Binding<'a> {
        case: &'a Value,
        trace: Vec<Value>,
        brain: Option<usize>,
    }
    impl Lookup for Binding<'_> {
        fn navigation_brain(&mut self, _: &mut Action, entity: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["brain", entity]));
            Ok(self.brain)
        }
        fn is_base_ai_brain(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
            self.trace.push(json!(["class", brain]));
            if self.case["mutation"] == "class_brain" {
                self.brain = Some(21);
            }
            if self.case["mutation"] == "class_clear" {
                self.brain = None;
            }
            Ok(self.case["class"].as_bool().unwrap_or(true) || self.case["derived"] == true)
        }
        fn entity_data(&mut self, _: &mut Action, entity: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["data", entity]));
            Ok(Some(40))
        }
        fn basic_controller(
            &mut self,
            action: &mut Action,
            data: Option<usize>,
        ) -> Result<Option<usize>> {
            self.trace.push(json!(["basic", data]));
            if self.case["mutation"] == "basic_heading_clear" {
                action.scalar_mut(Kind::Heading)?.changed = 0;
            }
            Ok(self.case["basic"].as_bool().unwrap_or(true).then_some(30))
        }
        fn heading_blocked(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
            self.trace.push(json!(["heading_gate", brain]));
            Ok(self.case[if brain == 21 {
                "replacement_heading_blocked"
            } else {
                "heading_blocked"
            }]
            .as_u64()
            .unwrap_or(0)
                != 0)
        }
        fn speed_blocked(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
            self.trace.push(json!(["speed_gate", brain]));
            Ok(self.case[if brain == 21 {
                "replacement_speed_blocked"
            } else {
                "speed_blocked"
            }]
            .as_u64()
            .unwrap_or(0)
                != 0)
        }
    }
    impl Host for Binding<'_> {
        fn facing(
            &mut self,
            action: &mut Action,
            controller: Option<usize>,
            direction: [f32; 3],
        ) -> Result<()> {
            self.trace
                .push(json!(["facing", controller, direction.map(f32::to_bits)]));
            if self.case["mutation"] == "facing_speed" {
                action.request(Kind::Speed, 9.5)?;
            }
            Ok(())
        }
        fn speed(&mut self, _: &mut Action, controller: Option<usize>, value: f32) -> Result<()> {
            self.trace
                .push(json!(["speed", controller, value.to_bits()]));
            Ok(())
        }
    }
    #[test]
    fn concrete_navigation_action_matches_original_slots_requests_acknowledgment_and_controller_dispatch(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_action.json"
        ))?;
        assert_eq!(fixture["registration"]["class_name"], Action::CLASS_NAME);
        assert_eq!(fixture["registration"]["factory"], Action::FACTORY_OFFSET);
        assert_eq!(
            super::super::ai_agent_profiles::Definition::built_in_biped("ShortThinBiped")
                .context("Missing native biped")?
                .action_class,
            Action::CLASS_NAME
        );
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let number = |key: &str, default: u32| c[key].as_u64().unwrap_or(default as u64) as u32;
            let registry = Registry {
                count: number("count", 5) as usize,
                heading: Class {
                    index: number("heading_index", 1) as i32,
                    constructor: true,
                },
                speed: Class {
                    index: number("speed_index", 3) as i32,
                    constructor: true,
                },
            };
            let mut action = Action::new(&registry)?;
            let mut host = Binding {
                case: c,
                trace: vec![],
                brain: c["brain"].as_bool().unwrap_or(true).then_some(20),
            };
            let operation = c["operation"].as_str().unwrap();
            if operation != "construct" {
                let heading = action.scalar_mut(Kind::Heading)?;
                heading.changed = number("heading_changed", 1) as u8;
                heading.value_bits = number("heading", 37.5f32.to_bits());
                let speed = action.scalar_mut(Kind::Speed)?;
                speed.changed = number("speed_changed", 1) as u8;
                speed.value_bits = number("speed", 3.25f32.to_bits());
                let request = f32::from_bits(number("request", (-2.5f32).to_bits()));
                match operation {
                    "heading" | "brain_heading" => action.request(Kind::Heading, request)?,
                    "brain_speed" => action.request(Kind::Speed, request)?,
                    "stop" => action.request(Kind::Speed, 0.)?,
                    "acknowledge" => action.acknowledge(),
                    "apply" => action.apply(10, &mut host)?,
                    _ => anyhow::bail!("Unexpected native operation"),
                }
            }
            let heading = action.scalar(Kind::Heading)?;
            let speed = action.scalar(Kind::Speed)?;
            assert_eq!(
                json!({"heading_changed":heading.changed,"heading":heading.value_bits,"speed_changed":speed.changed,"speed":speed.value_bits}),
                expected["state"],
                "state: {c}"
            );
            assert_eq!(json!(host.trace), expected["trace"], "trace: {c}");
            assert_eq!(
                json!(action
                    .attributes
                    .iter()
                    .map(|a| a.as_ref().map(|a| a.kind))
                    .collect::<Vec<_>>()),
                expected["slots"],
                "slots: {c}"
            );
        }
        Ok(())
    }
}
impl Scalar {
    pub fn request(&mut self, value: f32) {
        self.value_bits = value.to_bits();
        self.changed = 1;
    }
    pub fn acknowledge(&mut self) {
        self.changed = 0;
    } //18cee4, does not clear value
}
#[derive(Clone, Copy)]
pub struct Class {
    pub index: i32,
    pub constructor: bool,
}
pub struct Registry {
    pub count: usize,
    pub heading: Class,
    pub speed: Class,
}
#[derive(Debug, serde::Serialize)]
pub struct Action {
    pub attributes: Vec<Option<Scalar>>,
    heading: usize,
    speed: usize,
}
pub trait Lookup {
    fn navigation_brain(&mut self, action: &mut Action, entity: usize) -> Result<Option<usize>>;
    fn is_base_ai_brain(&mut self, action: &mut Action, brain: usize) -> Result<bool>;
    fn entity_data(&mut self, action: &mut Action, entity: usize) -> Result<Option<usize>>;
    fn basic_controller(
        &mut self,
        action: &mut Action,
        data: Option<usize>,
    ) -> Result<Option<usize>>;
    fn heading_blocked(&mut self, action: &mut Action, brain: usize) -> Result<bool>;
    fn speed_blocked(&mut self, action: &mut Action, brain: usize) -> Result<bool>;
}
pub trait Host: Lookup {
    fn facing(
        &mut self,
        action: &mut Action,
        controller: Option<usize>,
        direction: [f32; 3],
    ) -> Result<()>;
    fn speed(&mut self, action: &mut Action, controller: Option<usize>, value: f32) -> Result<()>;
}
impl Action {
    pub const CLASS_NAME: &'static str = "JPSBaseAIAction";
    pub const FACTORY_OFFSET: u32 = 0x18cd48;
    pub fn new(registry: &Registry) -> Result<Self> {
        ensure!(
            registry.count <= i32::MAX as usize,
            "Navigation attribute count exceeds its native signed-word domain"
        );
        let index = |class: Class| -> Result<usize> {
            ensure!(
                class.index >= 0 && (class.index as usize) < registry.count && class.constructor,
                "Required navigation action attribute class is not constructed by its registry"
            );
            Ok(class.index as usize)
        };
        let heading = index(registry.heading)?;
        let speed = index(registry.speed)?;
        ensure!(
            heading != speed,
            "Navigation attribute metadata aliases distinct classes to the same slot"
        );
        let mut attributes = Vec::new();
        attributes.try_reserve_exact(registry.count)?;
        attributes.resize_with(registry.count, || None);
        //18ce28 writes+0 and changed=1, then calls virtual18 to acknowledge.
        //4166bc's empty initialize does not add any other gameplay side effect.
        attributes[heading] = Some(Scalar {
            kind: Kind::Heading,
            changed: 0,
            value_bits: 0,
            secondary_bits: None,
        });
        attributes[speed] = Some(Scalar {
            kind: Kind::Speed,
            changed: 0,
            value_bits: 0,
            secondary_bits: None,
        });
        Ok(Self {
            attributes,
            heading,
            speed,
        })
    }
    pub fn scalar(&self, kind: Kind) -> Result<&Scalar> {
        let index = match kind {
            Kind::Heading => self.heading,
            Kind::Speed => self.speed,
            _ => anyhow::bail!("Extended action requests require their registered metadata slot"),
        };
        let value = self
            .attributes
            .get(index)
            .and_then(Option::as_ref)
            .context("Navigation action attribute is absent")?;
        ensure!(
            value.kind == kind,
            "Navigation action attribute class does not match its metadata slot"
        );
        Ok(value)
    }
    pub fn scalar_mut(&mut self, kind: Kind) -> Result<&mut Scalar> {
        let index = match kind {
            Kind::Heading => self.heading,
            Kind::Speed => self.speed,
            _ => anyhow::bail!("Extended action requests require their registered metadata slot"),
        };
        let value = self
            .attributes
            .get_mut(index)
            .and_then(Option::as_mut)
            .context("Navigation action attribute is absent")?;
        ensure!(
            value.kind == kind,
            "Navigation action attribute class does not match its metadata slot"
        );
        Ok(value)
    }
    pub fn request(&mut self, kind: Kind, value: f32) -> Result<()> {
        self.scalar_mut(kind)?.request(value);
        Ok(())
    }
    ///455b74 acknowledges every populated attribute, retaining its value.
    pub fn acknowledge(&mut self) {
        for value in self.attributes.iter_mut().flatten() {
            value.acknowledge();
        }
    }
    pub fn apply(&mut self, entity: usize, host: &mut impl Host) -> Result<()> {
        let Some(brain) = host.navigation_brain(self, entity)? else {
            return Ok(());
        };
        if !host.is_base_ai_brain(self, brain)? {
            return Ok(());
        }
        //18cf80 re-reads entity+18 after the class callback. Do not use
        //the pointer whose class was checked as a cached dispatch target.
        let brain = host.navigation_brain(self, entity)?;
        let data = host.entity_data(self, entity)?;
        let controller =
            if self.scalar(Kind::Heading)?.changed == 1 || self.scalar(Kind::Speed)?.changed == 1 {
                host.basic_controller(self, data)?
            } else {
                None
            };
        if self.scalar(Kind::Heading)?.changed == 1
            && !host.heading_blocked(
                self,
                brain.context("Navigation brain disappeared before heading dispatch")?,
            )?
        {
            //18cfe4..18d024: single degree conversion, double trig, then
            //separate single rounding of cosine and sine. No automatic reset.
            let degrees = f32::from_bits(self.scalar(Kind::Heading)?.value_bits);
            let radians = f32::from_bits(0x3c8efa35) * degrees;
            let (sin, cos) = super::native_angle::sin_cos_medium(radians as f64)?;
            host.facing(self, controller, [sin as f32, 0., cos as f32])?;
        }
        if self.scalar(Kind::Speed)?.changed == 1
            && !host.speed_blocked(
                self,
                brain.context("Navigation brain disappeared before speed dispatch")?,
            )?
        {
            let value = f32::from_bits(self.scalar(Kind::Speed)?.value_bits);
            host.speed(self, controller, value)?;
        }
        Ok(())
    }
}
