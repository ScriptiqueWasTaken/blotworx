//! Entity visibility's ordered-hit decision, REL2439d8..243ad8.
//! Bound-point selection and collision generation belong to the caller.
use anyhow::Result;
pub trait Host {
    fn entity(&mut self, hit:usize)->Result<Option<usize>>;
    fn root(&mut self, entity:Option<usize>)->Result<Option<usize>>;
    fn filter(&mut self, hit:usize)->Result<u32>;
}
pub fn visible(source:usize,target:usize,count:usize,skip_source:u32,skip_layers:i32,host:&mut impl Host)->Result<bool> {
    for hit in 0..count {
        let entity=host.entity(hit)?;
        let root=host.root(entity)?;
        let target_root=host.root(Some(target))?;
        if entity==Some(target) || root==target_root {return Ok(true);}
        if skip_source!=0 && entity==Some(source) {continue;}
        if skip_layers>0 {
            let shift=(host.filter(hit)?&31).wrapping_sub(1);
            // PPC slw uses the low6 bits, with bit5 producing a zero word.
            let bit=if shift&32!=0 {0} else {1u32<<(shift&31)};
            if (skip_layers as u32)&bit!=0 {continue;}
        }
        return Ok(false);
    }
    // Source returns false when a nonempty list contains only ignored hits.
    Ok(count==0)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json,Value};
    struct Binding<'a>{case:&'a Value,trace:Vec<Value>}
    impl Host for Binding<'_> {
        fn entity(&mut self,i:usize)->Result<Option<usize>> {
            self.trace.push(json!(["entity",i]));Ok(self.case["hits"][i]["entity"].as_u64().map(|v|v as usize))
        }
        fn root(&mut self,e:Option<usize>)->Result<Option<usize>> {
            self.trace.push(json!(["root",e]));Ok(e.and_then(|e|self.case["roots"][e].as_u64().map(|v|v as usize)))
        }
        fn filter(&mut self,i:usize)->Result<u32> {self.trace.push(json!(["filter",i]));Ok(self.case["hits"][i]["filter"].as_u64().unwrap() as u32)}
    }
    #[test]
    fn ordered_visibility_decision_matches_original_instructions()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/entity_visibility.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];let mut host=Binding{case:input,trace:vec![]};
            let result=visible(0,1,input["hits"].as_array().unwrap().len(),input["skip_source"].as_u64().unwrap() as u32,
                input["skip_layers"].as_i64().unwrap() as i32,&mut host)?;
            assert_eq!(json!({"visible":result,"trace":host.trace}),case["output"],"{input}");
        }
        Ok(())
    }
}
