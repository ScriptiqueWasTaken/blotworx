//! Authored mesh extra-data values. Retain float bits and exact names; these
//! are shader attributes, not inherited scene properties or texture-map IDs.
use anyhow::{ensure,Context,Result};
use epic_mickey_lib_rs::nif_file::{NifFile,NiObject};
use std::collections::BTreeMap;
#[derive(Clone,Debug,PartialEq)]
pub(crate) enum Attribute {
    Boolean(u8),Integer(i32),Float(u32),Floats(Vec<u32>),Color([u32;4]),
}
pub(crate) type Table=BTreeMap<String,Attribute>;

fn raw(nif:&NifFile,kind:&str,bytes:&[u8])->Result<(String,Attribute)> {
    ensure!(nif.version>=0x14010001,"Unsupported shader attribute name layout");
    let word=|offset:usize|->Result<u32> {
        let bytes=bytes.get(offset..offset.checked_add(4).context("Attribute offset overflow")?).context("Truncated shader attribute")?.try_into().unwrap();
        Ok(if nif.source_is_little_endian {u32::from_le_bytes(bytes)}else{u32::from_be_bytes(bytes)})
    };
    let name=nif.string_table.get(word(0)? as usize).context("Shader attribute name is absent")?.clone();
    let value=match kind {
        "NiColorExtraData"=>{
            ensure!(bytes.len()==20,"Invalid color shader attribute size");
            Attribute::Color([word(4)?,word(8)?,word(12)?,word(16)?])
        },
        "NiFloatsExtraData"=>{
            let count=word(4)? as usize;
            ensure!(count.checked_mul(4).and_then(|n|n.checked_add(8))==Some(bytes.len()),"Invalid float-array shader attribute size");
            Attribute::Floats((0..count).map(|i|word(8+i*4)).collect::<Result<_>>()?)
        },
        _=>unreachable!(),
    };Ok((name,value))
}
pub(crate) fn extract(nif:&NifFile,links:&[Option<u32>])->Result<Table> {
    let mut table=Table::new();
    for &index in links.iter().flatten() {
        let block=nif.blocks.get(index as usize).context("Shader extra-data link is absent")?;
        let entry=match block {
            NiObject::NiBooleanExtraData(data)=>Some((data.base.name.clone(),Attribute::Boolean(data.value))),
            NiObject::NiIntegerExtraData(data)=>Some((data.base.name.clone(),Attribute::Integer(data.value))),
            NiObject::NiFloatExtraData(data)=>Some((data.base.name.clone(),Attribute::Float(data.value.to_bits()))),
            NiObject::Unknown(data) if matches!(data.type_name.as_str(),"NiColorExtraData"|"NiFloatsExtraData")=>Some(raw(nif,&data.type_name,&data.raw_data)?),
            _=>None,
        };
        if let Some((name,value))=entry {
            ensure!(!table.contains_key(&name),"Duplicate shader attribute {name}");table.insert(name,value);
        }
    }Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;
    use epic_mickey_lib_rs::nif_file::RawBlockData;
    #[test]
    fn color_and_vector_words_survive_both_endian_layouts()->Result<()> {
        for little in [false,true] {
            let words=[0u32,0x80000000,0x7fc01234,0x3f800000,0x3f000000];
            let mut nif=NifFile {version:0x14030009,source_is_little_endian:little,string_table:vec!["authored".into()],..Default::default()};
            let bytes=|words:&[u32]|words.iter().flat_map(|v|if little {v.to_le_bytes()}else{v.to_be_bytes()}).collect();
            nif.blocks.push(NiObject::Unknown(RawBlockData {type_name:"NiColorExtraData".into(),raw_data:bytes(&words)}));
            assert_eq!(extract(&nif,&[Some(0)])?["authored"],Attribute::Color(words[1..].try_into().unwrap()));
            let vectors=[0,3,words[1],words[2],words[3]];
            nif.blocks[0]=NiObject::Unknown(RawBlockData {type_name:"NiFloatsExtraData".into(),raw_data:bytes(&vectors)});
            assert_eq!(extract(&nif,&[Some(0)])?["authored"],Attribute::Floats(vectors[2..].to_vec()));
            assert!(extract(&nif,&[Some(0),Some(0)]).is_err());
            if let NiObject::Unknown(raw)=&mut nif.blocks[0] {raw.raw_data.pop();}
            assert!(extract(&nif,&[Some(0)]).is_err());
            assert!(extract(&nif,&[Some(99)]).is_err());
        }Ok(())
    }
}
