//! Per-agent paths19544c/1c1854/196fc8, not shared CPathFinder48f0cc.
//! One canonical mapped object in the existing navigation arena. Unknown bytes
//! remain untouched; these are Rust operations on data, not Wii execution.
use anyhow::{Result,ensure};
use super::ai_graph_link_passes::Memory;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Kind {Base,Wander,Goto}
impl Kind {
    pub fn size(self)->u32{match self{Self::Base=>0x86c,Self::Wander=>0x878,Self::Goto=>0x8b4}}
    pub fn traversal(self)->Result<&'static [u8]>{match self{Self::Wander=>Ok(b"WanderTraversal"),Self::Goto=>Ok(b"GotoTraversal"),Self::Base=>anyhow::bail!("Base path has no derived traversal factory")}}
}
#[derive(Clone,Copy)]
pub struct Context {pub point_attribute_class:u32,pub global_vector:u32}
const BASE:u32=0x4eb980;const WANDER:u32=0x4eb818;const GOTO:u32=0x4eae38;
fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn put(m:&mut Memory,p:u32,o:u32,v:u32)->Result<()>{m.write(p.wrapping_add(o),v)}
pub fn construct(m:&mut Memory,c:Context,p:u32,kind:Kind)->Result<()> {
    ensure!(p!=0,"Required agent path allocation is null");
    put(m,p,0x868,BASE)?;put(m,p,0,u32::MAX)?;put(m,p,0x14,0x4ec1b0)?;
    for o in [0x18,0x1c,0x20,4]{put(m,p,o,0)?;}
    m.write_byte(p.wrapping_add(8),0)?;put(m,p,12,0)?;m.write_byte(p.wrapping_add(0x10),0)?;
    for o in (0x24..=0x60).step_by(4){put(m,p,o,0)?;}
    put(m,p,0x64,0x4ebe80)?;put(m,p,0x68,u32::MAX)?;
    for o in [0x6c,0x70,0x74,0x78,0x7c]{put(m,p,o,0)?;}
    m.write_byte(p.wrapping_add(0x80),0)?;
    for o in [0x88,0x8c,0x90]{put(m,p,o,0)?;}
    //99 is the original fixed sample array length, not a runtime cap.
    for i in 0..99u32{for o in [4,8,12]{put(m,p,0x98+i*0x14+o,0)?;}}
    for o in [0x854,0x858,0x85c,0x860,0x864]{put(m,p,o,0)?;}
    put(m,p,0x68,u32::MAX)?;
    //465680, with pure getter2b796c for the inline attribute's class.
    let slot=m.word(c.point_attribute_class.wrapping_add(8))?;
    put(m,p,0x24u32.wrapping_add(slot.wrapping_mul(4)),p.wrapping_add(0x64))?;
    match kind {
        Kind::Base=>{},
        Kind::Wander=>{put(m,p,0x868,WANDER)?;put(m,p,0x870,0)?;put(m,p,0x86c,0x4eb8a0)?;},
        Kind::Goto=>{
            put(m,p,0x868,GOTO)?;m.write_byte(p.wrapping_add(0x86c),0)?;
            for (dst,src) in [(0x870,0),(0x874,4),(0x878,8)]{let v=word(m,c.global_vector,src)?;put(m,p,dst,v)?;}
            put(m,p,0x87c,u32::MAX)?;put(m,p,0x880,0)?;put(m,p,0x888,0)?;put(m,p,0x884,0x4eb7a8)?;
            for o in [0x88c,0x890,0x894]{put(m,p,o,0)?;}
            for (dst,src) in [(0x898,0),(0x89c,4),(0x8a0,8)]{let v=word(m,c.global_vector,src)?;put(m,p,dst,v)?;}
            m.write_byte(p.wrapping_add(0x8a4),0)?;put(m,p,0x8a8,0)?;put(m,p,0x8ac,0)?;
            m.write_byte(p.wrapping_add(0x8b0),0)?;put(m,p,0x864,1f32.to_bits())?;
        }
    }Ok(())
}
pub trait ResetHost {
    fn reservation(&mut self,m:&mut Memory,manager:u32,traversal:u32,owner:u32)->Result<u32>;
    fn release(&mut self,m:&mut Memory,reservation:u32)->Result<()>;
}
///Actual195a84/1c1f1c virtual14. Select the live class before release;
///release callbacks may mutate fields, then these original writes still occur.
pub fn release(m:&mut Memory,p:u32,r:u32,h:&mut impl ResetHost)->Result<()> {
    let class=word(m,p,0x868)?;
    ensure!([BASE,WANDER,GOTO].contains(&class),"Agent path reservation release class is not implemented");
    h.release(m,r)?;
    m.write_byte(p.wrapping_add(0x80),0)?;m.write_byte(p.wrapping_add(0x10),0)?;
    if class==GOTO{m.write_byte(p.wrapping_add(0x8a4),0)?;}
    Ok(())
}
///195a0c uses the LIVE vtable after reservation lookup, which may change it.
pub fn reset(m:&mut Memory,p:u32,h:&mut impl ResetHost)->Result<()> {
    if m.byte(p.wrapping_add(0x80))?!=0 {
        let manager=word(m,p,4)?;let traversal=word(m,p,0)?;let owner=word(m,p,0x7c)?;
        let r=h.reservation(m,manager,traversal,owner)?;
        if r!=0 {
            release(m,p,r,h)?;
        }
    }
    m.write_byte(p.wrapping_add(8),0)?;m.write_byte(p.wrapping_add(0x10),0)?;put(m,p,0x68,u32::MAX)
}
pub trait DestructionHost:ResetHost {
    /// Global51bf38 free is distinct from C++8000b8a4, including null buffer.
    fn free_global(&mut self,m:&mut Memory,p:u32)->Result<()>;
    fn free_cpp(&mut self,m:&mut Memory,p:u32)->Result<()>;
}
///195670/2b7d08/1c1984. Inline destructors184538/196ca8/18d5d4 with
///deleting=0 are verified empty; do not invent reference release side effects.
pub fn destroy(m:&mut Memory,p:u32,kind:Kind,deleting:i32,h:&mut impl DestructionHost)->Result<u32> {
    if p==0{return Ok(0)}
    if kind==Kind::Goto {
        put(m,p,0x868,GOTO)?;
        let buffer=word(m,p,0x880)?;h.free_global(m,buffer)?;
        // Fresh action pointer after the buffer free callback.
        let action=word(m,p,0x8ac)?;
        if action!=0 {
            ensure!(m.word(action)?==0x4eaec4,"Goto path action destructor class is not implemented");
            h.free_global(m,action)?; //18d4b8 with deleting=1.
        }
    }
    // Wander's inline+86c destructor and Goto's inline+884 destructor have
    // deleting=0, then both invoke the same base destructor with deleting=0.
    put(m,p,0x868,BASE)?;reset(m,p,h)?;
    if deleting>0{h.free_cpp(m,p)?;}
    Ok(p)
}
pub trait FactoryHost:BindingHost {
    /// C++ allocation8000b7fc, including actual allocator/observer work.
    fn allocate_object(&mut self,m:&mut Memory,bytes:u32)->Result<u32>;
    /// Exact derived virtual+c, not a ready flag. Return is compared as a word.
    fn bind(&mut self,m:&mut Memory,path:u32,kind:Kind,name:&[u8],owner:usize)->Result<u32> where Self:Sized {
        Ok(u32::from(match kind {
            Kind::Goto=>bind_goto(m,path,Some(name),Some(owner),self)?,
            Kind::Wander=>bind_base(m,path,Some(name),Some(owner),self)?,
            Kind::Base=>anyhow::bail!("Base path has no concrete factory binding"),
        }))
    }
}
pub fn create(m:&mut Memory,c:Context,kind:Kind,owner:usize,h:&mut impl FactoryHost)->Result<u32> {
    let name=kind.traversal()?;let p=h.allocate_object(m,kind.size())?;
    // Original dereferences+868 even when allocation returned null.
    construct(m,c,p,kind)?;
    Ok(if h.bind(m,p,kind,name,owner)?==0{0}else{p})
}
pub trait BindingHost {
    fn world_service(&mut self,m:&mut Memory,path:u32)->Result<u32>;
    fn owner_service(&mut self,m:&mut Memory,path:u32,owner:usize)->Result<u32>;
    fn owner_word40(&mut self,m:&mut Memory,path:u32,owner:usize)->Result<u32>;
    fn traversal(&mut self,m:&mut Memory,path:u32,manager:u32,name:Option<&[u8]>)->Result<u32>;
    fn buffer_size(&mut self,m:&mut Memory,service:u32)->Result<u32>;
    fn radius(&mut self,m:&mut Memory,service:u32)->Result<f32>;
    /// Includes native observer before returned pointer is stored by caller.
    fn allocate(&mut self,m:&mut Memory,path:u32,bytes:u32)->Result<u32>;
}
pub fn bind_base(m:&mut Memory,p:u32,name:Option<&[u8]>,owner:Option<usize>,h:&mut impl BindingHost)->Result<bool> {
    let manager=h.world_service(m,p)?;put(m,p,4,manager)?;
    if manager==0{return Ok(false)}
    let Some(owner)=owner else{return Ok(false)};
    let service=h.owner_service(m,p,owner)?;put(m,p,12,service)?;
    let manager=word(m,p,4)?;
    let actor=h.owner_word40(m,p,owner)?;put(m,p,0x7c,actor)?;
    let traversal=h.traversal(m,p,manager,name)?;
    if traversal==u32::MAX{return Ok(false)}
    put(m,p,0,traversal)?;Ok(true)
}
pub fn bind_goto(m:&mut Memory,p:u32,name:Option<&[u8]>,owner:Option<usize>,h:&mut impl BindingHost)->Result<bool> {
    let bound=bind_base(m,p,name,owner,h)?;
    if bound {
        let service=word(m,p,12)?;ensure!(service!=0,"Goto path has no service for its buffer");
        let bytes=h.buffer_size(m,service)?;let allocation=h.allocate(m,p,bytes)?;
        put(m,p,0x880,allocation)?;ensure!(allocation!=0,"Goto path buffer allocation is null");
        m.write(allocation,bytes)?;m.write_byte(p.wrapping_add(0x8a4),0)?;
    }
    let service=word(m,p,12)?;ensure!(service!=0,"Goto path has no service after base bind");
    let radius=h.radius(m,service)?;put(m,p,0x8a8,(radius*radius).to_bits())?;
    let action=h.allocate(m,p,0x18)?;
    if action!=0 {
        put(m,action,0,0x4eaec4)?;put(m,action,4,0)?;
        for o in [8,12,16,20]{put(m,action,o,0)?;}
    }
    put(m,p,0x8ac,action)?;Ok(action!=0 && bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context as _;
    use serde_json::{Value,json};
    use std::io::Read;
    const P:u32=0x72000000;const OWNER:u32=0x73000000;const S:u32=0x74000000;const S2:u32=S+0x1000;
    const R:u32=0x75000000;const R2:u32=R+0x1000;const MANAGER:u32=0x76000000;const RES:u32=0x77000000;const HEAP:u32=0x79000000;
    fn unpack(hex:&str)->Result<Vec<u8>> {
        let compressed=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16)).collect::<std::result::Result<Vec<_>,_>>()?;
        let mut data=Vec::new();flate2::read::ZlibDecoder::new(&compressed[..]).read_to_end(&mut data)?;Ok(data)
    }
    fn image(m:&Memory,p:u32,n:usize)->Result<Vec<u8>>{(0..n).map(|i|m.byte(p.wrapping_add(i as u32))).collect()}
    struct Fixture<'a>{c:&'a Value,trace:Vec<Value>,allocs:Vec<(u32,u32)>}
    impl Fixture<'_> {
        fn flag(&self,k:&str)->bool{self.c[k].as_bool().unwrap_or(true)}
    }
    impl ResetHost for Fixture<'_> {
        fn reservation(&mut self,m:&mut Memory,manager:u32,id:u32,owner:u32)->Result<u32>{
            self.trace.push(json!(["reservation",manager,id,owner]));
            if self.c["mutation"]=="goto"{put(m,P,0x868,GOTO)?;}
            if self.c["mutation"]=="clear"{m.write_byte(P+0x80,0)?;}
            if self.c["mutation"]=="header"{m.write_byte(P+0x10,9)?;put(m,P,0x68,77)?;}
            Ok(if self.flag("reservation"){RES}else{0})
        }
        fn release(&mut self,m:&mut Memory,r:u32)->Result<()>{self.trace.push(json!(["release",r,m.byte(P+0x80)?,m.byte(P+0x10)?,word(m,P,0x68)?]));Ok(())}
    }
    impl DestructionHost for Fixture<'_> {
        fn free_global(&mut self,m:&mut Memory,p:u32)->Result<()> {
            self.trace.push(json!(["free_global",p,word(m,P,0x868)?,m.byte(P+0x80)?]));
            if p==HEAP {
                if self.c["mutation"]=="action_clear"{put(m,P,0x8ac,0)?;}
                if self.c["mutation"]=="action_swap"{put(m,P,0x8ac,HEAP+0x2100)?;}
            }Ok(())
        }
        fn free_cpp(&mut self,m:&mut Memory,p:u32)->Result<()> {self.trace.push(json!(["free_cpp",p,word(m,P,0x868)?,m.byte(P+0x80)?]));Ok(())}
    }
    impl BindingHost for Fixture<'_> {
        fn world_service(&mut self,_:&mut Memory,_:u32)->Result<u32>{self.trace.push(json!(["world"]));Ok(if self.flag("world"){MANAGER}else{0})}
        fn owner_service(&mut self,m:&mut Memory,p:u32,o:usize)->Result<u32>{self.trace.push(json!(["service",o]));if self.c["mutation"]=="service_world"{put(m,p,4,0)?;}Ok(S)}
        fn owner_word40(&mut self,m:&mut Memory,_:u32,o:usize)->Result<u32>{word(m,u32::try_from(o)?,0x40)}
        fn traversal(&mut self,_:&mut Memory,_:u32,manager:u32,_:Option<&[u8]>)->Result<u32>{self.trace.push(json!(["traversal",manager]));Ok(self.c["traversal"].as_i64().unwrap_or(4) as u32)}
        fn buffer_size(&mut self,m:&mut Memory,s:u32)->Result<u32>{word(m,s,0x94)}
        fn radius(&mut self,m:&mut Memory,s:u32)->Result<f32>{Ok(f32::from_bits(word(m,word(m,s,0x134)?,12)?))}
        fn allocate(&mut self,m:&mut Memory,p:u32,n:u32)->Result<u32>{
            let i=self.allocs.len();let mut address=HEAP+i as u32*0x1000;
            let bound=self.flag("world")&&self.flag("owner")&&self.c["traversal"].as_i64().unwrap_or(4)!=-1;
            if self.c["fail_action"]==true && n==24 && i==usize::from(bound){address=0;}
            self.trace.push(json!(["allocate",n,address]));self.allocs.push((address,n));
            if address!=0{m.regions.insert(address,vec![0xa5;n as usize]);}
            self.trace.push(json!(["observe",address,n,99]));
            if self.c["mutation"]=="first_service" && i==0{put(m,p,12,S2)?;}
            if self.c["mutation"]=="action_flag" && i==1{m.write_byte(p+0x8a4,9)?;}
            Ok(address)
        }
    }
    impl FactoryHost for Fixture<'_> {
        fn allocate_object(&mut self,_:&mut Memory,n:u32)->Result<u32>{self.trace.push(json!(["object",n]));Ok(P)}
        fn bind(&mut self,_:&mut Memory,p:u32,_:Kind,name:&[u8],owner:usize)->Result<u32>{self.trace.push(json!(["bind",p,std::str::from_utf8(name)?,owner]));Ok(self.c["bind_result"].as_u64().unwrap_or(1) as u32)}
    }
    // Reborrow adapter exercises the production FactoryHost::bind default;
    // Fixture itself deliberately overrides that virtual for bounded cases.
    impl BindingHost for &mut Fixture<'_> {
        fn world_service(&mut self,m:&mut Memory,p:u32)->Result<u32>{<Fixture<'_> as BindingHost>::world_service(*self,m,p)}
        fn owner_service(&mut self,m:&mut Memory,p:u32,o:usize)->Result<u32>{<Fixture<'_> as BindingHost>::owner_service(*self,m,p,o)}
        fn owner_word40(&mut self,m:&mut Memory,p:u32,o:usize)->Result<u32>{<Fixture<'_> as BindingHost>::owner_word40(*self,m,p,o)}
        fn traversal(&mut self,m:&mut Memory,p:u32,w:u32,n:Option<&[u8]>)->Result<u32>{<Fixture<'_> as BindingHost>::traversal(*self,m,p,w,n)}
        fn buffer_size(&mut self,m:&mut Memory,s:u32)->Result<u32>{<Fixture<'_> as BindingHost>::buffer_size(*self,m,s)}
        fn radius(&mut self,m:&mut Memory,s:u32)->Result<f32>{<Fixture<'_> as BindingHost>::radius(*self,m,s)}
        fn allocate(&mut self,m:&mut Memory,p:u32,n:u32)->Result<u32>{<Fixture<'_> as BindingHost>::allocate(*self,m,p,n)}
    }
    impl FactoryHost for &mut Fixture<'_> {
        fn allocate_object(&mut self,m:&mut Memory,n:u32)->Result<u32>{<Fixture<'_> as FactoryHost>::allocate_object(*self,m,n)}
    }
    #[test]
    fn original_path_constructors_reset_factories_and_bindings_match_all_bytes()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_agent_path.json"))?;
        for row in fixture["cases"].as_array().context("Missing path cases")? {
            let c=&row["input"];let op=c["op"].as_str().unwrap();let kind=match c["kind"].as_str().unwrap_or("Goto"){"Base"=>Kind::Base,"Wander"=>Kind::Wander,_=>Kind::Goto};
            let size=if op=="factory"{kind.size() as usize}else{0x8b4};
            let mut m=Memory::default();for (p,n,fill) in [(P,size,0xcd),(OWNER,0x100,0),(S,0x200,0),(S2,0x200,0),(R,0x10,0),(R2,0x10,0),(0x6729c,0x20,0),(0x564d0,12,0)]{m.regions.insert(p,vec![fill;n]);}
            put(&mut m,0x6729c,8,c["slot"].as_u64().unwrap_or(3) as u32)?;
            for i in 0..3{let v=c["vector"][i].as_f64().unwrap_or([1.25,-2.5,3.75][i]) as f32;put(&mut m,0x564d0,i as u32*4,v.to_bits())?;}
            put(&mut m,OWNER,0x40,123)?;put(&mut m,S,0x94,24)?;put(&mut m,S2,0x94,40)?;
            put(&mut m,S,0x134,R)?;put(&mut m,S2,0x134,R2)?;put(&mut m,R,12,3f32.to_bits())?;put(&mut m,R2,12,5f32.to_bits())?;
            if ["reset","destroy","bind_base","bind_goto"].contains(&op){
                for (o,v) in [(0,13),(4,MANAGER),(12,S),(0x68,27),(0x7c,123),(0x868,match kind{Kind::Base=>BASE,Kind::Wander=>WANDER,Kind::Goto=>GOTO})]{put(&mut m,P,o,v)?;}
                for (o,v) in [(8,6),(0x10,5),(0x80,c["active"].as_u64().unwrap_or(1) as u8),(0x8a4,7)]{m.write_byte(P+o,v)?;}
            }
            if op=="destroy" {
                put(&mut m,P,0x880,HEAP)?;put(&mut m,P,0x8ac,HEAP+0x2000)?;
                for a in [HEAP+0x2000,HEAP+0x2100]{m.regions.insert(a,vec![0xa5;24]);m.write(a,0x4eaec4)?;}
            }
            let mut h=Fixture{c,trace:vec![],allocs:vec![]};let context=Context{point_attribute_class:0x6729c,global_vector:0x564d0};
            let result=match op {
                "construct"=>{construct(&mut m,context,P,kind)?;Value::Null},
                "reset"=>{reset(&mut m,P,&mut h)?;Value::Null},
                "destroy"=>json!(destroy(&mut m,if c["null"]==true{0}else{P},kind,c["deleting"].as_i64().unwrap_or(0) as i32,&mut h)?),
                "factory"=>{if c["full_factory"]==true {let mut borrowed=&mut h;json!(create(&mut m,context,kind,OWNER as usize,&mut borrowed)?)}else{json!(create(&mut m,context,kind,OWNER as usize,&mut h)?) }},
                "bind_base"=>json!(bind_base(&mut m,P,Some(b"GotoTraversal"),h.flag("owner").then_some(OWNER as usize),&mut h)?),
                "bind_goto"=>json!(bind_goto(&mut m,P,Some(b"GotoTraversal"),h.flag("owner").then_some(OWNER as usize),&mut h)?),_=>unreachable!(),
            };
            let expected=unpack(row["image"].as_str().unwrap())?;let actual=image(&m,P,size)?;
            assert_eq!(actual.len(),expected.len());
            for (i,(a,b)) in actual.iter().zip(&expected).enumerate(){assert_eq!(a,b,"byte {i:x}: {c}");}
            assert_eq!(result,row["result"],"result: {c}");assert_eq!(json!(h.trace),row["trace"],"trace: {c}");
            let expected_allocations=row["allocations"].as_array().unwrap();let allocations=h.allocs.iter().filter(|(p,_)|*p!=0).collect::<Vec<_>>();assert_eq!(allocations.len(),expected_allocations.len());
            for ((p,n),e) in allocations.into_iter().zip(expected_allocations){assert_eq!(*p,e[0].as_u64().unwrap() as u32);assert_eq!(image(&m,*p,*n as usize)?,unpack(e[1].as_str().unwrap())?,"allocation: {c}");}
        }Ok(())
    }
}
