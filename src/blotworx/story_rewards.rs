//! Original inventory-event delegate27d1ec -> interface14784 and helpers.
//! Inventory/health/collectible owners are shared with ordinary gameplay.
use anyhow::{Result,Context,bail,ensure};
use crate::em1_bsq::Node;
#[derive(Debug,serde::Serialize)]
#[serde(tag="op",rename_all="lowercase")]
enum Operation {Add{item:String,amount_bits:u32},Health{value:u32},Owned{notify:u32,immediate:u32},Count{value:u32}}
fn plan(category:u16,item:&str,amount:i32,owned:bool,count:i32,health:Option<(i32,i32)>,player:bool)->Result<Vec<Operation>> {
    if !player {return Ok(Vec::new());}
    Ok(match category {
        0=>if item=="DualAmmo" {vec![Operation::Add{item:"Paint".into(),amount_bits:(amount as f32).to_bits()},Operation::Add{item:"Thinner".into(),amount_bits:(amount as f32).to_bits()}]} else {vec![Operation::Add{item:item.into(),amount_bits:(amount as f32).to_bits()}]},
        1=>health.map(|(current,maximum)|vec![Operation::Health{value:current.wrapping_add(amount).min(maximum) as u32}]).unwrap_or_default(),
        3|4|5=>if owned {vec![Operation::Count{value:count.wrapping_add(1) as u32}]} else {vec![Operation::Owned{notify:u32::from(category==3),immediate:u32::from(category==3)}]},
        7=>{let canonical=["Sketch_Anvil","Sketch_TV","Sketch_Watch"].into_iter().find(|name|item.contains(name)).unwrap_or(item);vec![Operation::Add{item:canonical.into(),amount_bits:(amount as f32).to_bits()}]},
        2|6=>bail!("BSQ item category{category} needs native health-capacity/sketch-upgrade receiver completion"),
        _=>Vec::new(), //14784's switch explicitly declines unknown categories.
    })
}
impl super::assets::World {
    pub(super) fn apply_story_reward(&mut self,item:&Node,amount:i32)->Result<()> {
        let name=item.value("Item").and_then(|v|v.as_str()).context("BSQ reward item name missing")?;
        let category=u16::try_from(item.value("Category").and_then(|v|v.as_u64()).context("BSQ reward category missing")?)?;
        let player=self.entities.get(self.player_entity).is_some()&&!self.removed_entities.contains(&self.player_entity);
        let health=self.entities.get(self.player_entity).and_then(|e|e.components.iter().find(|c|c.class_name=="HealthComponent"))
            .and_then(|c|c.properties.iter().find(|p|p.name=="MaxHealth")).and_then(|p|p.value.as_i64())
            .zip(self.runtime_health.get(&self.player_entity)).map(|(maximum,runtime)|(runtime.health,maximum as i32));
        let operations=plan(category,name,amount,self.collectibles.has(name),self.collectibles.named_count(name),health,player)?;
        for operation in operations {
            match operation {
                Operation::Add{item,amount_bits}=>{self.player_inventory_add(&item,f32::from_bits(amount_bits))?;}
                Operation::Health{value}=>self.set_script_health(self.player_entity,value as i32).map_err(|e|anyhow::anyhow!("{e}"))?,
                Operation::Count{value}=>self.collectibles.set_named_count(name,value as i32),
                Operation::Owned{notify,immediate}=>{
                    let before=self.collectibles.notifications.len();
                    self.collectibles.set_owned(name,true,notify!=0,immediate!=0)?;
                    if self.collectibles.notifications.len()>before && immediate!=0 {self.ui_notifications.push(self.collectibles.notifications.pop().unwrap());}
                }
            }
        }
        // Event8 overrides serialized item Quantity with its separate signed
        // Amount. Item Lua/state changes belong to other native UI paths.
        self.demo_story.reward_events=self.demo_story.reward_events.wrapping_add(1);
        Ok(())
    }
}
#[cfg(debug_assertions)]
fn reward_nodes<'a>(node:&'a Node,out:&mut Vec<&'a Node>) {
    match node {Node::Record{fields,..}=>{if node.value("_type").and_then(|v|v.as_u64())==Some(8) {out.push(node);}
        for child in fields {reward_nodes(child,out);}},Node::List{items,..}=>for child in items {reward_nodes(child,out);},_=>{}}
}
#[cfg(debug_assertions)]
fn item_record(node:&Node)->Result<&Node> {let Node::Record{fields,..}=node else {bail!("Reward event record missing")};fields.iter().find(|n|matches!(n,Node::Record{name,..} if name=="Item")).context("Reward item record missing")}
pub(super) fn event_item(node:&Node)->Result<&Node> {let Node::Record{fields,..}=node else {bail!("Reward event record missing")};fields.iter().find(|n|matches!(n,Node::Record{name,..} if name=="Item")).context("Reward item record missing")}
#[cfg(debug_assertions)]
pub(super) fn validate_story_rewards()->Result<()> {
    super::script_feedback::validate()?;
    super::spline_branch::validate()?;
    let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/story_rewards.json"))?;
    for c in reference["cases"].as_array().unwrap() {
        let operations=plan(c["category"].as_u64().unwrap() as u16,c["item"].as_str().unwrap(),c["amount"].as_i64().unwrap() as i32,c["owned"].as_bool().unwrap(),c["count"].as_i64().unwrap() as i32,Some((c["health"].as_i64().unwrap() as i32,c["maximum"].as_i64().unwrap() as i32)),c["player"].as_bool().unwrap())?;
        ensure!(serde_json::to_value(operations)?==c["events"],"Original reward operation mismatch: {c}");
    }
    let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let root=std::path::Path::new(&root);
    let mut w=super::assets::load(root,"Levels/MeanStreet_V1.level",&egui::Context::default())?;
    let inventory:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/story_reward_assets.json"))?;
    let rows=inventory["rows"].as_array().unwrap();let mut packed:Option<(String,crate::pack_io::StoredPack)>=None;
    for row in rows {
        let pack=row["pack"].as_str().unwrap();let path=row["path"].as_str().unwrap();
        if packed.as_ref().is_none_or(|p|p.0!=pack) {
            packed=None;let wanted=rows.iter().filter(|r|r["pack"]==pack).map(|r|r["path"].as_str().unwrap().to_ascii_lowercase()).collect::<std::collections::BTreeSet<_>>();
            packed=Some((pack.into(),crate::pack_io::read_filtered(std::fs::read(root.join("packfiles").join(pack))?,|_,path|wanted.contains(&path.to_ascii_lowercase()))?));
        }
        let file=packed.as_ref().unwrap().1.pak.files.iter().find(|f|f.path.eq_ignore_ascii_case(path)).context("Original reward asset missing")?;
        let doc=crate::em1_bsq::Document::load(&file.path,&file.data)?;let mut events=Vec::new();reward_nodes(&doc.root,&mut events);
        let amount=row["amount"].as_u64().unwrap() as u32 as i32;
        let event=events.into_iter().find(|n|n.value("Amount").and_then(|v|v.as_i64())==Some(amount as i64) && item_record(n).ok().is_some_and(|i|i.value("Item").and_then(|v|v.as_str())==row["item"].as_str() && i.value("Category")==Some(&row["category"]))).context("Reward catalog differs from original BSQ")?;
        w.apply_story_reward(item_record(event)?,amount)?;
    }
    ensure!(w.demo_story.reward_events==rows.len() as u64,"Original reward events were discarded");
    drop(w);
    let mut w=super::assets::load(root,"Levels/GV_ZoneD_Start.level",&egui::Context::default())?;
    w.enable_demo_story()?;w.story_sequences.queue.clear();w.cutscene_input=Default::default();
    w.set_inventory_float(w.player_entity,"ETicket",0,200.)?;
    w.globals.set("Mickey_Tickets",200);
    let sequence="GV_ZoneD_Herman_Freed";
    let owner=w.entities.iter().enumerate().find_map(|(owner,e)|e.components.iter().filter(|c|c.class_name=="ConversationComponent").flat_map(|c|c.properties.iter().filter(|p|p.name=="Sequences")).any(|p|super::actor_scripts::authored_paths(&p.value).is_ok_and(|paths|paths.iter().any(|p|p.to_ascii_lowercase().ends_with("gv_zoned_herman_freed.bsq")))).then_some(owner)).context("Original ZoneD reward speaker missing")?;
    let before=w.demo_story.reward_events;
    w.force_story_sequence(owner,sequence)?;
    for _ in 0..600 {if w.demo_story.dialogue.is_some(){w.acknowledge_demo_story(0);}w.advance_demo_story(super::STEP);if w.demo_story.reward_events>before {break;}}
    ensure!(w.demo_story.reward_events>before && w.demo_story.errors.is_empty(),"Original reward sequence did not reach Inventory event: {:?}",w.demo_story.errors);
    println!("PASS:40 native reward reference cases; all{} original inventory events execute; actual ZoneD BSQ reaches reward handler; speaker={}",rows.len(),w.entities[owner].name);Ok(())
}
