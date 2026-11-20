//! BasicAI navigation-wrapper creation REL300038. Factory results are retained
//! even when null; no-character entry preserves the previous wrapper.
use anyhow::Result;
pub trait Host {
    fn character(&mut self,owner:usize)->Result<Option<usize>>;
    fn height(&mut self,character:usize)->Result<Option<i32>>;
    fn radius(&mut self,character:usize)->Result<Option<i32>>;
    fn name(&mut self,owner:usize)->Result<Vec<u8>>;
    fn create(&mut self,radius:i32,height:i32,movement:i32,name:&[u8],user:[usize;2],enabled:u32)->Result<Option<usize>>;
}
pub fn initialize(owner:usize,wrapper:&mut Option<usize>,host:&mut impl Host)->Result<()> {
    let Some(character)=host.character(owner)? else {return Ok(())};
    let height=host.height(character)?.unwrap_or(0);
    let radius=host.radius(character)?.unwrap_or(0);
    let name=host.name(owner)?;
    *wrapper=host.create(radius,height,1,&name,[owner,0],1)?;
    Ok(())
}
/// Non-obstacle AIBrain handoff2dfa54..2dfa90. Missing controller, wrapper or
/// brain leaves the previous link flag untouched; only the actual owned brain
/// receives the gameplay component reference. This does not perform admission.
pub trait LinkHost {
    fn controller(&mut self, owner: usize) -> Result<Option<usize>>;
    fn initialize_controller(&mut self, controller: usize) -> Result<()>;
    fn wrapper(&mut self, controller: usize) -> Result<Option<usize>>;
    fn navigation_brain(&mut self, wrapper: usize) -> Result<Option<usize>>;
    fn bind_gameplay_brain(&mut self, brain: usize, gameplay: usize) -> Result<()>;
}
pub fn link_gameplay_brain(owner: usize, gameplay: usize, linked: &mut bool,
    host: &mut impl LinkHost) -> Result<()> {
    let Some(controller) = host.controller(owner)? else { return Ok(()) };
    host.initialize_controller(controller)?;
    let Some(wrapper) = host.wrapper(controller)? else { return Ok(()) };
    let Some(brain) = host.navigation_brain(wrapper)? else { return Ok(()) };
    host.bind_gameplay_brain(brain, gameplay)?;
    *linked = true;
    Ok(())
}
/// AIBrain fields4b/b0/e0. Reinitialization preserves all three unless the
/// original branch explicitly writes them; disabled navigation uses an
/// obstacle wrapper rather than manufacturing an ordinary walking agent.
#[derive(Default, Debug)]
pub struct GameplayNavigation {
    pub obstacle_mode: bool,
    pub linked: bool,
    pub obstacle: Option<usize>,
}
pub trait GameplayHost: LinkHost {
    fn character(&mut self, owner: usize) -> Result<Option<usize>>;
    /// Authored DisableKynapse property (descriptor26ca0 slot13).
    fn disable_kynapse(&mut self) -> Result<bool>;
    /// Actual navigation manager4dc availability; profile existence alone
    /// must never be substituted for this runtime state.
    fn navigation_available(&mut self) -> Result<bool>;
    ///1870d0 with profile Obstacle, owner name, user[owner,0], enabled0.
    fn create_obstacle(&mut self, owner: usize) -> Result<Option<usize>>;
}
/// Complete navigation-mode branch2df9e0..2dfb14 of AIBrain initialization.
/// Parameter setup before it and behavior/sensor setup after it remain owned
/// by the outer initializer.
pub fn initialize_gameplay_navigation(owner: usize, gameplay: usize,
    state: &mut GameplayNavigation, host: &mut impl GameplayHost) -> Result<()> {
    let character = host.character(owner)?;
    if character.is_none() || host.disable_kynapse()? {
        state.obstacle_mode = true;
    }
    if !state.obstacle_mode {
        link_gameplay_brain(owner, gameplay, &mut state.linked, host)?;
    } else if character.is_some() && host.navigation_available()? {
        state.obstacle = host.create_obstacle(owner)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    #[test]
    fn gameplay_navigation_mode_matches_original_obstacle_and_link_branches()->Result<()> {
        struct H<'a>{c:&'a Value,trace:Vec<Value>,gameplay:usize}
        impl LinkHost for H<'_>{
            fn controller(&mut self,o:usize)->Result<Option<usize>>{self.trace.push(json!(["controller",o]));Ok(Some(0x74000000))}
            fn initialize_controller(&mut self,c:usize)->Result<()>{self.trace.push(json!(["initialize",c]));Ok(())}
            fn wrapper(&mut self,_:usize)->Result<Option<usize>>{Ok(Some(0x75000000))}
            fn navigation_brain(&mut self,_:usize)->Result<Option<usize>>{Ok(Some(0x76000000))}
            fn bind_gameplay_brain(&mut self,_:usize,g:usize)->Result<()>{self.gameplay=g;Ok(())}
        }
        impl GameplayHost for H<'_>{
            fn character(&mut self,o:usize)->Result<Option<usize>>{self.trace.push(json!(["character",o]));Ok(self.c["character"].as_bool().unwrap().then_some(1))}
            fn disable_kynapse(&mut self)->Result<bool>{self.trace.push(json!(["disable"]));Ok(self.c["disabled"].as_bool().unwrap())}
            fn navigation_available(&mut self)->Result<bool>{Ok(self.c["available"].as_bool().unwrap())}
            fn create_obstacle(&mut self,o:usize)->Result<Option<usize>>{self.trace.push(json!(["obstacle",o]));Ok(self.c["result"].as_bool().unwrap().then_some(0x75000000))}
        }
        let capture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_gameplay_navigation_mode.json"))?;
        for row in capture["cases"].as_array().unwrap(){
            let c=&row["input"];let mut state=GameplayNavigation{obstacle_mode:c["mode"].as_bool().unwrap(),linked:c["linked"].as_bool().unwrap(),obstacle:c["previous"].as_bool().unwrap().then_some(0x7b000000)};
            let mut h=H{c,trace:vec![],gameplay:0x7b000000};
            initialize_gameplay_navigation(0x73000008,0x72000000,&mut state,&mut h)?;
            assert_eq!(json!(h.trace),row["trace"],"{c}");
            assert_eq!(json!(u8::from(state.obstacle_mode)),row["mode"],"{c}");
            assert_eq!(json!(u8::from(state.linked)),row["linked"],"{c}");
            assert_eq!(json!(state.obstacle.unwrap_or(0)),row["obstacle"],"{c}");
            assert_eq!(json!(h.gameplay),row["brain_gameplay"],"{c}");
        }
        Ok(())
    }
    #[test]
    fn original_gameplay_brain_navigation_link_preserves_failure_state_and_order() -> Result<()> {
        struct Link<'a> { c: &'a Value, wrapper: Option<usize>, gameplay: usize, trace: Vec<Value> }
        impl LinkHost for Link<'_> {
            fn controller(&mut self, owner: usize) -> Result<Option<usize>> {
                self.trace.push(json!(["controller", owner])); Ok(self.c["controller"].as_bool().unwrap().then_some(0x74000000))
            }
            fn initialize_controller(&mut self, controller: usize) -> Result<()> {
                self.trace.push(json!(["initialize", controller]));
                match self.c["mode"].as_str().unwrap() {
                    "replace" => self.wrapper = Some(0x75000000), "clear" => self.wrapper = None, _ => {}
                } Ok(())
            }
            fn wrapper(&mut self, _: usize) -> Result<Option<usize>> { Ok(self.wrapper) }
            fn navigation_brain(&mut self, _: usize) -> Result<Option<usize>> { Ok(self.c["brain"].as_bool().unwrap().then_some(0x76000000)) }
            fn bind_gameplay_brain(&mut self, brain: usize, gameplay: usize) -> Result<()> {
                assert_eq!(brain, 0x76000000); self.gameplay = gameplay; Ok(())
            }
        }
        let fixture: Value = serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_gameplay_navigation_link.json"))?;
        for row in fixture["cases"].as_array().unwrap() {
            let c = &row["input"]; let mut linked = c["linked"].as_bool().unwrap();
            let mut h = Link { c, wrapper: c["previous"].as_bool().unwrap().then_some(0x75000000), gameplay: 0x77000000, trace: vec![] };
            link_gameplay_brain(0x73000000, 0x72000000, &mut linked, &mut h)?;
            assert_eq!(json!(h.trace), row["trace"], "{c}");
            assert_eq!(json!(h.wrapper.unwrap_or(0)), row["wrapper"], "{c}");
            assert_eq!(json!(h.gameplay), row["brain_gameplay"], "{c}");
            assert_eq!(json!(u8::from(linked)), row["linked"], "{c}");
        }
        Ok(())
    }
    struct Binding<'a>{input:&'a Value,trace:Vec<Value>}
    impl Host for Binding<'_> {
        fn character(&mut self,owner:usize)->Result<Option<usize>> {self.trace.push(json!(["character",owner]));Ok(self.input["character"].as_bool().unwrap().then_some(17))}
        fn height(&mut self,character:usize)->Result<Option<i32>> {self.trace.push(json!(["height",character]));Ok(self.input["height"].as_i64().map(|v|v as i32))}
        fn radius(&mut self,character:usize)->Result<Option<i32>> {self.trace.push(json!(["radius",character]));Ok(self.input["radius"].as_i64().map(|v|v as i32))}
        fn name(&mut self,owner:usize)->Result<Vec<u8>> {self.trace.push(json!(["name",owner]));Ok(b"Gus".to_vec())}
        fn create(&mut self,radius:i32,height:i32,movement:i32,name:&[u8],user:[usize;2],enabled:u32)->Result<Option<usize>> {
            self.trace.push(json!(["create",radius,height,movement,std::str::from_utf8(name).unwrap(),user,enabled]));Ok(self.input["result"].as_u64().map(|v|v as usize))
        }
    }
    #[test]
    fn controller_navigation_creation_matches_original_query_and_store_order()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_controller_navigation.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];let mut wrapper=input["previous"].as_u64().map(|v|v as usize);
            let mut host=Binding{input,trace:vec![]};initialize(11,&mut wrapper,&mut host)?;
            assert_eq!(json!({"wrapper":wrapper,"trace":host.trace}),case["output"],"{input}");
        }
        Ok(())
    }
}
