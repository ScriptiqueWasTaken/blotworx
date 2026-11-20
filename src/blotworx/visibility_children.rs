//! REL297844: own component hooks, ordered children, then entity hidden flag.
//! Child snapshots are byte35, distinct from the current hidden byte34.
use anyhow::{ensure, Result};
use std::collections::HashSet;

pub trait Backend {
    fn local_hidden(&mut self, entity: usize, hidden: bool) -> Result<()>;
    fn children(&mut self, entity: usize) -> Result<Vec<usize>>;
    fn hidden(&self, entity: usize) -> bool;
    fn previous_hidden(&self, entity: usize) -> bool;
    fn snapshot_hidden(&mut self, entity: usize);
    fn commit_hidden(&mut self, entity: usize, hidden: bool);
}
enum Action {
    Enter(usize),
    Child(usize),
    Commit(usize),
}
pub fn set(backend: &mut impl Backend, entity: usize, hidden: bool) -> Result<()> {
    let mut pending = vec![Action::Enter(entity)];
    let mut ancestors = HashSet::new();
    while let Some(action) = pending.pop() {
        match action {
            Action::Enter(owner) => {
                ensure!(
                    ancestors.insert(owner),
                    "Cycle in visibility child hierarchy"
                );
                backend.local_hidden(owner, hidden)?;
                let children = backend.children(owner)?;
                pending.push(Action::Commit(owner));
                pending.extend(children.into_iter().rev().map(Action::Child));
            }
            Action::Child(child) => {
                if backend.hidden(child) == hidden {
                    continue;
                }
                if hidden {
                    backend.snapshot_hidden(child);
                } else if backend.previous_hidden(child) {
                    continue;
                }
                pending.push(Action::Enter(child));
            }
            Action::Commit(owner) => {
                backend.commit_hidden(owner, hidden);
                ancestors.remove(&owner);
            }
        }
    }
    Ok(())
}

#[cfg(any(test, debug_assertions))]
pub fn validate_instructions() -> Result<()> {
    struct Trace {
        children: Vec<Vec<usize>>,
        current: Vec<bool>,
        previous: Vec<bool>,
        calls: Vec<serde_json::Value>,
    }
    impl Backend for Trace {
        fn local_hidden(&mut self, id: usize, hidden: bool) -> Result<()> {
            self.calls
                .push(serde_json::json!(["local", id, usize::from(hidden)]));
            Ok(())
        }
        fn children(&mut self, id: usize) -> Result<Vec<usize>> {
            Ok(self.children[id].clone())
        }
        fn hidden(&self, id: usize) -> bool {
            self.current[id]
        }
        fn previous_hidden(&self, id: usize) -> bool {
            self.previous[id]
        }
        fn snapshot_hidden(&mut self, id: usize) {
            self.calls.push(serde_json::json!(["snapshot", id]));
            self.previous[id] = self.current[id];
        }
        fn commit_hidden(&mut self, id: usize, hidden: bool) {
            self.calls
                .push(serde_json::json!(["commit", id, usize::from(hidden)]));
            self.current[id] = hidden;
        }
    }
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/visibility_children.json"
    ))?;
    let cases = fixture["cases"].as_array().unwrap();
    ensure!(
        cases.len() == 128,
        "Visibility source fixture coverage changed"
    );
    for case in cases {
        let bits = |key: &str| {
            case[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() != 0)
                .collect()
        };
        let mut trace = Trace {
            children: serde_json::from_value(case["children"].clone())?,
            current: bits("current"),
            previous: bits("previous"),
            calls: Vec::new(),
        };
        set(&mut trace, 0, case["hidden"].as_bool().unwrap())?;
        let after = trace
            .current
            .iter()
            .zip(&trace.previous)
            .map(|(&current, &previous)| [usize::from(current), usize::from(previous)])
            .collect::<Vec<_>>();
        ensure!(
            serde_json::json!(after) == case["after"]
                && serde_json::json!(trace.calls) == case["trace"],
            "Visibility source state/order mismatch: {case}"
        );
    }
    println!("PASS: 128 original child visibility cases: snapshots, unhide veto and postorder flag commits");
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn original_child_visibility_state_and_order() -> anyhow::Result<()> {
        super::validate_instructions()
    }
}
