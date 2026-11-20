//!496140 current source-point cell refresh, using canonical typed parent state.
use super::{ai_graph_link_passes::Memory, ai_pathfinder_constructor::State};
use anyhow::{Context as _, Result};
fn word(m: &Memory, p: u32, o: u32) -> Result<u32> {
    m.word(p.wrapping_add(o))
}
fn scalar(m: &Memory, p: u32, o: u32) -> Result<f32> {
    Ok(f32::from_bits(word(m, p, o)?))
}
pub fn refresh(s: &mut State, m: &mut Memory) -> Result<()> {
    if s.flag_250
        .context("Pathfinder source-point flag is unwritten")?
        == 0
    {
        return Ok(());
    }
    let Some(manager) = s.references_3c_4c_50_54_58_60_6c[2] else {
        return Ok(());
    };
    if word(m, u32::try_from(manager)?, 0x4c)? == 0 {
        return Ok(());
    }
    let cache = &mut s.attribute_254.words_04_1c;
    let cell = if cache[4] == 0 {
        0
    } else {
        let slot = word(m, cache[4], 8)?.wrapping_add(cache[5].wrapping_mul(8));
        if m.byte(slot)? == 0 || word(m, word(m, slot, 4)?, 8)? != cache[0] {
            cache[6] = 0;
        }
        cache[6]
    };
    let position = s.points_200_274_2c4_314_378[0].position;
    if cell != 0
        && super::ai_cell_search::contains_position(m, cell, position, scalar(m, cell, 0x14)?)?
            & 255
            != 0
        && position[1] <= scalar(m, cell, 0x18)?
        && position[1] >= scalar(m, cell, 0x1c)?
    {
        s.flag_250 = Some(0);
        return Ok(());
    }
    let manager =
        s.references_3c_4c_50_54_58_60_6c[2].context("Pathfinder source manager disappeared")?;
    let provider = word(m, u32::try_from(manager)?, 0x4c)?;
    super::ai_spatial_walk::find_position(m, provider, position, &mut s.attribute_254.words_04_1c)?;
    s.flag_250 = Some(0);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    const P: usize = 0x68000000;
    const MANAGER: u32 = 0x69000000;
    const PROVIDER: u32 = 0x60000000;
    const SLOTS: u32 = 0x61000000;
    const POINT: u32 = 0x66000000;
    const CACHE: u32 = 0x67000000;
    fn bits(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    #[test]
    fn original_source_cell_refresh_uses_owned_parent_without_a_raw_mirror() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_source_point.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let input = &case["input"];
            let mut s = super::super::ai_pathfinder_modifiers::tests::state()?;
            s.identity = P;
            s.flag_250 = Some(bits(&input["flag"]) as u8);
            s.references_3c_4c_50_54_58_60_6c[2] = input["manager"]
                .as_bool()
                .unwrap()
                .then_some(MANAGER as usize);
            s.points_200_274_2c4_314_378[0].position =
                std::array::from_fn(|i| f32::from_bits(bits(&input["geometry"]["point"][i])));
            s.attribute_254.words_04_1c = std::array::from_fn(|i| bits(&input["cache"][i]));
            let mut m = super::super::ai_cell_search::tests::memory(&input["geometry"])?;
            m.regions.remove(&POINT);
            m.regions.remove(&CACHE);
            m.regions.insert(MANAGER, vec![0; 0x50]);
            m.write(
                MANAGER + 0x4c,
                if input["provider"] == true {
                    PROVIDER
                } else {
                    0
                },
            )?;
            m.write_byte(SLOTS, bits(&input["active"]) as u8)?;
            assert!(m.word(P as u32 + 0x204).is_err());
            assert!(m.word(P as u32 + 0x258).is_err());
            refresh(&mut s, &mut m)?;
            assert_eq!(
                json!(s.flag_250.unwrap()),
                case["flag"],
                "source flag {index}"
            );
            assert_eq!(
                json!(s.attribute_254.words_04_1c),
                case["cache"],
                "source cache {index}"
            );
            assert_eq!(
                json!(s.points_200_274_2c4_314_378[0].position.map(f32::to_bits)),
                input["geometry"]["point"],
                "source point changed {index}"
            );
        }
        Ok(())
    }
}
