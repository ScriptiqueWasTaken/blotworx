//! Particle random inputs. Native80017940 returns15 bits; box emitter
//! 804cd044..804cd070 divides that integer by32767 with fdivs.
pub(super) enum Stream<'a> {
    Preview { seed: u32 },
    Game(&'a mut super::game_random::State),
}
fn unit(raw: u32) -> f32 {
    raw as f32 / 32767.
}
impl Stream<'_> {
    pub fn unit(&mut self) -> f32 {
        match self {
            Self::Preview { seed } => {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (*seed >> 8) as f32 / 16777216.
            }
            Self::Game(state) => unit(state.draw()),
        }
    }
    pub fn raw15(&mut self) -> u32 {
        match self {
            Self::Game(state) => state.draw(),
            Self::Preview { .. } => (self.unit() * 32768.) as u32,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn random_and_unit_match_original_instructions() -> anyhow::Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/particle_random.json"
        ))?;
        assert_eq!(data["random"].as_array().unwrap().len(), 20);
        for case in data["random"].as_array().unwrap() {
            let mut state = super::super::game_random::State {
                seed: case["seed"].as_u64().unwrap() as u32,
                draws: 0,
            };
            let raw = Stream::Game(&mut state).raw15();
            assert_eq!(raw, case["raw"].as_u64().unwrap() as u32);
            assert_eq!(state.seed, case["next"].as_u64().unwrap() as u32);
            assert_eq!(
                unit(raw).to_bits(),
                case["unit_bits"].as_u64().unwrap() as u32
            );
        }
        for case in data["units"].as_array().unwrap() {
            assert_eq!(
                unit(case["raw"].as_u64().unwrap() as u32).to_bits(),
                case["unit_bits"].as_u64().unwrap() as u32
            );
        }
        Ok(())
    }
    #[test]
    fn shared_game_stream_preserves_native_raw_and_unit_domains() {
        let mut state = super::super::game_random::State::default();
        assert_eq!(Stream::Game(&mut state).raw15(), 16838);
        assert_eq!(Stream::Game(&mut state).unit(), 5758f32 / 32767.);
        assert_eq!(
            state.draw(),
            10113,
            "Particles must leave the same shared stream for other consumers"
        );
        assert_eq!(state.draws, 3);
        let mut endpoint = super::super::game_random::State {
            seed: 0x9112a683,
            draws: 0,
        };
        assert_eq!(Stream::Game(&mut endpoint).unit(), 1.);
    }
}
