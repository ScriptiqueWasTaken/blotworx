//! Recursive child update3541d0. Explicit task stack retains depth-first
//! order without a host recursion cap; native child count is freshly read.
use anyhow::{ensure, Result};
use std::collections::BTreeSet;
pub trait Backend {
    fn child_count(&mut self, transform: usize) -> Result<u16>;
    fn child_handle(&mut self, transform: usize, index: usize) -> Result<u32>;
    fn resolve(&mut self, handle: u32) -> Result<Option<usize>>;
    fn transform(&mut self, entity: usize) -> Result<Option<usize>>;
    fn update_352010(&mut self, transform: usize) -> Result<()>;
    fn body_3b850(&mut self, entity: usize) -> Result<Option<usize>>;
    fn sync_body_37cd80(&mut self, body: usize, entity: usize) -> Result<()>;
    fn phantom_7abe0(&mut self, entity: usize) -> Result<Option<usize>>;
    fn position_virtual_bc(&mut self, entity: usize) -> Result<[f32; 3]>; //descriptor1119c,flag0
    fn sync_phantom_378420(&mut self, phantom: usize, position: [f32; 4]) -> Result<()>;
}
enum Task {
    Walk { transform: usize, index: usize },
    Finish { entity: usize },
}
pub fn update(root: usize, backend: &mut impl Backend) -> Result<()> {
    let mut tasks = vec![Task::Walk {
        transform: root,
        index: 0,
    }];
    let mut active = BTreeSet::from([root]);
    while let Some(task) = tasks.pop() {
        match task {
            Task::Walk { transform, index } => {
                if index >= usize::from(backend.child_count(transform)?) {
                    active.remove(&transform);
                    continue;
                }
                let handle = backend.child_handle(transform, index)?;
                tasks.try_reserve(3)?;
                tasks.push(Task::Walk {
                    transform,
                    index: index + 1,
                });
                if handle == 0 {
                    continue;
                }
                let Some(entity) = backend.resolve(handle)? else {
                    continue;
                };
                let Some(child) = backend.transform(entity)? else {
                    continue;
                };
                backend.update_352010(child)?;
                ensure!(
                    active.insert(child),
                    "Cyclic transform descendant hierarchy"
                );
                tasks.push(Task::Finish { entity });
                tasks.push(Task::Walk {
                    transform: child,
                    index: 0,
                });
            }
            Task::Finish { entity } => {
                if let Some(body) = backend.body_3b850(entity)? {
                    backend.sync_body_37cd80(body, entity)?;
                } else if let Some(phantom) = backend.phantom_7abe0(entity)? {
                    let position = backend.position_virtual_bc(entity)?;
                    backend.sync_phantom_378420(
                        phantom,
                        super::native_hk_transform::entity_position_vector(position),
                    )?;
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    struct State {
        children: BTreeMap<usize, Vec<u32>>,
        calls: Vec<String>,
    }
    impl Backend for State {
        fn child_count(&mut self, t: usize) -> Result<u16> {
            self.calls.push(format!("count {t}"));
            Ok(self.children.get(&t).map_or(0, |v| v.len() as u16))
        }
        fn child_handle(&mut self, t: usize, i: usize) -> Result<u32> {
            self.calls.push(format!("handle {t} {i}"));
            Ok(self.children[&t][i])
        }
        fn resolve(&mut self, h: u32) -> Result<Option<usize>> {
            self.calls.push(format!("resolve {h}"));
            Ok((h != 7).then_some(h as usize * 10))
        }
        fn transform(&mut self, e: usize) -> Result<Option<usize>> {
            self.calls.push(format!("transform {e}"));
            Ok(match e {
                10 => Some(101),
                20 => Some(202),
                _ => None,
            })
        }
        fn update_352010(&mut self, t: usize) -> Result<()> {
            self.calls.push(format!("update {t}"));
            if t == 101 {
                self.children.get_mut(&100).unwrap().push(3);
            }
            Ok(())
        }
        fn body_3b850(&mut self, e: usize) -> Result<Option<usize>> {
            self.calls.push(format!("body {e}"));
            Ok((e == 10).then_some(350))
        }
        fn sync_body_37cd80(&mut self, b: usize, e: usize) -> Result<()> {
            assert_eq!((b, e), (350, 10));
            self.calls.push("sync body".into());
            Ok(())
        }
        fn phantom_7abe0(&mut self, e: usize) -> Result<Option<usize>> {
            assert_eq!(e, 20);
            self.calls.push("phantom".into());
            Ok(Some(420))
        }
        fn position_virtual_bc(&mut self, e: usize) -> Result<[f32; 3]> {
            assert_eq!(e, 20);
            self.calls.push("position".into());
            Ok([1., 2., 3.])
        }
        fn sync_phantom_378420(&mut self, p: usize, v: [f32; 4]) -> Result<()> {
            assert_eq!(p, 420);
            assert_eq!(v, [1., 2., 3., 0.]);
            assert_eq!(v[3].to_bits(), 0);
            self.calls.push("sync phantom".into());
            Ok(())
        }
    }
    #[test]
    fn native_depth_first_sync_order_live_count_and_missing_transform_skip() -> Result<()> {
        let mut s = State {
            children: BTreeMap::from([(100, vec![1, 0, 7]), (101, vec![2])]),
            calls: vec![],
        };
        update(100, &mut s)?;
        let effects: Vec<&str> = s
            .calls
            .iter()
            .filter(|c| !c.starts_with("count") && !c.starts_with("handle"))
            .map(String::as_str)
            .collect();
        assert_eq!(
            effects,
            [
                "resolve 1",
                "transform 10",
                "update 101",
                "resolve 2",
                "transform 20",
                "update 202",
                "body 20",
                "phantom",
                "position",
                "sync phantom",
                "body 10",
                "sync body",
                "resolve 7",
                "resolve 3",
                "transform 30"
            ]
        );
        assert_eq!(s.calls.last().unwrap(), "count 100");
        assert!(s.calls.contains(&"handle 100 3".into()));
        s.children.insert(101, vec![1]);
        s.calls.clear();
        assert!(update(100, &mut s)
            .unwrap_err()
            .to_string()
            .contains("Cyclic"));
        assert!(s.calls.contains(&"update 101".into()));
        assert!(!s.calls.contains(&"sync body".into()));
        Ok(())
    }
}
