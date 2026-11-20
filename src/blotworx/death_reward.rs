//! DeathRewardComponent event11 receiver, retail REL77c34 and77490.
//! Dynamic reward selection uses live inventory and weak-handle reservations.
//! Full native listener order and resource retirement remain incomplete.
use super::*;
#[derive(Debug, PartialEq)]
struct Row {
    ordinal: usize,
    spawn: i32,
    attach: Option<bool>,
}
fn rows(
    component: &epic_mickey_lib_rs::scene_file::Component,
    death_type: i32,
) -> Result<Vec<Row>> {
    let field = |name: &str| {
        component
            .properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| &p.value)
    };
    let Some(types) = field("DeathEventTypes") else {
        return Ok(Vec::new());
    };
    let Some(indices) = field("RewardSpawnIndex") else {
        return Ok(Vec::new());
    };
    let types = types.as_array().context("Invalid DeathEventTypes")?;
    let indices = indices.as_array().context("Invalid RewardSpawnIndex")?;
    // REL77684..776c4 requires both arrays to exist and have equal counts.
    if types.len() != indices.len() {
        return Ok(Vec::new());
    }
    let attach = field("RewardShouldAttach")
        .map(|v| v.as_array().context("Invalid RewardShouldAttach"))
        .transpose()?;
    let mut output = Vec::new();
    for (ordinal, (kind, spawn)) in types.iter().zip(indices).enumerate() {
        let kind = i32::try_from(kind.as_i64().context("Invalid death reward event type")?)?;
        if kind != death_type {
            continue;
        }
        output.push(Row {
            ordinal,
            spawn: i32::try_from(spawn.as_i64().context("Invalid reward spawn index")?)?,
            attach: attach
                .and_then(|a| a.get(ordinal))
                .map(|v| v.as_bool().context("Invalid reward attachment flag"))
                .transpose()?,
        });
    }
    Ok(output)
}
struct Distribution {
    palette: String,
    bones: Vec<String>,
    parent: Option<usize>,
    slot: usize,
    group: u32,
}
fn distribution_rotation(group: u32) -> Result<[f32; 9]> {
    // REL779ac..779d8: uint->float, separate fmuls by pi then1/8,
    // original double sin/cos, finally frsp. hkRotation stores padded columns.
    let angle = (group as f32 * f32::from_bits(0x40490fdb)) * 0.125;
    let (sin, cos) = super::super::native_angle::sin_cos_medium(f64::from(angle))?;
    let (sin, cos) = (sin as f32, cos as f32);
    Ok([cos, 0., -sin, 0., 1., 0., sin, 0., cos])
}
impl World {
    pub fn dynamic_reward_snapshot(&self)->serde_json::Value {
        serde_json::json!({"pending":self.dynamic_rewards.pending,"active_references":self.dynamic_rewards.reference_counts(),"live_component_cursors":self.dynamic_rewards.cursors.len()})
    }
    fn reward_float(&self, owner:usize, name:&str)->Result<f32> {
        let value=self.entities[owner].components.iter().find(|c|c.class_name=="DeathRewardComponent")
            .and_then(|c|c.properties.iter().find(|p|p.name==name))
            .and_then(|p|p.value.as_f64()).with_context(||format!("Dynamic reward {name} missing or invalid"))? as f32;
        ensure!(value.is_finite(),"Nonfinite dynamic reward {name}"); Ok(value)
    }
    fn spawn_dynamic_category(&mut self, owner:usize, kind:i32)->Result<Option<usize>> {
        use crate::blotworx::dynamic_reward;
        let pending=self.dynamic_rewards.pending;
        let mut palette=None;
        let category=match kind {
            0|6 => {
                let inventory=self.ammo.inventory.as_ref().context("Dynamic ammo reward has no live player inventory")?;
                let amounts=inventory.amounts;
                let maxima=[inventory.maxima[0].context("PaintMax missing")?,inventory.maxima[1].context("ThinnerMax missing")?];
                if kind==6 {
                    // Both channels must have room; no shared-ammo policy lookup.
                    (amounts[0]+pending[1]<maxima[0] && amounts[1]+pending[2]<maxima[1]).then_some(2)
                } else {dynamic_reward::ammo(amounts,maxima,[pending[1],pending[2]],||self.game_random.draw())}
            }
            1=>Some(2),
            2=> {
                let player=self.player_entity;
                let live=self.runtime_health.get(&player).context("Dynamic health reward has no player health")?;
                let authored=self.authored_health.get(&player).context("Dynamic health reward has no health descriptor")?;
                ensure!(pending[0].is_finite() && (-2147483648. ..2147483648.).contains(&pending[0]),"Pending reward health needs unsupported native integer conversion");
                dynamic_reward::health(live.health,authored.maximum,pending[0])
            }
            3=>Some(0),
            4=> {
                let component=self.entities[self.player_entity].components.iter().find(|c|c.class_name=="InventoryComponent").context("Dynamic sketch reward has no inventory")?;
                let inventory=crate::blotworx::inventory_state::Authored::from_component(component)?;
                let mut minimum=1000i32;
                let mut candidates=Vec::new();
                for (index,name) in inventory.sketches.iter().enumerate() {
                    //8ec80 resolves the first matching registered ability and queries Paint.
                    if !self.player_ability_registry.sketches.iter().find(|ability|ability.name==*name)
                        .is_some_and(|ability|ability.ownership.enabled(crate::blotworx::brush::Ink::Paint)) {continue;}
                    let (category,channel)=match name.as_str() {"Sketch_TV"=>(5,5),"Sketch_Anvil"=>(6,3),_=>(7,4)};
                    let count=inventory.counts.get(index).context("SketchCount index missing")?.wrapping_add(pending[channel] as i32);
                    let maximum=*inventory.maxima.get(index).context("SketchMax index missing")?;
                    ensure!((-2147483648. ..2147483648.).contains(&maximum) && (-2147483648. ..2147483648.).contains(&pending[channel]),"Sketch reward needs unsupported native integer conversion");
                    let maximum=maximum as i32;
                    if count>=maximum || count>minimum {continue;}
                    if count<minimum {minimum=count;candidates.clear();}
                    candidates.push(category);
                }
                if candidates.is_empty() {None} else {
                    // Native consumes a draw even when exactly one sketch wins.
                    let index=((candidates.len() as f32)*(self.game_random.draw() as f32/32767.)) as usize;
                    Some(candidates[index.min(candidates.len()-1)])
                }
            }
            5=> {
                let ten=self.reward_float(owner,"DynamicRewardTenETicketPercentage")?;
                let thirty=self.reward_float(owner,"DynamicRewardThirtyETicketPercentage")?;
                palette=Some(dynamic_reward::ticket(self.game_random.draw(),ten,thirty));
                None
            }
            _=>return Ok(None), // Native unmatched DynamicRewards enum declines.
        };
        let palette=match (palette,category) {
            (Some(name),_)=>name,
            (None,Some(category))=>["Palette_BigHealth","Palette_SmallHealth","Palette_DoubleAmmo","Palette_PaintAmmo","Palette_ThinnerAmmo","Palette_TVSketch","Palette_AnvilSketch","Palette_WatchSketch"][category],
            _=>return Ok(None),
        };
        let name=self.entities[owner].components.iter().find(|c|c.class_name=="DeathRewardComponent")
            .and_then(|c|c.properties.iter().find(|p|p.name==palette)).and_then(|p|p.value.as_str())
            .with_context(||format!("Dynamic reward palette {palette} missing"))?.to_owned();
        ensure!(!name.is_empty(),"Dynamic reward palette {palette} is empty; native null-factory reservations are not reconstructed");
        let position=self.entities[owner].transform[12..15].try_into()?;
        let root=self.spawn_palette_root(owner,&name,position,[1.,0.,0.,0.,1.,0.,0.,0.,1.],"")?;
        if let Some(category)=category {self.dynamic_rewards.reserve(category,root);}
        self.report.geometry.push(serde_json::json!({"kind":"dynamic_reward_selection","owner":owner,"entity":root,"dynamic_type":kind,"reservation_category":category,"palette_property":palette,"palette":name,"pending":self.dynamic_rewards.pending}));
        Ok(Some(root))
    }
    fn spawn_dynamic_reward(&mut self,owner:usize)->Result<Vec<usize>> {
        let component=self.entities[owner].components.iter().find(|c|c.class_name=="DeathRewardComponent").context("Missing reward component")?;
        let kinds=component.properties.iter().find(|p|p.name=="DynamicRewards").and_then(|p|p.value.as_array()).context("Invalid DynamicRewards")?
            .iter().map(|kind|Ok(i32::try_from(kind.as_i64().context("Invalid dynamic reward type")?)?)).collect::<Result<Vec<_>>>()?;
        if kinds.is_empty() {return Ok(Vec::new());}
        let mut cursor=self.dynamic_rewards.cursors.get(&owner).copied().unwrap_or(0)%kinds.len();
        for _ in 0..kinds.len() {
            let reward=self.spawn_dynamic_category(owner,kinds[cursor])?;
            cursor=(cursor+1)%kinds.len();
            if let Some(reward)=reward {
                self.dynamic_rewards.cursors.insert(owner,cursor);
                return Ok(vec![reward]);
            }
        }
        self.dynamic_rewards.cursors.insert(owner,cursor);
        Ok(Vec::new())
    }

    fn attach_death_reward(
        &mut self,
        owner: usize,
        reward: usize,
        state: &mut Distribution,
    ) -> Result<()> {
        if state.palette.is_empty() || state.bones.is_empty() {
            return Ok(());
        }
        if state.parent.is_none() || state.slot >= state.bones.len() {
            let rotation = distribution_rotation(state.group)?;
            let position = self.entities[owner].transform[12..15].try_into()?;
            let parent = self.spawn_palette_root(owner, &state.palette, position, rotation, "")?;
            self.configure_death_reward(owner, parent)?;
            state.parent = Some(parent);
            state.slot = 0;
            let group = state.group;
            state.group = state.group.wrapping_add(1);
            self.report.geometry.push(serde_json::json!({"kind":"death_reward_distribution","owner":owner,"entity":parent,"palette":state.palette,"group":group,"rotation":rotation}));
        }
        let parent = state.parent.unwrap();
        let bone = &state.bones[state.slot];
        if !bone.is_empty() {
            let transform = self.entities[reward]
                .components
                .iter_mut()
                .find(|c| c.class_name == "JPSTransformationComponent")
                .context("Reward has no transform")?;
            if let Some(property) = transform
                .properties
                .iter_mut()
                .find(|p| p.name == "Bone Attach Name")
            {
                property.value = serde_json::json!(bone.to_ascii_lowercase());
            } else {
                transform
                    .properties
                    .push(epic_mickey_lib_rs::scene_file::Property::new(
                        "String".into(),
                        "Bone Attach Name".into(),
                        false,
                        false,
                        false,
                        serde_json::json!(bone.to_ascii_lowercase()),
                    ));
            }
        }
        self.change_transform_parent(reward, Some(parent))?;
        self.report.geometry.push(serde_json::json!({"kind":"death_reward_attachment","owner":owner,"entity":reward,"parent":parent,"bone":bone,"slot":state.slot,"native_order_verified":false}));
        state.slot += 1;
        Ok(())
    }

    fn configure_death_reward(&mut self, owner: usize, reward: usize) -> Result<()> {
        let component = self.entities[owner]
            .components
            .iter()
            .find(|c| c.class_name == "DeathRewardComponent")
            .context("Missing death reward owner")?;
        let number = |name: &str| -> Result<Option<f32>> {
            component
                .properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| {
                    let value = p
                        .value
                        .as_f64()
                        .with_context(|| format!("Invalid death reward {name}"))?
                        as f32;
                    ensure!(value.is_finite(), "Non-finite death reward {name}");
                    Ok(value)
                })
                .transpose()
        };
        let pending = number("DynamicRewardPendingTime")?;
        let lifetime = number("DynamicRewardLifetime")?;
        let radius = number("RewardAttractDistance")?;
        if let Some(state) = self.pickup_update_states.get_mut(&reward) {
            // REL77858..77890 -> PlayerPickup vtable+31c=38a3f4:
            // set duration and delayed flag; do not reset elapsed.
            state
                .gate
                .delay(pending.context("Pickup reward has no resolved pending time")?);
            if let Some(radius) = radius.filter(|value| *value > 0.) {
                self.pickup_settings
                    .get_mut(&reward)
                    .context("Missing reward pickup settings")?
                    .radius = radius;
                if let Some(property) = self.entities[reward]
                    .components
                    .iter_mut()
                    .find(|c| c.class_name == "PlayerPickupComponent")
                    .and_then(|c| {
                        c.properties
                            .iter_mut()
                            .find(|p| p.name == "AutoAttractRadius")
                    })
                {
                    property.value = serde_json::json!(radius);
                }
            }
        }
        // REL778e4..77938 only assigns Lifetime when its current value is zero.
        if let Some(state) = self.runtime_lifetimes.get_mut(&reward) {
            if state.timer.remaining == 0. {
                let lifetime = lifetime.context("Reward has no resolved lifetime")?;
                state.timer.remaining = lifetime;
                if let Some(property) = self.entities[reward]
                    .components
                    .iter_mut()
                    .find(|c| c.class_name == "LifetimeComponent")
                    .and_then(|c| c.properties.iter_mut().find(|p| p.name == "Lifetime"))
                {
                    property.value = serde_json::json!(lifetime);
                }
            }
        }
        Ok(())
    }

    pub(in crate::blotworx) fn receive_death_rewards(
        &mut self,
        owner: usize,
        death_type: i32,
    ) -> Result<()> {
        if self.removed_entities.contains(&owner) || self.death_rewarded.contains(&owner) {
            return Ok(());
        }
        let mut components = self.entities[owner]
            .components
            .iter()
            .filter(|c| c.class_name == "DeathRewardComponent");
        let Some(component) = components.next() else {
            return Ok(());
        };
        ensure!(
            components.next().is_none(),
            "Multiple DeathRewardComponent receivers require separate listener state"
        );
        let removed=&self.removed_entities;
        self.dynamic_rewards.purge(|entity|!removed.contains(&entity));
        let rows = rows(component, death_type)?;
        let palette = component
            .properties
            .iter()
            .find(|p| p.name == "Animation NIF Palette")
            .map(|p| {
                p.value
                    .as_str()
                    .context("Invalid reward distribution palette")
            })
            .transpose()?
            .unwrap_or("")
            .to_owned();
        let bones = component
            .properties
            .iter()
            .find(|p| p.name == "AnimationAttachmentBones")
            .map(|p| -> Result<Vec<String>> {
                p.value
                    .as_array()
                    .context("Invalid distribution bone array")?
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .context("Invalid distribution bone name")
                    })
                    .collect()
            })
            .transpose()?
            .unwrap_or_default();
        let mut distribution = Distribution {
            palette,
            bones,
            parent: None,
            slot: 0,
            group: 0,
        };
        for row in rows {
            let spawned=if row.spawn == -1 {self.spawn_dynamic_reward(owner)} else {self.spawn_reward_index(owner,row.spawn)};
            match spawned {
                Ok(spawned) => {
                    for &reward in &spawned {
                        if let Err(error) = self.configure_death_reward(owner, reward) {
                            self.report.unsupported.push(format!(
                                "Death reward setup {}: {error:#}",
                                self.entities[reward].name
                            ));
                        }
                    }
                    self.report.geometry.push(serde_json::json!({"kind":"death_reward_spawner_dispatch","owner":owner,"death_type":death_type,"row":row.ordinal,"spawn_index":row.spawn,"spawned":spawned,"attachment_required":row.attach,"native_receiver_order_verified":false}));
                    if row.attach == Some(true) {
                        for reward in spawned {
                            if let Err(error) =
                                self.attach_death_reward(owner, reward, &mut distribution)
                            {
                                self.report.unsupported.push(format!(
                                    "Death reward attachment {}: {error:#}",
                                    self.entities[reward].name
                                ));
                            }
                        }
                    }
                }
                Err(error) => self.report.unsupported.push(format!(
                    "Death reward spawn {} index{}: {error:#}",
                    self.entities[owner].name, row.spawn
                )),
            }
        }
        // Native77cb8 writes its once flag even when no reward row matched.
        self.death_rewarded.insert(owner);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_health_type_matches_both_barrel_rows_and_live_indices() -> Result<()> {
        use epic_mickey_lib_rs::scene_file::{SceneFileVersion, ID};
        let scene = SceneFile::new(
            vec![],
            vec![],
            vec![],
            ID::new(0),
            SceneFileVersion::Version1,
        );
        let mut c = crate::component_schema::CATALOG.components["DeathRewardComponent"]
            .instantiate("DeathRewardComponent", &scene)?;
        for p in &mut c.properties {
            match p.name.as_str() {
                "DeathEventTypes" => p.value = serde_json::json!([2, 2]),
                "RewardSpawnIndex" => p.value = serde_json::json!([0, -1]),
                "RewardShouldAttach" => p.value = serde_json::json!([false, true]),
                _ => {}
            }
        }
        assert_eq!(
            rows(&c, 2)?,
            vec![
                Row {
                    ordinal: 0,
                    spawn: 0,
                    attach: Some(false)
                },
                Row {
                    ordinal: 1,
                    spawn: -1,
                    attach: Some(true)
                }
            ]
        );
        assert!(rows(&c, 1)?.is_empty());
        c.properties
            .iter_mut()
            .find(|p| p.name == "RewardSpawnIndex")
            .unwrap()
            .value = serde_json::json!([3, -1]);
        assert_eq!(rows(&c, 2)?[0].spawn, 3);
        c.properties
            .iter_mut()
            .find(|p| p.name == "RewardSpawnIndex")
            .unwrap()
            .value = serde_json::json!([3]);
        assert!(rows(&c, 2)?.is_empty());
        Ok(())
    }
}
