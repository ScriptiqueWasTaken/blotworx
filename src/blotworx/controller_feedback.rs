//! Kind9 animation feedback, REL31f43c..31f514. Footstep events invoke
//! GameEvents category0/2 and then emit noise kind0x6d; Noticeable_Noise only
//! emits noise. The BSS28231 selector's runtime writers remain to be connected.
use anyhow::Result;
/// Unnamed string lookup used by EVENT_Run_Lua and emitter start/stop,
/// REL2ee294/2f07b0/2f0b1c/2f0bc8.
/// A null payload/string or a different built-in payload type produces no
/// script. Lists use native ordered typed search, including typed-null stops.
pub fn lua_source<'a>(
    graph: &'a super::behavior::Document,
    payload: Option<&super::state_machine::Reference>,
) -> Result<Option<&'a str>> {
    Ok(string_source(graph,payload,None)?.1)
}
fn string_source<'a>(graph:&'a super::behavior::Document,payload:Option<&super::state_machine::Reference>,name:Option<&str>)->Result<(bool,Option<&'a str>)> {
    fn search<'a>(graph:&'a super::behavior::Document,id:&str,name:Option<&str>,active:&mut std::collections::HashSet<String>)->Result<(bool,Option<&'a str>)> {
        anyhow::ensure!(active.insert(id.to_owned()),"Cyclic animation payload list at {id}");
        let object=graph.objects.get(id).ok_or_else(||anyhow::anyhow!("Animation payload object missing: {id}"))?;
        let result=match object.klass.as_str() {
            "hkbStringEventPayload" if name.is_none()=>match object.fields.get("data") {
                Some(serde_json::Value::Null)=>Ok((true,None)),
                Some(serde_json::Value::String(source))=>Ok((true,Some(source.split('\0').next().unwrap_or("")))),
                _=>anyhow::bail!("String animation payload has invalid data"),
            },
            "hkbEventPayloadList"=>{
                let entries=object.fields.get("payloads").and_then(|value|value.as_array()).ok_or_else(||anyhow::anyhow!("Animation payload list has no payload array"))?;
                let mut found=(false,None);
                for entry in entries {
                    if entry.is_null() {continue;}
                    let reference:super::state_machine::Reference=serde_json::from_value(entry.clone())?;
                    found=search(graph,&reference.id,name,active)?;
                    // A typed null string is still a successful native search.
                    // Do not continue to a later script merely because it is null.
                    if found.0 {break;}
                }
                Ok(found)
            },
            "hkbNamedStringEventPayload" if name.is_some()=>{
                let key=object.fields.get("name").and_then(|value|value.as_str()).ok_or_else(||anyhow::anyhow!("Named string payload has no name"))?.split('\0').next().unwrap_or("");
                // REL2eec44 returns success on the type match even when the
                // name differs. Lists stop here; do not search for a later key.
                if !key.eq_ignore_ascii_case(name.unwrap()) {Ok((true,None))}
                else {match object.fields.get("data") {
                    Some(serde_json::Value::Null)=>Ok((true,None)),
                    Some(serde_json::Value::String(source))=>Ok((true,Some(source.split('\0').next().unwrap_or("")))),
                    _=>anyhow::bail!("Named string payload has invalid data"),
                }}
            },
            "hkbStringEventPayload"|"hkbIntEventPayload"|"hkbRealEventPayload"|"hkbNamedIntEventPayload"|"hkbNamedRealEventPayload"|"hkbNamedStringEventPayload"=>Ok((false,None)),
            class=>anyhow::bail!("Unnamed animation string lookup for {class} requires its native typed handler"),
        };
        active.remove(id);
        result
    }
    let Some(payload) = payload else {
        return Ok((false,None));
    };
    search(graph,&payload.id,name.filter(|name|!name.is_empty()),&mut Default::default())
}
///2f08c0..2f0a68: a bare string falls through named SoundFX lookup to the
/// unnamed event. Named/list payloads also carry AttachBone/Facing_* and must
/// not be treated as a bare string or silently discard their spatial data.
pub fn audio_source<'a>(
    graph: &'a super::behavior::Document,
    payload: Option<&super::state_machine::Reference>,
) -> Result<Option<&'a str>> {
    let (found,source)=string_source(graph,payload,Some("SoundFX"))?;
    let source=if found {source} else {lua_source(graph,payload)?};
    if source.is_none() {return Ok(None);}
    anyhow::ensure!(string_source(graph,payload,Some("AttachBone"))?.1.is_none(),"Animation audio AttachBone requires native spatial/bone playback");
    // Named real/int queries report a typed match even for a different key.
    // Any such leaf can activate Facing_X's explicit orientation path.
    fn numeric(graph:&super::behavior::Document,id:&str,active:&mut std::collections::HashSet<String>)->Result<bool> {
        anyhow::ensure!(active.insert(id.into()),"Cyclic audio payload list");
        let node=graph.objects.get(id).ok_or_else(||anyhow::anyhow!("Audio payload object missing: {id}"))?;
        let mut found=matches!(node.klass.as_str(),"hkbNamedRealEventPayload"|"hkbNamedIntEventPayload");
        if node.klass=="hkbEventPayloadList" {
            let entries=node.fields.get("payloads").and_then(|value|value.as_array()).ok_or_else(||anyhow::anyhow!("Audio payload list has no payloads"))?;
            for entry in entries.iter().filter(|entry|!entry.is_null()) {
                let child:super::state_machine::Reference=serde_json::from_value(entry.clone())?;
                if numeric(graph,&child.id,active)? {found=true;break;}
            }
        }
        active.remove(id);Ok(found)
    }
    if let Some(payload)=payload {
        anyhow::ensure!(!numeric(graph,&payload.id,&mut Default::default())?,"Animation audio Facing metadata requires native spatial playback");
    }
    Ok(source)
}
/// These route to specialized command/effect handlers in REL2f06a4 rather
/// than its ordinary kind9 fallback. Catalog presence is not implementation.
pub fn require_ordinary(id: u32) -> Result<()> {
    anyhow::ensure!(!matches!(id,0x3b|0x99|0x212..=0x21e),
        "Native specialized controller event {id:#x} requires its unimplemented payload/effect handler");
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    FootstepCategory(u32),
    Noise,
}
#[derive(Default)]
pub struct Runtime {
    /// Original BSS construction is zero. This is the retained native byte,
    /// not a guessed material/water/locale interpretation of the selector.
    pub variant: bool,
    pub noise_requests: u64,
    pub exported_events: u64,
    pub last_noise_source: Option<usize>,
    /// AnimationComponent+140/+144, not a set: duplicate pushes each receive
    /// a Stop on EVENT_Pop_Effects. The original controller retains ten names.
    pub pushed_emitters: std::collections::BTreeMap<usize, Vec<String>>,
}
pub fn dispatch(id: u32, variant: bool, mut emit: impl FnMut(Effect) -> Result<()>) -> Result<()> {
    if (0x38..=0x3a).contains(&id) {
        emit(Effect::FootstepCategory(if variant { 2 } else { 0 }))?;
        emit(Effect::Noise)?;
    } else if id == 0x8d {
        emit(Effect::Noise)?;
    }
    Ok(())
}
#[cfg(debug_assertions)]
pub(super) fn validate_payloads()->Result<()> {
    use super::{behavior::{Document,Object},state_machine::Reference};
    let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/animation_strings.json"))?;
    for case in fixture["cases"].as_array().unwrap() {
        let mut graph=Document::default();
        for (id,node) in case["nodes"].as_object().unwrap() {
            let (klass,fields)=match node["type"].as_str().unwrap() {
                "list"=>("hkbEventPayloadList",serde_json::json!({"payloads":node["children"].as_array().unwrap().iter().map(|child|child.as_str().map(|id|serde_json::json!({"ref":id})).unwrap_or(serde_json::Value::Null)).collect::<Vec<_>>()})),
                "string"=>("hkbStringEventPayload",serde_json::json!({"data":node["value"]})),
                "named"=>("hkbNamedStringEventPayload",serde_json::json!({"name":node.get("name").and_then(|value|value.as_str()).unwrap_or("ignored"),"data":node["value"]})),
                "int"=>("hkbIntEventPayload",serde_json::json!({"data":17})),
                _=>unreachable!(),
            };
            graph.objects.insert(id.clone(),Object{klass:klass.into(),fields});
        }
        let payload=Reference{id:case["root"].as_str().unwrap().into()};
        let actual=string_source(&graph,Some(&payload),case["query"].as_str())?;
        anyhow::ensure!(actual.0==case["native"]["found"].as_bool().unwrap() && actual.1==case["native"]["value"].as_str(),"Native ordered payload mismatch: {case}");
        anyhow::ensure!(audio_source(&graph,Some(&payload))?==case["native_audio"]["value"].as_str(),"Native audio payload selection mismatch: {case}");
    }
    // Exercise the selected list source through the real coroutine scheduler,
    // preserving owner context across a wait, not merely compare text.
    let mut graph=Document::default();
    graph.objects.insert("list".into(),Object{klass:"hkbEventPayloadList".into(),fields:serde_json::json!({"payloads":[{"ref":"script"},{"ref":"wrong"}]})});
    for (id,source) in [("script","assert(GetName(_self())=='payload-owner'); Wait(0); PayloadVisited=PayloadVisited+1"),("wrong","error('Incorrect list payload selected')")] {
        graph.objects.insert(id.into(),Object{klass:"hkbStringEventPayload".into(),fields:serde_json::json!({"data":source})});
    }
    let mut scripts=super::actor_scripts::Scripts::new(&[(0,"payload-owner".into())])?;
    scripts.ensure_global_module("@payload-fixture",b"PayloadVisited=0")?;
    let payload=Reference{id:"list".into()};
    scripts.schedule_animation_source(lua_source(&graph,Some(&payload))?.unwrap(),0)?;
    scripts.advance(0.)?;scripts.advance(0.)?;
    scripts.evaluate_callback_fixture("assert(PayloadVisited==1)")?;
    anyhow::ensure!(scripts.errors.is_empty(),"Payload coroutine failed: {:?}",scripts.errors);
    graph.objects.get_mut("list").unwrap().fields=serde_json::json!({"payloads":[{"ref":"list"}]});
    anyhow::ensure!(lua_source(&graph,Some(&payload)).is_err(),"Cyclic list not diagnosed");
    println!("PASS:{} original ordered/named string payload cases; selected list script executes through coroutine scheduler with preserved owner context; cycles diagnosed",fixture["cases"].as_array().unwrap().len());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn run_lua_uses_source_bytes_and_native_null_or_type_mismatch_admission() -> Result<()> {
        use super::super::{
            behavior::{Document, Object},
            state_machine::Reference,
        };
        let mut graph = Document::default();
        let reference = Reference {
            id: "payload".into(),
        };
        graph.objects.insert(
            reference.id.clone(),
            Object {
                klass: "hkbStringEventPayload".into(),
                fields: serde_json::json!({"data":"HideGremlinWrench()\0ignored"}),
            },
        );
        assert_eq!(
            lua_source(&graph, Some(&reference))?,
            Some("HideGremlinWrench()")
        );
        graph.objects.get_mut(&reference.id).unwrap().fields = serde_json::json!({"data":null});
        assert_eq!(lua_source(&graph, Some(&reference))?, None);
        graph.objects.get_mut(&reference.id).unwrap().klass = "hkbRealEventPayload".into();
        assert_eq!(lua_source(&graph, Some(&reference))?, None);
        graph.objects.get_mut(&reference.id).unwrap().klass = "hkbEventPayloadList".into();
        assert!(lua_source(&graph, Some(&reference)).is_err());
        assert_eq!(lua_source(&graph, None)?, None);
        Ok(())
    }
    #[test]
    fn audio_payload_does_not_drop_attachment_or_facing_metadata() -> Result<()> {
        use super::super::{
            behavior::{Document, Object},
            state_machine::Reference,
        };
        let mut graph = Document::default();
        let reference = Reference { id: "audio".into() };
        graph.objects.insert(
            reference.id.clone(),
            Object {
                klass: "hkbStringEventPayload".into(),
                fields: serde_json::json!({"data":"Play_sfx_mickey_walk\0unused"}),
            },
        );
        assert_eq!(
            audio_source(&graph, Some(&reference))?,
            Some("Play_sfx_mickey_walk")
        );
        for class in ["hkbNamedStringEventPayload", "hkbEventPayloadList"] {
            graph.objects.get_mut(&reference.id).unwrap().klass = class.into();
            assert!(audio_source(&graph, Some(&reference)).is_err());
        }
        assert_eq!(audio_source(&graph, None)?, None);
        Ok(())
    }
    #[test]
    fn all_steps_use_native_selector_then_noise_and_other_events_do_not() -> Result<()> {
        for id in 0x38..=0x3a {
            for variant in [false, true] {
                let mut effects = Vec::new();
                dispatch(id, variant, |effect| {
                    effects.push(effect);
                    Ok(())
                })?;
                assert_eq!(
                    effects,
                    [
                        Effect::FootstepCategory(if variant { 2 } else { 0 }),
                        Effect::Noise
                    ]
                );
            }
        }
        let mut effects = Vec::new();
        dispatch(0x8d, false, |effect| {
            effects.push(effect);
            Ok(())
        })?;
        assert_eq!(effects, [Effect::Noise]);
        for id in [0, 0x37, 0x3b, 0x8e, 0x8f, 0xc2] {
            dispatch(id, false, |_| {
                anyhow::bail!("Unexpected animation feedback")
            })?;
        }
        Ok(())
    }
    #[test]
    fn specialized_controller_commands_cannot_be_silent_ordinary_feedback() {
        for id in [0x3b, 0x99, 0x212, 0x213, 0x21e] {
            assert!(require_ordinary(id).is_err());
        }
        for id in [0, 0x38, 0x8d, 0x8e, 0x8f, 0xc2, 0x211] {
            assert!(require_ordinary(id).is_ok());
        }
    }
}
