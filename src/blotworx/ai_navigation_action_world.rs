//! Concrete18cef0 controller-output services. The owning navigation runtime
//! must supply real entity/brain/data lookup; this does not fabricate them.
use super::{
    ai_navigation_action::{Action, Host, Lookup},
    assets::World,
};
use super::{
    ai_navigation_brain::Brain, ai_navigation_construction::Constructed,
    ai_navigation_entity_base::Entity,
};
use anyhow::{Context, Result};
impl World {
    /// Consume owned construction-stage objects. This is also usable during
    /// initialization; it does not register them as a ready navigation agent.
    pub(super) fn apply_constructed_navigation_action(
        &mut self,
        entity: &Entity,
        navigation: &mut Constructed,
    ) -> Result<()> {
        anyhow::ensure!(
            navigation.base.owner == Some(entity.identity)
                && navigation.brain.owner_40 == Some(entity.identity),
            "Navigation action owner differs from its constructed entity"
        );
        let action = navigation
            .action
            .as_mut()
            .context("Navigation brain has no constructed action")?;
        anyhow::ensure!(
            navigation.base.action.is_some(),
            "Navigation action ownership handle absent"
        );
        //The inline pair's first word is the gameplay owner (190ac4/190ea8).
        //The second word remains owned, without assigning unverified semantics.
        let owner = entity.user_data[0];
        let actor = self
            .entities
            .get(owner)
            .context("Navigation entity gameplay owner absent")?;
        let count = actor
            .components
            .iter()
            .filter(|c| c.class_name == "BasicAIControllerComponent")
            .count();
        anyhow::ensure!(
            count <= 1,
            "Navigation action requires native component-slot arbitration"
        );
        self.apply_navigation_action(
            action,
            entity.identity,
            &mut OwnedLookup {
                entity,
                brain: &mut navigation.brain,
                controller: (count == 1).then_some(owner),
            },
        )
    }
    pub(super) fn apply_navigation_action(
        &mut self,
        action: &mut Action,
        entity: usize,
        lookup: &mut impl Lookup,
    ) -> Result<()> {
        action.apply(
            entity,
            &mut Binding {
                world: self,
                lookup,
            },
        )
    }
}

struct OwnedLookup<'a> {
    entity: &'a Entity,
    brain: &'a mut Brain,
    controller: Option<usize>,
}
impl Lookup for OwnedLookup<'_> {
    fn navigation_brain(&mut self, _: &mut Action, entity: usize) -> Result<Option<usize>> {
        anyhow::ensure!(
            entity == self.entity.identity,
            "Navigation entity identity differs"
        );
        if let Some(brain) = self.entity.common.brain {
            anyhow::ensure!(
                brain == self.brain.identity,
                "Navigation entity references a different owned brain"
            );
        }
        Ok(self.entity.common.brain)
    }
    fn is_base_ai_brain(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
        Ok(brain == self.brain.identity)
    }
    fn entity_data(&mut self, _: &mut Action, entity: usize) -> Result<Option<usize>> {
        anyhow::ensure!(
            entity == self.entity.identity,
            "Navigation inline data owner differs"
        );
        Ok(Some(entity))
    }
    fn basic_controller(&mut self, _: &mut Action, data: Option<usize>) -> Result<Option<usize>> {
        anyhow::ensure!(
            data == Some(self.entity.identity),
            "Navigation inline data binding differs"
        );
        Ok(self.controller)
    }
    fn heading_blocked(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
        anyhow::ensure!(
            brain == self.brain.identity,
            "Navigation heading brain differs"
        );
        Ok(self.brain.flags_58_5b[3])
    }
    fn speed_blocked(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
        anyhow::ensure!(
            brain == self.brain.identity,
            "Navigation speed brain differs"
        );
        Ok(self.brain.flags_58_5b[2])
    }
}

#[cfg(test)]
mod tests {
    use super::super::ai_navigation_action::{Class, Kind, Registry};
    use super::super::{
        ai_navigation_brain::Brain,
        ai_navigation_construction::{ActionClass, ActionFactory, Constructed, Layout},
    };
    use super::*;
    struct Boundary<'a> {
        owner: usize,
        brain: Option<&'a mut Brain>,
        basic_queries: u32,
    }
    impl Lookup for Boundary<'_> {
        fn navigation_brain(&mut self, _: &mut Action, entity: usize) -> Result<Option<usize>> {
            assert_eq!(entity, 10);
            Ok(self.brain.as_ref().map(|b| b.identity))
        }
        fn is_base_ai_brain(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
            Ok(self.brain.as_ref().is_some_and(|b| b.identity == brain))
        }
        fn entity_data(&mut self, _: &mut Action, entity: usize) -> Result<Option<usize>> {
            assert_eq!(entity, 10);
            Ok(Some(40))
        }
        fn basic_controller(
            &mut self,
            _: &mut Action,
            data: Option<usize>,
        ) -> Result<Option<usize>> {
            assert_eq!(data, Some(40));
            self.basic_queries += 1;
            Ok(Some(self.owner))
        }
        fn heading_blocked(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
            assert_eq!(brain, 20);
            Ok(self
                .brain
                .as_ref()
                .context("Navigation brain absent")?
                .flags_58_5b[3])
        }
        fn speed_blocked(&mut self, _: &mut Action, brain: usize) -> Result<bool> {
            assert_eq!(brain, 20);
            Ok(self
                .brain
                .as_ref()
                .context("Navigation brain absent")?
                .flags_58_5b[2])
        }
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gus_navigation_requests_reach_actual_controller_inputs_without_fabricating_an_agent(
    ) -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/DBC_Start_1st.Level",
            &egui::Context::default(),
        )?;
        let owner = world
            .scripts
            .resolve_name("dbc_1st_gus_tut.Gremlin_Gus 01")
            .context("Original Gus absent")?;
        let index = world.entities[owner]
            .components
            .iter()
            .position(|c| c.class_name == "BasicAIControllerComponent")
            .context("Original BasicAI absent")?;
        world.apply_basic_ai_disabled(owner, index, false)?;
        let maximum = world.entities[owner]
            .components
            .iter()
            .find(|c| c.class_name == "BasicMotionControllerComponent")
            .and_then(|c| c.properties.iter().find(|p| p.name == "Max Move Speed"))
            .and_then(|p| p.value.as_f64())
            .context("Original maximum speed absent")? as f32;
        anyhow::ensure!(
            maximum > 0.,
            "Original Gus speed must make this a nonzero input check"
        );
        world.frame_clock.publish(true, true);
        use super::super::ai_navigation_entity as entity_types;
        let entity_registry = entity_types::Registry {
            count: 5,
            diameter: entity_types::Class {
                index: 0,
                constructor: Some(entity_types::Kind::Diameter),
            },
            height: entity_types::Class {
                index: 2,
                constructor: Some(entity_types::Kind::Height),
            },
            maximum_speed: entity_types::Class {
                index: 4,
                constructor: Some(entity_types::Kind::MaximumSpeed),
            },
        };
        let mut owned = world.construct_navigation_entity(
            10,
            Some(b"Gus"),
            [owner, 0],
            &entity_registry,
            None,
        )?;
        assert_eq!(
            owned
                .scalar(entity_types::Kind::MaximumSpeed, &entity_registry)?
                .value_bits,
            maximum.to_bits()
        );
        assert!(owned.entity.radius > 0.);
        assert_eq!(
            owned
                .scalar(entity_types::Kind::Diameter, &entity_registry)?
                .value_bits,
            (2. * owned.entity.radius).to_bits()
        );
        assert_eq!(
            owned.entity.height.to_bits(),
            world.character_physics[&owner]
                .as_ref()
                .map_err(|e| anyhow::anyhow!("Gus character body unavailable: {e}"))?
                .navigation_height
                .to_bits()
        );
        use super::super::ai_navigation_attribute_query as attribute_query;
        struct ExactMetadata {
            indices: std::collections::BTreeMap<usize, i32>,
        }
        impl attribute_query::Host for ExactMetadata {
            fn class_index(&mut self, _: &mut Entity, class: usize) -> Result<i32> {
                self.indices
                    .get(&class)
                    .copied()
                    .context("Integration metadata index was not supplied")
            }
            fn attribute_class(&mut self, _: &mut Entity, _: usize) -> Result<usize> {
                anyhow::bail!("Exact attribute lookup unexpectedly queried a virtual Class")
            }
            fn parent_class(&mut self, _: &mut Entity, _: usize) -> Result<Option<usize>> {
                anyhow::bail!("Exact attribute lookup unexpectedly queried ancestry")
            }
        }
        let mut metadata = ExactMetadata {
            indices: std::collections::BTreeMap::from([
                (0x361b4, entity_registry.diameter.index),
                (0x36094, entity_registry.height.index),
                (0x360e4, entity_registry.maximum_speed.index),
                // Explicit empty slot in this supplied five-slot test layout.
                // This is not a guessed original CEntityLength index.
                (10000, 1),
            ]),
        };
        let scalar_count = owned.scalars.len();
        for (class, kind, value) in [
            (
                0x361b4,
                entity_types::Kind::Diameter,
                2. * owned.entity.radius,
            ),
            (0x36094, entity_types::Kind::Height, owned.entity.height),
            (0x360e4, entity_types::Kind::MaximumSpeed, maximum),
        ] {
            let attribute = attribute_query::query(&mut owned.entity, class, true, &mut metadata)?
                .context("Original Gus constructed scalar is absent")?;
            let scalar = owned
                .scalars
                .get(&attribute)
                .context("Attribute does not refer to its owned scalar")?;
            assert_eq!(scalar.kind, kind);
            assert_eq!(scalar.value_bits, value.to_bits());
        }
        assert_eq!(
            attribute_query::query(&mut owned.entity, 10000, true, &mut metadata)?,
            None
        );
        assert_eq!(owned.scalars.len(), scalar_count, "Query created a scalar");
        assert_eq!(
            owned.entity.attributes.len(),
            entity_registry.count,
            "Query resized the table"
        );
        let mut entity = owned.entity;
        use super::super::ai_navigation_definitions as definitions;
        struct NoProfileQueries;
        impl definitions::ProfileServices for NoProfileQueries {
            fn manager(&mut self, _: &mut Entity) -> Result<Option<usize>> {
                anyhow::bail!("Built-in Gus definition unexpectedly queries Profile manager")
            }
            fn find(&mut self, _: &mut Entity, _: usize, _: &[u8]) -> Result<Option<usize>> {
                anyhow::bail!("Unexpected Profile lookup")
            }
            fn attach(&mut self, _: &mut Entity, _: usize) -> Result<bool> {
                anyhow::bail!("Unexpected Profile attachment")
            }
            fn detach(&mut self, _: &mut Entity, _: usize) -> Result<()> {
                anyhow::bail!("Unexpected Profile detachment")
            }
        }
        let definition = world.character_physics[&owner]
            .as_ref()
            .map_err(|e| anyhow::anyhow!("Gus character body unavailable: {e}"))?
            .navigation_definition()
            .context("Original Gus navigation definition absent")?;
        assert_eq!(definition.name, "ShortThinBiped");
        let catalog = world.biped_navigation_catalog()?;
        let mut resolver = super::super::ai_navigation_resolver::Runtime {
            world: None,
            allocator: 0,
            fallback_allocator: 400,
        };
        let manager = super::super::ai_navigation_resolver::Manager {
            identity: 300,
            allocator: None,
            definition_root: Some(catalog.root),
        };
        let mut definitions=resolver.resolve(Some(&manager),&catalog.tree,Some(definition.name.as_bytes()))?
            .with_context(|| {
                let services=catalog.tree.find_tag(catalog.root,b"Services").ok().flatten();
                let names=services.into_iter().flat_map(|i|catalog.tree.nodes[i].children.iter())
                    .filter_map(|&i|catalog.tree.nodes[i].identifier.as_ref()).map(|s|String::from_utf8_lossy(s).into_owned()).collect::<Vec<_>>();
                format!("Original Gus definition did not resolve from actual database components; available pathfinders: {names:?}")
            })?;
        let brain = definitions
            .tree
            .find_tag(definitions.root, b"Brain")?
            .context("Resolved Gus brain absent")?;
        assert_eq!(
            definitions.tree.first_value(brain, b"Class")?,
            Some(b"JPSBaseAIBrain".as_slice())
        );
        let service = definitions
            .tree
            .find_tag(brain, b"Service")?
            .context("Resolved Gus pathfinder absent")?;
        assert_eq!(
            definitions.tree.first_value(service, b"Class")?,
            Some(b"Mpd::CPathFinder".as_slice())
        );
        assert!(definitions
            .tree
            .first_value(service, b"DataBase")?
            .is_some());
        let agents = definitions.tree.nodes[brain]
            .children
            .iter()
            .copied()
            .filter(|&i| definitions.tree.nodes[i].name.as_deref() == Some(b"Agent".as_slice()))
            .collect::<Vec<_>>();
        let classes = agents
            .iter()
            .map(|&i| definitions.tree.first_value(i, b"Class"))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(
            classes,
            vec![
                Some(b"JPSGotoAgent".as_slice()),
                Some(b"JPSWanderAgent".as_slice()),
                Some(b"Mpd::JPSFleeAgent".as_slice())
            ]
        );
        entity.common.definition = Some(definitions.root);
        assert!(entity.initialize_definition(
            definitions.root,
            &mut definitions::Binding {
                tree: &mut definitions.tree,
                profiles: &mut NoProfileQueries,
            }
        )?);
        assert_eq!(entity.profile_68, None);
        assert_eq!(entity.profile_node_70, None);
        assert!(entity.common.brain.is_none());
        let mut constructed = Constructed::new(
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
                attributes: Registry {
                    count: 5,
                    heading: Class {
                        index: 1,
                        constructor: true,
                    },
                    speed: Class {
                        index: 3,
                        constructor: true,
                    },
                },
            },
        )?;
        assert_eq!(constructed.base.owner, Some(10));
        assert_eq!(constructed.base.action, Some(30));
        use super::super::{
            ai_navigation_service_factory as service_factory,
            ai_pathfinder_constructor as pathfinder,
        };
        struct ConstructorServices {
            scale: f32,
            graph: Option<usize>,
            path_objects: Option<usize>,
            point_index: i32,
        }
        impl pathfinder::BaseHost for ConstructorServices {
            fn unit_scale(&mut self, _: &mut pathfinder::Base) -> Result<Option<f32>> {
                Ok(Some(self.scale))
            }
            fn profiling_enabled(&mut self, _: &mut pathfinder::Base) -> Result<bool> {
                Ok(false)
            }
            fn register_budget(&mut self, _: &mut pathfinder::Base, _: &str) -> Result<i32> {
                anyhow::bail!("Disabled constructor profiler unexpectedly registers a budget")
            }
        }
        impl pathfinder::Host for ConstructorServices {
            fn world_service(
                &mut self,
                _: &mut pathfinder::State,
                kind: pathfinder::Service,
            ) -> Result<Option<usize>> {
                Ok(match kind {
                    pathfinder::Service::Graph => self.graph,
                    pathfinder::Service::PathObjects => self.path_objects,
                })
            }
            fn point_attribute_index(&mut self, _: &mut pathfinder::State) -> Result<i32> {
                Ok(self.point_index)
            }
        }
        //Context/profiler/point metadata are explicit construction-stage
        //boundaries. Missing graph/path_objects services must not publish a ready agent.
        let mut context = ConstructorServices {
            scale: 1.,
            graph: None,
            path_objects: None,
            point_index: 3,
        };
        let mut counter = pathfinder::Counter::default();
        let registry = Registry {
            count: 5,
            heading: Class {
                index: 1,
                constructor: true,
            },
            speed: Class {
                index: 3,
                constructor: true,
            },
        };
        constructed
            .action
            .as_mut()
            .unwrap()
            .request(Kind::Heading, 45.)?;
        constructed
            .action
            .as_mut()
            .unwrap()
            .request(Kind::Speed, maximum * 0.25)?;
        let service_factory::Owned::Pathfinder(pathfinder) = service_factory::construct(
            &definitions.tree,
            service,
            50,
            &constructed,
            40,
            &registry,
            &mut counter,
            &mut context,
        )?
        .context("Original pathfinder Class was not constructed")?;
        assert_eq!(counter.created, 1);
        assert_eq!(pathfinder.base.owner, constructed.brain.identity);
        assert_ne!(
            pathfinder.base.action_identity,
            constructed.base.action.unwrap()
        );
        assert_eq!(pathfinder.base.action.scalar(Kind::Heading)?.value_bits, 0);
        assert_eq!(pathfinder.base.action.scalar(Kind::Speed)?.value_bits, 0);
        //Construct the actual authored IGoto class with its owned identifier.
        //This still precedes configuration/resource initialization/publication.
        use super::super::ai_goto_gap as gap;
        let goto_group = definitions
            .tree
            .find_tag(service, b"Modifiers_IGoto")?
            .context("Original Gus pathfinder has no IGoto modifiers")?;
        let goto_node = definitions
            .tree
            .find_tag(goto_group, b"IGoto")?
            .context("Original Gus IGoto definition absent")?;
        assert_eq!(
            definitions.tree.first_value(goto_node, b"Class")?,
            Some(b"Mpd::CGoto_GapDynamicAvoidance_v2".as_slice())
        );
        let goto_name = definitions.tree.nodes[goto_node]
            .identifier
            .as_deref()
            .context("Original Gus IGoto has no identifier")?;
        struct GapConstruction<'a> {
            owner: &'a Constructed,
            pathfinder: &'a pathfinder::State,
            scale: f32,
            tree: &'a super::super::ai_navigation_definitions::Tree,
            entity: &'a mut Entity,
            metadata: &'a mut ExactMetadata,
            gap_manager: Option<&'a super::super::ai_gap_manager::State>,
        }
        impl gap::Host for GapConstruction<'_> {
            fn unit_scale(&mut self, _: &mut gap::State) -> Result<Option<f32>> {
                Ok(Some(self.scale))
            }
            fn owner_entity(&mut self, state: &mut gap::State) -> Result<usize> {
                anyhow::ensure!(
                    state.pathfinder == self.pathfinder.identity
                        && self.pathfinder.base.owner == self.owner.brain.identity,
                    "Steering constructor follows a different owner chain"
                );
                self.owner
                    .base
                    .owner
                    .context("Constructed brain has no entity")
            }
            fn profiling_enabled(&mut self, _: &mut gap::State) -> Result<bool> {
                Ok(false)
            }
            fn register_budget(&mut self, _: &mut gap::State, _: &str) -> Result<i32> {
                anyhow::bail!("Disabled steering constructor profiler registered a budget")
            }
        }
        impl super::super::ai_goto_gap_attributes::Host for GapConstruction<'_> {
            fn name(&mut self, _: &mut gap::State, node: usize) -> Result<Option<usize>> {
                Ok(self.tree.nodes[node].name.as_ref().map(|_| node * 2 + 1))
            }
            fn value(&mut self, _: &mut gap::State, node: usize) -> Result<Option<usize>> {
                Ok(self.tree.nodes[node].value.as_ref().map(|_| node * 2 + 2))
            }
            fn text(&mut self, _: &mut gap::State, handle: usize) -> Result<Vec<u8>> {
                anyhow::ensure!(handle != 0, "Null tree string handle");
                let node = &self.tree.nodes[(handle - 1) / 2];
                if handle % 2 == 1 {
                    node.name.clone()
                } else {
                    node.value.clone()
                }
                .context("Retained source definition string disappeared")
            }
            fn number(&mut self, _: &mut gap::State, _: Option<usize>) -> Result<f64> {
                anyhow::bail!("Original Gus steering now requires an unsupplied numeric conversion")
            }
            fn integer(&mut self, _: &mut gap::State, _: Option<usize>) -> Result<i32> {
                anyhow::bail!("Original Gus steering now requires an unsupplied integer conversion")
            }
            fn diameter_attribute(
                &mut self,
                _: &mut gap::State,
                _: usize,
            ) -> Result<Option<usize>> {
                anyhow::bail!("Original Gus steering now requires an unsupplied diameter service")
            }
            fn diameter_value(&mut self, _: &mut gap::State, _: usize) -> Result<f32> {
                anyhow::bail!("Original Gus steering now requires an unsupplied diameter value")
            }
        }
        impl super::super::ai_navigation_modifier_init::Host<gap::State> for GapConstruction<'_> {
            fn child_count(&mut self, _: &mut gap::State, root: usize) -> Result<u32> {
                self.tree.nodes[root]
                    .children
                    .len()
                    .try_into()
                    .context("Too many native modifier children")
            }
            fn child(
                &mut self,
                _: &mut gap::State,
                root: usize,
                index: u32,
            ) -> Result<Option<usize>> {
                Ok(self.tree.nodes[root].children.get(index as usize).copied())
            }
            fn name(&mut self, _: &mut gap::State, node: usize) -> Result<Option<Vec<u8>>> {
                Ok(self.tree.nodes[node].name.clone())
            }
            fn initialize_attribute(
                &mut self,
                state: &mut gap::State,
                node: usize,
            ) -> Result<bool> {
                super::super::ai_goto_gap_attributes::initialize(state, node, self)
            }
        }
        impl super::super::ai_goto_gap_setup::Host for GapConstruction<'_> {
            fn world_service(
                &mut self,
                _: &mut gap::State,
                kind: super::super::ai_goto_gap_setup::Service,
            ) -> Result<Option<usize>> {
                Ok(match kind {
                    super::super::ai_goto_gap_setup::Service::Profiles => None,
                    super::super::ai_goto_gap_setup::Service::Gap => {
                        self.gap_manager.map(|manager| manager.identity)
                    }
                })
            }
            fn entity_attribute(
                &mut self,
                _: &mut gap::State,
                entity: usize,
                kind: super::super::ai_goto_gap_setup::Attribute,
            ) -> Result<Option<usize>> {
                anyhow::ensure!(
                    entity == self.entity.identity,
                    "Steering setup queries a different entity"
                );
                let class = match kind {
                    super::super::ai_goto_gap_setup::Attribute::Diameter => 0x361b4,
                    super::super::ai_goto_gap_setup::Attribute::Length => 0x360bc,
                    super::super::ai_goto_gap_setup::Attribute::Height => 0x36094,
                };
                attribute_query::query(self.entity, class, true, self.metadata)
            }
        }
        impl super::super::ai_navigation_modifier_init::GapHost for GapConstruction<'_> {
            fn setup(&mut self, state: &mut gap::State) -> Result<bool> {
                super::super::ai_goto_gap_setup::initialize(state, self)
            }
        }
        // Class-only configuration exercises verified manager defaults. This
        // is not evidence of global source-service admission or runtime updates.
        struct ManagerDefinition;
        impl super::super::ai_navigation_service_init::Host<super::super::ai_gap_manager::State>
            for ManagerDefinition
        {
            fn child_count(
                &mut self,
                _: &mut super::super::ai_gap_manager::State,
                _: usize,
            ) -> Result<u32> {
                Ok(1)
            }
            fn child(
                &mut self,
                _: &mut super::super::ai_gap_manager::State,
                _: usize,
                index: u32,
            ) -> Result<Option<usize>> {
                Ok((index == 0).then_some(0))
            }
            fn name(
                &mut self,
                _: &mut super::super::ai_gap_manager::State,
                _: usize,
            ) -> Result<Option<Vec<u8>>> {
                Ok(Some(b"Class".to_vec()))
            }
            fn initialize_attribute(
                &mut self,
                state: &mut super::super::ai_gap_manager::State,
                node: usize,
            ) -> Result<bool> {
                state.attribute(node, self)
            }
        }
        impl super::super::ai_gap_manager::Host for ManagerDefinition {
            fn value(
                &mut self,
                _: &mut super::super::ai_gap_manager::State,
                _: usize,
            ) -> Result<Option<usize>> {
                anyhow::bail!("Unexpected manager value query")
            }
            fn integer(
                &mut self,
                _: &mut super::super::ai_gap_manager::State,
                _: usize,
            ) -> Result<i32> {
                anyhow::bail!("Unexpected manager integer query")
            }
        }
        let mut gap_manager = super::super::ai_gap_manager::State::new(70);
        assert!(gap_manager.initialize(0, &mut ManagerDefinition)?);
        assert_eq!(gap_manager.pool.as_ref().unwrap().nodes.len(), 50);
        assert_eq!(gap_manager.pool.as_ref().unwrap().count, 1);
        // Explicit supplied test-layout registration; no claim about original
        // global CEntityLength index ordering, and no Length object is created.
        metadata.indices.remove(&10000);
        metadata.indices.insert(0x360bc, 1);
        let expected_diameter = entity.attributes[entity_registry.diameter.index as usize];
        let expected_height = entity.attributes[entity_registry.height.index as usize];
        let mut gap_context = GapConstruction {
            owner: &constructed,
            pathfinder: &pathfinder,
            scale: context.scale,
            tree: &definitions.tree,
            entity: &mut entity,
            metadata: &mut metadata,
            gap_manager: None,
        };
        let mut steering =
            gap::State::construct(60, pathfinder.identity, goto_name, &mut gap_context)?;
        assert_eq!(steering.flag_24, Some(0));
        assert!(super::super::ai_navigation_modifier_init::initialize(
            &mut steering,
            Some(goto_node),
            &mut gap_context
        )?);
        assert_eq!(
            steering.flag_24,
            Some(1),
            "Original authored ContinuousSpeed was not applied"
        );
        assert!(
            !steering.initialize_definition(Some(goto_node), &mut gap_context)?,
            "Missing managers were accepted as successful steering setup"
        );
        assert_eq!(steering.managers_4c_50, [Some(None), Some(None)]);
        gap_context.gap_manager = Some(&gap_manager);
        assert!(steering.initialize_definition(Some(goto_node), &mut gap_context)?);
        assert_eq!(
            steering.managers_4c_50,
            [Some(None), Some(Some(gap_manager.identity))]
        );
        assert_eq!(
            steering.attributes_68_6c_70,
            [Some(expected_height), Some(expected_diameter), Some(None)]
        );
        assert_eq!(steering.pool_d4.as_ref().unwrap().nodes.len(), 61);
        assert_eq!(steering.pool_118.as_ref().unwrap().nodes.len(), 36);
        assert_eq!(steering.name, b"IGoto");
        assert_eq!(steering.entity_58, constructed.base.owner);
        assert_eq!(steering.scalars_10_20[0], Some(20.));
        assert_eq!(steering.flags_f8_f9_fa, [Some(1), Some(0), Some(1)]);
        assert_eq!(steering.pool_d4.as_ref().unwrap().count, 0);
        assert_eq!(steering.pool_118.as_ref().unwrap().count, 0);
        assert_eq!(
            constructed
                .action
                .as_ref()
                .unwrap()
                .scalar(Kind::Heading)?
                .value_bits,
            45f32.to_bits()
        );
        assert_eq!(
            constructed
                .action
                .as_ref()
                .unwrap()
                .scalar(Kind::Speed)?
                .value_bits,
            (maximum * 0.25).to_bits()
        );
        assert_eq!(pathfinder.references_3c_4c_50_54_58_60_6c[1], None);
        assert_eq!(pathfinder.references_3c_4c_50_54_58_60_6c[4], None);
        assert!(
            constructed.base.services.is_empty(),
            "Uninitialized service was published in the brain"
        );
        entity.common.brain = Some(constructed.brain.identity);
        constructed
            .action
            .as_mut()
            .unwrap()
            .request(Kind::Heading, 0.)?;
        constructed
            .action
            .as_mut()
            .unwrap()
            .request(Kind::Speed, maximum * 0.5)?;
        world.apply_constructed_navigation_action(&entity, &mut constructed)?;
        assert_eq!(world.npc_relocations[&owner].facing, [0., 0., 1.]);
        assert_eq!(world.npc_relocations[&owner].desired_speed, -0.5);
        //The remaining cases retain the callback boundary so they can count
        //queries while mutating the same constructed brain suppression fields.
        let action = constructed
            .action
            .as_mut()
            .context("Constructed action absent")?;
        // Entity/data lookup remains supplied. Brain suppression and scalar
        // ownership now come from the actual constructed navigation objects.
        let mut lookup = Boundary {
            owner,
            brain: Some(&mut constructed.brain),
            basic_queries: 0,
        };
        action.request(Kind::Heading, 0.)?;
        action.request(Kind::Speed, maximum * 0.5)?;
        world.apply_navigation_action(action, 10, &mut lookup)?;
        assert_eq!(world.npc_relocations[&owner].facing, [0., 0., 1.]);
        assert_eq!(world.npc_relocations[&owner].desired_speed, -0.5);
        assert_eq!(world.npc_relocations[&owner].stored_input[1], -0.5);
        assert_eq!(
            action.scalar(Kind::Speed)?.changed,
            1,
            "Application must not implicitly acknowledge requests"
        );
        lookup.brain.as_mut().unwrap().flags_58_5b[3] = true;
        lookup.brain.as_mut().unwrap().flags_58_5b[2] = true;
        action.request(Kind::Heading, 90.)?;
        action.request(Kind::Speed, maximum * 0.25)?;
        world.apply_navigation_action(action, 10, &mut lookup)?;
        assert_eq!(world.npc_relocations[&owner].facing, [0., 0., 1.]);
        assert_eq!(world.npc_relocations[&owner].desired_speed, -0.5);
        lookup.brain.as_mut().unwrap().flags_58_5b[3] = false;
        world.apply_navigation_action(action, 10, &mut lookup)?;
        assert!(world.npc_relocations[&owner].facing[0] > 0.99999);
        assert!(world.npc_relocations[&owner].facing[2].abs() < 0.000001);
        assert_eq!(world.npc_relocations[&owner].desired_speed, -0.5);
        let facing = world.npc_relocations[&owner].facing;
        world.apply_basic_ai_disabled(owner, index, true)?;
        lookup.brain.as_mut().unwrap().flags_58_5b[2] = false;
        action.request(Kind::Heading, 180.)?;
        action.request(Kind::Speed, maximum)?;
        world.apply_navigation_action(action, 10, &mut lookup)?;
        assert_eq!(world.npc_relocations[&owner].facing, facing);
        assert_eq!(world.npc_relocations[&owner].desired_speed, -0.5);
        world.apply_basic_ai_disabled(owner, index, false)?;
        action.acknowledge();
        let queries = lookup.basic_queries;
        world.apply_navigation_action(action, 10, &mut lookup)?;
        assert_eq!(lookup.basic_queries, queries);
        assert_eq!(action.scalar(Kind::Speed)?.value_bits, maximum.to_bits());
        lookup.brain = None;
        action.request(Kind::Speed, 0.)?;
        world.apply_navigation_action(action, 10, &mut lookup)?;
        assert_eq!(lookup.basic_queries, queries);
        assert!(
            world.has_native_ai_agent(owner).is_err(),
            "Controlled action lookup must not register a fictitious navigation brain"
        );
        Ok(())
    }
}
struct Binding<'a, 'b, L> {
    world: &'a mut World,
    lookup: &'b mut L,
}
impl<L: Lookup> Lookup for Binding<'_, '_, L> {
    fn navigation_brain(&mut self, a: &mut Action, e: usize) -> Result<Option<usize>> {
        self.lookup.navigation_brain(a, e)
    }
    fn is_base_ai_brain(&mut self, a: &mut Action, b: usize) -> Result<bool> {
        self.lookup.is_base_ai_brain(a, b)
    }
    fn entity_data(&mut self, a: &mut Action, e: usize) -> Result<Option<usize>> {
        self.lookup.entity_data(a, e)
    }
    fn basic_controller(&mut self, a: &mut Action, d: Option<usize>) -> Result<Option<usize>> {
        self.lookup.basic_controller(a, d)
    }
    fn heading_blocked(&mut self, a: &mut Action, b: usize) -> Result<bool> {
        self.lookup.heading_blocked(a, b)
    }
    fn speed_blocked(&mut self, a: &mut Action, b: usize) -> Result<bool> {
        self.lookup.speed_blocked(a, b)
    }
}
impl<L: Lookup> Host for Binding<'_, '_, L> {
    // Portable BasicAI controller handles here are their actual gameplay
    //entity owners, as in the existing reflected controller input services.
    fn facing(
        &mut self,
        _: &mut Action,
        controller: Option<usize>,
        direction: [f32; 3],
    ) -> Result<()> {
        self.world.publish_npc_ai_facing(
            controller.context("Navigation action requires its real BasicAI controller")?,
            direction,
        )
    }
    fn speed(&mut self, _: &mut Action, controller: Option<usize>, value: f32) -> Result<()> {
        self.world.set_npc_ai_speed(
            controller.context("Navigation action requires its real BasicAI controller")?,
            false,
            value,
        )
    }
}
