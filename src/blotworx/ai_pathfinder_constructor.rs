//! Concrete Mpd::CPathFinder48f0cc and service463d58 construction. This is
//! construction-stage storage, not initialized navigation or a ready service.
use super::{
    ai_navigation_action::{Action, Registry},
    ai_navigation_construction::Constructed,
};
use anyhow::{ensure, Context, Result};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Reference {
    External(usize),
    /// The current pathfinder's canonical inline object at+3e0. This is not
    ///an invented numeric address or a separately allocated duplicate.
    InlineAcceptance,
}
pub struct Base {
    pub owner: usize,
    pub action_identity: usize,
    pub action: Action,
    pub references_c_10: [Option<Option<Reference>>; 2],
    pub scales_14_18_1c: [Option<f32>; 3],
    pub flag_20: u8,
    pub budget_24: Option<i32>,
}
/// Borrow the actual disjoint fields while base-brain initialization owns a
/// mutable base record. No temporary/cloned Constructed parent is needed.
#[derive(Clone,Copy)]
pub struct Owner<'a> {
    pub brain:&'a super::ai_navigation_brain::Brain,
    pub base:&'a super::ai_navigation_brain_base::State,
    pub action:Option<&'a Action>,
}
impl<'a> From<&'a Constructed> for Owner<'a> {
    fn from(owner:&'a Constructed)->Self {Self{brain:&owner.brain,base:&owner.base,action:owner.action.as_ref()}}
}
pub trait BaseHost {
    fn unit_scale(&mut self, base: &mut Base) -> Result<Option<f32>>;
    fn profiling_enabled(&mut self, base: &mut Base) -> Result<bool>;
    fn register_budget(&mut self, base: &mut Base, name: &str) -> Result<i32>;
}
impl Base {
    fn construct(
        owner: Owner<'_>,
        action_identity: usize,
        registry: &Registry,
        host: &mut impl BaseHost,
    ) -> Result<Self> {
        ensure!(
            owner.base.action.is_some() && owner.action.is_some(),
            "Pathfinder service requires its owner's real ActionClass instance"
        );
        //463d9c/463da8 query ActionClass and invoke its factory. A fresh action
        //is owned by the service; requests are not copied from its owner.
        let mut base = Self {
            owner: owner.brain.identity,
            action_identity,
            action: Action::new(registry)?,
            references_c_10: [None; 2],
            scales_14_18_1c: [None; 3],
            flag_20: 1,
            budget_24: None,
        };
        base.scales_14_18_1c[0] = Some(host.unit_scale(&mut base)?.unwrap_or(1.));
        base.scales_14_18_1c[1] = Some(0.5 * host.unit_scale(&mut base)?.unwrap_or(1.));
        base.scales_14_18_1c[2] = Some(host.unit_scale(&mut base)?.unwrap_or(1.));
        base.references_c_10 = [Some(None); 2];
        base.budget_24 = Some(-1);
        if host.profiling_enabled(&mut base)? {
            base.budget_24 = Some(host.register_budget(&mut base, "FindNextMove")?);
        }
        Ok(base)
    }
}
#[derive(Clone, Copy)]
pub enum PointAttribute {
    Inline254,
}
pub struct Point {
    pub position: [f32; 3],
    pub attributes: [Option<PointAttribute>; 16],
}
impl Default for Point {
    fn default() -> Self {
        Self {
            position: [0.; 3],
            attributes: [None; 16],
        }
    }
}
pub struct CellAttribute {
    pub words_04_1c: [u32; 7],
}
#[derive(Default)]
pub struct Counter {
    pub created: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Graph,
    PathObjects,
}
pub trait Host: BaseHost {
    fn world_service(&mut self, state: &mut State, kind: Service) -> Result<Option<usize>>;
    fn point_attribute_index(&mut self, state: &mut State) -> Result<i32>;
}
pub struct State {
    pub reset_fields:super::ai_pathfinder_reset::Fields,
    pub identity: usize,
    pub base: Base,
    // Written by48fec4/492440, not by the constructor.
    pub mode_28: Option<u32>,
    pub distances_2c_34: [Option<f32>; 2],
    pub precision_cc: Option<u8>,
    pub word_30: u32,
    pub word_38: u32,
    pub word_44: i32,
    pub word_64: u32,
    pub word_70: u32,
    pub words_88_8c: [i32; 2],
    pub words_90_94_98: [u32; 3],
    pub words_a4_a8_ac: [u32; 3],
    pub word_ec: i32,
    pub references_3c_4c_50_54_58_60_6c: [Option<usize>; 7],
    pub references_b0_b4_c8_d0_d4_f0: [Option<usize>; 6],
    pub reference_f8: Option<Option<usize>>,
    /// Canonical owned object addressed by reference_f8; no duplicate fields.
    pub lpf: Option<Box<super::ai_lpf_module::State>>,
    // Defaults are unwritten until after resource lookup/point publication.
    pub tables_fc_1fc: [super::ai_pathfinder_modifiers::Table; 13],
    pub points_200_274_2c4_314_378: [Point; 5],
    pub attribute_254: CellAttribute,
    pub flag_48: u8,
    pub flags_9c_9d: [u8; 2],
    pub flag_e4: Option<u8>,
    pub flags_f4_f5: [u8; 2],
    pub flag_250: Option<u8>,
    pub flag_41c: u8,
    pub vector_d8: [f32; 3],
    pub vector_3cc: [f32; 3],
    pub reference_3e4: Option<usize>,
    pub word_364: Option<u32>,
    pub word_3dc: Option<i32>,
    pub word_3e0: Option<u32>,
    pub scalars_3e8_3ec_408_414: [Option<f32>; 4],
    pub scalar_40c: Option<f32>,
    pub vector_3f4: [f32; 3],
    pub word_420: u32,
    pub word_424: u32,
    pub scalar_428: f32,
    pub word_42c: Option<u32>,
    pub word_430: Option<u32>,
}
impl State {
    ///18bb8c ->482ebc from the canonical parent and live graph-manager table.
    pub fn graph_provider(&self,m:&super::ai_graph_link_passes::Memory)->Result<Option<usize>> {
        let handle=if let Some(source)=self.references_3c_4c_50_54_58_60_6c[2] {
            let source=u32::try_from(source)?;
            let index=u32::try_from(self.references_3c_4c_50_54_58_60_6c[0].context("Source graph index is unwritten")?)?;
            if index>=m.word(source.wrapping_add(0x30))?{return Ok(None)}
            let records=m.word(source.wrapping_add(0x28))?;
            let record=records.wrapping_add(index.wrapping_mul(0x28));
            if m.byte(record.wrapping_add(0x24))?==0{return Ok(None)}
            m.word(record.wrapping_add(4))?
        }else{self.word_70};
        Ok((handle!=0).then_some(handle as usize))
    }
    pub fn construct(
        identity: usize,
        owner: &Constructed,
        action_identity: usize,
        registry: &Registry,
        counter: &mut Counter,
        host: &mut impl Host,
    ) -> Result<Self> {
        Self::construct_for(identity,owner.into(),action_identity,registry,counter,host)
    }
    pub fn construct_for(
        identity:usize,owner:Owner<'_>,action_identity:usize,registry:&Registry,
        counter:&mut Counter,host:&mut impl Host,
    )->Result<Self> {
        let base = Base::construct(owner, action_identity, registry, host)?;
        counter.created = counter.created.wrapping_add(1);
        let mut state = Self {
            identity,
            base,
            reset_fields:Default::default(),
            mode_28: None,
            distances_2c_34: [None; 2],
            precision_cc: None,
            word_30: 64,
            word_38: 32,
            word_44: -1,
            word_64: 0,
            word_70: 0,
            words_88_8c: [-1; 2],
            words_90_94_98: [0, 1024, 0],
            words_a4_a8_ac: [32, 0, 0],
            word_ec: -1,
            references_3c_4c_50_54_58_60_6c: [None; 7],
            references_b0_b4_c8_d0_d4_f0: [None; 6],
            reference_f8: None,
            lpf: None,
            tables_fc_1fc: std::array::from_fn(|_| {
                super::ai_pathfinder_modifiers::Table::default()
            }),
            points_200_274_2c4_314_378: std::array::from_fn(|_| Point::default()),
            attribute_254: CellAttribute {
                words_04_1c: [u32::MAX, u32::MAX, u32::MAX, u32::MAX, 0, u32::MAX, 0],
            },
            flag_48: 0,
            flags_9c_9d: [0; 2],
            flag_e4: None,
            flags_f4_f5: [1, 0],
            flag_250: None,
            flag_41c: 0,
            vector_d8: [f32::MAX; 3],
            vector_3cc: [0.; 3],
            reference_3e4: None,
            word_364: None,
            word_3dc: None,
            word_3e0: Some(0x517490),
            scalars_3e8_3ec_408_414: [None; 4],
            scalar_40c: None,
            vector_3f4: [0.; 3],
            word_420: 10,
            word_424: 500,
            scalar_428: 1.,
            word_42c: None,
            word_430: None,
        };
        state.references_3c_4c_50_54_58_60_6c[1] =
            host.world_service(&mut state, Service::Graph)?;
        state.references_3c_4c_50_54_58_60_6c[4] =
            host.world_service(&mut state, Service::PathObjects)?;
        state.flag_250 = Some(1);
        let index = host.point_attribute_index(&mut state)?;
        ensure!(
            (0..16).contains(&index),
            "Pathfinder inline point attribute metadata is outside its native fixed table"
        );
        state.points_200_274_2c4_314_378[0].attributes[index as usize] =
            Some(PointAttribute::Inline254);
        state.flag_e4 = Some(0);
        for header in &mut state.tables_fc_1fc {
            header.selected = Some(None);
        }
        state.word_42c = Some(256);
        state.word_430 = Some(0);
        state.reference_f8 = Some(None);
        //Unwritten fields from the native0x434-byte object are deliberately
        //not manufactured as zero. Definition initialization is still required.
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::super::ai_navigation_action::{Class, Kind};
    use super::super::ai_navigation_construction::{ActionClass, ActionFactory, Layout};
    use super::*;
    use serde_json::{json, Value};
    struct Binding<'a> {
        case: &'a Value,
        world: Option<u32>,
        trace: Vec<Value>,
    }
    impl BaseHost for Binding<'_> {
        fn unit_scale(&mut self, _: &mut Base) -> Result<Option<f32>> {
            self.trace.push(json!(["context"]));
            Ok(self
                .world
                .map(|_| f32::from_bits(self.case["scale"].as_u64().unwrap() as u32)))
        }
        fn profiling_enabled(&mut self, _: &mut Base) -> Result<bool> {
            self.trace.push(json!(["profiler"]));
            Ok(self.case["profiling"] == true)
        }
        fn register_budget(&mut self, _: &mut Base, name: &str) -> Result<i32> {
            self.trace.push(json!(["profiler"]));
            self.trace.push(json!(["budget", name]));
            Ok(self.case["budget"].as_i64().unwrap() as i32)
        }
    }
    impl Host for Binding<'_> {
        fn world_service(&mut self, s: &mut State, kind: Service) -> Result<Option<usize>> {
            let name = if kind == Service::Graph {
                "graph"
            } else {
                "path_objects"
            };
            let world = self.world;
            assert!(s.tables_fc_1fc.iter().all(|h| h.selected.is_none()));
            self.trace.push(json!(["context"]));
            self.trace.push(json!(["service", name, world]));
            self.trace.push(json!([
                "partial",
                s.flag_250,
                s.flag_e4,
                s.tables_fc_1fc[0].selected.map(|v| v.unwrap_or(0)),
                s.reference_f8.map(|r| r.unwrap_or(0))
            ]));
            if self.case["mutation"] == name {
                s.tables_fc_1fc[0].storage = Some(super::super::ai_pathfinder_modifiers::Storage {
                    identity: if kind == Service::Graph { 123 } else { 321 },
                    slots: Vec::new(),
                });
                s.flag_250 = Some(9);
                s.flag_e4 = Some(3);
                s.base.scales_14_18_1c[0] = Some(2.5);
                self.world = Some(2);
            }
            Ok(
                (world.is_some() && self.case[name] == true).then_some(if kind == Service::Graph {
                    80
                } else {
                    90
                }),
            )
        }
        fn point_attribute_index(&mut self, _: &mut State) -> Result<i32> {
            Ok(self.case["point_index"].as_i64().unwrap() as i32)
        }
    }
    fn bytes(state: &State) -> Vec<(usize, u8)> {
        let mut image = std::collections::BTreeMap::new();
        {
            let mut put = |at: usize, value: u32| {
                for (i, b) in value.to_be_bytes().into_iter().enumerate() {
                    image.insert(at + i, b);
                }
            };
            put(0, 0x51fb58);
            put(4, state.base.owner as u32);
            put(8, state.base.action_identity as u32);
            for (i, r) in state.base.references_c_10.iter().enumerate() {
                if let Some(r) = r {
                    put(0xc + i * 4, match r {
                        None=>0,
                        Some(Reference::External(id))=>*id as u32,
                        Some(Reference::InlineAcceptance)=>panic!("Constructor must not attach its inline attribute before48fec4"),
                    });
                }
            }
            for (i, s) in state.base.scales_14_18_1c.iter().enumerate() {
                if let Some(s) = s {
                    put(0x14 + i * 4, s.to_bits());
                }
            }
            if let Some(b) = state.base.budget_24 {
                put(0x24, b as u32);
            }
            for (at, w) in [
                (0x30, state.word_30),
                (0x38, state.word_38),
                (0x44, state.word_44 as u32),
                (0x64, state.word_64),
                (0x70, state.word_70),
                (0xec, state.word_ec as u32),
                (0x420, state.word_420),
                (0x424, state.word_424),
                (0x428, state.scalar_428.to_bits()),
            ] {
                put(at, w);
            }
            for (start, words) in [
                (0x88, state.words_88_8c.map(|w| w as u32).to_vec()),
                (0x90, state.words_90_94_98.to_vec()),
                (0xa4, state.words_a4_a8_ac.to_vec()),
            ] {
                for (i, w) in words.into_iter().enumerate() {
                    put(start + i * 4, w);
                }
            }
            for (at, r) in [0x3c, 0x4c, 0x50, 0x54, 0x58, 0x60, 0x6c]
                .into_iter()
                .zip(state.references_3c_4c_50_54_58_60_6c)
            {
                put(at, r.unwrap_or(0) as u32);
            }
            for (at, r) in [0xb0, 0xb4, 0xc8, 0xd0, 0xd4, 0xf0]
                .into_iter()
                .zip(state.references_b0_b4_c8_d0_d4_f0)
            {
                put(at, r.unwrap_or(0) as u32);
            }
            if let Some(r) = state.reference_f8 {
                put(0xf8, r.unwrap_or(0) as u32);
            }
            for (i, header) in state.tables_fc_1fc.iter().enumerate() {
                put(
                    0xfc + i * 20,
                    header.storage.as_ref().map_or(0, |s| s.identity as u32),
                );
                put(0x100 + i * 20, header.word_04);
                put(0x104 + i * 20, header.capacity);
                put(0x108 + i * 20, header.count);
                if let Some(selected) = header.selected {
                    put(0x10c + i * 20, selected.unwrap_or(0) as u32);
                }
            }
            for (at, point) in [0x200, 0x274, 0x2c4, 0x314, 0x378]
                .into_iter()
                .zip(&state.points_200_274_2c4_314_378)
            {
                put(at, 0x4ec1b0);
                for (i, v) in point.position.iter().enumerate() {
                    put(at + 4 + i * 4, v.to_bits());
                }
                for (i, attribute) in point.attributes.iter().enumerate() {
                    put(at + 16 + i * 4, attribute.map_or(0, |_| 0x254));
                }
            }
            put(0x254, 0x4ec160);
            for (i, w) in state.attribute_254.words_04_1c.into_iter().enumerate() {
                put(0x258 + i * 4, w);
            }
            for (at, vector) in [
                (0xd8, state.vector_d8),
                (0x3cc, state.vector_3cc),
                (0x3f4, state.vector_3f4),
            ] {
                for (i, v) in vector.into_iter().enumerate() {
                    put(at + i * 4, v.to_bits());
                }
            }
            if let Some(word) = state.word_3e0 { put(0x3e0, word); }
            put(0x3e4, state.reference_3e4.unwrap_or(0) as u32);
            if let Some(w) = state.word_42c {
                put(0x42c, w);
            }
            if let Some(w) = state.word_430 {
                put(0x430, w);
            }
        }
        for (at, b) in [
            (0x20, Some(state.base.flag_20)),
            (0x48, Some(state.flag_48)),
            (0x9c, Some(state.flags_9c_9d[0])),
            (0x9d, Some(state.flags_9c_9d[1])),
            (0xe4, state.flag_e4),
            (0xf4, Some(state.flags_f4_f5[0])),
            (0xf5, Some(state.flags_f4_f5[1])),
            (0x250, state.flag_250),
            (0x41c, Some(state.flag_41c)),
        ] {
            if let Some(b) = b {
                image.insert(at, u8::from(b));
            }
        }
        image.into_iter().collect()
    }
    #[test]
    fn navigation_pathfinder_constructor_matches_original_owned_action_resources_defaults_and_partial_writes(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_pathfinder_constructor.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let registry = Registry {
                count: c["count"].as_u64().unwrap() as usize,
                heading: Class {
                    index: c["heading"].as_i64().unwrap() as i32,
                    constructor: true,
                },
                speed: Class {
                    index: c["speed"].as_i64().unwrap() as i32,
                    constructor: true,
                },
            };
            let mut owner = Constructed::new(
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
                    attributes: registry,
                },
            )?;
            owner
                .action
                .as_mut()
                .unwrap()
                .request(Kind::Heading, 123.)?;
            owner.action.as_mut().unwrap().request(Kind::Speed, 5.)?;
            let registry = Registry {
                count: c["count"].as_u64().unwrap() as usize,
                heading: Class {
                    index: c["heading"].as_i64().unwrap() as i32,
                    constructor: true,
                },
                speed: Class {
                    index: c["speed"].as_i64().unwrap() as i32,
                    constructor: true,
                },
            };
            let mut counter = Counter {
                created: c["counter"].as_u64().unwrap() as u32,
            };
            let mut binding = Binding {
                case: c,
                world: (c["world"] == true).then_some(1),
                trace: Vec::new(),
            };
            let state = State::construct(50, &owner, 40, &registry, &mut counter, &mut binding)?;
            assert_eq!(state.identity, 50);
            assert_eq!(json!(binding.trace), expected["trace"], "trace:{c}");
            assert_eq!(
                counter.created,
                expected["counter"].as_u64().unwrap() as u32
            );
            let baseline: Vec<(usize, u8)> =
                serde_json::from_value(fixture["initial_bytes"].clone())?;
            let mut expected_bytes = baseline
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>();
            let overrides: Vec<(usize, u8)> =
                serde_json::from_value(expected["byte_overrides"].clone())?;
            for (offset, value) in overrides {
                assert!(
                    expected_bytes.contains_key(&offset),
                    "Override changes the initialized-byte mask"
                );
                expected_bytes.insert(offset, value);
            }
            assert_eq!(
                bytes(&state),
                expected_bytes.into_iter().collect::<Vec<_>>(),
                "initialized fields:{c}"
            );
            assert_eq!(
                json!(state
                    .base
                    .action
                    .attributes
                    .iter()
                    .map(|s| s.as_ref().map(|s| s.kind))
                    .collect::<Vec<_>>()),
                expected["slots"]
            );
            for (kind, key) in [(Kind::Heading, "heading"), (Kind::Speed, "speed")] {
                let s = state.base.action.scalar(kind)?;
                assert_eq!(
                    json!({"changed":s.changed,"value":s.value_bits}),
                    expected[key]
                );
                let original = owner.action.as_ref().unwrap().scalar(kind)?;
                assert_eq!(original.changed, 1);
                assert_eq!(
                    original.value_bits,
                    if kind == Kind::Heading {
                        123f32.to_bits()
                    } else {
                        5f32.to_bits()
                    }
                );
            }
        }
        Ok(())
    }
}
