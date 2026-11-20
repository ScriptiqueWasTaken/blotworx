//! Bounded CTAB reflection using the DirectX SDK binary constant-table layout.
//! Register locations are read from the program, never inferred from names.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeSet;
#[derive(Debug)]
pub(super) struct Type {
    pub class:u16, pub kind:u16, pub rows:u16, pub columns:u16, pub elements:u16,
    pub members:Vec<(String,Type)>,
}
#[derive(Debug)]
pub(super) struct Constant {
    pub name:String, pub register_set:u16, pub register:u16, pub registers:u16,
    pub ty:Type, pub defaults:Vec<u32>,
}
#[derive(Debug)]
pub(super) struct Table { pub creator:String, pub target:String, pub flags:u32, pub constants:Vec<Constant> }
fn range(data:&[u8],offset:usize,size:usize)->Result<&[u8]> {
    data.get(offset..offset.checked_add(size).context("CTAB range overflow")?).context("CTAB offset outside comment")
}
fn u32_at(data:&[u8],offset:usize)->Result<u32> {Ok(u32::from_le_bytes(range(data,offset,4)?.try_into()?))}
fn u16_at(data:&[u8],offset:usize)->Result<u16> {Ok(u16::from_le_bytes(range(data,offset,2)?.try_into()?))}
fn string(data:&[u8],offset:usize)->Result<String> {
    let bytes=data.get(offset..).context("CTAB string offset outside comment")?;
    let end=bytes.iter().position(|&b|b==0).context("Unterminated CTAB string")?;
    Ok(std::str::from_utf8(&bytes[..end])?.into())
}
fn ty(data:&[u8],offset:usize,visiting:&mut BTreeSet<usize>)->Result<Type> {
    ensure!(visiting.insert(offset),"Recursive CTAB type");range(data,offset,16)?;
    let count=u16_at(data,offset+10)? as usize;
    let base=u32_at(data,offset+12)? as usize;range(data,base,count*8)?;
    let mut members=Vec::with_capacity(count);
    for index in 0..count {
        let at=base+index*8;
        members.push((string(data,u32_at(data,at)? as usize)?,ty(data,u32_at(data,at+4)? as usize,visiting)?));
    }
    visiting.remove(&offset);
    Ok(Type {class:u16_at(data,offset)?,kind:u16_at(data,offset+2)?,rows:u16_at(data,offset+4)?,
        columns:u16_at(data,offset+6)?,elements:u16_at(data,offset+8)?,members})
}
impl Table {
    pub fn parse(data:&[u8],version:u32)->Result<Self> {
        ensure!(u32_at(data,0)?==28,"Unsupported CTAB header size");
        ensure!(u32_at(data,8)?==version,"CTAB shader version mismatch");
        let count=u32_at(data,12)? as usize;let base=u32_at(data,16)? as usize;
        range(data,base,count.checked_mul(20).context("CTAB constant count overflow")?)?;
        let mut constants=Vec::with_capacity(count);
        for index in 0..count {
            let at=base+index*20;let register_set=u16_at(data,at+4)?;let registers=u16_at(data,at+8)?;
            ensure!(register_set<=3,"Unsupported CTAB register set");
            let offset=u32_at(data,at+16)? as usize;
            let mut defaults=Vec::new();
            if offset!=0 {
                let size=registers as usize*if register_set==0 {4}else{16};
                let bytes=range(data,offset,size)?;
                defaults=bytes.chunks_exact(4).map(|word|u32::from_le_bytes(word.try_into().unwrap())).collect();
            }
            constants.push(Constant {name:string(data,u32_at(data,at)? as usize)?,register_set,
                register:u16_at(data,at+6)?,registers,ty:ty(data,u32_at(data,at+12)? as usize,&mut BTreeSet::new())?,defaults});
        }
        Ok(Self {creator:string(data,u32_at(data,4)? as usize)?,target:string(data,u32_at(data,24)? as usize)?,
            flags:u32_at(data,20)?,constants})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn minimal()->Vec<u8> {
        let mut data=vec![0;128];
        for (offset,value) in [(0,28u32),(4,100),(8,0xffff0300),(12,1),(16,28),(24,104),(28,108),(40,48)] {
            data[offset..offset+4].copy_from_slice(&value.to_le_bytes());
        }
        for (offset,value) in [(32,2u16),(34,3),(36,1),(48,1),(50,3),(52,1),(54,4),(56,1)] {
            data[offset..offset+2].copy_from_slice(&value.to_le_bytes());
        }
        data[100..102].copy_from_slice(b"c\0");data[104..107].copy_from_slice(b"ps\0");data[108..110].copy_from_slice(b"x\0");data
    }
    #[test]
    fn invalid_offsets_counts_and_cyclic_types_fail_without_allocation_spikes() {
        let mut data=minimal();assert!(Table::parse(&data,0xffff0300).is_ok());
        data[12..16].copy_from_slice(&u32::MAX.to_le_bytes());assert!(Table::parse(&data,0xffff0300).is_err());
        let mut data=minimal();data[58..60].copy_from_slice(&1u16.to_le_bytes());data[60..64].copy_from_slice(&64u32.to_le_bytes());
        data[64..68].copy_from_slice(&108u32.to_le_bytes());data[68..72].copy_from_slice(&48u32.to_le_bytes());
        assert!(Table::parse(&data,0xffff0300).is_err());
        let mut data=minimal();data[108..].fill(b'x');assert!(Table::parse(&data,0xffff0300).is_err());
    }
}
