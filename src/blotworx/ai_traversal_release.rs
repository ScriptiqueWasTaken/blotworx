//! Base traversal lease clearing4838b0. This is not instance destruction or
//! the complete Astar reset471d68. Budget boolean rejection is ignored.
use anyhow::{Context as _,Result};
use super::ai_traversal_instances::Instance;

pub trait Host {
    /// Includes context4685dc and the actual index/owner budget call.
    fn budget_mode0(&mut self,instance:&mut Instance)->Result<u32>;
    fn budget_mode1(&mut self,instance:&mut Instance)->Result<u32>;
}
impl Instance {
    pub fn clear_lease(&mut self,host:&mut impl Host)->Result<()> {
        let tag=self.lease.as_ref().context("Traversal release lease fields absent")?.field_20
            .context("Traversal release mode uninitialized")?;
        match tag {0=>{let _=host.budget_mode0(self)?;},1=>{let _=host.budget_mode1(self)?;},_=>{}}
        let lease=self.lease.as_mut().context("Budget release removed traversal lease fields")?;
        lease.owner_key=0;lease.field_20=Some(2);lease.reuse_count=0;lease.clock_bits=0;
        Ok(())
    }
}
