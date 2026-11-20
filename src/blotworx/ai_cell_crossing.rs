//! Retail navigation cell crossing42cae0. Operates on loaded canonical
//! records and caller-owned outputs/history, preserving native query budgets.
use super::{ai_edge_crossing::{self,Globals},ai_graph_link_passes::Memory};
use anyhow::Result;
fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn scalar(m:&Memory,p:u32,o:u32)->Result<f32>{Ok(f32::from_bits(word(m,p,o)?))}
fn point(m:&Memory,p:u32)->Result<[u32;3]>{Ok([word(m,p,0)?,word(m,p,4)?,word(m,p,8)?])}
fn put(m:&mut Memory,p:u32,v:[u32;3])->Result<()>{for (i,v) in v.into_iter().enumerate(){m.write(p.wrapping_add(i as u32*4),v)?}Ok(())}
#[derive(Clone,Copy)]
pub struct Vectors {pub up:u32,pub zero:u32}
pub(super) fn normal(m:&mut Memory,v:Vectors,edge:u32,output:u32)->Result<()>{
    let a=point(m,edge)?.map(f32::from_bits);let b=point(m,edge.wrapping_add(12))?.map(f32::from_bits);
    let d=std::array::from_fn::<_,3,_>(|i|a[i]-b[i]);let up=point(m,v.up)?.map(f32::from_bits);
    let z=d[0]*up[1]-d[1]*up[0];let y=d[2]*up[0]-d[0]*up[2];let x=d[1]*up[2]-d[2]*up[1];
    let length=super::cursor::native_sqrt(z*z+(x*x+y*y));
    let value=if length==0.{point(m,v.zero)?}else{[x/length,y/length,z/length].map(f32::to_bits)};
    put(m,output,value)
}
fn intersection(m:&mut Memory,g:Globals,start:u32,end:u32,edge:u32)->Result<Option<([u32;3],f32)>>{
    let s=point(m,start)?;let e=point(m,end)?;let a=point(m,edge)?;let b=point(m,edge.wrapping_add(12))?;
    let mut hit=[0;3];
    if !ai_edge_crossing::intersection(m,g,s,e,a,b,Some(&mut hit),None)?{return Ok(None)}
    let dz=f32::from_bits(hit[2])-scalar(m,start,8)?;let dx=f32::from_bits(hit[0])-scalar(m,start,0)?;
    Ok(Some((hit,dz*dz+dx*dx)))
}
/// History is exactly the original sixteen-word caller scratch area. The
/// native limit and repeat-cell rejection are retained, not a route-length cap.
pub fn crossing(m:&mut Memory,g:Globals,v:Vectors,cell:u32,start:u32,end:u32,next_cell:u32,output:u32,output_normal:u32,history:u32,history_count:u32)->Result<u32>{
    m.write(next_cell,cell)?;put(m,output,point(m,end)?)?;
    let mut nearest=f32::MAX;
    for (count_offset,table_offset) in [(0x20,0x24),(0x28,0x2c)]{
        let mut edge=cell.wrapping_add(word(m,cell,table_offset)?);let mut index=0;
        while index<word(m,cell,count_offset)?{
            if let Some((hit,distance))=intersection(m,g,start,end,edge)?{
                if distance<nearest{
                    nearest=distance;m.write(next_cell,word(m,edge,0x18)?)?;put(m,output,hit)?;normal(m,v,edge,output_normal)?;
                }
            }
            edge=edge.wrapping_add(32);index+=1;
        }
    }
    if word(m,cell,0x30)?!=0{
        let dx=scalar(m,end,0)?-scalar(m,start,0)?;let dz=scalar(m,end,8)?-scalar(m,start,8)?;
        let length=super::cursor::native_sqrt(dz*dz+(dx*dx+0.));
        if length==0.{put(m,output,point(m,end)?)?;m.write(next_cell,cell)?;g.finish(m)?;return Ok(1)}
        let direction=[dx/length,dz/length];
        let scale=g.scale(m)?.unwrap_or(1.);let margin=f32::from_bits(0x3c23d70a);
        let bias=(margin*(margin*scale))*scale;
        let mut candidate=f32::MAX;let mut penalty=0.;let mut target=cell;let mut hit_point=[0;3];
        let mut edge=cell.wrapping_add(word(m,cell,0x34)?);let mut index=0;
        while index<word(m,cell,0x30)?{
            if scalar(m,edge,0x24)? != -2.{
                if let Some((hit,distance))=intersection(m,g,start,end,edge)?{
                    let dot=scalar(m,edge,0x28)?*direction[1]+scalar(m,edge,0x20)?*direction[0];
                    let blocked=dot<=scalar(m,edge,0x24)? || word(m,edge,0x18)?==0;
                    let cost=if blocked{bias}else{0.};
                    if distance+cost<candidate+penalty{
                        candidate=distance;penalty=cost;target=if blocked{0}else{word(m,edge,0x18)?};hit_point=hit;
                        normal(m,v,edge,output_normal)?;
                    }
                }
            }
            edge=edge.wrapping_add(44);index+=1;
        }
        if candidate!=f32::MAX && candidate<nearest{
            m.write(next_cell,target)?;put(m,output,hit_point)?;
            if target==0{m.write(history_count,0)?;g.finish(m)?;return Ok(1)}
            nearest=candidate;
        }
    }
    g.finish(m)?;
    if nearest!=0.{m.write(history_count,0)?;put(m,output_normal,point(m,v.zero)?)?;return Ok(1)}
    let count=m.word(history_count)?;
    let mut repeated=count>=16;
    if !repeated{for i in 0..count{if word(m,history,i.wrapping_mul(4))?==cell{repeated=true;break}}}
    if repeated{
        m.write(history_count,0)?;put(m,output,point(m,start)?)?;put(m,output_normal,point(m,v.zero)?)?;m.write(next_cell,0)?;return Ok(0)
    }
    m.write(history.wrapping_add(count.wrapping_mul(4)),cell)?;
    m.write(history_count,m.word(history_count)?.wrapping_add(1))?;
    put(m,output_normal,point(m,v.zero)?)?;Ok(1)
}

#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn retail_cell_crossing_outputs_history_and_budgets_match()->Result<()>{
  let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_cell_crossing.json"))?;
  for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
   let mut m=Memory{regions:[(0x62000000,vec![0;0x800]),(0x63000000,vec![0;0x100]),(0x61000000,vec![0;0x200]),(0x5a108,vec![0;4]),(0x4a508,vec![0;20]),(0x564d0,vec![0;0x40])].into(),resources:Default::default(),stride:0,journal:Default::default()};
   for (p,v) in case["memory"].as_object().unwrap(){m.write(p.parse()?,v.as_u64().unwrap() as u32)?}
   let output=if case["input"]["alias"].as_bool().unwrap(){0x63000010}else{0x63000030};
   let result=crossing(&mut m,Globals{root_slot:0x5a108,intersections:0x4a508,polygon_visits:0},Vectors{up:0x564f4,zero:0x564d0},0x62000000,0x63000000,0x63000010,0x63000020,output,0x63000040,0x63000050,0x63000090)?;
   let actual=serde_json::json!({"result":result,"next":m.word(0x63000020)?,"point":point(&m,output)?,"end":point(&m,0x63000010)?,"normal":point(&m,0x63000040)?,"history":(0..16).map(|j|m.word(0x63000050+j*4)).collect::<Result<Vec<_>>>()?,"count":m.word(0x63000090)?,"counter":m.word(0x4a508)?,"pending":m.word(0x4a518)?,"budget":m.word(0x6100017c)?});
   for (key,value) in actual.as_object().unwrap(){assert_eq!(value,&case[key],"case {index}, {key}: {}",case["input"]);}
  }
  Ok(())
 }
}
