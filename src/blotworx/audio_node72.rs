//! Version72 parameter-node records. Preserve authored settings and bit values
//! before connecting their runtime consumers; no legacy-prefix reinterpretation.
use anyhow::Result;
use super::audio_source72::{Reader,Property,Range,bundles};
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
pub struct Fx {pub slot:u8,pub id:u32,pub shared:u8,pub rendered:u8}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
pub struct PathItem {pub first:u32,pub count:u32,pub range_bits:[u32;2]}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
pub struct Automation {pub mode:u32,pub looping:u8,pub transition:u32,pub follow_orientation:u8,pub vertices:Vec<[u32;4]>,pub items:Vec<PathItem>}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
pub struct Positioning {
    pub flags:u8,pub has_3d:Option<u8>,pub panner:Option<u8>,pub kind:Option<u32>,
    pub attenuation:Option<u32>,pub spatialized:Option<u8>,pub dynamic:Option<u8>,pub automation:Option<Automation>,
}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
pub struct StateGroup {pub id:u32,pub sync:u8,pub states:Vec<[u32;2]>}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
pub struct Rtpc {pub control:u32,pub parameter:u32,pub curve:u32,pub scaling:u8,pub points:Vec<[u32;3]>}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
pub struct Advanced {
    pub virtual_queue:u8,pub kill_newest:u8,pub use_virtual:u8,pub maximum:u16,
    pub global_limit:u8,pub below_threshold:u8,pub override_maximum:u8,pub override_virtual:u8,
}
#[derive(Clone,Debug,PartialEq,Eq,serde::Serialize)]
pub struct Node {
    pub override_fx:u8,pub fx_bypass:Option<u8>,pub fx:Vec<Fx>,pub bus:u32,pub parent:u32,pub priority:[u8;2],
    pub properties:Vec<Property>,pub ranges:Vec<Range>,pub positioning:Positioning,
    pub aux_flags:[u8;4],pub aux_buses:Option<[u32;4]>,pub advanced:Advanced,
    pub states:Vec<StateGroup>,pub rtpc:Vec<Rtpc>,pub feedback_bus:Option<u32>,
}
fn short(r:&mut Reader<'_>)->Result<u16>{Ok(u16::from_le_bytes(r.take(2)?.try_into()?))}
impl Node {
    pub fn parse(bytes:&[u8],feedback:bool)->Result<(Self,&[u8])> {
        let mut r=Reader {bytes,at:0};
        let override_fx=r.byte()?;let count=r.byte()?;
        let fx_bypass=if count>0 {Some(r.byte()?)}else{None};
        let mut fx=Vec::new();for _ in 0..count {fx.push(Fx {slot:r.byte()?,id:r.word()?,shared:r.byte()?,rendered:r.byte()?});}
        let bus=r.word()?;let parent=r.word()?;let priority=[r.byte()?,r.byte()?];
        let (properties,ranges)=bundles(&mut r)?;
        let flags=r.byte()?;
        let mut positioning=Positioning {flags,has_3d:None,panner:None,kind:None,attenuation:None,spatialized:None,dynamic:None,automation:None};
        if flags&1!=0 {
            let has_3d=r.byte()?;positioning.has_3d=Some(has_3d);
            if has_3d==0 {positioning.panner=Some(r.byte()?);} else {
                let kind=r.word()?;positioning.kind=Some(kind);positioning.attenuation=Some(r.word()?);positioning.spatialized=Some(r.byte()?);
                if kind==3 {positioning.dynamic=Some(r.byte()?);}
                if kind==2 {
                    let mode=r.word()?;let looping=r.byte()?;let transition=r.word()?;let follow_orientation=r.byte()?;
                    let count=r.word()?;let mut vertices=Vec::new();for _ in 0..count {vertices.push([r.word()?,r.word()?,r.word()?,r.word()?]);}
                    let count=r.word()?;let mut items=Vec::new();for _ in 0..count {items.push(PathItem {first:r.word()?,count:r.word()?,range_bits:[0;2]});}
                    for item in &mut items {item.range_bits=[r.word()?,r.word()?];}
                    positioning.automation=Some(Automation {mode,looping,transition,follow_orientation,vertices,items});
                }
            }
        }
        let aux_flags=[r.byte()?,r.byte()?,r.byte()?,r.byte()?];
        let aux_buses=if aux_flags[3]!=0 {Some([r.word()?,r.word()?,r.word()?,r.word()?])}else{None};
        let advanced=Advanced {virtual_queue:r.byte()?,kill_newest:r.byte()?,use_virtual:r.byte()?,maximum:short(&mut r)?,global_limit:r.byte()?,below_threshold:r.byte()?,override_maximum:r.byte()?,override_virtual:r.byte()?};
        let count=r.word()?;let mut states=Vec::new();
        for _ in 0..count {
            let id=r.word()?;let sync=r.byte()?;let count=short(&mut r)?;
            let mut values=Vec::new();for _ in 0..count {values.push([r.word()?,r.word()?]);}
            states.push(StateGroup {id,sync,states:values});
        }
        let count=short(&mut r)?;let mut rtpc=Vec::new();
        for _ in 0..count {
            let control=r.word()?;let parameter=r.word()?;let curve=r.word()?;let scaling=r.byte()?;let count=short(&mut r)?;
            let mut points=Vec::new();for _ in 0..count {points.push([r.word()?,r.word()?,r.word()?]);}
            rtpc.push(Rtpc {control,parameter,curve,scaling,points});
        }
        let feedback_bus=if feedback {Some(r.word()?)}else{None};
        Ok((Self {override_fx,fx_bypass,fx,bus,parent,priority,properties,ranges,positioning,aux_flags,aux_buses,advanced,states,rtpc,feedback_bus},&bytes[r.at..]))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_clock_keeps_authored_volume_and_inherited_positioning()->Result<()> {
        let mut bytes=vec![0;44];bytes[6..10].copy_from_slice(&0x209a0520u32.to_le_bytes());
        bytes[12]=2;bytes[13]=0;bytes[14]=7;bytes[15..19].copy_from_slice(&(-15f32).to_bits().to_le_bytes());
        let (node,rest)=Node::parse(&bytes,false)?;
        assert!(rest.is_empty());assert_eq!(node.parent,0x209a0520);
        assert_eq!(node.properties[0],Property {id:0,bits:(-15f32).to_bits()});
        assert_eq!(node.positioning.flags,0);assert_eq!(node.positioning.has_3d,None);
        assert!(Node::parse(&bytes[..43],false).is_err());
        Ok(())
    }
}
