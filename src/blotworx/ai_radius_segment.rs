//! Native radius segment45a3b4, stationary45a0f4 and endpoint459edc.
//! Arithmetic and ordered/unordered branches follow the retail Wii routines.
use super::{ai_edge_crossing::Globals,ai_graph_link_passes::Memory};
use anyhow::Result;
type V=[f32;3];
fn sqrt(x:f32)->f32{super::cursor::native_sqrt(x)}
fn put(output:Option<&mut V>,v:V){if let Some(output)=output{*output=v}}
fn endpoint(p:V,a:V,b:V,r:f32,output:Option<&mut V>)->bool{
    let oz=p[2]-a[2];let ox=p[0]-a[0];let r2=r*r;
    if oz*oz+ox*ox<=r2{put(output,a);return true}
    let dz=b[2]-a[2];let dx=b[0]-a[0];let length=dz*dz+dx*dx;
    let cross=oz*dx-ox*dz;
    if cross*cross>length*r2{return false}
    let t=cross/length;
    let along=sqrt(r2/length-t*t);
    let x=(t*dz+p[0])-along*dx;
    let z=(p[2]-t*dx)-along*dz;
    let projection=(z-a[2])*dz+(x-a[0])*dx;
    if projection<=0. || projection>=length{return false}
    put(output,[x,p[1],z]);true
}
fn stationary(p:V,a:V,b:V,r:f32)->bool{
    let dz=b[2]-a[2];let dx=b[0]-a[0];let length=sqrt(dz*dz+dx*dx);
    if length==0.{return false}
    let oz=p[2]-a[2];let ox=p[0]-a[0];
    let projection=(dz*oz+dx*ox)/length;let r2=r*r;
    if projection<0.{
        let z=a[2]-p[2];let x=a[0]-p[0];
        if z*z+x*x>=r2{return false}return true
    }
    if projection>length{
        let z=b[2]-p[2];let x=b[0]-p[0];
        if z*z+x*x>=r2{return false}return true
    }
    let distance=(dz*ox-dx*oz)/length;
    if distance*distance>=r2{return false}
    true
}
pub fn intersection(m:&mut Memory,g:Globals,a:V,b:V,c:V,d:V,r:f32,mut output:Option<&mut V>)->Result<bool>{
    g.radius_intersection(m)?;
    if c[0]==d[0] && c[2]==d[2]{return Ok(false)}
    if a[0]==b[0] && a[2]==b[2]{
        if let Some(out)=output.as_deref_mut(){*out=a}
        return Ok(stationary(a,c,d,r))
    }
    let rx=b[0]-a[0];let rz=b[2]-a[2];let ex=d[0]-c[0];let ez=d[2]-c[2];
    if rz*ex-rx*ez<0.{return Ok(false)}
    let ox=a[0]-c[0];let oz=a[2]-c[2];let qx=a[0]-d[0];let qz=a[2]-d[2];
    if rx*ox+rz*oz>0. && rx*qx+rz*qz>0.{return Ok(false)}
    if ox*ez-oz*ex<0. && (b[0]-c[0])*ez-(b[2]-c[2])*ex<0.{return Ok(false)}
    let length=ex*ex+ez*ez;let cross=ox*ez-oz*ex;
    let mut point=[0.;3];
    if cross*cross<=length*(r*r){point=a}
    else{
        let denominator=ez*rx-ex*rz;
        if denominator*cross>=0.{return Ok(false)}
        let length=sqrt(length);
        let offset=if cross>0.{r*length}else{(-r)*length};
        let t=(offset-cross)/denominator;
        if t<0. || t>1.{return Ok(false)}
        point[0]=a[0]+t*rx;point[2]=a[2]+t*rz;
    }
    let projection=ex*(point[0]-c[0])+ez*(point[2]-c[2]);
    if projection<0.{return Ok(endpoint(c,a,b,r,output))}
    if projection>length{return Ok(endpoint(d,a,b,r,output))}
    if let Some(output)=output{
        *output=[point[0],c[1]+(projection/length)*(d[1]-c[1]),point[2]];
    }
    Ok(true)
}
/// Native42c8fc/42c3ac polyline adapter, including budget publication.
/// All points are retained canonical data references; no closing edge is added.
pub struct Native {pub globals:Globals}
impl super::ai_cell_obstacles::Host for Native{
    fn polyline42c8fc(&mut self,m:&mut Memory,start:u32,end:u32,radius:u32,points:u32,count:u32)->Result<u32>{
        if count<2{return Ok(0)}
        fn point(m:&Memory,p:u32)->Result<[u32;3]>{Ok([m.word(p)?,m.word(p.wrapping_add(4))?,m.word(p.wrapping_add(8))?])}
        let r=f32::from_bits(radius);let mut previous=points;let mut hit=false;
        for i in 1..count{
            let next=points.wrapping_add(i.wrapping_mul(12));
            let a=point(m,start)?;let b=point(m,end)?;let c=point(m,previous)?;let d=point(m,next)?;
            hit=if r<=0.{super::ai_edge_crossing::intersection(m,self.globals,a,b,c,d,None,None)?}
                else{intersection(m,self.globals,a.map(f32::from_bits),b.map(f32::from_bits),c.map(f32::from_bits),d.map(f32::from_bits),r,None)?};
            if hit{break}previous=next;
        }
        self.globals.finish(m)?;Ok(u32::from(hit))
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    use super::super::ai_cell_obstacles::Host as _;
    #[test]
    fn retail_radius_geometry_and_polyline_budget_match()->Result<()>{
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_radius_segment.json"))?;
        for (index,case) in fixture["cases"].as_array().unwrap().iter().enumerate(){
            let input=&case["input"];
            let mut m=Memory{regions:[(0x60000000,vec![0;0x100]),(0x61000000,vec![0;0x200]),(0x5a108,vec![0;4]),(0x4a508,vec![0;20])].into(),resources:Default::default(),stride:0,journal:Default::default()};
            m.write(0x5a108,0x61000000)?;m.write(0x6100004c,0x61000100)?;
            m.write(0x6100017c,1f32.to_bits())?;m.write(0x4a510,0.125f32.to_bits())?;
            m.write(0x4a514,0.25f32.to_bits())?;m.write(0x4a518,0.5f32.to_bits())?;
            let vec=|name:&str|->V{std::array::from_fn(|i|f32::from_bits(input[name][i].as_u64().unwrap() as u32))};
            let g=Globals{root_slot:0x5a108,intersections:0x4a508,polygon_visits:0};
            let radius=input["radius"].as_u64().unwrap() as u32;
            let result=if input["op"]=="segment"{
                let mut output=vec("initial");let present=input["output"].as_bool().unwrap();
                let hit=intersection(&mut m,g,vec("a"),vec("b"),vec("c"),vec("d"),f32::from_bits(radius),present.then_some(&mut output))?;
                assert_eq!(serde_json::json!(output.map(f32::to_bits)),case["output"],"output case {index}: {input}");
                u32::from(hit)
            }else{
                for (p,key) in [(0x60000000,"a"),(0x60000010,"b")]{for (i,v) in vec(key).into_iter().enumerate(){m.write(p+i as u32*4,v.to_bits())?;}}
                for (j,p) in input["points"].as_array().unwrap().iter().enumerate(){for i in 0..3{m.write(0x60000020+j as u32*12+i as u32*4,p[i].as_u64().unwrap() as u32)?;}}
                Native{globals:g}.polyline42c8fc(&mut m,0x60000000,0x60000010,radius,0x60000020,input["count"].as_u64().unwrap() as u32)?
            };
            assert_eq!(result,case["result"].as_u64().unwrap() as u32,"result case {index}: {input}");
            assert_eq!(serde_json::json!([m.word(0x4a508)?,m.word(0x4a50c)?]),case["counters"],"counters case {index}");
            assert_eq!(m.word(0x4a518)?,case["pending"].as_u64().unwrap() as u32,"pending case {index}");
            assert_eq!(m.word(0x6100017c)?,case["budget"].as_u64().unwrap() as u32,"budget case {index}");
        }
        Ok(())
    }
}
