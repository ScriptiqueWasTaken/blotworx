//! Compose Goto refresh with the native spatial-query implementation using
//! one borrowed provider and its existing storage. No profile results invented.
use super::{
    ai_entity_profile_query as profile, ai_goto_cones,
    ai_goto_gap::State,
    ai_goto_refresh::{self as refresh, Attribute},
};
use anyhow::Result;
pub struct NativeProfiles<'a, H> {
    pub host: &'a mut H,
    pub context: profile::Context,
}
impl<H: refresh::Host + profile::Host> ai_goto_cones::Host for NativeProfiles<'_, H> {
    fn torso(&mut self, s: &mut State, a: usize) -> Result<Option<[f32; 3]>> {
        self.host.torso(s, a)
    }
    fn forward(&mut self, s: &mut State, a: usize) -> Result<[f32; 3]> {
        self.host.forward(s, a)
    }
    fn position(&mut self, s: &mut State, a: usize) -> Result<[f32; 3]> {
        self.host.position(s, a)
    }
    fn up(&mut self, s: &mut State) -> Result<[f32; 3]> {
        self.host.up(s)
    }
    fn zero(&mut self, s: &mut State) -> Result<[f32; 3]> {
        self.host.zero(s)
    }
}
impl<H: refresh::Host + profile::Host> refresh::Host for NativeProfiles<'_, H> {
    fn time(&mut self, s: &mut State) -> Result<f32> {
        refresh::Host::time(self.host, s)
    }
    fn force_refresh(&mut self, s: &mut State, p: usize) -> Result<u8> {
        self.host.force_refresh(s, p)
    }
    fn unit_scale(&mut self, s: &mut State) -> Result<Option<f32>> {
        self.host.unit_scale(s)
    }
    fn scalar(&mut self, s: &mut State, a: usize) -> Result<f32> {
        self.host.scalar(s, a)
    }
    fn attribute(&mut self, s: &mut State, a: usize, k: Attribute) -> Result<Option<usize>> {
        self.host.attribute(s, a, k)
    }
    fn predicate(&mut self, s: &mut State, c: usize, a: usize, b: usize) -> Result<u32> {
        self.host.predicate(s, c, a, b)
    }
    fn profile_first(
        &mut self,
        _: &mut State,
        m: usize,
        p: [f32; 3],
        r: f32,
    ) -> Result<Option<usize>> {
        Ok(profile::first(self.host, self.context, u32::try_from(m)?, p, r)?.map(|a| a as usize))
    }
    fn profile_next(&mut self, _: &mut State, m: usize) -> Result<Option<usize>> {
        Ok(profile::next(self.host, self.context, u32::try_from(m)?)?.map(|a| a as usize))
    }
    fn actors_first(&mut self, s: &mut State) -> Result<Option<usize>> {
        self.host.actors_first(s)
    }
    fn actor_at(&mut self, s: &mut State, n: usize) -> Result<usize> {
        self.host.actor_at(s, n)
    }
    fn actors_next(&mut self, s: &mut State, n: usize) -> Result<Option<usize>> {
        self.host.actors_next(s, n)
    }
    fn sphere(&mut self, s: &mut State, a: usize, p: [f32; 3], d: f32) -> Result<()> {
        self.host.sphere(s, a, p, d)
    }
    fn rectangle(&mut self, s: &mut State, a: usize, p: [f32; 3], d: f32, l: f32) -> Result<()> {
        self.host.rectangle(s, a, p, d, l)
    }
    // Preserve the caller's concrete shape virtuals and canonical actor provider.
}
