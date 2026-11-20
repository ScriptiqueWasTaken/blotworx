//!492b14..4957f8 modifier groups, factory dispatch and owned table growth.
//! Metadata registries and actual modifier implementations are mandatory hosts.
use super::{ai_pathfinder_constructor::State, ai_traversal_registry::names_equal};
use anyhow::{ensure, Context, Result};

pub struct Storage {
    pub identity: usize,
    /// None is unwritten storage, not an invented null modifier.
    pub slots: Vec<Option<usize>>,
}
#[derive(Default)]
pub struct Table {
    pub storage: Option<Storage>,
    pub word_04: u32,
    pub capacity: u32,
    pub count: u32,
    pub selected: Option<Option<usize>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind(pub usize);
impl Kind {
    pub const NAMES: [&'static [u8]; 13] = [
        b"IRefineGoal",
        b"IDetectGoalReached",
        b"IDetectGoalChanged",
        b"ICanGo",
        b"IDetectAccident",
        b"ISelectPathNodeCandidate",
        b"IDetectPathNodeReached",
        b"IComputeTargetPoint",
        b"IGoto",
        b"ISteering",
        b"IFindNodesFromPositions",
        b"ILpfShortcut",
        b"IEdgeStatusAwareness",
    ];
    pub fn table(self) -> usize {
        [0, 1, 2, 6, 3, 4, 5, 7, 8, 9, 10, 11, 12][self.0]
    }
}
pub trait Host {
    fn name(&mut self, state: &mut State, node: usize) -> Result<Option<Vec<u8>>>;
    fn value(&mut self, state: &mut State, node: usize) -> Result<Option<usize>>;
    fn text(&mut self, state: &mut State, value: usize) -> Result<Vec<u8>>;
    fn identifier(&mut self, state: &mut State, node: usize) -> Result<Option<usize>>;
    fn child_count(&mut self, state: &mut State, node: usize) -> Result<u32>;
    fn child(&mut self, state: &mut State, node: usize, index: u32) -> Result<Option<usize>>;
    fn class_child(&mut self, state: &mut State, node: usize) -> Result<Option<usize>>;
    fn class(&mut self, state: &mut State, kind: Kind, value: usize) -> Result<Option<usize>>;
    fn construct(
        &mut self,
        state: &mut State,
        kind: Kind,
        class: usize,
        identifier: usize,
    ) -> Result<usize>;
    fn initialize_modifier(
        &mut self,
        state: &mut State,
        kind: Kind,
        object: usize,
        node: usize,
    ) -> Result<bool>;
    /// Dispatch a nested child through the complete492440 attribute handler.
    fn initialize_attribute(&mut self, state: &mut State, node: usize) -> Result<bool>;
    fn object_name(&mut self, state: &mut State, kind: Kind, object: usize) -> Result<Vec<u8>>;
    fn allocate_table(&mut self, state: &mut State, kind: Kind, capacity: u32) -> Result<Storage>;
    fn free_table(&mut self, state: &mut State, kind: Kind, storage: usize) -> Result<()>;
}
/// Required derived48fec4 default selection/construction operations. Concrete
///objects and registered metadata must exist; unknown implementations error.
pub trait DefaultsHost: Host {
    fn default_class(&self,kind:Kind)->Result<usize>;
    fn object_class(&mut self,state:&mut State,kind:Kind,object:usize)->Result<usize>;
    fn parent_class(&mut self,state:&mut State,class:usize)->Result<Option<usize>>;
    fn construct_default(&mut self,state:&mut State,kind:Kind,class:usize,name:&[u8])->Result<Option<usize>>;
    /// Actual virtual10 with a NULL definition. Native48fec4 discards the
    ///defined boolean result; Rust implementation errors are not booleans.
    fn initialize_default(&mut self,state:&mut State,kind:Kind,object:usize)->Result<bool>;
}
/// Exact490568..4920d4 phase order. ILpfShortcut is not in this phase.
pub const DEFAULT_ORDER:[Kind;12]=[Kind(0),Kind(1),Kind(2),Kind(4),Kind(5),Kind(6),
    Kind(3),Kind(9),Kind(8),Kind(7),Kind(10),Kind(12)];

/// Complete default-modifier phase, not the surrounding48fec4 search-storage,
///reset or service publication. Success here alone is not a ready pathfinder.
pub fn initialize_defaults(state:&mut State,host:&mut impl DefaultsHost)->Result<()> {
    for kind in DEFAULT_ORDER {
        if selected(state,kind)?.is_some() {continue;}
        let wanted=host.default_class(kind)?;
        let mut index=0;
        while index<state.tables_fc_1fc[kind.table()].count {
            let object=entry(state,kind,index)?;
            let mut class=host.object_class(state,kind,object)?;
            let mut visited=std::collections::BTreeSet::new();
            let matches=loop {
                if class==wanted {break true;}
                ensure!(visited.insert(class),"Cyclic default modifier class metadata");
                let Some(parent)=host.parent_class(state,class)? else {break false};
                class=parent;
            };
            if matches {
                // The source re-reads this table slot AFTER the class getter.
                let object=entry(state,kind,index)?;
                state.tables_fc_1fc[kind.table()].selected=Some(Some(object));
                break;
            }
            index=index.wrapping_add(1);
        }
        if selected(state,kind)?.is_some() {continue;}
        let object=host.construct_default(state,kind,wanted,b"default")?;
        state.tables_fc_1fc[kind.table()].selected=Some(object);
        let object=object.context("Default modifier allocation is null before its required virtual initializer")?;
        ensure!(object!=0,"Default modifier identity is null");
        let _=host.initialize_default(state,kind,object)?;
        let table=&mut state.tables_fc_1fc[kind.table()];
        if table.count==table.capacity {
            table.capacity=if table.capacity==0 {4}else{table.capacity.wrapping_mul(2)};
            let capacity=table.capacity;
            grow(state,kind,capacity,true,host)?;
        }
        // Initialization/allocation/free callbacks can replace the selection.
        // A written null entry is distinct from unwritten table storage.
        let object=selected(state,kind)?.unwrap_or(0);
        let table=&mut state.tables_fc_1fc[kind.table()];
        *table.storage.as_mut().context("Default modifier append has no table allocation")?
            .slots.get_mut(table.count as usize).context("Default modifier append exceeds allocated storage")?=Some(object);
        table.count=table.count.wrapping_add(1);
    }
    Ok(())
}
fn entry(state: &State, kind: Kind, index: u32) -> Result<usize> {
    let table = &state.tables_fc_1fc[kind.table()];
    table
        .storage
        .as_ref()
        .context("Modifier table has no allocation")?
        .slots
        .get(index as usize)
        .copied()
        .flatten()
        .context("Modifier table entry is outside initialized storage")
}
fn selected(state: &State, kind: Kind) -> Result<Option<usize>> {
    state.tables_fc_1fc[kind.table()]
        .selected
        .context("Modifier default used before initialization")
}
fn grow(
    state: &mut State,
    kind: Kind,
    capacity: u32,
    clamp: bool,
    host: &mut impl Host,
) -> Result<()> {
    let mut storage = host.allocate_table(state, kind, capacity)?;
    // Allocator callbacks can change count/capacity; the native code reads them
    // again before copying. Group reservation does not apply this clamp.
    if clamp {
        let table = &mut state.tables_fc_1fc[kind.table()];
        if table.count > table.capacity {
            table.count = table.capacity;
        }
    }
    let mut index = 0;
    while index < state.tables_fc_1fc[kind.table()].count {
        let object = entry(state, kind, index)?;
        *storage
            .slots
            .get_mut(index as usize)
            .context("Modifier allocation is shorter than the live copy count")? = Some(object);
        index += 1;
    }
    if let Some(old) = state.tables_fc_1fc[kind.table()]
        .storage
        .as_ref()
        .map(|s| s.identity)
    {
        host.free_table(state, kind, old)?;
    }
    state.tables_fc_1fc[kind.table()].storage = Some(storage);
    Ok(())
}
fn group(state: &mut State, kind: Kind, node: usize, host: &mut impl Host) -> Result<bool> {
    let count = host.child_count(state, node)?;
    if count > state.tables_fc_1fc[kind.table()].capacity {
        state.tables_fc_1fc[kind.table()].capacity = count;
        grow(state, kind, count, false, host)?;
    }
    let mut requested = None;
    let mut index = 0;
    while index < host.child_count(state, node)? {
        let child = host
            .child(state, node, index)?
            .context("Modifier group child is null")?;
        let name = host
            .name(state, child)?
            .context("Modifier group child has no tag")?;
        if names_equal(&name, b"default") {
            requested = host.value(state, child)?;
        } else if !host.initialize_attribute(state, child)? {
            return Ok(false);
        }
        index = index.wrapping_add(1);
    }
    if let Some(requested) = requested {
        let mut index = 0;
        while index < state.tables_fc_1fc[kind.table()].count {
            let object = entry(state, kind, index)?;
            let name = host.object_name(state, kind, object)?;
            if names_equal(&name, &host.text(state, requested)?) {
                let object = entry(state, kind, index)?;
                state.tables_fc_1fc[kind.table()].selected = Some(Some(object));
                break;
            }
            index = index.wrapping_add(1);
        }
        if selected(state, kind)?.is_none() {
            return Ok(false);
        }
    }
    if selected(state, kind)?.is_none() && state.tables_fc_1fc[kind.table()].count != 0 {
        let object = entry(state, kind, 0)?;
        state.tables_fc_1fc[kind.table()].selected = Some(Some(object));
    }
    Ok(true)
}
fn instance(state: &mut State, kind: Kind, node: usize, host: &mut impl Host) -> Result<bool> {
    let Some(identifier) = host.identifier(state, node)? else {
        return Ok(false);
    };
    let class = if let Some(class) = host.class_child(state, node)? {
        host.value(state, class)?
    } else {
        None
    };
    let Some(class) = class else { return Ok(false) };
    let Some(class) = host.class(state, kind, class)? else {
        return Ok(false);
    };
    let object = host.construct(state, kind, class, identifier)?;
    ensure!(object != 0, "Modifier factory returned null");
    if !host.initialize_modifier(state, kind, object, node)? {
        return Ok(false);
    }
    let table = &mut state.tables_fc_1fc[kind.table()];
    if table.count == table.capacity {
        table.capacity = if table.capacity == 0 {
            4
        } else {
            table.capacity.wrapping_mul(2)
        };
        let capacity = table.capacity;
        grow(state, kind, capacity, true, host)?;
    }
    let table = &mut state.tables_fc_1fc[kind.table()];
    *table
        .storage
        .as_mut()
        .context("Modifier append has no table allocation")?
        .slots
        .get_mut(table.count as usize)
        .context("Modifier append is outside allocated storage")? = Some(object);
    table.count = table.count.wrapping_add(1);
    Ok(true)
}
/// None means no modifier tag matched. Registered-but-unimplemented factories
/// must error through Host, never return a fictitious initialized object.
pub fn initialize(state: &mut State, node: usize, host: &mut impl Host) -> Result<Option<bool>> {
    for (index, name) in Kind::NAMES.iter().enumerate() {
        let tag = host
            .name(state, node)?
            .context("Modifier group has no tag")?;
        let mut expected = b"Modifiers_".to_vec();
        expected.extend_from_slice(name);
        if names_equal(&tag, &expected) {
            return Ok(Some(group(state, Kind(index), node, host)?));
        }
    }
    for (index, name) in Kind::NAMES.iter().enumerate() {
        let tag = host
            .name(state, node)?
            .context("Modifier instance has no tag")?;
        if names_equal(&tag, name) {
            return Ok(Some(instance(state, Kind(index), node, host)?));
        }
    }
    Ok(None)
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::{
        ai_navigation_action::{Class, Registry},
        ai_navigation_construction::{ActionClass, ActionFactory, Constructed, Layout},
        ai_pathfinder_attributes as attributes,
        ai_pathfinder_constructor::{Base, BaseHost, Counter, Service},
    };
    use super::*;
    use serde_json::{json, Value};
    struct Construction;
    impl BaseHost for Construction {
        fn unit_scale(&mut self, _: &mut Base) -> Result<Option<f32>> {
            Ok(None)
        }
        fn profiling_enabled(&mut self, _: &mut Base) -> Result<bool> {
            Ok(false)
        }
        fn register_budget(&mut self, _: &mut Base, _: &str) -> Result<i32> {
            anyhow::bail!("Unexpected profiler")
        }
    }
    impl super::super::ai_pathfinder_constructor::Host for Construction {
        fn world_service(&mut self, _: &mut State, _: Service) -> Result<Option<usize>> {
            Ok(None)
        }
        fn point_attribute_index(&mut self, _: &mut State) -> Result<i32> {
            Ok(0)
        }
    }
    pub(in crate::blotworx) fn state() -> Result<State> {
        let registry = || Registry {
            count: 2,
            heading: Class {
                index: 0,
                constructor: true,
            },
            speed: Class {
                index: 1,
                constructor: true,
            },
        };
        let owner = Constructed::new(
            20,
            Some(10),
            Some(30),
            &ActionClass {
                metadata: 90,
                factory: Some(ActionFactory::BaseAi),
            },
            &Layout {
                agent_classes: 3,
                service_classes: 4,
                attributes: registry(),
            },
        )?;
        State::construct(
            50,
            &owner,
            40,
            &registry(),
            &mut Counter::default(),
            &mut Construction,
        )
    }
    struct Row {
        name: String,
        identifier: Option<String>,
        value: Option<String>,
        children: Vec<usize>,
        class_child: Option<usize>,
    }
    struct Binding<'a> {
        case: &'a Value,
        rows: Vec<Row>,
        objects: Vec<Value>,
        trace: Vec<Value>,
        allocations: usize,
        mutated: bool,
        replacement: Option<usize>,
    }
    impl Binding<'_> {
        fn event(&mut self, event: Value) {
            if event[0] == "name" {
                if let Some(last) = self.trace.last_mut() {
                    if last[0] == "name" && last[1] == event[1] {
                        last[2] = json!(last[2].as_u64().unwrap() + 1);
                        return;
                    }
                }
                self.trace.push(json!(["name", event[1], 1]));
            } else {
                self.trace.push(event);
            }
        }
        fn add(&mut self, name: &str, identifier: Option<String>, value: Option<String>) -> usize {
            let index = self.rows.len();
            self.rows.push(Row {
                name: name.into(),
                identifier,
                value,
                children: vec![],
                class_child: None,
            });
            index
        }
        fn add_instance(&mut self, name: &str) -> usize {
            let kind = self.case["kind"].as_u64().unwrap() as usize;
            let index = self.add(
                std::str::from_utf8(Kind::NAMES[kind]).unwrap(),
                (self.case["identifier"] != false).then(|| name.into()),
                None,
            );
            let class = self.add(
                "Class",
                None,
                (self.case["class_value"] != false).then(|| "SuppliedClass".into()),
            );
            if self.case["class_child"] != false {
                self.rows[index].class_child = Some(class);
            }
            index
        }
        fn object(&mut self, name: &str) -> usize {
            let identity = 100 + self.objects.len();
            self.objects.push(json!({"identity":identity,"name":name}));
            identity
        }
        fn string(&self, handle: usize) -> Result<Vec<u8>> {
            let row = &self.rows[(handle - 1) / 2];
            let value = if handle % 2 == 1 {
                row.value.as_ref()
            } else {
                row.identifier.as_ref()
            };
            Ok(value
                .context("Test string disappeared")?
                .as_bytes()
                .to_vec())
        }
    }
    impl Host for Binding<'_> {
        fn name(&mut self, _: &mut State, node: usize) -> Result<Option<Vec<u8>>> {
            self.event(json!(["name", node]));
            Ok(Some(self.rows[node].name.as_bytes().to_vec()))
        }
        fn value(&mut self, _: &mut State, node: usize) -> Result<Option<usize>> {
            self.event(json!(["value", node]));
            Ok(self.rows[node].value.as_ref().map(|_| node * 2 + 1))
        }
        fn text(&mut self, _: &mut State, value: usize) -> Result<Vec<u8>> {
            self.string(value)
        }
        fn identifier(&mut self, _: &mut State, node: usize) -> Result<Option<usize>> {
            self.event(json!(["identifier", node]));
            Ok(self.rows[node].identifier.as_ref().map(|_| node * 2 + 2))
        }
        fn child_count(&mut self, _: &mut State, node: usize) -> Result<u32> {
            self.event(json!(["count", node]));
            Ok(self.rows[node].children.len() as u32)
        }
        fn child(&mut self, _: &mut State, node: usize, index: u32) -> Result<Option<usize>> {
            self.event(json!(["child", node, index]));
            Ok(self.rows[node].children.get(index as usize).copied())
        }
        fn class_child(&mut self, _: &mut State, node: usize) -> Result<Option<usize>> {
            self.event(json!(["class_child", node]));
            Ok(self.rows[node].class_child)
        }
        fn class(&mut self, _: &mut State, kind: Kind, value: usize) -> Result<Option<usize>> {
            let text = String::from_utf8(self.string(value)?)?;
            self.event(json!(["class", kind.0, text]));
            Ok((self.case["class_found"] != false).then_some(80 + kind.0))
        }
        fn construct(
            &mut self,
            _: &mut State,
            kind: Kind,
            class: usize,
            identifier: usize,
        ) -> Result<usize> {
            ensure!(class == 80 + kind.0, "Wrong category metadata");
            let name = String::from_utf8(self.string(identifier)?)?;
            self.event(json!(["construct", kind.0, name]));
            Ok(self.object(&name))
        }
        fn initialize_modifier(
            &mut self,
            _: &mut State,
            kind: Kind,
            object: usize,
            node: usize,
        ) -> Result<bool> {
            self.event(json!(["initialize", kind.0, object, node]));
            if !self.mutated {
                if self.case["mutation"] == "child_append" {
                    let child = self.add_instance("Appended");
                    self.rows[0].children.push(child);
                    self.mutated = true;
                } else if self.case["mutation"] == "child_shrink" {
                    self.rows[0].children.truncate(1);
                    self.mutated = true;
                }
            }
            Ok(self.case["accepted"] != false)
        }
        fn initialize_attribute(&mut self, state: &mut State, node: usize) -> Result<bool> {
            attributes::initialize(state, node, self)
        }
        fn object_name(&mut self, state: &mut State, kind: Kind, object: usize) -> Result<Vec<u8>> {
            self.event(json!(["object_name", object]));
            let name = self.objects[object - 100]["name"]
                .as_str()
                .unwrap()
                .as_bytes()
                .to_vec();
            if self.case["mutation"] == "object_name_replace" && !self.mutated {
                state.tables_fc_1fc[kind.table()]
                    .storage
                    .as_mut()
                    .unwrap()
                    .slots[0] = self.replacement;
                self.mutated = true;
            }
            Ok(name)
        }
        fn allocate_table(
            &mut self,
            state: &mut State,
            kind: Kind,
            capacity: u32,
        ) -> Result<Storage> {
            self.event(json!(["allocate", kind.0, capacity]));
            let identity = 201 + self.allocations;
            self.allocations += 1;
            if self.case["mutation"] == "allocate_count" && !self.mutated {
                state.tables_fc_1fc[kind.table()].count = 3;
                state.tables_fc_1fc[kind.table()].capacity = 1;
                self.mutated = true;
            }
            Ok(Storage {
                identity,
                slots: vec![None; capacity as usize],
            })
        }
        fn free_table(&mut self, state: &mut State, kind: Kind, storage: usize) -> Result<()> {
            self.event(json!(["free", kind.0, storage]));
            if self.case["mutation"] == "free_count" && !self.mutated {
                state.tables_fc_1fc[kind.table()].count = 0;
                self.mutated = true;
            }
            Ok(())
        }
    }
    impl attributes::Host for Binding<'_> {
        fn name(&mut self, s: &mut State, n: usize) -> Result<Option<Vec<u8>>> {
            Host::name(self, s, n)
        }
        fn value(&mut self, s: &mut State, n: usize) -> Result<Option<usize>> {
            Host::value(self, s, n)
        }
        fn text(&mut self, s: &mut State, n: usize) -> Result<Vec<u8>> {
            Host::text(self, s, n)
        }
        fn integer(&mut self, _: &mut State, _: Option<usize>) -> Result<i32> {
            anyhow::bail!("Unexpected numeric attribute")
        }
        fn number(&mut self, _: &mut State, _: usize) -> Result<f64> {
            anyhow::bail!("Unexpected numeric attribute")
        }
        fn unit_scale(&mut self, _: &mut State) -> Result<Option<f32>> {
            anyhow::bail!("Unexpected scale")
        }
        fn find_database(&mut self, _: &mut State, _: usize, _: usize) -> Result<Option<usize>> {
            anyhow::bail!("Unexpected database")
        }
        fn register_database_callback(&mut self, _: &mut State, _: usize) -> Result<()> {
            anyhow::bail!("Unexpected database callback")
        }
        fn modifier_attribute(&mut self, s: &mut State, n: usize) -> Result<Option<bool>> {
            initialize(s, n, self)
        }
    }
    impl DefaultsHost for Binding<'_> {
        fn default_class(&self, kind: Kind) -> Result<usize> { Ok(80 + kind.0) }
        fn object_class(&mut self, state: &mut State, kind: Kind, object: usize) -> Result<usize> {
            self.event(json!(["object_class", kind.0, object]));
            if !self.mutated {
                if self.case["mutation"] == "class_slot" {
                    state.tables_fc_1fc[kind.table()].storage.as_mut().unwrap().slots[0] = self.replacement;
                    self.mutated = true;
                } else if self.case["mutation"] == "class_count" {
                    state.tables_fc_1fc[kind.table()].count = 0;
                    self.mutated = true;
                }
            }
            Ok(match self.case["mode"].as_str() {
                Some("exact") => 80 + kind.0,
                Some("ancestor") => 180 + kind.0,
                _ => 280 + kind.0,
            })
        }
        fn parent_class(&mut self, _: &mut State, class: usize) -> Result<Option<usize>> {
            Ok((180..193).contains(&class).then(||class-100))
        }
        fn construct_default(&mut self, _: &mut State, kind: Kind, _: usize, name: &[u8]) -> Result<Option<usize>> {
            self.event(json!(["construct_default",kind.0,std::str::from_utf8(name)?]));
            Ok((self.case["null_allocation"]!=true).then(||self.object("default")))
        }
        fn initialize_default(&mut self, state: &mut State, kind: Kind, object: usize) -> Result<bool> {
            ensure!(selected(state,kind)?==Some(object),"Default must be published before initialization");
            self.event(json!(["initialize_default",kind.0,object]));
            if self.case["mutation"]=="init_clear" {state.tables_fc_1fc[kind.table()].selected=Some(None);}
            Ok(self.case["accepted"]!=false)
        }
    }
    #[test]
    fn navigation_pathfinder_default_phase_matches_original_control() -> Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_pathfinder_defaults.json"))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c=&expected["input"];let kind=Kind(c["kind"].as_u64().unwrap_or(0) as usize);
            let mut s=state()?;
            let mut h=Binding{case:c,rows:vec![],objects:vec![],trace:vec![],allocations:0,mutated:false,replacement:None};
            for k in 0..13 {
                if (k!=kind.0 && c["all_missing"]!=true) || c["mode"]=="preselected" {
                    s.tables_fc_1fc[Kind(k).table()].selected=Some(Some(h.object("Selected")));
                }
            }
            let count=c["count"].as_u64().unwrap_or(0) as u32;
            let old=(0..count).map(|i|h.object(&format!("Old{i}"))).collect::<Vec<_>>();
            if c["mutation"]=="class_slot" {h.replacement=Some(h.object("Replacement"));}
            let capacity=c["capacity"].as_u64().unwrap_or(count as u64) as u32;
            let table=&mut s.tables_fc_1fc[kind.table()];table.count=count;table.capacity=capacity;
            if capacity!=0 {
                let mut slots=vec![None;capacity as usize];
                for (i,object) in old.into_iter().enumerate(){slots[i]=Some(object);}
                table.storage=Some(Storage{identity:200,slots});
            }
            initialize_defaults(&mut s,&mut h)?;
            let tables=s.tables_fc_1fc.iter().enumerate().map(|(i,t)|json!([i,{
                "allocation":t.storage.as_ref().map(|s|s.identity),"capacity":t.capacity,"count":t.count,
                "entries":(0..t.count).map(|n|t.storage.as_ref().unwrap().slots[n as usize].filter(|&p|p!=0)).collect::<Vec<_>>(),
                "selected":t.selected.unwrap()}])).collect::<Vec<_>>();
            assert_eq!(json!({"trace":h.trace,"tables":tables,"objects":h.objects}),
                json!({"trace":expected["trace"],"tables":expected["tables"],"objects":expected["objects"]}),"{c}");
        }
        Ok(())
    }
    #[test]
    fn navigation_pathfinder_null_default_is_not_a_ready_service() -> Result<()> {
        let c=json!({"null_allocation":true});let mut s=state()?;
        let mut h=Binding{case:&c,rows:vec![],objects:vec![],trace:vec![],allocations:0,mutated:false,replacement:None};
        assert!(initialize_defaults(&mut s,&mut h).unwrap_err().to_string().contains("allocation is null"));
        assert_eq!(s.tables_fc_1fc[0].selected,Some(None));
        assert_eq!(s.tables_fc_1fc[0].count,0);
        assert_eq!(h.trace,vec![json!(["construct_default",0,"default"])]);
        Ok(())
    }
    #[test]
    fn navigation_pathfinder_modifiers_match_original_all_categories_defaults_growth_and_reentrant_callbacks(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_pathfinder_modifiers.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let kind = Kind(c["kind"].as_u64().unwrap() as usize);
            let mut s = state()?;
            let mut h = Binding {
                case: c,
                rows: vec![],
                objects: vec![],
                trace: vec![],
                allocations: 0,
                mutated: false,
                replacement: None,
            };
            if let Some(nodes) = c["nodes"].as_array() {
                for node in nodes {
                    h.rows.push(Row {
                        name: node["name"].as_str().unwrap().into(),
                        identifier: node["identifier"].as_str().map(str::to_string),
                        value: node["value"].as_str().map(str::to_string),
                        children: node["children"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_u64().unwrap() as usize)
                            .collect(),
                        class_child: node["class_child"].as_u64().map(|n| n as usize),
                    });
                }
            } else if c["op"] == "instance" {
                h.add_instance("New");
            } else {
                h.add(
                    &format!("Modifiers_{}", std::str::from_utf8(Kind::NAMES[kind.0])?),
                    None,
                    None,
                );
                if c["default_first"] == true {
                    let child = h.add("default", None, c["default"].as_str().map(str::to_string));
                    h.rows[0].children.push(child);
                }
                if let Some(items) = c["items"].as_array() {
                    for name in items {
                        let child = h.add_instance(name.as_str().unwrap());
                        h.rows[0].children.push(child);
                    }
                }
                if c.as_object().unwrap().contains_key("default") && c["default_first"] != true {
                    let child = h.add("default", None, c["default"].as_str().map(str::to_string));
                    h.rows[0].children.push(child);
                }
                if c["bad_child"] == true {
                    let child = h.add("Unknown", None, None);
                    h.rows[0].children.push(child);
                }
            }
            let mut old = vec![];
            if let Some(existing) = c["existing"].as_array() {
                for name in existing {
                    old.push(h.object(name.as_str().unwrap()));
                }
            }
            if c["mutation"] == "object_name_replace" {
                h.replacement = Some(h.object("Replacement"));
            }
            let capacity = c["capacity"].as_u64().unwrap_or(old.len() as u64) as u32;
            let table = &mut s.tables_fc_1fc[kind.table()];
            table.capacity = capacity;
            table.count = old.len() as u32;
            if capacity != 0 {
                let mut slots = vec![None; capacity as usize];
                for (i, &obj) in old.iter().enumerate() {
                    slots[i] = Some(obj);
                }
                table.storage = Some(Storage {
                    identity: 200,
                    slots,
                });
            }
            table.selected = Some(c["selected"].as_u64().map(|i| old[i as usize]));
            let result = attributes::initialize(&mut s, 0, &mut h)?;
            let tables=s.tables_fc_1fc.iter().enumerate().filter_map(|(i,t)| {
                let entries=(0..t.count).map(|n|entry(&s,Kind([0,1,2,4,5,6,3,7,8,9,10,11,12][i]),n).unwrap()).collect::<Vec<_>>();
                let v=json!({"allocation":t.storage.as_ref().map(|s|s.identity),"capacity":t.capacity,"count":t.count,
                    "entries":entries,"selected":t.selected.unwrap()});
                (t.storage.is_some() || t.capacity!=0 || t.count!=0 || t.selected.flatten().is_some()).then(||json!([i,v]))
            }).collect::<Vec<_>>();
            assert_eq!(
                json!({"result":result,"trace":h.trace,"tables":tables,"objects":h.objects}),
                json!({"result":expected["result"],"trace":expected["trace"],"tables":expected["tables"],"objects":expected["objects"]}),
                "{c}"
            );
        }
        Ok(())
    }
}
