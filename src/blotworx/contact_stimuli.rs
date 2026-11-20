//! Functional contact delivery for PhysicsStimulusComponent. Box3D contacts
//! replace the native Havok callbacks; callback ordering and radial origin
//! parity remain unverified. Rates are read live, including indexed IO writes.
use super::{assets::World, projectile_stimulus::{Delivery,Emission}};
use anyhow::{ensure,Context,Result};
use epic_mickey_lib_rs::scene_file::Component;
use std::collections::{BTreeMap,BTreeSet};
#[derive(Default,serde::Serialize)]
pub struct State {
    pub batches:u64,
    pub channels:u64,
    pub last_emitter:Option<String>,
    pub last_recipient:Option<String>,
    pub last_samples:Vec<(u32,f32,f32)>,
}
#[derive(Debug,PartialEq)]
pub(super) struct FloatInput<'a> {pub name:&'a str,pub value:f32,pub index:usize}
pub(super) fn parse_float_input(text:&str)->Result<FloatInput<'_>> {
    let mut words=text.split(',').map(str::trim);
    let name=words.next().filter(|s|!s.is_empty()).context("Float input needs a property")?;
    let value=words.next().context("Float input needs a value")?.parse::<f32>()?;
    ensure!(value.is_finite(),"Nonfinite float input");
    let index=words.next().map(str::parse::<usize>).transpose()?.unwrap_or(0);
    ensure!(words.next().is_none(),"Float input has surplus arguments");
    Ok(FloatInput{name,value,index})
}
fn samples(component:&Component,dt:f32,layer:u32)->Result<Vec<(u32,f32,f32)>> {
    ensure!(dt.is_finite() && dt>=0.,"Invalid stimulus elapsed time");
    let field=|name:&str|component.properties.iter().find(|p|p.name==name).map(|p|&p.value);
    let array=|name:&str|->Result<&[serde_json::Value]> {
        field(name).map(|v|v.as_array().map(Vec::as_slice).context("Invalid stimulus array"))
            .transpose().map(|v|v.unwrap_or(&[]))
    };
    let layers=array("Collidable Collision Layers")?;
    let per_layer=field("Stimulus Type Per Collision Layer").and_then(|v|v.as_bool()).unwrap_or(false);
    let admits=|v:&serde_json::Value|->Result<bool> {
        let selector=u32::try_from(v.as_u64().context("Invalid stimulus layer")?)?;
        Ok(layer<32 && super::trigger_damage::layer_mask(selector)&(1u32<<layer)!=0)
    };
    if !layers.iter().map(admits).collect::<Result<Vec<_>>>()?.into_iter().any(|v|v) {return Ok(vec![])}
    let types=array("StimulusTypeList")?;let rates=array("StimulusPerSecondList")?;
    let radii=array("StimulusRadiusList")?;
    if rates.is_empty() {return Ok(vec![])}
    let mut result=Vec::new();
    for (i,kind) in types.iter().enumerate() {
        if per_layer && !layers.get(i).map(admits).transpose()?.unwrap_or(false) {continue}
        let kind=u32::try_from(kind.as_u64().context("Invalid contact stimulus type")?)?;
        ensure!(kind<15,"Unknown contact stimulus type");
        let rate=rates[i.min(rates.len()-1)].as_f64().context("Invalid contact stimulus rate")? as f32;
        let amount=rate*dt;
        ensure!(rate.is_finite() && amount.is_finite(),"Nonfinite contact stimulus rate");
        let radius=radii.get(i).or_else(||radii.last()).map(|v|v.as_f64().context("Invalid contact stimulus radius"))
            .transpose()?.unwrap_or(0.) as f32;
        ensure!(radius.is_finite() && radius>=0.,"Invalid contact stimulus radius");
        if amount>0. {result.push((kind,amount,radius));}
    }
    Ok(result)
}
fn group_emissions(target:usize,emissions:Vec<Emission>,
    mut area:impl FnMut([f32;3],f32,Option<usize>)->Result<Vec<usize>>)
    ->Result<BTreeMap<usize,Vec<Emission>>> {
    let mut batches=BTreeMap::<usize,Vec<Emission>>::new();
    for emission in emissions {
        match emission.delivery {
            Delivery::Target|Delivery::Area{radius:0.,..}=>batches.entry(target).or_default().push(emission),
            Delivery::Area{radius,..}=>{
                let owners=area(emission.position,radius,emission.owner.map(usize::try_from).transpose()?)?
                    .into_iter().collect::<BTreeSet<_>>();
                for recipient in owners {batches.entry(recipient).or_default().push(emission.clone());}
            }
        }
    }
    Ok(batches)
}
impl World {
    pub(super) fn set_contact_float(&mut self,entity:usize,name:&str,index:usize,value:f32)->Result<()> {
        ensure!(!self.removed_entities.contains(&entity) && value.is_finite(),"Invalid indexed float target/value");
        ensure!(matches!(name,"StimulusPerSecondList"|"StimulusRadiusList"),"Unsupported indexed float {name}");
        let field=self.entities[entity].components.iter_mut().find(|c|c.class_name=="PhysicsStimulusComponent")
            .and_then(|c|c.properties.iter_mut().find(|p|p.name==name)).context("Contact float property missing")?;
        ensure!(field.class_name=="Float","Contact float property has wrong type");
        let item=field.value.as_array_mut().and_then(|v|v.get_mut(index)).context("Contact float index out of range")?;
        *item=serde_json::json!(value);
        Ok(())
    }
    pub(super) fn dispatch_io_float(&mut self,entity:usize,text:&str)->Result<()> {
        let input=parse_float_input(text)?;
        if matches!(input.name,"StimulusPerSecondList"|"StimulusRadiusList") {
            return self.set_contact_float(entity,input.name,input.index,input.value);
        }
        ensure!(input.index==0,"Scalar float input has nonzero index");
        self.set_entity_float_property(entity,input.name,input.value)
    }
    fn contact_entity(&self,owner:usize)->Option<usize> {
        let entity=self.collision_entity(owner)?;
        self.scripts.resolve_name(&entity.name).filter(|&i|
            self.entities[i].id==entity.id && self.entities[i].scene==entity.scene)
            .or_else(||self.entities.iter().position(|candidate|std::ptr::eq(candidate,entity)))
    }
    pub(super) fn advance_contact_stimuli(&mut self,dt:f32)->Result<()> {
        // Read active rates before querying physics. Disabled emitters neither
        // allocate contacts nor retain another copy of their reflected data.
        for emitter in 0..self.entities.len() {
            if self.removed_entities.contains(&emitter) || !self.entity_scene_active(emitter) {continue}
            let components=self.entities[emitter].components.iter().enumerate()
                .filter(|(i,c)|c.class_name=="PhysicsStimulusComponent" &&
                    !self.component_control.disabled.get(&emitter).and_then(|v|v.get(i)).copied().unwrap_or(false) &&
                    c.properties.iter().find(|p|p.name=="StimulusPerSecondList").and_then(|p|p.value.as_array())
                        .is_some_and(|v|v.iter().any(|v|v.as_f64().is_some_and(|v|v>0.))))
                .map(|(i,_)|i).collect::<Vec<_>>();
            if components.is_empty() {continue}
            let mut owners=BTreeSet::new();
            for part in self.entities[emitter].collision_parts.clone() {owners.insert(self.report.collision_parts[part].owner);}
            for (&source,&owner) in &self.dynamic_runtime_sources {
                if self.dynamic_bodies[source].entity==emitter {owners.insert(owner);}
            }
            if let Some(Ok(body))=self.character_physics.get(&emitter) {owners.insert(body.owner);}
            if emitter==self.player_entity {if let Some(body)=&self.player_physics {owners.insert(body.owner);}}
            let mut recipients=BTreeSet::new();
            for owner in owners {
                for shape in self.collision.owner_shapes(owner).collect::<Vec<_>>() {
                    for [a,b] in self.collision.touching_shape_pairs(shape)? {
                        let a_entity=self.collision.shape_owner(a).and_then(|o|self.contact_entity(o));
                        let b_entity=self.collision.shape_owner(b).and_then(|o|self.contact_entity(o));
                        let recipient=if a_entity==Some(emitter) {b_entity.map(|i|(i,b))}
                            else if b_entity==Some(emitter) {a_entity.map(|i|(i,a))}else{None};
                        if let Some((target,shape))=recipient.filter(|(target,_)|*target!=emitter) {recipients.insert((target,shape));}
                    }
                }
            }
            for index in components {
                // Multiple mesh triangles and compound parts must not multiply
                // a component's continuous per-second rate on one recipient.
                let mut delivered=BTreeSet::new();
                for &(target,shape) in &recipients {
                    if delivered.contains(&target) || self.removed_entities.contains(&target) {continue}
                    let layer=self.collision.simulation_filter_word(shape)?&31;
                    let component=&self.entities[emitter].components[index];
                    let samples=samples(component,dt,layer)?;
                    if samples.is_empty() {continue}
                    delivered.insert(target);
                    let alias=component.properties.iter().find(|p|p.name=="StimulusAlias")
                        .and_then(|p|p.value.as_str()).unwrap_or("").to_owned();
                    let position=self.entity_position_or_bound_center(target)?.context("Contact recipient absent")?;
                    let direction=self.emitter_stimulus_direction(emitter,target)?;
                    let emissions=samples.iter().map(|&(stimulus,amount,radius)|Emission{
                        stimulus,amount,position,direction,target:Some(target as u64),
                        owner:Some(emitter as u64),tag:alias.clone(),delivery:if radius==0. {Delivery::Target}
                        else{Delivery::Area{radius,persistent:false,projectile:emitter as u64}}
                    }).collect();
                    let batches=group_emissions(target,emissions,|position,radius,owner|
                        self.area_stimulus_recipients(position,radius,owner))?;
                    for (recipient,emissions) in batches {
                        self.receive_combat(recipient,&emissions)?;
                        self.contact_stimuli.batches+=1;
                        self.contact_stimuli.channels+=emissions.len() as u64;
                        self.contact_stimuli.last_emitter=Some(self.entities[emitter].name.clone());
                        self.contact_stimuli.last_recipient=Some(self.entities[recipient].name.clone());
                        self.contact_stimuli.last_samples=emissions.iter().map(|e|(e.stimulus,e.amount,
                            match e.delivery {Delivery::Target=>0.,Delivery::Area{radius,..}=>radius})).collect();
                    }
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_mad_doctor_brick_contact_delivers_damage_and_obeys_live_rate_disable()->Result<()> {
        use super::super::{controller::{Controller,Input,TickPhase},physics,STEP};
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),
            "Levels/2D_MadDoctor01.level",&egui::Context::default())?;
        world.enable_demo_story()?;
        world.attach_player_body()?;
        let brick=world.scripts.resolve_name("nos_hm_zonez.Brick_Prop").context("Original brick absent")?;
        let source=world.dynamic_bodies.iter().position(|b|b.entity==brick).context("Original brick body absent")?;
        let owner=*world.dynamic_runtime_sources.get(&source).context("Original brick body is not admitted")?;
        world.set_entity_motion_enabled(brick,false)?;
        let shape=world.dynamic_constructions[&source].bodies[0].shapes[0];
        let center=world.collision.simulation_pose(shape)?.center;
        let hit=world.collision.with_player_queries(||world.collision.ray_hits(
            physics::add(center,[0.,50.,0.]),[0.,-100.,0.]))?.into_iter()
            .filter(|hit|hit.owner==owner && hit.normal[1]>0.7)
            .max_by(|a,b|a.position[1].total_cmp(&b.position[1])).context("No original brick top surface")?;
        let mut player=world.take_startup_player();
        // Place the ACTUAL authored hull against this surface. The gameplay
        // origin is not necessarily its lowest point; assuming a foot offset
        // left the earlier fixture above the collider rather than touching it.
        let hull=world.report.player_collision.as_ref().context("Player hull absent")?;
        let minimum=hull.vertices.iter().map(|p|p.iter().zip(hit.normal).map(|(a,b)|a*b).sum::<f32>())
            .fold(f32::INFINITY,f32::min)-hull.radius;
        let spawn=physics::sub(hit.position,physics::mul(hit.normal,minimum+0.005));
        world.with_live_player(&mut player,|world| {
            world.relocate_player_pose(spawn,[1.,0.,0.,0.,1.,0.,0.,0.,1.],true)?;
            world.run_callback_fixture("local brick=GetEntity('nos_hm_zonez.Brick_Prop'); SetPropertyFloat(brick,'StimulusPerSecondList',0,0); SetPropertyFloat(brick,'StimulusPerSecondList',0.5,1); assert(GetPropertyFloat(brick,'StimulusPerSecondList',1)==0.5)")
        })?;
        let before=world.runtime_health.get(&world.player_entity).context("Player health absent")?.health;
        let mut controller=Controller::new(player);
        let advance=|world:&mut World,controller:&mut Controller|->Result<()> {
            controller.advance_with_tick_phases_and_camera(world,|w|&w.collision,1./30.,true,Input::default(),
                |_|Ok(Some([0.,0.,-1.])),|phase,player,_,_,_,world|->Result<()> {
                    match phase {
                        TickPhase::BeforeMovement=>world.publish_player_ground_record(player)?,
                        TickPhase::SimulatePhysics=>world.advance_player_physics(player,STEP)?,
                        TickPhase::AfterMovement=>{
                            world.animation_time+=STEP;
                            world.with_live_player(player,|w|w.advance_contact_stimuli(STEP))?;
                        }
                    }
                    Ok(())
                })?;
            Ok(())
        };
        for _ in 0..60 {advance(&mut world,&mut controller)?;}
        let after=world.runtime_health[&world.player_entity].health;
        let evidence=serde_json::json!({"fixture":"isolated original brick contact, not traversal",
            "spawn":spawn,"player_position":controller.player.position,"before":before,"after":after,
            "emissions":world.contact_stimuli,"brick_filter":world.collision.simulation_filter_word(shape)?,
            "player_filter":world.collision.simulation_filter_word(world.player_physics.as_ref().unwrap().shape)?,
            "touching_pairs":world.collision.touching_shape_pairs(world.player_physics.as_ref().unwrap().shape)?,
            "brick_shapes":world.collision.owner_shapes(owner).collect::<Vec<_>>(),
            "character_query_owners":world.collision.contact_owners(),"grounded":controller.player.grounded});
        println!("{evidence}");
        std::fs::write("target/2d-functional-audit/original-brick-contact.json",serde_json::to_vec_pretty(&evidence)?)?;
        ensure!(world.contact_stimuli.batches>0,"Original brick produced no contact emissions");
        ensure!(after<before,"Original brick did not damage the primary player");
        let batches=world.contact_stimuli.batches;
        world.dispatch_io_float(brick,"StimulusPerSecondList,0,1")?;
        for _ in 0..30 {advance(&mut world,&mut controller)?;}
        ensure!(world.contact_stimuli.batches==batches,"Disabled contact rate still emitted");
        Ok(())
    }
    #[test]fn contact_float_io_keeps_optional_index_separate_from_the_value()->Result<()> {
        assert_eq!(parse_float_input("StimulusPerSecondList,0.5,1")?,FloatInput{name:"StimulusPerSecondList",value:0.5,index:1});
        assert_eq!(parse_float_input(" Lifetime, 99 ")?.index,0);
        for text in ["Rate,NaN,0","Rate,1,-1","Rate,1,0,2","Rate"] {assert!(parse_float_input(text).is_err());}
        Ok(())
    }
    #[test]fn contact_channels_reach_each_receiver_in_one_ordered_health_batch()->Result<()> {
        let make=|stimulus,delivery|Emission{stimulus,amount:0.25,position:[1.,2.,3.],direction:[0.,1.,0.],
            owner:Some(1),target:Some(3),tag:String::new(),delivery};
        let batches=group_emissions(3,vec![make(0,Delivery::Target),
            make(2,Delivery::Area{radius:2.,persistent:false,projectile:1})],|_,radius,owner| {
                assert_eq!(radius,2.);assert_eq!(owner,Some(1));Ok(vec![3,4,4])
            })?;
        assert_eq!(batches.len(),2);
        let values=batches[&3].iter().map(|e|(e.stimulus,e.amount)).collect::<Vec<_>>();
        assert_eq!(values,vec![(0,0.25),(2,0.25)]);
        assert_eq!(super::super::health::stimulus_damage(&values,false)?,2);
        assert_eq!(batches[&4].len(),1);
        Ok(())
    }
    #[test]fn contact_stimulus_rates_are_live_ordered_and_layer_filtered()->Result<()> {
        use epic_mickey_lib_rs::scene_file::Property;
        use epic_mickey_lib_rs::scene_file::ID;
        let mut c=Component::new("PhysicsStimulusComponent".into(),"Contact fixture".into(),
            ID::new(0),ID::new(1),ID::new(0),vec![]);
        for (name,value) in [("Collidable Collision Layers",serde_json::json!([3])),
            ("StimulusTypeList",serde_json::json!([3,2])),("StimulusPerSecondList",serde_json::json!([0.,0.5]))] {
            c.properties.push(Property::new("Float".into(),name.into(),false,false,false,value));
        }
        assert_eq!(samples(&c,0.5,18)?,vec![(2,0.25,0.)]);
        assert!(samples(&c,0.5,4)?.is_empty());
        c.properties[2].value=serde_json::json!([0.5,0.]);
        assert_eq!(samples(&c,0.5,3)?,vec![(3,0.25,0.)]);
        Ok(())
    }
}
