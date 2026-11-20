//! Retail27e1b0..27e2fc random-dialogue selection. History belongs to the
//! cached event object, not an individual presentation/coroutine.
use anyhow::{Result, ensure};
#[derive(Clone, Debug)]
pub(super) struct State {
    pub last: i32,
    pub seen: Vec<bool>,
}
impl State {
    pub fn new(lines: usize) -> Self {
        Self {
            last: -1,
            seen: vec![false; lines],
        }
    }
    pub fn select(&mut self, mode: u16, mut random: impl FnMut() -> u32) -> Result<usize> {
        let count = self.seen.len();
        let mut candidates = Vec::new();
        match mode {
            0 => {
                let unseen = self.seen.iter().any(|seen| !*seen);
                if !unseen {
                    self.seen.fill(false);
                }
                for i in 0..count {
                    // The loop-condition loads total count before every
                    // iteration, including the first.
                    if self.last == i as i32 && count > 1 {
                        continue;
                    }
                    if !self.seen[i] {
                        candidates.push(i as u8);
                    }
                }
            }
            1 => {
                let next = self.last.wrapping_add(1);
                candidates.push(if next < count as i32 { next as u8 } else { 0 });
            }
            2 => {
                for i in 0..count {
                    if self.last != i as i32 || count <= 1 {
                        candidates.push(i as u8);
                    }
                }
            }
            _ => {}
        }
        // Retail calls C rand unconditionally, including sequential mode.
        let value = random();
        ensure!(
            !candidates.is_empty(),
            "Native dialogue selector has zero candidates (retail division/index behavior is unsafe)"
        );
        ensure!(
            candidates.len() <= 64,
            "Native dialogue candidate scratch would overflow its64-byte stack array; behavior is unverified"
        );
        let selected = candidates[value as usize % candidates.len()] as i8 as i32;
        ensure!(
            selected >= 0 && (selected as usize) < count,
            "Native dialogue selected an unsafe signed-byte line index"
        );
        self.last = selected;
        self.seen[selected as usize] = true;
        Ok(selected as usize)
    }
}
#[cfg(debug_assertions)]
pub(super) fn validate_reference() -> Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/dialogue_selection.json"
    ))?;
    let cases = fixture["cases"].as_array().unwrap();
    for case in cases {
        let mut state = State {
            last: case["last"].as_i64().unwrap() as i32,
            seen: serde_json::from_value(case["seen"].clone())?,
        };
        let mut draws = 0;
        let selected = state.select(case["mode"].as_u64().unwrap() as u16, || {
            draws += 1;
            case["random"].as_u64().unwrap() as u32
        })?;
        ensure!(
            selected as i64 == case["chosen"].as_i64().unwrap()
                && serde_json::json!(state.seen) == case["after"]
                && draws == case["draws"].as_u64().unwrap(),
            "Native dialogue case differs: {case}"
        );
    }
    for case in fixture["unsafe"].as_array().unwrap() {
        let mut state = State {
            last: case["last"].as_i64().unwrap() as i32,
            seen: serde_json::from_value(case["seen"].clone())?,
        };
        ensure!(
            state
                .select(case["mode"].as_u64().unwrap() as u16, || 0)
                .is_err(),
            "Unsafe native selector was silently accepted"
        );
    }
    println!(
        "PASS: {} original PPC dialogue-selection cases; native unsafe cases diagnosed",
        cases.len()
    );
    Ok(())
}
