//! Retail navigation descriptor186f38 and built-in biped definitions187ffc.
//! A matching definition is not a constructed/registered navigation agent.
#[derive(Debug,Clone,serde::Serialize)]
pub struct Definition {
    pub name:&'static str, pub entity_class:&'static str, pub action_class:&'static str,
    pub brain:&'static str, pub brain_class:&'static str, pub service:&'static str,
    pub agents:[&'static str;3],
}
/// Native inputs are radius, height and movement kind. Unrecognized selectors
/// retain the original omitted-token behavior; they are not clamped to enums.
pub fn descriptor(radius:i32,height:i32,movement:i32)->String {
    let mut name=String::from(match height {0=>"Short",2=>"Tall",_=>""});
    name.push_str(match radius {0=>"Thin",2=>"Wide",_=>""});
    if movement==1 {name.push_str("Biped");}
    name
}
macro_rules! biped {
    ($name:literal)=>{Definition {name:$name,entity_class:"JPSBaseAIEntity",action_class:"JPSBaseAIAction",
        brain:$name,brain_class:"JPSBaseAIBrain",service:concat!($name,"PathFinder"),
        agents:["GotoAgent","WanderAgent","FleeAgent"]}};
}
static BIPEDS:[Definition;9]=[
    biped!("ShortThinBiped"),biped!("ThinBiped"),biped!("TallThinBiped"),
    biped!("ShortBiped"),biped!("Biped"),biped!("TallBiped"),
    biped!("ShortWideBiped"),biped!("WideBiped"),biped!("TallWideBiped"),
];
impl Definition {
    pub fn built_in_bipeds()->&'static [Self] {&BIPEDS}
    ///188214 installs all nine combinations before separately adding Mickey
    /// and Obstacle. These immutable definitions are shared between actors;
    /// matching one does not construct an agent or apply optional XML overrides.
    pub fn built_in_biped(name:&str)->Option<&'static Self> {
        BIPEDS.iter().find(|definition|definition.name==name)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_resolves_shape_profile_without_claiming_agent_construction()->anyhow::Result<()> {
        use anyhow::Context as _;
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world=super::super::assets::load(std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level",&egui::Context::default())?;
        let summary=world.character_physics_summary();
        let gus=summary["bodies"].as_array().context("Missing body summary")?.iter()
            .find(|b|b["name"]=="dbc_1st_gus_tut.Gremlin_Gus 01").context("Original Gus body absent")?;
        let profile=&gus["built_in_agent_profile"];
        assert_eq!(profile["name"],"ShortThinBiped");
        assert_eq!(profile["service"],"ShortThinBipedPathFinder");
        assert_eq!(profile["brain_class"],"JPSBaseAIBrain");
        assert_eq!(gus["navigation_agent_constructed"],false);
        assert!(std::ptr::eq(Definition::built_in_biped("ShortThinBiped").unwrap(),Definition::built_in_biped("ShortThinBiped").unwrap()));
        Ok(())
    }
    #[test]
    fn descriptor_and_biped_definition_match_original_instruction_calls()->anyhow::Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_agent_profiles.json"))?;
        for c in fixture["descriptors"].as_array().unwrap() {
            assert_eq!(descriptor(c["radius"].as_i64().unwrap() as i32,c["height"].as_i64().unwrap() as i32,c["movement"].as_i64().unwrap() as i32),c["name"].as_str().unwrap());
        }
        for c in fixture["definitions"].as_array().unwrap() {
            let name=c["name"].as_str().unwrap();let d=Definition::built_in_biped(name).unwrap();
            let expected=serde_json::json!([
                ["node","entities","Entity",name],["value","entity","Class",d.entity_class],
                ["value","entity","ActionClass",d.action_class],["value","entity","Brain",d.brain],
                ["brain","brains",d.brain_class,d.name],["value","brain","Service",d.service],
                ["value","brain","Agent",d.agents[0]],["value","brain","Agent",d.agents[1]],
                ["value","brain","Agent",d.agents[2]],
            ]);
            assert_eq!(expected,c["trace"]);
        }
        assert!(Definition::built_in_biped("Gremlin:Gus").is_none());
        Ok(())
    }
}
