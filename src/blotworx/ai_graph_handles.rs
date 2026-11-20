//! Graph handle chain acquisition481764, pool scan47b8d8 and serial478d8c.
//! The node fields retain native values; this is not a navigation search.
use anyhow::{Context as _,Result,ensure};
use std::collections::{BTreeMap,BTreeSet};

#[derive(Clone,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct Node {
    /// Active connection0 and cached traversal interfaceC. None is null.
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub connection:Option<u32>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub traversal:Option<usize>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub point:Option<super::ai_graph_queries::Point>, //4
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub graph:Option<u32>, //8
    pub free:u8, //10
    pub word14:u32,
    pub word18:u32,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub manager_1c:Option<usize>,
    pub word20:u32,
    pub byte24:u8,
    pub scalar28:u32,
    pub scalar2c:u32,
    pub byte30:u8,
    pub word34:u32,
    pub byte38:u8,
    pub byte39:u8,
    pub word3c:u32,
    pub next:Option<usize>, //40
    pub serial:u32, //44
}
#[derive(Clone,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct Resource {
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub size18:Option<u32>,
    pub byte8:u8,
    pub priority:u32, //c
    pub state:u32, //10; native comparison is signed
    pub word1c:u32,
}
#[derive(Debug,serde::Serialize,serde::Deserialize)]
pub struct State {
    pub phase:u32, //manager2d4
    pub serial:u32, //manager2e0
    pub pool:Vec<usize>, //manager168/16c
    pub nodes:BTreeMap<usize,Node>,
    pub resources:BTreeMap<u32,Resource>,
    pub current:Option<usize>, //178
    pub tracked_resource:Option<u32>, //150
    pub tracked158:u32,
    pub tracked160:u32,
    pub deferred_enabled:u8, //17c
}
impl State {
    ///47fc94: every segment must have a ready resource and clear30/38.
    pub fn ready(&self,mut handle:Option<usize>)->Result<bool> {
        let mut visited=BTreeSet::new();
        while let Some(id)=handle {
            ensure!(visited.insert(id),"Cyclic graph readiness chain");
            let n=self.node(id)?;
            if n.word14==0 {return Ok(false)}
            let resource=self.resources.get(&n.word14).context("Graph readiness resource is absent")?;
            if resource.state!=5 || n.byte38!=0 || n.byte30!=0 {return Ok(false)}
            handle=n.next;
        }
        Ok(true)
    }
    ///47fce4: this predicate checks38 without requiring ready resources.
    pub fn marked(&self,mut handle:Option<usize>)->Result<bool> {
        let mut visited=BTreeSet::new();
        while let Some(id)=handle {
            ensure!(visited.insert(id),"Cyclic graph marked chain");
            let n=self.node(id)?;if n.byte38!=0{return Ok(true)};handle=n.next;
        }
        Ok(false)
    }
    fn node(&self,id:usize)->Result<&Node>{self.nodes.get(&id).context("Graph handle is not in its arena")}
    fn node_mut(&mut self,id:usize)->Result<&mut Node>{self.nodes.get_mut(&id).context("Graph handle is not in its arena")}
    /// Scanning does not reserve a node. The caller sets10 only after obtaining
    /// the serial, just as the native scan does not mutate the chosen slot.
    fn available(&self)->Result<Option<usize>> {
        for &id in &self.pool {if self.node(id)?.free!=0{return Ok(Some(id));}}
        Ok(None)
    }
    pub fn acquire(&mut self,source:Option<usize>)->Result<Option<usize>> {
        let Some(mut source)=source else{return Ok(None)};
        if self.phase!=6{return Ok(None)};
        let Some(head)=self.available()? else{return Ok(None)};
        let serial=self.serial;self.serial=self.serial.wrapping_sub(1);
        let mut destination=head;let mut visited=BTreeSet::new();
        loop {
            ensure!(visited.insert(source),"Cyclic graph source handle chain");
            self.node_mut(destination)?.free=0;
            if self.node(source)?.next.is_some() {
                let next=self.available()?;
                self.node_mut(destination)?.next=next;
                if next.is_none() {
                    // Native failure rollback clears only these four fields.
                    let mut current=Some(head);let mut rollback=BTreeSet::new();
                    while let Some(id)=current {
                        ensure!(rollback.insert(id),"Cyclic partial graph handle chain");
                        let n=self.node_mut(id)?;
                        n.free=1;n.byte30=0;n.byte38=0;n.word34=0;current=n.next;
                    }
                    return Ok(None)
                }
            }
            // Fresh reads preserve native aliasing when source is in the pool.
            let source_node=self.node(source)?.clone();
            let n=self.node_mut(destination)?;
            n.byte38=source_node.byte38;n.byte30=source_node.byte30;
            n.word34=source_node.word34;n.scalar2c=source_node.scalar2c;
            n.word18=source_node.word18;n.byte24=source_node.byte24;
            n.word20=source_node.word20;n.byte39=source_node.byte39;
            n.word14=source_node.word14;n.serial=serial;
            let next_source=self.node(source)?.next;
            let next_destination=self.node(destination)?.next;
            let Some(next_source)=next_source else{break};
            source=next_source;
            destination=next_destination.context("Graph clone lost its allocated next node")?;
        }
        Ok(Some(head))
    }
    ///481ad0. Keep shared resource ownership, deferred work and child-release
    /// ordering. An explicit stack avoids overflowing the Rust call stack on
    /// long valid chains; there is no maximum chain length.
    pub fn release(&mut self,handle:Option<usize>)->Result<bool> {
        let Some(mut id)=handle else{return Ok(false)};
        let mut parents=Vec::new();let mut visited=BTreeSet::new();
        let result=loop {
            if self.current==Some(id){self.current=None;}
            let n=self.node(id)?.clone();
            if n.free!=0 || n.word3c!=0 {break false;}
            ensure!(visited.insert(id),"Cyclic graph release chain");
            let resource=n.word14;
            if resource==0 {
                let n=self.node_mut(id)?;
                n.free=1;n.word14=0;n.byte39=0;
            } else {
                // Resource references are portable arena keys, not Wii pointers.
                ensure!(self.resources.contains_key(&resource),"Graph handle resource is not in its arena");
                if n.byte24!=0 {
                    let mut shared=false;
                    for &other in &self.pool {
                        let node=self.node(other)?;
                        if other!=id && node.free==0 && node.word14==resource && node.byte24!=0 {shared=true;break;}
                    }
                    if !shared {self.resources.get_mut(&resource).unwrap().byte8=0;}
                }
                let mut count=0usize;let mut priority=0;
                for &other in &self.pool {
                    let node=self.node(other)?;
                    if other==id || node.free!=0 || node.word14==0 || node.word14!=resource {continue;}
                    count+=1;
                    let next=match node.word20 {0=>2,1=>3,2=>4,_=>0};
                    if next>priority {
                        priority=next;
                        self.resources.get_mut(&resource).unwrap().priority=next;
                    }
                }
                let state=self.resources[&resource].state as i32;
                if count==0 && state!=2 && state<5 {
                    if self.tracked_resource==Some(resource) {
                        self.tracked_resource=None;self.tracked160=0;self.tracked158=0;
                    }
                    let r=self.resources.get_mut(&resource).unwrap();
                    r.state=0;r.priority=0;r.word1c=0;
                    let n=self.node_mut(id)?;
                    n.free=1;n.word14=0;
                    // This branch deliberately preserves byte39 (481cf8).
                } else if count==0 && self.deferred_enabled!=0 {
                    let n=self.node_mut(id)?;
                    n.free=0;n.word20=0;n.scalar28=0;n.byte39=1;
                    self.resources.get_mut(&resource).unwrap().priority=2;
                } else {
                    if count==0 {
                        let r=self.resources.get_mut(&resource).unwrap();r.state=6;r.priority=0;
                    }
                    let n=self.node_mut(id)?;n.free=1;n.word14=0;n.byte39=0;
                }
            }
            let n=self.node_mut(id)?;
            n.byte30=0;n.byte38=0;n.byte24=0;n.word34=0;
            if let Some(child)=n.next {
                self.node_mut(child)?.word3c=0;
                parents.push(id);id=child;
            } else {
                let n=self.node_mut(id)?;n.next=None;n.serial=0;
                break true;
            }
        };
        let released_parent=!parents.is_empty();
        for parent in parents.into_iter().rev() {
            let n=self.node_mut(parent)?;n.next=None;n.serial=0;
        }
        Ok(result || released_parent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_graph_handle_pool_clone_and_exhaustion_match()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_handles.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut state:State=serde_json::from_value(case["input"]["state"].clone())?;
            let source=case["input"]["source"].as_u64().map(|n|n as usize);
            let result=state.acquire(source)?;
            assert_eq!(serde_json::to_value(result)?,case["result"],"{}",case["input"]);
            assert_eq!(serde_json::to_value(state)?,case["state"],"{}",case["input"]);
        }
        Ok(())
    }
    #[test]
    fn original_graph_handle_release_resources_and_deferred_work_match()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_handle_release.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut state:State=serde_json::from_value(case["input"]["state"].clone())?;
            let handle=case["input"]["source"].as_u64().map(|n|n as usize);
            let result=state.release(handle)?;
            assert_eq!(serde_json::to_value(result)?,case["result"],"{}",case["input"]);
            assert_eq!(serde_json::to_value(state)?,case["state"],"{}",case["input"]);
        }
        Ok(())
    }
}
