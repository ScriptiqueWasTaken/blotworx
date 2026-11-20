//! Reflected AIBrain radius service for the native navigation-agent handoff.
//! The caller supplies an actually owned agent record; no registration is
//! inferred merely because an entity has a BasicAIControllerComponent.
use anyhow::{Context,Result,ensure};
use super::{assets::World,ai_agent_motion::{self,Host,Radius}};
struct Binding<'a> {
    world:&'a World,
    agent:Option<(usize,&'a mut [u32;4])>,
}
impl Host for Binding<'_> {
    fn agent(&mut self,_:usize)->Result<Option<usize>> {Ok(self.agent.as_ref().map(|(id,_)|*id))}
    fn radius(&mut self,owner:usize,kind:Radius)->Result<u32> {
        let actor=self.world.entities.get(owner).context("Agent preparation owner absent")?;
        let mut components=actor.components.iter().filter(|c|c.class_name=="AIBrainComponent");
        let brain=components.next().context("Agent preparation requires its owning AIBrain")?;
        ensure!(components.next().is_none(),"Agent preparation requires AIBrain slot arbitration");
        let name=match kind {Radius::Tether=>"Tether Radius",Radius::Wander=>"Wander Radius"};
        let mut properties=brain.properties.iter().filter(|p|p.name==name);
        let value=properties.next().with_context(||format!("Agent preparation missing {name}"))?;
        ensure!(properties.next().is_none(),"Ambiguous agent preparation {name}");
        Ok((value.value.as_f64().with_context(||format!("Invalid AIBrain {name}"))? as f32).to_bits())
    }
    fn set_scalar(&mut self,agent:usize,bits:u32)->Result<()> {
        let (id,record)=self.agent.as_mut().context("Agent preparation lost its motion record")?;
        ensure!(*id==agent,"Agent preparation motion owner changed");
        record[3]=bits;
        Ok(())
    }
}
impl World {
    pub(super) fn prepare_owned_ai_agent_motion(&self,owner:usize,kind:Radius,agent:Option<(usize,&mut [u32;4])>)->Result<()> {
        ai_agent_motion::prepare(owner,kind,&mut Binding{world:self,agent})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_preparation_uses_brain_tether_and_wander_without_changing_vector()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.Level",&egui::Context::default())?;
        let owner=world.scripts.resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01").context("Original Gus absent")?;
        let brain=world.entities[owner].components.iter().find(|c|c.class_name=="AIBrainComponent").context("Original Gus brain absent")?;
        // Supplied owned motion storage exercises the reflection/write bridge;
        // it is not evidence that World constructed a registered Gus agent.
        for (kind,name) in [(Radius::Tether,"Tether Radius"),(Radius::Wander,"Wander Radius")] {
            let expected=brain.properties.iter().find(|p|p.name==name).context("Original radius absent")?.value.as_f64().context("Radius is not numeric")? as f32;
            let mut motion=[1f32.to_bits(),(-2f32).to_bits(),3f32.to_bits(),7f32.to_bits()];
            world.prepare_owned_ai_agent_motion(owner,kind,Some((17,&mut motion)))?;
            assert_eq!(motion,[1f32.to_bits(),(-2f32).to_bits(),3f32.to_bits(),expected.to_bits()]);
        }
        // No agent must avoid even looking up an absent gameplay owner.
        world.prepare_owned_ai_agent_motion(usize::MAX,Radius::Tether,None)?;
        Ok(())
    }
}
