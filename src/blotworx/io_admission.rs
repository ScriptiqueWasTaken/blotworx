//! Retail328c70..328d2c IO admission regression through the real World dispatcher.
#[cfg(debug_assertions)]
pub(super) fn validate()->anyhow::Result<()> {
    use anyhow::{Context,ensure};
    use super::actor_io::{Connection,Delivery};
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut w=super::assets::load(std::path::Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
    let owner=w.player_entity;
    let row=|input:&str,target:&str|Delivery {connection:Connection {instigator:None,target_guid:None,owner,input:input.into(),output:"OnUser1".into(),target:target.into(),parameters:"_self".into(),delay:0.,flags:0,group:String::new()},activator:None};
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
    Ok(())
}
