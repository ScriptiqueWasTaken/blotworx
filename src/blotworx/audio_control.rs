//! CAkActionMute global parameter-node dispatch: DOL80351650/803517d8,
//! with subclass calls8034fc90/8034fd60. Scheduling/mixing are separate.
use super::audio_bank::Action;
use anyhow::Result;

pub trait ParameterNodeControl {
    type Handle: Copy;
    fn acquire_node(&mut self, id: u32) -> Result<Option<Self::Handle>>;
    fn mute_global(&mut self, node: Self::Handle, curve: u8, transition: i32) -> Result<()>;
    fn unmute_global(&mut self, node: Self::Handle, curve: u8, transition: i32) -> Result<()>;
    fn release_node(&mut self, node: Self::Handle);
}
#[derive(Debug, PartialEq)]
pub enum Delivery {
    MissingTarget,
    Applied,
}

/// Execute only after the audio scheduler admits the action's loaded delay.
/// Transition is passed in native integer units; no guessed seconds conversion
/// or fade approximation is performed here. A mixer must implement the sink.
pub fn execute_due(
    action: &Action<'_>,
    sink: &mut impl ParameterNodeControl,
    random: impl FnMut() -> u16,
) -> Result<Delivery> {
    let parameters = action.global_mute()?;
    let Some(node) = sink.acquire_node(action.target)? else {
        return Ok(Delivery::MissingTarget);
    };
    // Native transition RNG is consumed after successful target acquisition.
    let result = (|| {
        let time = parameters.transition_time(random)?;
        if action.kind == 0x6010 {
            sink.mute_global(node, parameters.curve, time)?;
        } else {
            sink.unmute_global(node, parameters.curve, time)?;
        }
        Ok(Delivery::Applied)
    })();
    sink.release_node(node);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{ensure, Context};
    #[derive(Default)]
    struct Sink {
        calls: Vec<(u32, bool, u8, i32)>,
        acquired: Vec<u32>,
        released: Vec<u32>,
        missing: bool,
        fail: bool,
    }
    impl ParameterNodeControl for Sink {
        type Handle = u32;
        fn acquire_node(&mut self, id: u32) -> Result<Option<u32>> {
            if self.missing {
                Ok(None)
            } else {
                self.acquired.push(id);
                Ok(Some(id))
            }
        }
        fn mute_global(&mut self, id: u32, curve: u8, time: i32) -> Result<()> {
            self.calls.push((id, true, curve, time));
            ensure!(!self.fail, "Diagnostic sink failure");
            Ok(())
        }
        fn unmute_global(&mut self, id: u32, curve: u8, time: i32) -> Result<()> {
            self.calls.push((id, false, curve, time));
            Ok(())
        }
        fn release_node(&mut self, id: u32) {
            self.released.push(id);
        }
    }
    fn payload() -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [500u32, 0, 0] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes.push(0xe4);
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes
    }
    #[test]
    fn native_node_dispatch_masks_curve_releases_targets_and_preserves_missing_rng() -> Result<()> {
        let bytes = payload();
        let mut action = Action {
            kind: 0x6010,
            target: 7,
            delay: [0; 3],
            subsection_size: 33,
            parameters: &bytes,
        };
        let mut sink = Sink::default();
        assert_eq!(
            execute_due(&action, &mut sink, || panic!(
                "Equal transition modifiers must not consume RNG"
            ))?,
            Delivery::Applied
        );
        action.kind = 0x7010;
        execute_due(&action, &mut sink, || 0)?;
        assert_eq!(sink.calls, [(7, true, 4, 500), (7, false, 4, 500)]);
        assert_eq!(sink.released, [7, 7]);
        sink.fail = true;
        action.kind = 0x6010;
        assert!(execute_due(&action, &mut sink, || 0).is_err());
        assert_eq!(sink.released, [7, 7, 7]);
        sink.missing = true;
        assert_eq!(
            execute_due(&action, &mut sink, || panic!("Missing target consumed RNG"))?,
            Delivery::MissingTarget
        );
        action.kind = 0x6011;
        assert!(
            execute_due(&action, &mut sink, || 0).is_err(),
            "Object-scoped mute must not become global node mute"
        );
        for length in 0..bytes.len() {
            action.kind = 0x6010;
            action.parameters = &bytes[..length];
            assert!(action.global_mute().is_err());
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_guardian_mixer_events_resolve_native_transitions_and_targets() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let bytes = std::fs::read(std::path::Path::new(&root).join("Audio/Wii/Global.bnk"))?;
        let bank = super::super::audio_bank::Bank::parse(&bytes)?;
        let mut sink = Sink::default();
        for (name, kind, delay, curve, muting) in [
            ("Mute_Guardians", 0x6010, 0, 4, true),
            ("Unmute_Guardians", 0x7010, 64000, 7, false),
        ] {
            let ids = bank.named_event_actions(name)?;
            assert_eq!(ids.len(), 2);
            for id in ids {
                let action = bank.objects[&id].action()?;
                assert_eq!(action.kind, kind);
                assert_eq!(action.loaded_delay(), [delay, 0, 0]);
                assert_eq!(
                    bank.objects[&action.target].kind, 7,
                    "Expected an actor-mixer target"
                );
                let parsed = action.global_mute()?;
                assert_eq!(parsed.transition, [500, 0, 0]);
                assert!(parsed.exceptions.is_empty());
                assert_eq!(
                    execute_due(&action, &mut sink, || panic!("Unexpected transition RNG"))?,
                    Delivery::Applied
                );
                assert_eq!(
                    *sink.calls.last().unwrap(),
                    (action.target, muting, curve, 500)
                );
            }
        }
        assert_eq!(sink.acquired, sink.released);
        assert_eq!(sink.calls.len(), 4);
        Ok(())
    }
}
