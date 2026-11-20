//! Concrete authored references for Patrol initialization and route traversal.
//! Normal AI factory admission/selection and navigation tasks remain required.
use super::{ai_patrol_route::{self,Host},assets::World};
use anyhow::{ensure,Context,Result};

impl World {
    fn patrol_route_component(&self, owner:usize, class:&str)->Result<Option<usize>> {
        let entity=self.entities.get(owner).context("Patrol route owner absent")?;
        let mut matches=entity.components.iter().enumerate().filter(|(_,c)|c.class_name==class);
        let first=matches.next().map(|(i,_)|i);
        ensure!(matches.next().is_none(),"Patrol route requires native {class} slot arbitration");
        Ok(first)
    }
    /// This binding covers resolved scene-local cooked entity IDs. Native
    /// runtime handles remain separate; never reinterpret an ID as an index.
    fn patrol_route_reference(&self, owner:usize, class:&str, name:&str)->Result<Option<usize>> {
        let slot=self.patrol_route_component(owner,class)?.context("Patrol reference component absent")?;
        let entity=&self.entities[owner];
        let Some(property)=entity.components[slot].properties.iter().find(|p|p.name==name) else {
            return Ok(None); // Native entity getter's initialized null destination.
        };
        let id=property.value.as_u64().and_then(|v|u32::try_from(v).ok())
            .with_context(||format!("Invalid {class}.{name} entity reference"))?;
        if id==0 {return Ok(None);}
        let mut found=self.entities.iter().enumerate().filter(|(i,e)|e.scene==entity.scene
            && e.id==id && !self.removed_entities.contains(i));
        let first=found.next().map(|(i,_)|i);
        ensure!(found.next().is_none(),"Ambiguous cooked Patrol reference {id} in {}",entity.scene);
        if first.is_some() {return Ok(first);}
        // Retirement is a known null result. An ID never bound by the loader
        // is an unsupported reference, not evidence of a native null pointer.
        if self.entities.iter().enumerate().any(|(i,e)|e.scene==entity.scene && e.id==id && self.removed_entities.contains(&i)) {
            return Ok(None);
        }
        anyhow::bail!("Unresolved cooked Patrol reference {id} for {class}.{name} in {}",entity.scene)
    }
    pub(super) fn initialize_patrol_node_routes(&mut self, range:std::ops::Range<usize>)->Result<()> {
        let mut nodes=Vec::new();
        for owner in range {
            if self.patrol_route_component(owner,"PatrolNodeComponent")?.is_some() {
                ensure!(!self.patrol_predecessors.contains_key(&owner),"Patrol node initialized twice");
                self.patrol_predecessors.insert(owner,None);
                nodes.push(owner);
            }
        }
        // Construct all nodes before applying post-initialization references,
        // including references to later objects in this publication range.
        for owner in nodes { self.link_native_patrol_node(owner)?; }
        Ok(())
    }
    pub(super) fn link_native_patrol_node(&mut self,owner:usize)->Result<()> {
        ensure!(self.patrol_predecessors.contains_key(&owner),"Patrol node was not constructed");
        ai_patrol_route::link(owner,owner,&mut Binding(self))
    }
    /// Target read portion1abbd8..1abc38. Subscription/factory admission is
    /// not fabricated here. A missing BasicAI leaves the retained target alone.
    pub(super) fn initialize_native_patrol_targets(&mut self,range:std::ops::Range<usize>)->Result<()> {
        for owner in range {
            if self.ai_brains.get(&owner).is_none_or(|b|b.patrol.is_empty()) {continue;}
            if self.patrol_route_component(owner,"BasicAIControllerComponent")?.is_none() {continue;}
            let target=self.patrol_route_reference(owner,"BasicAIControllerComponent","InitialPatrolNode")?;
            for state in self.ai_brains.get_mut(&owner).unwrap().patrol.values_mut() {state.target=target;}
        }
        Ok(())
    }
    pub(super) fn advance_native_patrol_target(&mut self,owner:usize,slot:usize)->Result<()> {
        let mut state=self.ai_brains.get_mut(&owner).context("Patrol brain absent")?.patrol.remove(&slot)
            .context("Patrol behavior absent")?;
        let result=ai_patrol_route::advance(&mut state.target,state.reverse,&mut Binding(self));
        // Retain receiver state and any preceding random draw even on failure.
        self.ai_brains.get_mut(&owner).context("Patrol route removed its brain")?.patrol.insert(slot,state);
        result
    }
}
struct Binding<'a>(&'a mut World);
impl Host for Binding<'_> {
    fn resolve(&mut self,target:Option<usize>)->Result<Option<usize>> {
        Ok(target.filter(|&i|i<self.0.entities.len() && !self.0.removed_entities.contains(&i)))
    }
    fn node(&mut self,entity:Option<usize>)->Result<Option<usize>> {
        let Some(owner)=entity else {return Ok(None)};
        if self.0.patrol_route_component(owner,"PatrolNodeComponent")?.is_none() {return Ok(None);}
        ensure!(self.0.patrol_predecessors.contains_key(&owner),"Referenced Patrol node is not initialized");
        Ok(Some(owner))
    }
    fn previous(&mut self,node:usize)->Result<Option<usize>> {
        // The source getter retains its raw predecessor pointer, but the
        // subsequent123ac0 encoding declines an entity removed from the registry.
        Ok(self.0.patrol_predecessors.get(&node).copied().context("Patrol predecessor storage absent")?
            .filter(|&i|i<self.0.entities.len() && !self.0.removed_entities.contains(&i)))
    }
    fn branch_probability(&mut self,node:usize)->Result<f32> {
        let slot=self.0.patrol_route_component(node,"PatrolNodeComponent")?.context("Patrol node absent")?;
        let property=self.0.entities[node].components[slot].properties.iter().find(|p|p.name=="BranchProbability");
        //36ce94 starts with +0 before the reflected getter.
        property.map(|p|p.value.as_f64().map(|v|v as f32).context("Invalid Patrol BranchProbability")).transpose().map(|v|v.unwrap_or(0.))
    }
    fn default_next(&mut self,node:usize)->Result<Option<usize>> {self.0.patrol_route_reference(node,"PatrolNodeComponent","DefaultNextNode")}
    fn branch_next(&mut self,node:usize)->Result<Option<usize>> {self.0.patrol_route_reference(node,"PatrolNodeComponent","BranchNextNode")}
    fn random(&mut self)->Result<u32> {Ok(self.0.game_random.draw())}
    fn set_previous(&mut self,node:usize,owner:usize)->Result<()> {
        *self.0.patrol_predecessors.get_mut(&node).context("Patrol backlink target absent")?=Some(owner);Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(world:&mut World,owner:usize,class:&str,name:&str,value:serde_json::Value)->Result<()> {
        let slot=world.patrol_route_component(owner,class)?.context("Fixture component absent")?;
        world.entities[owner].components[slot].properties.iter_mut().find(|p|p.name==name)
            .context("Fixture property absent")?.value=value;Ok(())
    }
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_patrol_routes_bind_live_references_and_preserve_shared_random_order()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),"Levels/GV_ZoneA_Start.level",&egui::Context::default())?;
        let gus=world.scripts.resolve_name("gremlinvillage_zonea.Gremlin_Variant4 01").context("Original Gremlin absent")?;
        let slot=*world.ai_brains[&gus].patrol.keys().next().context("Original Gremlin declares no Patrol")?;
        let marker=world.entities.iter().enumerate().find(|(i,e)|*i!=gus && e.scene==world.entities[gus].scene && e.id!=0)
            .map(|(i,_)|i).context("Original Gus scene has no reference marker")?;
        // Controlled runtime property edit, never a change to imported assets.
        // Initial target binding itself does not require a PatrolNode target.
        let marker_id=world.entities[marker].id;
        set(&mut world,gus,"BasicAIControllerComponent","InitialPatrolNode",serde_json::json!(marker_id))?;
        world.initialize_native_patrol_targets(gus..gus+1)?;
        ensure!(world.ai_brains[&gus].patrol[&slot].target==Some(marker),"InitialPatrolNode did not reach its owned receiver");
        set(&mut world,gus,"BasicAIControllerComponent","InitialPatrolNode",serde_json::json!(0))?;
        world.initialize_native_patrol_targets(gus..gus+1)?;
        ensure!(world.ai_brains[&gus].patrol[&slot].target.is_none(),"Explicit null initial target retained an old destination");
        let missing_id=(1..=u32::MAX).find(|&id|!world.entities.iter().any(|e|e.scene==world.entities[gus].scene && e.id==id)).unwrap();
        set(&mut world,gus,"BasicAIControllerComponent","InitialPatrolNode",serde_json::json!(missing_id))?;
        ensure!(world.initialize_native_patrol_targets(gus..gus+1).is_err(),"An unbound nonzero reference became a fabricated null target");
        set(&mut world,gus,"BasicAIControllerComponent","InitialPatrolNode",serde_json::json!(0))?;
        let nodes:Vec<_>=world.patrol_predecessors.keys().copied().collect();
        let a=*nodes.first().context("Original GV has no Patrol node")?;
        let b=world.entities.iter().enumerate().find(|(i,e)|*i!=a && e.scene==world.entities[a].scene && e.id!=0
            && !e.components.iter().any(|c|c.class_name=="PatrolNodeComponent"))
            .map(|(i,_)|i).context("Original node scene has no second fixture owner")?;
        // Construct a second runtime node from the imported component. This
        // is a controlled mod fixture, not an authored two-node DBC route.
        let node_component=world.entities[a].components.iter().find(|c|c.class_name=="PatrolNodeComponent").unwrap().clone();
        world.entities[b].components.push(node_component);
        world.initialize_patrol_node_routes(b..b+1)?;
        let (a_id,b_id)=(world.entities[a].id,world.entities[b].id);
        for (node,next) in [(a,b_id),(b,a_id)] {
            for name in ["DefaultNextNode","BranchNextNode"] {set(&mut world,node,"PatrolNodeComponent",name,serde_json::json!(next))?;}
            world.link_native_patrol_node(node)?;
        }
        ensure!(world.patrol_predecessors[&a]==Some(b) && world.patrol_predecessors[&b]==Some(a),"Live node post-initialization did not publish backlinks");
        world.ai_brains.get_mut(&gus).unwrap().patrol.get_mut(&slot).unwrap().target=Some(a);
        let before=world.game_random;
        world.advance_native_patrol_target(gus,slot)?;
        let mut expected=before;expected.draw();
        ensure!(world.game_random==expected && world.ai_brains[&gus].patrol[&slot].target==Some(b),"Forward route did not consume exactly the shared C rand draw");
        world.ai_brains.get_mut(&gus).unwrap().patrol.get_mut(&slot).unwrap().reverse=255;
        world.advance_native_patrol_target(gus,slot)?;
        ensure!(world.game_random==expected && world.ai_brains[&gus].patrol[&slot].target==Some(a),"Reverse route changed shared RNG or lost its predecessor");
        ensure!(world.ai_brains[&gus].selector.current.is_none(),"Route service fabricated behavior selection");
        println!("Bound original GV Patrol nodes: {}. Controlled Gremlin target and two-node runtime fixture reach live owned state without forced selection.",nodes.len());
        Ok(())
    }
}
