//! JPSBaseAIEntity190e44, shape base190a8c and owned entity scalar factories.
//! Reflected property/profile services are required; construction does not
//! initialize a definition or register an agent with the navigation manager.
use super::ai_navigation_entity_base::Entity;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Kind {
    Diameter,
    Height,
    MaximumSpeed,
}
#[derive(Clone, Copy)]
pub struct Class {
    pub index: i32,
    pub constructor: Option<Kind>,
}
pub struct Registry {
    pub count: usize,
    pub diameter: Class,
    pub height: Class,
    pub maximum_speed: Class,
}
impl Registry {
    fn class(&self, kind: Kind) -> Class {
        match kind {
            Kind::Diameter => self.diameter,
            Kind::Height => self.height,
            Kind::MaximumSpeed => self.maximum_speed,
        }
    }
}
pub struct Scalar {
    pub kind: Kind,
    pub value_bits: u32,
}
pub struct Owned {
    pub entity: Entity,
    pub scalars: BTreeMap<usize, Scalar>,
    pub outlines: super::ai_navigation_outline::Store,
    next_scalar_identity: usize,
}
pub trait Host {
    /// Physics component lookup, reflected selector default1 and2967b4.
    fn radius(&mut self, entity: &mut Owned) -> Result<f32>;
    /// The same retained physics component, selector default1 and29680c.
    fn height(&mut self, entity: &mut Owned) -> Result<f32>;
    /// Motion component lookup/reflected Max Move Speed, default+0.
    fn maximum_speed(&mut self, entity: &mut Owned) -> Result<f32>;
    /// Optional global5a108 object's scalar+c; absence uses1. Read freshly
    /// inside each factory, after its preceding reflected profile query.
    fn unit_scale(&mut self, entity: &mut Owned) -> Result<Option<f32>>;
}
impl Owned {
    pub fn new(
        identity: usize,
        name: Option<&[u8]>,
        user: [usize; 2],
        registry: &Registry,
        host: &mut impl Host,
    ) -> Result<Self> {
        let mut owned = Self {
            entity: Entity::new(identity, name, user, registry.count)?,
            scalars: BTreeMap::new(),
            outlines: super::ai_navigation_outline::Store::default(),
            next_scalar_identity: 1,
        };
        let radius = host.radius(&mut owned)?;
        owned.entity.radius = radius;
        let diameter = owned.create(Kind::Diameter, registry, host)?;
        owned.store_nonnegative(diameter, 2. * owned.entity.radius)?;
        let height = host.height(&mut owned)?;
        owned.entity.height = height;
        let height_attribute = owned.create(Kind::Height, registry, host)?;
        owned.store_nonnegative(height_attribute, owned.entity.height)?;
        let speed = host.maximum_speed(&mut owned)?;
        let speed_attribute = owned.create(Kind::MaximumSpeed, registry, host)?;
        owned.store_nonnegative(speed_attribute, speed)?;
        Ok(owned)
    }
    fn create(&mut self, kind: Kind, registry: &Registry, host: &mut impl Host) -> Result<usize> {
        let class = registry.class(kind);
        ensure!(
            class.index >= 0 && (class.index as usize) < self.entity.attributes.len(),
            "Navigation entity scalar metadata has an invalid registered slot"
        );
        let constructor = class
            .constructor
            .context("Navigation entity scalar has no registered factory")?;
        let scale = host.unit_scale(self)?.unwrap_or(1.);
        let factor = match constructor {
            Kind::Diameter => f32::from_bits(0x3f333333),
            Kind::Height => f32::from_bits(0x3fe66666),
            Kind::MaximumSpeed => 0.5,
        };
        //These scalar classes have no request-changed byte. Value is+4;
        //their initialize slot4175e4 is empty. Do not reuse action scalars.
        let identity = self.next_scalar_identity;
        self.next_scalar_identity = identity
            .checked_add(1)
            .context("Navigation scalar identities exhausted")?;
        self.scalars.insert(
            identity,
            Scalar {
                kind: constructor,
                value_bits: (factor * scale).to_bits(),
            },
        );
        *self
            .entity
            .attributes
            .get_mut(class.index as usize)
            .context("Navigation scalar factory removed its registered slot")? = Some(identity);
        Ok(identity)
    }
    fn store_nonnegative(&mut self, identity: usize, value: f32) -> Result<()> {
        //190b88/190c4c/190ee8 ordered compare: negative and unordered values
        //retain the factory default. Both signed zeros and +infinity store.
        if value >= 0. {
            self.scalars
                .get_mut(&identity)
                .context("Navigation scalar ownership lost")?
                .value_bits = value.to_bits();
        }
        Ok(())
    }
    ///45f994(entity,metadata,1) direct-slot lookup. Negative/out-of-range
    ///metadata and a null slot mean absence, not a fabricated default scalar.
    pub fn optional_scalar(&self,kind:Kind,registry:&Registry)->Result<Option<&Scalar>> {
        let index=registry.class(kind).index;
        if index<0{return Ok(None)}
        let Some(identity)=self.entity.attributes.get(index as usize).copied().flatten().filter(|&p|p!=0) else{return Ok(None)};
        Ok(Some(self.scalars.get(&identity).context("Navigation scalar instance is absent")?))
    }
    pub fn scalar(&self, kind: Kind, registry: &Registry) -> Result<&Scalar> {
        let index = registry.class(kind).index;
        ensure!(index >= 0, "Navigation scalar slot is negative");
        let identity = self
            .entity
            .attributes
            .get(index as usize)
            .copied()
            .flatten()
            .context("Navigation scalar slot is absent")?;
        self.scalars
            .get(&identity)
            .context("Navigation scalar instance is absent")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Binding<'a> {
        case: &'a Value,
        trace: Vec<Value>,
    }
    impl Binding<'_> {
        fn value(&self, key: &str) -> f32 {
            f32::from_bits(self.case[key].as_u64().unwrap() as u32)
        }
    }
    impl Host for Binding<'_> {
        fn radius(&mut self, entity: &mut Owned) -> Result<f32> {
            assert_eq!(entity.entity.user_data, [10, 11]);
            self.trace.push(json!([
                "radius",
                self.case["radius_selector"].as_i64().unwrap_or(1)
            ]));
            Ok(self.value("radius"))
        }
        fn height(&mut self, entity: &mut Owned) -> Result<f32> {
            assert_eq!(
                entity.entity.radius.to_bits(),
                if self.case["mutation"] == "radius" {
                    0.8f32.to_bits()
                } else {
                    self.value("radius").to_bits()
                }
            );
            self.trace.push(json!([
                "height",
                self.case["height_selector"].as_i64().unwrap_or(1)
            ]));
            Ok(self.value("height"))
        }
        fn maximum_speed(&mut self, entity: &mut Owned) -> Result<f32> {
            assert_eq!(
                entity.entity.height.to_bits(),
                if self.case["mutation"] == "height" {
                    4.0f32.to_bits()
                } else {
                    self.value("height").to_bits()
                }
            );
            self.trace.push(json!(["speed"]));
            Ok(self.case["speed"]
                .as_u64()
                .map_or(0., |b| f32::from_bits(b as u32)))
        }
        fn unit_scale(&mut self, entity: &mut Owned) -> Result<Option<f32>> {
            if self.case["mutation"] == "radius" && entity.scalars.is_empty() {
                entity.entity.radius = 0.8;
            }
            if self.case["mutation"] == "height" && entity.scalars.len() == 1 {
                entity.entity.height = 4.;
            }
            Ok(self.case["scale"]
                .as_u64()
                .map(|b| f32::from_bits(b as u32)))
        }
    }
    #[test]
    fn navigation_ai_entity_constructor_matches_original_dimensions_and_scalar_factories(
    ) -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/ai_navigation_entity.json"
        ))?;
        for expected in fixture["cases"].as_array().unwrap() {
            let c = &expected["input"];
            let class = |i: usize, kind| Class {
                index: c["indices"][i].as_i64().unwrap() as i32,
                constructor: Some(kind),
            };
            let registry = Registry {
                count: c["count"].as_u64().unwrap() as usize,
                diameter: class(0, Kind::Diameter),
                height: class(1, Kind::Height),
                maximum_speed: class(2, Kind::MaximumSpeed),
            };
            let mut binding = Binding {
                case: c,
                trace: Vec::new(),
            };
            let owned = Owned::new(20, Some(b"Gus"), [10, 11], &registry, &mut binding)?;
            assert_eq!(
                binding.trace,
                expected["trace"].as_array().unwrap().to_vec(),
                "trace:{c}"
            );
            assert_eq!(
                json!([owned.entity.radius.to_bits(), owned.entity.height.to_bits()]),
                expected["dimensions"],
                "dimensions:{c}"
            );
            let attributes = owned
                .entity
                .attributes
                .iter()
                .map(|slot| {
                    slot.map(|id| {
                        let scalar = &owned.scalars[&id];
                        json!({"kind":scalar.kind,"value":scalar.value_bits})
                    })
                })
                .collect::<Vec<_>>();
            assert_eq!(json!(attributes), expected["attributes"], "attributes:{c}");
            assert_eq!(
                owned.scalar(Kind::MaximumSpeed, &registry)?.kind,
                Kind::MaximumSpeed
            );
        }
        Ok(())
    }
}
