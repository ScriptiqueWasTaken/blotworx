//! Shared goto parameters at BSS11120+6f8/708/70c.
//! Constructor1c0554..1c06b4 and destructor1c0704..1c0804 only.
//! This is an explicit native protocol, not a Rust per-instance Drop policy:
//! nonzero constructor entry skips the counter increment in the original.
use anyhow::Result;

#[derive(Default,Debug,serde::Serialize)]
pub struct Shared {
    pub radius:Option<usize>,
    pub lookahead:Option<usize>,
    pub counter:u32,
}

pub trait Host {
    /// The original console-parameter registry operation14a0d8. All values
    /// retain their native domain; parsing default expressions is host-owned.
    fn parameter(&mut self,shared:&mut Shared,name:&str,default:&str,min:f32,max:f32)->Result<Option<usize>>;
    /// Intrusive retain/release include their actual synchronization and, on
    /// release-to-zero, virtual destruction. No success-only substitutes.
    fn retain(&mut self,shared:&mut Shared,handle:usize)->Result<()>;
    fn release(&mut self,shared:&mut Shared,handle:usize)->Result<()>;
}

impl Shared {
    pub fn construct(&mut self,host:&mut impl Host)->Result<()> {
        if self.counter!=0 {return Ok(());}
        let radius=host.parameter(self,"AIAvoidMickeyRadius","5.f",-f32::MAX,f32::MAX)?;
        // Read after the registry callback, then preserve this captured old
        // handle through release. The callback may mutate the shared state.
        if self.radius!=radius {
            if let Some(old)=self.radius {host.release(self,old)?;}
            self.radius=radius;
            if let Some(new)=radius {host.retain(self,new)?;}
        }
        let lookahead=host.parameter(self,"AIAvoidMickeyLookaheadTime","1.f",-f32::MAX,f32::MAX)?;
        if self.lookahead!=lookahead {
            if let Some(old)=self.lookahead {host.release(self,old)?;}
            self.lookahead=lookahead;
            if let Some(new)=lookahead {host.retain(self,new)?;}
        }
        self.counter=self.counter.wrapping_add(1);
        Ok(())
    }

    pub fn destruct(&mut self,host:&mut impl Host)->Result<()> {
        self.counter=self.counter.wrapping_sub(1);
        if self.counter!=0 {return Ok(());}
        if let Some(radius)=self.radius {
            host.release(self,radius)?;
            self.radius=None;
        }
        // Native rereads lookahead after radius release/destruction.
        if let Some(lookahead)=self.lookahead {
            host.release(self,lookahead)?;
            self.lookahead=None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    use std::collections::BTreeMap;
    struct Fixture<'a> {case:&'a Value,trace:Vec<Value>,references:BTreeMap<usize,u32>}
    impl Fixture<'_> {
        fn mutate(&mut self,s:&mut Shared,phase:&str) {
            if let Some(fields)=self.case["mutations"][phase].as_object() {
                for (name,value) in fields {
                    match name.as_str() {
                        "radius"=>s.radius=value.as_u64().map(|v|v as usize),
                        "lookahead"=>s.lookahead=value.as_u64().map(|v|v as usize),
                        "counter"=>s.counter=value.as_u64().unwrap() as u32,
                        _=>panic!("Unknown fixture field {name}"),
                    }
                }
            }
        }
    }
    impl Host for Fixture<'_> {
        fn parameter(&mut self,s:&mut Shared,name:&str,default:&str,min:f32,max:f32)->Result<Option<usize>> {
            self.trace.push(json!(["parameter",name,default,min.to_bits(),max.to_bits()]));
            let key=if name=="AIAvoidMickeyRadius" {"radius"}else{assert_eq!(name,"AIAvoidMickeyLookaheadTime");"lookahead"};
            self.mutate(s,&format!("create_{key}"));
            Ok(self.case[format!("new_{key}")].as_u64().map(|v|v as usize))
        }
        fn retain(&mut self,s:&mut Shared,handle:usize)->Result<()> {
            self.trace.push(json!(["retain",handle]));self.mutate(s,&format!("retain_{handle}"));
            let count=self.references.get_mut(&handle).unwrap();*count=count.wrapping_add(1);Ok(())
        }
        fn release(&mut self,s:&mut Shared,handle:usize)->Result<()> {
            self.trace.push(json!(["release",handle]));self.mutate(s,&format!("release_{handle}"));
            let count=self.references.get_mut(&handle).unwrap();*count=count.wrapping_sub(1);
            if *count==0 {self.trace.push(json!(["destroy",handle]));self.mutate(s,&format!("destroy_{handle}"));}
            Ok(())
        }
    }
    #[test]
    fn original_goto_shared_configuration_counts_assignments_and_callback_order_match()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_goto_configuration.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];
            let mut shared=Shared {radius:c["radius"].as_u64().map(|v|v as usize),
                lookahead:c["lookahead"].as_u64().map(|v|v as usize),counter:c["counter"].as_u64().unwrap_or(0) as u32};
            let mut host=Fixture {case:c,trace:vec![],references:[10,11,20,21].into_iter()
                .map(|h|(h,c["references"].as_u64().unwrap_or(1) as u32)).collect()};
            let mut snapshots=vec![];
            for op in c["operations"].as_array().unwrap() {
                host.trace.push(json!([op]));
                match op.as_str().unwrap(){"construct"=>shared.construct(&mut host)?,"destruct"=>shared.destruct(&mut host)?,_=>unreachable!()}
                snapshots.push(json!({"radius":shared.radius,"lookahead":shared.lookahead,"counter":shared.counter,"references":host.references}));
            }
            assert_eq!(json!(host.trace),expected["trace"],"service trace: {c}");
            assert_eq!(json!(snapshots),expected["snapshots"],"shared state/reference counts: {c}");
        }
        Ok(())
    }
}
