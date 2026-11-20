//! hkbAttachmentModifier vtable806b4e78. Own the native inactive lifecycle;
//! physical attachment creation/teardown still requires a body/character host.
use super::behavior::Document;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Handle {
    pub frame: Option<String>,
    pub body: Option<String>,
    pub character: Option<String>,
    pub bone: i16,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Setup {
    pub scalar_bits: [u32; 7],
    pub kind: i8,
}
#[derive(Clone, Debug)]
struct Definition {
    enabled: bool,
    attacher: Option<Handle>,
    attachee: Option<Handle>,
    setup: Option<Setup>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Admission {
    MissingAttacher,
    MissingAttachee,
    InvalidAttacher,
    InvalidAttachee,
    NeedsBodies,
    NeedsCharacters,
    Ready,
}

///80205b08..80205c40. A frame alone is not a valid handle. Setup is not
///dereferenced until BOTH handles resolve to a rigid body or character.
pub fn admission(
    attacher: Option<&Handle>,
    attachee: Option<&Handle>,
    setup: impl FnOnce() -> Result<i8>,
) -> Result<Admission> {
    let Some(a) = attacher else {
        return Ok(Admission::MissingAttacher);
    };
    let Some(b) = attachee else {
        return Ok(Admission::MissingAttachee);
    };
    if a.body.is_none() && a.character.is_none() {
        return Ok(Admission::InvalidAttacher);
    }
    if b.body.is_none() && b.character.is_none() {
        return Ok(Admission::InvalidAttachee);
    }
    let kind = setup()?;
    if matches!(kind, 1 | 2) && (a.body.is_none() || b.body.is_none()) {
        return Ok(Admission::NeedsBodies);
    }
    if kind == 3 && (a.character.is_none() || b.character.is_none()) {
        return Ok(Admission::NeedsCharacters);
    }
    Ok(Admission::Ready)
}
fn pointer(fields: &serde_json::Value, name: &str) -> Result<Option<String>> {
    let value = fields
        .get(name)
        .with_context(|| format!("Missing attachment member {name}"))?;
    if value.is_null() {
        Ok(None)
    } else {
        Ok(Some(
            value["ref"]
                .as_str()
                .with_context(|| format!("Invalid attachment pointer {name}"))?
                .into(),
        ))
    }
}
fn handle(graph: &Document, fields: &serde_json::Value, name: &str) -> Result<Option<Handle>> {
    let Some(id) = pointer(fields, name)? else {
        return Ok(None);
    };
    let object = graph
        .objects
        .get(&id)
        .context("Attachment handle reference is absent")?;
    ensure!(
        object.klass == "hkbHandle",
        "Attachment reference is not an hkbHandle"
    );
    let f = &object.fields;
    Ok(Some(Handle {
        frame: pointer(f, "frame")?,
        body: pointer(f, "rigidBody")?,
        character: pointer(f, "character")?,
        bone: i16::try_from(
            f["animationBoneIndex"]
                .as_i64()
                .context("Missing attachment bone index")?,
        )?,
    }))
}
impl Definition {
    fn load(graph: &Document, id: &str) -> Result<Self> {
        let object = graph
            .objects
            .get(id)
            .context("Missing attachment modifier")?;
        ensure!(
            object.klass == "hkbAttachmentModifier",
            "Invalid attachment modifier class"
        );
        let f = &object.fields;
        ensure!(
            f.get("variableBindingSet")
                .context("Missing attachment binding field")?
                .is_null(),
            "Attachment modifier bound handle/scalar assignment is not implemented"
        );
        let setup = pointer(f, "attachmentSetup")?
            .map(|id| -> Result<Setup> {
                let object = graph.objects.get(&id).context("Missing attachment setup")?;
                ensure!(
                    object.klass == "hkbAttachmentSetup",
                    "Invalid attachment setup class"
                );
                let mut scalar_bits = [0; 7];
                for (slot, name) in scalar_bits.iter_mut().zip([
                    "blendInTime",
                    "moveAttacherFraction",
                    "gain",
                    "extrapolationTimeStep",
                    "fixUpGain",
                    "maxLinearDistance",
                    "maxAngularDistance",
                ]) {
                    *slot = (object.fields[name]
                        .as_f64()
                        .with_context(|| format!("Missing attachment setup {name}"))?
                        as f32)
                        .to_bits();
                }
                Ok(Setup {
                    scalar_bits,
                    kind: i8::try_from(
                        object.fields["attachmentType"]
                            .as_i64()
                            .context("Missing attachment type")?,
                    )?,
                })
            })
            .transpose()?;
        Ok(Self {
            enabled: f["enable"].as_bool().context("Missing attachment enable")?,
            attacher: handle(graph, f, "attacherHandle")?,
            attachee: handle(graph, f, "attacheeHandle")?,
            setup,
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Instance {
    pub active: bool,
    pub activations: u64,
    pub modifies: u64,
    pub attempts: u64,
    pub deactivations: u64,
    pub last_admission: Option<Admission>,
    attacher: Option<Handle>,
    attachee: Option<Handle>,
    setup: Option<Setup>,
    enabled: bool,
    cached_body: Option<String>,
    old_filter: u32,
    attachment: Option<u64>,
}
impl Instance {
    fn new(definition: Definition) -> Self {
        Self {
            active: false,
            activations: 0,
            modifies: 0,
            attempts: 0,
            deactivations: 0,
            last_admission: None,
            attacher: definition.attacher,
            attachee: definition.attachee,
            setup: definition.setup,
            enabled: definition.enabled,
            cached_body: None,
            old_filter: 0,
            attachment: None,
        }
    }
    fn attempt(&mut self) -> Result<()> {
        self.attempts = self.attempts.wrapping_add(1);
        let decision = admission(self.attacher.as_ref(), self.attachee.as_ref(), || {
            self.setup
                .as_ref()
                .map(|s| s.kind)
                .context("Valid attachment handles require an attachment setup")
        })?;
        self.last_admission = Some(decision);
        ensure!(decision!=Admission::Ready,
            "Physical Havok attachment requires native body/character/local-frame creation, motion/filter restoration and event delivery; no substitute attachment was created");
        Ok(())
    }
    fn activate(&mut self) -> Result<()> {
        //80205a10 resets only these three runtime words, then attempts attach.
        self.cached_body = None;
        self.old_filter = 0;
        self.attachment = None;
        self.activations = self.activations.wrapping_add(1);
        self.attempt()?;
        // Native wrapper802120a4 writes node-record74 only AFTER the virtual
        // activation returns. Preserve reset fields if an unsupported host
        // aborts, but do not report that node as successfully activated.
        self.active = true;
        Ok(())
    }
    fn modify(&mut self) -> Result<()> {
        //80205a80 itself does NOT read enable. The surrounding wrapper owns
        // that gate, activation and deactivation before reaching this method.
        ensure!(self.active, "Inactive attachment modifier reached output");
        self.modifies = self.modifies.wrapping_add(1);
        if self.attachment.is_none() {
            self.attempt()?;
        }
        Ok(())
    }
    fn deactivate(&mut self) -> Result<()> {
        //80206388 returns immediately for null attachment.80205a3c..a68
        //then releases and clears each copied handle, even if attach declined.
        ensure!(
            self.attachment.is_none(),
            "Physical attachment teardown and restored body/event delivery are not implemented"
        );
        self.attacher = None;
        self.attachee = None;
        self.active = false;
        self.deactivations = self.deactivations.wrapping_add(1);
        Ok(())
    }
}
#[derive(Default)]
pub struct Instances {
    pub instances: BTreeMap<String, Instance>,
}
impl Instances {
    pub fn activate(&mut self, graph: &Document, id: &str) -> Result<()> {
        self.synchronize(graph, id)?;
        Ok(())
    }
    /// Conditional modifier wrappers8021200c(update),80211b80(output),
    ///80211e40(event) own node-record byte74. No input bindings are discarded:
    ///this adapter rejects binding sets until handle/scalar assignment is wired.
    pub fn synchronize(&mut self, graph: &Document, id: &str) -> Result<bool> {
        if !self.instances.contains_key(id) {
            self.instances
                .insert(id.into(), Instance::new(Definition::load(graph, id)?));
        }
        let fields = &graph
            .objects
            .get(id)
            .context("Missing attachment wrapper definition")?
            .fields;
        ensure!(
            fields
                .get("variableBindingSet")
                .context("Missing attachment binding field")?
                .is_null(),
            "Attachment modifier bound handle/scalar assignment is not implemented"
        );
        let enabled = fields["enable"]
            .as_bool()
            .context("Missing attachment wrapper enable")?;
        let instance = self.instances.get_mut(id).unwrap();
        instance.enabled = enabled;
        if enabled {
            if !instance.active {
                instance.activate()?;
            }
        } else if instance.active {
            instance.deactivate()?;
        }
        Ok(enabled)
    }
    pub fn modify(&mut self, graph: &Document, id: &str) -> Result<()> {
        if self.synchronize(graph, id)? {
            self.instances.get_mut(id).unwrap().modify()?;
        }
        Ok(())
    }
    pub fn deactivate(&mut self, id: &str) -> Result<()> {
        let instance = self
            .instances
            .get_mut(id)
            .context("Uninitialized retired attachment modifier")?;
        if instance.active {
            instance.deactivate()?;
        }
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"instances":self.instances,"physical_attachment_backend_implemented":false,
            "runtime_handle_binding_implemented":false})
    }
}

impl super::assets::World {
    pub(super) fn attachment_modifier_summary(&self) -> serde_json::Value {
        serde_json::json!({"player":self.player_graph.attachment_modifiers.summary(),
            "authored":self.authored_havok.attachment_summary(),
            "spawned":self.havok_actors.iter().filter(|(_,(_,actor))|!actor.prepared.attachment_modifiers.instances.is_empty())
                .map(|(&entity,(_,actor))|serde_json::json!({"entity":entity,"state":actor.prepared.attachment_modifiers.summary()})).collect::<Vec<_>>()})
    }
    pub(super) fn attachment_modifier_counts(&self) -> (usize, u64) {
        let start = self.authored_havok.attachment_counts();
        self.player_graph
            .attachment_modifiers
            .instances
            .values()
            .chain(
                self.havok_actors
                    .values()
                    .flat_map(|(_, actor)| actor.prepared.attachment_modifiers.instances.values()),
            )
            .fold(start, |(active, attempts), i| {
                (
                    active + usize::from(i.active),
                    attempts.wrapping_add(i.attempts),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn empty(enabled: bool) -> Definition {
        Definition {
            enabled,
            attacher: None,
            attachee: None,
            setup: None,
        }
    }
    fn h(body: bool, character: bool) -> Handle {
        Handle {
            frame: Some("frame".into()),
            body: body.then(|| "body".into()),
            character: character.then(|| "character".into()),
            bone: -1,
        }
    }
    fn tagged(value: &serde_json::Value) -> Option<Handle> {
        match value.as_str().unwrap() {
            "none" => None,
            "invalid" => Some(h(false, false)),
            "body" => Some(h(true, false)),
            "character" => Some(h(false, true)),
            _ => panic!("Unknown native reference handle tag"),
        }
    }
    #[test]
    fn guard_decisions_match_original_instruction_cases() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/attachment_modifier.json"
        ))?;
        let mut checks = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let a = tagged(&case["a"]);
            let b = tagged(&case["b"]);
            let kind = case["kind"].as_i64().unwrap() as i8;
            if case["command"] == "try" {
                assert_eq!(
                    serde_json::to_value(admission(a.as_ref(), b.as_ref(), || Ok(kind))?)?,
                    case["result"]["decision"]
                );
            } else {
                let mut definition = empty(true);
                definition.attacher = a;
                definition.attachee = b;
                definition.setup = Some(Setup {
                    scalar_bits: [0; 7],
                    kind,
                });
                let mut instance = Instance::new(definition);
                instance.active = true;
                instance.cached_body = Some("stale".into());
                instance.old_filter = 0x12345678;
                if case["command"] == "activate"
                    || (case["command"] == "modify" && case["result"]["attachment"] == 123)
                {
                    instance.attachment = Some(123);
                }
                match case["command"].as_str().unwrap() {
                    "activate" => instance.activate()?,
                    "modify" => instance.modify()?,
                    "deactivate" => instance.deactivate()?,
                    _ => unreachable!(),
                }
                assert_eq!(instance.cached_body.is_some(), case["result"]["body"] != 0);
                assert_eq!(
                    u64::from(instance.old_filter),
                    case["result"]["filter"].as_u64().unwrap()
                );
                assert_eq!(
                    instance.attachment.unwrap_or(0),
                    case["result"]["attachment"].as_u64().unwrap()
                );
                assert_eq!(instance.attacher.is_some(), case["result"]["attacher"] != 0);
                assert_eq!(instance.attachee.is_some(), case["result"]["attachee"] != 0);
                assert_eq!(
                    case["result"]["old_motion"], 154,
                    "Native inactive paths must not touch oldMotionType"
                );
            }
            checks += 1;
        }
        assert_eq!(checks, 69);
        Ok(())
    }
    #[test]
    fn handle_admission_preserves_native_guard_order() -> Result<()> {
        let invalid = h(false, false);
        let body = h(true, false);
        let character = h(false, true);
        for (a, b, expected) in [
            (None, None, Admission::MissingAttacher),
            (Some(&body), None, Admission::MissingAttachee),
            (Some(&invalid), Some(&body), Admission::InvalidAttacher),
            (Some(&body), Some(&invalid), Admission::InvalidAttachee),
        ] {
            assert_eq!(
                admission(a, b, || anyhow::bail!("Setup must not be read"))?,
                expected
            );
        }
        for kind in [1, 2] {
            assert_eq!(
                admission(Some(&body), Some(&character), || Ok(kind))?,
                Admission::NeedsBodies
            );
        }
        assert_eq!(
            admission(Some(&body), Some(&body), || Ok(3))?,
            Admission::NeedsCharacters
        );
        assert_eq!(
            admission(Some(&body), Some(&character), || Ok(0))?,
            Admission::Ready
        );
        assert_eq!(
            admission(Some(&character), Some(&character), || Ok(3))?,
            Admission::Ready
        );
        assert!(admission(Some(&body), Some(&body), || anyhow::bail!("Missing setup")).is_err());
        Ok(())
    }
    #[test]
    fn inactive_attachment_lifecycle_retries_and_clears_handles() -> Result<()> {
        let mut definition = empty(true);
        definition.attachee = Some(h(true, false));
        let mut instance = Instance::new(definition.clone());
        instance.old_filter = 99;
        instance.activate()?;
        assert_eq!(instance.old_filter, 0);
        assert_eq!(instance.last_admission, Some(Admission::MissingAttacher));
        instance.modify()?;
        instance.modify()?;
        assert_eq!(
            (instance.activations, instance.modifies, instance.attempts),
            (1, 2, 3)
        );
        instance.deactivate()?;
        assert!(!instance.active && instance.attachee.is_none());
        instance.activate()?;
        assert!(
            instance.attachee.is_none(),
            "Reactivation must not restore a released definition handle"
        );
        assert!(
            definition.attachee.is_some(),
            "Instance teardown changed its source definition"
        );
        let mut other = Instance::new(definition);
        other.activate()?;
        assert!(other.attachee.is_some());
        assert!(instance.attachment.is_none() && instance.cached_body.is_none());
        Ok(())
    }
    #[test]
    fn enabled_output_and_valid_handle_failures_are_explicit() -> Result<()> {
        let mut graph = Document::default();
        graph.objects.insert("modifier".into(),super::super::behavior::Object{klass:"hkbAttachmentModifier".into(),
            fields:serde_json::json!({"enable":false,"variableBindingSet":null,"attachmentSetup":null,"attacherHandle":null,"attacheeHandle":null})});
        let mut instances = Instances::default();
        instances.activate(&graph, "modifier")?;
        instances.modify(&graph, "modifier")?;
        let instance = &instances.instances["modifier"];
        assert_eq!(
            (
                instance.active,
                instance.activations,
                instance.modifies,
                instance.attempts
            ),
            (false, 0, 0, 0)
        );
        graph.objects.get_mut("modifier").unwrap().fields["enable"] = true.into();
        instances.synchronize(&graph, "modifier")?;
        instances.modify(&graph, "modifier")?;
        assert_eq!(
            (
                instances.instances["modifier"].activations,
                instances.instances["modifier"].attempts
            ),
            (1, 2)
        );
        graph.objects.get_mut("modifier").unwrap().fields["enable"] = false.into();
        instances.synchronize(&graph, "modifier")?;
        assert!(!instances.instances["modifier"].active);
        assert_eq!(instances.instances["modifier"].deactivations, 1);
        graph.objects.get_mut("modifier").unwrap().fields["enable"] = true.into();
        instances.synchronize(&graph, "modifier")?;
        assert_eq!(instances.instances["modifier"].activations, 2);
        graph.objects.get_mut("modifier").unwrap().fields["variableBindingSet"] =
            serde_json::json!({"ref":"unimplemented"});
        assert!(instances.synchronize(&graph, "modifier").is_err());
        let mut definition = empty(true);
        definition.attacher = Some(h(true, false));
        definition.attachee = Some(h(true, false));
        definition.setup = Some(Setup {
            scalar_bits: [0; 7],
            kind: 0,
        });
        let mut instance = Instance::new(definition);
        assert!(
            format!("{:#}", instance.activate().unwrap_err()).contains("Physical Havok attachment")
        );
        assert!(
            !instance.active && instance.attachment.is_none() && instance.cached_body.is_none()
        );
        assert_eq!(instance.last_admission, Some(Admission::Ready));
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gremlin_definitions_retain_setup_and_decline_empty_handles() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let name = "Characters/Gremlin/Behaviors/GremlinBehaviorGraph.hkx_wii";
        let stored = crate::pack_io::read_filtered(
            std::fs::read(root.join("packfiles/gremlinvillage_zonea.pak"))?,
            |_, path| path.eq_ignore_ascii_case(name),
        )?;
        let file = stored
            .pak
            .files
            .iter()
            .find(|file| file.path.eq_ignore_ascii_case(name))
            .context("Missing original Gremlin graph")?;
        let graph = Document::load(&file.data)?;
        let mut instances = Instances::default();
        let mut count = 0;
        for (id, object) in &graph.objects {
            if object.klass != "hkbAttachmentModifier" {
                continue;
            }
            instances.activate(&graph, id)?;
            instances.modify(&graph, id)?;
            let instance = &instances.instances[id];
            assert_eq!(instance.last_admission, Some(Admission::MissingAttacher));
            assert_eq!(
                instance.setup.as_ref().unwrap().scalar_bits[2],
                0.3f32.to_bits()
            );
            assert_eq!(instance.setup.as_ref().unwrap().kind, 0);
            instances.deactivate(id)?;
            assert!(!instances.instances[id].active);
            count += 1;
        }
        assert_eq!(count, 4);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gremlin_cutscene_enters_live_graph_and_owns_attachment_lifecycle() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let owner = world
            .ai_brains
            .iter()
            .find(|(_, brain)| {
                brain.ai_type.starts_with("Gremlin") && !brain.scripted_idle.is_empty()
            })
            .map(|(&owner, _)| owner)
            .context("Original Gremlin ScriptedIdle absent")?;
        let name = world.entities[owner].name.clone();
        world.run_callback_fixture(&format!(
            "DisableComponent({name:?},'Usable'); EnterCutscene({name:?},6)"
        ))?;
        let instances = world.authored_havok.attachment_states(owner)?;
        assert!(
            instances.instances.values().any(|i| i.active
                && i.activations == 1
                && i.last_admission == Some(Admission::MissingAttacher)),
            "Original teleport did not activate an owned attachment modifier"
        );
        let (_, errors) = world.authored_havok.step_and_export(owner)?;
        ensure!(errors.is_empty(), "{errors:?}");
        let instances = world.authored_havok.attachment_states(owner)?;
        assert!(
            instances
                .instances
                .values()
                .any(|i| i.modifies > 0 && i.attempts > 1),
            "Live pose generation did not retry attachment"
        );
        std::fs::write(
            "target/attachment-gremlin-fixture.json",
            serde_json::to_vec_pretty(&serde_json::json!({
            "entity":owner,"name":name,"ai_type":world.ai_brains[&owner].ai_type,"state":instances.summary()}))?,
        )?;
        world.retire_entity(owner)?;
        assert!(!world.authored_havok.contains(owner));
        Ok(())
    }
}
