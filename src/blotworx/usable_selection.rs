//! Nearest eligible use target. Eligibility and grab-height checks belong to
//! the caller; the authored radius is squared, without a positive-radius clamp.
pub fn nearest(position:[f32;3],candidates:impl IntoIterator<Item=(usize,[f32;3],f32)>)->Option<usize> {
    let mut selected=Selection::default();
    for (owner,target,radius) in candidates {
        selected.consider(position,owner,target,radius);
    }
    selected.owner()
}
#[derive(Default)]
pub struct Selection {best:Option<(usize,f32)>}
impl Selection {
    pub fn consider(&mut self,position:[f32;3],owner:usize,target:[f32;3],radius:f32) {
        let delta=std::array::from_fn::<_,3,_>(|i|position[i]-target[i]);
        let distance=(delta[0]*delta[0]+delta[1]*delta[1])+delta[2]*delta[2];
        if distance<=radius*radius && self.best.is_none_or(|(_,best)|distance<best) {
            self.best=Some((owner,distance));
        }
    }
    pub fn owner(&self)->Option<usize> {self.best.map(|(owner,_)|owner)}
}
