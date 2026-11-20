//! Reflected Havok behavior assets: Wii Havok 7 and PC Havok 2012.1.
//! This is an asset loader; decoding a generator does not mean we execute it.
use anyhow::{bail, ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

type Address = (usize, usize);

/// Authored dependency contract. Clip presence is reported separately because
/// the default animation list does not load every cinematic in the project.
#[derive(Debug, serde::Serialize)]
pub struct CharacterAssets {
    #[serde(skip)]
    pub(super) reflection: Reflection,
    pub default_character_selected: bool,
    pub property_words: Vec<u32>,
    pub project: String,
    pub character: String,
    pub behavior: String,
    pub rig: String,
    pub rig_bones: usize,
    pub node_counts: BTreeMap<String, usize>,
    pub events: Vec<String>,
    pub variables: Vec<String>,
    pub root_states: BTreeMap<i64, String>,
    pub clips: BTreeMap<String, String>,
    #[serde(skip)]
    pub(super) animation_binding_names:Vec<String>,
    pub clips_outside_character_table: Vec<String>,
    pub unavailable_clips: Vec<String>,
}
fn strings(value: &Value) -> Result<Vec<String>> {
    value
        .as_array()
        .context("Expected behavior string array")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .context("Expected behavior string")
        })
        .collect()
}
fn relative(base: &str, child: &str) -> Result<String> {
    let child = child.replace('\\', "/");
    ensure!(
        !child.starts_with('/') && !child.contains(':') && !child.split('/').any(|s| s == ".."),
        "Unsafe behavior asset path {child}"
    );
    Ok(format!("{base}/{child}"))
}
/// PC ParamsHKXBehavior::SetDefaultCharacterName (9badb0): strip the
/// directory, truncate at lowercase "behavior" (preferred) or "project",
/// then uppercase only the first character. Never choose an arbitrary rig.
pub(super) fn default_character_name(project_path: &str) -> Result<String> {
    let basename = project_path.rsplit(['/', '\\']).next().unwrap_or(project_path);
    ensure!(basename.is_ascii(), "Non-ASCII native default character name needs locale conversion");
    let end = basename.find("behavior").or_else(|| basename.find("project"))
        .context("Behavior path has no native default character-name delimiter")?;
    ensure!(end > 0, "Empty native default character name");
    let mut name = basename[..end].as_bytes().to_vec();
    name[0] = name[0].to_ascii_uppercase();
    Ok(String::from_utf8(name)?)
}

pub fn character_assets<'a>(
    project_path: &str,
    character_name: &str,
    read: impl Fn(&str) -> Option<&'a [u8]>,
) -> Result<(CharacterAssets, Document)> {
    character_assets_with_reflection(project_path, character_name, read, &Reflection::default())
}
pub(super) fn character_assets_with_reflection<'a>(
    project_path: &str,
    character_name: &str,
    read: impl Fn(&str) -> Option<&'a [u8]>,
    inherited: &Reflection,
) -> Result<(CharacterAssets, Document)> {
    let mut reflection = inherited.clone();
    let project_path = project_path.replace('\\', "/");
    let base = project_path
        .rsplit_once('/')
        .context("Behavior project has no asset directory")?
        .0;
    let project = Document::load_with_reflection(
        read(&project_path).context("Missing behavior project")?,
        &reflection,
    )
    .with_context(|| format!("Behavior project {project_path}"))?;
    reflection.merge(&project.reflection)?;
    let project_strings = project.single("hkbProjectStringData")?;
    let mut selected = None;
    for path in strings(&project_strings["characterFilenames"])? {
        let path = relative(base, &path)?;
        let character = Document::load_with_reflection(
            read(&path).with_context(|| format!("Missing character definition {path}"))?,
            &reflection,
        )
        .with_context(|| format!("Behavior character {path}"))?;
        reflection.merge(&character.reflection)?;
        // Shipped paintanvil requests "Anvil", while its character resource
        // names itself "anvil". Preserve ambiguity detection after folding.
        if character_name.is_empty()
            || character.single("hkbCharacterStringData")?["name"]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(character_name))
        {
            ensure!(
                selected.is_none(),
                "Ambiguous behavior character selection (unnamed multi-character policy is not reconstructed)"
            );
            selected = Some((path, character));
        }
    }
    let (character_path, character) =
        selected.with_context(|| format!("Missing behavior character {character_name}"))?;
    let character_strings = character.single("hkbCharacterStringData")?;
    let behavior = relative(
        base,
        character_strings["behaviorFilename"]
            .as_str()
            .context("Character has no behavior")?,
    )?;
    let rig = relative(
        base,
        character_strings["rigName"]
            .as_str()
            .context("Character has no rig")?,
    )?;
    let rig_file =
        HkxFile::load_bytes(read(&rig).with_context(|| format!("Missing character rig {rig}"))?)
            .map_err(anyhow::Error::msg)?;
    let rig_bones = rig_file
        .parse_skeleton()
        .context("Character rig has no skeleton")?
        .bones
        .len();
    let graph = Document::load_with_reflection(
        read(&behavior).with_context(|| format!("Missing character behavior {behavior}"))?,
        &reflection,
    )
    .with_context(|| format!("Behavior graph {behavior}"))?;
    reflection.merge(&graph.reflection)?;
    let character_data = character.single("hkbCharacterData")?;
    let property_values = character_data["characterPropertyValues"]["ref"]
        .as_str()
        .context("Missing character property values")?;
    let values = character
        .objects
        .get(property_values)
        .context("Missing property value object")?
        .fields["wordVariableValues"]
        .as_array()
        .context("Missing property words")?;
    let property_names = strings(&character_strings["characterPropertyNames"])?;
    let graph_property_names =
        strings(&graph.single("hkbBehaviorGraphStringData")?["characterPropertyNames"])?;
    let character_infos = character_data["characterPropertyInfos"]
        .as_array()
        .context("Missing property types")?;
    let graph_infos = graph.single("hkbBehaviorGraphData")?["characterPropertyInfos"]
        .as_array()
        .context("Missing graph property types")?;
    ensure!(
        property_names.len() == values.len()
            && character_infos.len() == values.len()
            && graph_property_names.len() == graph_infos.len(),
        "Character property table lengths differ"
    );
    let mut property_words = Vec::with_capacity(graph_property_names.len());
    for (name, info) in graph_property_names.iter().zip(graph_infos) {
        let matches: Vec<_> = property_names
            .iter()
            .enumerate()
            .filter(|(_, n)| *n == name)
            .collect();
        ensure!(
            matches.len() == 1,
            "Missing or ambiguous character property {name}"
        );
        let index = matches[0].0;
        ensure!(
            character_infos[index]["type"] == info["type"],
            "Character property type mismatch: {name}"
        );
        let value = values[index]["value"]
            .as_i64()
            .context("Invalid property word")?;
        ensure!(
            (i32::MIN as i64..=u32::MAX as i64).contains(&value),
            "Property word overflow"
        );
        property_words.push(value as u32);
    }
    let mut result = CharacterAssets {
        reflection,
        default_character_selected: character_name.is_empty(),
        property_words,
        project: project_path.clone(),
        character: character_path,
        behavior,
        rig,
        rig_bones,
        node_counts: BTreeMap::new(),
        events: vec![],
        variables: vec![],
        root_states: BTreeMap::new(),
        clips: BTreeMap::new(),animation_binding_names:Vec::new(),
        clips_outside_character_table: Vec::new(),
        unavailable_clips: vec![],
    };
    let names = graph.single("hkbBehaviorGraphStringData")?;
    result.events = strings(&names["eventNames"])?;
    result.variables = strings(&names["variableNames"])?;
    let root = graph.single("hkbBehaviorGraph")?["rootGenerator"]["ref"]
        .as_str()
        .context("Missing root generator")?;
    let root = graph.objects.get(root).context("Missing root object")?;
    ensure!(
        root.klass == "hkbStateMachine",
        "Unsupported character root generator {}",
        root.klass
    );
    for state in root.fields["states"]
        .as_array()
        .context("Missing root states")?
    {
        let state = &graph
            .objects
            .get(state["ref"].as_str().context("Invalid state reference")?)
            .context("Missing state")?
            .fields;
        ensure!(
            result
                .root_states
                .insert(
                    state["stateId"].as_i64().context("Invalid state id")?,
                    state["name"].as_str().context("Missing state name")?.into()
                )
                .is_none(),
            "Duplicate root state id"
        );
    }
    let character_animation_names = if character_strings["animationNames"].is_array() {
        strings(&character_strings["animationNames"])?
    } else {
        // Havok 2012 partitions the character's animation name table by
        // bundle. Preserve those authored names; project paths remain the
        // authoritative resource mapping below.
        let mut names = Vec::new();
        for bundle in character_strings["animationBundleNameData"]
            .as_array()
            .context("Missing character animation name bundles")?
        {
            names.extend(strings(&bundle["assetNames"])?);
        }
        names
    };
    result.animation_binding_names=character_animation_names.clone();
    let animation_names: HashSet<_> = character_animation_names
        .into_iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let project_animations = strings(&project_strings["animationFilenames"])?;
    for object in graph.objects.values() {
        *result.node_counts.entry(object.klass.clone()).or_default() += 1;
        if object.klass != "hkbClipGenerator" {
            continue;
        }
        let name = object.fields["animationName"]
            .as_str()
            .context("Clip has no animation name")?;
        // Character string tables can be stale: retail watch omits its tpose
        // while both the graph and project explicitly reference it. Resolve the
        // authored project filename, retaining this discrepancy as metadata.
        if !animation_names.contains(&name.to_ascii_lowercase()) {
            result.clips_outside_character_table.push(name.to_owned());
        }
        let matching: Vec<_> = project_animations
            .iter()
            .filter(|s| {
                s.rsplit(['/', '\\']).next().is_some_and(|s| {
                    name.rsplit(['/', '\\'])
                        .next()
                        .is_some_and(|name| s.eq_ignore_ascii_case(name))
                })
            })
            .collect();
        ensure!(
            matching.len() == 1,
            "Animation {name} has {} project paths",
            matching.len()
        );
        let path = relative(base, matching[0])?;
        if read(&path).is_none() {
            result.unavailable_clips.push(path.clone());
        }
        result.clips.insert(name.into(), path);
    }
    result.unavailable_clips.sort();
    result.unavailable_clips.dedup();
    Ok((result, graph))
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Member {
    name: String,
    class: Option<String>,
    kind: u8,
    subtype: u8,
    count: usize,
    flags: u16,
    offset: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Schema {
    size: usize,
    parent: Option<String>,
    members: Vec<Member>,
}
/// File-local reflection plus an explicitly supplied registry. No global cache:
/// mod/game sessions must not donate incompatible layouts to one another.
#[derive(Clone, Debug, Default)]
pub(super) struct Reflection {
    abi: Option<(String, i32, [u8; 4])>,
    schemas: HashMap<String, Schema>,
    signatures: BTreeMap<String, u32>,
}
impl Reflection {
    fn from_view(view: &View<'_>) -> Result<Self> {
        let file = view.file;
        let mut signatures = BTreeMap::new();
        let section = file
            .sections
            .iter()
            .position(|section| section.section_tag == "__classnames__")
            .context("Missing behavior class-name section")?;
        for name in &file.class_names {
            let offset = name.offset as usize;
            ensure!(
                offset >= 5 && view.bytes((section, offset - 1), 1)? == [9],
                "Invalid class signature location"
            );
            let signature = view.word((section, offset - 5))?;
            if let Some(previous) = signatures.insert(name.name.clone(), signature) {
                ensure!(
                    previous == signature,
                    "Conflicting class signatures for {}",
                    name.name
                );
            }
        }
        Ok(Self {
            abi: Some((
                file.header.contents_version.clone(),
                file.header.file_version,
                [
                    file.header.pointer_size,
                    file.header.endianness,
                    file.header.padding_option,
                    file.header.base_format,
                ],
            )),
            schemas: view.schemas.clone(),
            signatures,
        })
    }
    fn merge(&mut self, other: &Self) -> Result<()> {
        if self.abi.is_none() {
            *self = other.clone();
            return Ok(());
        }
        if other.abi.is_none() {
            return Ok(());
        }
        ensure!(
            self.abi == other.abi,
            "Incompatible behavior reflection ABI/version"
        );
        for (name, signature) in &other.signatures {
            if let Some(previous) = self.signatures.get(name) {
                ensure!(
                    previous == signature,
                    "Behavior reflection signature mismatch for {name}"
                );
            }
        }
        for (name, schema) in &other.schemas {
            if let Some(previous) = self.schemas.get(name) {
                ensure!(
                    previous == schema,
                    "Conflicting behavior reflection layout for {name}"
                );
            }
        }
        self.signatures.extend(other.signatures.clone());
        self.schemas.extend(other.schemas.clone());
        Ok(())
    }
}
#[derive(Debug, serde::Serialize)]
pub struct Object {
    pub klass: String,
    pub fields: Value,
}
#[derive(Debug, Default, serde::Serialize)]
pub struct Document {
    pub objects: BTreeMap<String, Object>,
    #[serde(skip)]
    reflection: Reflection,
}
impl Document {
    pub(super) fn contents_version(&self) -> Option<&str> {
        self.reflection.abi.as_ref().map(|(version,_,_)|version.as_str())
    }
    pub fn condition_programs(&self) -> Result<BTreeMap<String, super::expression::Program>> {
        let names = strings(&self.single("hkbBehaviorGraphStringData")?["variableNames"])?;
        self.objects
            .iter()
            .filter(|(_, object)| object.klass == "hkbExpressionCondition")
            .map(|(id, object)| {
                let source = object.fields["expression"]
                    .as_str()
                    .context("Condition has no expression")?;
                let program = super::expression::Program::compile(source, &names)
                    .with_context(|| format!("Compile behavior condition {id}: {source}"))?;
                Ok((id.clone(), program))
            })
            .collect()
    }
    pub fn single(&self, class: &str) -> Result<&Value> {
        let mut objects = self.objects.values().filter(|o| o.klass == class);
        let object = objects.next().with_context(|| format!("Missing {class}"))?;
        ensure!(objects.next().is_none(), "Ambiguous {class}");
        Ok(&object.fields)
    }
    pub fn load(bytes: &[u8]) -> Result<Self> {
        Self::load_with_reflection(bytes, &Reflection::default())
    }
    fn load_with_reflection(bytes: &[u8], inherited: &Reflection) -> Result<Self> {
        let file = HkxFile::load_bytes(bytes).map_err(anyhow::Error::msg)?;
        let mut view = View::new(&file)?;
        let local = Reflection::from_view(&view)?;
        let mut reflection = inherited.clone();
        reflection.merge(&local)?;
        for class in view
            .objects
            .values()
            .filter(|class| !matches!(class.as_str(), "hkClass" | "hkClassEnum"))
        {
            if !view.schemas.contains_key(class) {
                let signature = local
                    .signatures
                    .get(class)
                    .with_context(|| format!("Missing class signature for {class}"))?;
                ensure!(
                    inherited.signatures.get(class) == Some(signature),
                    "No matching behavior reflection signature for {class}"
                );
                ensure!(
                    inherited.schemas.contains_key(class),
                    "Missing behavior schema {class}"
                );
            }
        }
        view.schemas = reflection.schemas.clone();
        let mut objects = BTreeMap::new();
        for (&at, class) in &view.objects {
            if matches!(class.as_str(), "hkClass" | "hkClassEnum") {
                continue;
            }
            objects.insert(
                id(at),
                Object {
                    klass: class.clone(),
                    fields: view.decode(at, class, &mut HashSet::new())?,
                },
            );
        }
        fn validate(value: &Value, objects: &BTreeMap<String, Object>) -> Result<()> {
            match value {
                Value::Object(fields) => {
                    if fields.len() == 1 {
                        if let Some(reference) = fields.get("ref") {
                            ensure!(
                                objects.contains_key(
                                    reference.as_str().context("Invalid object reference")?
                                ),
                                "Unresolved behavior reference {reference}"
                            );
                        }
                    }
                    for v in fields.values() {
                        validate(v, objects)?;
                    }
                }
                Value::Array(values) => {
                    for v in values {
                        validate(v, objects)?;
                    }
                }
                _ => (),
            }
            Ok(())
        }
        for object in objects.values() {
            validate(&object.fields, &objects)?;
        }
        Ok(Self {
            objects,
            reflection,
        })
    }
}
fn id(at: Address) -> String {
    format!("{}:{}", at.0, at.1)
}
fn add(at: Address, offset: usize) -> Result<Address> {
    Ok((
        at.0,
        at.1.checked_add(offset)
            .context("Behavior offset overflow")?,
    ))
}
struct View<'a> {
    file: &'a HkxFile,
    pointers: HashMap<Address, Address>,
    objects: BTreeMap<Address, String>,
    schemas: HashMap<String, Schema>,
}
impl<'a> View<'a> {
    fn bytes(&self, at: Address, size: usize) -> Result<&[u8]> {
        let data = self
            .file
            .section_data
            .get(at.0)
            .context("Behavior section missing")?;
        let limit: usize = self.file.sections[at.0].local_fixups_offset.try_into()?;
        let end = at.1.checked_add(size).context("Behavior extent overflow")?;
        ensure!(end <= limit, "Behavior read crosses into fixup table");
        data.get(at.1..end).context("Truncated behavior member")
    }
    fn word(&self, at: Address) -> Result<u32> {
        let b = self.bytes(at, 4)?.try_into()?;
        Ok(if self.file.header.endianness == 1 {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
    fn half(&self, at: Address) -> Result<u16> {
        let b = self.bytes(at, 2)?.try_into()?;
        Ok(if self.file.header.endianness == 1 {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }
    fn wide(&self, at: Address) -> Result<u64> {
        let b = self.bytes(at, 8)?.try_into()?;
        Ok(if self.file.header.endianness == 1 {
            u64::from_le_bytes(b)
        } else {
            u64::from_be_bytes(b)
        })
    }
    fn pointer(&self, at: Address) -> Result<Option<Address>> {
        let pointer = self.pointers.get(&at).copied();
        if let Some(target) = pointer {
            self.bytes(target, 1)?;
        } else {
            ensure!(self.word(at)? == 0, "Unresolved pointer at {at:?}");
        }
        Ok(pointer)
    }
    fn required(&self, at: Address) -> Result<Address> {
        self.pointer(at)?
            .context("Missing required behavior pointer")
    }
    fn string(&self, at: Address) -> Result<String> {
        let limit: usize = self
            .file
            .sections
            .get(at.0)
            .context("String section missing")?
            .local_fixups_offset
            .try_into()?;
        let bytes = self.bytes(
            at,
            limit.checked_sub(at.1).context("String outside section")?,
        )?;
        let end = bytes
            .iter()
            .position(|&b| b == 0)
            .context("Unterminated behavior string")?;
        Ok(std::str::from_utf8(&bytes[..end])?.to_owned())
    }
    fn name(&self, at: Address) -> Result<String> {
        self.string(self.required(at)?)
    }
    fn new(file: &'a HkxFile) -> Result<Self> {
        ensure!(
            file.header.pointer_size == 4
                && ((file.header.endianness == 0
                    && file.header.contents_version.starts_with("Havok-7.0"))
                    || (file.header.endianness == 1
                        && file.header.contents_version == "hk_2012.1.0-r1")),
            "Unsupported reflected behavior ABI: {} / pointer {} / endian {}",
            file.header.contents_version,
            file.header.pointer_size,
            file.header.endianness
        );
        let mut view = Self {
            file,
            pointers: HashMap::new(),
            objects: BTreeMap::new(),
            schemas: HashMap::new(),
        };
        for (section, fixups) in file.local_fixups.iter().enumerate() {
            for f in fixups {
                ensure!(
                    view.pointers
                        .insert(
                            (section, f.src_offset as usize),
                            (section, f.dst_offset as usize)
                        )
                        .is_none(),
                    "Duplicate local pointer"
                );
            }
        }
        for (section, fixups) in file.global_fixups.iter().enumerate() {
            for f in fixups {
                ensure!(
                    view.pointers
                        .insert(
                            (section, f.src_offset as usize),
                            (f.dst_section as usize, f.dst_offset as usize)
                        )
                        .is_none(),
                    "Duplicate global pointer"
                );
            }
        }
        for (section, fixups) in file.virtual_fixups.iter().enumerate() {
            for f in fixups {
                let name = view.string((f.section_index as usize, f.class_name_offset as usize))?;
                ensure!(
                    view.objects
                        .insert((section, f.src_offset as usize), name)
                        .is_none(),
                    "Duplicate behavior object"
                );
            }
        }
        for (&at, name) in &view.objects {
            if name != "hkClass" {
                continue;
            }
            let class = view.name(at)?;
            let parent = view
                .pointer(add(at, 4)?)?
                .map(|p| view.name(p))
                .transpose()?;
            let size = view.word(add(at, 8)?)? as usize;
            let count = view.word(add(at, 28)?)? as usize;
            let mut members = vec![];
            if count > 0 {
                let array = view.required(add(at, 24)?)?;
                view.bytes(
                    array,
                    count.checked_mul(24).context("Member array overflow")?,
                )?;
                for i in 0..count {
                    let m = add(array, i * 24)?;
                    let class = view
                        .pointer(add(m, 4)?)?
                        .map(|p| view.name(p))
                        .transpose()?;
                    members.push(Member {
                        name: view.name(m)?,
                        class,
                        kind: view.bytes(add(m, 12)?, 1)?[0],
                        subtype: view.bytes(add(m, 13)?, 1)?[0],
                        count: view.half(add(m, 14)?)? as usize,
                        flags: view.half(add(m, 16)?)?,
                        offset: view.half(add(m, 18)?)? as usize,
                    });
                }
            }
            ensure!(
                view.schemas
                    .insert(
                        class,
                        Schema {
                            size,
                            parent,
                            members
                        }
                    )
                    .is_none(),
                "Duplicate behavior schema"
            );
        }
        Ok(view)
    }
    fn size(&self, kind: u8, class: Option<&str>) -> Result<usize> {
        Ok(match kind {
            1..=4 => 1,
            5 | 6 | 32 => 2,
            7 | 8 | 11 | 20 | 29 | 30 | 33 => 4,
            9 | 10 => 8,
            12 | 13 => 16,
            25 => {
                self.schemas
                    .get(class.context("Struct has no class")?)
                    .context("Unknown struct schema")?
                    .size
            }
            _ => bail!("Unsupported behavior element stride {kind}"),
        })
    }
    fn decode(&self, at: Address, class: &str, stack: &mut HashSet<String>) -> Result<Value> {
        ensure!(
            stack.insert(class.into()),
            "Cyclic inline behavior schema {class}"
        );
        let schema = self
            .schemas
            .get(class)
            .with_context(|| format!("Missing behavior schema {class}"))?;
        self.bytes(at, schema.size)?;
        let mut fields = if let Some(parent) = &schema.parent {
            self.decode(at, parent, stack)?
                .as_object()
                .context("Invalid parent")?
                .clone()
        } else {
            Map::new()
        };
        for m in &schema.members {
            if m.flags & 0x400 != 0 {
                continue;
            } // serialized(false)
            let pos = add(at, m.offset)?;
            let value = if m.count > 0 {
                let stride = self.size(m.kind, m.class.as_deref())?;
                self.bytes(
                    pos,
                    m.count
                        .checked_mul(stride)
                        .context("Inline array overflow")?,
                )?;
                Value::Array(
                    (0..m.count)
                        .map(|i| {
                            self.value(
                                add(pos, i * stride)?,
                                m.kind,
                                m.subtype,
                                m.class.as_deref(),
                                stack,
                            )
                        })
                        .collect::<Result<_>>()?,
                )
            } else {
                self.value(pos, m.kind, m.subtype, m.class.as_deref(), stack)?
            };
            ensure!(
                fields.insert(m.name.clone(), value).is_none(),
                "Duplicate inherited behavior member"
            );
        }
        stack.remove(class);
        Ok(Value::Object(fields))
    }
    fn value(
        &self,
        at: Address,
        kind: u8,
        subtype: u8,
        class: Option<&str>,
        stack: &mut HashSet<String>,
    ) -> Result<Value> {
        Ok(match kind {
            1 => json!(self.bytes(at, 1)?[0] != 0),
            2 | 3 => json!(self.bytes(at, 1)?[0] as i8),
            4 => json!(self.bytes(at, 1)?[0]),
            5 => json!(self.half(at)? as i16),
            6 => json!(self.half(at)?),
            7 => json!(self.word(at)? as i32),
            8 | 30 => json!(self.word(at)?),
            9 => json!(self.wide(at)? as i64),
            10 => json!(self.wide(at)?),
            11 => {
                let value = f32::from_bits(self.word(at)?);
                ensure!(value.is_finite(), "Non-finite behavior float");
                json!(value)
            }
            12 | 13 => Value::Array(
                (0..4)
                    .map(|i| self.value(add(at, i * 4)?, 11, 0, None, stack))
                    .collect::<Result<_>>()?,
            ),
            20 => self
                .pointer(at)?
                .map(|p| json!({"ref": id(p)}))
                .unwrap_or(Value::Null),
            29 | 33 => self
                .pointer(at)?
                .map(|p| self.string(p).map(Value::String))
                .transpose()?
                .unwrap_or(Value::Null),
            24 | 31 => {
                ensure!(!matches!(subtype, 24 | 31), "Recursive enum type");
                self.value(at, subtype, 0, None, stack)?
            }
            25 => self.decode(at, class.context("Struct missing class")?, stack)?,
            22 | 26 => {
                let count = self.word(add(at, 4)?)? as usize;
                let mut values = vec![];
                if count > 0 {
                    let array = self.required(at)?;
                    let stride = self.size(subtype, class)?;
                    self.bytes(
                        array,
                        count
                            .checked_mul(stride)
                            .context("Behavior array overflow")?,
                    )?;
                    for i in 0..count {
                        values.push(self.value(
                            add(array, i * stride)?,
                            subtype,
                            0,
                            class,
                            stack,
                        )?);
                    }
                }
                Value::Array(values)
            }
            _ => bail!("Unsupported behavior member {kind}/{subtype} at {at:?}"),
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_character_name_uses_filename_prefix_not_first_rig() -> anyhow::Result<()> {
        assert_eq!(super::default_character_name(r"props\ysl_brokenbroomsbehaviorproject.hkx")?, "Ysl_brokenbrooms");
        assert_eq!(super::default_character_name("a/testproject.hkx")?, "Test");
        assert_eq!(super::default_character_name("a/projectabbehavior.hkx")?, "Projectab");
        assert!(super::default_character_name("a/behavior.hkx").is_err());
        assert!(super::default_character_name("a/NoDelimiter.hkx").is_err());
        Ok(())
    }

    use super::*;
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn stripped_retail_watch_uses_matching_types_and_rejects_incompatible_registry() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let stored = crate::pack_io::read_filtered(
            std::fs::read(std::path::Path::new(&root).join("packfiles/_dynamic.pak"))?,
            |_, path| {
                matches!(
                    path.replace('\\', "/").to_ascii_lowercase().as_str(),
                    "characters/mickey/characters/mickey.hkx_wii"
                        | "skinnedprops/watch/characters/watch.hkx_wii"
                )
            },
        )?;
        let find = |suffix: &str| {
            stored
                .pak
                .files
                .iter()
                .find(|file| {
                    file.path
                        .replace('\\', "/")
                        .to_ascii_lowercase()
                        .ends_with(suffix)
                })
                .with_context(|| format!("Missing original fixture {suffix}"))
        };
        let donor = Document::load(&find("/mickey.hkx_wii")?.data)?;
        let bytes = &find("/watch.hkx_wii")?.data;
        let file = HkxFile::load_bytes(bytes).map_err(anyhow::Error::msg)?;
        ensure!(
            View::new(&file)?.schemas.is_empty(),
            "Fixture unexpectedly contains reflection types"
        );
        ensure!(
            Document::load(bytes).is_err(),
            "A stripped file must require an explicit registry"
        );
        let decoded = Document::load_with_reflection(bytes, &donor.reflection)?;
        let reference: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/stripped_watch.json"
        ))?;
        ensure!(
            serde_json::to_value(&decoded)?["objects"] == reference["objects"],
            "Stripped fields differ from the independent Python decoder"
        );
        ensure!(decoded.objects.len() == 5, "Stripped object was omitted");
        ensure!(
            decoded.single("hkbCharacterStringData")?["name"] == "watch",
            "Incorrect reflection offsets"
        );
        ensure!(
            strings(&decoded.single("hkbCharacterStringData")?["animationNames"])?
                == ["watch_countdown.hkx", "watch_exit.hkx", "watch_spawn.hkx"],
            "Incorrect stripped animation names"
        );
        ensure!(
            decoded.single("hkbCharacterData").is_ok()
                && decoded.single("hkbMirroredSkeletonInfo").is_ok(),
            "Required character metadata was omitted"
        );
        let signature = donor.reflection.signatures["hkbCharacterData"];
        ensure!(signature == 0xcd83778f, "Unexpected retail signature");
        let names = file
            .sections
            .iter()
            .find(|section| section.section_tag == "__classnames__")
            .context("Missing names")?;
        let class = file
            .class_names
            .iter()
            .find(|class| class.name == "hkbCharacterData")
            .context("Missing character signature")?;
        let mut altered = bytes.clone();
        let offset = names.absolute_data_start as usize + class.offset as usize - 5;
        altered[offset] ^= 1;
        let error = Document::load_with_reflection(&altered, &donor.reflection)
            .err()
            .context("Changed signature was accepted")?;
        ensure!(
            error.to_string().contains("signature mismatch"),
            "Wrong rejection: {error:#}"
        );
        let mut foreign = donor.reflection.clone();
        foreign.abi.as_mut().unwrap().0.push_str("-foreign");
        ensure!(
            Document::load_with_reflection(bytes, &foreign)
                .err()
                .context("Foreign registry accepted")?
                .to_string()
                .contains("ABI/version"),
            "Wrong ABI rejection"
        );
        let mut conflicting = donor.reflection.clone();
        conflicting
            .schemas
            .get_mut("hkbCharacterData")
            .unwrap()
            .size += 4;
        ensure!(
            conflicting.merge(&donor.reflection).is_err(),
            "Conflicting layouts were accepted"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original locally extracted Mickey assets"]
    fn retail_behavior_graph() -> Result<()> {
        let bytes = std::fs::read("target/managed-player-assets/MickeyBehavior.hkx_wii")?;
        let graph = Document::load(&bytes)?;
        ensure!(
            graph.condition_programs()?.len() == 16,
            "Missing compiled conditions"
        );
        ensure!(graph.objects.len() == 796, "Incomplete graph");
        ensure!(
            graph.single("hkbBehaviorGraphStringData")?["eventNames"]
                .as_array()
                .unwrap()
                .len()
                == 163,
            "Event table differs"
        );
        let python: Value =
            serde_json::from_slice(&std::fs::read("target/blotworx-mickey-behavior.json")?)?;
        fn equivalent(a: &Value, b: &Value) -> bool {
            match (a, b) {
                (Value::Number(a), Value::Number(b)) if a.is_f64() || b.is_f64() => {
                    (a.as_f64().unwrap() as f32).to_bits() == (b.as_f64().unwrap() as f32).to_bits()
                }
                (Value::Array(a), Value::Array(b)) => {
                    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equivalent(a, b))
                }
                (Value::Object(a), Value::Object(b)) => {
                    a.len() == b.len()
                        && a.iter()
                            .all(|(k, a)| b.get(k).is_some_and(|b| equivalent(a, b)))
                }
                _ => a == b,
            }
        }
        // Python expands f32 to f64 when formatting JSON; compare the original
        // f32 bits, with exact discrete values, array order and reference IDs.
        for (id, object) in &graph.objects {
            ensure!(
                python["objects"][id]["klass"] == object.klass,
                "Class differs at {id}"
            );
            ensure!(
                equivalent(&python["objects"][id]["fields"], &object.fields),
                "Members differ at {id}"
            );
        }
        for name in ["MickeyBehaviorProject", "Mickey_Character"] {
            Document::load(&std::fs::read(format!(
                "target/managed-player-assets/{name}.hkx_wii"
            ))?)?;
        }
        Ok(())
    }
    #[test]
    fn behavior_paths_stay_inside_project() {
        assert_eq!(
            relative("Characters/Mickey", "Behaviors\\MickeyBehavior.hkx").unwrap(),
            "Characters/Mickey/Behaviors/MickeyBehavior.hkx"
        );
        for path in [
            "../outside.hkx",
            "C:/outside.hkx",
            "/outside.hkx",
            "folder\\..\\outside.hkx",
        ] {
            assert!(relative("Characters/Mickey", path).is_err());
        }
    }
}
