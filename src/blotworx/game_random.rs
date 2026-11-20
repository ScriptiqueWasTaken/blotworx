//! Retail C rand arithmetic, DOL80017940..8001795c. The shipped seed at
//! 806fc530 is 1 (r13=80704520, offset -7ff0). Full boot-time caller ordering
//! and all runtime consumers remain unverified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct State {
    pub seed: u32,
    pub draws: u64,
}
impl Default for State {
    fn default() -> Self {
        Self { seed: 1, draws: 0 }
    }
}
impl State {
    pub fn draw(&mut self) -> u32 {
        self.draws += 1;
        next(&mut self.seed)
    }
}
pub fn next(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(0x41c64e6d).wrapping_add(0x3039);
    (*seed >> 16) & 0x7fff
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retail_instruction_arithmetic_preserves_seed_and_fifteen_bit_outputs() {
        let mut seed = 1;
        assert_eq!(
            (0..6).map(|_| next(&mut seed)).collect::<Vec<_>>(),
            [16838, 5758, 10113, 17515, 31051, 5627]
        );
        let mut zero = 0;
        assert_eq!(next(&mut zero), 0);
        assert_eq!(zero, 0x3039);
        let mut maximal = u32::MAX;
        let output = next(&mut maximal);
        assert_eq!(maximal, 0xbe39e1cc);
        assert_eq!(output, 0x3e39);
    }
}
