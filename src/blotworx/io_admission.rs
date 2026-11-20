//! Retail328c70..328d2c IO admission regression through the real World dispatcher.
#[cfg(debug_assertions)]
pub(super) fn validate()->anyhow::Result<()> {
    use anyhow::{Context,ensure};
    use super::actor_io::{Connection,Delivery};
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut w=super::assets::load(std::path::Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
    let owner=w.player_entity;
    let row=|input:&str,target:&str|Delivery {connection:Connection {instigator:None,target_guid:None,owner,input:input.into(),output:"OnUser1".into(),target:target.into(),parameters:"_self".into(),delay:0.,flags:0,group:String::new()},activator:None,payload:None};
    w.run_callback_fixture("IoVisits=0; IoAbsent=nil; IoFalse=false; IoNumber=42; IoText='abc'; IoTable=setmetatable({},{__call=function() error('Native IO must not call tables') end}); function IoValid(target,owner) assert(target==nil); assert(owner==GetPlayer()); IoVisits=IoVisits+1 end; function IoBroken() error('IO_EXECUTION_FAILED') end")?;
    w.run_callback_fixture("assert(type(RemovePhysicsFromWorld)=='function'); assert(select('#',RemovePhysicsFromWorld(nil))==0)")?;
    let errors=w.report.unsupported.len();
    for name in ["IoAbsent","IoFalse","IoNumber","IoText","IoTable"] {
        w.dispatch_actor_deliveries(vec![row(name,"_target_group")]);
    }
    ensure!(w.report.unsupported.len()==errors,"Noncallable IO became a failed script");
    ensure!(w.report.warnings.iter().filter(|s|s.contains("retail callable-type gate")).count()>=5,"Skipped IO not diagnosed");
    w.dispatch_actor_deliveries(vec![row("IoValid","BwxMissingTarget")]);
    w.run_callback_fixture("assert(IoVisits==1); function IoAbsent(target,owner) assert(target==GetPlayer()); assert(owner==GetPlayer()); IoVisits=IoVisits+1 end")?;
    w.dispatch_actor_deliveries(vec![row("IoAbsent","_self")]);
    w.run_callback_fixture("assert(IoVisits==2)")?;
    w.dispatch_actor_deliveries(vec![row("IoBroken","_self")]);
    ensure!(w.report.unsupported.iter().skip(errors).any(|s|s.contains("IO_EXECUTION_FAILED")),"Executed script failure was hidden");
    println!("PASS: retail IO callable-type gate, no target expansion for noncallables, nil target/source parameter, late function definition and real execution errors");
    drop(w);

    let mut world = super::assets::load_with_bootstrap_mode(
        std::path::Path::new(&root),
        "Levels/GV_ZoneD_Start.level",
        &egui::Context::default(),
        None,
        None,
        Default::default(),
        Default::default(),
        Default::default(),
        true,
        false,
    )?;
    let valve = world.scripts.resolve_name("gremlinvillage_zoned_dynamic.GV_FanGear01")
        .context("Missing authored first Zone D valve")?;
    let point = world.entities[valve].transform[12..15].try_into()?;
    let spin = world.report.spin_settings.as_ref().context("Missing authored spin settings")?;
    let (knockback, spin) = (spin.preview_value("KnockbackStim"), spin.preview_value("SpinStim"));
    world.run_callback_fixture(r#"
        local turn = GV_ZoneD_ValveTurned
        function GV_ZoneD_ValveTurned(valve, event, index)
            assert(DamageEvent_HasStimulusType(event, "ST_SPIN"))
            assert(DamageEvent_HasStimulusType(event, ST_KNOCKBACK))
            assert(not DamageEvent_HasStimulusType(event, ST_PHYSICAL))
            assert(DamageEvent_EffectiveAmount(event, "ST_SPIN") > 0)
            assert(DamageEvent_EffectiveAmount(event, ST_PHYSICAL) == 0)
            turn(valve, event, index)
        end
    "#)?;
    super::spin::dispatch_target(
        world.player_entity as u64, valve as u64, point, point,
        knockback, spin, "Spin",
        |event| world.receive_combat(valve, &event),
    )?;
    let since = world.report.unsupported.len();
    world.dispatch_actor_inputs(world.animation_time);
    ensure!(
        !world.report.unsupported[since..].iter().any(|error| error.starts_with("Authored IO")),
        "Zone D valve receiver failed: {:?}",
        &world.report.unsupported[since..],
    );
    world.run_callback_fixture("assert(GV_ZoneD_ValveActive == 2, 'Valve did not advance'); assert(TimerGetState() == 'Ticking', 'Timer did not start')")?;
    println!("PASS: authored Zone D valve receives grouped spin damage and starts its timer");
    Ok(())
}
