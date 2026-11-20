//! CAkActionPlay due execution, DOL8034eb60..8034ece0. Queue admission,
//! object registration, nested node playback and device output are separate.
use super::audio_bank::Action;
use anyhow::Result;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Context {
    /// Registered audio object handle, not a scene/entity serialization ID.
    pub object: u32,
    pub playing_id: u32,
    pub sample_offset: u32,
    /// Pending action+18/+1c/+20/+24, forwarded without reinterpretation.
    pub words: [u32; 4],
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Request {
    pub target: u32,
    pub context: Context,
    pub transition: i32,
    pub curve: u8,
    pub kind: u32,
    /// Initialized request words+28/+6c/+70/+78/+7c and bytes+80/+81.
    /// Uninitialized native stack padding is deliberately not represented.
    pub native_zero_words: [u32; 5],
    pub native_flags: [u8; 2],
}
pub trait Control {
    type Handle: Copy;
    fn acquire_play_node(&mut self, id: u32) -> Result<Option<Self::Handle>>;
    fn play_node(
        &mut self,
        node: Self::Handle,
        request: &Request,
        random: &mut dyn FnMut() -> u16,
    ) -> Result<u32>;
    fn release_play_node(&mut self, node: Self::Handle);
}
/// CAkActionPlay virtual+30,8034ed80: writes zero to its notification word.
/// Ordinary Play has no node lookup, RNG draw or other side effect here.
pub fn pending_notification() -> u32 {
    0
}
/// The native action returns the child status, or15 for a missing target. It
/// does not allocate a playing ID here or replace a missing node with a sound.
pub fn execute_due(
    action: &Action<'_>,
    context: Context,
    sink: &mut impl Control,
    mut random: impl FnMut() -> u16,
) -> Result<u32> {
    let play = action.play()?;
    let Some(node) = sink.acquire_play_node(action.target)? else {
        return Ok(15);
    };
    let result = (|| {
        let request = Request {
            target: action.target,
            context,
            transition: play.transition_time(&mut random)?,
            curve: play.fade_flags & 31,
            kind: 0,
            native_zero_words: [0; 5],
            native_flags: [0, 1],
        };
        sink.play_node(node, &request, &mut random)
    })();
    sink.release_play_node(node);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Host {
        present: bool,
        status: u32,
        references: u32,
        calls: Vec<serde_json::Value>,
        fail: bool,
    }
    impl Control for Host {
        type Handle = u32;
        fn acquire_play_node(&mut self, id: u32) -> Result<Option<u32>> {
            if self.present {
                self.references += 1;
                Ok(Some(id))
            } else {
                Ok(None)
            }
        }
        fn play_node(
            &mut self,
            node: u32,
            request: &Request,
            _: &mut dyn FnMut() -> u16,
        ) -> Result<u32> {
            assert_eq!(node, request.target);
            self.calls.push(serde_json::json!({"kind":"play","target":request.target,"request_kind":request.kind,
                "context":request.context,"transition":request.transition,"curve":request.curve,
                "native_zero_words":request.native_zero_words,"native_flags":request.native_flags}));
            anyhow::ensure!(!self.fail, "Unsupported test voice host");
            Ok(self.status)
        }
        fn release_play_node(&mut self, _: u32) {
            self.references -= 1;
            self.calls.push(serde_json::json!({"kind":"release"}));
        }
    }
    #[test]
    fn original_play_request_target_order_randomness_and_release_match() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_play_action.json"
        ))?;
        for case in fixture["cases"].as_array().unwrap() {
            let encoded = case["encoded"].as_str().unwrap();
            let bytes = (0..encoded.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let action = super::super::audio_bank::Object {
                kind: 3,
                payload: &bytes,
            }
            .action()?;
            let mut host = Host {
                present: case["present"].as_bool().unwrap(),
                status: case["status"].as_u64().unwrap() as u32,
                references: 3,
                calls: Vec::new(),
                fail: false,
            };
            let mut random = super::super::game_random::State::default();
            let status = execute_due(
                &action,
                serde_json::from_value(case["context"].clone())?,
                &mut host,
                || random.draw() as u16,
            )?;
            assert_eq!(case["status"], status);
            assert_eq!(case["calls"], serde_json::json!(host.calls));
            assert_eq!(case["draws"], random.draws);
            assert_eq!(case["seed"], random.seed);
            assert_eq!(host.references, 3);
            if host.present {
                host.fail = true;
                host.calls.clear();
                assert!(execute_due(&action, Context::default(), &mut host, || 0).is_err());
                assert_eq!(
                    host.references, 3,
                    "Voice-host errors must release the acquired target"
                );
            }
        }
        Ok(())
    }
}
