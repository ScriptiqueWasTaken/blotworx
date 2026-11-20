//! Standard concrete graph dispatch for the goto cost binding. Policy/refresh
//! services stay on the caller's owner; graph lookup never delegates to stubs.
use super::{
    ai_astar_graph_services::GraphContext, ai_goto_cost::Args, ai_goto_cost_binding::Backend,
    ai_goto_gap::State, ai_graph_connection, ai_graph_connection_binding as binding,
    ai_graph_link_passes::Memory, ai_graph_queries::Point,
    ai_pathfinder_constructor::State as Parent,
};
use anyhow::Result;
use std::collections::BTreeMap;
pub struct NativeGraph<'a, B> {
    pub backend: &'a mut B,
    pub context: ai_graph_connection::Context,
}
impl<B: Backend> Backend for NativeGraph<'_, B> {
    fn connection_index(
        &mut self,
        _: &mut State,
        g: &mut GraphContext<'_, Memory>,
        h: usize,
        target: Option<Point>,
    ) -> Result<u32> {
        binding::connection_index(g, self.context, h, target)
    }
    fn traversal(
        &mut self,
        _: &mut State,
        g: &mut GraphContext<'_, Memory>,
        h: usize,
    ) -> Result<Option<usize>> {
        binding::traversal(g, h)
    }
    fn service_cost(
        &mut self,
        s: &mut State,
        p: &mut BTreeMap<usize, Parent>,
        g: &mut GraphContext<'_, Memory>,
        service: usize,
        args: Args,
    ) -> Result<u32> {
        self.backend.service_cost(s, p, g, service, args)
    }
    fn external_acceptance_cost(
        &mut self,
        s: &mut State,
        p: &mut BTreeMap<usize, Parent>,
        g: &mut GraphContext<'_, Memory>,
        args: Args,
    ) -> Result<u32> {
        self.backend.external_acceptance_cost(s, p, g, args)
    }
    fn refresh_gap(
        &mut self,
        s: &mut State,
        p: &mut BTreeMap<usize, Parent>,
        g: &mut GraphContext<'_, Memory>,
    ) -> Result<()> {
        self.backend.refresh_gap(s, p, g)
    }
}
