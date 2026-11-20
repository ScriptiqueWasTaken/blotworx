//!1973c4 shares196274 with Goto, then initializes its distinct90..a4 fields.
use anyhow::Result;
use super::ai_traversal_instances::{Concrete,Instance};
#[derive(Debug,serde::Serialize)]
pub struct State {
    pub base:super::ai_goto_traversal::State,
    pub scalar_90:f32,
    pub direction_94:[f32;3],
    pub scalar_a0:f32,
    pub scalar_a4:f32,
}
pub fn create(h:&mut impl super::ai_goto_traversal::AllocationHost)->Result<Option<Instance>> {
    Ok(h.allocate(0xa8)?.map(construct))
}
pub fn construct(handle:usize)->Instance {
    super::ai_traversal_instances::construct_astar(handle,Concrete::Wander(State{
        base:super::ai_goto_traversal::construct_fields(),scalar_90:f32::MAX,
        direction_94:[0.;3],scalar_a0:0.25,scalar_a4:10.,
    }))
}
