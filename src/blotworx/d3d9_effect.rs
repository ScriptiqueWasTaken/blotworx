//! Original FXO parameter/technique/pass structure. Values remain typed raw
//! words until a version adapter supplies their scene/renderer bindings.
use anyhow::{ensure,Context,Result};
use std::{collections::BTreeSet,ops::Range};
#[derive(Debug)]
pub(super) enum Data { Words(Vec<u32>), Objects(Vec<u32>), Sampler(Vec<State>) }
#[derive(Debug)]
pub(super) struct Value {
    pub kind:u32,pub class:u32,pub name:Option<String>,pub semantic:Option<String>,
    pub elements:u32,pub columns:u32,pub rows:u32,pub data:Data,
}
#[derive(Debug)]
pub(super) struct State {pub kind:u32,pub index:u32,pub value:Value}
#[derive(Debug)]
pub(super) struct Parameter {pub value:Value,pub flags:u32,pub annotations:Vec<Value>}
#[derive(Debug)]
pub(super) struct Pass {pub name:Option<String>,pub annotations:Vec<Value>,pub states:Vec<State>}
#[derive(Debug)]
pub(super) struct Technique {pub name:Option<String>,pub annotations:Vec<Value>,pub passes:Vec<Pass>}
#[derive(Debug)]
pub(super) struct Object {pub id:u32,pub payload:Range<usize>,pub source_kind:Option<u32>}
#[derive(Debug)]
pub(super) struct Effect {pub parameters:Vec<Parameter>,pub techniques:Vec<Technique>,pub objects:Vec<Object>,pub object_slots:u32,pub trailing_bytes:usize}
#[derive(Debug)]
pub(super) enum Shader {
    FixedFunction,
    Program {version:u32,payload:Range<usize>},
    Selector {payload:Range<usize>},
}
#[derive(Debug)]
pub(super) struct Pair {pub vertex:Option<Shader>,pub pixel:Option<Shader>}
struct Reader<'a> {data:&'a [u8],cursor:usize}
impl<'a> Reader<'a> {
    fn bytes(&self,at:usize,len:usize)->Result<&'a [u8]> {self.data.get(at..at.checked_add(len).context("FXO range overflow")?).context("FXO range outside resource")}
    fn word(&self,at:usize)->Result<u32>{Ok(u32::from_le_bytes(self.bytes(at,4)?.try_into()?))}
    fn next(&mut self)->Result<u32>{let word=self.word(self.cursor)?;self.cursor+=4;Ok(word)}
    fn count(&mut self,stride:usize)->Result<usize>{let count=self.next()? as usize;self.bytes(self.cursor,count.checked_mul(stride).context("FXO count overflow")?)?;Ok(count)}
    fn string(&self,offset:u32)->Result<Option<String>> {
        let at=8usize.checked_add(offset as usize).context("FXO string overflow")?;
        let len=self.word(at)? as usize;if len==0{return Ok(None);}
        let data=self.bytes(at+4,len)?;ensure!(data.last()==Some(&0),"Unterminated FXO string");
        Ok(Some(std::str::from_utf8(&data[..len-1])?.to_owned()))
    }
    fn words(&self,at:usize,count:usize)->Result<Vec<u32>> {
        Ok(self.bytes(at,count.checked_mul(4).context("FXO word count overflow")?)?.chunks_exact(4).map(|w|u32::from_le_bytes(w.try_into().unwrap())).collect())
    }
    fn value(&self,ty:u32,value:u32,visiting:&mut BTreeSet<(u32,u32)>)->Result<Value> {
        ensure!(visiting.insert((ty,value)),"Recursive FXO value");
        let at=8+ty as usize;self.bytes(at,20)?;
        let kind=self.word(at)?;let class=self.word(at+4)?;
        let name=self.string(self.word(at+8)?)?;let semantic=self.string(self.word(at+12)?)?;
        let elements=self.word(at+16)?;let at_value=8+value as usize;
        let (columns,rows,data)=match class {
            0..=3=>{
                ensure!((1..=3).contains(&kind),"Invalid numeric FXO type");
                let columns=self.word(at+20)?;let rows=self.word(at+24)?;
                let count=(columns as usize).checked_mul(rows as usize).and_then(|n|n.checked_mul(elements.max(1) as usize)).context("FXO numeric value overflow")?;
                (columns,rows,Data::Words(self.words(at_value,count)?))
            },
            4 if (10..=14).contains(&kind)=>{
                let mut sampler=Reader {data:self.data,cursor:at_value};let count=sampler.count(16)?;
                let mut states=Vec::with_capacity(count);
                for _ in 0..count {states.push(sampler.state(visiting,true)?);}
                (0,0,Data::Sampler(states))
            },
            4=>{ensure!((4..=16).contains(&kind),"Invalid object FXO type");(0,0,Data::Objects(self.words(at_value,elements.max(1) as usize)?))},
            _=>anyhow::bail!("FXO value class {class} requires its struct decoder"),
        };
        visiting.remove(&(ty,value));
        Ok(Value {kind,class,name,semantic,elements,columns,rows,data})
    }
    fn annotations(&mut self,count:usize)->Result<Vec<Value>> {
        self.bytes(self.cursor,count.checked_mul(8).context("FXO annotations overflow")?)?;
        let mut result=Vec::with_capacity(count);
        for _ in 0..count {let ty=self.next()?;let value=self.next()?;result.push(self.value(ty,value,&mut BTreeSet::new())?);}
        Ok(result)
    }
    fn state(&mut self,visiting:&mut BTreeSet<(u32,u32)>,sampler:bool)->Result<State> {
        let kind=self.next()?;let index=self.next()?;let ty=self.next()?;let value=self.next()?;
        Ok(State {kind:if sampler {kind&!0xa0}else{kind},index,value:self.value(ty,value,visiting)?})
    }
    fn object_payload(&mut self,len:usize)->Result<Range<usize>> {
        let start=self.cursor;self.bytes(start,len)?;
        let aligned=len.checked_add(3).context("FXO object size overflow")?&!3;
        self.bytes(start,aligned)?;self.cursor+=aligned;Ok(start..start+len)
    }
}
impl Effect {
    pub fn shader_pair(&self,data:&[u8],technique:usize,pass:usize)->Result<Pair> {
        let pass=self.techniques.get(technique).context("Effect technique is absent")?.passes.get(pass).context("Effect pass is absent")?;
        let mut pair=Pair {vertex:None,pixel:None};
        for state in &pass.states {
            if !matches!(state.kind,146|147) {continue;}
            let Data::Objects(ids)=&state.value.data else{anyhow::bail!("Effect shader state is not an object")};
            ensure!(ids.len()==1,"Effect shader state has multiple objects");let id=ids[0];
            let object=self.objects.iter().find(|o|o.id==id);
            let shader=match object {
                None if id==0=>Shader::FixedFunction,
                None=>anyhow::bail!("Effect shader object {id} is absent"),
                Some(object) if object.payload.is_empty()=>Shader::FixedFunction,
                Some(object) if object.source_kind==Some(2)=>Shader::Selector {payload:object.payload.clone()},
                Some(object)=>{
                    let bytes=data.get(object.payload.clone()).context("Effect shader object outside resource")?;
                    let version=u32::from_le_bytes(bytes.get(..4).context("Effect shader object is truncated")?.try_into()?);
                    let stage=version>>16;ensure!(stage==if state.kind==146 {0xfffe}else{0xffff},"Effect shader stage mismatch");
                    Shader::Program {version,payload:object.payload.clone()}
                },
            };
            // Assignment order is authored pass order. Later writes replace
            // earlier writes exactly as the effect state stream specifies.
            if state.kind==146 {pair.vertex=Some(shader);}else{pair.pixel=Some(shader);}
        }
        Ok(pair)
    }
    pub fn parse(data:&[u8])->Result<Self> {
        let mut r=Reader {data,cursor:0};ensure!(r.next()?==0xfeff0901,"Unsupported FXO header");
        let offset=r.next()? as usize;r.cursor=8usize.checked_add(offset).context("FXO structure offset overflow")?;
        let parameters=r.next()? as usize;let techniques=r.next()? as usize;let _=r.next()?;let object_slots=r.next()?;
        r.bytes(r.cursor,parameters.checked_mul(16).context("FXO parameter count overflow")?)?;
        let mut params=Vec::with_capacity(parameters);
        for _ in 0..parameters {
            let ty=r.next()?;let value=r.next()?;let flags=r.next()?;let count=r.next()? as usize;
            let annotations=r.annotations(count)?;
            params.push(Parameter {value:r.value(ty,value,&mut BTreeSet::new())?,flags,annotations});
        }
        r.bytes(r.cursor,techniques.checked_mul(12).context("FXO technique count overflow")?)?;
        let mut techniques_out=Vec::with_capacity(techniques);
        for _ in 0..techniques {
            let name_offset=r.next()?;let name=r.string(name_offset)?;let annos=r.next()? as usize;let passes=r.next()? as usize;
            let annotations=r.annotations(annos)?;
            r.bytes(r.cursor,passes.checked_mul(12).context("FXO pass count overflow")?)?;
            let mut out=Vec::with_capacity(passes);
            for _ in 0..passes {
                let name_offset=r.next()?;let name=r.string(name_offset)?;let annos=r.next()? as usize;let count=r.next()? as usize;
                let annotations=r.annotations(annos)?;r.bytes(r.cursor,count.checked_mul(16).context("FXO state count overflow")?)?;
                let mut states=Vec::with_capacity(count);
                for _ in 0..count {states.push(r.state(&mut BTreeSet::new(),false)?);}
                out.push(Pass {name,annotations,states});
            }
            techniques_out.push(Technique {name,annotations,passes:out});
        }
        let small=r.next()? as usize;let large=r.next()? as usize;let mut objects=Vec::new();
        for _ in 0..small {
            let id=r.next()?;let len=r.next()? as usize;ensure!(id<object_slots,"FXO object identity outside slots");
            objects.push(Object {id,payload:r.object_payload(len)?,source_kind:None});
        }
        for _ in 0..large {
            let technique=r.next()?;let index=r.next()? as usize;let _=r.next()?;let state=r.next()? as usize;let kind=r.next()?;let len=r.next()? as usize;
            let value=if technique==u32::MAX {
                let param=params.get(index).context("Invalid FXO object parameter")?;
                let Data::Sampler(states)=&param.value.data else{anyhow::bail!("FXO object parameter is not a sampler")};
                &states.get(state).context("Invalid FXO object sampler state")?.value
            }else{&techniques_out.get(technique as usize).context("Invalid FXO object technique")?.passes.get(index).context("Invalid FXO object pass")?.states.get(state).context("Invalid FXO object state")?.value};
            let Data::Objects(ids)=&value.data else{anyhow::bail!("FXO large object reference is not an object")};
            let id=*ids.first().context("Empty FXO object reference")?;ensure!(id<object_slots,"FXO object identity outside slots");
            objects.push(Object {id,payload:r.object_payload(len)?,source_kind:Some(kind)});
        }
        Ok(Self {parameters:params,techniques:techniques_out,objects,object_slots,trailing_bytes:data.len()-r.cursor})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(id:u32)->State {State {kind:146,index:0,value:Value {kind:16,class:4,name:None,semantic:None,elements:0,columns:0,rows:0,data:Data::Objects(vec![id])}}}
    fn effect(states:Vec<State>,objects:Vec<Object>)->Effect {Effect {parameters:vec![],techniques:vec![Technique {name:None,annotations:vec![],passes:vec![Pass {name:None,annotations:vec![],states}]}],objects,object_slots:3,trailing_bytes:0}}
    #[test]
    fn pass_assignment_order_and_selector_identity_are_preserved()->Result<()> {
        let e=effect(vec![state(1),state(2)],vec![Object {id:1,payload:0..4,source_kind:None},Object {id:2,payload:4..8,source_kind:Some(2)}]);
        assert!(matches!(e.shader_pair(&[0,3,254,255,0,0,0,0],0,0)?.vertex,Some(Shader::Selector {payload}) if payload== (4..8)));Ok(())
    }
    #[test]
    fn malformed_references_and_wrong_stage_are_errors() {
        assert!(effect(vec![state(1)],vec![]).shader_pair(&[],0,0).is_err());
        let e=effect(vec![state(1)],vec![Object {id:1,payload:0..4,source_kind:None}]);
        assert!(e.shader_pair(&0xffff0300u32.to_le_bytes(),0,0).is_err());
        assert!(e.shader_pair(&[],0,0).is_err());
        assert!(e.shader_pair(&[],1,0).is_err());
    }
    #[test]
    fn truncated_and_overflowing_layouts_are_rejected() {
        for data in [vec![],0xfeff0901u32.to_le_bytes().to_vec(),[0xfeff0901u32.to_le_bytes(),u32::MAX.to_le_bytes()].concat()] {assert!(Effect::parse(&data).is_err());}
    }
}
