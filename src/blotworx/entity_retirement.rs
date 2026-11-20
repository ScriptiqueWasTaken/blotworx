//! Live deferred cleanup admission and ChildComponent preparation.
//! Full component readiness/teardown, scene-manager phase and refcounts remain open.
use super::*;
impl World {
    pub(in crate::blotworx) fn cleanup_snapshot(&self)->serde_json::Value {
        serde_json::json!({"pending":self.cleanup_queue.len(),"preparing":self.cleanup_preparing.len(),"completed":self.cleanup_completed,"native_full_lifecycle_verified":false})
    }
    pub(in crate::blotworx) fn queue_entity_retirement(&mut self,entity:usize)->Result<bool> {
        let Some(owner)=self.entities.get(entity) else {return Ok(false)};
        if owner.scene.is_empty() || self.removed_entities.contains(&entity) || self.cleanup_queue.contains(entity) {return Ok(false);}
        ensure!(self.cleanup_preparing.insert(entity),"Cyclic owned-child cleanup preparation at {}",self.entities[entity].name);
        let result=(||->Result<bool> {
            let count=self.entities[entity].components.len();
            ensure!(self.entities[entity].components.iter().filter(|component|component.class_name=="ChildComponent").count()<=1,"Multiple ChildComponent preparation requires per-component live slots");
            // Base1edecc snapshots count and invokes preparation in forward order.
            for index in 0..count {
                match self.entities[entity].components[index].class_name.as_str() {
                    "ChildComponent"=> {
                        //311a44 reads declared/live slots in authored order, retaining
                        //each valid child around a recursive manager enqueue.
                        for child in self.owned_retirement_children(entity)?.into_iter().flatten() {
                            if !self.removed_entities.contains(&child) {self.queue_entity_retirement(child)?;}
                        }
                    }
                    "FXComponent"=> {
                        if let Some(effect)=self.named_fx.live.get_mut(&entity) {effect.begin_removal(true)?;}
                    }
                    _=>{}
                }
            }
            Ok(self.cleanup_queue.append_prepared(entity))
        })();
        self.cleanup_preparing.remove(&entity);
        let admitted=result?;
        let limitation="Deferred cleanup connects scene/duplicate admission, ChildComponent preparation, named-FX draining, event44 and native swap-tail order to existing resource retirement. Full component hooks/readiness, scene-manager phase, scene lists/hash removal and native reference reclamation remain incomplete.";
        if admitted && !self.report.unsupported.iter().any(|line|line==limitation) {self.report.unsupported.push(limitation.into());}
        Ok(admitted)
    }
    pub(in crate::blotworx) fn process_entity_cleanup(&mut self)->Result<()> {
        let mut cursor=0;
        while let Some(entity)=self.cleanup_queue.at(cursor) {
            if self.removed_entities.contains(&entity) {self.cleanup_queue.remove_at(cursor);continue;}
            if self.named_fx.live.get(&entity).is_some_and(|effect|!effect.ready) {cursor+=1;continue;}
            //1edf44 queues kind44 before component/resource teardown. Use the
            //real deferred event queue; receivers lacking subscriptions diagnose.
            let event=super::super::entity_events::Basic{event_kind:0x44,source:Some(entity),header:None}.into();
            self.entity_event_queue.enqueue_with(&event,None,&mut |_,_|anyhow::bail!("Basic cleanup event must not compare auxiliary references"))?;
            self.retire_entity(entity)?;
            if self.removed_entities.contains(&entity) {
                self.cleanup_completed=self.cleanup_completed.wrapping_add(1);
                //181d34 swaps the tail into the removed slot and revisits it.
                self.cleanup_queue.remove_at(cursor);
            } else {cursor+=1;}
        }
        Ok(())
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate_owned_cleanup()->Result<()> {
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
    let mut world=load(Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
    world.enable_demo_story()?;
    ensure!(world.cleanup_queue.len()==0,"Scenario must start without pending cleanup");
    let parent=world.entities.iter().position(|e|e.name=="gremlinvillage_zonea.Breakable Barrel Health 03").context("Parent barrel missing")?;
    let child=world.entities.iter().position(|e|e.name=="gremlinvillage_zonea.Breakable Barrel Ammo 01").context("Child barrel missing")?;
    let position=world.entities[parent].transform[12..15].try_into()?;
    let rotation=[1.,0.,0.,0.,1.,0.,0.,0.,1.];
    let grandchild=world.spawn_palette_root(parent,"eticket_e_parentable",position,rotation,"")?;
    let observer=world.spawn_palette_root(parent,"eticket_e_parentable",position,rotation,"")?;
    world.change_transform_parent(child,Some(parent))?;
    world.change_transform_parent(grandchild,Some(child))?;
    world.run_callback_fixture("assert(select('#',DestroyEntity('gremlinvillage_zonea.Breakable Barrel Health 03'))==0); assert(IsValidHandle(GetEntity('gremlinvillage_zonea.Breakable Barrel Health 03'))); DestroyEntity('gremlinvillage_zonea.Breakable Barrel Health 03')")?;
    ensure!((0..world.cleanup_queue.len()).filter_map(|i|world.cleanup_queue.at(i)).collect::<Vec<_>>()==[grandchild,child,parent],"Owned children were not prepared before parent publication, or duplicate admission failed");
    ensure!([grandchild,child,parent].iter().all(|entity|!world.removed_entities.contains(entity)),"Preparation destroyed an entity synchronously");
    world.process_entity_cleanup()?;
    ensure!(world.cleanup_queue.len()==0 && world.cleanup_completed==3,"Owned cleanup did not finish exactly three owners");
    let events=world.entity_event_queue.entries().iter().filter_map(|entry|match &entry.event {super::super::entity_events::Event::Basic(event) if event.event_kind==0x44=>event.source,_=>None}).collect::<Vec<_>>();
    ensure!(events==[grandchild,parent,child],"Cleanup did not use native swap-tail order: {events:?}");
    for entity in [grandchild,child,parent] {
        ensure!(world.removed_entities.contains(&entity),"Owned child survived source cleanup");
        ensure!(!world.actors.contains_key(&entity) && !world.pickup_receivers.contains_key(&entity),"Retired owned entity retained animation/pickup lifecycle");
        ensure!(world.entities[entity].draws.clone().all(|draw|world.removed_draws.contains(&draw)),"Retired owned entity retained draws");
    }
    ensure!(!world.removed_entities.contains(&observer) && world.pickup_receivers.contains_key(&observer),"Unowned neighboring pickup was destroyed");
    ensure!(!world.queue_entity_retirement(parent)?,"Retired owner reentered queue");
    std::fs::write("target/owned-cleanup-validation.json",serde_json::to_vec_pretty(&serde_json::json!({"parent":parent,"child":child,"grandchild":grandchild,"observer":observer,"preparation_publication":[grandchild,child,parent],"finish_events":events,"cleanup":world.cleanup_snapshot(),"full_native_lifecycle_verified":false}))?)?;
    println!("PASS: real Lua DestroyEntity prepares original owned barrel/ticket graph without synchronous removal, suppresses duplicates, retires all owned resources in native swap-tail order and preserves unowned pickup");
    Ok(())
}
