//! Base ability ownership and Mickey's Give parser, reconstructed from EM1.
//! Config/save initialization and action/HUD dispatch remain separate systems.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Ownership {
    pub owned: [bool; 2],
    pub upgrades: [i32; 2],
    pub disabled: bool,
}
impl Ownership {
    /// Common8f4d0 stores the shared disable byte without changing ownership
    /// or upgrades. Blast a398c additionally observes Paint's effective edge.
    pub fn set_disabled(&mut self, disabled: bool) -> Option<bool> {
        let before = self.enabled(super::brush::Ink::Paint);
        self.disabled = disabled;
        let after = self.enabled(super::brush::Ink::Paint);
        (before != after).then_some(after)
    }
    /// REL8b578: ownership byte +25/+26 and shared disable byte +28.
    pub fn enabled(&self, ink: super::brush::Ink) -> bool {
        self.owned[usize::from(ink == super::brush::Ink::Thinner)] && !self.disabled
    }
    /// RELaf300 returns 2 for any unmatched selector, including All.
    /// RELaf470 enables both channels for 2; RELaf3d0 stores raw signed levels.
    pub fn give(&mut self, selector: &str, level: i32) {
        let selected = match selector {
            "Paint" => Some(0),
            "Thinner" => Some(1),
            _ => None,
        };
        for index in 0..2 {
            if selected.is_none_or(|selected| selected == index) {
                self.owned[index] = true;
                self.upgrades[index] = level;
            }
        }
    }
    /// REL8f1a0 invokes base ownership setter with All/false, retaining levels
    /// and the independent shared-disable byte.
    pub fn revoke_all(&mut self) {
        self.owned = [false; 2];
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Give<'a> {
    Ability {
        name: &'a str,
        selector: &'a str,
        level: i32,
    },
    Health {
        amount: i32,
    },
}

/// DOL8001c084 saturates at signed 32-bit limits, accepts a decimal prefix,
/// and returns zero if no digits are consumed. Default C whitespace only.
pub(super) fn decimal(token: &str) -> i32 {
    let bytes = token.as_bytes();
    let mut cursor = 0;
    while bytes
        .get(cursor)
        .is_some_and(|b| matches!(*b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
    {
        cursor += 1;
    }
    let negative = bytes.get(cursor) == Some(&b'-');
    if negative || bytes.get(cursor) == Some(&b'+') {
        cursor += 1;
    }
    let limit = if negative {
        2147483648_u64
    } else {
        2147483647_u64
    };
    let mut value = 0_u64;
    while let Some(&digit) = bytes.get(cursor).filter(|digit| digit.is_ascii_digit()) {
        value = (value * 10 + u64::from(digit - b'0')).min(limit);
        cursor += 1;
    }
    if negative {
        (-(value as i64)) as i32
    } else {
        value as i32
    }
}

/// REL8d134/8d204: strtok uses only ASCII space, and reads three operands.
/// Missing third operand defaults to one; explicit numeric zero also becomes
/// one. Two-operand Health is a distinct HealthComponent operation.
pub fn parse(command: &str) -> Option<Give<'_>> {
    let mut words = command
        .split('\0')
        .next()?
        .split(' ')
        .filter(|word| !word.is_empty());
    if words.next()? != "Give" {
        return None;
    }
    let name = words.next()?;
    let selector = words.next()?;
    match words.next() {
        Some(amount) => {
            let value = decimal(amount);
            let level = if value == 0 { 1 } else { value };
            Some(Give::Ability {
                name,
                selector,
                level,
            })
        }
        None if name == "Health" => Some(Give::Health {
            amount: decimal(selector),
        }),
        None => Some(Give::Ability {
            name,
            selector,
            level: 1,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blotworx::brush::Ink;
    #[test]
    fn give_preserves_native_selectors_levels_and_shared_disable() {
        let mut state = Ownership::default();
        assert!(!state.enabled(Ink::Paint));
        assert!(!state.enabled(Ink::Thinner));
        state.give("Thinner", 3);
        assert_eq!(state.upgrades, [0, 3]);
        assert_eq!(state.owned, [false, true]);
        state.give("Paint", -2);
        assert_eq!(state.upgrades, [-2, 3]);
        state.disabled = true;
        assert!(!state.enabled(Ink::Paint));
        assert!(!state.enabled(Ink::Thinner));
        state.give("unmatched", 7);
        assert_eq!(state.upgrades, [7, 7]);
        assert!(state.owned.iter().all(|owned| *owned));
        assert!(state.disabled);
    }
    #[test]
    fn give_parser_matches_original_tokenization_and_zero_coercion() {
        assert_eq!(
            parse("Give Brush All 0 ignored"),
            Some(Give::Ability {
                name: "Brush",
                selector: "All",
                level: 1
            })
        );
        assert_eq!(
            parse("  Give  Spin Paint -2tail"),
            Some(Give::Ability {
                name: "Spin",
                selector: "Paint",
                level: -2
            })
        );
        assert_eq!(
            parse("Give Blast Thinner"),
            Some(Give::Ability {
                name: "Blast",
                selector: "Thinner",
                level: 1
            })
        );
        assert_eq!(parse("Give Health 10"), Some(Give::Health { amount: 10 }));
        assert_eq!(
            parse("Give Brush\tAll 2"),
            Some(Give::Ability {
                name: "Brush\tAll",
                selector: "2",
                level: 1
            })
        );
        assert_eq!(parse("Give Brush"), None);
        assert_eq!(parse("give Brush All 1"), None);
        assert_eq!(parse("Give\0 Brush All 1"), None);
        assert_eq!(decimal("999999999999999999999"), i32::MAX);
        assert_eq!(decimal("-999999999999999999999"), i32::MIN);
        assert_eq!(decimal("\t+42tail"), 42);
        assert_eq!(decimal("garbage"), 0);
    }
}
