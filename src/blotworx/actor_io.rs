//! Authored entity output -> input/script dispatch for the native scene.
//! This is the supported subset of InputOutputComponent, not a replacement
//! for the game-event registry, trigger volumes or the BSQ scheduler.
use super::assets::RuntimeEntity;
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::scene_file::{Component, Entity};

/// Retail REL3290dc tokenization. Commas are separators except when preceded
/// by a backslash, which is removed. Search starts after each token's first
/// byte: leading/consecutive commas are retained, trailing empty fields are
/// omitted, and whitespace is not trimmed. C strings stop at the first NUL.
pub(super) fn parameters(source: &str) -> Vec<String> {
    let mut bytes=source.as_bytes().split(|&b|b==0).next().unwrap_or_default().to_vec();
    let mut start=0;
    let mut result=Vec::new();
    while start<bytes.len() {
        let mut search=start+1;
        let separator=loop {
            let Some(relative)=bytes[search..].iter().position(|&b|b==b',') else {break None;};
            let comma=search+relative;
            if bytes[comma-1]==b'\\' {
                bytes.remove(comma-1);
                search=comma;
            } else {break Some(comma);}
        };
        let end=separator.unwrap_or(bytes.len());
        result.push(String::from_utf8(bytes[start..end].to_vec()).expect("Removing ASCII separators preserves UTF-8"));
        if let Some(comma)=separator {start=comma+1;} else {break;}
    }
    result
}

impl super::assets::World {
    /// Cooked prefab IO retains the source prefab's local names. Resolve
    /// those only inside the callback owner's explicit prefab membership.
    pub(super) fn resolve_io_target(&self, owner:usize, name:&str)->Result<Option<usize>> {
        if let Some(target)=self.scripts.resolve_name(name) {return Ok(Some(target));}
        let Some(prefab)=self.scripts.prefab_owner(owner)? else {return Ok(None);};
        let root=&self.entities[prefab];
        let path=root.components.iter().find(|c|c.class_name=="JPSPrefabComponent")
            .and_then(|c|c.properties.iter().find(|p|p.name=="Prefab Path"))
            .and_then(|p|p.value.as_str());
        let local=if let Some((prefix,local))=name.split_once('.') {
            let Some(path)=path else {return Ok(None);};
            let path=path.replace('\\',"/");
            let file=path.rsplit('/').next().unwrap_or("");
            let stem=file.rsplit_once('.').map_or(file,|(stem,_)|stem);
            if !prefix.eq_ignore_ascii_case(stem) {return Ok(None);}
            local
        } else {name};
        let candidate=format!("{}.{}",root.name,local);
        let Some(target)=self.scripts.resolve_name(&candidate) else {return Ok(None);};
        Ok((!self.removed_entities.contains(&target)
            && self.entities[target].scene==root.scene
            && self.scripts.prefab_owner(target)?==Some(prefab)).then_some(target))
    }
}

/// IO rows are additive property layers, unlike ordinary scalar overrides.
/// Retail REL326250..326a14 visits the local layer first, then +0x14 masters,
/// stopping after the first layer whose DoNotInherit value is true.
pub(super) fn component_index(objects: &[Entity]) -> std::collections::HashMap<u32, &Component> {
    objects
        .iter()
        .flat_map(|entity| &entity.components)
        .map(|component| (component.link_id.to_u32(), component))
        .collect()
}
pub(super) fn inherit_connections(
    components: &mut [Component],
    index: &std::collections::HashMap<u32, &Component>,
) -> Result<()> {
    let fields = [
        "Output",
        "Input",
        "Target",
        "Parameters",
        "DelayWait",
        "IoFlags",
        "TargetGroup",
        "Instigator",
        "TargetGUID",
    ];
    for effective in components
        .iter_mut()
        .filter(|component| component.class_name == "InputOutputComponent")
    {
        let mut rows: [Vec<serde_json::Value>; 9] = Default::default();
        let mut optional_present=[false;2];
        let mut current = index
            .get(&effective.link_id.to_u32())
            .copied()
            .context("IO component has no source property layer")?;
        let mut visited = std::collections::HashSet::new();
        loop {
            ensure!(
                visited.insert(current.link_id.to_u32()),
                "Cyclic IO component inheritance"
            );
            ensure!(
                current.class_name == "InputOutputComponent",
                "IO master has a different component class"
            );
            let count = current
                .properties
                .iter()
                .find(|property| property.name == "Output")
                .map(|property| property.value.as_array().context("Invalid IO Output layer"))
                .transpose()?
                .map_or(0, Vec::len);
            for (field, values) in fields.iter().zip(&mut rows) {
                let array = current
                    .properties
                    .iter()
                    .find(|property| property.name == *field)
                    .map(|property| {
                        property
                            .value
                            .as_array()
                            .with_context(|| format!("Invalid IO {field} layer"))
                    })
                    .transpose()?;
                if let Some(index)=["Instigator","TargetGUID"].iter().position(|name|name==field) {
                    optional_present[index]|=array.is_some();
                }
                for row in 0..count {
                    let value = array.and_then(|array| array.get(row)).cloned();
                    // Native optional arrays keep the row's initialized defaults.
                    values.push(value.unwrap_or_else(|| match *field {
                        "DelayWait" => serde_json::json!(0.0),
                        "IoFlags" | "TargetGUID" => serde_json::json!(0),
                        _ => serde_json::json!(""),
                    }));
                }
            }
            if current.properties.iter().any(|property| {
                property.name == "DoNotInherit" && property.value.as_bool() == Some(true)
            }) {
                break;
            }
            let master = current.master_link_id.to_u32();
            if master == 0 {
                break;
            }
            current = index
                .get(&master)
                .copied()
                .with_context(|| format!("Missing IO master component {master}"))?;
        }
        for (field, values) in fields.iter().zip(rows) {
            if let Some(index)=["Instigator","TargetGUID"].iter().position(|name|name==field) {
                if !optional_present[index] {continue;}
            }
            if let Some(property) = effective
                .properties
                .iter_mut()
                .find(|property| property.name == *field)
            {
                property.value = serde_json::Value::Array(values);
            } else if matches!(*field,"Instigator"|"TargetGUID") && !values.is_empty() {
                effective.properties.push(epic_mickey_lib_rs::scene_file::Property::new(
                    if *field=="TargetGUID" {"Integer"}else{"String"}.into(),field.to_string(),false,false,false,serde_json::Value::Array(values)));
            } else if !values.is_empty() {
                anyhow::bail!("Effective IO component lacks {field}");
            }
        }
    }
    Ok(())
}
#[derive(Clone)]
pub struct Connection {
    /// EM2 event-listening entity, independent of the action's target.
    /// None preserves the older game's local-only schema.
    pub instigator: Option<String>,
    pub target_guid: Option<u32>,
    pub owner: usize,
    pub input: String,
    pub output: String,
    pub target: String,
    pub parameters: String,
    pub delay: f32,
    pub flags: u32,
    pub group: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn io_layers_preserve_empty_overrides_order_and_explicit_inheritance_stop() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{Property, ID};
        let layer = |id: u32, master: u32, inputs: &[&str], stop: bool| {
            let fields = [
                ("Input", serde_json::json!(inputs)),
                ("Output", serde_json::json!(vec!["OnUser1"; inputs.len()])),
                ("Target", serde_json::json!(vec!["_self"; inputs.len()])),
                ("Parameters", serde_json::json!(vec![""; inputs.len()])),
                ("DelayWait", serde_json::json!(vec![0.; inputs.len()])),
                ("IoFlags", serde_json::json!(vec![0; inputs.len()])),
                ("TargetGroup", serde_json::json!([])),
                ("DoNotInherit", serde_json::json!(stop)),
            ];
            Component::new(
                "InputOutputComponent".into(),
                "IO".into(),
                ID::new(0),
                ID::from_u32(id),
                ID::from_u32(master),
                fields
                    .into_iter()
                    .map(|(name, value)| {
                        Property::new(String::new(), name.into(), false, false, false, value)
                    })
                    .collect(),
            )
        };
        let master = layer(1, 0, &["Inherited"], false);
        let child = layer(2, 1, &[], false);
        let entity = Entity::new(
            "JPSGeneralEntity".into(),
            "Owner".into(),
            ID::from_u32(10),
            ID::new(0),
            0,
            0,
            vec![master, child.clone()],
        );
        let mut effective = vec![child];
        inherit_connections(
            &mut effective,
            &component_index(std::slice::from_ref(&entity)),
        )?;
        let inputs = |components: &[Component]| {
            components[0]
                .properties
                .iter()
                .find(|p| p.name == "Input")
                .unwrap()
                .value
                .clone()
        };
        assert_eq!(inputs(&effective), serde_json::json!(["Inherited"]));
        let mut entity = entity;
        entity.components[1] = layer(2, 1, &["Local"], false);
        let mut effective = vec![entity.components[1].clone()];
        inherit_connections(
            &mut effective,
            &component_index(std::slice::from_ref(&entity)),
        )?;
        assert_eq!(
            inputs(&effective),
            serde_json::json!(["Local", "Inherited"])
        );
        let mut outputs = Outputs::default();
        outputs.bind(0, "Owner", &effective)?;
        outputs.emit(0, "OnUser1", 0, 0.);
        assert_eq!(
            outputs
                .ready(0.)
                .iter()
                .map(|delivery| delivery.connection.input.as_str())
                .collect::<Vec<_>>(),
            ["Local", "Inherited"]
        );
        entity.components[1] = layer(2, 1, &["Local"], true);
        let mut effective = vec![entity.components[1].clone()];
        inherit_connections(
            &mut effective,
            &component_index(std::slice::from_ref(&entity)),
        )?;
        assert_eq!(inputs(&effective), serde_json::json!(["Local"]));
        Ok(())
    }
    #[test]
    fn binding_spawn_preserves_delayed_actions_and_rejects_partial_bindings() {
        use epic_mickey_lib_rs::scene_file::{Component, Property, ID};
        let component = |bad: bool| {
            let fields = [
                ("Output", serde_json::json!(["OnSpawn"])),
                ("Input", serde_json::json!(["EnableMotion"])),
                ("Target", serde_json::json!([""])),
                ("Parameters", serde_json::json!([""])),
                ("DelayWait", serde_json::json!([0.53])),
                (
                    "IoFlags",
                    if bad {
                        serde_json::json!([])
                    } else {
                        serde_json::json!([0])
                    },
                ),
            ];
            Component::new(
                "InputOutputComponent".into(),
                "IO".into(),
                ID { id: 0 },
                ID { id: 0 },
                ID { id: 0 },
                fields
                    .into_iter()
                    .map(|(name, value)| {
                        Property::new(String::new(), name.into(), false, false, false, value)
                    })
                    .collect(),
            )
        };
        let mut outputs = Outputs::default();
        outputs.bind(1, "existing", &[component(false)]).unwrap();
        outputs.emit(1, "OnSpawn", 10, 0.);
        assert!(outputs
            .bind(2, "invalid", &[component(false), component(true)])
            .is_err());
        assert_eq!(outputs.connections.len(), 1);
        outputs.bind(2, "spawn", &[component(false)]).unwrap();
        outputs.emit(2, "OnSpawn", 20, 0.25);
        assert!(outputs.ready(0.5).is_empty());
        let first = outputs.ready(0.6);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].activator, Some(10));
        let second = outputs.ready(0.8);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].activator, Some(20));
        assert!(outputs.bind(2, "duplicate", &[component(false)]).is_err());
    }
    #[test]
    fn retiring_owner_cancels_only_its_pending_outputs_and_preserves_other_indices() {
        let connection = |owner| Connection {instigator:None,target_guid:None,
            owner,
            input: "Activate".into(),
            output: "OnUse".into(),
            target: String::new(),
            parameters: String::new(),
            delay: 1.,
            flags: 0,
            group: String::new(),
        };
        let mut outputs = Outputs {
            connections: vec![connection(1), connection(2), connection(1), connection(3)],
            ..Default::default()
        };
        for (index, connection) in outputs.connections.iter().enumerate() {
            outputs
                .order
                .entry((connection.owner, connection.output.clone()))
                .or_default()
                .push(index);
        }
        for owner in 1..=3 {
            outputs.emit(owner, "OnUse", 0, 0.);
        }
        outputs.remove_owner(1);
        assert!(outputs.ready(0.5).is_empty());
        let ready = outputs.ready(1.);
        assert_eq!(
            ready.iter().map(|d| d.connection.owner).collect::<Vec<_>>(),
            [2, 3]
        );
        outputs.emit(1, "OnUse", 0, 2.);
        assert!(outputs.ready(3.).is_empty());
        outputs.emit(3, "OnUse", 0, 4.);
        outputs.remove_owner(2);
        assert_eq!(outputs.ready(5.)[0].connection.owner, 3);
    }
    #[test]
    fn once_disabled_delay_and_swap_removal_preserve_native_dispatch_order() {
        let mut outputs = Outputs::default();
        for (input, flags, delay) in [
            ("once", 1, 1.),
            ("middle", 0, 0.),
            ("disabled", 2, 0.),
            ("tail", 0, 0.),
        ] {
            let index = outputs.connections.len();
            outputs.connections.push(Connection {instigator:None,target_guid:None,
                owner: 1,
                input: input.into(),
                output: "OnUser1".into(),
                target: "_self".into(),
                parameters: String::new(),
                delay,
                flags,
                group: String::new(),
            });
            if flags & 2 == 0 {
                outputs
                    .order
                    .entry((1, "OnUser1".into()))
                    .or_default()
                    .push(index);
            }
        }
        outputs.emit(1, "OnUser1", 9, 0.);
        assert_eq!(
            outputs
                .ready(0.)
                .iter()
                .map(|delivery| delivery.connection.input.as_str())
                .collect::<Vec<_>>(),
            ["tail", "middle"]
        );
        outputs.emit(1, "OnUser1", 10, 0.5);
        assert_eq!(
            outputs
                .ready(0.5)
                .iter()
                .map(|delivery| delivery.connection.input.as_str())
                .collect::<Vec<_>>(),
            ["tail", "middle"]
        );
        let delayed = outputs.ready(1.);
        assert_eq!(delayed.len(), 1);
        assert_eq!(delayed[0].connection.input, "once");
        assert_eq!(delayed[0].activator, Some(9));
        outputs.emit(1, "OnUser1", 11, 2.);
        outputs.remove_owner(1);
        assert!(outputs.ready(3.).is_empty());
    }
}
/// One grouped hit. Delayed outputs retain its effective stimulus amounts even
/// after the combat call that built them has returned.
#[derive(Clone)]
pub(super) enum EventPayload {
    Combat(std::sync::Arc<[(u32, f32)]>),
    AnimationEnded(std::sync::Arc<str>),
}
impl mlua::UserData for EventPayload {}

#[derive(Clone)]
pub struct Delivery {
    pub connection: Connection,
    pub activator: Option<usize>,
    pub(super) payload: Option<EventPayload>,
}
struct Pending {
    index: usize,
    activator: Option<usize>,
    payload: Option<EventPayload>,
    due: f32,
}
#[derive(Default)]
pub struct Outputs {
    connections: Vec<Connection>,
    pending: Vec<Pending>,
    order: std::collections::BTreeMap<(usize, String), Vec<usize>>,
}
impl Outputs {
    pub fn pending_count(&self)->usize {self.pending.len()}
    pub fn has_connections(&self, owner: usize, output: &str) -> bool {
        self.order
            .get(&(owner, output.to_owned()))
            .is_some_and(|indices| !indices.is_empty())
    }
    pub fn has_pending_for_owner(&self, owner: usize) -> bool {
        self.pending
            .iter()
            .any(|pending| self.connections[pending.index].owner == owner)
    }
    /// Remove the destroyed owner's bindings without changing the order or
    /// deadlines of surviving entities' pending deliveries.
    pub fn remove_owner(&mut self, owner: usize) {
        let mut remap = Vec::with_capacity(self.connections.len());
        let mut next = 0;
        self.connections.retain(|connection| {
            if connection.owner == owner {
                remap.push(None);
                false
            } else {
                remap.push(Some(next));
                next += 1;
                true
            }
        });
        self.pending.retain_mut(|pending| {
            if let Some(index) = remap[pending.index] {
                pending.index = index;
                true
            } else {
                false
            }
        });
        self.order.retain(|(source, _), _| *source != owner);
        for indices in self.order.values_mut() {
            *indices = indices.iter().filter_map(|&index| remap[index]).collect();
        }
    }
    pub fn load(entities: &[RuntimeEntity]) -> Result<Self> {
        let mut outputs = Self::default();
        for (owner, entity) in entities.iter().enumerate() {
            outputs.bind(owner, &entity.name, &entity.components)?;
        }
        Ok(outputs)
    }
    /// Bind one newly created entity without rebuilding the dispatcher or
    /// discarding existing delayed actions. Failed decoding publishes nothing.
    pub fn bind(
        &mut self,
        owner: usize,
        name: &str,
        components: &[epic_mickey_lib_rs::scene_file::Component],
    ) -> Result<()> {
        self.bind_resolved(owner,name,components,|connection| {
            ensure!(connection.instigator.as_deref().is_none_or(|name|name=="_self"),
                "Remote IO instigator needs source resolution");
            Ok(Some(owner))
        })
    }
    /// Resolve event sources before committing any rows. EM2's Instigator
    /// redirects event subscription, not the Lua callback's target argument.
    pub fn bind_resolved(
        &mut self, owner:usize,name:&str,components:&[Component],
        mut source:impl FnMut(&Connection)->Result<Option<usize>>,
    )->Result<()> {
        ensure!(
            !self
                .connections
                .iter()
                .any(|connection| connection.owner == owner),
            "Entity IO already bound: {name}"
        );
        let mut connections = Vec::new();
        for component in components
            .iter()
            .filter(|c| c.class_name == "InputOutputComponent")
        {
            let values = |name: &str| {
                component
                    .properties
                    .iter()
                    .find(|p| p.name == name)
                    .and_then(|p| p.value.as_array())
            };
            let Some(outputs) = values("Output") else {
                continue;
            };
            let field = |name: &str, index: usize| -> Result<&serde_json::Value> {
                values(name)
                    .and_then(|v| v.get(index))
                    .with_context(|| format!("Incomplete {name} array at IO owner {owner}"))
            };
            for (i, output) in outputs.iter().enumerate() {
                let string = |name| -> Result<String> {
                    Ok(field(name, i)?
                        .as_str()
                        .context("Invalid IO string")?
                        .into())
                };
                let delay = field("DelayWait", i)?
                    .as_f64()
                    .context("Invalid IO delay")? as f32;
                ensure!(
                    delay.is_finite() && delay >= 0.,
                    "Invalid IO delay on {name}"
                );
                connections.push(Connection {
                    instigator: values("Instigator").and_then(|v|v.get(i)).map(|v|
                        v.as_str().map(str::to_owned).context("Invalid IO instigator")).transpose()?,
                    target_guid: values("TargetGUID").and_then(|v|v.get(i)).map(|v|
                        v.as_u64().and_then(|n|u32::try_from(n).ok())
                            .or_else(||v.as_i64().and_then(|n|i32::try_from(n).ok()).map(|n|n as u32))
                            .context("Invalid IO target GUID")).transpose()?,
                    owner,
                    input: string("Input")?,
                    output: output.as_str().context("Invalid output name")?.into(),
                    target: string("Target")?,
                    parameters: string("Parameters")?,
                    delay,
                    flags: field("IoFlags", i)?
                        .as_u64()
                        .and_then(|v| u32::try_from(v).ok())
                        .context("Invalid IO flags")?,
                    group: values("TargetGroup")
                        .and_then(|v| v.get(i))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .into(),
                });
            }
        }
        let sources=connections.iter().map(|row|if row.flags&2!=0 {Ok(None)}else{source(row)}).collect::<Result<Vec<_>>>()?;
        let base = self.connections.len();
        for (index, connection) in connections.iter().enumerate() {
            if connection.flags & 2 == 0 {
                let Some(source)=sources[index] else {continue;};
                self.order
                    .entry((source, connection.output.clone()))
                    .or_default()
                    .push(base + index);
            }
        }
        self.connections.extend(connections);
        Ok(())
    }
    pub fn emit(&mut self, owner: usize, output: &str, activator: usize, time: f32) {
        self.emit_optional(owner,output,Some(activator),time);
    }
    pub(super) fn emit_optional(&mut self, owner:usize,output:&str,activator:Option<usize>,time:f32) {
        self.emit_with_payload(owner, output, activator, time, None);
    }

    pub(super) fn emit_combat(
        &mut self,
        owner: usize,
        output: &str,
        activator: Option<usize>,
        time: f32,
        effective: &[(u32, f32)],
    ) {
        if self.has_connections(owner, output) {
            self.emit_with_payload(owner, output, activator, time,
                Some(EventPayload::Combat(effective.into())));
        }
    }

    pub(super) fn emit_animation_ended(&mut self, owner: usize, time: f32, name: &str) {
        if self.has_connections(owner, "OnAnimGBEnded") {
            self.emit_with_payload(owner, "OnAnimGBEnded", Some(owner), time,
                Some(EventPayload::AnimationEnded(name.into())));
        }
    }

    fn emit_with_payload(
        &mut self,
        owner: usize,
        output: &str,
        activator: Option<usize>,
        time: f32,
        payload: Option<EventPayload>,
    ) {
        let Some(indices) = self.order.get_mut(&(owner, output.to_owned())) else {
            return;
        };
        let mut position = 0;
        while position < indices.len() {
            let index = indices[position];
            let connection = &self.connections[index];
            self.pending.push(Pending {
                index,
                activator,
                payload: payload.clone(),
                due: time + connection.delay,
            });
            // REL32859c/3286cc: consume after dispatch admission, swapping the
            // final row into this slot. Delayed delivery retains its payload.
            if connection.flags & 1 != 0 {
                indices.swap_remove(position);
            } else {
                position += 1;
            }
        }
    }
    pub fn ready(&mut self, time: f32) -> Vec<Delivery> {
        self.ready_for(time, None)
    }
    pub fn ready_for_owner(&mut self, time: f32, owner: usize) -> Vec<Delivery> {
        self.ready_for(time, Some(owner))
    }
    fn ready_for(&mut self, time: f32, owner: Option<usize>) -> Vec<Delivery> {
        let mut ready = Vec::new();
        self.pending.retain(|pending| {
            let connection = &self.connections[pending.index];
            if time >= pending.due && owner.is_none_or(|owner| connection.owner == owner) {
                ready.push(Delivery {
                    connection: connection.clone(),
                    activator: pending.activator,
                    payload: pending.payload.clone(),
                });
                false
            } else {
                true
            }
        });
        ready
    }
}

#[cfg(test)]
mod native_io_parameters_tests {
    #[test]
    fn tokenizer_matches_retail_separator_and_escape_instructions() -> anyhow::Result<()> {
        let data:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/io_parameters.json"))?;
        for case in data["cases"].as_array().unwrap() {
            assert_eq!(serde_json::to_value(super::parameters(case["input"].as_str().unwrap()))?,case["tokens"],"input {:?}",case["input"]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod em2_source_tests {
    use super::*;
    use epic_mickey_lib_rs::scene_file::{Property, ID};
    fn component(flags:u32)->Component {
        let fields=[
            ("Output",serde_json::json!(["OnStartTouch"])),
            ("Input",serde_json::json!(["Activate"])),
            ("Target",serde_json::json!(["NamedTarget"])),
            ("Parameters",serde_json::json!([""])),
            ("DelayWait",serde_json::json!([1.])),
            ("IoFlags",serde_json::json!([flags])),
            ("Instigator",serde_json::json!(["_player"])),
            ("TargetGUID",serde_json::json!([-1])),
        ];
        Component::new("InputOutputComponent".into(),"IO".into(),ID::new(0),ID::new(0),ID::new(0),
            fields.into_iter().map(|(name,value)|Property::new(String::new(),name.into(),false,false,false,value)).collect())
    }
    #[test]
    fn remote_source_preserves_owner_activator_guid_and_once_deadline()->Result<()> {
        let mut outputs=Outputs::default();
        outputs.bind_resolved(12,"Owner",&[component(1)],|row|{
            assert_eq!(row.instigator.as_deref(),Some("_player"));
            assert_eq!(row.target_guid,Some(u32::MAX));
            Ok(Some(3))
        })?;
        outputs.emit(12,"OnStartTouch",8,0.);
        assert_eq!(outputs.pending_count(),0);
        outputs.emit(3,"OnStartTouch",8,2.);
        outputs.emit(3,"OnStartTouch",9,2.);
        assert_eq!(outputs.pending_count(),1);
        assert!(outputs.ready(2.99).is_empty());
        let delivery=outputs.ready(3.).pop().unwrap();
        assert_eq!(delivery.connection.owner,12);
        assert_eq!(delivery.activator,Some(8));
        assert_eq!(delivery.connection.target_guid,Some(u32::MAX));
        Ok(())
    }
    #[test]
    fn source_resolution_failure_is_atomic_and_disabled_rows_do_not_resolve()->Result<()> {
        let mut outputs=Outputs::default();let mut visited=0;
        assert!(outputs.bind_resolved(12,"Owner",&[component(0),component(0)],|_|{
            visited+=1;if visited==2 {anyhow::bail!("missing source");} Ok(Some(3))
        }).is_err());
        assert!(outputs.connections.is_empty());assert!(outputs.order.is_empty());
        outputs.bind_resolved(12,"Owner",&[component(2)],|_|panic!("disabled source queried"))?;
        outputs.emit(3,"OnStartTouch",8,0.);assert_eq!(outputs.pending_count(),0);
        Ok(())
    }
    #[test]
    fn retiring_remote_subscription_owner_cancels_its_deliveries()->Result<()> {
        let mut outputs=Outputs::default();
        outputs.bind_resolved(12,"Owner",&[component(0)],|_|Ok(Some(3)))?;
        outputs.bind_resolved(14,"Other",&[component(0)],|_|Ok(Some(3)))?;
        outputs.emit(3,"OnStartTouch",8,0.);outputs.remove_owner(12);
        assert_eq!(outputs.pending_count(),1);
        let ready=outputs.ready(1.);assert_eq!(ready.len(),1);assert_eq!(ready[0].connection.owner,14);
        outputs.emit(3,"OnStartTouch",9,2.);assert_eq!(outputs.ready(3.).len(),1);
        Ok(())
    }
}
