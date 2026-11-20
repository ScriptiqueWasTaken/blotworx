//! Retail polygon44cca4, polygon/region44cec8 and region4446dc containment.
//! Child-volume441600 remains an explicit required service for kind1 regions.
use anyhow::{Result,ensure};
use std::collections::BTreeSet;
use super::ai_graph_link_passes::Memory;
fn word(m:&Memory,base:u32,offset:u32)->Result<u32>{m.word(base.wrapping_add(offset))}
fn scalar(m:&Memory,base:u32,offset:u32)->Result<f32>{Ok(f32::from_bits(word(m,base,offset)?))}
pub(super) fn vertex(m:&Memory,base:u32)->Result<[f32;3]>{Ok([scalar(m,base,8)?,scalar(m,base,12)?,scalar(m,base,16)?])}
pub(super) fn select(test:f32,positive:f32,negative:f32)->f32{if test>=0.{positive}else{negative}}

/// Linked vertices at polygon8 use XYZ8/c/10 and next30. The source expects
/// an explicitly terminated outline, including its authored closing vertex;
/// do not invent a closing edge or deduplicate repeated vertices.
pub fn polygon(m:&Memory,outline:u32,point:[u32;3],context_scale:Option<f32>)->Result<bool>{
    let tolerance=0.3f32*context_scale.unwrap_or(1.); //44ccc0..44ccd0
    let p=point.map(f32::from_bits);
    let mut previous=word(m,outline,8)?;
    let mut next=word(m,previous,0x30)?;
    let mut inside=false;let mut visited=BTreeSet::new();
    while next!=0 {
        ensure!(visited.insert((previous,next)),"Cyclic native navigation outline");
        let a=vertex(m,previous)?;let b=vertex(m,next)?;
        let dz=a[2]-b[2];
        let min_z=select(dz,b[2],a[2]);let max_z=select(dz,a[2],b[2]);
        // These two source branches require ordered >= and <= respectively.
        if p[2]>=min_z && p[2]<=max_z {
            let dx=a[0]-b[0];let max_x=select(dx,a[0],b[0]);
            if p[0]<=max_x {
                if a[2]!=b[2] {
                    if a[0]==b[0] {
                        if p[0]==a[0] {
                            let y=a[1]+((p[2]-a[2])*(b[1]-a[1]))/(b[2]-a[2]);
                            if (y-p[1]).abs()<tolerance {return Ok(true)}
                        }
                        if p[2]>min_z {inside=!inside;}
                    } else {
                        let z=p[2]-a[2];let denominator=b[2]-a[2];
                        let crossing=a[0]+(z*(b[0]-a[0]))/denominator;
                        if p[0]<=crossing {
                            if p[0]==crossing {
                                let y=a[1]+(z*(b[1]-a[1]))/denominator;
                                if (y-p[1]).abs()<tolerance {return Ok(true)}
                            }
                            if p[2]>min_z {inside=!inside;}
                        }
                    }
                } else {
                    let min_x=select(dx,b[0],a[0]);
                    if p[0]>=min_x {
                        let y=a[1]+((p[0]-a[0])*(b[1]-a[1]))/(b[0]-a[0]);
                        if (y-p[1]).abs()<tolerance {return Ok(true)}
                    }
                }
            }
        }
        previous=word(m,previous,0x30)?;next=word(m,next,0x30)?;
    }
    Ok(inside)
}

///441600's last argument is1 in both region routines below. No child-volume
/// test may silently return true when its concrete geometry is unavailable.
pub trait Children {fn contains(&mut self,memory:&Memory,child:u32,point:[u32;3],flag:u32)->Result<u8>;}
fn bounds(m:&Memory,region:u32,p:[f32;3])->Result<bool>{
    Ok(!(p[0]<scalar(m,region,0x14)? || p[0]>scalar(m,region,0x20)? || p[2]<scalar(m,region,0x1c)? || p[2]>scalar(m,region,0x28)?))
}
fn within_height(m:&Memory,region:u32,y:f32,margin:f32)->Result<bool>{
    Ok(!(y>scalar(m,region,0x24)?+margin || y<scalar(m,region,0x18)?-margin))
}
fn children(m:&Memory,region:u32,point:[u32;3],host:&mut impl Children)->Result<bool>{
    let list=word(m,region,12)?;let mut node=word(m,list,8)?;let mut visited=BTreeSet::new();
    while node!=0 {
        ensure!(visited.insert(node),"Cyclic navigation child-volume list");
        if host.contains(m,word(m,node,4)?,point,1)?!=0 {return Ok(true)}
        node=word(m,node,12)?;
    }Ok(false)
}

///44cec8. The null-region path derives bounds from the actual vertex chain.
pub fn polygon_in_region(m:&Memory,config:u32,region:Option<u32>,outline:u32,point:[u32;3],context_scale:Option<f32>,host:&mut impl Children)->Result<bool>{
    let p=point.map(f32::from_bits);let mut child_test=false;
    if let Some(region)=region {
        if !bounds(m,region,p)? {return Ok(false)}
        if m.byte(config.wrapping_add(0x58))?!=0 {
            match word(m,region,0x10)? {
                kind @ (0|1)=>{
                    if !within_height(m,region,p[1],0.5*scalar(m,config,0x60)?)? {return Ok(false)}
                    child_test=kind==1;
                }
                _=>return Ok(false),
            }
        }
    } else {
        let mut min=[f32::MAX;3];let mut max=[-f32::MAX;3];
        let mut node=word(m,outline,8)?;let mut visited=BTreeSet::new();
        while node!=0 {
            ensure!(visited.insert(node),"Cyclic native navigation bound vertices");
            let v=vertex(m,node)?;
            for axis in [0,1,2] {
                if axis==1 && m.byte(config.wrapping_add(0x58))?==0 {continue}
                if min[axis]>v[axis] {min[axis]=v[axis];}
                if max[axis]<v[axis] {max[axis]=v[axis];}
            }
            node=word(m,node,0x30)?;
        }
        if p[0]<min[0] || p[0]>max[0] || p[2]<min[2] || p[2]>max[2] {return Ok(false)}
        if m.byte(config.wrapping_add(0x58))?!=0 {
            let margin=0.5*scalar(m,config,0x60)?;
            if p[1]>max[1]+margin || p[1]<min[1]-margin {return Ok(false)}
        }
    }
    if !polygon(m,outline,point,context_scale)? {return Ok(false)}
    if child_test {children(m,region.unwrap(),point,host)}else{Ok(true)}
}

///4446dc. Outline parity is distinct from the kind1 child-volume admission;
/// nested polygon checks and the final child check retain native call order.
pub fn region(m:&Memory,region:u32,point:[u32;3],context_scale:Option<f32>,host:&mut impl Children)->Result<bool>{
    let p=point.map(f32::from_bits);if !bounds(m,region,p)? {return Ok(false)}
    let config=word(m,region,4)?;let mut child_test=false;
    if m.byte(config.wrapping_add(0x58))?!=0 {
        match word(m,region,0x10)? {
            kind @ (0|1)=>{
                if !within_height(m,region,p[1],scalar(m,config,0x60)?)? {return Ok(false)}
                child_test=kind==1;
            }
            _=>return Ok(false),
        }
    }
    let list=word(m,region,8)?;let mut node=word(m,list,8)?;let mut inside=false;let mut visited=BTreeSet::new();
    while node!=0 {
        ensure!(visited.insert(node),"Cyclic navigation outline list");
        if polygon_in_region(m,word(m,region,4)?,Some(region),word(m,node,4)?,point,context_scale,host)? {inside=!inside;}
        node=word(m,node,12)?;
    }
    if !inside {return Ok(false)}
    if child_test {children(m,region,point,host)}else{Ok(true)}
}

#[cfg(test)] mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use serde_json::{Value,json};
    const CFG:u32=0x60000000;const REGION:u32=0x61000000;const POLY:u32=0x62000000;const VERT:u32=0x63000000;
    const NODES:u32=0x64000000;const LIST:u32=0x65000000;const CHILD_NODES:u32=0x66000000;const CHILD:u32=0x67000000;const CHILD_LIST:u32=0x68000000;
    fn bits(v:&Value)->u32{v.as_u64().unwrap() as u32}
    fn vector(v:&Value)->[u32;3]{std::array::from_fn(|i|bits(&v[i]))}
    pub(super) fn memory(c:&Value)->Result<Memory>{
        let polygons=c["polygons"].as_array().unwrap();let children=c["children"].as_array().unwrap();
        let vertex_bytes=polygons.iter().enumerate().map(|(i,p)|i*0x10000+p.as_array().unwrap().len()*0x40).max().unwrap_or(0);
        let mut m=Memory{regions:BTreeMap::from([
            (CFG,vec![0;0xd8]),(REGION,vec![0;0xc0]),(POLY,vec![0;polygons.len().saturating_sub(1)*0x1000+0x20]),
            (VERT,vec![0;vertex_bytes]),(NODES,vec![0;polygons.len()*0x20]),(LIST,vec![0;0x20]),
            (CHILD_NODES,vec![0;children.len()*0x20]),(CHILD_LIST,vec![0;0x20])]),resources:BTreeMap::new(),stride:36,journal:BTreeMap::new()};
        m.write(CFG+0x58,bits(&c["height"])<<24)?;m.write(CFG+0x60,bits(&c["margin"]))?;
        m.write(CFG+0x5c,c["radius"].as_u64().map_or(0.1f32.to_bits(),|v|v as u32))?;m.write(CFG+0xb0,c["budget_index"].as_i64().unwrap_or(-1) as u32)?;
        m.write(REGION+4,CFG)?;m.write(REGION+8,LIST)?;m.write(REGION+12,CHILD_LIST)?;m.write(REGION+16,bits(&c["kind"]))?;
        for (i,v) in c["bounds"].as_array().unwrap().iter().enumerate(){m.write(REGION+20+i as u32*4,bits(v))?;}
        m.write(LIST+8,if polygons.is_empty(){0}else{NODES})?;
        for (i,p) in polygons.iter().enumerate(){
            let p=p.as_array().unwrap();let at=POLY+i as u32*0x1000;let base=VERT+i as u32*0x10000;
            m.write(at+8,if p.is_empty(){0}else{base})?;m.write(at+12,if p.is_empty(){0}else{base+(p.len()-1) as u32*0x40})?;m.write(at+16,p.len() as u32)?;m.write(NODES+i as u32*0x20+4,at)?;
            m.write(NODES+i as u32*0x20+12,if i+1<polygons.len(){NODES+(i+1) as u32*0x20}else{0})?;
            for (j,v) in p.iter().enumerate(){
                let at=base+j as u32*0x40;
                for axis in 0..3{m.write(at+8+axis as u32*4,bits(&v[axis]))?;}
                m.write(at+0x2c,if j>0{base+(j-1) as u32*0x40}else{0})?;m.write(at+0x30,if j+1<p.len(){base+(j+1) as u32*0x40}else{0})?;
            }
        }
        m.write(CHILD_LIST+8,if children.is_empty(){0}else{CHILD_NODES})?;
        for i in 0..children.len(){m.write(CHILD_NODES+i as u32*0x20+4,CHILD+i as u32*0x100)?;m.write(CHILD_NODES+i as u32*0x20+12,if i+1<children.len(){CHILD_NODES+(i+1) as u32*0x20}else{0})?;}
        Ok(m)
    }
    struct Fixture<'a>{case:&'a Value,trace:Vec<Value>}
    impl Children for Fixture<'_>{fn contains(&mut self,_:&Memory,child:u32,point:[u32;3],flag:u32)->Result<u8>{
        self.trace.push(json!([child,point,flag]));Ok(bits(&self.case["children"][((child-CHILD)/0x100) as usize]) as u8)
    }}
    #[test] fn original_polygon_and_region_geometry_matches_instructions()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_region_geometry.json"))?;
        for expected in fixture["cases"].as_array().unwrap(){
            let c=&expected["input"];let m=memory(c)?;let p=vector(&c["point"]);let scale=c["scale"].as_u64().map(|v|f32::from_bits(v as u32));
            let mut host=Fixture{case:c,trace:Vec::new()};
            let actual=match c["op"].as_str().unwrap(){
                "polygon"=>polygon(&m,POLY,p,scale)?,
                "polygon_in_region"=>polygon_in_region(&m,CFG,c["region_enabled"].as_bool().unwrap().then_some(REGION),POLY,p,scale,&mut host)?,
                "region"=>region(&m,REGION,p,scale,&mut host)?,
                _=>unreachable!(),
            };
            assert_eq!(json!(actual),expected["result"],"{c}");assert_eq!(host.trace,*expected["trace"].as_array().unwrap(),"{c}");
        }Ok(())
    }
    #[test] fn malformed_geometry_reports_cycles_truncation_and_missing_children()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_region_geometry.json"))?;
        let c=&fixture["cases"][0]["input"];let mut m=memory(c)?;let p=vector(&c["point"]);
        let mut host=Fixture{case:c,trace:Vec::new()};
        m.write(VERT+4*0x40+0x30,VERT)?;assert!(polygon(&m,POLY,[2f32.to_bits(),5f32.to_bits(),2f32.to_bits()],None).is_err());
        m.write(VERT+4*0x40+0x30,0)?;
        m.regions.get_mut(&CFG).unwrap().truncate(0x58);assert!(region(&m,REGION,p,None,&mut host).is_err());
        let mut m=memory(c)?;m.write(NODES+12,NODES)?;assert!(region(&m,REGION,p,None,&mut host).is_err());
        struct Missing;impl Children for Missing{fn contains(&mut self,_:&Memory,_:u32,_:[u32;3],_:u32)->Result<u8>{anyhow::bail!("Child-volume geometry is unavailable")}}
        let mut c=c.clone();c["height"]=json!(1);c["kind"]=json!(1);c["children"]=json!([1]);let m=memory(&c)?;
        assert!(region(&m,REGION,p,None,&mut Missing).is_err());
        Ok(())
    }
}
#[cfg(test)]
pub(super) fn fixture_memory(case:&serde_json::Value)->Result<Memory>{tests::memory(case)}
