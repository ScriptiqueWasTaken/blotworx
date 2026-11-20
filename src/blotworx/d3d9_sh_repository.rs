//! PC b3f840/b3f8d0/b3f730/b3f770/b3f660: insertion-ordered SH storage.
//! Null identities are distinct from valid runtime index zero. Expiry retains
//! payloads until Update compacts the repository, preserving survivor order.
use std::{collections::BTreeMap,sync::Arc};
#[derive(Debug)]
pub(super) struct Entry<K,S,V> {
    pub owner:Option<K>,pub scene:Option<S>,pub value:Arc<V>,
}
#[derive(Debug)]
pub(super) struct Repository<K:Ord,S,V> {
    entries:Vec<Entry<K,S,V>>,names:BTreeMap<Option<K>,String>,pending:usize,
}
impl<K:Ord,S,V> Default for Repository<K,S,V> {
    fn default()->Self {Self {entries:Vec::new(),names:BTreeMap::new(),pending:0}}
}
impl<K:Ord+Clone,S:Eq,V> Repository<K,S,V> {
    pub fn entries(&self)->&[Entry<K,S,V>] {&self.entries}
    pub fn add(&mut self,owner:Option<K>,scene:Option<S>,value:Arc<V>,name:String) {
        self.names.insert(owner.clone(),name);
        self.entries.push(Entry {owner,scene,value});
    }
    pub fn update(&mut self,owner:Option<K>,scene:Option<S>,value:Arc<V>,name:String) {
        if let Some(entry)=self.entries.iter_mut().find(|e|e.owner==owner) {
            entry.scene=scene;entry.value=value;
        } else {self.add(owner,scene,value,name);}
    }
    pub fn remove_owner(&mut self,owner:&Option<K>) {
        self.names.remove(owner);
        for entry in &mut self.entries {if &entry.owner==owner {
            entry.owner=None;entry.scene=None;self.pending+=1;
        }}
    }
    pub fn remove_scene(&mut self,scene:&Option<S>) {
        for entry in &mut self.entries {if &entry.scene==scene {
            self.names.remove(&entry.owner);
            entry.owner=None;entry.scene=None;self.pending+=1;
        }}
    }
    pub fn flush(&mut self) {
        if self.pending!=0 {
            self.entries.retain(|e|e.owner.is_some()||e.scene.is_some());
            self.pending=0;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_pc_repository_transcript() {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/pc_sh_repository.json")).unwrap();
        let mut repo=Repository::<u64,u64,u64>::default();
        for step in fixture["steps"].as_array().unwrap() {
            let command=&step["command"];
            let identity=|key:&str|command[key].as_u64().filter(|v|*v!=0);
            match command["op"].as_str().unwrap() {
                "add"|"update"=>{
                    let value=Arc::new(command["value"].as_u64().unwrap());
                    let name=command["name"].as_str().unwrap().to_owned();
                    if command["op"]=="add" {repo.add(identity("owner"),identity("scene"),value,name);}
                    else {repo.update(identity("owner"),identity("scene"),value,name);}
                },
                "remove_owner"=>repo.remove_owner(&identity("owner")),
                "remove_scene"=>repo.remove_scene(&identity("scene")),
                "flush"=>repo.flush(),_=>panic!("unknown command"),
            }
            let rows:Vec<_>=repo.entries().iter().map(|e|serde_json::json!({"owner":e.owner.unwrap_or(0),"scene":e.scene.unwrap_or(0),"value":*e.value})).collect();
            let names:BTreeMap<_,_>=repo.names.iter().map(|(k,v)|(k.unwrap_or(0).to_string(),v)).collect();
            assert_eq!(serde_json::json!({"rows":rows,"count":repo.entries.len(),"pending_expiry":repo.pending,"names":names}),step["after"],"{command}");
        }
    }
}
