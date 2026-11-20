//! Apprentice resources/FireSequence admission282e4c; event execution is separate.
//! Lua stays unbound until activation/event/completion services are connected.
use crate::em1_bsq::{Document, Node};
use anyhow::{ensure, Context, Result};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
#[derive(Debug)]
pub enum Condition {
    All(Vec<Self>),
    Any(Vec<Self>),
    State(String, i16, u16),
    Mood(String),
}
pub trait ContextOwner {
    fn global(&mut self, name: &str) -> Result<Option<i16>>;
    fn mood(&mut self, entity: usize, name: &str) -> Result<bool>;
}
fn text<'a>(n: &'a Node, k: &str) -> Result<&'a str> {
    n.value(k)
        .and_then(|v| v.as_str())
        .with_context(|| format!("BSQ lacks string {k}"))
}
fn short(n: &Node, k: &str) -> Result<u16> {
    Ok(u16::try_from(
        n.value(k)
            .and_then(|v| v.as_u64())
            .with_context(|| format!("BSQ lacks short {k}"))?,
    )?)
}
fn children<'a>(n: &'a Node, k: &str) -> Result<&'a [Node]> {
    let Node::Record { fields, .. } = n else {
        anyhow::bail!("BSQ record missing")
    };
    fields
        .iter()
        .find_map(|v| match v {
            Node::List { name, items, .. } if name == k => Some(items.as_slice()),
            _ => None,
        })
        .with_context(|| format!("BSQ lacks list {k}"))
}
impl Condition {
    pub fn load(n: &Node) -> Result<Self> {
        Ok(match short(n, "_type")? {
            0 => Self::All(
                children(n, "Conditions")?
                    .iter()
                    .map(Self::load)
                    .collect::<Result<_>>()?,
            ),
            1 => Self::Any(
                children(n, "Conditions")?
                    .iter()
                    .map(Self::load)
                    .collect::<Result<_>>()?,
            ),
            2 => Self::State(
                text(n, "Name")?.into(),
                i16::try_from(
                    n.value("Value")
                        .and_then(|v| v.as_i64())
                        .context("BSQ state value missing")?,
                )?,
                short(n, "Comparison")?,
            ),
            3 => Self::Mood(text(n, "Mood")?.into()),
            tag => anyhow::bail!("Retail BSQ condition factory280114 does not construct tag {tag}"),
        })
    }
    pub fn evaluate(&self, entity: usize, c: &mut impl ContextOwner) -> Result<bool> {
        match self {
            Self::All(children) => {
                for child in children {
                    if !child.evaluate(entity, c)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Self::Any(children) => {
                for child in children {
                    if child.evaluate(entity, c)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Self::State(name, value, op) => Ok(c
                .global(name)?
                .is_some_and(|actual| compare(actual, *value, *op))),
            Self::Mood(name) => c.mood(entity, name),
        }
    }
}
///28087c: signed16, unknown comparisons decline; missing globals are NOT zero.
fn compare(actual: i16, expected: i16, op: u16) -> bool {
    match op {
        0 => actual == expected,
        1 => actual != expected,
        2 => actual > expected,
        3 => actual >= expected,
        4 => actual < expected,
        5 => actual <= expected,
        _ => false,
    }
}
pub struct Resource {
    pub path: String,
    pub key: String,
    pub name: String,
    pub flags: u16,
    pub priority: u16,
    pub condition: Condition,
    pub played_key: String,
    pub event_count: usize,
    pub bytes: Arc<[u8]>,
}
impl Resource {
    pub fn load(path: &str, bytes: &[u8]) -> Result<Self> {
        let doc = Document::load(path, bytes)?;
        ensure!(!doc.database, "Database is not executable BSQ");
        let Node::Record { fields, .. } = &doc.root else {
            anyhow::bail!("BSQ root missing")
        };
        let seq = fields
            .iter()
            .find(|n| n.name() == "Sequence")
            .context("BSQ sequence missing")?;
        let Node::Record { fields, .. } = seq else {
            anyhow::bail!("BSQ sequence record missing")
        };
        let condition = fields
            .iter()
            .find(|n| matches!(n, Node::Record { .. }))
            .context("BSQ root condition missing")?;
        let name = text(seq, "Name")?.to_owned();
        //27a448:128-byte C buffer, copy127 then append only available suffix.
        let mut played = name.as_bytes()[..name.len().min(127)].to_vec();
        played.extend_from_slice(&b"_Played"[..(127 - played.len()).min(7)]);
        Ok(Self {
            path: path.into(),
            key: resource_key(path)?.into(),
            name,
            flags: short(seq, "Flags")?,
            priority: short(seq, "Priority")?,
            condition: Condition::load(condition)?,
            played_key: String::from_utf8(played).context("BSQ played key splits UTF8")?,
            event_count: children(seq, "Events")?.len(),
            bytes: Arc::from(bytes),
        })
    }
    fn eligible(&self, entity: usize, c: &mut impl ContextOwner) -> Result<bool> {
        if self.flags & 4 != 0 && c.global(&self.played_key)?.is_some_and(|v| v != 0) {
            return Ok(false);
        }
        self.condition.evaluate(entity, c)
    }
}
///282a9c: prefer last backslash if any; otherwise last slash. GetSubstring
///ends at the FIRST dot in the original path. Bare names fail native loading.
fn resource_key(path: &str) -> Result<&str> {
    let start = path
        .rfind('\\')
        .or_else(|| path.rfind('/'))
        .context("BSQ resource path has no folder separator")?
        + 1;
    let end = path.find('.').unwrap_or(path.len());
    ensure!(
        start < end && end <= path.len(),
        "BSQ resource filename range is invalid"
    );
    Ok(&path[start..end])
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Request {
    pub entity: usize,
    pub resource: usize,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    Unregistered,
    Active,
    Ineligible,
    QueuedAlready,
    Queued,
}
#[derive(serde::Serialize)]
pub struct Unresolved {
    pub entity: usize,
    pub path: String,
    pub reason: String,
}
#[derive(Default)]
pub struct State {
    pub resources: Vec<Resource>,
    names: BTreeMap<String, usize>,
    registrations: BTreeMap<usize, Vec<usize>>,
    pub queue: VecDeque<Request>,
    pub instances: super::story_instance::Pool,
    pub unresolved: Vec<Unresolved>,
}
impl State {
    #[cfg(debug_assertions)]
    pub(super) fn registered_resources(&self,entity:usize)->&[usize] {
        self.registrations.get(&entity).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn load(
        entities: &[super::assets::RuntimeEntity],
        mut source: impl FnMut(&str) -> Result<Option<(String, Vec<u8>)>>,
    ) -> Result<Self> {
        let mut state = Self::default();
        state.admit_range(entities,0..entities.len(),source)?;
        Ok(state)
    }
    /// Append component registrations without replacing cached resource IDs,
    /// queued requests or live event history owned by the world.
    pub(super) fn admit_range(
        &mut self, entities:&[super::assets::RuntimeEntity],range:std::ops::Range<usize>,
        mut source:impl FnMut(&str)->Result<Option<(String,Vec<u8>)>>,
    )->Result<()> {
        ensure!(range.start<=range.end && range.end<=entities.len(),"Invalid conversation component range");
        for (entity, owner) in entities.iter().enumerate().skip(range.start).take(range.end-range.start) {
            for c in owner
                .components
                .iter()
                .filter(|c| c.class_name == "ConversationComponent")
            {
                let value = &c
                    .properties
                    .iter()
                    .find(|p| p.name == "Sequences")
                    .context("Conversation Sequences missing")?
                    .value;
                for path in super::actor_scripts::authored_paths(value)? {
                    self.unresolved.retain(|u|u.entity!=entity || u.path!=path);
                    let key = match resource_key(&path) {
                        Ok(key) => key.to_owned(),
                        Err(error) => {
                            self.unresolved.push(Unresolved {
                                entity,
                                path,
                                reason: format!("Native filename derivation declined: {error:#}"),
                            });
                            continue;
                        }
                    };
                    let id = if let Some(&id) = self.names.get(&key) {
                        Some(id)
                    } else {
                        let id = if let Some((path, bytes)) = source(&path).with_context(|| {
                            format!("Conversation {} resource {path}", owner.name)
                        })? {
                            let mut r = Resource::load(&path, &bytes)
                                .with_context(|| format!("BSQ {path}"))?;
                            r.key = key.clone();
                            let id = self.resources.len();
                            self.names.insert(key, id);
                            self.resources.push(r);
                            Some(id)
                        } else {
                            None
                        };
                        id
                    };
                    if let Some(id) = id {
                        self.register(entity, id);
                    } else {
                        self.unresolved.push(Unresolved {
                            entity,
                            path,
                            reason: "Resource absent from loaded assets".into(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
    fn register(&mut self, entity: usize, id: usize) {
        let ids = self.registrations.entry(entity).or_default();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    fn has_active(&self, entity: usize, key: &str) -> Result<bool> {
        //2822e4 resolves active instance ROOT +27ab18 internal Name. It does
        //not compare the manager filename key/resource pointer identities.
        for instance in self
            .instances
            .slots
            .iter()
            .filter(|i| i.active && i.owner == Some(entity))
        {
            let resource = self
                .resources
                .get(instance.root()?)
                .context("Active story root resource missing")?;
            if resource.name == key {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn fire(
        &mut self,
        entity: usize,
        name: &str,
        c: &mut impl ContextOwner,
    ) -> Result<Admission> {
        self.fire_with_active(entity, name, c, false)
    }
    /// The functional presentation scheduler owns its live playback outside
    /// the native instance pool. Feed that activity into the same admission
    /// gates rather than letting a playing sequence queue itself again.
    pub(super) fn fire_with_active(
        &mut self,
        entity: usize,
        name: &str,
        c: &mut impl ContextOwner,
        presentation_active: bool,
    ) -> Result<Admission> {
        let Some(&resource) = self.names.get(name) else {
            return Ok(Admission::Unregistered);
        };
        if !self
            .registrations
            .get(&entity)
            .is_some_and(|ids| ids.contains(&resource))
        {
            return Ok(Admission::Unregistered);
        }
        let request = Request { entity, resource };
        if presentation_active || self.has_active(entity, name)? {
            return Ok(Admission::Active);
        }
        if !self.resources[resource].eligible(entity, c)? {
            return Ok(Admission::Ineligible);
        }
        //Queued duplicates are tested AFTER conditions, not at the entry.
        if self.queue.contains(&request) {
            return Ok(Admission::QueuedAlready);
        }
        if presentation_active || self.has_active(entity, name)? {
            return Ok(Admission::Active);
        }
        if self.resources[resource].priority > 1 {
            self.queue.push_front(request);
        } else {
            self.queue.push_back(request);
        }
        Ok(Admission::Queued)
    }
    /// ForceSequence callback2833b4 ->282fb0 retains registration, queue,
    /// active-instance and priority rules, but skips eligibility evaluation.
    pub(super) fn force_with_active(
        &mut self, entity: usize, name: &str, presentation_active: bool,
    ) -> Result<Admission> {
        let Some(&resource) = self.names.get(name) else { return Ok(Admission::Unregistered) };
        if !self.registrations.get(&entity).is_some_and(|ids| ids.contains(&resource)) {
            return Ok(Admission::Unregistered);
        }
        let request = Request { entity, resource };
        // Native282290 (queued) precedes2822e4 (active) on the force path.
        if self.queue.contains(&request) { return Ok(Admission::QueuedAlready); }
        if presentation_active || self.has_active(entity, name)? { return Ok(Admission::Active); }
        if self.resources[resource].priority > 1 { self.queue.push_front(request); }
        else { self.queue.push_back(request); }
        Ok(Admission::Queued)
    }
    pub(super) fn remove_owner(&mut self, entity: usize) {
        self.queue.retain(|request| request.entity != entity);
        self.registrations.remove(&entity);
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"resources":self.resources.len(),"registered_entities":self.registrations.len(),
        "resource_bytes":self.resources.iter().map(|r|r.bytes.len()).sum::<usize>(),"authored_events":self.resources.iter().map(|r|r.event_count).sum::<usize>(),
        "queued":self.queue,"instances":self.instances.slots,"unresolved_declarations":self.unresolved,"event_scheduler_connected":false,"lua_fire_sequence_bound":false})
    }
}
impl super::assets::World {
    pub(super) fn request_demo_conversation(&mut self, entity: usize) -> Result<()> {
        let _ = self.request_conversation(entity)?;
        Ok(())
    }
    pub(super) fn request_conversation(&mut self, entity: usize) -> Result<Admission> {
        let names = self
            .story_sequences
            .registrations
            .get(&entity)
            .into_iter()
            .flatten()
            .map(|&id| self.story_sequences.resources[id].key.clone())
            .collect::<Vec<_>>();
        let mut first = Admission::Unregistered;
        for name in names {
            let admission = self.request_story_sequence(entity, &name)?;
            if matches!(
                admission,
                Admission::Queued | Admission::Active | Admission::QueuedAlready
            ) {
                return Ok(admission);
            }
            if first == Admission::Unregistered {
                first = admission;
            }
        }
        Ok(first)
    }
    pub(super) fn request_story_sequence(
        &mut self,
        entity: usize,
        name: &str,
    ) -> Result<Admission> {
        struct Live<'a>(&'a super::global_state::State);
        impl ContextOwner for Live<'_> {
            fn global(&mut self, name: &str) -> Result<Option<i16>> {
                Ok(self.0.get(name))
            }
            fn mood(&mut self, _: usize, name: &str) -> Result<bool> {
                anyhow::bail!("BSQ mood {name} needs its actor CharacterState owner")
            }
        }
        let active = self.demo_story.current_request.is_some_and(|request| {
            request.entity == entity
                && self.story_sequences.resources[request.resource].name == name
        });
        self.story_sequences
            .fire_with_active(entity, name, &mut Live(&self.globals), active)
    }
    pub(super) fn force_story_sequence(&mut self, entity: usize, name: &str) -> Result<Admission> {
        let active = self.demo_story.current_request.is_some_and(|request| {
            request.entity == entity && self.story_sequences.resources[request.resource].name == name
        });
        self.story_sequences.force_with_active(entity, name, active)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Host {
        values: BTreeMap<String, i16>,
        calls: Vec<String>,
        fail: Option<usize>,
        answers: Vec<bool>,
    }
    impl ContextOwner for Host {
        fn global(&mut self, k: &str) -> Result<Option<i16>> {
            self.calls.push(k.into());
            Ok(self.values.get(k).copied())
        }
        fn mood(&mut self, _: usize, k: &str) -> Result<bool> {
            let i = k.parse::<usize>()?;
            self.calls.push(k.into());
            ensure!(self.fail != Some(i), "Supplied child failure");
            Ok(self.answers[i])
        }
    }
    fn host() -> Host {
        Host {
            values: Default::default(),
            calls: vec![],
            fail: None,
            answers: vec![],
        }
    }
    #[test]
    fn resource_identity_uses_filename_first_load_and_unique_entity_registration() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{Component, Property, ID};
        let entity = |paths: Vec<&str>| super::super::assets::RuntimeEntity {
            authored_scale: 1.,
            authored_rotation: [0.; 9],
            runtime_rotation: [0.; 9],
            model_asset: None,
            mesh_draws: Default::default(),
            jigsaw_asset: None,
            jigsaw_states: vec![],
            scene: "test".into(),
            id: 0,
            name: "test".into(),
            transform: [0.; 16],
            authored_transform: [0.; 16],
            draws: 0..0,
            collision_parts: 0..0,
            components: vec![Component::new(
                "ConversationComponent".into(),
                "Conversation".into(),
                ID::new(12),
                ID::new(1),
                ID::new(0),
                vec![Property::new(
                    "String".into(),
                    "Sequences".into(),
                    true,
                    false,
                    false,
                    serde_json::json!(paths),
                )],
            )],
        };
        let entities = [
            entity(vec![
                "One/Shared.bsq",
                "Absent/Missing.bsq",
                "Two/Shared.bsq",
                "One/Shared.bsq",
                "Broken/.bsq",
                "NoFolder.bsq",
            ]),
            entity(vec!["Another/Shared.bsq"]),
        ];
        let bytes = Document::new_sequence()?.save()?;
        let mut reads = vec![];
        let s = State::load(&entities, |path| {
            reads.push(path.to_owned());
            match path {
                "One/Shared.bsq" => Ok(Some((path.into(), bytes.clone()))),
                "Absent/Missing.bsq" => Ok(None),
                _ => anyhow::bail!("Cached filename should prevent this load"),
            }
        })?;
        assert_eq!(reads, ["One/Shared.bsq", "Absent/Missing.bsq"]);
        assert_eq!(s.resources.len(), 1);
        assert_eq!(s.resources[0].key, "Shared");
        assert_eq!(s.resources[0].name, "");
        assert_eq!(s.registrations[&0], [0]);
        assert_eq!(s.registrations[&1], [0]);
        assert_eq!(s.unresolved.len(), 3);
        assert_eq!(resource_key("Root\\Dir/Sub/Shared.bsq")?, "Dir/Sub/Shared");
        assert!(resource_key("Shared.bsq").is_err());
        assert!(resource_key("Root/.bsq").is_err());
        Ok(())
    }
    #[test]
    fn conditions_match_original_integer_and_short_circuit_instructions() -> Result<()> {
        let d: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/story_sequence_conditions.json"
        ))?;
        for c in d["comparisons"].as_array().unwrap() {
            assert_eq!(
                compare(
                    c["actual"].as_i64().unwrap() as i16,
                    c["expected"].as_i64().unwrap() as i16,
                    c["operator"].as_u64().unwrap() as u16
                ),
                c["result"].as_bool().unwrap(),
                "{c}"
            );
        }
        for c in d["compound"].as_array().unwrap() {
            let mut h = host();
            h.answers = serde_json::from_value(c["values"].clone())?;
            h.fail = c["fail"].as_u64().map(|v| v as usize);
            let children = (0..h.answers.len())
                .map(|i| Condition::Mood(i.to_string()))
                .collect();
            let condition = if c["kind"] == "all" {
                Condition::All(children)
            } else {
                Condition::Any(children)
            };
            let result = condition.evaluate(123, &mut h);
            assert_eq!(result.is_err(), c["stopped"].as_bool().unwrap(), "{c}");
            if let Ok(value) = result {
                assert_eq!(value, c["result"].as_bool().unwrap(), "{c}");
            }
            assert_eq!(
                h.calls,
                c["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>(),
                "{c}"
            );
        }
        let mut h = host();
        assert!(!Condition::State("absent".into(), 0, 0).evaluate(1, &mut h)?);
        Ok(())
    }
    fn resource(name: &str, priority: u16, flags: u16) -> Resource {
        Resource {
            path: format!("Test/{name}.bsq"),
            key: name.into(),
            name: name.into(),
            priority,
            flags,
            played_key: format!("{name}_Played"),
            condition: Condition::All(vec![]),
            event_count: 0,
            bytes: Arc::from([]),
        }
    }
    #[test]
    fn request_admission_preserves_native_gates_duplicates_and_priority_partition() -> Result<()> {
        let mut s = State::default();
        s.resources = vec![
            resource("A", 1, 4),
            resource("B", 2, 0),
            resource("C", 9, 0),
        ];
        for (i, r) in s.resources.iter().enumerate() {
            s.names.insert(r.name.clone(), i);
        }
        s.registrations.insert(7, vec![0, 1, 2, 0]);
        let mut h = host();
        assert_eq!(s.fire(8, "A", &mut h)?, Admission::Unregistered);
        assert!(h.calls.is_empty());
        h.values.insert("A_Played".into(), -1);
        assert_eq!(s.fire(7, "A", &mut h)?, Admission::Ineligible);
        assert!(s.queue.is_empty());
        assert_eq!(s.force_with_active(7, "A", false)?, Admission::Queued);
        assert_eq!(s.force_with_active(7, "A", true)?, Admission::QueuedAlready);
        s.queue.clear();
        assert_eq!(s.force_with_active(7, "A", true)?, Admission::Active);
        assert_eq!(s.force_with_active(8, "A", false)?, Admission::Unregistered);
        h.values.insert("A_Played".into(), 0);
        assert_eq!(s.fire(7, "A", &mut h)?, Admission::Queued);
        let n = h.calls.len();
        assert_eq!(s.fire(7, "A", &mut h)?, Admission::QueuedAlready);
        assert_eq!(
            h.calls.len(),
            n + 1,
            "Queued duplicates must still re-evaluate eligibility"
        );
        s.instances
            .slots
            .push(super::super::story_instance::Instance {
                owner: Some(7),
                sequence: Some(0),
                active: true,
                ..Default::default()
            });
        let n = h.calls.len();
        assert_eq!(s.fire(7, "A", &mut h)?, Admission::Active);
        assert_eq!(h.calls.len(), n);
        assert_eq!(s.fire(7, "B", &mut h)?, Admission::Queued);
        assert_eq!(s.fire(7, "C", &mut h)?, Admission::Queued);
        assert_eq!(
            s.queue.iter().map(|r| r.resource).collect::<Vec<_>>(),
            [2, 1, 0]
        );
        assert_eq!(
            s.fire(7, "a", &mut h)?,
            Admission::Unregistered,
            "Interned sequence names are case sensitive"
        );
        s.queue.clear();
        s.resources[0].name = "DifferentInternalName".into();
        assert_eq!(
            s.fire(7, "A", &mut h)?,
            Admission::Queued,
            "Active duplicate detection uses internal root Name, not filename key"
        );
        s.instances.slots[0].stack = vec![1];
        assert_eq!(
            s.fire(7, "B", &mut h)?,
            Admission::Active,
            "Active root accessor reads first stack entry"
        );
        Ok(())
    }
    #[test]
    fn live_presentation_activity_and_retired_owners_obey_admission_gates() -> Result<()> {
        let mut s = State::default();
        s.resources = vec![resource("A", 1, 4)];
        s.names.insert("A".into(), 0);
        s.registrations.insert(7, vec![0]);
        s.registrations.insert(8, vec![0]);
        let mut h = host();
        h.values.insert("A_Played".into(), 1);
        assert_eq!(s.fire_with_active(7, "A", &mut h, true)?, Admission::Active);
        assert!(
            h.calls.is_empty(),
            "An already playing sequence re-evaluated or queued itself"
        );
        assert!(s.queue.is_empty());
        assert_eq!(
            s.fire_with_active(9, "A", &mut h, true)?,
            Admission::Unregistered
        );
        h.values.insert("A_Played".into(), 0);
        assert_eq!(s.fire(7, "A", &mut h)?, Admission::Queued);
        assert_eq!(s.fire(8, "A", &mut h)?, Admission::Queued);
        s.remove_owner(7);
        assert_eq!(s.queue.iter().map(|r| r.entity).collect::<Vec<_>>(), [8]);
        assert_eq!(s.fire(7, "A", &mut h)?, Admission::Unregistered);
        Ok(())
    }
    #[test]
    fn played_global_key_preserves_native_fixed_buffer_truncation() -> Result<()> {
        for length in [0, 1, 120, 123, 126, 127, 128, 200] {
            let mut d = Document::new_sequence()?;
            let Node::Record { fields, .. } = &mut d.root else {
                unreachable!()
            };
            let Node::Record { fields, .. } =
                fields.iter_mut().find(|n| n.name() == "Sequence").unwrap()
            else {
                unreachable!()
            };
            let Node::Field { value, .. } = fields.iter_mut().find(|n| n.name() == "Name").unwrap()
            else {
                unreachable!()
            };
            *value = serde_json::json!("x".repeat(length));
            let r = Resource::load("Test/test.bsq", &d.save()?)?;
            let expected = format!("{}_Played", "x".repeat(length.min(127)));
            assert_eq!(r.played_key, &expected[..expected.len().min(127)]);
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires immutable original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_retail_bsq_resources_decode_native_conditions_without_retaining_trees() -> Result<()>
    {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut archives = std::fs::read_dir(root.join("packfiles"))?
            .map(|e| e.map(|v| v.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        archives.retain(|p| p.extension().is_some_and(|v| v.eq_ignore_ascii_case("pak")));
        archives.sort();
        let mut seen = std::collections::BTreeSet::new();
        let mut count = 0;
        let mut events = 0;
        for archive in archives {
            let mut wanted = std::collections::BTreeSet::new();
            for (i, f) in crate::pack_io::index(&archive)?.iter().enumerate() {
                let key = f.path.replace('\\', "/").to_ascii_lowercase();
                if key.ends_with(".bsq") && !key.contains("/globalstate/") && seen.insert(key) {
                    wanted.insert(i);
                }
            }
            if wanted.is_empty() {
                continue;
            }
            let files = crate::pack_io::read_filtered(std::fs::read(&archive)?, |i, _| {
                wanted.contains(&i)
            })?
            .pak
            .files;
            for (_, f) in files.iter().enumerate().filter(|(i, _)| wanted.contains(i)) {
                let r = Resource::load(&f.path, &f.data)?;
                assert_eq!(&*r.bytes, &f.data);
                assert!(r.played_key.len() <= 127);
                events += r.event_count;
                count += 1;
            }
        }
        assert!(count >= 1600);
        println!("Decoded {count} BSQ resources and {events} root events");
        Ok(())
    }
    #[test]
    #[ignore = "requires immutable original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_registers_owned_sequences_and_uses_live_global_gates() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let s = &world.story_sequences;
        assert!(!s.resources.is_empty());
        assert!(!s.registrations.is_empty());
        let (owner,resource)=s.registrations.iter().flat_map(|(&owner,ids)|ids.iter().map(move|&id|(owner,id)))
            .find(|&(_,id)|matches!(&s.resources[id].condition,Condition::All(children) if children.is_empty())).context("Original GV lacks an unconditional test sequence")?;
        let r = &s.resources[resource];
        let name = r.key.clone();
        let played = r.played_key.clone();
        if r.flags & 4 != 0 {
            world.globals.set(&played, 0);
        }
        assert_eq!(
            world.request_story_sequence(owner, &name)?,
            Admission::Queued
        );
        assert_eq!(
            world.request_story_sequence(owner, &name)?,
            Admission::QueuedAlready
        );
        assert_eq!(world.story_sequences.queue.len(), 1);
        assert_eq!(
            world.request_story_sequence(world.player_entity, &name)?,
            Admission::Unregistered
        );
        println!("GV catalog: {}", world.story_sequences.summary());
        Ok(())
    }
}
