//! Mpd::JPSFleeAgent1be97c/1bec1c/1beb70/1bea58. This derives from
//! Agent1965b4, not PathAgent. Work buffers live only in the shared arena.
use anyhow::{Context,Result};
use super::{ai_agent_base::Agent,ai_graph_link_passes::Memory};
/// Captured service interface identity. Inline acceptance belongs to the
/// specific pathfinder queried; it must not become an invented numeric address.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Acceptance {External(usize),PathfinderInline(usize)}
#[cfg(test)]
fn external(value:Option<Acceptance>)->usize {
    match value {None=>0,Some(Acceptance::External(id))=>id,Some(Acceptance::PathfinderInline(_))=>panic!("Native flat fixture has no inline interface")}
}
pub struct State {
    pub base:Agent,
    pub references_34_38_3c_40_44:[Option<usize>;5],
    pub word_48:u32,
    pub reference_4c:Option<Acceptance>,
    pub arrays_50_54:[Option<usize>;2],
    pub count_58:u32,pub capacity_5c:u32,
    pub arrays_60_64:[Option<usize>;2],
    pub count_68:u32,pub capacity_6c:u32,
    pub index_70:i32,
    pub scalars_74_78_7c_80_84:[f32;5],
    pub flag_88:u8,pub speed_8c:f32,pub flags_90_91:[u8;2],pub index_94:i32,
}
impl super::ai_agent_base::Owner for State {
    fn base(&self)->&super::ai_agent_base::Agent {&self.base}
    fn base_mut(&mut self)->&mut super::ai_agent_base::Agent {&mut self.base}
}
pub trait Host {
    /// The original World+0xc unit-scale fallback, not a configured speed.
    fn global_unit_scale(&mut self)->Result<Option<f32>>;
    /// Read the current owner's actual brain+14 entity link.
    fn owner_entity(&mut self,state:&mut State)->Result<usize>;
    fn release_reservation(&mut self,state:&mut State,m:&mut Memory,reservation:usize)->Result<()>;
    fn detach_service(&mut self,state:&mut State,m:&mut Memory,service:usize,entity:usize)->Result<()>;
}
pub trait BindingHost:Host {
    fn owner_service(&mut self,state:&mut State)->Result<Option<usize>>;
    fn world_service(&mut self,state:&mut State,class:u32)->Result<Option<usize>>;
    fn service_3c(&mut self,state:&mut State,service:usize)->Result<Option<Acceptance>>;
    fn entity_speed(&mut self,state:&mut State,entity:usize)->Result<Option<f32>>;
    /// Actual196690, including derived virtual10=1bef20, not a ready flag.
    fn bind_base(&mut self,state:&mut State,m:&mut Memory,attributes:usize)->Result<bool>;
    fn service_field_50(&mut self,state:&mut State,service:usize)->Result<Option<usize>>;
    /// Includes the native allocation observer before the returned pointer is
    /// stored. Mutations to capacities affect only subsequent fresh reads.
    fn allocate(&mut self,state:&mut State,m:&mut Memory,bytes:u32)->Result<u32>;
}
pub trait DestructionHost:Host {
    fn free(&mut self,state:&mut State,m:&mut Memory,allocation:usize)->Result<()>;
    /// Actual19662c with deleting=0. Managed object deletion is separate.
    fn destroy_base(&mut self,state:&mut State,m:&mut Memory)->Result<()>;
}
fn link(value:u32)->Option<usize>{(value!=0).then_some(value as usize)}
impl State {
    pub fn new(owner:Option<usize>,host:&mut impl Host)->Result<Self>{
        let mut state=Self{base:Agent::new(owner),references_34_38_3c_40_44:[None;5],word_48:u32::MAX,
            reference_4c:None,arrays_50_54:[None;2],count_58:0,capacity_5c:8,
            arrays_60_64:[None;2],count_68:0,capacity_6c:8,index_70:-1,
            scalars_74_78_7c_80_84:[0.,0.,0.,0.,1.],flag_88:0,speed_8c:1.,flags_90_91:[0;2],index_94:-1};
        state.speed_8c=host.global_unit_scale()?.unwrap_or(1.);Ok(state)
    }
    pub fn reset(&mut self,m:&mut Memory,host:&mut impl Host)->Result<()> {
        //196808 is the verified empty base virtual14; do not cache actions.
        self.flag_88=0;self.index_70=-1;self.flags_90_91[0]=0;self.scalars_74_78_7c_80_84[3]=-1.;
        if let Some(reservation)=self.references_34_38_3c_40_44[4] {
            let reserved_entity=m.word(u32::try_from(reservation)?.wrapping_add(0x24))? as usize;
            let entity=host.owner_entity(self)?;
            if reserved_entity==entity {host.release_reservation(self,m,reservation)?;}
        }
        self.references_34_38_3c_40_44[4]=None;
        if let Some(service)=self.references_34_38_3c_40_44[2] {
            let entity=host.owner_entity(self)?;host.detach_service(self,m,service,entity)?;
        }
        self.count_68=0;self.count_58=0;Ok(())
    }
    ///1bec0c tail-dispatches virtual14=1beb70 for this exact class.
    pub fn world_removed28(&mut self,m:&mut Memory,h:&mut impl Host)->Result<()>{self.reset(m,h)}
    ///2b7f18 is the verified empty insertion callback for all three agents.
    pub fn world_inserted2c(&mut self){}
    pub fn bind(&mut self,attributes:usize,m:&mut Memory,h:&mut impl BindingHost)->Result<bool>{
        self.references_34_38_3c_40_44[0]=h.owner_service(self)?;
        self.references_34_38_3c_40_44[1]=h.world_service(self,0x61d14)?;
        self.references_34_38_3c_40_44[2]=h.world_service(self,0x45dcc)?;
        let Some(service)=self.references_34_38_3c_40_44[0] else{return Ok(false)};
        self.reference_4c=h.service_3c(self,service)?;
        if self.references_34_38_3c_40_44[1].is_none(){return Ok(false)}
        let entity=h.owner_entity(self)?;
        self.speed_8c=match h.entity_speed(self,entity)?{Some(speed)=>speed,None=>h.global_unit_scale()?.unwrap_or(1.)};
        if !h.bind_base(self,m,attributes)?{return Ok(false)}
        if self.references_34_38_3c_40_44[3].is_none(){
            if let Some(service)=self.references_34_38_3c_40_44[0]{self.references_34_38_3c_40_44[3]=h.service_field_50(self,service)?;}
        }
        if self.index_94<0 || self.word_48==u32::MAX{return Ok(false)}
        self.references_34_38_3c_40_44[4]=None;
        let size=self.capacity_6c.wrapping_mul(4);self.arrays_60_64[0]=link(h.allocate(self,m,size)?);
        let count=self.capacity_5c;let size=count.wrapping_mul(12).wrapping_add(16);
        let p=h.allocate(self,m,size)?;
        self.arrays_50_54[0]=if p==0{None}else{
            //Actual DOL8000d334 header, then18db44 zero XYZ constructors.
            m.write(p,12)?;m.write(p.wrapping_add(4),count)?;
            let base=p.wrapping_add(16);
            for i in 0..count{for offset in [0,4,8]{m.write(base.wrapping_add(i.wrapping_mul(12)).wrapping_add(offset),0)?;}}
            Some(base as usize)
        };
        let size=self.capacity_6c.wrapping_mul(4);self.arrays_60_64[1]=link(h.allocate(self,m,size)?);
        let size=self.capacity_5c.wrapping_mul(4);self.arrays_50_54[1]=link(h.allocate(self,m,size)?);
        Ok(true)
    }
    pub fn destroy(&mut self,identity:usize,deleting:i32,m:&mut Memory,h:&mut impl DestructionHost)->Result<()> {
        if let Some(reservation)=self.references_34_38_3c_40_44[4]{
            let reserved_entity=m.word(u32::try_from(reservation)?.wrapping_add(0x24))? as usize;
            let entity=h.owner_entity(self)?;
            if reserved_entity==entity{h.release_reservation(self,m,reservation)?;}
        }
        if let Some(p)=self.arrays_60_64[0]{h.free(self,m,p)?;}
        if let Some(p)=self.arrays_50_54[0]{h.free(self,m,u32::try_from(p)?.wrapping_sub(16) as usize)?;}
        if let Some(p)=self.arrays_50_54[1]{h.free(self,m,p)?;}
        if let Some(p)=self.arrays_60_64[1]{h.free(self,m,p)?;}
        h.destroy_base(self,m)?;
        if deleting>0{h.free(self,m,identity)?;}
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    const BASE:usize=0x72000000;const OWNER:usize=0x73000000;const OTHER:usize=OWNER+0x100;
    const ENTITY:usize=0x74000000;const ENTITY2:usize=ENTITY+0x100;const RES:u32=0x75000000;
    const S0:usize=0x76000000;const S1:usize=S0+0x100;const S2:usize=S0+0x200;const S3:usize=S0+0x300;
    const A60:usize=0x78000000;const A50:usize=A60+0x1000;const A54:usize=A60+0x2000;const A64:usize=A60+0x3000;
    struct Fixture<'a>{c:&'a Value,trace:Vec<Value>,allocations:Vec<(u32,u32)>}
    impl Fixture<'_>{
        fn flag(&self,k:&str)->bool{self.c[k].as_bool().unwrap_or(true)}
        fn bits(&self,k:&str,default:u32)->u32{self.c[k].as_i64().map(|v|v as u32).or_else(||self.c[k].as_u64().map(|v|v as u32)).unwrap_or(default)}
    }
    impl Host for Fixture<'_>{
        fn global_unit_scale(&mut self)->Result<Option<f32>>{Ok(self.flag("world").then(||f32::from_bits(self.bits("global_speed",2f32.to_bits()))))}
        fn owner_entity(&mut self,s:&mut State)->Result<usize>{match s.base.owner{Some(OWNER)=>Ok(ENTITY),Some(OTHER)=>Ok(ENTITY2),_=>anyhow::bail!("Flee owner entity is unavailable")}}
        fn release_reservation(&mut self,s:&mut State,_:&mut Memory,r:usize)->Result<()>{
            self.trace.push(json!(["release",r,s.flag_88,s.index_70 as u32,s.flags_90_91[0],s.scalars_74_78_7c_80_84[3].to_bits(),s.count_58,s.count_68]));
            match self.c["mutation"].as_str(){
                Some("service_clear"|"both")=>s.references_34_38_3c_40_44[2]=None,
                Some("service_swap")=>s.references_34_38_3c_40_44[2]=Some(S1),_=>{}
            }
            if matches!(self.c["mutation"].as_str(),Some("owner_swap"|"both")){s.base.owner=Some(OTHER);}
            Ok(())
        }
        fn detach_service(&mut self,s:&mut State,_:&mut Memory,p:usize,e:usize)->Result<()>{
            self.trace.push(json!(["detach",p,e,s.references_34_38_3c_40_44[4].unwrap_or(0),s.count_58,s.count_68]));
            if self.c["mutation"]=="counts"{s.count_58=99;s.count_68=98;}Ok(())
        }
    }
    impl BindingHost for Fixture<'_>{
        fn owner_service(&mut self,s:&mut State)->Result<Option<usize>>{self.trace.push(json!(["owner_service",s.base.owner.unwrap_or(0)]));Ok(self.flag("owner_service").then_some(S0))}
        fn world_service(&mut self,_:&mut State,c:u32)->Result<Option<usize>>{
            self.trace.push(json!(["world_service",c]));Ok(match c{0x61d14=>self.flag("required_world").then_some(S1),0x45dcc=>self.flag("optional_world").then_some(S2),_=>anyhow::bail!("Unknown Flee world class")})
        }
        fn service_3c(&mut self,_:&mut State,p:usize)->Result<Option<Acceptance>>{self.trace.push(json!(["service_3c",p]));Ok(Some(Acceptance::External(105)))}
        fn entity_speed(&mut self,_:&mut State,e:usize)->Result<Option<f32>>{self.trace.push(json!(["entity_speed",e]));Ok(self.flag("speed_record").then(||f32::from_bits(self.bits("entity_speed",3f32.to_bits()))))}
        fn bind_base(&mut self,s:&mut State,_:&mut Memory,a:usize)->Result<bool>{
            self.trace.push(json!(["base_bind",a,s.references_34_38_3c_40_44[0].unwrap_or(0),s.references_34_38_3c_40_44[1].unwrap_or(0),s.references_34_38_3c_40_44[2].unwrap_or(0),external(s.reference_4c),s.speed_8c.to_bits()]));
            s.base.action_04=Some(99);s.base.speed=11.;s.word_48=self.bits("word48",3);s.index_94=self.bits("index94",5) as i32;
            s.capacity_5c=self.bits("capacity5c",3);s.capacity_6c=self.bits("capacity6c",2);
            if self.c["mutation"]=="base_service_clear"{s.references_34_38_3c_40_44[0]=None;}
            if self.c["mutation"]=="base_service_swap"{s.references_34_38_3c_40_44[0]=Some(S1);}
            Ok(self.flag("base_bind"))
        }
        fn service_field_50(&mut self,_:&mut State,p:usize)->Result<Option<usize>>{Ok(Some(match p{S0=>S3,S1=>12345,_=>anyhow::bail!("Unknown Flee owner service")}))}
        fn allocate(&mut self,s:&mut State,m:&mut Memory,n:u32)->Result<u32>{
            let i=self.allocations.len();let p=if self.c["fail_allocation"].as_u64()==Some(i as u64){0}else{0x79000000+i as u32*0x1000};
            self.trace.push(json!(["allocate",n,p]));self.allocations.push((p,n));
            if p!=0{anyhow::ensure!(m.regions.insert(p,vec![0xcd;n as usize]).is_none(),"Repeated live Flee allocation");}
            self.trace.push(json!(["observe",n,p]));
            if self.c["mutation"]=="first_capacities" && i==0{s.capacity_5c=1;s.capacity_6c=4;}
            if self.c["mutation"]=="array_capacity" && i==1{s.capacity_5c=2;s.capacity_6c=5;}
            Ok(p)
        }
    }
    impl DestructionHost for Fixture<'_>{
        fn free(&mut self,s:&mut State,_:&mut Memory,p:usize)->Result<()>{
            self.trace.push(json!(["free",p]));
            if self.c["mutation"]=="free_swap" && p==A60{s.arrays_50_54[0]=Some(A54+16);s.arrays_60_64[1]=None;}
            if self.c["mutation"]=="free_revive" && p==A50{s.arrays_50_54[1]=Some(12345);}Ok(())
        }
        fn destroy_base(&mut self,s:&mut State,_:&mut Memory)->Result<()>{self.trace.push(json!(["base_destroy",s.base.action_04.unwrap_or(0),s.base.owner.unwrap_or(0),s.references_34_38_3c_40_44[4].unwrap_or(0)]));Ok(())}
    }
    fn image(s:&State)->Vec<u32>{
        let b=&s.base;
        let mut v=vec![b.action_04.unwrap_or(0) as u32,b.owner.unwrap_or(0) as u32,b.world_service_61d14.unwrap_or(0) as u32,b.owner_service_6af6c.unwrap_or(0) as u32,b.speed.to_bits()];
        v.extend(b.action_vector.map(f32::to_bits));v.extend([b.action_scalar.to_bits(),b.bound_owner.unwrap_or(0) as u32,b.blackboard_head as u32,b.world_service_45dcc.unwrap_or(0) as u32]);
        v.extend(s.references_34_38_3c_40_44.map(|p|p.unwrap_or(0) as u32));
        v.extend([s.word_48,external(s.reference_4c) as u32,s.arrays_50_54[0].unwrap_or(0) as u32,s.arrays_50_54[1].unwrap_or(0) as u32,s.count_58,s.capacity_5c,s.arrays_60_64[0].unwrap_or(0) as u32,s.arrays_60_64[1].unwrap_or(0) as u32,s.count_68,s.capacity_6c,s.index_70 as u32]);
        v.extend(s.scalars_74_78_7c_80_84.map(f32::to_bits));v.extend([s.speed_8c.to_bits(),s.index_94 as u32]);v
    }
    #[test]
    fn flee_native_construction_binding_and_lifecycle_match_original_instructions()->Result<()>{
        let f:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_flee_agent.json"))?;
        for case in f["cases"].as_array().context("Flee reference cases absent")?{
            let c=&case["input"];let mut h=Fixture{c,trace:vec![],allocations:vec![]};
            let mut s=State::new(h.flag("owner").then_some(OWNER),&mut h)?;
            let mut m=Memory{regions:Default::default(),resources:Default::default(),stride:0,journal:Default::default()};
            m.regions.insert(RES,vec![0;0x28]);m.write(RES+0x24,if h.flag("reservation_owner"){ENTITY}else{ENTITY2} as u32)?;
            if c["op"]!="new"{
                s.base.action_04=Some(90);s.base.world_service_61d14=Some(91);s.base.owner_service_6af6c=Some(92);s.base.speed=7.;s.base.action_vector=[1.,-2.,3.];s.base.action_scalar=4.;
                s.base.bound_owner=Some(OTHER);s.base.blackboard_head=93;s.base.world_service_45dcc=Some(94);
                s.references_34_38_3c_40_44=[Some(S0),Some(S1),h.flag("service").then_some(S2),(!c["missing_40"].as_bool().unwrap_or(false)).then_some(S3),h.flag("reservation").then_some(RES as usize)];
                s.word_48=6;s.reference_4c=Some(Acceptance::External(105));s.arrays_50_54=[Some(A50+16),Some(A54)];s.count_58=3;s.capacity_5c=8;
                s.arrays_60_64=[Some(A60),Some(A64)];s.count_68=4;s.capacity_6c=8;s.index_70=12;
                s.scalars_74_78_7c_80_84=[11.,12.,13.,14.,15.];s.flag_88=255;s.speed_8c=9.;s.flags_90_91=[254,253];s.index_94=7;
            }
            let result=match c["op"].as_str().unwrap(){
                "new"=>None,"reset"=>{s.reset(&mut m,&mut h)?;None},"remove"=>{s.world_removed28(&mut m,&mut h)?;None},
                "bind"=>Some(s.bind(88,&mut m,&mut h)?),"destroy"=>{s.destroy(BASE,c["deleting"].as_i64().unwrap_or(0) as i32,&mut m,&mut h)?;None},_=>anyhow::bail!("Unknown Flee fixture operation")
            };
            let allocations=h.allocations.iter().filter(|(p,_)|*p!=0).map(|(p,_)|json!([p,m.regions[p].iter().map(|b|format!("{b:02x}")).collect::<String>()])).collect::<Vec<_>>();
            let actual=json!({"words":image(&s),"bytes":[s.flag_88,s.flags_90_91[0],s.flags_90_91[1]],"trace":h.trace,"allocations":allocations,"result":result});
            for key in actual.as_object().unwrap().keys(){assert_eq!(actual[key],case[key],"{key}: {c}");}
        }Ok(())
    }
}
