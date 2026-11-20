//! Live attraction over original pickup arithmetic and current inventory.
//! World phase ordering/overlaps still use the existing gameplay compatibility path.
use super::*;
use crate::blotworx::{pickup, inventory_commands};
struct Host<'a> { world: &'a mut World, owner: usize }
impl Host<'_> {
    fn health_accepts(&self) -> Result<bool> {
        let player=self.world.player_entity;
        let commands=&self.world.pickup_receivers.get(&self.owner).context("Pickup receiver missing")?.commands;
        pickup::attraction_commands(commands, |amount| {
            let Some(authored)=self.world.authored_health.get(&player) else {return Ok(false)};
            let runtime=self.world.runtime_health.get(&player).context("Player live health missing")?;
            let component=self.world.entities[player].components.iter().position(|c|c.class_name=="HealthComponent").unwrap();
            let disabled=self.world.component_control.disabled.get(&player).and_then(|s|s.get(&component)).copied().unwrap_or(false);
            Ok(crate::blotworx::health::alive(runtime.health,u8::from(authored.infinite))
                && crate::blotworx::health::preview_heal(runtime.health,authored.maximum,amount,u32::from(disabled)).is_some())
        })
    }
}
struct Preview<'a> { state: &'a crate::blotworx::inventory_state::Authored }
impl inventory_commands::Backend for Preview<'_> {
    fn amount(&mut self,text:&str)->Result<f32> {crate::blotworx::config_value::decimal_float(text).map_err(anyhow::Error::msg)}
    fn add(&mut self,item:&str,amount:f32,preview:bool,_:&mut String)->Result<bool> {
        ensure!(preview,"Attraction inventory query attempted mutation");
        self.state.preview_add(item,amount,&crate::blotworx::native_names::initial_table())
    }
    fn remove(&mut self,_:&str,_:f32)->Result<()> {
        anyhow::bail!("Pickup attraction Remove command requires live inventory removal/event dispatch")
    }
}
impl pickup::AttractionWorld for Host<'_> {
    fn enabled(&mut self,_:usize)->Result<bool> {Ok(self.world.pickup_settings[&self.owner].auto_attracts)}
    fn player(&mut self)->Result<Option<usize>> {
        Ok((!self.world.removed_entities.contains(&self.world.player_entity)).then_some(self.world.player_entity))
    }
    fn valid_player(&mut self,player:Option<usize>)->Result<bool> {
        let Some(player)=player else {return Ok(false)};
        if let Some(authored)=self.world.authored_health.get(&player) {
            let health=self.world.runtime_health.get(&player).context("Attraction player health missing")?;
            if crate::blotworx::health::alive(health.health,u8::from(authored.infinite)) {return Ok(true)}
        }
        ensure!(!self.world.entities[player].components.iter().any(|c|c.class_name=="PaintThinnerCounterComponent"),
            "Attraction counter-based player validity requires native counter status");
        Ok(false)
    }
    fn radius(&mut self,_:usize)->Result<f32> {Ok(self.world.pickup_settings[&self.owner].radius)}
    fn position(&mut self,entity:usize)->Result<[f32;4]> {
        Ok(crate::blotworx::native_hk_transform::entity_position_vector(self.world.entities[entity].transform[12..15].try_into()?))
    }
    fn inventory(&mut self,player:usize)->Result<Option<usize>> {
        Ok(self.world.entities[player].components.iter().any(|c|c.class_name=="InventoryComponent").then_some(player))
    }
    fn commands_accept(&mut self,_:usize,_:usize)->Result<bool> {self.health_accepts()}
    fn inventory_preview(&mut self,_:usize,_:usize)->Result<bool> {
        // Fresh global player and live reflected values, not import-time snapshots.
        let player=self.world.player_entity;
        let component=self.world.entities[player].components.iter().find(|c|c.class_name=="InventoryComponent").context("Live attraction inventory missing")?;
        let mut state=crate::blotworx::inventory_state::Authored::from_component(component)?;
        if let Some(ammo)=&self.world.ammo.inventory {
            for (i,name) in ["Paint","Thinner"].into_iter().enumerate() {
                state.floats.insert(name.into(),ammo.amounts[i]);
                if let Some(max)=ammo.maxima[i] {state.floats.insert(format!("{name}Max"),max);}
            }
        }
        inventory_commands::preview(&self.world.pickup_receivers[&self.owner].commands,&mut Preview{state:&state})
    }
}
impl pickup::AttractionStart for Host<'_> {
    fn read_wait(&mut self,wait:&mut f32)->Result<()> {*wait=self.world.pickup_settings[&self.owner].wait;Ok(())}
    fn transform(&mut self)->Result<Option<usize>> {Ok(self.world.entities[self.owner].components.iter().any(|c|c.class_name=="JPSTransformationComponent").then_some(self.owner))}
    fn delete_parent(&mut self)->Result<bool> {Ok(self.world.pickup_settings[&self.owner].delete_parent)}
    fn health_commands_accept(&mut self)->Result<bool> {self.health_accepts()}
    fn parent(&mut self,transform:usize)->Result<Option<usize>> {self.world.current_transform_parent(transform)}
    fn detach(&mut self,transform:usize)->Result<()> {self.world.change_transform_parent(transform,None)}
    fn cleanup_parent(&mut self,parent:usize)->Result<()> {
        self.world.scripts.schedule_retirement(parent)
    }
}
impl World {
    pub(super) fn pickup_attraction_eligible(&mut self,owner:usize)->Result<bool> {
        pickup::should_attract(owner,&mut Host{world:self,owner})
    }
    // REL3897d8 refreshes baseline before the delayed collection gate.
    pub(super) fn refresh_pickup_bob_baseline(&mut self,owner:usize)->Result<()> {
        if self.current_transform_parent(owner)?.is_some()
            || self.pickup_update_states[&owner].attraction.active
            || self.pickup_attraction_eligible(owner)?
        {
            self.pickup_update_states.get_mut(&owner).unwrap().bob.base_height=self.entities[owner].transform[13];
        }
        Ok(())
    }
    pub(in crate::blotworx) fn pickup_update_snapshot(&self)->serde_json::Value {
        serde_json::json!({"component_clock_connected":true,"native_world_phase_parity_verified":false,
            "owners":self.pickup_update_states.iter().map(|(&owner,state)|serde_json::json!({
                "entity":owner,"name":self.entities[owner].name,"position":&self.entities[owner].transform[12..15],
                "parent":self.transform_hierarchy.get(&owner).map(|local|local.parent),
                "elapsed":state.gate.elapsed,"pending":state.gate.delayed,"duration":state.gate.duration,
                "attracting":state.attraction.active,"wait":state.attraction.wait,
                "bob_phase":state.bob.phase,"bob_base_height":state.bob.base_height,
                "collection_blocked":self.pickup_collection_blocked.contains(&owner)})).collect::<Vec<_>>(),
            "diagnostics":self.report.unsupported.iter().filter(|entry|entry.starts_with("Pickup attraction ") || entry.starts_with("Pickup baseline ")).collect::<Vec<_>>()})
    }
    pub(super) fn advance_pickup_attraction(&mut self,delta:f32)->Result<()> {
        let owners=self.pickup_update_states.keys().copied().collect::<Vec<_>>();
        for owner in owners {
            if self.removed_entities.contains(&owner) || !self.entity_scene_active(owner)
                || self.pickup_collection_blocked.contains(&owner)
                || self.pickup_update_states[&owner].gate.delayed {continue}
            let mut state=self.pickup_update_states.remove(&owner).unwrap();
            let result=(||->Result<()> {
                if state.attraction.active {
                    if state.attraction.waiting(delta) {return Ok(())}
                    if pickup::should_attract(owner,&mut Host{world:self,owner})? {
                        let current=crate::blotworx::native_hk_transform::entity_position_vector(self.entities[owner].transform[12..15].try_into()?);
                        let target=crate::blotworx::native_hk_transform::entity_position_vector(self.entities[self.player_entity].transform[12..15].try_into()?);
                        let position=pickup::attraction_step(current,target,self.pickup_settings[&owner].speed,delta)?;
                        self.relocate_entity(owner,position[..3].try_into()?,self.entities[owner].runtime_rotation)?;
                    } else {state.attraction.active=false;}
                } else if pickup::should_attract(owner,&mut Host{world:self,owner})? {
                    pickup::begin_attraction(&mut state.attraction,&mut state.gate,&mut Host{world:self,owner})?;
                } else if self.current_transform_parent(owner)?.is_none() && self.pickup_settings[&owner].should_bob {
                    let height=state.bob.advance_height(delta,self.pickup_bob_globals.frequency,self.pickup_bob_globals.amplitude)?;
                    let mut position:[f32;3]=self.entities[owner].transform[12..15].try_into()?;
                    position[1]=height;
                    self.relocate_entity(owner,position,self.entities[owner].runtime_rotation)?;
                }
                Ok(())
            })();
            self.pickup_update_states.insert(owner,state);
            if let Err(error)=result {
                let message=format!("Pickup attraction {}: {error:#}",self.entities[owner].name);
                if !self.report.unsupported.contains(&message) {self.report.unsupported.push(message);}
            }
        }
        Ok(())
    }
}
