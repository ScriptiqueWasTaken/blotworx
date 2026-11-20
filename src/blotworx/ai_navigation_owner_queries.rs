//! Canonical owner-chain and direct attribute reads for borrowed pathfinders.
//! Brain/entity records remain in their owning maps throughout callbacks.
use std::collections::BTreeMap;
use anyhow::{Context,Result,ensure};
use super::{ai_pathfinder_constructor::State,ai_navigation_construction::Constructed,ai_navigation_entity::{Owned,Registry,Kind}};
pub fn entity(parent:&State,brains:&BTreeMap<usize,Constructed>)->Result<usize>{
 let brain=brains.get(&parent.base.owner).context("Navigation owner brain is absent")?;
 ensure!(brain.brain.identity==parent.base.owner,"Navigation brain identity differs");Ok(brain.base.owner.unwrap_or(0))
}
pub fn scalar(entity:usize,entities:&BTreeMap<usize,Owned>,registry:&Registry,kind:Kind)->Result<Option<f32>>{
 let owned=entities.get(&entity).context("Navigation owner entity is absent")?;
 ensure!(owned.entity.identity==entity,"Navigation entity identity differs");
 Ok(owned.optional_scalar(kind,registry)?.map(|s|f32::from_bits(s.value_bits)))
}
