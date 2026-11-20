//! Global factory ownership pool, independent of World14/28 admission.
//! Actual457820..45783c and457c28..457d00 use global4e1a8. No derived
//! callback occurs in either operation; pool state must be owned and mapped.
use anyhow::Result;
use super::ai_graph_link_passes::Memory;

pub fn available(memory:&Memory,root_slot:u32)->Result<bool>{
    let pool=memory.word(root_slot)?;
    // Native checks equality, not >=, and does not check the free-node field.
    Ok(memory.word(pool.wrapping_add(0x14))? != memory.word(pool.wrapping_add(0x18))?)
}
pub fn append(memory:&mut Memory,root_slot:u32,entity:usize)->Result<Option<usize>>{
    let pool=memory.word(root_slot)?;
    let node=memory.word(pool.wrapping_add(0x10))?;
    // Retain the native no-free-node result, even if count differs from limit.
    super::ai_spatial_construction::append(memory,pool,u32::try_from(entity)?)?;
    Ok((node!=0).then_some(node as usize))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_factory_pool_matches_original_capacity_and_membership_tail()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_navigation_factory_pool.json"))?;
        for row in fixture["cases"].as_array().unwrap(){
            let mut memory:Memory=serde_json::from_value(row["memory"].clone())?;
            for (i,step) in row["steps"].as_array().unwrap().iter().enumerate(){
                if i==1 && row["input"]["swap"]==true{memory.write(0x4e1a8,0x62001000)?;}
                assert_eq!(available(&memory,0x4e1a8)?,step["available"].as_bool().unwrap(),"{} step{i}",row["input"]);
                let node=append(&mut memory,0x4e1a8,step["actor"].as_u64().unwrap() as usize)?;
                assert_eq!(node.unwrap_or(0),step["node"].as_u64().unwrap() as usize,"{} step{i}",row["input"]);
            }
            assert_eq!(serde_json::to_value(memory)?,row["output"],"{}",row["input"]);
        }
        assert!(available(&Memory::default(),0x4e1a8).is_err(),"Missing ownership pool must diagnose rather than publish readiness");
        Ok(())
    }
}
