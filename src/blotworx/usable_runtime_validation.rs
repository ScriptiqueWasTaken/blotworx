#[cfg(debug_assertions)]
pub(super) fn validate() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use epic_mickey_lib_rs::scene_file::{Component, ID, Property};
    let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut w = super::assets::load(
        std::path::Path::new(&root),
        "Levels/GV_ZoneA_Start.level",
        &egui::Context::default(),
    )?;
    w.process_deferred_entity_events()?;
    let owner = w
        .entities
        .iter()
        .enumerate()
        .find(|(i, e)| {
            w.entity_scene_active(*i)
                && !w.hidden_entities.contains(i)
                && e.components
                    .iter()
                    .any(|c| c.class_name == "UsableComponent")
        })
        .map(|(i, _)| i)
        .context("Original placed usable missing")?;
    let ci = w.entities[owner]
        .components
        .iter()
        .position(|c| c.class_name == "UsableComponent")
        .unwrap();
    let name = w.entities[owner].name.clone();
    w.outputs.remove_owner(owner);
    w.retire_deferred_output_events(owner);
    let fields = [
        ("Output", serde_json::json!(["OnUse"])),
        ("Input", serde_json::json!(["UseProbe"])),
        ("Target", serde_json::json!(["_self"])),
        ("Parameters", serde_json::json!(["_activator"])),
        ("DelayWait", serde_json::json!([0.1])),
        ("IoFlags", serde_json::json!([0])),
        ("TargetGroup", serde_json::json!([""])),
    ];
    let io = Component::new(
        "InputOutputComponent".into(),
        "Probe".into(),
        ID::new(0),
        ID::new(0),
        ID::new(0),
        fields
            .into_iter()
            .map(|(n, v)| Property::new(String::new(), n.into(), false, false, false, v))
            .collect(),
    );
    w.outputs.bind(owner, &name, &[io])?;
    w.bind_deferred_output_events(owner)?;
    let once = w.entities[owner].components[ci]
        .properties
        .iter_mut()
        .find(|p| p.name == "UsableOnce")
        .context("Resolved UsableOnce missing")?;
    once.value = true.into();
    w.run_callback_fixture("UseCalls=0; function UseProbe(target,activator) assert(activator==GetPlayer()); assert(_activator()==GetPlayer()); UseCalls=UseCalls+1 end; assert(OnUse==14)")?;
    w.use_usable(owner)?;
    ensure!(
        w.component_control.disabled[&owner][&ci],
        "Use-once did not share component disable state"
    );
    w.run_callback_fixture("assert(UseCalls==0)")?;
    w.process_deferred_entity_events()?;
    w.run_callback_fixture("assert(UseCalls==0)")?;
    w.dispatch_actor_inputs(w.animation_time + 0.11);
    w.run_callback_fixture("assert(UseCalls==1)")?;
    ensure!(
        w.use_usable(owner).is_err(),
        "Disabled usable admitted a second use"
    );
    w.run_callback_fixture(&format!("EnableComponent(GetEntity({name:?}),'Usable')"))?;
    w.use_usable(owner)?;
    w.process_deferred_entity_events()?;
    w.dispatch_actor_inputs(w.animation_time + 0.11);
    w.run_callback_fixture("assert(UseCalls==2)")?;
    w.run_callback_fixture(&format!("EnableComponent(GetEntity({name:?}),'Usable')"))?;
    for i in 0..w.entities.len() {
        if i != owner {
            if let Some(c) = w.entities[i]
                .components
                .iter()
                .position(|c| c.class_name == "UsableComponent")
            {
                w.set_component_disabled(i, c, true)?;
            }
        }
    }
    w.cutscene_input = Default::default();
    w.demo_story.dialogue = None;
    w.entities[owner].components[ci]
        .properties
        .retain(|p| p.name != "UsableRadius");
    let point = w.entities[owner].transform[12..15].try_into()?;
    ensure!(
        w.usable_target(point).is_none(),
        "Omitted radius invented an interaction range"
    );
    w.run_callback_fixture(&format!(
        "SetPropertyFloat(GetEntity({name:?}),'UsableRadius',3.5)"
    ))?;
    ensure!(
        w.usable_target(point) == Some(owner),
        "Live radius write did not reach selection"
    );
    ensure!(
        w.deferred_event_runtime.use_io_deliveries == 2,
        "Use event delivery count incorrect"
    );
    w.outputs.remove_owner(owner);
    w.retire_deferred_output_events(owner);
    w.entities[owner]
        .components
        .retain(|c| c.class_name != "ConversationComponent");
    let before = w.deferred_event_runtime.processed;
    w.use_usable(owner)?;
    w.process_deferred_entity_events()?;
    ensure!(
        w.deferred_event_runtime.processed > before,
        "Usable without IO suppressed native event"
    );
    w.run_callback_fixture("assert(UseCalls==2)")?;
    println!(
        "PASS: original usable owner queues OnUse with player activator, respects delayed IO, disables once, re-enables through native component API and uses reflected radius/default0"
    );
    Ok(())
}
