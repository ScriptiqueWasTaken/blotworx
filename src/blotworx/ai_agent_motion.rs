//! Agent motion request record at+30; REL18c850/18c908 and18de24.
//! Owning agent admission is required, rather than inferred from BasicAI alone.
use anyhow::Result;
#[derive(Clone,Copy,Debug,serde::Serialize)]
pub enum Radius { Tether, Wander }
pub trait Host {
    fn agent(&mut self,owner:usize)->Result<Option<usize>>;
    fn radius(&mut self,owner:usize,kind:Radius)->Result<u32>;
    fn set_scalar(&mut self,agent:usize,bits:u32)->Result<()>;
}
pub fn prepare(owner:usize,kind:Radius,host:&mut impl Host)->Result<()> {
    if let Some(agent)=host.agent(owner)? {
        let bits=host.radius(owner,kind)?;
        host.set_scalar(agent,bits)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Binding<'a>{input:&'a Value,record:[u32;4],trace:Vec<Value>}
    impl Host for Binding<'_> {
        fn agent(&mut self,owner:usize)->Result<Option<usize>> {
            assert_eq!(owner,11);self.trace.push(json!(["agent",owner]));
            Ok(self.input["agent"].as_bool().unwrap().then_some(17))
        }
        fn radius(&mut self,owner:usize,kind:Radius)->Result<u32> {
            self.trace.push(json!(["radius",owner,match kind{Radius::Tether=>8,Radius::Wander=>12}]));
            Ok(self.input["radius"].as_u64().unwrap() as u32)
        }
        fn set_scalar(&mut self,agent:usize,bits:u32)->Result<()> {
            assert_eq!(agent,17);self.trace.push(json!(["scalar",agent,bits]));self.record[3]=bits;Ok(())
        }
    }
    #[test]
    fn preparation_reads_brain_radius_only_for_bound_agent()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_agent_motion.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];let mut host=Binding{input,record:[0x3f800000,0xc0000000,0x40400000,0x40e00000],trace:vec![]};
            prepare(11,if input["wander"]==true{Radius::Wander}else{Radius::Tether},&mut host)?;
            assert_eq!(json!({"trace":host.trace,"record":host.record}),case["output"],"{input}");
        }
        Ok(())
    }
}
