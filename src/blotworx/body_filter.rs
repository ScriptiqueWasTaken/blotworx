//! Common rigid-body filter packing, REL37ec70/37edf0.
//! World refresh and component property/lifecycle binding remain required.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fields {
    pub layer: u32,
    pub group: u32,
    pub sub_a: u32,
    pub sub_b: u32,
    pub painted: u8,
    pub real: u8,
}
/// Native shifts/ORs don't mask input fields. Real forces painted=1, but
/// otherwise both raw byte values survive rather than being bool-normalized.
pub fn pack(fields: Fields) -> u32 {
    let painted = if fields.real != 0 { 1 } else { fields.painted };
    fields.layer
        | (fields.group << 17)
        | (fields.sub_a << 5)
        | (fields.sub_b << 10)
        | ((painted as u32) << 15)
        | ((fields.real as u32) << 16)
}
/// REL37edf0 retains extracted group/subgroup bits while replacing layer and
/// current Painted/Real flags. Do not carry the old packed flags forward.
pub fn change_layer(previous: u32, layer: u32, painted: u8, real: u8) -> u32 {
    pack(Fields {
        layer,
        group: previous >> 17,
        sub_a: (previous >> 5) & 31,
        sub_b: (previous >> 10) & 31,
        painted,
        real,
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refresh {
    pub world: usize,
    pub body: usize,
    pub mode: u8,
    pub collections: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterFilter {
    pub group: u32,
    pub filter_info: u32,
}
pub trait CharacterProperties {
    /// Virtual+278 writes stack+14, initialized0; return low byte controls
    /// native initialization-flag read. Getters may allocate other shared IDs.
    fn layer(
        &mut self,
        destination: &mut u32,
        groups: &mut super::physics_filter::GroupCounter,
    ) -> anyhow::Result<u32>;
    fn initialization_flags(&mut self) -> anyhow::Result<u8>;
    fn real(&mut self, groups: &mut super::physics_filter::GroupCounter) -> anyhow::Result<u8>;
    fn painted(&mut self, groups: &mut super::physics_filter::GroupCounter) -> anyhow::Result<u8>;
}
/// Character Cinfo REL379cc4..379ddc: reserve/publish ID before layer getter,
/// Real then Painted, fixed subgroupA1. Do not reread group after callbacks.
/// Errors retain counter/getter effects and stop; no allocation rollback.
pub fn character(
    groups: &mut super::physics_filter::GroupCounter,
    properties: &mut impl CharacterProperties,
) -> anyhow::Result<CharacterFilter> {
    let group = groups.reserve();
    let mut layer = 0;
    if properties.layer(&mut layer, groups)? as u8 == 0 {
        // Native tests initialization bit3 but proceeds either way in this
        // executable. Preserve the field read without inventing rejection.
        let _flags = properties.initialization_flags()?;
    }
    let real = properties.real(groups)?;
    let painted = properties.painted(groups)?;
    Ok(CharacterFilter {
        group,
        filter_info: pack(Fields {
            layer,
            group,
            sub_a: 1,
            sub_b: 0,
            painted,
            real,
        }),
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Deferred,
    Immediate,
}
pub trait WorldRefresh {
    fn critical_locks(&mut self, world: usize) -> anyhow::Result<u32>;
    /// Original queue append/counter update precedes body retain.
    fn enqueue(&mut self, operation: Refresh) -> anyhow::Result<()>;
    fn retain(&mut self, body: usize) -> anyhow::Result<()>;
    fn current_world(&mut self, body: usize) -> anyhow::Result<Option<usize>>;
    /// Full unlocked802dacd0 body/agent/collection refresh, not a no-op.
    fn immediate(
        &mut self,
        world: usize,
        body: usize,
        mode: u32,
        collections: u32,
    ) -> anyhow::Result<()>;
    fn release(&mut self, body: usize) -> anyhow::Result<()>;
}
/// DOL802dacd0 admission checks only world+94, unlike phantom update's sum.
pub fn refresh(
    world: usize,
    body: usize,
    mode: u32,
    collections: u32,
    backend: &mut impl WorldRefresh,
) -> anyhow::Result<Admission> {
    if backend.critical_locks(world)? != 0 {
        backend.enqueue(Refresh {
            world,
            body,
            mode: mode as u8,
            collections: collections as u8,
        })?;
        backend.retain(body)?;
        Ok(Admission::Deferred)
    } else {
        backend.immediate(world, body, mode, collections)?;
        Ok(Admission::Immediate)
    }
}
impl Refresh {
    /// Type12 dispatch802d2bbc: skip moved/detached body, then release. Backend
    /// failure stops before release; poisoned queue keeps operation for diagnosis.
    pub fn execute(&self, backend: &mut impl WorldRefresh) -> anyhow::Result<()> {
        if backend.current_world(self.body)? == Some(self.world) {
            refresh(
                self.world,
                self.body,
                self.mode as u32,
                self.collections as u32,
                backend,
            )?;
        }
        backend.release(self.body)
    }
}
pub trait Backend {
    fn system(&mut self) -> anyhow::Result<Option<usize>>;
    fn body_count(&mut self, system: usize) -> anyhow::Result<usize>;
    fn body(&mut self, system: usize, index: usize) -> anyhow::Result<usize>;
    fn filter(&mut self, body: usize) -> anyhow::Result<u32>;
    fn write(&mut self, body: usize, value: u32) -> anyhow::Result<()>;
    fn world(&mut self, body: usize) -> anyhow::Result<Option<usize>>;
    fn refresh(
        &mut self,
        world: usize,
        body: usize,
        mode: u32,
        collections: u32,
    ) -> anyhow::Result<()>;
    fn real(&mut self) -> anyhow::Result<u8>;
    fn painted(&mut self) -> anyhow::Result<u8>;
}
fn next(index: usize, backend: &mut impl Backend) -> anyhow::Result<Option<usize>> {
    use anyhow::Context;
    let system = backend
        .system()?
        .context("Rigid-body system disappeared during filter update")?;
    let count = backend.body_count(system)?;
    anyhow::ensure!(
        count <= i32::MAX as usize,
        "Native body count outside signed32 range"
    );
    if index >= count {
        return Ok(None);
    }
    let body = backend.body(system, index)?;
    anyhow::ensure!(body != 0, "Null rigid body during filter update");
    Ok(Some(body))
}
/// REL37ec70. Writes precede required world refresh; backend failure leaves
/// earlier writes intact. System/count/body are reread across callbacks.
pub fn set_all(fields: Fields, backend: &mut impl Backend) -> anyhow::Result<u32> {
    if backend.system()?.is_none() {
        return Ok(0);
    }
    let value = pack(fields);
    let mut index = 0;
    while let Some(body) = next(index, backend)? {
        if backend.filter(body)? != value {
            backend.write(body, value)?;
            if let Some(world) = backend.world(body)? {
                backend.refresh(world, body, 0, 1)?;
            }
        }
        index += 1;
    }
    Ok(value)
}
/// REL37edf0. Unlike set_all, unchanged words still write/refresh. Flags are
/// fetched Real then Painted for each visited body. Return first old category.
pub fn set_layer(layer: u32, backend: &mut impl Backend) -> anyhow::Result<i32> {
    if backend.system()?.is_none() {
        return Ok(-1);
    }
    let mut previous_category = -1;
    let mut index = 0;
    while let Some(body) = next(index, backend)? {
        let previous = backend.filter(body)?;
        if previous_category == -1 {
            previous_category = (previous & 31) as i32;
        }
        let real = backend.real()?;
        let painted = backend.painted()?;
        backend.write(body, change_layer(previous, layer, painted, real))?;
        if let Some(world) = backend.world(body)? {
            backend.refresh(world, body, 0, 1)?;
        }
        index += 1;
    }
    Ok(previous_category)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn character_filter_retains_reserved_group_getter_order_and_subgroup_one() -> anyhow::Result<()>
    {
        struct Properties {
            calls: Vec<&'static str>,
            result: u32,
            write: bool,
            fail: bool,
        }
        impl CharacterProperties for Properties {
            fn layer(
                &mut self,
                p: &mut u32,
                g: &mut super::super::physics_filter::GroupCounter,
            ) -> anyhow::Result<u32> {
                assert_eq!(*p, 0);
                assert_eq!(g.value, 1);
                self.calls.push("layer");
                assert_eq!(g.reserve(), 2);
                if self.write {
                    *p = 1
                };
                if self.fail {
                    anyhow::bail!("Layer backend failed")
                };
                Ok(self.result)
            }
            fn initialization_flags(&mut self) -> anyhow::Result<u8> {
                self.calls.push("flags");
                Ok(8)
            }
            fn real(
                &mut self,
                g: &mut super::super::physics_filter::GroupCounter,
            ) -> anyhow::Result<u8> {
                assert_eq!(g.value, 2);
                self.calls.push("real");
                Ok(1)
            }
            fn painted(
                &mut self,
                _: &mut super::super::physics_filter::GroupCounter,
            ) -> anyhow::Result<u8> {
                self.calls.push("painted");
                Ok(0)
            }
        }
        for (result, write, expected, calls) in [
            (1, true, 0x38021, vec!["layer", "real", "painted"]),
            (
                0x100,
                false,
                0x38020,
                vec!["layer", "flags", "real", "painted"],
            ),
        ] {
            let mut groups = super::super::physics_filter::GroupCounter { value: 0 };
            let mut properties = Properties {
                calls: vec![],
                result,
                write,
                fail: false,
            };
            let prepared = character(&mut groups, &mut properties)?;
            assert_eq!(
                prepared,
                CharacterFilter {
                    group: 1,
                    filter_info: expected
                }
            );
            assert_eq!(groups.value, 2);
            assert_eq!(properties.calls, calls);
            assert_eq!(
                super::super::collidable_owner::EntityData::character(Some(71), prepared.group)
                    .word_0c,
                1
            );
        }
        let mut groups = super::super::physics_filter::GroupCounter { value: 0 };
        let mut properties = Properties {
            calls: vec![],
            result: 1,
            write: true,
            fail: true,
        };
        assert!(character(&mut groups, &mut properties).is_err());
        assert_eq!(groups.value, 2);
        assert_eq!(properties.calls, ["layer"]);
        Ok(())
    }
    #[test]
    fn world_refresh_preserves_deferred_bytes_membership_and_reference_order() -> anyhow::Result<()>
    {
        #[derive(Default)]
        struct World {
            locked: u32,
            current: Option<usize>,
            pending: Vec<Refresh>,
            calls: Vec<String>,
            fail: bool,
        }
        impl WorldRefresh for World {
            fn critical_locks(&mut self, _: usize) -> anyhow::Result<u32> {
                Ok(self.locked)
            }
            fn enqueue(&mut self, o: Refresh) -> anyhow::Result<()> {
                self.calls.push("enqueue".into());
                self.pending.push(o);
                Ok(())
            }
            fn retain(&mut self, b: usize) -> anyhow::Result<()> {
                assert_eq!(b, 8);
                self.calls.push("retain".into());
                Ok(())
            }
            fn current_world(&mut self, b: usize) -> anyhow::Result<Option<usize>> {
                assert_eq!(b, 8);
                self.calls.push("membership".into());
                Ok(self.current)
            }
            fn immediate(&mut self, w: usize, b: usize, m: u32, c: u32) -> anyhow::Result<()> {
                assert_eq!((w, b), (3, 8));
                self.calls.push(format!("refresh {m} {c}"));
                if self.fail {
                    anyhow::bail!("Agents unavailable")
                };
                Ok(())
            }
            fn release(&mut self, b: usize) -> anyhow::Result<()> {
                assert_eq!(b, 8);
                self.calls.push("release".into());
                Ok(())
            }
        }
        let mut world = World {
            locked: 1,
            current: Some(3),
            ..Default::default()
        };
        assert_eq!(
            refresh(3, 8, 0x100, 0x101, &mut world)?,
            Admission::Deferred
        );
        assert_eq!(world.calls, vec!["enqueue", "retain"]);
        let operation = world.pending[0];
        assert_eq!((operation.mode, operation.collections), (0, 1));
        world.calls.clear();
        world.locked = 0;
        operation.execute(&mut world)?;
        assert_eq!(world.calls, vec!["membership", "refresh 0 1", "release"]);
        world.calls.clear();
        world.locked = 1;
        operation.execute(&mut world)?;
        assert_eq!(
            world.calls,
            vec!["membership", "enqueue", "retain", "release"]
        );
        assert_eq!(world.pending.len(), 2);
        world.locked = 0;
        world.calls.clear();
        world.current = Some(4);
        operation.execute(&mut world)?;
        assert_eq!(world.calls, vec!["membership", "release"]);
        world.calls.clear();
        assert_eq!(
            refresh(3, 8, 0x100, 0x101, &mut world)?,
            Admission::Immediate
        );
        assert_eq!(world.calls, vec!["refresh 256 257"]);
        world.calls.clear();
        world.current = Some(3);
        world.fail = true;
        assert!(operation.execute(&mut world).is_err());
        assert_eq!(world.calls, vec!["membership", "refresh 0 1"]);
        Ok(())
    }
    struct Trace {
        present: bool,
        bodies: Vec<(usize, u32, Option<usize>)>,
        calls: Vec<String>,
        fail: bool,
        grow: bool,
    }
    impl Backend for Trace {
        fn system(&mut self) -> anyhow::Result<Option<usize>> {
            Ok(self.present.then_some(1))
        }
        fn body_count(&mut self, _: usize) -> anyhow::Result<usize> {
            Ok(self.bodies.len())
        }
        fn body(&mut self, _: usize, i: usize) -> anyhow::Result<usize> {
            Ok(self.bodies[i].0)
        }
        fn filter(&mut self, b: usize) -> anyhow::Result<u32> {
            Ok(self.bodies.iter().find(|v| v.0 == b).unwrap().1)
        }
        fn write(&mut self, b: usize, v: u32) -> anyhow::Result<()> {
            self.calls.push(format!("write {b}"));
            self.bodies.iter_mut().find(|r| r.0 == b).unwrap().1 = v;
            Ok(())
        }
        fn world(&mut self, b: usize) -> anyhow::Result<Option<usize>> {
            self.calls.push(format!("world {b}"));
            Ok(self.bodies.iter().find(|r| r.0 == b).unwrap().2)
        }
        fn refresh(&mut self, w: usize, b: usize, m: u32, c: u32) -> anyhow::Result<()> {
            assert_eq!((w, m, c), (3, 0, 1));
            self.calls.push(format!("refresh {b}"));
            if self.fail {
                anyhow::bail!("World refresh failed")
            };
            if self.grow {
                self.grow = false;
                self.bodies.push((40, 0, None));
            }
            Ok(())
        }
        fn real(&mut self) -> anyhow::Result<u8> {
            self.calls.push("real".into());
            Ok(1)
        }
        fn painted(&mut self) -> anyhow::Result<u8> {
            self.calls.push("painted".into());
            Ok(0)
        }
    }
    #[test]
    fn native_setters_keep_distinct_refresh_rules_and_live_body_list() -> anyhow::Result<()> {
        let fields = Fields {
            layer: 1,
            group: 0,
            sub_a: 0,
            sub_b: 0,
            painted: 0,
            real: 1,
        };
        let mut trace = Trace {
            present: true,
            bodies: vec![(10, 0x18001, Some(3)), (20, 0, None), (30, 0, Some(3))],
            calls: vec![],
            fail: false,
            grow: true,
        };
        assert_eq!(set_all(fields, &mut trace)?, 0x18001);
        assert_eq!(
            trace.calls,
            vec![
                "write 20",
                "world 20",
                "write 30",
                "world 30",
                "refresh 30",
                "write 40",
                "world 40"
            ]
        );
        trace.calls.clear();
        assert_eq!(set_layer(1, &mut trace)?, 1);
        assert_eq!(
            &trace.calls[..5],
            ["real", "painted", "write 10", "world 10", "refresh 10"]
        );
        assert!(trace.calls.iter().any(|c| c == "refresh 30"));
        trace.calls.clear();
        trace.fail = true;
        assert!(set_all(Fields { layer: 2, ..fields }, &mut trace).is_err());
        assert_eq!(trace.bodies[0].1, 0x18002);
        assert_eq!(trace.bodies[1].1, 0x18001);
        assert_eq!(trace.calls, vec!["write 10", "world 10", "refresh 10"]);
        trace.present = false;
        assert_eq!(set_all(fields, &mut trace)?, 0);
        assert_eq!(set_layer(2, &mut trace)?, -1);
        trace.present = true;
        trace.bodies.clear();
        assert_eq!(set_all(fields, &mut trace)?, 0x18001);
        assert_eq!(set_layer(2, &mut trace)?, -1);
        Ok(())
    }
    #[test]
    fn native_packing_preserves_full_fields_and_real_forces_painted() {
        let fields = Fields {
            layer: 1,
            group: 0,
            sub_a: 0,
            sub_b: 0,
            painted: 0,
            real: 1,
        };
        assert_eq!(pack(fields), 0x18001);
        assert_eq!(pack(Fields { real: 0, ..fields }), 1);
        assert_eq!(
            pack(Fields {
                painted: 1,
                real: 0,
                ..fields
            }),
            0x8001
        );
        assert_eq!(
            pack(Fields {
                layer: u32::MAX,
                ..fields
            }),
            u32::MAX
        );
        assert_eq!(
            pack(Fields {
                layer: 0,
                group: 0x8000,
                sub_a: 0x100,
                sub_b: 0,
                painted: 2,
                real: 0
            }),
            0x12000
        );
        assert_eq!(
            pack(Fields {
                layer: 0,
                group: 0,
                sub_a: 0,
                sub_b: 0,
                painted: 0xff,
                real: 2
            }),
            0x28000
        );
    }
    #[test]
    fn layer_change_retains_group_subgroups_and_replaces_flags() {
        let previous = pack(Fields {
            layer: 3,
            group: 0x1234,
            sub_a: 7,
            sub_b: 9,
            painted: 1,
            real: 1,
        });
        let next = change_layer(previous, 2, 0, 0);
        assert_eq!(next, (0x1234 << 17) | (7 << 5) | (9 << 10) | 2);
        assert_eq!(change_layer(previous, 0x80000002, 0, 1), next | 0x80018000);
    }
}
