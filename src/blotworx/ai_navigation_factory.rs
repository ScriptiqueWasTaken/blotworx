//! Navigation entity/brain factory4577b0. Concrete constructors, initialized
//! services, World context and registry/pool ownership remain mandatory hosts.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Entity,
    Brain,
    Action,
}
/// Common fields touched by4577b0, owned by the same concrete entity record
/// its constructor installs. This is not a substitute constructed entity.
#[derive(Debug, serde::Serialize)]
pub struct Record {
    pub pool_node: Option<usize>, //c
    pub word_14: u32,
    pub brain: Option<usize>,      //18
    pub flags: u32,                //1c
    pub definition: Option<usize>, //24
    pub word_28: u32,
    pub world: Option<usize>, //30
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn label(kind: Kind) -> &'static str {
        match kind {
            Kind::Entity => "Entity",
            Kind::Brain => "Brain",
            Kind::Action => "Action",
        }
    }
    struct Binding<'a> {
        case: &'a Value,
        trace: Vec<Value>,
        constructed: bool,
        world_reads: usize,
        inline_entity: Option<super::super::ai_navigation_entity_base::Entity>,
    }
    impl Binding<'_> {
        fn flag(&self, key: &str) -> bool {
            self.case[key].as_bool().unwrap_or(true)
        }
    }
    impl Host for Binding<'_> {
        fn record_mut<'a>(&'a mut self, runtime: &'a mut Runtime, entity: usize) -> Result<&'a mut Record> {
            if self.case["inline_storage"] == true {
                let owned = self.inline_entity.as_mut().context("Missing inline entity")?;
                ensure!(owned.identity == entity, "Wrong inline entity identity");
                Ok(&mut owned.common)
            } else {
                runtime.records.get_mut(&entity).context("Missing constructed entity")
            }
        }
        fn contains_entity(&self, runtime: &Runtime, entity: usize) -> bool {
            if self.case["inline_storage"] == true {
                self.inline_entity.as_ref().is_some_and(|e|e.identity == entity)
            } else {runtime.records.contains_key(&entity)}
        }
        fn enabled_45e6bc(&mut self, _: &mut Runtime) -> Result<bool> {
            self.trace.push(json!(["enabled"]));
            Ok(self.flag("enabled"))
        }
        fn world_context(&mut self, _: &mut Runtime) -> Result<Option<usize>> {
            self.trace.push(json!(["world"]));
            self.world_reads += 1;
            Ok((self.flag("world")
                && (!self.constructed || self.flag("world_after"))
                && (self.world_reads != 3 || self.flag("world_store")))
            .then_some(300))
        }
        fn pool_has_free(&mut self, _: &mut Runtime) -> Result<bool> {
            self.trace.push(json!(["pool_available"]));
            Ok(self.flag("free"))
        }
        fn class_name(
            &mut self,
            _: &mut Runtime,
            node: usize,
            key: &str,
        ) -> Result<Option<Vec<u8>>> {
            self.trace.push(json!(["class_name", node, key]));
            Ok(self.flag("class_value").then(|| {
                if key == "Class" {
                    if node == 1 {
                        b"EntityClass".to_vec()
                    } else {
                        b"BrainClass".to_vec()
                    }
                } else {
                    b"ActionClass".to_vec()
                }
            }))
        }
        fn class_metadata(
            &mut self,
            _: &mut Runtime,
            kind: Kind,
            name: Option<&[u8]>,
        ) -> Result<Option<usize>> {
            self.trace.push(json!([
                "metadata",
                label(kind),
                name.map(String::from_utf8_lossy)
            ]));
            let key = match kind {
                Kind::Entity => "entity_class",
                Kind::Brain => "brain_class",
                Kind::Action => "action_class",
            };
            Ok((name.is_some() && self.flag(key)).then_some(match kind {
                Kind::Entity => 50,
                Kind::Brain => 51,
                Kind::Action => 52,
            }))
        }
        fn class_factory(
            &mut self,
            _: &mut Runtime,
            kind: Kind,
            metadata: usize,
        ) -> Result<Option<usize>> {
            self.trace.push(json!(["factory", label(kind), metadata]));
            Ok(self
                .flag(if kind == Kind::Entity {
                    "entity_factory"
                } else {
                    "brain_factory"
                })
                .then_some(if kind == Kind::Entity { 1000 } else { 1001 }))
        }
        fn construct_entity(
            &mut self,
            runtime: &mut Runtime,
            factory: usize,
            name: &[u8],
            user: Option<usize>,
        ) -> Result<Option<usize>> {
            assert_eq!(user, Some(7));
            self.trace
                .push(json!(["entity_construct", factory, name, user]));
            self.constructed = true;
            if !self.flag("entity") {
                return Ok(None);
            }
            let record = Record {
                    pool_node: Some(888),
                    word_14: 77,
                    brain: None,
                    flags: 99,
                    definition: Some(88),
                    word_28: 2,
                    world: Some(400),
                };
            if self.case["inline_storage"] == true {
                let mut entity=super::super::ai_navigation_entity_base::Entity::new(10,Some(name),[7,0],0)?;
                entity.common=record;
                self.inline_entity=Some(entity);
            } else {runtime.records.insert(10,record);}
            Ok(Some(10))
        }
        fn initialize_entity(
            &mut self,
            runtime: &mut Runtime,
            entity: usize,
            definition: usize,
        ) -> Result<bool> {
            let record = self.record_mut(runtime,entity)?;
            let (world,word_28)=(record.world,record.word_28);
            self.trace.push(json!([
                "entity_initialize",
                entity,
                definition,
                world,
                word_28
            ]));
            if self.case["entity_mutation"] == true {
                self.record_mut(runtime,entity)?.word_28 = 3;
            }
            Ok(self.flag("entity_initialize"))
        }
        fn brain_definition(
            &mut self,
            _: &mut Runtime,
            definition: usize,
        ) -> Result<Option<usize>> {
            self.trace.push(json!(["brain_definition", definition]));
            Ok(self.flag("brain_definition").then_some(2))
        }
        fn construct_brain(
            &mut self,
            _: &mut Runtime,
            factory: usize,
            entity: usize,
            action_class: usize,
        ) -> Result<Option<usize>> {
            self.trace
                .push(json!(["brain_construct", factory, entity, action_class]));
            Ok(self.flag("brain").then_some(20))
        }
        fn initialize_brain(
            &mut self,
            runtime: &mut Runtime,
            brain: usize,
            definition: usize,
        ) -> Result<bool> {
            let current=self.record_mut(runtime,10)?.brain;
            self.trace
                .push(json!(["brain_initialize", brain, definition, current]));
            if self.case.as_object().unwrap().contains_key("replacement") {
                self.record_mut(runtime,10)?.brain = self.case["replacement"].as_u64().map(|v| v as usize);
            }
            Ok(self.flag("brain_initialize"))
        }
        fn destroy_brain(&mut self, _: &mut Runtime, brain: usize) -> Result<()> {
            self.trace.push(json!(["brain_destroy", brain]));
            Ok(())
        }
        fn destroy_entity(&mut self, runtime: &mut Runtime, entity: usize) -> Result<()> {
            let record = if self.case["inline_storage"] == true {
                self.inline_entity.take().context("Destroyed inline entity was not constructed")?.common
            } else {runtime.records.remove(&entity).context("Destroyed entity was not constructed")?};
            self.trace
                .push(json!(["entity_destroy", entity, record.brain]));
            Ok(())
        }
        fn register_pool(&mut self, _: &mut Runtime, entity: usize) -> Result<Option<usize>> {
            self.trace.push(json!(["pool_register", entity]));
            Ok(self.flag("pool_node").then_some(900))
        }
    }
    #[test]
    fn navigation_factory_gates_handoff_cleanup_and_final_fields_match_original() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_factory.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
          for inline in [false,true] {
            let mut input=expected["input"].clone();
            input["inline_storage"]=json!(inline);
            let c = &input;
            let mut runtime = Runtime {
                generated_name: c["serial"].as_i64().unwrap_or(0) as i32,
                ..Default::default()
            };
            let mut host = Binding {
                case: c,
                trace: vec![],
                constructed: false,
                world_reads: 0,
                inline_entity: None,
            };
            let name = if c.as_object().unwrap().contains_key("name") {
                c["name"].as_str().map(str::as_bytes)
            } else {
                Some(b"NPC".as_slice())
            };
            let definition = host.flag("definition").then_some(1);
            let result = runtime.create(
                name,
                definition,
                c["flags"].as_u64().unwrap_or(0x7e000001) as u32,
                Some(7),
                &mut host,
            )?;
            assert_eq!(json!(result), expected["result"], "result: {c}");
            assert_eq!(json!(host.trace), expected["trace"], "trace: {c}");
            assert_eq!(
                json!(if inline {host.inline_entity.as_ref().map(|e|&e.common)} else {runtime.records.get(&10)}),
                expected["record"],
                "record: {c}"
            );
            assert_eq!(
                json!(runtime.generated_name),
                expected["serial"],
                "serial: {c}"
            );
            if inline {assert!(runtime.records.is_empty(),"Owned entity duplicated its common record");}
          }
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct Runtime {
    pub records: BTreeMap<usize, Record>,
    pub generated_name: i32,
}
pub trait Host {
    /// Access the common fields in the actual constructed entity. The default
    /// store supports standalone factory users; an owned-entity host overrides
    /// this to borrow its inline common record, avoiding two divergent copies.
    fn record_mut<'a>(&'a mut self, runtime: &'a mut Runtime, entity: usize) -> Result<&'a mut Record> {
        runtime.records.get_mut(&entity).context("Navigation factory lost its concrete entity record")
    }
    fn contains_entity(&self, runtime: &Runtime, entity: usize) -> bool {
        runtime.records.contains_key(&entity)
    }
    fn enabled_45e6bc(&mut self, runtime: &mut Runtime) -> Result<bool>;
    fn world_context(&mut self, runtime: &mut Runtime) -> Result<Option<usize>>;
    fn pool_has_free(&mut self, runtime: &mut Runtime) -> Result<bool>;
    fn class_name(
        &mut self,
        runtime: &mut Runtime,
        node: usize,
        key: &str,
    ) -> Result<Option<Vec<u8>>>;
    fn class_metadata(
        &mut self,
        runtime: &mut Runtime,
        kind: Kind,
        name: Option<&[u8]>,
    ) -> Result<Option<usize>>;
    fn class_factory(
        &mut self,
        runtime: &mut Runtime,
        kind: Kind,
        metadata: usize,
    ) -> Result<Option<usize>>;
    /// Must construct a real entity and install its common record in runtime.
    fn construct_entity(
        &mut self,
        runtime: &mut Runtime,
        factory: usize,
        name: &[u8],
        user: Option<usize>,
    ) -> Result<Option<usize>>;
    fn initialize_entity(
        &mut self,
        runtime: &mut Runtime,
        entity: usize,
        definition: usize,
    ) -> Result<bool>;
    fn brain_definition(
        &mut self,
        runtime: &mut Runtime,
        definition: usize,
    ) -> Result<Option<usize>>;
    fn construct_brain(
        &mut self,
        runtime: &mut Runtime,
        factory: usize,
        entity: usize,
        action_class: usize,
    ) -> Result<Option<usize>>;
    fn initialize_brain(
        &mut self,
        runtime: &mut Runtime,
        brain: usize,
        definition: usize,
    ) -> Result<bool>;
    fn destroy_brain(&mut self, runtime: &mut Runtime, brain: usize) -> Result<()>;
    /// Must destroy concrete resources and remove their common record.
    fn destroy_entity(&mut self, runtime: &mut Runtime, entity: usize) -> Result<()>;
    fn register_pool(&mut self, runtime: &mut Runtime, entity: usize) -> Result<Option<usize>>;
}
fn c_string(value: &[u8]) -> &[u8] {
    &value[..value.iter().position(|&b| b == 0).unwrap_or(value.len())]
}
impl Runtime {
    fn discard(&mut self, entity: usize, host: &mut impl Host) -> Result<()> {
        host.destroy_entity(self, entity)?;
        ensure!(
            !host.contains_entity(self, entity),
            "Navigation destructor retained its destroyed entity record"
        );
        Ok(())
    }
    pub fn create(
        &mut self,
        name: Option<&[u8]>,
        definition: Option<usize>,
        flags: u32,
        user: Option<usize>,
        host: &mut impl Host,
    ) -> Result<Option<usize>> {
        if !host.enabled_45e6bc(self)? {
            return Ok(None);
        }
        if host.world_context(self)?.is_none() {
            return Ok(None);
        }
        let Some(definition) = definition else {
            return Ok(None);
        };
        if !host.pool_has_free(self)? {
            return Ok(None);
        }
        let name = if let Some(name) = name {
            let name = c_string(name);
            if name.len() > 0x3ff {
                return Ok(None);
            }
            name.to_vec()
        } else {
            let name = format!("_{}", self.generated_name).into_bytes();
            self.generated_name = self.generated_name.wrapping_add(1);
            name
        };
        let class = host.class_name(self, definition, "Class")?;
        let Some(metadata) =
            host.class_metadata(self, Kind::Entity, class.as_deref().map(c_string))?
        else {
            return Ok(None);
        };
        let Some(factory) = host.class_factory(self, Kind::Entity, metadata)? else {
            return Ok(None);
        };
        let entity = host.construct_entity(self, factory, &name, user)?;
        //457948 re-reads the live context after construction.
        if host.world_context(self)?.is_none() {
            if let Some(entity) = entity {
                self.discard(entity, host)?;
            }
            return Ok(None);
        }
        let entity =
            entity.context("Navigation entity allocation returned null with a live World")?;
        host.record_mut(self, entity)?.word_28 = 0;
        //457988 reads again after the admission check and word28 write. It
        //stores even a newly null context; it does not repeat admission.
        let world = host.world_context(self)?;
        let record = host.record_mut(self, entity)?;
        record.world = world;
        record.definition = Some(definition);
        if !host.initialize_entity(self, entity, definition)? {
            self.discard(entity, host)?;
            return Ok(None);
        }
        if let Some(brain_definition) = host.brain_definition(self, definition)? {
            let class = host.class_name(self, brain_definition, "Class")?;
            if let Some(metadata) =
                host.class_metadata(self, Kind::Brain, class.as_deref().map(c_string))?
            {
                let action = host.class_name(self, definition, "ActionClass")?;
                let Some(action) =
                    host.class_metadata(self, Kind::Action, action.as_deref().map(c_string))?
                else {
                    self.discard(entity, host)?;
                    return Ok(None);
                };
                let Some(factory) = host.class_factory(self, Kind::Brain, metadata)? else {
                    self.discard(entity, host)?;
                    return Ok(None);
                };
                let brain = host.construct_brain(self, factory, entity, action)?;
                host.record_mut(self, entity)?.brain = brain;
                let Some(brain) = brain else {
                    self.discard(entity, host)?;
                    return Ok(None);
                };
                if !host.initialize_brain(self, brain, brain_definition)? {
                    //The initializer can replace/clear the field. Retail reads
                    //it afresh, destroys it, then explicitly clears the slot.
                    if let Some(brain) = host.record_mut(self, entity)?.brain {
                        host.destroy_brain(self, brain)?;
                    }
                    host.record_mut(self, entity)?.brain = None;
                    self.discard(entity, host)?;
                    return Ok(None);
                }
            }
            //Unknown Brain class follows the original diagnostic/miss path:
            //the already initialized entity can still be returned without it.
        }
        let node = host.register_pool(self, entity)?;
        let record = host.record_mut(self, entity)?;
        record.pool_node = node;
        record.word_14 = 0;
        record.flags = flags;
        record.definition = None;
        Ok(Some(entity))
    }
}
