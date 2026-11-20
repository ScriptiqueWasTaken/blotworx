//! Retail entity liveness298a30 and current-player identity269d54.
//! Component lookup/lifecycle and Lua argument coercion belong to the World.

pub fn alive(
    exists: bool,
    require_component: bool,
    health_alive: Option<bool>,
    counter_present: bool,
    counter_status: impl FnOnce() -> u32,
) -> bool {
    if !exists { return false; }
    if health_alive.is_none() && !counter_present { return !require_component; }
    if health_alive == Some(true) { return true; }
    counter_present && counter_status() != 2
}

pub fn player(entity: Option<usize>, current: Option<usize>) -> bool {
    entity.is_some() && entity == current
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_liveness_and_player_instructions_match() -> anyhow::Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/entity_liveness.json"))?;
        let mut count=0;
        for kind in ["alive","lua_alive"] {
            for row in fixture[kind].as_array().unwrap() {
                let input=&row["input"];
                let health=input["health"].as_array().map(|health|health[0].as_i64().unwrap()>0 || health[1].as_u64().unwrap()!=0);
                let counter=input["counter"].as_i64().map(|v|v as u32);
                let mut read=false;
                let result=alive(input["exists"].as_bool().unwrap(),input["require_component"].as_bool().unwrap(),health,counter.is_some(),|| {
                    read=true;counter.unwrap()
                });
                assert_eq!(result,row["result"].as_bool().unwrap(),"{kind}: {input}");
                assert_eq!(read,row["trace"].as_array().unwrap().iter().any(|v|v=="counter_status"),"Counter short circuit: {input}");
                count+=1;
            }
        }
        for row in fixture["player"].as_array().unwrap() {
            let input=&row["input"];
            let valid=|value:&serde_json::Value|value.as_u64().filter(|v|(1..=2).contains(v)).map(|v|v as usize);
            assert_eq!(player(valid(&input["candidate"]),valid(&input["player"])),row["result"].as_bool().unwrap(),"{input}");
            count+=1;
        }
        assert!(player(Some(0),Some(0)),"Host index zero is a valid opaque entity");
        assert_eq!(count,688);
        println!("Matched {count} original liveness/player cases including counter short-circuit behavior");
        Ok(())
    }
}
