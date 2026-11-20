//! Named entity registration: REL1f2864 returns the linked list's first
//! member. REL1f2bac suppresses duplicate IDs and appends new members at tail.
//! Reproducing source scene creation/activation order remains separate work.
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Default, Debug)]
pub struct Registry {
    members: BTreeMap<String, Vec<usize>>,
    suspended: BTreeMap<usize,Vec<String>>,
    factory_sources: BTreeSet<usize>,
}
impl Registry {
    pub fn register(&mut self, name: &str, entity: usize) {
        if let Some(names)=self.suspended.get_mut(&entity) {
            if !names.iter().any(|n|n==name){names.push(name.into());}
            return;
        }
        let members = self.members.entry(name.into()).or_default();
        if !members.contains(&entity) {
            members.push(entity);
        }
    }
    pub fn first(&self, name: &str) -> Option<usize> {
        // Explicit lookup can still address a source by its canonical name
        // when no live entity has inherited that name as a gameplay group.
        self.first_live(name).or_else(|| self.members(name).first().copied())
    }
    fn members(&self, name: &str) -> &[usize] {
        self.members.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn live_members(&self, name: &str) -> impl Iterator<Item = usize> + '_ {
        self.members(name).iter().copied().filter(|id| !self.factory_sources.contains(id))
    }
    pub fn first_live(&self, name: &str) -> Option<usize> {
        self.live_members(name).next()
    }
    /// Native iterators advance their linked node after invoking the callback.
    /// Consult the current list so callbacks that append members remain visible.
    pub fn next(&self, name: &str, current: usize) -> Result<Option<usize>, &'static str> {
        let members = self.members(name);
        let at=members.iter().position(|&member|member==current)
            .ok_or("Current group member removed during iteration; native iterator retirement is not reconstructed")?;
        Ok(members[at + 1..].iter().copied().find(|id| !self.factory_sources.contains(id)))
    }
    pub fn remove(&mut self, name: &str, entity: usize) {
        if let Some(names)=self.suspended.get_mut(&entity){names.retain(|n|n!=name);}
        if let Some(members) = self.members.get_mut(name) {
            members.retain(|&member| member != entity);
            if members.is_empty() {
                self.members.remove(name);
            }
        }
    }
    pub fn remove_entity(&mut self, entity: usize) {
        self.suspended.remove(&entity);
        self.factory_sources.remove(&entity);
        self.members.retain(|_, members| {
            members.retain(|&member| member != entity);
            !members.is_empty()
        });
    }
    pub fn mark_factory_source(&mut self, entity:usize, primary:&str) {
        self.factory_sources.insert(entity);
        if let Some(names)=self.suspended.get_mut(&entity){names.retain(|n|n==primary);}
        self.members.retain(|name,members| {
            if name!=primary {members.retain(|member|*member!=entity);}
            !members.is_empty()
        });
    }
    /// Cold scene storage is not a second live registration. Retain names for
    ///reactivation without making dormant scene aliases shadow live objects.
    pub fn suspend(&mut self,entity:usize) {
        self.suspend_many(&[entity]);
    }
    pub fn suspend_many(&mut self,entities:&[usize]) {
        let retiring=entities.iter().copied().filter(|id|!self.suspended.contains_key(id)).collect::<BTreeSet<_>>();
        if retiring.is_empty(){return}
        for &id in &retiring {self.suspended.insert(id,Vec::new());}
        let suspended=&mut self.suspended;
        self.members.retain(|name,members|{
            members.retain(|id|{
                if retiring.contains(id){suspended.get_mut(id).unwrap().push(name.clone());false}else{true}
            });
            !members.is_empty()
        });
    }
    pub fn resume(&mut self,entity:usize) {
        if let Some(names)=self.suspended.remove(&entity){for name in names{self.register(&name,entity);}}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_source_name_resolves_to_its_live_clone() {
        let mut registry = Registry::default();
        registry.register("Pete", 1);
        registry.register("Other", 1);
        registry.mark_factory_source(1, "Pete");
        assert_eq!(registry.first("Pete"), Some(1));
        assert_eq!(registry.first_live("Pete"), None);
        assert_eq!(registry.first("Other"), None);

        registry.register("Pete", 2);
        registry.suspend(1);
        registry.resume(1);
        registry.register("Pete", 3);
        assert_eq!(registry.first("Pete"), Some(2));
        assert_eq!(registry.live_members("Pete").collect::<Vec<_>>(), [2, 3]);
        assert_eq!(registry.next("Pete", 2), Ok(Some(3)));
        registry.remove_entity(2);
        registry.remove_entity(3);
        assert_eq!(registry.first("Pete"), Some(1));
        assert_eq!(registry.first_live("Pete"), None);
    }
    #[test]
    fn outgoing_scene_names_do_not_shadow_live_aliases_or_return_after_retirement() {
        let mut r=Registry::default();r.register("marker",1);r.register("marker",2);
        r.suspend(1);r.suspend(1);assert_eq!(r.first("marker"),Some(2));
        r.register("late",1);assert_eq!(r.first("late"),None);
        r.remove("marker",1);r.resume(1);assert_eq!(r.members("marker"),[2]);assert_eq!(r.first("late"),Some(1));
        r.suspend(1);r.remove_entity(1);r.resume(1);assert_eq!(r.first("late"),None);
        r.suspend(2);r.resume(2);assert_eq!(r.first("marker"),Some(2));
    }
    #[test]
    fn first_member_tracks_registration_order_without_duplicates() {
        let mut registry = Registry::default();
        registry.register("door", 17);
        registry.register("door", 2);
        registry.register("door", 17);
        assert_eq!(registry.members("door"), [17, 2]);
        assert_eq!(registry.first("door"), Some(17));
        registry.remove("door", 17);
        assert_eq!(registry.first("door"), Some(2));
        registry.register("door", 17);
        assert_eq!(registry.members("door"), [2, 17]);
        registry.remove("door", 2);
        registry.remove("door", 17);
        assert_eq!(registry.first("door"), None);
    }
}
