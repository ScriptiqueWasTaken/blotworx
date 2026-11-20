//! Live RobotArmGeneral task/timer driver. Concrete virtuals are from the
//! retail vtable4d9b7c; mixed behavior lists remain explicitly unsupported.
use super::{assets::World,robot_arm::{Runtime,AttackType},robot_arm_behavior::Host};
use anyhow::{Context as _,ensure,Result};
impl World {
    pub(super) fn robot_arm_slot(&self,owner:usize)->Option<usize> {
        self.ai_brains.get(&owner)?.behaviors.iter().position(|name|name=="RobotArmGeneral")
    }
    pub(super) fn robot_arm_timer(&mut self,owner:usize,value:f32)->Result<()> {
        let slot=self.robot_arm_slot(owner).context("Arm behavior absent")?;
        self.ai_brains.get_mut(&owner).unwrap().behavior_tasks.get_mut(&slot).context("Arm common task absent")?.timer=value;Ok(())
    }
    pub(super) fn robot_arm_request_changed(&mut self,owner:usize)->Result<()> {
        let Some(slot)=self.robot_arm_slot(owner) else {return Ok(())};
        match self.robot_arm_controls[&owner].phase {
            1=>{if self.entities[owner].components.iter().any(|c|c.class_name=="AnimationComponent") {self.send_native_actor_animation_event(owner,0xc1)?;}},
            2=>self.robot_arm_controls.get_mut(&owner).unwrap().current_attack_type=-1,
            _=>{},
        }
        self.ai_brains.get_mut(&owner).unwrap().behavior_tasks.get_mut(&slot).unwrap().timer=0.;Ok(())
    }
    pub(super) fn finish_robot_arm_smash(&mut self,owner:usize)->Result<()> {
        //5f61c resolves the weak destination afresh, before publishing phase6.
        let target=self.robot_arm_controls[&owner].smash_request.as_ref().and_then(|r|r.destination)
            .filter(|target|!self.removed_entities.contains(target));
        let Some(target)=target else {self.robot_arm_controls.get_mut(&owner).unwrap().phase=7;return Ok(())};
        self.robot_arm_controls.get_mut(&owner).unwrap().phase=6;
        if self.entities[target].components.iter().any(|c|c.class_name=="AnimationComponentGB") {
            self.request_actor_sequence(target,"Break")?;
        }
        self.robot_arm_timer(owner,0.)
    }
    pub(super) fn advance_robot_arm_ai(&mut self,delta:f32)->Result<()> {
        if self.ai_manager_enabled==Some(false) {return Ok(())}
        let owners=self.robot_arm_controls.keys().copied().collect::<Vec<_>>();
        for owner in owners {
            if self.removed_entities.contains(&owner)||!self.entity_scene_active(owner) {continue;}
            let disabled=self.entities[owner].components.iter().enumerate().any(|(i,c)|c.class_name=="AIBrainComponent" && self.component_control.disabled.get(&owner).and_then(|flags|flags.get(&i))==Some(&true));
            if disabled {continue;}
            let Some(slot)=self.robot_arm_slot(owner) else {continue};
            ensure!(self.ai_brains[&owner].behaviors.len()==1,"RobotArmGeneral mixed behavior selection requires its other concrete virtuals");
            super::ai_brain_update::advance_live(
                super::ai_task_runtime::Context{owner,delta},
                &mut Driver{world:self,owner,slot})?;
        }
        Ok(())
    }
}
/// Full common selector/update receiver over the real World. No selector,
/// lifecycle, behavior task or RNG is removed while a callback runs.
struct Driver<'a>{world:&'a mut World,owner:usize,slot:usize}
impl Driver<'_>{
    fn brain(&self)->Result<&super::ai_behavior_events::Brain>{self.world.ai_brains.get(&self.owner).context("Arm callback retired its canonical brain")}
    fn brain_mut(&mut self)->Result<&mut super::ai_behavior_events::Brain>{self.world.ai_brains.get_mut(&self.owner).context("Arm callback retired its canonical brain")}
    fn require(&self,b:usize)->Result<()>{ensure!(b==self.slot,"Unknown arm behavior receiver");Ok(())}
    fn clear_child(&mut self,b:usize)->Result<()>{self.require(b)?;self.brain_mut()?.behavior_tasks.get_mut(&b).context("Arm common task absent")?.child=None;Ok(())}
    fn finish(&mut self,b:usize)->Result<()>{
        self.require(b)?;
        if let Some(child)=self.brain()?.behavior_tasks.get(&b).context("Arm common task absent")?.child{
            self.world.stop_ai_child(self.owner,child)?;
            if let Some(child)=self.brain()?.behavior_tasks.get(&b).context("Arm common task absent")?.child{self.world.destroy_ai_child(self.owner,child)?;}
            self.clear_child(b)?;
        }
        //RobotArmGeneral slot50=5d73c, an actual empty function.
        self.notify_finished(b)
    }
    fn notify_finished(&mut self,b:usize)->Result<()>{
        let name=self.brain()?.behaviors.get(b).context("Arm behavior name absent")?.clone();
        self.world.entity_event_queue.enqueue(&super::entity_events::Event::BehaviorFinished(super::entity_events::BehaviorFinished{source:Some(self.owner),header:None,name}),None)?;Ok(())
    }
}
impl super::ai_selection::LiveHost for Driver<'_>{
    fn selector(&self)->Result<&super::ai_selection::Selector>{Ok(&self.brain()?.selector)}
    fn selector_mut(&mut self)->Result<&mut super::ai_selection::Selector>{Ok(&mut self.brain_mut()?.selector)}
}
impl super::ai_selection::LifecycleHost for Driver<'_>{
    fn lifecycle(&self)->Result<&super::ai_selection::Lifecycle>{Ok(&self.brain()?.selection_lifecycle)}
    fn lifecycle_mut(&mut self)->Result<&mut super::ai_selection::Lifecycle>{Ok(&mut self.brain_mut()?.selection_lifecycle)}
    fn lifecycle_random(&mut self)->Result<i32>{Ok(self.world.game_random.draw() as i32)}
    fn selection_phase(&mut self)->Result<()>{super::ai_selection::choose_live(self)?;Ok(())}
    fn selection_running(&self)->Result<bool>{Ok(self.brain()?.selector.updating)}
    fn set_selection_running(&mut self,value:bool)->Result<()>{self.brain_mut()?.selector.updating=value;Ok(())}
}
impl super::ai_selection::Host for Driver<'_>{
    fn score_20(&mut self,b:usize)->Result<i32>{self.require(b)?;Ok(13)} //5cfe4
    fn current_gate_0c(&mut self,b:usize)->Result<bool>{self.require(b)?;Ok(false)} //12e630
    fn candidate_gate_24(&mut self,b:usize)->Result<bool>{self.require(b)?;Ok(true)} //5d13c
    fn candidate_gate_28(&mut self,b:usize)->Result<bool>{self.require(b)?;Ok(false)} //12e648
    fn current_gate_2c(&mut self,b:usize)->Result<bool>{self.require(b)?;Ok(false)} //12e650
    fn leave_and_notify(&mut self,b:usize)->Result<()>{self.finish(b)}
    fn enter_4c(&mut self,b:usize,_:bool)->Result<()>{
        self.require(b)?;let task=self.brain_mut()?.behavior_tasks.get_mut(&b).context("Arm entry task absent")?;
        task.mode=0;task.timer=0.;Ok(()) //5d144 blr
    }
    fn random(&mut self)->Result<i32>{Ok(self.world.game_random.draw() as i32)}
}
impl super::ai_brain_update::LiveHost for Driver<'_>{
    fn selector(&self)->Result<&super::ai_selection::Selector>{Ok(&self.brain()?.selector)}
    fn selector_mut(&mut self)->Result<&mut super::ai_selection::Selector>{Ok(&mut self.brain_mut()?.selector)}
    fn lifecycle(&self)->Result<&super::ai_selection::Lifecycle>{Ok(&self.brain()?.selection_lifecycle)}
    fn lifecycle_mut(&mut self)->Result<&mut super::ai_selection::Lifecycle>{Ok(&mut self.brain_mut()?.selection_lifecycle)}
    fn update_allowed(&mut self,b:usize,o:usize)->Result<bool>{self.require(b)?;ensure!(o==self.owner,"Arm update owner differs");Ok(true)} //12e640
    fn update_current(&mut self,b:usize,c:super::ai_task_runtime::Context)->Result<u32>{self.require(b)?;ensure!(c.owner==self.owner,"Arm update owner differs");self.world.update_ai_behavior_task(self.owner,b,c.delta,update,completed)}
    fn child(&mut self,b:usize)->Result<Option<usize>>{self.require(b)?;Ok(self.brain()?.behavior_tasks.get(&b).context("Arm common task absent")?.child)}
    fn stop_child(&mut self,b:usize,t:usize,o:usize)->Result<()>{self.require(b)?;ensure!(o==self.owner,"Arm child owner differs");self.world.stop_ai_child(o,t)}
    fn destroy_child(&mut self,b:usize,t:usize)->Result<()>{self.require(b)?;self.world.destroy_ai_child(self.owner,t)}
    fn clear_child(&mut self,b:usize)->Result<()>{Driver::clear_child(self,b)}
    fn exit(&mut self,b:usize,_:super::ai_task_runtime::Context)->Result<()>{self.require(b)} //5d73c blr
    fn owner_entity(&mut self,o:usize)->Result<Option<usize>>{ensure!(o==self.owner,"Arm source owner differs");Ok(Some(o))}
    fn name(&mut self,b:usize)->Result<String>{self.require(b)?;Ok(self.brain()?.behaviors[b].clone())}
    fn enqueue_finished(&mut self,e:super::entity_events::BehaviorFinished)->Result<()>{self.world.entity_event_queue.enqueue(&super::entity_events::Event::BehaviorFinished(e),None)?;Ok(())}
    fn selection_requested(&mut self,o:usize)->Result<bool>{ensure!(o==self.owner,"Arm selection owner differs");Ok(self.brain()?.reevaluation_requested)}
    fn select(&mut self,o:usize)->Result<()>{ensure!(o==self.owner,"Arm selection owner differs");super::ai_selection::execute_live(self)?;self.brain_mut()?.reevaluation_requested=false;Ok(())}
    fn background_active(&mut self,b:usize)->Result<bool>{self.require(b)?;Ok(self.brain()?.background_behaviors.contains(&b))}
    fn background_update(&mut self,b:usize,_:super::ai_task_runtime::Context)->Result<()>{self.require(b)} //12e664 blr
}

fn update(world:&mut World,slot:usize,context:super::ai_task_runtime::Context)->Result<u32> {
    super::robot_arm_behavior::update(&mut Binding {world,owner:context.owner,slot})
}
fn completed(_: &mut World,_:usize,_:super::ai_task_runtime::Context)->Result<()> {Ok(())} //12e670 blr
struct Binding<'a> {world:&'a mut World,owner:usize,slot:usize}
impl Binding<'_> {
    fn state(&self)->&Runtime {&self.world.robot_arm_controls[&self.owner]}
    fn state_mut(&mut self)->&mut Runtime {self.world.robot_arm_controls.get_mut(&self.owner).unwrap()}
    fn attack(&self,kind:i32)->Result<&AttackType> {self.state().attack_types.get(usize::try_from(kind)?).context("Arm attack index outside native array")}
    fn speed(&self,tracking:bool)->Result<f32> {
        let key=if tracking {&self.state().tracking_speed_key}else{&self.state().sweep_speed_key};
        let key=key.as_ref().context("Arm speed attribute unbound")?;
        super::config_value::decimal_float(self.world.runtime_settings.get(key).context("Arm speed attribute disappeared")?).map_err(anyhow::Error::msg)
    }
    fn positions(&self)->Result<([f32;3],[f32;3])> {
        let player=self.world.scripts.current_player_entity()?.context("Arm target player absent")?;
        Ok((self.world.entities[self.owner].transform[12..15].try_into()?,self.world.entities[player].transform[12..15].try_into()?))
    }
    fn motor_enabled(&mut self,entity:usize,value:bool)->Result<()> {
        if let Some(component)=self.world.entities[entity].components.iter().position(|c|c.class_name=="RotatorComponent") {self.world.set_component_disabled(entity,component,!value)?;}Ok(())
    }
}
impl Host for Binding<'_> {
    fn phase(&self)->Result<u32> {Ok(self.state().phase)}
    fn set_phase(&mut self,value:u32)->Result<()> {self.state_mut().phase=value;Ok(())}
    fn smash_type(&self)->Result<i32> {Ok(self.state().smash_request.as_ref().map_or(-1,|r|r.attack_type))}
    fn smash_angle(&self)->Result<f32> {Ok(self.state().smash_request.as_ref().map_or(0.,|r|r.angle_degrees))}
    fn clear_smash(&mut self)->Result<()> {if let Some(r)=self.state_mut().smash_request.as_mut() {r.attack_type=-1;r.destination=None;}Ok(())}
    fn electro_type(&self)->Result<i32> {Ok(self.state().electrocute_request.as_ref().map_or(-1,|r|r.attack_type))}
    fn clear_electro_type(&mut self)->Result<()> {if let Some(r)=self.state_mut().electrocute_request.as_mut() {r.attack_type=-1;}Ok(())}
    fn hold_electrocution(&self)->Result<bool> {Ok(self.state().electrocute_request.as_ref().is_some_and(|r|r.enabled))}
    fn current_attack(&self)->Result<i32> {Ok(self.state().current_attack_type)}
    fn set_current_attack(&mut self,value:i32)->Result<()> {self.state_mut().current_attack_type=value;Ok(())}
    fn angle(&self,kind:i32)->Result<f32> {Ok(self.attack(kind)?.angle_degrees)}
    fn delay(&self,kind:i32)->Result<f32> {Ok(self.attack(kind)?.delay_seconds)}
    fn cooldown(&self,kind:i32)->Result<f32> {Ok(self.attack(kind)?.cooldown_seconds)}
    fn set_timer(&mut self,value:f32)->Result<()> {self.world.robot_arm_timer(self.owner,value)}
    fn can_attack(&mut self)->Result<bool> {let (source,target)=self.positions()?;let r=self.world.entities[self.owner].runtime_rotation;super::robot_arm_target::can_attack(source,target,[r[2],r[5],r[8]])}
    fn choose_attack(&mut self)->Result<i32> {let state=self.world.robot_arm_controls.get_mut(&self.owner).unwrap();super::robot_arm_target::choose(state.enabled_mask,&mut state.maximum_attack_type,||Ok(self.world.game_random.draw() as i32))}
    fn root_enabled(&mut self,value:bool)->Result<()> {self.motor_enabled(self.owner,value)}
    fn turn(&mut self,degrees:f32,speed:f32)->Result<()> {self.world.start_robot_arm_turn(self.owner,degrees,speed)}
    fn tracking_speed(&mut self)->Result<f32> {self.speed(true)}
    fn sweep_speed(&mut self)->Result<f32> {self.speed(false)}
    fn player_angle(&mut self)->Result<f32> {let (source,target)=self.positions()?;super::robot_arm_target::player_angle(source,target)}
    fn lock_eyes(&mut self)->Result<()> {
        let beam=self.state().spotlight_entity.filter(|e|!self.world.removed_entities.contains(e));
        if let Some(beam)=beam {if let Some(parent)=self.world.current_transform_parent(beam)? {
            self.motor_enabled(parent,false)?;self.world.instant_rotator_position(parent,0.)?;
            if let Some(grandparent)=self.world.current_transform_parent(parent)? {self.motor_enabled(grandparent,false)?;self.world.instant_rotator_position(grandparent,0.)?;}
        }}Ok(())
    }
    fn reset_attack_clock(&mut self)->Result<()> {let ticks=self.world.frame_clock.clock.scaled_ticks;self.state_mut().attack_clock.reset(ticks);Ok(())}
    fn attack_elapsed(&mut self)->Result<f32> {let stamp=self.state().attack_clock;Ok(if stamp.is_running() {self.world.frame_clock.clock.rate.seconds(stamp.elapsed_ticks(self.world.frame_clock.clock.scaled_ticks))}else{0.})}
    fn spotlight(&mut self,value:i32)->Result<()> {self.world.apply_robot_arm_spotlight(self.owner,value,self.state().spotlight.disabled)}
    fn has_animation(&mut self)->Result<bool> {Ok(self.world.entities[self.owner].components.iter().any(|c|c.class_name=="AnimationComponent"))}
    fn attack_variable(&mut self,value:i32)->Result<()> {self.world.write_animation_variable(self.owner,0x16,1,value as u32)?;Ok(())}
    fn animation_wait(&mut self,start:u32,done:u32,timeout:f32,wait:bool)->Result<()> {
        let id=self.world.create_ai_animation_wait(self.owner,super::ai_animation_wait::Definition {start_event:start,done_event:done,timeout,apply_heading:false,heading:-1.,agent_40:true})?;
        self.world.install_ai_child(self.owner,self.slot,id,wait)
    }
    fn scripted_audio(&mut self)->Result<()> {let player=self.world.scripts.current_player_entity()?.context("Arm break audio player missing")?;self.world.post_audio_from_game(player,"Play_sfx_DBC_Evnt_RobotArm_Break_Walkway");Ok(())}
    fn finish_smash(&mut self)->Result<()> {self.world.finish_robot_arm_smash(self.owner)}
    fn break_impact(&mut self)->Result<()> {self.world.script_feedback.shake([4.,5.,0.05,4.,0.02,0.045])?;self.world.script_feedback.rumble(0,0.5,self.world.game_options.rumble!=0);Ok(())}
    fn has_child(&self)->Result<bool> {Ok(self.world.ai_brains[&self.owner].behavior_tasks[&self.slot].child.is_some())}
    fn restore_turn(&mut self)->Result<()> {self.world.restore_robot_arm_turn(self.owner)}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_arm_queues_turns_breaks_and_restores_tracking() -> Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.Level",&egui::Context::default())?;
        let name="dbc_1st_zonea_dynamic.Robot_Arm";
        let owner=world.scripts.resolve_name(name).context("Original arm absent")?;
        let target="dbc_1st_zonea_dynamic.DBC_ZoneD_Break_02 01";
        let target_id=world.scripts.resolve_name(target).context("Original breakable walkway absent")?;
        world.advance_robot_arm_ai(1./30.)?;
        let slot=world.robot_arm_slot(owner).unwrap();
        ensure!(world.ai_brains[&owner].selector.current==Some(slot),"Original arm never selected");
        world.run_callback_fixture(&format!("RequestRobotArmSmash({name:?},44,{target:?},0)"))?;
        ensure!(world.robot_arm_controls[&owner].phase==0 && world.ai_brains[&owner].behavior_tasks[&slot].child.is_none(),"Lua request bypassed the scheduler");
        world.advance_robot_arm_ai(1./30.)?;
        ensure!(world.robot_arm_controls[&owner].phase==4,"Queued turn did not enter phase4");
        ensure!(world.ai_brains[&owner].behavior_tasks[&slot].timer.to_bits()==1.1f32.to_bits(),"Turn fallback delay differs");
        let beam=world.robot_arm_controls[&owner].spotlight_entity;
        if let Some(beam)=beam {if let Some(parent)=world.current_transform_parent(beam)? {ensure!(world.rotators.iter().filter(|((entity,_),_)|*entity==parent).all(|(_,r)|r.disabled && r.state.angle==0.),"Eye motors not locked at neutral");}}
        //The actual ordinary dispatcher must reject another owner's completion.
        for (source,expected) in [(Some(target_id),1.1f32),(Some(owner),0.)] {
            world.deferred_event_runtime.dispatch_ordinary_output(&mut world.entity_event_registry,
                super::super::entity_events::Basic {event_kind:0x0d,source,header:None},
                &mut world.outputs,&world.entities,&world.scene_resources,world.animation_time,
                &mut world.ai_brains,&world.robot_arm_controls,&world.cutscene_actions)?;
            ensure!(world.ai_brains[&owner].behavior_tasks[&slot].timer.to_bits()==expected.to_bits(),"Reached-position source/timer incorrect");
        }
        world.advance_robot_arm_ai(1./30.)?;
        ensure!(world.robot_arm_controls[&owner].phase==5,"Turn completion never started smash animation");
        let child=world.ai_brains[&owner].behavior_tasks[&slot].child.context("Smash child not constructed")?;
        let task=&world.ai_brains[&owner].animation_waits[&child];
        ensure!(task.subscribed && task.task.definition.start_event==0x74 && task.task.definition.timeout==60.,"Smash wait not bound to actual controller feedback");
        ensure!(world.ai_brains[&owner].behavior_tasks[&slot].mode==0,"Scripted smash incorrectly blocks phase5 impact handling");
        world.receive_robot_arm_animation(owner,0xce)?;
        ensure!(world.robot_arm_controls[&owner].phase==6,"Impact feedback did not resolve the walkway");
        ensure!(world.actors.get(&target_id).is_some_and(|actor|actor.requested_id()==Some(world.actor_sequences[&target_id].find_name("Break"))),"Walkway Break was not submitted to its live animation component");
        world.advance_robot_arm_ai(1./30.)?;
        ensure!(world.robot_arm_controls[&owner].phase==7,"Break impact not advanced");
        world.ai_brains.get_mut(&owner).unwrap().receive_animation_feedback(owner,&super::super::entity_events::Animation {source:Some(owner),header:None,id:0xc1,flag:1})?;
        world.receive_robot_arm_animation(owner,0xc1)?;
        world.advance_robot_arm_ai(1./30.)?;
        ensure!(world.robot_arm_controls[&owner].phase==8 && world.ai_brains[&owner].behavior_tasks[&slot].child.is_none(),"Completed animation retained its child or blocked cleanup");
        world.advance_robot_arm_ai(1./30.)?;
        ensure!(world.robot_arm_controls[&owner].phase==0 && world.robot_arm_controls[&owner].smash_request.as_ref().unwrap().attack_type==-1,"Smash request never cleared");
        let rotator=world.rotators.iter().find(|((entity,_),_)|*entity==owner).unwrap().1;
        ensure!(!rotator.rotate_to_position && rotator.entity_target==Some(world.scripts.current_player_entity()?),"Completed smash did not restore player tracking");
        Ok(())
    }
}
