//! Scene-owned PC SH sources. Native light assignment and per-mesh visibility
//! remain separate from source registration; this never sorts by distance.
use anyhow::{ensure,Result};
use std::{collections::{BTreeMap,BTreeSet},sync::Arc};
use super::{d3d9_sh_cache::Source,d3d9_sh::LightRecord,pc_float::Precision};
pub(super) type Key=(usize,usize);
#[derive(Debug,Default)]
pub(super) struct Registry {
    sources:BTreeMap<Key,Source>,errors:BTreeMap<Key,String>,
    cached:BTreeMap<Key,(Precision,bool,Arc<LightRecord>)>,
    order:super::d3d9_sh_repository::Repository<Key,usize,()>,
}
impl Registry {
    /// Complete active-source enumeration. Omitted/disabled/invalid entries
    /// retire their old record immediately, including after a failed decode.
    pub fn refresh(&mut self,rows:impl IntoIterator<Item=(Key,Result<Option<Source>>)>) {
        self.refresh_scoped(rows.into_iter().map(|(key,value)|(key,None,String::new(),value)));
    }
    /// Scene identity and baking-component identity are owned by the runtime.
    pub fn refresh_scoped(&mut self,rows:impl IntoIterator<Item=(Key,Option<usize>,String,Result<Option<Source>>)>) {
        let mut seen=BTreeSet::new();
        let mut duplicates=BTreeSet::new();
        for (key,scene,name,value) in rows {
            if duplicates.contains(&key)||!seen.insert(key) {
                duplicates.insert(key);
                self.sources.remove(&key);self.cached.remove(&key);
                self.errors.insert(key,"Duplicate native SH source identity".into());continue;
            }
            match value {
                Ok(Some(source))=>{
                    self.errors.remove(&key);
                    self.order.update(Some(key),scene,Arc::new(()),name);
                    if !self.sources.get(&key).is_some_and(|old|old.same_bits(&source)) {self.cached.remove(&key);self.sources.insert(key,source);}
                },
                Ok(None)=>{self.sources.remove(&key);self.cached.remove(&key);self.errors.remove(&key);},
                Err(error)=>{self.sources.remove(&key);self.cached.remove(&key);self.errors.insert(key,format!("{error:#}"));},
            }
        }
        let removed:Vec<_>=self.order.entries().iter().filter_map(|entry|entry.owner).filter(|key|!seen.contains(key)||!self.sources.contains_key(key)).collect();
        for key in removed {self.order.remove_owner(&Some(key));}
        self.order.flush();
        self.sources.retain(|key,_|seen.contains(key));self.errors.retain(|key,_|seen.contains(key));self.cached.retain(|key,_|seen.contains(key));
    }
    /// Precision and the raycast configuration must come from the live owner.
    /// This generates task records, not completed mesh SH/visibility results.
    /// Preserves observed registration order and compacts retired entries at
    /// refresh completion. Native scene callback timing remains unverified.
    pub fn records(&mut self,precision:Precision,raycasts:bool)->Result<Vec<(Key,Arc<LightRecord>)>> {
        ensure!(self.errors.is_empty(),"Invalid PC scene light sources: {:?}",self.errors);
        let mut result=Vec::with_capacity(self.sources.len());
        for entry in self.order.entries() {
            let key=entry.owner.expect("flushed scene source identity");
            let source=&self.sources[&key];
            if !self.cached.get(&key).is_some_and(|(p,r,_)|*p==precision&&*r==raycasts) {
                let Some(cache)=source.cache(raycasts,precision)? else {self.cached.remove(&key);continue;};
                self.cached.insert(key,(precision,raycasts,Arc::new(cache.to_record())));
            }
            result.push((key,self.cached[&key].2.clone()));
        }
        Ok(result)
    }
    pub fn summary(&self)->serde_json::Value {
        serde_json::json!({"sources":self.sources.len(),"cached_records":self.cached.len(),
            "errors":self.errors.iter().map(|(key,error)|serde_json::json!({"owner":key.0,"component":key.1,"error":error})).collect::<Vec<_>>(),
            "scope":"Resolved live PC SH sources; per-mesh probe ownership, visibility queries and shader publication remain separate"})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn source()->Source {
        Source {generate_runtime:true,raycast:Some(false),subtractive:false,translation:[0.;3],rotation:[1.,0.,0.,0.,1.,0.,0.,0.,1.],kind:super::super::d3d9_light::Kind::Ambient,dimmer:1.,ambient:[0.1;3],diffuse:[0.;3],minimum_range:0.,maximum_range:0.,attenuation:[0.;3],inner_angle:0.,outer_angle:0.,exponent:0.}
    }
    #[test]
    fn updates_reuse_unchanged_records_and_retire_invalid_or_removed_sources()->Result<()> {
        let mut registry=Registry::default();registry.refresh([((1,2),Ok(Some(source())))]);
        let first=registry.records(Precision::Extended,false)?[0].1.clone();
        registry.refresh([((1,2),Ok(Some(source())))]);
        assert!(Arc::ptr_eq(&first,&registry.records(Precision::Extended,false)?[0].1));
        let mut moved=source();moved.translation=[3.,4.,5.];registry.refresh([((1,2),Ok(Some(moved)))]);
        let second=registry.records(Precision::Extended,false)?[0].1.clone();assert_eq!(second.position,[3.,4.,5.]);assert!(!Arc::ptr_eq(&first,&second));
        registry.refresh([((1,2),Err(anyhow::anyhow!("Malformed light")))]);assert!(registry.records(Precision::Extended,false).is_err());assert!(registry.cached.is_empty());
        registry.refresh([]);assert!(registry.records(Precision::Extended,false)?.is_empty());assert!(registry.errors.is_empty());Ok(())
    }
    #[test]
    fn registration_order_survives_updates_and_compacts_removals()->Result<()> {
        let mut registry=Registry::default();
        registry.refresh([((9,0),Ok(Some(source()))),((1,0),Ok(Some(source())))]);
        let keys=|r:&mut Registry|->Result<Vec<Key>> {Ok(r.records(Precision::Extended,false)?.into_iter().map(|(k,_)|k).collect())};
        assert_eq!(keys(&mut registry)?,vec![(9,0),(1,0)]);
        registry.refresh([((1,0),Ok(Some(source()))),((9,0),Ok(Some(source()))),((5,0),Ok(Some(source())))]);
        assert_eq!(keys(&mut registry)?,vec![(9,0),(1,0),(5,0)]);
        registry.refresh([((5,0),Ok(Some(source()))),((1,0),Ok(Some(source())))]);
        assert_eq!(keys(&mut registry)?,vec![(1,0),(5,0)]);Ok(())
    }
    #[test]
    fn scoped_sources_preserve_scene_zero_and_duplicate_errors()->Result<()> {
        let mut registry=Registry::default();
        registry.refresh_scoped([((0,2),Some(0),"light".into(),Ok(Some(source())))]);
        assert_eq!(registry.order.entries()[0].scene,Some(0));
        registry.refresh([((0,2),Ok(Some(source()))),((0,2),Ok(Some(source()))),((0,2),Ok(Some(source())))]);
        assert!(registry.records(Precision::Extended,false).is_err());
        assert!(registry.order.entries().is_empty());
        registry.refresh_scoped([((0,2),Some(1),"new".into(),Ok(Some(source())))]);
        assert_eq!(registry.order.entries()[0].scene,Some(1));Ok(())
    }
    #[test]
    fn signed_zero_changes_invalidate_cached_native_records()->Result<()> {
        let mut registry=Registry::default();registry.refresh([((1,2),Ok(Some(source())))]);
        let first=registry.records(Precision::Extended,false)?[0].1.clone();
        let mut changed=source();changed.translation[0]=-0.;registry.refresh([((1,2),Ok(Some(changed)))]);
        let second=registry.records(Precision::Extended,false)?[0].1.clone();
        assert!(!Arc::ptr_eq(&first,&second));assert_eq!(second.position[0].to_bits(),(-0f32).to_bits());Ok(())
    }
    #[test]
    fn disabled_and_duplicate_sources_never_retain_old_light_records()->Result<()> {
        let mut registry=Registry::default();registry.refresh([((1,2),Ok(Some(source())))]);registry.records(Precision::Single,true)?;
        registry.refresh([((1,2),Ok(None))]);assert!(registry.records(Precision::Single,true)?.is_empty());
        registry.refresh([((1,2),Ok(Some(source()))),((1,2),Ok(Some(source())))]);assert!(registry.records(Precision::Single,true).is_err());assert!(registry.sources.is_empty());Ok(())
    }
}
