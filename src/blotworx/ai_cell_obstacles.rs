//! Retail cell obstacle admission42d3b8 and polygon test42c9ec.
//! The radius-aware polyline intersection42c8fc remains a mandatory service.
use super::ai_graph_link_passes::Memory;
use anyhow::Result;
fn word(m:&Memory,p:u32,o:u32)->Result<u32>{m.word(p.wrapping_add(o))}
fn scalar(m:&Memory,p:u32,o:u32)->Result<f32>{Ok(f32::from_bits(word(m,p,o)?))}
fn select(t:f32,a:f32,b:f32)->f32{if t>=0. {a}else{b}}
pub trait Host {
    fn polyline42c8fc(&mut self,m:&mut Memory,start:u32,end:u32,radius:u32,points:u32,count:u32)->Result<u32>;
}
/// The closing vertex is authored data; the original does not add an edge.
pub fn contains(m:&Memory,point:u32,mut points:u32,count:u32)->Result<u32>{
    if count<2{return Ok(0)}
    let x=scalar(m,point,0)?;let z=scalar(m,point,8)?;
    let mut inside=0;
    for _ in 1..count {
        let next=points.wrapping_add(12);
        let ax=scalar(m,points,0)?;let az=scalar(m,points,8)?;
        let bx=scalar(m,next,0)?;let bz=scalar(m,next,8)?;
        let dz=az-bz;
        let upper=select(select(dz,az,bz)-z,1.01,0.);
        let lower=select(select(dz,bz,az)-z,0.,1.01);
        let right=select(select(ax-bx,ax,bx)-x,1.01,0.);
        if right+(lower+upper)>=3. {
            let crossing=ax+((z-az)*(bx-ax))/(bz-az);
            if ax==bx || x<=crossing {inside=u32::from(inside==0);}
        }
        points=next;
    }
    Ok(inside)
}
fn count_between(start:u32,end:u32)->u32{((end.wrapping_sub(start) as i32)/12) as u32}
pub fn blocked(m:&mut Memory,cell:u32,start:u32,end:u32,radius:u32,flags:u32,host:&mut impl Host)->Result<u32>{
    for (mask,count_offset,table_offset) in [(2,0x38,0x3c),(1,0x40,0x44)] {
        if flags&mask!=0 || word(m,cell,count_offset)?==0 {continue}
        let mut table=cell.wrapping_add(word(m,cell,table_offset)?);
        let mut points=cell.wrapping_add(m.word(table)?);
        let mut i=0;
        while i<word(m,cell,count_offset)? {
            let next=cell.wrapping_add(word(m,table,4)?);
            if host.polyline42c8fc(m,start,end,radius,points,count_between(points,next))?&255==1 {return Ok(1)}
            points=next;table=table.wrapping_add(4);i+=1;
        }
    }
    if flags&4!=0 || word(m,cell,0x48)?==0 {return Ok(0)}
    let sx=scalar(m,start,0)?;let sz=scalar(m,start,8)?;
    let ex=scalar(m,end,0)?;let ez=scalar(m,end,8)?;
    if sx==ex && sz==ez {return Ok(0)}
    let dx=ex-sx;let dz=ez-sz;
    let length=super::cursor::native_sqrt(dx*dx+dz*dz);
    let dx=dx/length;let dz=dz/length;
    let mut table=cell.wrapping_add(word(m,cell,0x4c)?);
    let mut points=cell.wrapping_add(m.word(table)?);
    let mut i=0;
    while i<word(m,cell,0x48)? {
        let next=cell.wrapping_add(word(m,table,16)?);
        let facing=dz*scalar(m,table,12)?+dx*scalar(m,table,4)?;
        // Native GE skips this obstacle; unordered values take the test path.
        if !(facing>=0.) {
            let count=count_between(points,next);
            if contains(m,start,points,count)?&255==1 {return Ok(1)}
            if host.polyline42c8fc(m,start,end,radius,points,count)?&255==1 {return Ok(1)}
        }
        points=next;table=table.wrapping_add(16);i+=1;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    struct Reference<'a>{input:&'a Value,trace:Vec<Value>,calls:usize}
    impl Host for Reference<'_>{
        fn polyline42c8fc(&mut self,_:&mut Memory,start:u32,end:u32,radius:u32,points:u32,count:u32)->Result<u32>{
            self.trace.push(json!([start,end,radius,points,count]));
            let values=self.input["results"].as_array().unwrap();
            let result=values[self.calls.min(values.len()-1)].as_u64().unwrap() as u32;
            self.calls+=1;Ok(result)
        }
    }
    #[test]
    fn retail_cell_obstacle_flags_and_polygon_boundaries_match()->Result<()>{
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_cell_obstacles.json"))?;
        for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
            let mut m=Memory{regions:[(0x60000000,vec![0;0x800]),(0x61000000,vec![0;32])].into(),resources:Default::default(),stride:0,journal:Default::default()};
            for (address,value) in case["memory"].as_object().unwrap(){m.write(address.parse()?,value.as_u64().unwrap() as u32)?;}
            let input=&case["input"];let mut host=Reference{input,trace:vec![],calls:0};
            let result=if input["op"]=="contains" {contains(&m,0x61000000,0x60000400,input["vertices"].as_u64().unwrap() as u32)?}
                else{blocked(&mut m,0x60000000,0x61000000,0x61000010,input["radius"].as_u64().unwrap() as u32,input["flags"].as_u64().unwrap() as u32,&mut host)?};
            assert_eq!(result,case["result"].as_u64().unwrap() as u32,"case {index}: {input}");
            assert_eq!(json!(host.trace),case["trace"],"case {index}: {input}");
        }
        Ok(())
    }
}
