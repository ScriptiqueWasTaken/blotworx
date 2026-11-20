//! Real definition-tree binding for465d74 ->492440 ->492b14. Shared scalar
//! and all thirteen modifier-table handlers run unchanged. Native number
//! conversion, database ownership and concrete modifier implementations are
//! mandatory backend services; an unimplemented modifier cannot pass silently.
use anyhow::{Context,Result};
use super::{ai_navigation_definitions::Tree,ai_definition_strings::{Strings,Field},ai_pathfinder_constructor::State,
    ai_pathfinder_attributes as attributes,ai_pathfinder_modifiers::{self as modifiers,Kind,Storage}};
pub trait Backend {
    fn integer(&mut self,state:&mut State,text:Option<&[u8]>)->Result<i32>;
    fn number(&mut self,state:&mut State,text:&[u8])->Result<f64>;
    fn unit_scale(&mut self,state:&mut State)->Result<Option<f32>>;
    fn find_database(&mut self,state:&mut State,manager:usize,name:&[u8])->Result<Option<usize>>;
    fn register_database_callback(&mut self,state:&mut State,database:usize)->Result<()>;
    fn class(&mut self,state:&mut State,kind:Kind,name:&[u8])->Result<Option<usize>>;
    fn construct(&mut self,state:&mut State,kind:Kind,class:usize,identifier:&[u8])->Result<usize>;
    fn initialize_modifier(&mut self,state:&mut State,kind:Kind,object:usize,tree:&mut Tree,node:usize)->Result<bool>;
    fn object_name(&mut self,state:&mut State,kind:Kind,object:usize)->Result<Vec<u8>>;
    fn allocate_table(&mut self,state:&mut State,kind:Kind,capacity:u32)->Result<Storage>;
    fn free_table(&mut self,state:&mut State,kind:Kind,storage:usize)->Result<()>;
}
pub trait DefaultsBackend:Backend {
    fn default_class(&self,kind:Kind)->Result<usize>;
    fn object_class(&mut self,state:&mut State,kind:Kind,object:usize)->Result<usize>;
    fn parent_class(&mut self,state:&mut State,class:usize)->Result<Option<usize>>;
    fn construct_default(&mut self,state:&mut State,kind:Kind,class:usize,name:&[u8])->Result<Option<usize>>;
    fn initialize_default(&mut self,state:&mut State,kind:Kind,object:usize,tree:&mut Tree)->Result<bool>;
}
pub struct Binding<'a,B> {
    pub tree:&'a mut Tree,
    pub backend:&'a mut B,
    //45b8bc returns a raw buffer. Keep its source/allocation identity, not
    //a snapshot of its bytes or an invented retained string ownership.
    strings:Strings,
}
impl<'a,B:Backend> Binding<'a,B> {
    pub fn new(tree:&'a mut Tree,backend:&'a mut B)->Self {Self{tree,backend,strings:Strings::default()}}
    fn string(&self,handle:usize)->Result<Vec<u8>> {self.strings.text(self.tree,handle)}
    fn value_handle(&mut self,node:usize,identifier:bool)->Result<Option<usize>> {
        self.strings.capture(self.tree,node,if identifier{Field::Identifier}else{Field::Value})
    }
    pub fn initialize(&mut self,state:&mut State,root:usize)->Result<bool> {
        super::ai_navigation_service_init::initialize(state,root,self)
    }
}
impl<B:Backend> super::ai_navigation_service_init::Host<State> for Binding<'_,B> {
    fn child_count(&mut self,_:&mut State,root:usize)->Result<u32> {Ok(u32::try_from(self.tree.node(root)?.children.len())?)}
    fn child(&mut self,_:&mut State,root:usize,index:u32)->Result<Option<usize>> {Ok(self.tree.node(root)?.children.get(index as usize).copied())}
    fn name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>> {Ok(self.tree.node(node)?.name.clone())}
    fn initialize_attribute(&mut self,state:&mut State,node:usize)->Result<bool> {attributes::initialize(state,node,self)}
}
impl<B:Backend> attributes::Host for Binding<'_,B> {
    fn name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>> {Ok(self.tree.node(node)?.name.clone())}
    fn value(&mut self,_:&mut State,node:usize)->Result<Option<usize>> {self.value_handle(node,false)}
    fn text(&mut self,_:&mut State,handle:usize)->Result<Vec<u8>> {self.string(handle)}
    fn integer(&mut self,state:&mut State,value:Option<usize>)->Result<i32> {
        let text=value.map(|id|self.string(id)).transpose()?;self.backend.integer(state,text.as_deref())
    }
    fn number(&mut self,state:&mut State,value:usize)->Result<f64> {let text=self.string(value)?;self.backend.number(state,&text)}
    fn unit_scale(&mut self,state:&mut State)->Result<Option<f32>> {self.backend.unit_scale(state)}
    fn find_database(&mut self,state:&mut State,manager:usize,value:usize)->Result<Option<usize>> {let text=self.string(value)?;self.backend.find_database(state,manager,&text)}
    fn register_database_callback(&mut self,state:&mut State,database:usize)->Result<()> {self.backend.register_database_callback(state,database)}
    fn modifier_attribute(&mut self,state:&mut State,node:usize)->Result<Option<bool>> {modifiers::initialize(state,node,self)}
}
impl<B:Backend> modifiers::Host for Binding<'_,B> {
    fn name(&mut self,_:&mut State,node:usize)->Result<Option<Vec<u8>>> {Ok(self.tree.node(node)?.name.clone())}
    fn value(&mut self,_:&mut State,node:usize)->Result<Option<usize>> {self.value_handle(node,false)}
    fn text(&mut self,_:&mut State,handle:usize)->Result<Vec<u8>> {self.string(handle)}
    fn identifier(&mut self,_:&mut State,node:usize)->Result<Option<usize>> {self.value_handle(node,true)}
    fn child_count(&mut self,_:&mut State,node:usize)->Result<u32> {Ok(u32::try_from(self.tree.node(node)?.children.len())?)}
    fn child(&mut self,_:&mut State,node:usize,index:u32)->Result<Option<usize>> {Ok(self.tree.node(node)?.children.get(index as usize).copied())}
    fn class_child(&mut self,_:&mut State,node:usize)->Result<Option<usize>> {self.tree.find_tag(node,b"Class")}
    fn class(&mut self,state:&mut State,kind:Kind,value:usize)->Result<Option<usize>> {let text=self.string(value)?;self.backend.class(state,kind,&text)}
    fn construct(&mut self,state:&mut State,kind:Kind,class:usize,identifier:usize)->Result<usize> {let text=self.string(identifier)?;self.backend.construct(state,kind,class,&text)}
    fn initialize_modifier(&mut self,state:&mut State,kind:Kind,object:usize,node:usize)->Result<bool> {self.backend.initialize_modifier(state,kind,object,self.tree,node)}
    fn initialize_attribute(&mut self,state:&mut State,node:usize)->Result<bool> {attributes::initialize(state,node,self)}
    fn object_name(&mut self,state:&mut State,kind:Kind,object:usize)->Result<Vec<u8>> {self.backend.object_name(state,kind,object)}
    fn allocate_table(&mut self,state:&mut State,kind:Kind,capacity:u32)->Result<Storage> {self.backend.allocate_table(state,kind,capacity)}
    fn free_table(&mut self,state:&mut State,kind:Kind,storage:usize)->Result<()> {self.backend.free_table(state,kind,storage)}
}
impl<B:DefaultsBackend> modifiers::DefaultsHost for Binding<'_,B> {
    fn default_class(&self,kind:Kind)->Result<usize>{self.backend.default_class(kind)}
    fn object_class(&mut self,state:&mut State,kind:Kind,object:usize)->Result<usize>{self.backend.object_class(state,kind,object)}
    fn parent_class(&mut self,state:&mut State,class:usize)->Result<Option<usize>>{self.backend.parent_class(state,class)}
    fn construct_default(&mut self,state:&mut State,kind:Kind,class:usize,name:&[u8])->Result<Option<usize>>{self.backend.construct_default(state,kind,class,name)}
    fn initialize_default(&mut self,state:&mut State,kind:Kind,object:usize)->Result<bool>{self.backend.initialize_default(state,kind,object,self.tree)}
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_navigation_action::{Class,Registry},ai_navigation_construction::{Constructed,Layout,ActionClass,ActionFactory},
        ai_pathfinder_constructor::{Base,BaseHost,Counter,Service},ai_navigation_definitions::Node};
    struct Construction;
    impl BaseHost for Construction {
        fn unit_scale(&mut self,_:&mut Base)->Result<Option<f32>> {Ok(None)}
        fn profiling_enabled(&mut self,_:&mut Base)->Result<bool> {Ok(false)}
        fn register_budget(&mut self,_:&mut Base,_:&str)->Result<i32> {anyhow::bail!("Unexpected profiler")}
    }
    impl super::super::ai_pathfinder_constructor::Host for Construction {
        fn world_service(&mut self,_:&mut State,kind:Service)->Result<Option<usize>> {Ok(Some(match kind{Service::Graph=>5,Service::PathObjects=>6}))}
        fn point_attribute_index(&mut self,_:&mut State)->Result<i32> {Ok(0)}
    }
    fn state()->Result<State> {
        let registry=||Registry{count:2,heading:Class{index:0,constructor:true},speed:Class{index:1,constructor:true}};
        let owner=Constructed::new(20,Some(10),Some(30),&ActionClass{metadata:90,factory:Some(ActionFactory::BaseAi)},
            &Layout{agent_classes:3,service_classes:1,attributes:registry()})?;
        State::construct(50,&owner,40,&registry(),&mut Counter::default(),&mut Construction)
    }
    #[derive(Default)]
    struct MissingModifiers {classes:Vec<Vec<u8>>,allocations:usize}
    impl Backend for MissingModifiers {
        fn integer(&mut self,_:&mut State,_:Option<&[u8]>)->Result<i32> {anyhow::bail!("Numeric parser not supplied by this fixture")}
        fn number(&mut self,_:&mut State,_:&[u8])->Result<f64> {anyhow::bail!("Numeric parser not supplied by this fixture")}
        fn unit_scale(&mut self,_:&mut State)->Result<Option<f32>> {Ok(None)}
        fn find_database(&mut self,_:&mut State,_:usize,_:&[u8])->Result<Option<usize>> {anyhow::bail!("Database must not precede the authored modifiers")}
        fn register_database_callback(&mut self,_:&mut State,_:usize)->Result<()> {anyhow::bail!("No database admitted")}
        fn class(&mut self,_:&mut State,_:Kind,name:&[u8])->Result<Option<usize>> {self.classes.push(name.to_vec());anyhow::bail!("Required modifier not implemented: {}",String::from_utf8_lossy(name))}
        fn construct(&mut self,_:&mut State,_:Kind,_:usize,_:&[u8])->Result<usize> {anyhow::bail!("Unresolved modifier must not construct")}
        fn initialize_modifier(&mut self,_:&mut State,_:Kind,_:usize,_:&mut Tree,_:usize)->Result<bool> {anyhow::bail!("No modifier constructed")}
        fn object_name(&mut self,_:&mut State,_:Kind,_:usize)->Result<Vec<u8>> {anyhow::bail!("No modifier constructed")}
        fn allocate_table(&mut self,_:&mut State,_:Kind,capacity:u32)->Result<Storage> {self.allocations+=1;Ok(Storage{identity:self.allocations,slots:vec![None;capacity as usize]})}
        fn free_table(&mut self,_:&mut State,_:Kind,_:usize)->Result<()> {anyhow::bail!("Fresh fixture should not free a table")}
    }
    #[test]
    fn source_biped_service_runs_real_attributes_and_rejects_missing_modifier()->Result<()> {
        use super::super::{ai_biped_catalog::{Catalog,Database},ai_navigation_resolver as resolver};
        let catalog=Catalog::new(&[Database{radius:0,height:0,path:Some(b"AI/ShortThinBiped/example.hpd".to_vec())}])?;
        let mut resolution=resolver::Runtime{world:None,allocator:1,fallback_allocator:2};
        let manager=resolver::Manager{identity:7,allocator:None,definition_root:Some(catalog.root)};
        let mut resolved=resolution.resolve(Some(&manager),&catalog.tree,Some(b"ShortThinBiped"))?.context("Biped not resolved")?;
        let brain=resolved.tree.find_tag(resolved.root,b"Brain")?.context("Brain not resolved")?;
        let service=resolved.tree.find_tag(brain,b"Service")?.context("Service not resolved")?;
        let mut state=state()?;let mut backend=MissingModifiers::default();
        let error=Binding::new(&mut resolved.tree,&mut backend).initialize(&mut state,service).unwrap_err();
        assert!(error.to_string().contains("Mpd::CGoto_GapDynamicAvoidance_v2"));
        assert_eq!(backend.classes,[b"Mpd::CGoto_GapDynamicAvoidance_v2".to_vec()]);
        assert_eq!(state.flag_41c,1,"Authored AvoidCpuPeaks was not dispatched");
        let table=&state.tables_fc_1fc[Kind(8).table()];
        assert_eq!((table.capacity,table.count,table.selected),(2,0,Some(None)));
        Ok(())
    }
    #[test]
    fn definition_handles_follow_live_buffers_and_reject_replaced_storage()->Result<()> {
        let mut tree=Tree::default();let node=tree.append(None,Node{value:Some(b"Old".to_vec()),..Node::default()})?;
        let mut backend=MissingModifiers::default();let mut binding=Binding::new(&mut tree,&mut backend);let mut state=state()?;
        let old=attributes::Host::value(&mut binding,&mut state,node)?.context("Old string absent")?;
        binding.tree.nodes[node].value.as_mut().unwrap().copy_from_slice(b"New");
        assert_eq!(attributes::Host::text(&mut binding,&mut state,old)?,b"New");
        let kept=std::mem::replace(&mut binding.tree.nodes[node].value,Some(b"Replacement".to_vec()));
        assert!(attributes::Host::text(&mut binding,&mut state,old).is_err(),"Freed native buffer became a cached snapshot");
        let new=attributes::Host::value(&mut binding,&mut state,node)?.context("New string absent")?;
        assert_eq!(attributes::Host::text(&mut binding,&mut state,new)?,b"Replacement");
        drop(kept);
        Ok(())
    }
}
