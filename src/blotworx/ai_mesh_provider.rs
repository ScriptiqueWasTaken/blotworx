//!431b5c/431b6c/431c6c spatial-provider updates over shared runtime mesh bytes.
use super::{ai_graph_link_passes::Memory, ai_runtime_mesh};
use anyhow::Result;
fn w(m: &Memory, p: u32, o: u32) -> Result<u32> {
    m.word(p.wrapping_add(o))
}
pub fn begin(m: &mut Memory, p: u32) -> Result<()> {
    m.write(p.wrapping_add(0x14), 0)?;
    m.write(p.wrapping_add(0x1c), 0)
}
pub fn set(m: &mut Memory, p: u32, index: u32, sector: u32) -> Result<()> {
    if index >= w(m, p, 12)? {
        return Ok(());
    }
    let slot = w(m, p, 8)?.wrapping_add(index.wrapping_mul(8));
    let active = m.byte(slot)?;
    if sector == 0 {
        if active == 0 {
            return Ok(());
        }
        let old = w(m, slot, 4)?;
        let id = w(m, old, 8)?;
        let removed = w(m, p, 0x10)?;
        let count = w(m, p, 0x14)?;
        m.write(removed.wrapping_add(count.wrapping_mul(4)), id)?;
        m.write(p.wrapping_add(0x14), count.wrapping_add(1))?;
        m.write_byte(slot, 0)?;
        m.write(slot.wrapping_add(4), 0)?;
    } else {
        if active == 1 {
            let old = w(m, slot, 4)?;
            if w(m, old, 8)? == w(m, sector, 8)? && w(m, old, 0x30)? == w(m, sector, 0x30)? {
                return Ok(());
            }
        }
        if active == 1 {
            let old = w(m, slot, 4)?;
            let id = w(m, old, 8)?;
            let removed = w(m, p, 0x10)?;
            let count = w(m, p, 0x14)?;
            m.write(removed.wrapping_add(count.wrapping_mul(4)), id)?;
            m.write(p.wrapping_add(0x14), count.wrapping_add(1))?;
        }
        let added = w(m, p, 0x18)?;
        let count = w(m, p, 0x1c)?;
        m.write(added.wrapping_add(count.wrapping_mul(4)), index)?;
        m.write(p.wrapping_add(0x1c), count.wrapping_add(1))?;
        m.write_byte(slot, 1)?;
        m.write(slot.wrapping_add(4), sector)?;
    }
    let version = w(m, p, 4)?;
    m.write(p.wrapping_add(4), version.wrapping_add(1))
}
pub fn finish(m: &mut Memory, p: u32) -> Result<()> {
    let mut i = 0u32;
    while i < w(m, p, 0x14)? {
        let removed = w(m, p, 0x10)?;
        let id = m.word(removed.wrapping_add(i.wrapping_mul(4)))?;
        let mut j = 0u32;
        while j < w(m, p, 12)? {
            let slot = w(m, p, 8)?.wrapping_add(j.wrapping_mul(8));
            if m.byte(slot)? == 1 {
                let sector = w(m, slot, 4)?;
                ai_runtime_mesh::clear_neighbors(m, sector, Some(id))?;
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    let mut i = 0u32;
    while i < w(m, p, 0x1c)? {
        let added = w(m, p, 0x18)?;
        let index = m.word(added.wrapping_add(i.wrapping_mul(4)))?;
        let slot = w(m, p, 8)?.wrapping_add(index.wrapping_mul(8));
        let sector = w(m, slot, 4)?;
        let mut j = 0u32;
        while j < w(m, p, 12)? {
            let slot = w(m, p, 8)?.wrapping_add(j.wrapping_mul(8));
            if m.byte(slot)? != 0 {
                let other = w(m, slot, 4)?;
                ai_runtime_mesh::bind_neighbor(m, sector, other)?;
                ai_runtime_mesh::bind_neighbor(m, other, sector)?;
            }
            j = j.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn number(v: &Value) -> u32 { v.as_u64().unwrap() as u32 }
    #[test]
    fn original_provider_updates_and_mesh_neighbors_match() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_mesh_provider.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c = &case["input"];
            let mut m = Memory::default();
            let mut sectors = Vec::new();
            let bases = [0x65000000, 0x65010000, 0x65020000];
            for (i, mesh) in fixture["meshes"].as_array().unwrap().iter().enumerate() {
                let hex = mesh.as_str().unwrap();
                let bytes = (0..hex.len()).step_by(2).map(|j| u8::from_str_radix(&hex[j..j+2], 16)).collect::<std::result::Result<Vec<_>, _>>()?;
                m.regions.insert(bases[i], bytes);
                sectors.push(ai_runtime_mesh::load(&mut m, bases[i])?);
            }
            let p = 0x60000000;
            for (base, size) in [(p,32),(0x61000000,24),(0x62000000,64),(0x63000000,64)] { m.regions.insert(base,vec![0;size]); }
            for (off,v) in [(4,number(&c["version"])),(8,0x61000000),(12,3),(16,0x62000000),(20,2),(24,0x63000000),(28,2)] { m.write(p+off,v)?; }
            for i in 0..3 {
                m.write(0x61000000+i*8,(number(&c["active"][i as usize])<<24)|0xaabbcc)?;
                m.write(0x61000004+i*8,if i<2 {sectors[i as usize]} else {0})?;
            }
            for i in 0..16 { m.write(0x62000000+i*4,0xdead0100+i)?; m.write(0x63000000+i*4,0xdead0200+i)?; }
            for op in c["ops"].as_array().unwrap() {
                match op[0].as_str().unwrap() {
                    "begin" => begin(&mut m,p)?, "finish" => finish(&mut m,p)?,
                    "set" => { let n=op[2].as_i64().unwrap(); set(&mut m,p,number(&op[1]),if n<0 {0} else {sectors[n as usize]})?; }
                    _ => unreachable!(),
                }
            }
            for (key,base,n) in [("provider",p,8),("slots",0x61000000,6),("removed",0x62000000,16),("added",0x63000000,16)] {
                let actual=(0..n).map(|i|m.word(base+i*4)).collect::<Result<Vec<_>>>()?;
                assert_eq!(serde_json::to_value(actual)?,case[key],"{key}: {c}");
            }
            for (i,base) in bases.iter().enumerate() {
                let hash:String=ring::digest::digest(&ring::digest::SHA256,&m.regions[base]).as_ref().iter().map(|b|format!("{b:02x}")).collect();
                assert_eq!(hash,case["mesh_hashes"][i].as_str().unwrap(),"mesh {i}: {c}");
            }
        }
        Ok(())
    }
}
