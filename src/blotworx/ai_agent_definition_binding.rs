//!456530 over the actual ordered definition tree and canonical action store.
//! Kernel handles are index+1, so a real root at Tree index0 is never null.
use anyhow::{Context,Result};
use super::{ai_agent_action_init as init,ai_agent_base::Owner,
    ai_agent_actions::View,ai_navigation_definitions::Tree};
pub trait Backend<S:Owner>:super::ai_agent_actions::ConstructionHost {
    fn attribute(&mut self,agent:&mut S,tree:&mut Tree,actions:&mut View<'_>,node:usize)->Result<u8>;
}
pub struct Binding<'a,'s,H> {pub tree:&'a mut Tree,pub actions:View<'s>,pub backend:&'a mut H}
fn index(handle:usize)->Result<usize>{handle.checked_sub(1).context("Null agent definition node")}
impl<H> Binding<'_, '_, H> {
    pub fn initialize<S:Owner>(&mut self,agent:&mut S,root:usize)->Result<bool> where H:Backend<S> {
        let handle=root.checked_add(1).context("Agent definition handle overflow")?;
        init::initialize(agent,handle,self)
    }
    pub fn acknowledge<S:Owner>(&mut self,agent:&S)->Result<bool> where H:Backend<S>{init::acknowledge(agent,self)}
}
impl<S:Owner,H:Backend<S>> init::Host<S> for Binding<'_, '_, H> {
    fn child_count(&mut self,root:usize)->Result<u32>{u32::try_from(self.tree.node(index(root)?)?.children.len()).context("Agent definition child count overflow")}
    fn child(&mut self,root:usize,i:u32)->Result<usize>{
        self.tree.node(index(root)?)?.children.get(i as usize).context("Agent definition child disappeared")?
            .checked_add(1).context("Agent definition child handle overflow")
    }
    fn name_matches(&mut self,node:usize,name:&str)->Result<bool>{
        let value=self.tree.node(index(node)?)?.name.as_deref().context("Agent attribute has no name")?;
        Ok(super::ai_traversal_registry::names_equal(value,name.as_bytes()))
    }
    fn value_matches(&mut self,node:usize,value:&str)->Result<bool>{
        let raw=self.tree.node(index(node)?)?.value.as_deref().context("ShareBrainAction has no value")?;
        Ok(super::ai_traversal_registry::names_equal(raw,value.as_bytes()))
    }
    fn owner_action(&mut self,owner:usize)->Result<Option<usize>>{self.actions.owner_action(owner)}
    fn action_class(&mut self,action:usize)->Result<usize>{self.actions.action_class(action)}
    fn construct_action(&mut self,class:usize)->Result<Option<usize>>{Ok(Some(self.actions.construct(class,self.backend)?))}
    fn attribute(&mut self,agent:&mut S,node:usize)->Result<u8>{self.backend.attribute(agent,self.tree,&mut self.actions,index(node)?)}
    fn acknowledge_action(&mut self,action:usize)->Result<()>{self.actions.acknowledge(action)}
}

/// The real Flee virtual10, rather than a success callback or a second copy of
/// the definition. Its required traversal/task/speed services remain explicit.
impl<H:super::ai_flee_definition_binding::Backend+super::ai_agent_actions::ConstructionHost> Backend<super::ai_flee_agent::State> for H {
    fn attribute(&mut self,s:&mut super::ai_flee_agent::State,tree:&mut Tree,_:&mut View<'_>,node:usize)->Result<u8> {
        Ok(u8::from(super::ai_flee_definition_binding::Binding::new(tree,self).attribute(s,Some(node))?))
    }
}
impl<H:super::ai_path_agent_attribute_binding::Backend+super::ai_agent_actions::ConstructionHost> Backend<super::ai_goto_agent::Agent> for H {
    fn attribute(&mut self,a:&mut super::ai_goto_agent::Agent,tree:&mut Tree,_:&mut View<'_>,node:usize)->Result<u8>{
        Ok(u8::from(super::ai_path_agent_attribute_binding::Binding::new(tree,self).goto(a,Some(node))?))
    }
}
impl<H:super::ai_agent_actions::ConstructionHost> Backend<super::ai_path_agent::Agent> for H {
    fn attribute(&mut self,a:&mut super::ai_path_agent::Agent,tree:&mut Tree,_:&mut View<'_>,node:usize)->Result<u8>{
        Ok(u8::from(super::ai_path_agent_attribute_binding::Binding::new(tree,self).wander(a,Some(node))?))
    }
}
impl<H:super::ai_agent_base::Host> super::ai_agent_base::Host for Binding<'_, '_, H> {
    fn owner_is_112a0(&mut self,o:usize)->Result<bool>{self.backend.owner_is_112a0(o)}
    fn blackboard_head(&mut self,o:usize)->Result<Option<usize>>{self.backend.blackboard_head(o)}
    fn owner_service_6af6c(&mut self,o:usize)->Result<Option<usize>>{self.backend.owner_service_6af6c(o)}
    fn world_service_61d14(&mut self)->Result<Option<usize>>{self.backend.world_service_61d14()}
    fn world_service_45dcc(&mut self)->Result<Option<usize>>{self.backend.world_service_45dcc()}
    fn initialize_attributes(&mut self,a:&mut super::ai_agent_base::Agent,n:usize)->Result<bool>{self.backend.initialize_attributes(a,n)}
    fn speed_value_360e4(&mut self,o:usize)->Result<Option<f32>>{self.backend.speed_value_360e4(o)}
    fn action_vector(&mut self,o:usize)->Result<Option<[f32;3]>>{self.backend.action_vector(o)}
    fn action_scalar(&mut self,o:usize)->Result<Option<f32>>{self.backend.action_scalar(o)}
}
impl<H:super::ai_agent_base::Host+Backend<super::ai_flee_agent::State>>
    super::ai_agent_base::BindingHost<super::ai_flee_agent::State> for Binding<'_, '_, H> {
    fn initialize_owned_attributes(&mut self,s:&mut super::ai_flee_agent::State,root:usize)->Result<bool>{self.initialize(s,root)}
}

impl<H:super::ai_agent_base::Host+Backend<super::ai_goto_agent::Agent>>
    super::ai_agent_base::BindingHost<super::ai_goto_agent::Agent> for Binding<'_, '_, H> {
    fn initialize_owned_attributes(&mut self,a:&mut super::ai_goto_agent::Agent,root:usize)->Result<bool>{self.initialize(a,root)}
}
impl<H:super::ai_agent_base::Host+Backend<super::ai_path_agent::Agent>>
    super::ai_agent_base::BindingHost<super::ai_path_agent::Agent> for Binding<'_, '_, H> {
    fn initialize_owned_attributes(&mut self,a:&mut super::ai_path_agent::Agent,root:usize)->Result<bool>{self.initialize(a,root)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{ai_navigation_action::{self as action,Kind},ai_navigation_construction::{ActionClass,ActionFactory,Constructed,Layout},ai_navigation_definitions::Node,ai_agent_actions::{Store,View},ai_agent_base::Agent};
    fn metadata()->(ActionClass,Layout) {
        (ActionClass{metadata:40,factory:Some(ActionFactory::BaseAi)},Layout{agent_classes:3,service_classes:1,attributes:action::Registry{count:2,heading:action::Class{index:0,constructor:true},speed:action::Class{index:1,constructor:true}}})
    }
    fn definitions(shared:bool)->Result<(Tree,usize)> {
        let mut t=Tree::default();let root=t.append(None,Node::default())?;
        for (name,value) in [("Class","Mpd::JPSFleeAgent"),("ShareBrainAction",if shared {"true"}else{"false"}),("Speed","2")] {
            t.append(Some(root),Node{name:Some(name.as_bytes().to_vec()),value:Some(value.as_bytes().to_vec()),..Default::default()})?;
        }Ok((t,root))
    }
    struct Requests;
    impl super::super::ai_agent_actions::ConstructionHost for Requests {
        fn action_class_constructed(&mut self,a:&mut View<'_>,id:usize)->Result<()>{
            // Global metadata registration is supplied for this storage fixture.
            assert_eq!(a.action_class(id)?,40);assert!(a.private.objects.contains_key(&id));Ok(())
        }
    }
    impl Backend<Agent> for Requests {
        fn attribute(&mut self,a:&mut Agent,_:&mut Tree,actions:&mut View<'_>,_:usize)->Result<u8>{
            actions.action_mut(a.action_04.context("Attribute has no action")?)?.request(Kind::Speed,2.)?;Ok(1)
        }
    }
    #[test]
    fn real_tree_root_zero_and_shared_private_actions_keep_one_canonical_object()->Result<()> {
        for shared in [false,true] {
            let (class,layout)=metadata();let mut parent=Constructed::new(7,Some(5),Some(20),&class,&layout)?;
            parent.action.as_mut().unwrap().request(Kind::Speed,9.)?;
            let mut store=Store::default();let mut next=20;
            let (mut tree,root)=definitions(shared)?;assert_eq!(root,0);
            let mut a=Agent::new(Some(7));
            let mut b=Binding{tree:&mut tree,actions:View{parent:(&mut parent).into(),private:&mut store,class:&class,registry:&layout.attributes,next_identity:&mut next},backend:&mut Requests};
            assert!(b.initialize(&mut a,root)?);
            let id=a.action_04.unwrap();assert_eq!(id,if shared{20}else{21});
            assert_eq!(b.actions.private.objects.len(),usize::from(!shared));
            assert_eq!(b.actions.action_mut(id)?.scalar(Kind::Speed)?.value_bits,2f32.to_bits());
            b.acknowledge(&a)?;
            assert_eq!(b.actions.action_mut(id)?.scalar(Kind::Speed)?.changed,u8::from(shared));
            assert_eq!(b.actions.parent.action.as_ref().unwrap().scalar(Kind::Speed)?.changed,1);
            if shared{assert!(b.actions.destroy_private(id).is_err());}
            else{b.actions.destroy_private(id)?;assert!(b.actions.private.objects.is_empty());assert_eq!(b.actions.parent.action.as_ref().unwrap().scalar(Kind::Speed)?.value_bits,9f32.to_bits());}
            assert_eq!(*b.actions.next_identity,if shared{20}else{21});
        }Ok(())
    }
    #[test]
    fn private_action_access_rejects_foreign_ownership_and_missing_metadata()->Result<()> {
        let (mut class,layout)=metadata();let mut parent=Constructed::new(7,Some(5),Some(20),&class,&layout)?;
        let mut store=Store::default();let mut next=20;
        let mut v=View{parent:(&mut parent).into(),private:&mut store,class:&class,registry:&layout.attributes,next_identity:&mut next};
        assert!(v.owner_action(99).is_err());let id=v.construct(40,&mut Requests)?;
        v.private.objects.get_mut(&id).unwrap().owner=99;
        assert!(v.action_mut(id).is_err());assert!(v.action_class(id).is_err());assert!(v.destroy_private(id).is_err());
        v.private.objects.get_mut(&id).unwrap().owner=7;assert!(v.private.has_owner(7));v.destroy_private(id)?;
        assert!(!v.private.has_owner(7));drop(v);
        class.factory=None;
        let mut v=View{parent:(&mut parent).into(),private:&mut store,class:&class,registry:&layout.attributes,next_identity:&mut next};
        assert!(v.construct(40,&mut Requests).unwrap_err().to_string().contains("no factory"));assert!(v.construct(41,&mut Requests).is_err());
        assert_eq!(*v.next_identity,21);Ok(())
    }
    struct Services {traversal:i32,trace:Vec<&'static str>}
    use super::super::{ai_flee_agent::State,ai_graph_link_passes::Memory};
    impl super::super::ai_agent_actions::ConstructionHost for Services {
        fn action_class_constructed(&mut self,a:&mut View<'_>,id:usize)->Result<()>{
            assert_eq!(a.action_class(id)?,40);self.trace.push("action_constructed");Ok(())
        }
    }
    impl super::super::ai_flee_agent::Host for Services {
        fn global_unit_scale(&mut self)->Result<Option<f32>>{Ok(None)}
        fn owner_entity(&mut self,_:&mut State)->Result<usize>{anyhow::bail!("Unexpected entity query")}
        fn release_reservation(&mut self,_:&mut State,_:&mut Memory,_:usize)->Result<()>{anyhow::bail!("Unexpected reservation")}
        fn detach_service(&mut self,_:&mut State,_:&mut Memory,_:usize,_:usize)->Result<()>{anyhow::bail!("Unexpected detach")}
    }
    impl super::super::ai_agent_base::Host for Services {
        fn owner_is_112a0(&mut self,o:usize)->Result<bool>{assert_eq!(o,7);self.trace.push("class");Ok(true)}
        fn blackboard_head(&mut self,_:usize)->Result<Option<usize>>{self.trace.push("blackboard");Ok(Some(0))}
        fn owner_service_6af6c(&mut self,_:usize)->Result<Option<usize>>{self.trace.push("pathfinder");Ok(Some(50))}
        fn world_service_61d14(&mut self)->Result<Option<usize>>{self.trace.push("world");Ok(Some(60))}
        fn world_service_45dcc(&mut self)->Result<Option<usize>>{self.trace.push("optional");Ok(None)}
        fn initialize_attributes(&mut self,_:&mut Agent,_:usize)->Result<bool>{anyhow::bail!("Lost complete derived Flee agent")}
        fn speed_value_360e4(&mut self,_:usize)->Result<Option<f32>>{self.trace.push("base_speed");Ok(Some(5.))}
        fn action_vector(&mut self,_:usize)->Result<Option<[f32;3]>>{anyhow::bail!("Unexpected motion cache")}
        fn action_scalar(&mut self,_:usize)->Result<Option<f32>>{anyhow::bail!("Unexpected scalar cache")}
    }
    impl super::super::ai_flee_definition_binding::Backend for Services {
        fn integer(&mut self,_:&mut State,_:&mut Tree,_:&[u8])->Result<i32>{anyhow::bail!("Unexpected integer")}
        fn number(&mut self,_:&mut State,_:&mut Tree,t:&[u8])->Result<f64>{assert_eq!(t,b"2");self.trace.push("number");Ok(2.)}
        fn unit_scale(&mut self,_:&mut State,_:&mut Tree)->Result<Option<f32>>{self.trace.push("scale");Ok(Some(3.))}
        fn traversal(&mut self,_:&mut State,_:&mut Tree,t:Option<&[u8]>)->Result<i32>{assert_eq!(t,Some(b"FleeTraversal".as_slice()));self.trace.push("traversal");Ok(self.traversal)}
        fn task_manager(&mut self,_:&mut State,_:&mut Tree)->Result<usize>{self.trace.push("task_manager");Ok(70)}
        fn safety_task(&mut self,_:&mut State,_:&mut Tree,m:usize,t:Option<&[u8]>)->Result<i32>{assert_eq!(m,70);assert_eq!(t,Some(b"SafetyTask".as_slice()));self.trace.push("safety_task");Ok(4)}
        fn maximum_speed(&mut self,_:&mut State,_:&mut Tree)->Result<Option<f32>>{self.trace.push("limit");Ok(Some(5.))}
    }
    #[test]
    fn base_binding_executes_real_flee_attributes_inside_the_complete_agent()->Result<()> {
        for valid in [false,true] {
            let (class,layout)=metadata();let mut parent=Constructed::new(7,Some(5),Some(20),&class,&layout)?;
            let mut store=Store::default();let mut next=20;let (mut tree,root)=definitions(false)?;
            let traversal=tree.append(Some(root),Node{name:Some(b"TraversalConfig".to_vec()),value:Some(b"FleeTraversal".to_vec()),..Default::default()})?;
            // Native order is the serialized order, not a sorted schema order.
            tree.nodes[root].children.retain(|n|*n!=traversal);tree.nodes[root].children.insert(2,traversal);
            tree.append(Some(root),Node{name:Some(b"CheckSafetyTask".to_vec()),value:Some(b"SafetyTask".to_vec()),..Default::default()})?;
            let mut services=Services{traversal:if valid{2}else{-1},trace:vec![]};
            let mut s=State::new(Some(7),&mut services)?;
            let mut b=Binding{tree:&mut tree,actions:View{parent:(&mut parent).into(),private:&mut store,class:&class,registry:&layout.attributes,next_identity:&mut next},backend:&mut services};
            assert_eq!(super::super::ai_agent_base::bind_owned(&mut s,root,&mut b)?,valid);
            assert_eq!(s.base.action_04,Some(21));assert_eq!(b.actions.private.objects.len(),1);
            if valid{assert_eq!(s.word_48,2);assert_eq!(s.index_94,4);assert_eq!(s.speed_8c,5.);assert_eq!(s.base.speed,5.);}
            else{assert_eq!(s.word_48,u32::MAX);assert_eq!(s.index_94,-1);assert_eq!(s.speed_8c,1.);assert_eq!(s.base.speed,-1.);}
            let trace=&b.backend.trace;
            assert_eq!(&trace[..7],["class","blackboard","pathfinder","world","optional","action_constructed","traversal"]);
            if valid{assert_eq!(&trace[7..],["scale","number","limit","task_manager","safety_task","base_speed"]);}else{assert_eq!(trace.len(),7);}
        }Ok(())
    }
}
