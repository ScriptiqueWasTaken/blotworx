//! JPSBaseAIBrain construction18d14c and initialization18d69c. Base brain,
//! services, concrete observers and traversal construction are required hosts;
//! a profile name alone is never accepted as a successfully initialized brain.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
pub enum WorldService {
    Word42060,
}
pub use super::ai_navigation_agent_factory::Class as AgentClass;
#[derive(Clone, Copy, Debug)]
pub enum Observer {
    FixedTurn,
    Default,
}
#[derive(Debug, serde::Serialize)]
pub struct Brain {
    pub identity: usize,
    pub flag_1c: bool,
    pub reference_20: Option<usize>,
    pub entries_24: Vec<usize>,
    pub gameplay_brain_2c: Option<usize>,
    pub motion_30: Option<usize>,
    pub flags_34_35: [bool; 2],
    pub values_38_3c: [f32; 2],
    pub owner_40: Option<usize>,
    pub reference_44: Option<usize>,
    pub agent_48: Option<usize>,
    pub agent_4c: Option<usize>,
    pub agent_50: Option<usize>,
    pub service_54: Option<usize>,
    pub flags_58_5b: [bool; 4],
    pub word_5c: i32,
    pub entries_60: Vec<usize>,
    pub flag_68: bool,
    pub observer_6c: Option<usize>,
    pub observer_70: Option<usize>,
    pub traversal_74: Option<usize>,
    pub flag_78: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Binding<'a> {
        case: &'a Value,
        trace: Vec<Value>,
    }
    impl Binding<'_> {
        fn flag(&self, key: &str) -> bool {
            self.case[key].as_bool().unwrap_or(true)
        }
    }
    impl Construction for Binding<'_> {
        fn construct_base(
            &mut self,
            identity: usize,
            owner: Option<usize>,
            action_class: usize,
        ) -> Result<()> {
            self.trace
                .push(json!(["construct_base", identity, owner, action_class]));
            Ok(())
        }
    }
    impl Host for Binding<'_> {
        fn output_target_class(&mut self)->Result<usize>{Ok(1)}
        fn initialize_base(&mut self, _: &mut Brain, attributes: Option<usize>) -> Result<bool> {
            self.trace.push(json!(["initialize", attributes]));
            Ok(self.flag("base"))
        }
        fn owner_service_6af6c(&mut self, _: &mut Brain) -> Result<Option<usize>> {
            self.trace.push(json!(["actor_service"]));
            Ok(self.flag("actor").then_some(54))
        }
        fn agent(&mut self, _: &mut Brain, kind: AgentClass) -> Result<Option<usize>> {
            let (name, key, id) = match kind {
                AgentClass::Goto => ("117f0", "117f0", 76),
                AgentClass::Flee => ("1179c", "1179c", 80),
                AgentClass::Wander => ("11498", "11498", 72),
            };
            self.trace.push(json!(["agent", name]));
            Ok(self.flag(key).then_some(id))
        }
        fn world_service(&mut self,_:&mut Brain,kind:WorldService)->Result<Option<usize>>{
            let WorldService::Word42060=kind;
            self.trace.push(json!(["world","42060"]));Ok(self.flag("region_service").then_some(300))
        }
        fn construct_observer(
            &mut self,
            brain: &mut Brain,
            kind: Observer,
            name: &str,
        ) -> Result<usize> {
            let (size, id) = match kind {
                Observer::FixedTurn => (0x44, 112),
                Observer::Default => (0x48, 108),
            };
            self.trace.push(json!(["allocate", size]));
            let service=brain.service_54.context("Observer service is null")?;
            self.trace.push(json!(["observer", name, service]));
            Ok(id)
        }
        fn initialize_observer(&mut self, e: &mut Brain, id: usize) -> Result<()> {
            self.trace.push(json!(["observer_init", id]));
            if self.case["mutation"] == format!("observer{id}") {
                e.service_54 = Some(55)
            }
            Ok(())
        }
        fn construct_action(&mut self, _: &mut Brain) -> Result<Option<usize>> {
            self.trace.push(json!(["allocate", 0x18]));
            Ok(self.flag("action").then_some(48))
        }
        fn filter_1ac(&mut self, _: &mut Brain, service: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["filter", service]));
            Ok(self
                .flag("filter")
                .then_some(if service == 54 { 201 } else { 211 }))
        }
        fn install_filter_callback_18d620(&mut self, _: &mut Brain, filter: usize) -> Result<()> {
            self.trace.push(json!(["filter_callback", filter]));
            Ok(())
        }
        fn construct_traversal_1c1854(&mut self, _: &mut Brain) -> Result<usize> {
            self.trace.push(json!(["traversal_allocate"]));
            self.trace.push(json!(["traversal_construct"]));
            Ok(116)
        }
        fn set_traversal_flag_8b0(&mut self, _: &mut Brain, traversal: usize) -> Result<()> {
            self.trace.push(json!(["traversal_flag", traversal]));
            Ok(())
        }
        fn initialize_traversal(
            &mut self,
            e: &mut Brain,
            traversal: usize,
            name: &str,
        ) -> Result<bool> {
            self.trace.push(json!(["traversal_init", traversal, name]));
            if self.case["mutation"] == "traversal" {
                e.service_54 = Some(55)
            }
            Ok(self.flag("traversal"))
        }
        fn register_region_186a7c(
            &mut self,
            _: &mut Brain,
            service: usize,
        ) -> Result<Option<usize>> {
            self.trace.push(json!(["register", service]));
            Ok(self.flag("region").then_some(301))
        }
        fn route_context_f8(&mut self, _: &mut Brain, service: usize) -> Result<usize> {
            self.trace.push(json!(["route_context", service]));
            Ok(if service == 54 { 203 } else { 213 })
        }
        fn set_route_owner(&mut self, _: &mut Brain, context: usize, owner: usize) -> Result<()> {
            self.trace.push(json!(["route_owner", context, owner]));
            Ok(())
        }
        fn refresh_route(&mut self, _: &mut Brain, context: usize) -> Result<()> {
            self.trace.push(json!(["refresh", context]));
            Ok(())
        }
        fn output_198(&mut self, _: &mut Brain, service: usize) -> Result<usize> {
            self.trace.push(json!(["output", service]));
            Ok(if service == 54 { 202 } else { 212 })
        }
        fn output_class(&mut self, _: &mut Brain, output: usize) -> Result<usize> {
            self.trace.push(json!(["output_class", output]));
            Ok(2)
        }
        fn parent_class(&mut self, class: usize) -> Result<Option<usize>> {
            Ok(match class {
                2 => Some(3),
                3 => self.flag("class_match").then_some(1),
                _ => None,
            })
        }
        fn install_output_callback_18d620(&mut self, _: &mut Brain, output: usize) -> Result<()> {
            self.trace.push(json!(["output_callback", output]));
            Ok(())
        }
    }
    #[test]
    fn native_navigation_brain_construction_and_initialization_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_brain.json"
        ))?;
        assert_eq!(fixture["registration"]["class_name"], Brain::CLASS_NAME);
        assert_eq!(fixture["registration"]["factory"], Brain::FACTORY_OFFSET);
        assert_eq!(
            super::super::ai_agent_profiles::Definition::built_in_biped("ShortThinBiped")
                .context("Missing native biped definition")?
                .brain_class,
            Brain::CLASS_NAME
        );
        for expected in fixture["cases"].as_array().unwrap() {
            let case = &expected["input"];
            let mut host = Binding {
                case,
                trace: vec![],
            };
            let mut e = Brain::new(1, host.flag("owner").then_some(3), 90, &mut host)?;
            let result = if case["op"] == "initialize" {
                if case["seeded"] == true {
                    e.motion_30 = Some(800);
                    e.agent_48 = Some(900);
                    e.agent_4c = Some(901);
                    e.agent_50 = Some(902);
                    e.service_54 = Some(903);
                    e.observer_6c = Some(700);
                    e.observer_70 = Some(701);
                    e.traversal_74 = Some(740);
                }
                Some(e.initialize(Some(88), &mut host)?)
            } else {
                None
            };
            let state = json!({"1c":u8::from(e.flag_1c),"20":e.reference_20,"24":None::<usize>,
                "28_2a":[e.entries_24.len(),e.entries_24.capacity()],"2c":e.gameplay_brain_2c,"30":e.motion_30,
                "34":u8::from(e.flags_34_35[0]),"35":u8::from(e.flags_34_35[1]),
                "38":e.values_38_3c[0].to_bits(),"3c":e.values_38_3c[1].to_bits(),"40":e.owner_40,"44":e.reference_44,
                "48":e.agent_48,"4c":e.agent_4c,"50":e.agent_50,"54":e.service_54,
                "58":u8::from(e.flags_58_5b[0]),"59":u8::from(e.flags_58_5b[1]),"5a":u8::from(e.flags_58_5b[2]),"5b":u8::from(e.flags_58_5b[3]),
                "5c":e.word_5c as u32,"60":None::<usize>,"64_66":[e.entries_60.len(),e.entries_60.capacity()],
                "68":u8::from(e.flag_68),"6c":e.observer_6c,"70":e.observer_70,"74":e.traversal_74,"78":u8::from(e.flag_78)});
            assert_eq!(state, expected["state"], "state: {case}");
            assert_eq!(json!(host.trace), expected["trace"], "trace: {case}");
            assert_eq!(json!(result), expected["result"], "return: {case}");
        }
        Ok(())
    }
}
pub trait Construction {
    ///458b70 receives the owning navigation entity and ActionClass metadata;
    ///its third argument is not an entity/profile name.
    fn construct_base(
        &mut self,
        identity: usize,
        owner: Option<usize>,
        action_class: usize,
    ) -> Result<()>;
}
pub trait Host: Construction {
    /// The target class's portable metadata identity, not a host/Wii pointer.
    fn output_target_class(&mut self)->Result<usize>;
    fn initialize_base(&mut self, brain: &mut Brain, attributes: Option<usize>) -> Result<bool>;
    fn owner_service_6af6c(&mut self, brain: &mut Brain) -> Result<Option<usize>>;
    ///4594e8 searches THIS brain's ordered agents, not the World service list.
    fn agent(&mut self,brain:&mut Brain,class:AgentClass)->Result<Option<usize>>;
    fn world_service(&mut self, brain: &mut Brain, kind: WorldService) -> Result<Option<usize>>;
    /// Must construct the actual18dc20/18db5c derived observer, including its
    ///native field initialization. A failed required allocation is an error.
    fn construct_observer(
        &mut self,
        brain: &mut Brain,
        kind: Observer,
        name: &str,
    ) -> Result<usize>;
    fn initialize_observer(&mut self, brain: &mut Brain, observer: usize) -> Result<()>;
    ///18db2c/18db1c: actual action with zero vector and scalar. Allocation
    ///failure is a genuine false return from18d69c, unlike observer failure.
    fn construct_action(&mut self, brain: &mut Brain) -> Result<Option<usize>>;
    fn filter_1ac(&mut self, brain: &mut Brain, service: usize) -> Result<Option<usize>>;
    fn install_filter_callback_18d620(&mut self, brain: &mut Brain, filter: usize) -> Result<()>;
    fn construct_traversal_1c1854(&mut self, brain: &mut Brain) -> Result<usize>;
    fn set_traversal_flag_8b0(&mut self, brain: &mut Brain, traversal: usize) -> Result<()>;
    fn initialize_traversal(
        &mut self,
        brain: &mut Brain,
        traversal: usize,
        name: &str,
    ) -> Result<bool>;
    fn register_region_186a7c(
        &mut self,
        brain: &mut Brain,
        world_service: usize,
    ) -> Result<Option<usize>>;
    fn route_context_f8(&mut self, brain: &mut Brain, service: usize) -> Result<usize>;
    fn set_route_owner(&mut self, brain: &mut Brain, context: usize, owner: usize) -> Result<()>;
    fn refresh_route(&mut self, brain: &mut Brain, context: usize) -> Result<()>;
    fn output_198(&mut self, brain: &mut Brain, service: usize) -> Result<usize>;
    fn output_class(&mut self, brain: &mut Brain, output: usize) -> Result<usize>;
    fn parent_class(&mut self, class: usize) -> Result<Option<usize>>;
    fn install_output_callback_18d620(&mut self, brain: &mut Brain, output: usize) -> Result<()>;
}
impl Brain {
    pub const CLASS_NAME: &'static str = "JPSBaseAIBrain";
    pub const FACTORY_OFFSET: u32 = 0x18d0b4;
    pub fn new(
        identity: usize,
        owner: Option<usize>,
        action_class: usize,
        host: &mut impl Construction,
    ) -> Result<Self> {
        host.construct_base(identity, owner, action_class)?;
        Ok(Self {
            identity,
            flag_1c: false,
            reference_20: None,
            entries_24: vec![],
            gameplay_brain_2c: None,
            motion_30: None,
            flags_34_35: [false; 2],
            values_38_3c: [0.; 2],
            owner_40: owner,
            reference_44: None,
            agent_48: None,
            agent_4c: None,
            agent_50: None,
            service_54: None,
            flags_58_5b: [false; 4],
            word_5c: -1,
            entries_60: vec![],
            flag_68: true,
            observer_6c: None,
            observer_70: None,
            traversal_74: None,
            flag_78: false,
        })
    }
    fn service(&self) -> Result<usize> {
        self.service_54
            .context("Navigation brain lost its required owner service")
    }
    pub fn initialize<H: Host>(&mut self, attributes: Option<usize>, host: &mut H) -> Result<bool> {
        if !host.initialize_base(self, attributes)? {
            return Ok(false);
        }
        self.service_54 = host.owner_service_6af6c(self)?;
        if self.service_54.is_none() {
            return Ok(false);
        }
        self.agent_4c = host.agent(self, AgentClass::Goto)?;
        if self.agent_4c.is_none() {
            return Ok(false);
        }
        self.agent_50 = host.agent(self, AgentClass::Flee)?;
        if self.agent_50.is_none() {
            return Ok(false);
        }
        self.agent_48 = host.agent(self, AgentClass::Wander)?;
        if self.agent_48.is_none() {
            return Ok(false);
        }
        let observer = host.construct_observer(self, Observer::FixedTurn, "FixedTurn")?;
        self.observer_70 = Some(observer);
        host.initialize_observer(self, observer)?;
        let observer = host.construct_observer(self, Observer::Default, "Default")?;
        self.observer_6c = Some(observer);
        host.initialize_observer(self, observer)?;
        self.motion_30 = host.construct_action(self)?;
        if self.motion_30.is_none() {
            return Ok(false);
        }
        let service = self.service()?;
        if let Some(filter) = host.filter_1ac(self, service)? {
            host.install_filter_callback_18d620(self, filter)?;
        }
        self.traversal_74 = Some(host.construct_traversal_1c1854(self)?);
        let traversal = self
            .traversal_74
            .context("Navigation traversal construction is absent")?;
        host.set_traversal_flag_8b0(self, traversal)?;
        let traversal = self
            .traversal_74
            .context("Navigation traversal flag callback removed its owner")?;
        if !host.initialize_traversal(self, traversal, "CanReachTraversal")? {
            return Ok(false);
        }
        if let Some(service) = host.world_service(self, WorldService::Word42060)? {
            let Some(owner) = host.register_region_186a7c(self, service)? else {
                return Ok(false);
            };
            let service = self.service()?;
            let context = host.route_context_f8(self, service)?;
            host.set_route_owner(self, context, owner)?;
            let service = self.service()?;
            let context = host.route_context_f8(self, service)?;
            host.refresh_route(self, context)?;
        }
        let service = self.service()?;
        let output = host.output_198(self, service)?;
        let mut class = host.output_class(self, output)?;
        let target_class=host.output_target_class()?;
        let mut visited = BTreeSet::new();
        loop {
            if class == target_class {
                host.install_output_callback_18d620(self, output)?;
                break;
            }
            ensure!(
                visited.insert(class),
                "Cyclic navigation output class ancestry"
            );
            let Some(parent) = host.parent_class(class)? else {
                break;
            };
            class = parent;
        }
        Ok(true)
    }
}
