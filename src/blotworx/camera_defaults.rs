//! Default-profile mutation protocol, EM1 REL e4810..e49f0, and profile
//! aliases e5178..e5280. Numeric text formatting, property ownership, profile
//! transitions and target notifications belong to the real camera backend.
use anyhow::Result;

/// Offsets in the native profile's configuration-property pointer array.
/// Constructor1ddd04 names the keys MaxDist, PlacementPitch and LookPitch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    MaxDistance,
    PlacementPitch,
    LookPitch,
}
/// Native enabled/name slots +160/+164, +161/+168, +162/+16c.
/// Original-name slots are Default+150, Combat+158, Travel+154.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Default,
    Combat,
    Travel,
}
impl Kind {
    pub fn index(self) -> usize {
        match self {
            Self::Default => 0,
            Self::Combat => 1,
            Self::Travel => 2,
        }
    }
    pub fn mode(self) -> i32 {
        match self {
            Self::Default => 1,
            Self::Combat => 3,
            Self::Travel => 2,
        }
    }
}
pub trait Backend {
    fn enable_custom(&mut self) -> Result<()>; // controller+160=1
    fn custom_profile(&mut self) -> Result<usize>; //+164 ->1e0eb8
    /// Native %1% formatting followed by ConfigProperty::set(14af40).
    /// The backend must preserve its parse/rounding and callback behavior.
    fn set_formatted_float(&mut self, profile: usize, property: Property, value: f32)
        -> Result<()>;
    fn controller_mode(&mut self) -> Result<i32>; //+15c, read after all writes
    fn selected_name(&mut self) -> Result<String>; //+150
    /// virtual+5c=e5178, including aliases, transition and target callbacks.
    fn reapply_profile(&mut self, name: &str, mode: i32, duration: f32) -> Result<()>;
}
/// Lua10bb8 passes arguments1/2/3 as f1/f2/f3. The helper writes them to
/// profile+04/+1c/+18 respectively. Native failures keep earlier side effects.
pub fn set(values: [f32; 3], backend: &mut impl Backend) -> Result<()> {
    set_kind(Kind::Default, values, backend)
}
/// REL e4810, e4a70 and e4cd0 differ in profile/enable slots and mode gate.
/// The backend selects the corresponding slots before this shared protocol.
pub fn set_kind(kind: Kind, values: [f32; 3], backend: &mut impl Backend) -> Result<()> {
    backend.enable_custom()?;
    let profile = backend.custom_profile()?;
    for (property, value) in [
        Property::MaxDistance,
        Property::PlacementPitch,
        Property::LookPitch,
    ]
    .into_iter()
    .zip(values)
    {
        backend.set_formatted_float(profile, property, value)?;
    }
    if backend.controller_mode()? == kind.mode() {
        let name = backend.selected_name()?;
        // REL4ac490 is float0.5, not an immediate/cut transition.
        backend.reapply_profile(&name, 1, 0.5)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub struct Alias<'a> {
    pub original: &'a str,
    pub custom: &'a str,
    pub enabled: bool,
}
/// Input order is native Default(+150), Combat(+158), Travel(+154).
/// Names represent interned strings; exact text equality preserves identity.
/// A disabled matching alias does not prevent a later enabled alias matching.
pub fn resolve_alias<'a>(requested: &'a str, aliases: [Alias<'a>; 3]) -> &'a str {
    for alias in aliases {
        if requested == alias.original && alias.enabled {
            return alias.custom;
        }
    }
    requested
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Trace {
        calls: Vec<String>,
        writes: Vec<(Property, u32)>,
        mode: i32,
        fail: Option<Property>,
    }
    impl Backend for Trace {
        fn enable_custom(&mut self) -> Result<()> {
            self.calls.push("enable".into());
            Ok(())
        }
        fn custom_profile(&mut self) -> Result<usize> {
            self.calls.push("lookup CustomDefault".into());
            Ok(7)
        }
        fn set_formatted_float(&mut self, p: usize, property: Property, v: f32) -> Result<()> {
            assert_eq!(p, 7);
            self.calls.push(format!("write {property:?}"));
            if self.fail == Some(property) {
                anyhow::bail!("Property backend rejected write")
            }
            self.writes.push((property, v.to_bits()));
            Ok(())
        }
        fn controller_mode(&mut self) -> Result<i32> {
            self.calls.push("mode".into());
            Ok(self.mode)
        }
        fn selected_name(&mut self) -> Result<String> {
            self.calls.push("selected".into());
            Ok("Default".into())
        }
        fn reapply_profile(&mut self, name: &str, mode: i32, duration: f32) -> Result<()> {
            assert_eq!((name, mode, duration.to_bits()), ("Default", 1, 0x3f000000));
            self.calls.push("apply".into());
            Ok(())
        }
    }
    fn trace(mode: i32) -> Trace {
        Trace {
            calls: vec![],
            writes: vec![],
            mode,
            fail: None,
        }
    }
    #[test]
    fn writes_native_slot_order_then_reapplies_with_half_second_transition() -> Result<()> {
        let mut t = trace(1);
        set([6.25, 18., -10.], &mut t)?;
        assert_eq!(
            t.calls,
            [
                "enable",
                "lookup CustomDefault",
                "write MaxDistance",
                "write PlacementPitch",
                "write LookPitch",
                "mode",
                "selected",
                "apply"
            ]
        );
        assert_eq!(
            t.writes,
            [
                (Property::MaxDistance, 6.25_f32.to_bits()),
                (Property::PlacementPitch, 18_f32.to_bits()),
                (Property::LookPitch, (-10_f32).to_bits())
            ]
        );
        Ok(())
    }
    #[test]
    fn other_modes_write_defaults_without_selecting_or_reapplying() -> Result<()> {
        for mode in [-1, 0, 2, 3] {
            let mut t = trace(mode);
            set([1., 2., 3.], &mut t)?;
            assert_eq!(t.calls.last().unwrap(), "mode");
            assert_eq!(t.writes.len(), 3);
        }
        Ok(())
    }
    #[test]
    fn property_failure_preserves_prefix_and_does_not_run_later_callbacks() {
        let mut t = trace(1);
        t.fail = Some(Property::PlacementPitch);
        assert!(set([1., 2., 3.], &mut t).is_err());
        assert_eq!(
            t.calls,
            [
                "enable",
                "lookup CustomDefault",
                "write MaxDistance",
                "write PlacementPitch"
            ]
        );
        assert_eq!(t.writes, [(Property::MaxDistance, 1_f32.to_bits())]);
    }
    #[test]
    fn aliases_preserve_native_priority_and_disabled_fallthrough() {
        let mut aliases = [
            Alias {
                original: "Shared",
                custom: "CustomDefault",
                enabled: false,
            },
            Alias {
                original: "Shared",
                custom: "CustomCombat",
                enabled: true,
            },
            Alias {
                original: "Shared",
                custom: "CustomTravel",
                enabled: true,
            },
        ];
        assert_eq!(resolve_alias("Shared", aliases), "CustomCombat");
        aliases[0].enabled = true;
        assert_eq!(resolve_alias("Shared", aliases), "CustomDefault");
        assert_eq!(resolve_alias("shared", aliases), "shared");
        assert_eq!(resolve_alias("Other", aliases), "Other");
    }
}
