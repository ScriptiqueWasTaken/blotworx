//! Concrete gameplay property/name side of BasicAI300038. The owning
//! navigation factory must return its actual initialized wrapper or null.
use anyhow::{Context,Result,ensure};
use super::{assets::World,ai_controller_navigation::{self,Host}};
pub trait Factory {
    fn create(&mut self,radius:i32,height:i32,movement:i32,name:&[u8],user:[usize;2],enabled:u32)->Result<Option<usize>>;
}
pub trait GameplayFactory: Factory {
    fn create_obstacle(&mut self, name: &[u8], user: [usize;2]) -> Result<Option<usize>>;
    fn navigation_brain(&self, wrapper: usize) -> Result<Option<usize>>;
    fn bind_gameplay_brain(&mut self, brain: usize, gameplay: usize) -> Result<()>;
}
struct GameplayBinding<'a, F> { world: &'a World, owner: usize, wrapper: &'a mut Option<usize>, factory: &'a mut F, available: bool }
impl<F: GameplayFactory> ai_controller_navigation::GameplayHost for GameplayBinding<'_, F> {
    fn character(&mut self, owner:usize)->Result<Option<usize>> {
        Binding{world:self.world,factory:self.factory,owner:self.owner}.character(owner)
    }
    fn disable_kynapse(&mut self)->Result<bool> {
        let actor=&self.world.entities[self.owner];
        let component=actor.components.iter().find(|c|c.class_name=="AIBrainComponent").context("Gameplay AIBrain absent")?;
        let mut properties=component.properties.iter().filter(|p|p.name=="DisableKynapse");
        let value=properties.next();ensure!(properties.next().is_none(),"Ambiguous DisableKynapse property");
        value.map(|p|p.value.as_bool().context("Invalid DisableKynapse boolean")).transpose().map(|v|v.unwrap_or(false))
    }
    fn navigation_available(&mut self)->Result<bool>{Ok(self.available)}
    fn create_obstacle(&mut self,owner:usize)->Result<Option<usize>> {
        self.factory.create_obstacle(self.world.entities[owner].name.as_bytes(),[owner,0])
    }
}
impl<F: GameplayFactory> ai_controller_navigation::LinkHost for GameplayBinding<'_, F> {
    fn controller(&mut self, owner: usize) -> Result<Option<usize>> {
        ensure!(owner == self.owner, "Gameplay navigation owner changed");
        let actor = self.world.entities.get(owner).context("Gameplay navigation owner absent")?;
        let mut components = actor.components.iter().enumerate().filter(|(_, c)| c.class_name == "BasicAIControllerComponent");
        let component = components.next().map(|(i, _)| i);
        ensure!(components.next().is_none(), "Gameplay navigation requires controller slot arbitration");
        Ok(component)
    }
    fn initialize_controller(&mut self, _: usize) -> Result<()> {
        self.world.initialize_owned_navigation_wrapper(self.owner, self.wrapper, self.factory)
    }
    fn wrapper(&mut self, _: usize) -> Result<Option<usize>> { Ok(*self.wrapper) }
    fn navigation_brain(&mut self, wrapper: usize) -> Result<Option<usize>> { self.factory.navigation_brain(wrapper) }
    fn bind_gameplay_brain(&mut self, brain: usize, gameplay: usize) -> Result<()> { self.factory.bind_gameplay_brain(brain, gameplay) }
}
struct Binding<'a,F> {world:&'a World,factory:&'a mut F,owner:usize}
impl<F:Factory> Host for Binding<'_,F> {
    fn character(&mut self,owner:usize)->Result<Option<usize>> {
        let actor=self.world.entities.get(owner).context("Navigation controller owner absent")?;
        let mut components=actor.components.iter().enumerate().filter(|(_,c)|c.class_name=="PhysicsRigidBodyCharacterComponent");
        let index=components.next().map(|(i,_)|i);
        ensure!(components.next().is_none(),"Navigation creation requires character slot arbitration");
        Ok(index)
    }
    fn height(&mut self,character:usize)->Result<Option<i32>> {self.selector(character,"Entity Height")}
    fn radius(&mut self,character:usize)->Result<Option<i32>> {self.selector(character,"Entity Radius")}
    fn name(&mut self,owner:usize)->Result<Vec<u8>> {Ok(self.world.entities[owner].name.as_bytes().to_vec())}
    fn create(&mut self,radius:i32,height:i32,movement:i32,name:&[u8],user:[usize;2],enabled:u32)->Result<Option<usize>> {
        self.factory.create(radius,height,movement,name,user,enabled)
    }
}
impl<F:Factory> Binding<'_,F> {
    fn selector(&self,character:usize,name:&str)->Result<Option<i32>> {
        let component=self.world.entities[self.owner].components.get(character).context("Navigation character receiver absent")?;
        let mut properties=component.properties.iter().filter(|p|p.name==name);
        let value=properties.next();ensure!(properties.next().is_none(),"Ambiguous navigation {name}");
        value.map(|p|Ok(i32::try_from(p.value.as_i64().with_context(||format!("Invalid navigation {name}"))?)?)).transpose()
    }
}
impl World {
    /// The caller supplies the actual manager4dc flag, never profile presence.
    /// Covers only the initializer's navigation section, not behavior admission.
    pub(super) fn initialize_owned_gameplay_navigation(&self, owner:usize, gameplay:usize,
        wrapper:&mut Option<usize>, state:&mut ai_controller_navigation::GameplayNavigation,
        navigation_available:bool, factory:&mut impl GameplayFactory)->Result<()> {
        let actor=self.entities.get(owner).context("Gameplay navigation owner absent")?;
        ensure!(actor.components.iter().filter(|c|c.class_name=="AIBrainComponent").count()==1,
            "Gameplay navigation requires its actual AIBrain component");
        ai_controller_navigation::initialize_gameplay_navigation(owner,gameplay,state,
            &mut GameplayBinding{world:self,owner,wrapper,factory,available:navigation_available})
    }
    /// Non-obstacle branch only. The outer gameplay AIBrain initializer must
    /// still perform its authored obstacle-mode/property/lifecycle decisions.
    pub(super) fn link_owned_gameplay_navigation(&self, owner: usize, gameplay: usize,
        wrapper: &mut Option<usize>, linked: &mut bool, factory: &mut impl GameplayFactory) -> Result<()> {
        let actor = self.entities.get(owner).context("Gameplay navigation owner absent")?;
        ensure!(actor.components.iter().filter(|c| c.class_name == "AIBrainComponent").count() == 1,
            "Gameplay navigation requires its actual AIBrain component");
        ai_controller_navigation::link_gameplay_brain(owner, gameplay, linked,
            &mut GameplayBinding { world: self, owner, wrapper, factory, available:false })
    }
    pub(super) fn initialize_owned_navigation_wrapper(&self,owner:usize,wrapper:&mut Option<usize>,factory:&mut impl Factory)->Result<()> {
        let actor=self.entities.get(owner).context("Navigation controller owner absent")?;
        ensure!(actor.components.iter().filter(|c|c.class_name=="BasicAIControllerComponent").count()==1,"Navigation creation requires one actual BasicAI controller");
        ai_controller_navigation::initialize(owner,wrapper,&mut Binding{world:self,factory,owner})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_controller_creation_supplies_actual_profile_and_retains_factory_failure()->Result<()> {
        struct Capture {calls:Vec<(i32,i32,i32,Vec<u8>,[usize;2],u32)>,result:Option<usize>,links:Vec<(usize,usize)>}
        impl Factory for Capture {
            fn create(&mut self,r:i32,h:i32,m:i32,n:&[u8],u:[usize;2],e:u32)->Result<Option<usize>> {
                self.calls.push((r,h,m,n.to_vec(),u,e));Ok(self.result)
            }
        }
        impl GameplayFactory for Capture {
            fn create_obstacle(&mut self,n:&[u8],u:[usize;2])->Result<Option<usize>> {
                self.calls.push((0,0,0,n.to_vec(),u,0));Ok(self.result)
            }
            fn navigation_brain(&self, wrapper:usize)->Result<Option<usize>> {ensure!(wrapper==17,"Wrong imported controller wrapper");Ok(Some(23))}
            fn bind_gameplay_brain(&mut self, brain:usize, gameplay:usize)->Result<()> {self.links.push((brain,gameplay));Ok(())}
        }
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.Level",&egui::Context::default())?;
        let owner=world.scripts.resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01").context("Original Gus absent")?;
        let mut capture=Capture{calls:vec![],result:Some(17),links:vec![]};let mut wrapper=Some(99);
        world.initialize_owned_navigation_wrapper(owner,&mut wrapper,&mut capture)?;
        assert_eq!(wrapper,Some(17));
        assert_eq!(capture.calls,vec![(0,0,1,world.entities[owner].name.as_bytes().to_vec(),[owner,0],1)]);
        // Supplied factory result tests the original gameplay-to-navigation
        //bridge, not successful full initialization of a normal live brain.
        let mut linked=false;
        world.link_owned_gameplay_navigation(owner,71,&mut wrapper,&mut linked,&mut capture)?;
        assert!(linked);assert_eq!(capture.links,[(23,71)]);
        let mut mode=ai_controller_navigation::GameplayNavigation::default();
        world.initialize_owned_gameplay_navigation(owner,74,&mut wrapper,&mut mode,true,&mut capture)?;
        assert!(mode.linked);assert!(!mode.obstacle_mode);assert_eq!(capture.links,[(23,71),(23,74)]);
        let component=world.entities[owner].components.iter_mut().find(|c|c.class_name=="AIBrainComponent").unwrap();
        if let Some(property)=component.properties.iter_mut().find(|p|p.name=="DisableKynapse") {
            property.value=serde_json::json!(true);
        } else {
            // Reuse the game's actual property schema, including its type.
            let mut property=crate::component_schema::observed_property(&crate::component_schema::CATALOG.components["AIBrainComponent"].fields["DisableKynapse"].variants[0]);
            property.value=serde_json::json!(true);component.properties.push(property);
        }
        let before=capture.calls.len();
        world.initialize_owned_gameplay_navigation(owner,75,&mut wrapper,&mut mode,false,&mut capture)?;
        assert!(mode.obstacle_mode);assert_eq!(capture.calls.len(),before);assert_eq!(mode.obstacle,None);
        world.initialize_owned_gameplay_navigation(owner,75,&mut wrapper,&mut mode,true,&mut capture)?;
        assert_eq!(mode.obstacle,Some(17));assert_eq!(wrapper,Some(17));
        assert_eq!(capture.calls.last().unwrap(),&(0,0,0,world.entities[owner].name.as_bytes().to_vec(),[owner,0],0));
        // Factory failure must replace an old wrapper with null.
        capture.result=None;world.initialize_owned_navigation_wrapper(owner,&mut wrapper,&mut capture)?;
        assert_eq!(wrapper,None);
        world.link_owned_gameplay_navigation(owner,72,&mut wrapper,&mut linked,&mut capture)?;
        assert!(linked,"Source failure does not clear an existing link flag");assert_eq!(capture.links,[(23,71),(23,74)]);
        // Native missing-character branch retains an existing wrapper and
        // does not touch the factory. This is a controlled lifecycle branch.
        world.entities[owner].components.retain(|c|c.class_name!="PhysicsRigidBodyCharacterComponent");
        let before=capture.calls.len();wrapper=Some(99);
        world.initialize_owned_navigation_wrapper(owner,&mut wrapper,&mut capture)?;
        assert_eq!(wrapper,Some(99));assert_eq!(capture.calls.len(),before);
        world.entities[owner].components.retain(|c|c.class_name!="BasicAIControllerComponent");
        linked=false;
        world.link_owned_gameplay_navigation(owner,73,&mut wrapper,&mut linked,&mut capture)?;
        assert!(!linked);assert_eq!(wrapper,Some(99));assert_eq!(capture.calls.len(),before);
        Ok(())
    }
}
