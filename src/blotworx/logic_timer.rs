//! LogicTimer runtime, REL3601c0/3608a0/360ccc. Separate from the HUD timer.
#[derive(Clone, Debug, serde::Serialize)]
pub struct State {
    pub remaining: f32,
    pub high_next: bool,
    pub expirations: u64,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn freeze_and_reset_preserve_phase() {
        let mut s=State::default();s.reset(1.,None);
        assert_eq!(s.advance(2.,true),None);assert_eq!(s.remaining,1.);
        assert_eq!(s.advance(1.,false),Some(true));
        s.reset(1.,None);assert_eq!(s.advance(1.,false),Some(false));
        assert_eq!(s.expirations,2);
    }
    #[test]
    fn one_expiry_per_update_and_no_overshoot_carry() {
        let mut s=State::default();s.reset(0.1,None);
        assert_eq!(s.advance(10.,false),Some(true));assert_eq!(s.expirations,1);
        s.reset(0.1,None);assert_eq!(s.remaining,0.1);
    }
    #[test]
    fn random_endpoints_and_single_precision_fused_interpolation() {
        let mut s=State::default();s.reset(9.,Some((2.,6.,0)));assert_eq!(s.remaining,2.);
        s.reset(9.,Some((2.,6.,32767)));assert_eq!(s.remaining,6.);
        s.reset(9.,Some((6.,2.,32767)));assert_eq!(s.remaining,2.);
    }
    #[test]
    fn zero_interval_fires_on_each_update_but_nan_does_not() {
        let mut s=State::default();assert_eq!(s.advance(0.,false),Some(true));
        s.reset(0.,None);assert_eq!(s.advance(0.,false),Some(false));
        s.remaining=f32::NAN;assert_eq!(s.advance(1.,false),None);
    }
}
impl Default for State {
    fn default() -> Self { Self {remaining:0.,high_next:true,expirations:0} }
}
impl State {
    pub fn reset(&mut self, interval: f32, random: Option<(f32,f32,u32)>) {
        self.remaining=if let Some((min,max,draw))=random {
            (max-min).mul_add(draw as f32/32767.,min)
        } else {interval};
    }
    /// Exactly one expiry per update; discard overshoot on subsequent reset.
    /// Neither disabling nor reset changes the alternating high/low phase.
    pub fn advance(&mut self, delta:f32, disabled:bool)->Option<bool> {
        if disabled {return None}
        self.remaining-=delta;
        if !(self.remaining<=0.) {return None}
        let high=self.high_next;
        self.high_next=!self.high_next;
        self.expirations+=1;
        Some(high)
    }
}
