//! Patrol route selection1ac64c and node post-initialization36cd78.
//! Navigation reachability, behavior admission and movement remain separate.
use anyhow::Result;

pub trait Host {
    fn resolve(&mut self, target: Option<usize>) -> Result<Option<usize>>;
    fn node(&mut self, entity: Option<usize>) -> Result<Option<usize>>;
    fn previous(&mut self, node: usize) -> Result<Option<usize>>;
    fn branch_probability(&mut self, node: usize) -> Result<f32>;
    fn default_next(&mut self, node: usize) -> Result<Option<usize>>;
    fn branch_next(&mut self, node: usize) -> Result<Option<usize>>;
    fn random(&mut self) -> Result<u32>;
    fn set_previous(&mut self, node: usize, owner: usize) -> Result<()>;
}

/// Source leaves the retained target untouched if its entity/node is absent.
/// Reverse traversal consumes no random draw. Forward traversal consumes one
/// even when its probability is zero/one or its selected successor is null.
pub fn advance(target: &mut Option<usize>, reverse: u8, host: &mut impl Host) -> Result<()> {
    let entity = host.resolve(*target)?;
    let Some(node) = host.node(entity)? else { return Ok(()) };
    *target = if reverse != 0 {
        host.previous(node)?
    } else {
        let probability = host.branch_probability(node)?;
        let draw = host.random()?;
        anyhow::ensure!(draw <= 32767, "Patrol requires the original C rand domain");
        let sample = draw as f32 / 32767.;
        // Native ble includes unordered: NaN probability chooses the branch.
        if sample > probability { host.default_next(node)? } else { host.branch_next(node)? }
    };
    Ok(())
}

/// Both getters/lookups execute in source order, including duplicate targets.
/// A converging route deliberately retains its last assigned predecessor.
pub fn link(node: usize, owner: usize, host: &mut impl Host) -> Result<()> {
    let first = host.default_next(node)?;
    if first.is_some() {
        if let Some(next) = host.node(first)? { host.set_previous(next, owner)?; }
    }
    let second = host.branch_next(node)?;
    if second.is_some() {
        if let Some(next) = host.node(second)? { host.set_previous(next, owner)?; }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Binding<'a> { input: &'a Value, trace: Vec<Value>, previous: [Option<usize>; 3] }
    fn identity(v: &Value) -> Option<usize> { v.as_u64().map(|v|v as usize) }
    impl Host for Binding<'_> {
        fn resolve(&mut self,t:Option<usize>)->Result<Option<usize>> {
            self.trace.push(json!(["resolve",t])); Ok(identity(&self.input["resolved"]))
        }
        fn node(&mut self,e:Option<usize>)->Result<Option<usize>> {
            self.trace.push(json!(["node",e]));
            Ok(e.filter(|&e| self.input["nodes"].as_array().unwrap().iter().any(|v|v.as_u64()==Some(e as u64))))
        }
        fn previous(&mut self,n:usize)->Result<Option<usize>> {self.trace.push(json!(["previous",n]));Ok(self.previous[n])}
        fn branch_probability(&mut self,n:usize)->Result<f32> {self.trace.push(json!(["probability",n]));Ok(f32::from_bits(self.input["probability"].as_u64().unwrap() as u32))}
        fn default_next(&mut self,n:usize)->Result<Option<usize>> {self.trace.push(json!(["default",n]));Ok(identity(&self.input["default"]))}
        fn branch_next(&mut self,n:usize)->Result<Option<usize>> {self.trace.push(json!(["branch",n]));Ok(identity(&self.input["branch"]))}
        fn random(&mut self)->Result<u32> {self.trace.push(json!(["random"]));Ok(self.input["draw"].as_u64().unwrap() as u32)}
        fn set_previous(&mut self,n:usize,o:usize)->Result<()> {self.trace.push(json!(["set_previous",n,o]));self.previous[n]=Some(o);Ok(())}
    }
    #[test]
    fn patrol_route_and_backlinks_match_original_instructions()->Result<()> {
        let data: Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_patrol_route.json"))?;
        for case in data["cases"].as_array().unwrap() {
            let input=&case["input"];
            let mut binding=Binding{input,trace:vec![],previous:serde_json::from_value(input["previous"].clone())?};
            let mut target=identity(&input["target"]);
            if input["op"]=="link" {link(0,7,&mut binding)?;} else {advance(&mut target,input["reverse"].as_u64().unwrap() as u8,&mut binding)?;}
            assert_eq!(json!({"target":target,"previous":binding.previous,"trace":binding.trace}),case["output"],"{input}");
        }
        Ok(())
    }
    #[test]
    fn patrol_property_bindings_use_original_registered_slots()->Result<()> {
        let data:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_patrol_property_catalog.json"))?;
        for (class,slot,name) in [("BasicAIControllerComponent",24,"InitialPatrolNode"),
            ("PatrolNodeComponent",0,"DefaultNextNode"),("PatrolNodeComponent",4,"BranchNextNode"),
            ("PatrolNodeComponent",8,"BranchProbability")] {
            assert_eq!(data["catalogs"][class]["properties"][slot/4]["slot"],slot);
            assert_eq!(data["catalogs"][class]["properties"][slot/4]["name"],name);
        }
        Ok(())
    }
}
