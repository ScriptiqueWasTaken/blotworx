//! Registry123a38 via chained lookup137900. Hash/equality are native registry
//! callbacks; reconstructing registration/allocation remains separate work.
use anyhow::{ensure, Result};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
/// Registry constructor2b21ac installs hash183d1c and equality183d04.
pub fn hash_key(key: u32) -> u32 {
    let mut hash = 0u32;
    for byte in key.to_be_bytes() {
        let signed = (byte as i8 as i32) as u32;
        hash ^= signed
            .wrapping_add(0xebb961ac)
            .wrapping_add(hash << 5)
            .wrapping_add(hash >> 2);
    }
    hash
}
/// REL1460c0 switch table4e43d4. Selectors above14 return32771;
/// this is the native result, not a host-selected entity/bucket cap.
pub fn bucket_size(selector: u32) -> u16 {
    const COUNTS: [u16; 15] = [
        1, 2, 5, 11, 17, 37, 67, 131, 257, 521, 1031, 2053, 4099, 8209, 16411,
    ];
    COUNTS.get(selector as usize).copied().unwrap_or(32771)
}
pub struct Node {
    pub key: u32,
    pub entity: Option<usize>,
    pub next: Option<usize>,
}
/// Initialized caller-owned bucket/node state; native allocation/registration
/// isn't fabricated by Default or a generic host hash-map insertion.
pub struct Table {
    pub count: u16,
    pub buckets: Vec<Option<usize>>,
    pub nodes: BTreeMap<usize, Node>,
    pub free_head: Option<usize>,
    pub entries: u32,
}
impl Table {
    pub fn find_node(
        &self,
        key: u32,
        hash: impl FnOnce(u32) -> u32,
    ) -> Result<Option<(usize, Option<usize>, usize)>> {
        if self.count == 0 {
            return Ok(None);
        }
        let index = (hash(key) % u32::from(self.count)) as usize;
        let mut node = *self
            .buckets
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Missing removal bucket {index}"))?;
        let mut previous = None;
        let mut visited = BTreeSet::new();
        while let Some(token) = node {
            ensure!(visited.insert(token), "Cyclic removal bucket chain");
            let record = self
                .nodes
                .get(&token)
                .ok_or_else(|| anyhow::anyhow!("Missing removal node {token}"))?;
            if record.key == key {
                return Ok(Some((index, previous, token)));
            }
            previous = Some(token);
            node = record.next;
        }
        Ok(None)
    }
    ///2c15e0 or captured reverse iterator removal2b2344..2b23e8.
    pub fn unlink_node(
        &mut self,
        index: usize,
        previous: Option<usize>,
        token: usize,
    ) -> Result<()> {
        let next = self
            .nodes
            .get(&token)
            .ok_or_else(|| anyhow::anyhow!("Missing unlink node {token}"))?
            .next;
        if let Some(previous) = previous {
            self.nodes
                .get_mut(&previous)
                .ok_or_else(|| anyhow::anyhow!("Missing unlink predecessor {previous}"))?
                .next = next;
        } else {
            *self
                .buckets
                .get_mut(index)
                .ok_or_else(|| anyhow::anyhow!("Missing unlink bucket {index}"))? = next;
        }
        self.nodes.get_mut(&token).unwrap().next = self.free_head;
        self.free_head = Some(token);
        self.entries = self.entries.wrapping_sub(1);
        Ok(())
    }
    pub fn remove_key(&mut self, key: u32, hash: impl FnOnce(u32) -> u32) -> Result<bool> {
        let Some((index, previous, token)) = self.find_node(key, hash)? else {
            return Ok(false);
        };
        self.unlink_node(index, previous, token)?;
        Ok(true)
    }
    ///2c1790/2c1c14: acquire before reading caller key/value holders. Native
    /// pool mode18 is caller supplied; exhausted nonzero mode faults natively.
    pub fn acquire(
        &mut self,
        pool_mode: u32,
        allocate: impl FnOnce(&mut Self) -> Result<usize>,
    ) -> Result<usize> {
        if let Some(token) = self.free_head {
            let next = self
                .nodes
                .get(&token)
                .ok_or_else(|| anyhow::anyhow!("Missing free registry node {token}"))?
                .next;
            ensure!(
                next != Some(token),
                "Self-linked free registry node {token}"
            );
            self.free_head = next;
            return Ok(token);
        }
        ensure!(
            pool_mode == 0,
            "Native registry node pool exhausted in fixed-pool mode"
        );
        let token = allocate(self)?;
        ensure!(token != 0, "Registry allocator returned a null node");
        ensure!(
            !self.nodes.contains_key(&token),
            "Registry allocator reused an owned node token {token}"
        );
        // Unpublished host allocation record; native input words are assigned
        // by prepare before linking, not inferred from allocator memory.
        self.nodes.insert(
            token,
            Node {
                key: 0,
                entity: None,
                next: None,
            },
        );
        Ok(token)
    }
    pub fn prepare(&mut self, token: usize, key: u32, value: u32) -> Result<()> {
        let node = self
            .nodes
            .get_mut(&token)
            .ok_or_else(|| anyhow::anyhow!("Missing acquired registry node {token}"))?;
        node.next = None;
        node.key = key;
        node.entity = Some(value as usize);
        Ok(())
    }
    /// Constructor2bcbb4: nonpositive signed selector leaves zero buckets.
    /// Vec owns host storage; native allocator/capacity bookkeeping is separate.
    pub fn initialized(selector: i32) -> Result<Self> {
        let count = if selector > 0 {
            bucket_size(selector as u32)
        } else {
            0
        };
        let mut buckets = Vec::new();
        buckets.try_reserve_exact(usize::from(count))?;
        buckets.resize(usize::from(count), None);
        Ok(Self {
            count,
            buckets,
            nodes: BTreeMap::new(),
            free_head: None,
            entries: 0,
        })
    }
    /// Insert helpers2c1790/2c1c14 initialize the node before linking it through
    /// 2cc648/2ccd30. Caller owns allocation/free-list acquisition. Duplicate
    /// replacement preserves chain position and returns the old node to pool.
    pub fn link_prepared(&mut self, token: usize, hash: u32) -> Result<()> {
        self.nodes
            .get_mut(&token)
            .ok_or_else(|| anyhow::anyhow!("Missing prepared insertion node {token}"))?
            .next = None;
        self.entries = self.entries.wrapping_add(1);
        self.link_node(token, hash)
    }
    fn link_node(&mut self, token: usize, hash: u32) -> Result<()> {
        ensure!(
            self.count != 0,
            "Prepared insertion requires initialized buckets"
        );
        let index = (hash % u32::from(self.count)) as usize;
        let mut current = *self
            .buckets
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("Missing insertion bucket {index}"))?;
        let key = self
            .nodes
            .get(&token)
            .ok_or_else(|| anyhow::anyhow!("Missing prepared insertion node {token}"))?
            .key;
        let mut previous = None;
        let mut visited = BTreeSet::new();
        while let Some(old) = current {
            ensure!(old != token, "Prepared insertion token is already active");
            ensure!(visited.insert(old), "Cyclic insertion bucket chain");
            let node = self
                .nodes
                .get(&old)
                .ok_or_else(|| anyhow::anyhow!("Missing active insertion node {old}"))?;
            let next = node.next;
            if node.key == key {
                if let Some(previous) = previous {
                    self.nodes.get_mut(&previous).unwrap().next = Some(token);
                } else {
                    self.buckets[index] = Some(token);
                }
                self.nodes.get_mut(&token).unwrap().next = next;
                self.nodes.get_mut(&old).unwrap().next = self.free_head;
                self.free_head = Some(old);
                self.entries = self.entries.wrapping_sub(1);
                return Ok(());
            }
            previous = Some(old);
            current = next;
        }
        self.nodes.get_mut(&token).unwrap().next = self.buckets[index];
        self.buckets[index] = Some(token);
        Ok(())
    }
    ///2cc72c: snapshot active nodes in iterator order, zero/resize buckets,
    /// reinsert existing nodes via link helper without incrementing entries.
    pub fn rehash(&mut self, selector: u32, mut hash: impl FnMut(u32) -> u32) -> Result<()> {
        let mut active = Vec::new();
        let mut visited = BTreeSet::new();
        for index in 0..usize::from(self.count) {
            let mut next = *self
                .buckets
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("Missing rehash bucket {index}"))?;
            while let Some(token) = next {
                ensure!(visited.insert(token), "Repeated/cyclic rehash node {token}");
                ensure!(
                    active.len() < usize::from(u16::MAX),
                    "Native16bit rehash snapshot count would overflow"
                );
                active.try_reserve(1)?;
                active.push(token);
                next = self
                    .nodes
                    .get(&token)
                    .ok_or_else(|| anyhow::anyhow!("Missing rehash node {token}"))?
                    .next;
            }
        }
        self.count = 0; // Native clears logical count before bucket allocation.
        let count = bucket_size(selector);
        let mut buckets = Vec::new();
        buckets.try_reserve_exact(usize::from(count))?;
        buckets.resize(usize::from(count), None);
        self.buckets = buckets;
        self.count = count;
        for token in active {
            let key = self.nodes[&token].key;
            self.link_node(token, hash(key))?;
        }
        Ok(())
    }
}
impl Backend for Table {
    fn bucket_count(&mut self) -> Result<u16> {
        Ok(self.count)
    }
    fn hash(&mut self, key: u32) -> Result<u32> {
        Ok(hash_key(key))
    }
    fn bucket(&mut self, i: usize) -> Result<Option<usize>> {
        self.buckets
            .get(i)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Missing entity-handle bucket {i}"))
    }
    fn equals(&mut self, n: usize, k: u32) -> Result<bool> {
        Ok(self
            .nodes
            .get(&n)
            .ok_or_else(|| anyhow::anyhow!("Missing entity-handle node {n}"))?
            .key
            == k)
    }
    fn next(&mut self, n: usize) -> Result<Option<usize>> {
        Ok(self
            .nodes
            .get(&n)
            .ok_or_else(|| anyhow::anyhow!("Missing entity-handle node {n}"))?
            .next)
    }
    fn entity(&mut self, n: usize) -> Result<Option<usize>> {
        Ok(self
            .nodes
            .get(&n)
            .ok_or_else(|| anyhow::anyhow!("Missing entity-handle node {n}"))?
            .entity
            .filter(|e| *e != 0))
    }
}
impl super::transform_hierarchy::Resolver for Table {
    fn resolve(
        &mut self,
        key: u32,
        _: &mut super::transform_child_list::Children,
    ) -> Result<Option<usize>> {
        resolve(key, self)
    }
}
/// Registry+20 reverse map, hash a5184/equality a516c. Node value is a handle,
/// not an entity token. Native keys are opaque32bit interface identities.
pub struct Reverse {
    pub table: Table,
}
impl Reverse {
    pub fn initialized(selector: i32) -> Result<Self> {
        Ok(Self {
            table: Table::initialized(selector)?,
        })
    }
    /// REL123ac0 copies found handle to output, or zero on missing key.
    pub fn handle(&mut self, entity: u32) -> Result<u32> {
        match resolve(entity, self)? {
            None => Ok(0),
            Some(value) => u32::try_from(value).map_err(|_| {
                anyhow::anyhow!("Reverse registry handle exceeds native32bit storage")
            }),
        }
    }
}
impl Backend for Reverse {
    fn bucket_count(&mut self) -> Result<u16> {
        self.table.bucket_count()
    }
    fn hash(&mut self, key: u32) -> Result<u32> {
        Ok(key)
    }
    fn bucket(&mut self, i: usize) -> Result<Option<usize>> {
        self.table.bucket(i)
    }
    fn equals(&mut self, n: usize, k: u32) -> Result<bool> {
        self.table.equals(n, k)
    }
    fn next(&mut self, n: usize) -> Result<Option<usize>> {
        self.table.next(n)
    }
    fn entity(&mut self, n: usize) -> Result<Option<usize>> {
        self.table.entity(n)
    }
}
pub trait Backend {
    fn bucket_count(&mut self) -> Result<u16>;
    fn hash(&mut self, key: u32) -> Result<u32>;
    fn bucket(&mut self, index: usize) -> Result<Option<usize>>;
    fn equals(&mut self, node: usize, key: u32) -> Result<bool>;
    fn next(&mut self, node: usize) -> Result<Option<usize>>;
    fn entity(&mut self, node: usize) -> Result<Option<usize>>;
}
pub fn resolve(key: u32, backend: &mut impl Backend) -> Result<Option<usize>> {
    if backend.bucket_count()? == 0 {
        return Ok(None);
    }
    let hash = backend.hash(key)?;
    let count = backend.bucket_count()?;
    ensure!(
        count != 0,
        "Registry bucket count became zero during hash callback"
    );
    let mut node = backend.bucket((hash % u32::from(count)) as usize)?;
    let mut visited = BTreeSet::new();
    while let Some(current) = node {
        ensure!(visited.insert(current), "Cyclic entity-handle bucket chain");
        if backend.equals(current, key)? {
            return backend.entity(current);
        }
        node = backend.next(current)?;
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rehash_reuses_nodes_and_preserves_native_chain_reversal() -> Result<()> {
        let mut table = Table::initialized(8)?;
        for (token, key) in [(100, 1), (200, 258)] {
            table.nodes.insert(
                token,
                Node {
                    key,
                    entity: Some(token),
                    next: None,
                },
            );
            table.link_prepared(token, key)?;
        }
        assert_eq!(table.buckets[1], Some(200));
        let mut order = vec![];
        table.rehash(8, |key| {
            order.push(key);
            key
        })?;
        assert_eq!(order, [258, 1]);
        assert_eq!(table.buckets[1], Some(100));
        assert_eq!(table.nodes[&100].next, Some(200));
        assert_eq!(table.entries, 2);
        assert_eq!(table.nodes.len(), 2);
        assert_eq!(table.free_head, None);
        table.rehash(1, |key| key)?;
        assert_eq!(table.count, 2);
        let mut reverse = Reverse { table };
        assert_eq!(reverse.handle(1)?, 100);
        assert_eq!(reverse.handle(258)?, 200);
        let mut empty = Table::initialized(0)?;
        empty.rehash(8, |_| panic!("Empty map mustn't hash"))?;
        assert_eq!(empty.count, 257);
        assert_eq!(empty.entries, 0);
        Ok(())
    }
    #[test]
    fn acquisition_reuses_retired_nodes_and_reports_exhausted_pool() -> Result<()> {
        let mut table = Table::initialized(8)?;
        let token = table.acquire(0, |_| Ok(100))?;
        table.prepare(token, 1, 10)?;
        table.link_prepared(token, hash_key(1))?;
        let token = table.acquire(0, |_| Ok(200))?;
        table.prepare(token, 1, 20)?;
        table.link_prepared(token, hash_key(1))?;
        assert_eq!(table.free_head, Some(100));
        assert_eq!(table.entries, 1);
        let token = table.acquire(1, |_| anyhow::bail!("Allocation must be skipped"))?;
        assert_eq!(token, 100);
        assert_eq!(table.nodes[&100].entity, Some(10));
        table.prepare(token, 7, 30)?;
        table.link_prepared(token, hash_key(7))?;
        assert_eq!(table.nodes.len(), 2);
        assert_eq!(table.entries, 2);
        assert_eq!(resolve(1, &mut table)?, Some(20));
        assert_eq!(resolve(7, &mut table)?, Some(30));
        assert!(table
            .acquire(1, |_| anyhow::bail!("Must not allocate in fixed-pool mode"))
            .unwrap_err()
            .to_string()
            .contains("exhausted"));
        assert!(table.acquire(0, |_| Ok(200)).is_err());
        assert_eq!(table.nodes.len(), 2);
        Ok(())
    }
    #[test]
    fn prepared_link_replaces_in_place_and_recycles_old_nodes() -> Result<()> {
        let mut table = Table::initialized(8)?;
        let first = 1;
        let second = first + 257; // Reverse raw-key hash collisions.
        for (token, key, value) in [
            (100, first, 10),
            (200, second, 20),
            (300, first, 30),
            (400, second, 40),
        ] {
            table.nodes.insert(
                token,
                Node {
                    key,
                    entity: Some(value),
                    next: None,
                },
            );
            table.link_prepared(token, key)?;
        }
        let bucket = (first % 257) as usize;
        assert_eq!(table.entries, 2);
        assert_eq!(table.buckets[bucket], Some(400));
        assert_eq!(table.nodes[&400].next, Some(300));
        assert_eq!(table.nodes[&300].next, None);
        assert_eq!(table.free_head, Some(200));
        assert_eq!(table.nodes[&200].next, Some(100));
        assert_eq!(table.nodes[&100].next, None);
        let mut reverse = Reverse { table };
        assert_eq!(reverse.handle(first)?, 30);
        assert_eq!(reverse.handle(second)?, 40);
        assert_eq!(reverse.table.nodes.len(), 4);
        Ok(())
    }
    #[test]
    fn reverse_raw_identity_hash_retains_native_handles_for_child_removal() -> Result<()> {
        let mut reverse = Reverse::initialized(8)?;
        let key = 0x1000u32;
        let other = key + 257;
        let bucket = (key % 257) as usize;
        reverse.table.buckets[bucket] = Some(100);
        reverse.table.nodes.insert(
            100,
            Node {
                key,
                entity: Some(7),
                next: Some(200),
            },
        );
        reverse.table.nodes.insert(
            200,
            Node {
                key: other,
                entity: Some(u32::MAX as usize),
                next: None,
            },
        );
        assert_eq!(reverse.handle(key)?, 7);
        assert_eq!(reverse.handle(other)?, u32::MAX);
        assert_eq!(reverse.handle(key + 1)?, 0);
        let mut children = super::super::transform_child_list::Children::default();
        for h in [3, 7, 9] {
            children.register(h, |_, _| anyhow::bail!("Unexpected lookup"))?;
        }
        assert!(children.remove(reverse.handle(key)?));
        assert_eq!(children.entries(), [3, 9]);
        assert_eq!(reverse.table.nodes.len(), 2);
        assert_eq!(reverse.table.count, 257);
        Ok(())
    }
    #[test]
    fn native_constructor_bucket_selector_is_not_literal_capacity() -> Result<()> {
        assert_eq!(
            (
                bucket_size(0),
                bucket_size(8),
                bucket_size(14),
                bucket_size(15),
                bucket_size(u32::MAX)
            ),
            (1, 257, 16411, 32771, 32771)
        );
        for selector in [i32::MIN, -1, 0, 8, 15] {
            let mut table = Table::initialized(selector)?;
            let expected = if selector <= 0 {
                0
            } else if selector == 8 {
                257
            } else {
                32771
            };
            assert_eq!(table.count, expected);
            assert_eq!(table.buckets.len(), usize::from(expected));
            assert!(table.nodes.is_empty());
            assert!(table.buckets.iter().all(Option::is_none));
            assert_eq!(resolve(7, &mut table)?, None);
        }
        Ok(())
    }
    #[test]
    fn retail_signed_byte_hash_and_owned_table_bind_to_child_marking() -> Result<()> {
        for (key, expected) in [
            (0, 0x8d1ac4d5),
            (1, 0x8d1ac4d4),
            (7, 0x8d1ac4da),
            (0x7fffffff, 0x8d5538e0),
            (0x80000000, 0x8cd77b73),
            (0xdeadbeef, 0x8d237bd7),
            (0xffffffff, 0x8d1452d6),
        ] {
            assert_eq!(hash_key(key), expected);
        }
        let mut table = Table {
            count: 3,
            buckets: vec![None, None, Some(100)],
            nodes: BTreeMap::from([
                (
                    100,
                    Node {
                        key: 1,
                        entity: Some(10),
                        next: Some(200),
                    },
                ),
                (
                    200,
                    Node {
                        key: 7,
                        entity: Some(20),
                        next: None,
                    },
                ),
            ]),
            free_head: None,
            entries: 2,
        };
        assert_eq!(resolve(7, &mut table)?, Some(20));
        assert_eq!(resolve(8, &mut table)?, None);
        let mut children = super::super::transform_child_list::Children::default();
        children.register(7, |_, _| anyhow::bail!("Unexpected lookup"))?;
        let components = BTreeMap::from([(20, 300)]);
        let mut records = BTreeMap::from([(300, vec![0; 0x140])]);
        super::super::transform_children::mark(&mut super::super::transform_hierarchy::Mark {
            children: &mut children,
            resolver: &mut table,
            components: &components,
            records: &mut records,
        })?;
        assert_eq!(records[&300][0x124], 1);
        assert_eq!(table.nodes.len(), 2);
        Ok(())
    }
    struct State {
        count: u16,
        calls: Vec<String>,
        cycle: bool,
    }
    impl Backend for State {
        fn bucket_count(&mut self) -> Result<u16> {
            self.calls.push("count".into());
            Ok(self.count)
        }
        fn hash(&mut self, k: u32) -> Result<u32> {
            self.calls.push(format!("hash {k}"));
            self.count = 3;
            Ok(8)
        }
        fn bucket(&mut self, i: usize) -> Result<Option<usize>> {
            self.calls.push(format!("bucket {i}"));
            Ok(Some(10))
        }
        fn equals(&mut self, n: usize, k: u32) -> Result<bool> {
            self.calls.push(format!("equals {n}"));
            Ok(n == 20 && k == 7)
        }
        fn next(&mut self, n: usize) -> Result<Option<usize>> {
            self.calls.push(format!("next {n}"));
            Ok(if n == 10 {
                Some(20)
            } else if self.cycle {
                Some(10)
            } else {
                None
            })
        }
        fn entity(&mut self, n: usize) -> Result<Option<usize>> {
            self.calls.push(format!("entity {n}"));
            Ok(Some(90))
        }
    }
    #[test]
    fn native_hash_callback_fresh_count_and_bucket_chain() -> Result<()> {
        let mut s = State {
            count: 2,
            calls: vec![],
            cycle: false,
        };
        assert_eq!(resolve(7, &mut s)?, Some(90));
        assert_eq!(
            s.calls,
            [
                "count",
                "hash 7",
                "count",
                "bucket 2",
                "equals 10",
                "next 10",
                "equals 20",
                "entity 20"
            ]
        );
        s.calls.clear();
        assert_eq!(resolve(8, &mut s)?, None);
        assert!(!s.calls.iter().any(|c| c.starts_with("entity")));
        s.calls.clear();
        s.count = 0;
        assert_eq!(resolve(7, &mut s)?, None);
        assert_eq!(s.calls, ["count"]);
        s.count = 1;
        s.cycle = true;
        assert!(resolve(8, &mut s)
            .unwrap_err()
            .to_string()
            .contains("Cyclic"));
        Ok(())
    }
}
