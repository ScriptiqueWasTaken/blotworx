//! Concrete LPF definition strings and native integer conversion. Context
//! manager/name resolution and floating conversion require their real runtime.
use anyhow::{Context,Result};
use super::{ai_navigation_definitions::Tree,ai_definition_strings::{Strings,Field},
    ai_lpf_module::{self,State,Point,PointPool,Taboo},ai_goto_gap::{Pool},ai_pathfinder_constructor::State as Pathfinder};
pub trait Backend {
    fn unit_scale(&mut self,state:&mut State)->Result<Option<f32>>;
    fn manager(&mut self,state:&mut State)->Result<Option<usize>>;
    fn context(&mut self,state:&mut State,manager:Option<usize>,name:&[u8])->Result<Option<usize>>;
    fn errno(&mut self)->&mut i32;
    fn number(&mut self,state:&mut State,text:&[u8])->Result<f64>;
    /// Actual owner allocator identity (or a real null allocation), not a
    /// placeholder standing in for an initialized module.
    fn allocate_module(&mut self,pathfinder:&mut Pathfinder)->Result<Option<usize>>;
    fn point_pool(&mut self,state:&mut State,_:PointPool)->Result<Option<Pool<Point>>> {
        Ok(Some(Pool::new(state.local_path_max_length.context("LPF path length unwritten")?,Point::node)?))
    }
    fn release_scratch(&mut self,state:&mut State)->Result<()> {state.scratch=None;Ok(())}
    fn taboo_pool(&mut self,state:&mut State)->Result<Option<Pool<Taboo>>> {
        Ok(Some(Pool::new(state.taboo_edge_max_count.context("LPF taboo count unwritten")?,Taboo::node)?))
    }
}
pub struct Binding<'a,B>{tree:&'a mut Tree,backend:&'a mut B,strings:Strings}
impl<'a,B:Backend> Binding<'a,B> {
    pub fn new(tree:&'a mut Tree,backend:&'a mut B)->Self {Self{tree,backend,strings:Strings::default()}}
}
impl<B:Backend> ai_lpf_module::Host for Binding<'_,B> {
    fn unit_scale(&mut self,state:&mut State)->Result<Option<f32>> {self.backend.unit_scale(state)}
    fn manager(&mut self,state:&mut State)->Result<Option<usize>> {self.backend.manager(state)}
    fn child_count(&mut self,_:&mut State,root:usize)->Result<u32> {Ok(u32::try_from(self.tree.node(root)?.children.len())?)}
    fn child(&mut self,_:&mut State,root:usize,index:u32)->Result<Option<usize>> {Ok(self.tree.node(root)?.children.get(index as usize).copied())}
    fn name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>> {Ok(self.tree.node(node)?.name.clone())}
    fn value(&mut self,_:&mut State,node:usize)->Result<Option<usize>> {self.strings.capture(self.tree,node,Field::Value)}
    fn integer(&mut self,_:&mut State,value:usize)->Result<i32> {
        let text=self.strings.text(self.tree,value)?;Ok(super::native_text_number::atoi(&text,self.backend.errno()))
    }
    fn number(&mut self,state:&mut State,value:usize)->Result<f64> {let text=self.strings.text(self.tree,value)?;self.backend.number(state,&text)}
    fn context(&mut self,state:&mut State,manager:Option<usize>,value:usize)->Result<Option<usize>> {let text=self.strings.text(self.tree,value)?;self.backend.context(state,manager,&text)}
    fn point_pool(&mut self,state:&mut State,kind:PointPool)->Result<Option<Pool<Point>>> {self.backend.point_pool(state,kind)}
    fn release_scratch(&mut self,state:&mut State)->Result<()> {self.backend.release_scratch(state)}
    fn taboo_pool(&mut self,state:&mut State)->Result<Option<Pool<Taboo>>> {self.backend.taboo_pool(state)}
}
impl Pathfinder {
    /// LPF ownership portion48ffd8..490080. The remaining48fec4 prelude,
    /// definition/options, search buffers and default modifiers are separate;
    /// success here is NOT a ready/registered pathfinder.
    pub fn initialize_lpf_definition(&mut self,tree:&mut Tree,root:usize,backend:&mut impl Backend)->Result<bool> {
        let Some(node)=tree.find_tag(root,b"LpfModule")? else{return Ok(true)};
        let Some(identity)=backend.allocate_module(self)? else {
            self.reference_f8=Some(None);self.lpf=None;return Ok(false);
        };
        let module=State::construct(identity,self.identity,&mut Binding::new(tree,backend))?;
        self.reference_f8=Some(Some(identity));self.lpf=Some(Box::new(module));
        self.lpf.as_mut().context("Owned LPF module missing")?.initialize(Some(node),&mut Binding::new(tree,backend))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_navigation_action::{Class,Registry},ai_navigation_construction::{Constructed,ActionClass,ActionFactory,Layout},
        ai_pathfinder_constructor::{Base,BaseHost,Counter,Service},ai_navigation_service_factory as factory,
        ai_biped_catalog::{Catalog,Database},ai_navigation_resolver as resolver,ai_navigation_definitions::Node};
    struct Construction;
    impl BaseHost for Construction {
        fn unit_scale(&mut self,_:&mut Base)->Result<Option<f32>> {Ok(Some(1.))}
        fn profiling_enabled(&mut self,_:&mut Base)->Result<bool> {Ok(false)}
        fn register_budget(&mut self,_:&mut Base,_:&str)->Result<i32> {anyhow::bail!("Unexpected profiler")}
    }
    impl super::super::ai_pathfinder_constructor::Host for Construction {
        fn world_service(&mut self,_:&mut Pathfinder,kind:Service)->Result<Option<usize>> {Ok(Some(match kind{Service::Graph=>80,Service::PathObjects=>90}))}
        fn point_attribute_index(&mut self,_:&mut Pathfinder)->Result<i32> {Ok(0)}
    }
    struct ContextServices {manager:bool,context:bool,allocated:bool,errno:i32,allocations:u32}
    impl Backend for ContextServices {
        fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>> {Ok(Some(1.))}
        fn manager(&mut self,_:&mut State)->Result<Option<usize>> {Ok(self.manager.then_some(7))}
        fn context(&mut self,_:&mut State,manager:Option<usize>,name:&[u8])->Result<Option<usize>> {
            assert_eq!(manager,Some(7));assert_eq!(name,b"LpfContext1");Ok(self.context.then_some(17))
        }
        fn errno(&mut self)->&mut i32 {&mut self.errno}
        fn number(&mut self,_:&mut State,_:&[u8])->Result<f64> {anyhow::bail!("No floating conversion supplied in this integer-only definition fixture")}
        fn allocate_module(&mut self,_:&mut Pathfinder)->Result<Option<usize>> {self.allocations+=1;Ok(self.allocated.then_some(100))}
    }
    fn registry()->Registry {Registry{count:2,heading:Class{index:0,constructor:true},speed:Class{index:1,constructor:true}}}
    fn owner()->Result<Constructed> {
        Constructed::new(20,Some(10),Some(30),&ActionClass{metadata:90,factory:Some(ActionFactory::BaseAi)},&Layout{agent_classes:3,service_classes:1,attributes:registry()})
    }
    fn resolve(catalog:&Catalog,name:&[u8])->Result<resolver::Resolved> {
        resolver::Runtime{world:None,allocator:1,fallback_allocator:2}.resolve(
            Some(&resolver::Manager{identity:7,allocator:None,definition_root:Some(catalog.root)}),&catalog.tree,Some(name))?.context("Biped definition missing")
    }
    fn service(tree:&Tree,root:usize)->Result<usize> {
        tree.find_tag(tree.find_tag(root,b"Brain")?.context("Brain definition missing")?,b"Service")?.context("Service definition missing")
    }
    #[test]
    fn source_biped_owned_lpf_stage_binds_native_integer_options_and_failures()->Result<()> {
        for (manager,context,allocated) in [(true,true,true),(false,true,true),(true,false,true),(true,true,false)] {
            let catalog=Catalog::new(&[Database{radius:0,height:0,path:Some(b"AI/ShortThinBiped/example.hpd".to_vec())}])?;
            let mut resolved=resolve(&catalog,b"ShortThinBiped")?;let root=service(&resolved.tree,resolved.root)?;
            let mut owned=factory::construct(&resolved.tree,root,50,&owner()?,40,&registry(),&mut Counter::default(),&mut Construction)?.context("Pathfinder not constructed")?;
            let mut backend=ContextServices{manager,context,allocated,errno:7,allocations:0};
            assert_eq!(owned.initialize_lpf_definition(&mut resolved.tree,root,&mut backend)?,manager && context && allocated);
            let factory::Owned::Pathfinder(state)=owned;
            assert_eq!(state.reference_f8,Some(allocated.then_some(100)));
            assert_eq!(state.lpf.is_some(),allocated);assert_eq!(backend.allocations,1);assert_eq!(backend.errno,7);
            if let Some(module)=&state.lpf {
                assert_eq!(module.pathfinder,Some(state.identity));
                if manager && context {
                    assert_eq!(module.local_path_max_length,Some(128));assert_eq!(module.taboo_edge_max_count,Some(30));
                    assert_eq!(module.points.as_ref().unwrap().nodes.len(),128);assert_eq!(module.scratch.as_ref().unwrap().nodes.len(),128);assert_eq!(module.taboo.as_ref().unwrap().nodes.len(),30);
                } else {assert!(module.points.is_none() && module.scratch.is_none() && module.taboo.is_none());}
            }
        }
        Ok(())
    }
    #[test]
    fn absent_lpf_does_not_allocate_and_integer_overflow_preserves_native_errno()->Result<()> {
        let mut tree=Tree::default();let root=tree.append(None,Node::default())?;
        let mut pathfinder=Pathfinder::construct(50,&owner()?,40,&registry(),&mut Counter::default(),&mut Construction)?;
        let mut backend=ContextServices{manager:true,context:true,allocated:true,errno:7,allocations:0};
        assert!(pathfinder.initialize_lpf_definition(&mut tree,root,&mut backend)?);
        assert_eq!(backend.allocations,0);assert_eq!(pathfinder.reference_f8,Some(None));assert!(pathfinder.lpf.is_none());
        let attribute=tree.append(None,Node{name:Some(b"SmoothingMaxNodeSkip".to_vec()),value:Some(b"2147483648suffix".to_vec()),..Node::default()})?;
        let mut binding=Binding::new(&mut tree,&mut backend);let mut module=State::construct(100,50,&mut binding)?;
        assert!(module.attribute(Some(attribute),&mut binding)?);assert_eq!(module.smoothing_max_node_skip,Some(i32::MAX as u32));
        assert_eq!(backend.errno,34);assert!(module.points.is_none());Ok(())
    }
    #[test]
    #[ignore="requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_lpf_definition_initializes_owned_pools_with_supplied_context()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let world=super::super::assets::load(std::path::Path::new(&root),"Levels/DBC_Start_1st.Level",&egui::Context::default())?;
        let gus=world.scripts.resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01").context("Original Gus absent")?;
        let physics=world.entities[gus].components.iter().find(|c|c.class_name=="PhysicsRigidBodyCharacterComponent").context("Gus character component absent")?;
        let selector=|name:&str|->Result<i32>{Ok(physics.properties.iter().find(|p|p.name==name).context("Gus shape selector missing")?.value.as_i64().context("Gus selector is not integer")? as i32)};
        let name=super::super::ai_agent_profiles::descriptor(selector("Entity Radius")?,selector("Entity Height")?,1);
        let catalog=world.biped_navigation_catalog()?;let mut resolved=resolve(&catalog,name.as_bytes())?;let root=service(&resolved.tree,resolved.root)?;
        let mut owned=factory::construct(&resolved.tree,root,50,&owner()?,40,&registry(),&mut Counter::default(),&mut Construction)?.context("Original service class absent")?;
        // Global context/manager objects remain explicit supplied boundaries.
        // These objects are not registered as a live World agent by this test.
        let mut backend=ContextServices{manager:true,context:true,allocated:true,errno:7,allocations:0};
        assert!(owned.initialize_lpf_definition(&mut resolved.tree,root,&mut backend)?);
        let factory::Owned::Pathfinder(state)=owned;let module=state.lpf.as_ref().context("Original LPF module not owned")?;
        assert_eq!(state.reference_f8,Some(Some(module.identity)));
        assert_eq!(module.points.as_ref().unwrap().nodes.len(),128);assert_eq!(module.taboo.as_ref().unwrap().nodes.len(),30);
        Ok(())
    }
}
