//! Retail DeathReward dynamic selection (REL764b8..77490).
//! Pending amounts mirror the native eight category lists; no cap on objects.
use std::collections::BTreeMap;
#[derive(Default)]
pub struct State {
    pub cursors: BTreeMap<usize, usize>,
    pub pending: [f32; 6],
    references: [Vec<usize>; 8],
}
// Constructor75ba8..75c24: amount, then mask of health/paint/thinner/anvil/watch/TV.
const AMOUNTS: [f32; 8] = [3., 1., 10., 10., 10., 1., 1., 1.];
const MASKS: [u32; 8] = [1, 1, 6, 2, 4, 32, 8, 16];
impl State {
    pub fn reference_counts(&self)->[usize;8] {std::array::from_fn(|index|self.references[index].len())}
    pub fn reserve(&mut self, category: usize, entity: usize) {
        self.references[category].push(entity);
        for channel in 0..6 {
            if MASKS[category] & (1 << channel) != 0 { self.pending[channel] += AMOUNTS[category]; }
        }
    }
    /// Native7750c iterates category order, each reference list backwards.
    pub fn purge(&mut self, valid: impl Fn(usize) -> bool) {
        for category in 0..8 {
            for index in (0..self.references[category].len()).rev() {
                if valid(self.references[category][index]) { continue; }
                self.references[category].remove(index);
                for channel in 0..6 {
                    if MASKS[category] & (1 << channel) != 0 { self.pending[channel] -= AMOUNTS[category]; }
                }
            }
        }
    }
}
/// Helper76818: pending health is truncated before signed integer addition.
pub fn health(current: i32, maximum: i32, pending: f32) -> Option<usize> {
    let current = current.wrapping_add(pending as i32);
    if current >= maximum { None } else if current <= maximum.wrapping_sub(3) { Some(0) } else { Some(1) }
}
/// Helper76bcc. Shared-ammo policy is not queried by the original selector.
pub fn ammo(amounts: [f32; 2], maxima: [f32; 2], pending: [f32; 2], draw: impl FnOnce() -> u32) -> Option<usize> {
    let values = [amounts[0] + pending[0], amounts[1] + pending[1]];
    if values[0] >= maxima[0] && values[1] >= maxima[1] { return None; }
    let paint = if values[0] > values[1] { false } else if values[0] == values[1] {
        (draw() as f32 / 32767.) > 0.5
    } else { true };
    Some(if paint { 3 } else { 4 })
}
/// Helper76e9c uses one draw, strict comparisons, and separate / then *.
pub fn ticket(draw: u32, ten: f32, thirty: f32) -> &'static str {
    let percentage = (draw as f32 / 32767.) * 100.;
    if percentage < thirty { "Palette_ThirtyETicket" }
    else if percentage < ten { "Palette_TenETicket" }
    else { "Palette_ETicket" }
}
