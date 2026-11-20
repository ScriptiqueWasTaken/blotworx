//! PCf385d0/f38990. Unlike the EM1 predicate, PC casts use projection bounds
//! and do not discard a contact merely because its reported fraction is zero.
use anyhow::{ensure,Result};
use super::pc_float::Precision;
pub(super) const FILTER:u32=0x18009;
#[derive(Clone,Copy,Debug)]
pub(super) struct Hit {pub body:usize,pub key:u32,pub point:[f32;3],pub fraction:f32}
pub(super) trait Backend {
    fn cast(&mut self,camera:[f32;3],target:[f32;3],radius:f32,filter:u32)->Result<Vec<Hit>>;
    fn overlap(&mut self,camera:[f32;3],radius:f32,filter:u32)->Result<Vec<Hit>>;
    fn should_ignore(&mut self,hit:Hit)->Result<bool>;
    fn is_thinned(&mut self,hit:Hit)->Result<bool>;
}
/// A returned fraction is the first blocking contact; None is a clear cast.
/// Metadata reads are ordered and skipped only by the corresponding native
/// flags. Backends must supply collector order and native key/filter identity.
pub(super) fn visible(camera:[f32;3],target:[f32;3],radius:f32,bypass_thinned:bool,bypass_ignore:bool,precision:Precision,backend:&mut impl Backend)->Result<Option<f32>> {
    ensure!(camera.iter().chain(&target).chain([&radius]).all(|v|v.is_finite())&&radius>=0.,"Invalid PC camera visibility cast");
    let direction=std::array::from_fn(|i|target[i]-camera[i]);
    for hit in backend.cast(camera,target,radius,FILTER)? {
        let from_camera=std::array::from_fn(|i|hit.point[i]-camera[i]);
        if super::pc_float::dot_stored(direction,from_camera,precision)?<0. {continue;}
        let from_target=std::array::from_fn(|i|hit.point[i]-target[i]);
        if super::pc_float::dot_stored(direction,from_target,precision)?>0. {continue;}
        if !bypass_ignore&&backend.should_ignore(hit)? {continue;}
        if !bypass_thinned&&backend.is_thinned(hit)? {continue;}
        ensure!(hit.fraction.is_finite(),"Nonfinite PC blocking fraction");return Ok(Some(hit.fraction));
    }Ok(None)
}
/// Preserve the actual f38ab6 branch, even though its ShouldIgnore polarity
/// differs from visibility casts. This is not an opportunity to fix the game.
pub(super) fn endpoint_clear(camera:[f32;3],radius:f32,backend:&mut impl Backend)->Result<bool> {
    ensure!(camera.iter().chain([&radius]).all(|v|v.is_finite())&&radius>=0.,"Invalid PC camera endpoint overlap");
    for hit in backend.overlap(camera,radius,FILTER)? {
        if backend.should_ignore(hit)?&&!backend.is_thinned(hit)? {return Ok(false);}
    }Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Host {hits:Vec<Hit>,ignore:bool,thinned:bool,reads:Vec<&'static str>}
    impl Backend for Host {
        fn cast(&mut self,_:[f32;3],_:[f32;3],_:f32,f:u32)->Result<Vec<Hit>>{assert_eq!(f,FILTER);Ok(self.hits.clone())}
        fn overlap(&mut self,_:[f32;3],_:f32,f:u32)->Result<Vec<Hit>>{assert_eq!(f,FILTER);Ok(self.hits.clone())}
        fn should_ignore(&mut self,_:Hit)->Result<bool>{self.reads.push("ignore");Ok(self.ignore)}
        fn is_thinned(&mut self,_:Hit)->Result<bool>{self.reads.push("thinned");Ok(self.thinned)}
    }
    fn hit(x:f32,fraction:f32)->Hit {Hit {body:7,key:13,point:[x,0.,0.],fraction}}
    #[test]
    fn pc_segment_projection_keeps_boundary_and_zero_fraction_contacts()->Result<()> {
        let mut h=Host {hits:vec![hit(-1.,0.1),hit(3.,0.2),hit(0.,0.)],ignore:false,thinned:false,reads:vec![]};
        assert_eq!(visible([0.;3],[2.,0.,0.],0.1,false,false,Precision::Extended,&mut h)?,Some(0.));
        assert_eq!(h.reads,["ignore","thinned"]);Ok(())
    }
    #[test]
    fn pc_metadata_short_circuit_and_endpoint_polarity_are_preserved()->Result<()> {
        let mut h=Host {hits:vec![hit(1.,0.5)],ignore:true,thinned:false,reads:vec![]};
        assert_eq!(visible([0.;3],[2.,0.,0.],0.1,false,false,Precision::Single,&mut h)?,None);assert_eq!(h.reads,["ignore"]);
        h.reads.clear();assert!(!endpoint_clear([1.,0.,0.],0.1,&mut h)?);assert_eq!(h.reads,["ignore","thinned"]);
        h.ignore=false;h.reads.clear();assert!(endpoint_clear([1.,0.,0.],0.1,&mut h)?);assert_eq!(h.reads,["ignore"]);
        h.reads.clear();assert_eq!(visible([0.;3],[2.,0.,0.],0.1,true,true,Precision::Double,&mut h)?,Some(0.5));assert!(h.reads.is_empty());Ok(())
    }
}
