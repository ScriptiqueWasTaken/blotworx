//! Concrete finalization stage for the existing native graph admission driver.
//! Listener delivery/clock and tagged-policy registration remain real services.
use super::{
    ai_graph_admission::{Admission, Listener},
    ai_graph_finalize::{self, Context, Host, Mesh},
    ai_graph_handles::State,
    ai_graph_link_passes::{Lifecycle, Memory},
    ai_graph_loading::Loader,
};
use anyhow::{ensure, Context as _, Result};
pub trait Events: Host {
    fn arrived(
        &mut self,
        m: &mut Memory,
        a: &mut Admission,
        l: &mut Loader,
        s: &mut State,
        r: Option<u32>,
        listener: Listener,
    ) -> Result<()>;
    fn clock_bits(
        &mut self,
        m: &mut Memory,
        a: &mut Admission,
        l: &mut Loader,
        s: &mut State,
    ) -> Result<u32>;
}
pub struct Native<C> {
    pub context: Context,
    pub callbacks: C,
}
impl<C: Events> Lifecycle for Native<C> {
    fn finalize(
        &mut self,
        m: &mut Memory,
        a: &mut Admission,
        l: &mut Loader,
        s: &mut State,
        r: Option<u32>,
    ) -> Result<()> {
        let resource = r.context("Cannot finalize a null graph resource")?;
        let result = ai_graph_finalize::finalize(
            m,
            a,
            l,
            s,
            &mut self.context,
            resource,
            &mut self.callbacks,
        )?;
        // The original kernel records a null mesh and continues after rejection.
        // Expose malformed requested geometry as failure before admission sets5.
        ensure!(
            !matches!(result, Mesh::Rejected(_)),
            "Required runtime navigation mesh was rejected: {result:?}"
        );
        Ok(())
    }
    fn arrived(
        &mut self,
        m: &mut Memory,
        a: &mut Admission,
        l: &mut Loader,
        s: &mut State,
        r: Option<u32>,
        v: Listener,
    ) -> Result<()> {
        self.callbacks.arrived(m, a, l, s, r, v)
    }
    fn clock_bits(
        &mut self,
        m: &mut Memory,
        a: &mut Admission,
        l: &mut Loader,
        s: &mut State,
    ) -> Result<u32> {
        self.callbacks.clock_bits(m, a, l, s)
    }
}
