//! World filter selection, DOL802df6fc..802df77c; default801621b4.
use anyhow::Result;
/// Shared REL1d390 counter. Native callers include character construction and
/// entity/attachment group updates; never allocate a separate counter per body.
#[derive(Debug, PartialEq, Eq)]
pub struct GroupCounter {
    pub value: u32,
}
impl GroupCounter {
    /// PPC addi/stw publishes the full word before dependent callbacks. Packed
    /// group extraction later narrows bits; allocation itself does not clamp.
    pub fn reserve(&mut self) -> u32 {
        self.value = self.value.wrapping_add(1);
        self.value
    }
}

/// Game filter table+3c, constructor23bd28 initializes all32 rows to -1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    pub rows: [u32; 32],
}
#[derive(Clone, Debug)]
pub struct GameFilter {
    pub matrix: Matrix,
    pub mode_30: u8,
    pub mode_31: u8,
    pub flag_32: u8,
    pub flag_33: u8,
    pub flag_34: u8,
    pub excluded_group_38: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct QueryFlags {
    pub bytes: [u8; 5],
    pub excluded_group: u32,
}
impl Default for GameFilter {
    fn default() -> Self {
        Self {
            matrix: Default::default(),
            mode_30: 0,
            mode_31: 0,
            flag_32: 1,
            flag_33: 0,
            flag_34: 0,
            excluded_group_38: 0,
        }
    }
}
impl GameFilter {
    /// Constructor23bd28..23be20 resets the shared counter as well as its own
    /// fields. Default alone supplies field values, not this global side effect.
    pub fn construct(groups: &mut GroupCounter) -> Self {
        groups.value = 0;
        Self::default()
    }
    /// REL23c670..23c6e0 (also23c89c/23cab4). Active override copies bytes
    /// +30..34/group+38, invokes query, then clears only +30, not other fields.
    pub fn query<T>(
        &mut self,
        flags: Option<QueryFlags>,
        execute: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let Some(flags) = flags.filter(|f| f.bytes[0] != 0) else {
            return execute(self);
        };
        self.mode_30 = flags.bytes[0];
        self.mode_31 = flags.bytes[1];
        self.flag_32 = flags.bytes[2];
        self.flag_33 = flags.bytes[3];
        self.flag_34 = flags.bytes[4];
        self.excluded_group_38 = flags.excluded_group;
        let result = execute(self);
        self.mode_30 = 0;
        result
    }
    /// REL23c15c: resolve container side first, then inspect parent shapes.
    pub fn shape_container(
        &self,
        body: usize,
        other: ContainerInput,
        records: &mut impl ShapeRecords,
    ) -> Result<u32> {
        let second = container_filter(other, records)?;
        let mut current = body;
        let mut visited = std::collections::BTreeSet::new();
        loop {
            anyhow::ensure!(visited.insert(current), "Cycle in mixed-filter body chain");
            let Some(parent) = records.parent_body(current)? else {
                return Ok(self.collision_info(records.body_filter(current)?, second));
            };
            let shape = records.body_shape(parent)?;
            let flags = records.dispatch_flags(shape)?;
            let first = if flags & 0x80 != 0 {
                let key = records.body_key(current)?;
                Some(records.direct_child_filter(shape, key)?)
            } else if flags & 0x100 != 0 {
                let key = records.body_key(current)?;
                Some(records.temporary_child_filter(shape, key)?)
            } else if flags & 0x400000 != 0 {
                Some(0)
            } else if flags & 0x800000 != 0 {
                return Ok(0x01000000);
            } else {
                None
            };
            if let Some(first) = first {
                let first = if first == 0 {
                    root_filter(body, records)?
                } else {
                    first
                };
                return Ok(self.collision_info(first, second));
            }
            current = parent;
        }
    }
    /// REL23c0a0: child getter zero falls back to the root body's filter.
    pub fn container_pair(
        &self,
        a: ContainerInput,
        b: ContainerInput,
        records: &mut impl ContainerRecords,
    ) -> Result<u32> {
        let first = container_filter(a, records)?;
        let second = container_filter(b, records)?;
        Ok(self.collision_info(first, second))
    }
    /// REL23c308: child getter zero is passed directly, without fallback.
    pub fn container_query(
        &self,
        query_info: u32,
        container: usize,
        key: u32,
        records: &mut impl ContainerRecords,
    ) -> Result<u32> {
        let child = records.child_filter(container, key)?;
        Ok(self.collision_info(query_info, child))
    }
    /// REL23bf54/23be94, reached by collidable callback23c094 reading+1c.
    pub fn collision_info(&self, a: u32, b: u32) -> u32 {
        let yes = 0x01000000;
        let x = (a & 31) as usize;
        let y = (b & 31) as usize;
        if self.mode_30 != 0 {
            if y == 0 {
                return yes;
            }
            let matches = a & (1u32 << (y - 1)) != 0;
            if self.mode_31 == 0 {
                return if matches { yes } else { 0 };
            }
            if !matches {
                return 0;
            }
            let group = b >> 17;
            if group != 0 && group == self.excluded_group_38 {
                return 0;
            }
            let allowed = (self.flag_34 != 0 && b & 0x10000 != 0)
                || (self.flag_32 != 0 && b & 0x8000 != 0)
                || (self.flag_33 != 0 && b & 0x8000 == 0);
            return if allowed { yes } else { 0 };
        }
        if x == 0 || y == 0 {
            return yes;
        }
        if x == 10 {
            if y != 10 && y != 11 && b & 0x8000 == 0 {
                return 0;
            }
        } else if y == 10 {
            if x != 10 && x != 11 && a & 0x8000 == 0 {
                return 0;
            }
        } else if x != 9 && y != 9 && x != 11 && y != 11 {
            if a & 0x8000 == 0 || b & 0x8000 == 0 {
                return 0;
            }
        }
        if (a ^ b) & 0xfffe0000 == 0 && a & 0xfffe0000 != 0 {
            if (a >> 5) & 31 == (b >> 10) & 31 || (b >> 5) & 31 == (a >> 10) & 31 {
                return 0;
            }
            if x != 13 && y != 13 {
                return yes;
            }
        }
        if self.matrix.rows[x] & (1u32 << y) != 0 {
            yes
        } else {
            0
        }
    }
}
#[derive(Clone, Copy)]
pub struct ContainerInput {
    pub container: usize,
    pub key: u32,
    pub body: usize,
}
pub trait ContainerRecords {
    fn child_filter(&mut self, container: usize, key: u32) -> Result<u32>;
    fn parent_body(&mut self, body: usize) -> Result<Option<usize>>;
    fn body_filter(&mut self, body: usize) -> Result<u32>;
}
pub trait ShapeRecords: ContainerRecords {
    fn body_shape(&mut self, body: usize) -> Result<usize>;
    /// Actual collision dispatcher table+110 indexed by native shape type.
    fn dispatch_flags(&mut self, shape: usize) -> Result<u32>;
    fn body_key(&mut self, body: usize) -> Result<u32>;
    fn direct_child_filter(&mut self, shape: usize, key: u32) -> Result<u32>;
    /// DOL80245ea8 temporary container resolution followed by getter+18.
    fn temporary_child_filter(&mut self, shape: usize, key: u32) -> Result<u32>;
}
fn container_filter(input: ContainerInput, records: &mut impl ContainerRecords) -> Result<u32> {
    let child = records.child_filter(input.container, input.key)?;
    if child != 0 {
        return Ok(child);
    }
    root_filter(input.body, records)
}
fn root_filter(mut body: usize, records: &mut impl ContainerRecords) -> Result<u32> {
    let mut visited = std::collections::BTreeSet::new();
    loop {
        anyhow::ensure!(
            visited.insert(body),
            "Cycle in native collidable parent chain"
        );
        if let Some(parent) = records.parent_body(body)? {
            body = parent;
        } else {
            return records.body_filter(body);
        }
    }
}
impl Default for Matrix {
    fn default() -> Self {
        Self {
            rows: [u32::MAX; 32],
        }
    }
}
impl Matrix {
    /// REL23c370 /23c4bc: symmetric mask edits, every bit retained.
    pub fn masks(&mut self, a: u32, b: u32, enabled: bool) {
        for index in 0..32 {
            let bit = 1u32 << index;
            let mask = (if a & bit != 0 { b } else { 0 }) | (if b & bit != 0 { a } else { 0 });
            if enabled {
                self.rows[index] |= mask;
            } else {
                self.rows[index] &= !mask;
            }
        }
    }
    /// REL23c44c /23c484: symmetric single-category edits.
    pub fn pair(&mut self, a: usize, b: usize, enabled: bool) -> Result<()> {
        anyhow::ensure!(a < 32 && b < 32, "Game filter category outside32 rows");
        self.masks(1u32 << a, 1u32 << b, enabled);
        Ok(())
    }
    /// REL240ddc..2410b0, after installing the game's supplied filter.
    pub fn retail() -> Self {
        let mut matrix = Self::default();
        for (a, b) in [
            (0x00dddefe, 0x00dddefe),
            (1, u32::MAX),
            (u32::MAX, 1),
            (0x80, u32::MAX),
        ] {
            matrix.masks(a, b, true);
        }
        for (a, b) in [(7, 3), (7, 18), (7, 7)] {
            matrix.pair(a, b, false).unwrap();
        }
        for mask in [
            0x2000, 0x40, 0x4000, 0x8000, 0x20, 0x20000, 0x80000, 0x200000,
        ] {
            matrix.masks(mask, 0xfffffffe, false);
        }
        for (a, b) in [
            (6, 9),
            (14, 3),
            (14, 18),
            (15, 11),
            (5, 11),
            (5, 10),
            (21, 8),
        ] {
            matrix.pair(a, b, true).unwrap();
        }
        for (a, b) in [
            (18, 1),
            (18, 22),
            (18, 23),
            (9, 3),
            (9, 18),
            (9, 9),
            (9, 11),
            (9, 10),
            (9, 8),
            (3, 20),
            (16, 8),
            (16, 9),
            (16, 11),
            (16, 10),
        ] {
            matrix.pair(a, b, false).unwrap();
        }
        for (a, b) in [
            (17, 11),
            (17, 10),
            (17, 3),
            (17, 18),
            (19, 1),
            (19, 22),
            (19, 23),
        ] {
            matrix.pair(a, b, true).unwrap();
        }
        matrix.masks(0x1000, u32::MAX, true);
        matrix.pair(9, 12, false).unwrap();
        matrix
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Default,
    Supplied(usize),
}
impl Filter {
    /// Cinfo+58: null allocates the native default filter, otherwise retains
    /// the supplied object. Ownership must be handled by the world lifecycle.
    pub fn from_cinfo(filter: Option<usize>) -> Self {
        filter.map_or(Self::Default, Self::Supplied)
    }
    /// Keep the native return register, including its high-byte boolean format.
    /// Default does not inspect collidable flags/layers; supplied filters must.
    pub fn collision(
        &self,
        collidables: [usize; 2],
        mut supplied: impl FnMut(usize, [usize; 2]) -> Result<u32>,
    ) -> Result<u32> {
        match *self {
            Self::Default => Ok(0x01000000),
            Self::Supplied(identity) => supplied(identity, collidables),
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn shared_group_counter_retains_full_ids_wraps_and_resets_only_on_construction() {
        let mut groups = super::GroupCounter { value: 44 };
        let _filter = super::GameFilter::construct(&mut groups);
        assert_eq!(groups.value, 0);
        assert_eq!(groups.reserve(), 1);
        // Another native consumer shares the same sequence before a character.
        assert_eq!(groups.reserve(), 2);
        let group = groups.reserve();
        assert_eq!(group, 3);
        let data = super::super::collidable_owner::EntityData::character(Some(71), group);
        let packed = super::super::body_filter::pack(super::super::body_filter::Fields {
            layer: 1,
            group,
            sub_a: 0,
            sub_b: 0,
            painted: 1,
            real: 1,
        });
        assert_eq!(packed >> 17, data.word_0c);
        assert_eq!(groups.value, 3);
        let _independent_fields = super::GameFilter::default();
        assert_eq!(groups.value, 3);
        groups.value = 0x7fff;
        let full = groups.reserve();
        assert_eq!(full, 0x8000);
        assert_eq!(
            super::super::collidable_owner::EntityData::character(None, full).word_0c,
            0x8000
        );
        groups.value = u32::MAX;
        assert_eq!(groups.reserve(), 0);
        assert_eq!(groups.reserve(), 1);
        let _filter = super::GameFilter::construct(&mut groups);
        assert_eq!(groups.value, 0);
    }
    use super::*;
    #[test]
    fn packed_filter_matches_original_instruction_reference() -> Result<()> {
        #[derive(serde::Deserialize)]
        struct Context {
            rows: [u32; 32],
            flags: [u8; 5],
            group: u32,
        }
        #[derive(serde::Deserialize)]
        struct Reference {
            source_sha256: String,
            entry: u32,
            contexts: Vec<Context>,
            cases: Vec<[u32; 4]>,
        }
        let reference: Reference =
            serde_json::from_str(include_str!("filter_instruction_reference.json"))?;
        assert_eq!(
            reference.source_sha256,
            "ac25d520f329a3d0507d656f0ab1cf1c34025bb36ff866187ad082ae3e0a4ebf"
        );
        assert_eq!(reference.entry, 0x23bf54);
        assert_eq!(reference.cases.len(), 14408);
        for (index, [context, a, b, expected]) in reference.cases.iter().copied().enumerate() {
            let context = &reference.contexts[context as usize];
            let filter = GameFilter {
                matrix: Matrix { rows: context.rows },
                mode_30: context.flags[0],
                mode_31: context.flags[1],
                flag_32: context.flags[2],
                flag_33: context.flags[3],
                flag_34: context.flags[4],
                excluded_group_38: context.group,
            };
            assert_eq!(
                filter.collision_info(a, b),
                expected,
                "Original instruction case {index}: {a:08x}/{b:08x}"
            );
        }
        Ok(())
    }
    #[test]
    fn query_override_clears_only_mode_and_keeps_other_copied_fields() -> Result<()> {
        let mut filter = GameFilter::default();
        filter.mode_30 = 9;
        let flags = QueryFlags {
            bytes: [1, 2, 0, 1, 5],
            excluded_group: 7,
        };
        let value = filter.query(Some(flags), |f| {
            assert_eq!(
                (
                    f.mode_30,
                    f.mode_31,
                    f.flag_32,
                    f.flag_33,
                    f.flag_34,
                    f.excluded_group_38
                ),
                (1, 2, 0, 1, 5, 7)
            );
            f.flag_34 = 3;
            Ok(42)
        })?;
        assert_eq!(value, 42);
        assert_eq!(filter.mode_30, 0);
        assert_eq!(
            (
                filter.mode_31,
                filter.flag_32,
                filter.flag_33,
                filter.flag_34,
                filter.excluded_group_38
            ),
            (2, 0, 1, 3, 7)
        );
        filter.query(
            Some(QueryFlags {
                bytes: [0; 5],
                excluded_group: 99,
            }),
            |f| {
                assert_eq!(f.excluded_group_38, 7);
                Ok(())
            },
        )?;
        assert!(filter
            .query::<()>(Some(flags), |_| anyhow::bail!("Query backend unavailable"))
            .is_err());
        assert_eq!(filter.mode_30, 0);
        Ok(())
    }
    #[test]
    fn mixed_shape_filter_keeps_dispatch_priority_and_zero_fallback() -> Result<()> {
        struct Records {
            flags: u32,
            value: u32,
            calls: Vec<&'static str>,
        }
        impl ContainerRecords for Records {
            fn child_filter(&mut self, _: usize, _: u32) -> Result<u32> {
                self.calls.push("other child");
                Ok(0x8004)
            }
            fn parent_body(&mut self, b: usize) -> Result<Option<usize>> {
                self.calls.push("parent");
                Ok(if b == 10 { Some(11) } else { None })
            }
            fn body_filter(&mut self, _: usize) -> Result<u32> {
                self.calls.push("root");
                Ok(3)
            }
        }
        impl ShapeRecords for Records {
            fn body_shape(&mut self, b: usize) -> Result<usize> {
                assert_eq!(b, 11);
                self.calls.push("shape");
                Ok(20)
            }
            fn dispatch_flags(&mut self, _: usize) -> Result<u32> {
                self.calls.push("flags");
                Ok(self.flags)
            }
            fn body_key(&mut self, b: usize) -> Result<u32> {
                assert_eq!(b, 10);
                self.calls.push("key");
                Ok(7)
            }
            fn direct_child_filter(&mut self, _: usize, k: u32) -> Result<u32> {
                assert_eq!(k, 7);
                self.calls.push("direct");
                Ok(self.value)
            }
            fn temporary_child_filter(&mut self, _: usize, _: u32) -> Result<u32> {
                self.calls.push("temporary");
                Ok(self.value)
            }
        }
        let filter = GameFilter::default();
        let other = ContainerInput {
            container: 1,
            key: 2,
            body: 30,
        };
        let mut records = Records {
            flags: 0x800080,
            value: 0x8003,
            calls: vec![],
        };
        assert_eq!(filter.shape_container(10, other, &mut records)?, 0x01000000);
        assert_eq!(
            records.calls,
            vec!["other child", "parent", "shape", "flags", "key", "direct"]
        );
        records.flags = 0x100;
        records.value = 0;
        records.calls.clear();
        assert_eq!(filter.shape_container(10, other, &mut records)?, 0);
        assert_eq!(
            records.calls,
            vec![
                "other child",
                "parent",
                "shape",
                "flags",
                "key",
                "temporary",
                "parent",
                "parent",
                "root"
            ]
        );
        records.flags = 0x800000;
        records.calls.clear();
        assert_eq!(filter.shape_container(10, other, &mut records)?, 0x01000000);
        assert_eq!(
            records.calls,
            vec!["other child", "parent", "shape", "flags"]
        );
        Ok(())
    }
    #[test]
    fn container_pair_fallback_and_query_zero_are_distinct() -> Result<()> {
        struct Records(Vec<&'static str>);
        impl ContainerRecords for Records {
            fn child_filter(&mut self, c: usize, _: u32) -> Result<u32> {
                self.0.push(if c == 1 { "child1" } else { "child2" });
                Ok(0)
            }
            fn parent_body(&mut self, b: usize) -> Result<Option<usize>> {
                self.0.push("parent");
                Ok(if b == 10 { Some(11) } else { None })
            }
            fn body_filter(&mut self, b: usize) -> Result<u32> {
                self.0.push("root");
                Ok(if b == 11 { 3 } else { 0x8004 })
            }
        }
        let filter = GameFilter::default();
        let mut records = Records(vec![]);
        assert_eq!(
            filter.container_pair(
                ContainerInput {
                    container: 1,
                    key: 7,
                    body: 10
                },
                ContainerInput {
                    container: 2,
                    key: 8,
                    body: 20
                },
                &mut records
            )?,
            0
        );
        assert_eq!(
            records.0,
            vec!["child1", "parent", "parent", "root", "child2", "parent", "root"]
        );
        records.0.clear();
        assert_eq!(filter.container_query(3, 1, 7, &mut records)?, 0x01000000);
        assert_eq!(records.0, vec!["child1"]);
        Ok(())
    }
    #[test]
    fn packed_collision_predicate_keeps_category_flags_groups_and_mask_mode() {
        let mut filter = GameFilter::default();
        filter.matrix = Matrix::retail();
        assert_eq!(filter.collision_info(0, 3), 0x01000000);
        assert_eq!(filter.collision_info(3, 4), 0);
        assert_eq!(filter.collision_info(0x8003, 0x8004), 0x01000000);
        assert_eq!(filter.collision_info(10, 3), 0);
        assert_eq!(filter.collision_info(10, 0x8003), 0x01000000);
        assert_eq!(filter.collision_info(7 | 0x8000, 7 | 0x8000), 0);
        let a = 0x20000 | 0x8000 | (2 << 5) | (4 << 10) | 3;
        let b = 0x20000 | 0x8000 | (5 << 5) | (7 << 10) | 4;
        filter.matrix.rows = [0; 32];
        assert_eq!(filter.collision_info(a, b), 0x01000000);
        assert_eq!(filter.collision_info(a, (b & !0x7c00) | (2 << 10)), 0);
        filter.mode_30 = 1;
        assert_eq!(filter.collision_info(4, 3), 0x01000000);
        assert_eq!(filter.collision_info(2, 3), 0);
        filter.mode_31 = 1;
        assert_eq!(filter.collision_info(4, 3), 0);
        assert_eq!(filter.collision_info(4, 0x8003), 0x01000000);
        filter.excluded_group_38 = 1;
        assert_eq!(filter.collision_info(4, 0x28003), 0);
    }
    #[test]
    fn retail_matrix_preserves_symmetric_overrides_and_late_pair_disable() -> Result<()> {
        let matrix = Matrix::retail();
        for a in 0..32 {
            for b in 0..32 {
                assert_eq!(
                    matrix.rows[a] & (1 << b) != 0,
                    matrix.rows[b] & (1 << a) != 0
                );
            }
        }
        for (a, b, enabled) in [
            (0, 5, true),
            (5, 10, true),
            (5, 9, false),
            (7, 7, false),
            (17, 18, true),
            (19, 23, true),
            (18, 23, false),
            (12, 8, true),
            (12, 9, false),
        ] {
            assert_eq!(matrix.rows[a] & (1 << b) != 0, enabled);
        }
        let mut custom = Matrix { rows: [0; 32] };
        custom.masks(0x80000000, 1, true);
        assert_eq!(custom.rows[31], 1);
        assert_eq!(custom.rows[0], 0x80000000);
        assert!(custom.pair(32, 0, true).is_err());
        Ok(())
    }
    #[test]
    fn default_and_supplied_filters_preserve_native_return_register() -> Result<()> {
        assert_eq!(
            Filter::from_cinfo(None)
                .collision([5, 9], |_, _| panic!("Default has no supplied call"))?,
            0x01000000
        );
        let custom = Filter::from_cinfo(Some(7));
        assert_eq!(
            custom.collision([5, 9], |id, pair| {
                assert_eq!((id, pair), (7, [5, 9]));
                Ok(1)
            })?,
            1
        );
        assert!(custom
            .collision([5, 9], |_, _| anyhow::bail!(
                "Unimplemented supplied filter"
            ))
            .is_err());
        Ok(())
    }
}
