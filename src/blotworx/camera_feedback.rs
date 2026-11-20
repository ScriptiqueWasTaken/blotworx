//! Ordinary kind9 branch of the native camera receiver, RELdb790..db83c.
//! Other camera event kinds and the ledge orientation controller remain open.
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
#[derive(Default, serde::Serialize)]
pub struct State {
    // Constructor d199c/d1a88..90 explicitly clears these two native bytes.
    pub airborne: bool,
    pub spin_follow_active: bool,
    pub received: u64,
    pub last_id: Option<u32>,
    pub last_flag: Option<u8>,
    pub follow_requests: u64,
    pub injected: u64,
    pub last_injected_id: Option<u32>,
}
pub trait Effects {
    fn spin_enabled(&mut self) -> Result<bool>;
    fn follow_tracker(&mut self) -> Result<()>;
    fn ledge_transition(&mut self) -> Result<()>;
}
impl State {
    /// Generator d97bc..d97e0 only writes when its current target has a tracker.
    pub fn observe_tracker(&mut self, eligible: Option<bool>) {
        if let Some(eligible) = eligible {
            self.airborne = !eligible;
            if eligible {
                self.spin_follow_active = false;
            }
        }
    }
    pub fn receive(&mut self, id: u32, flag: u8, effects: &mut impl Effects) -> Result<()> {
        self.received += 1;
        self.last_id = Some(id);
        self.last_flag = Some(flag);
        if flag == 0 {
            self.injected += 1;
            self.last_injected_id = Some(id);
        }
        if id == 0x55 && self.airborne && effects.spin_enabled()? {
            // db7e4 precedes the null-tracker check/call. Do not roll back on error.
            self.spin_follow_active = true;
            effects.follow_tracker()?;
            self.follow_requests += 1;
        }
        if id == 0x19 {
            effects.ledge_transition()?;
        }
        Ok(())
    }
}
pub fn spin_enabled(settings: &BTreeMap<String, String>) -> Result<bool> {
    // db7cc registers this property with the literal default "false".
    let Some(text) = settings.get("FreeCamera_FollowJump_Spin") else {
        return Ok(false);
    };
    match text.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => {
            let value = text
                .parse::<f64>()
                .context("Unsupported camera Spin boolean syntax")?;
            ensure!(
                value.is_finite()
                    && value.trunc() >= i32::MIN as f64
                    && value.trunc() <= i32::MAX as f64,
                "Invalid camera Spin boolean"
            );
            Ok(value.trunc() as i32 != 0)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Host {
        enabled: bool,
        reads: u32,
        follows: u32,
        ledge: u32,
        fail: bool,
    }
    impl Effects for Host {
        fn spin_enabled(&mut self) -> Result<bool> {
            self.reads += 1;
            Ok(self.enabled)
        }
        fn follow_tracker(&mut self) -> Result<()> {
            self.follows += 1;
            if self.fail {
                anyhow::bail!("Missing tracker")
            };
            Ok(())
        }
        fn ledge_transition(&mut self) -> Result<()> {
            self.ledge += 1;
            anyhow::bail!("Ledge output unavailable")
        }
    }
    #[test]
    fn ordinary_receiver_preserves_native_gates_and_error_writes() -> Result<()> {
        let mut state = State::default();
        let mut host = Host {
            enabled: true,
            ..Default::default()
        };
        state.receive(85, 1, &mut host)?;
        assert_eq!(host.reads, 0);
        state.observe_tracker(Some(false));
        state.receive(84, 1, &mut host)?;
        assert_eq!(host.reads, 0);
        host.enabled = false;
        state.receive(85, 1, &mut host)?;
        assert_eq!(host.follows, 0);
        host.enabled = true;
        state.receive(85, 1, &mut host)?;
        assert!(state.spin_follow_active);
        assert_eq!(host.follows, 1);
        state.observe_tracker(None);
        assert!(state.airborne);
        state.observe_tracker(Some(true));
        assert!(!state.airborne && !state.spin_follow_active);
        state.observe_tracker(Some(false));
        host.fail = true;
        assert!(state.receive(85, 1, &mut host).is_err());
        assert!(state.spin_follow_active);
        assert!(state.receive(25, 1, &mut host).is_err());
        assert_eq!(host.ledge, 1);
        Ok(())
    }
    #[test]
    fn spin_configuration_keeps_retail_false_and_validates_only_supported_text() -> Result<()> {
        let mut settings = BTreeMap::new();
        assert!(!spin_enabled(&settings)?);
        for (text, expected) in [
            ("true", true),
            ("false", false),
            ("1", true),
            ("-2", true),
            ("0.9", false),
        ] {
            settings.insert("FreeCamera_FollowJump_Spin".into(), text.into());
            assert_eq!(spin_enabled(&settings)?, expected);
        }
        Ok(())
    }
}
