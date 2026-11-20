//! ProjectileComponent stimulus dispatch (retail REL 0x3909fc..0x390cb0).
//! Collision classification and downstream CSG/combat receivers are separate.
use anyhow::{ensure, Context, Result};
use serde_json::Value;

/// Target lookup order at REL 0x390604..0x390794. The first matching
/// component wins even if components appear in a different serialized order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetBody {
    Dynamic,
    Phantom,
    Static,
    Character,
}
impl TargetBody {
    pub fn class_name(self) -> &'static str {
        match self {
            Self::Dynamic => "PhysicsRigidBodyDynamicComponent",
            Self::Phantom => "PhysicsRigidBodyPhantomShapeComponent",
            Self::Static => "PhysicsRigidBodyStaticComponent",
            Self::Character => "PhysicsRigidBodyCharacterComponent",
        }
    }
    pub fn find<'a>(classes: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let mut found = [false; 4];
        for class in classes {
            let index = match class {
                "PhysicsRigidBodyDynamicComponent" => 0,
                "PhysicsRigidBodyPhantomShapeComponent" => 1,
                "PhysicsRigidBodyStaticComponent" => 2,
                "PhysicsRigidBodyCharacterComponent" => 3,
                _ => continue,
            };
            found[index] = true;
        }
        found
            .iter()
            .position(|&v| v)
            .map(|index| [Self::Dynamic, Self::Phantom, Self::Static, Self::Character][index])
    }
    pub fn target_flag(self) -> bool {
        self != Self::Static
    }
    /// r31 is cleared only when the phantom lookup succeeds. This controls
    /// the later surface-effect path, not stimulus dispatch itself.
    pub fn allows_surface_effect(self) -> bool {
        self != Self::Phantom
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Stimuli {
    pub types: Vec<u32>,
    pub amounts: Vec<f32>,
    pub radii: Vec<f32>,
    pub impact_modes: Vec<u32>,
}
impl Stimuli {
    /// Consume effective (inheritance-resolved) ProjectileComponent properties.
    pub fn load(properties: &[Value]) -> Result<Self> {
        let array = |name: &str| -> Result<&Vec<Value>> {
            let mut found = properties.iter().filter(|p| p["name"] == name);
            let value = found
                .next()
                .with_context(|| format!("Missing projectile {name}"))?;
            ensure!(found.next().is_none(), "Duplicate projectile {name}");
            value["value"]
                .as_array()
                .with_context(|| format!("Invalid projectile {name}"))
        };
        let integers = |name: &str| -> Result<Vec<u32>> {
            array(name)?
                .iter()
                .map(|v| {
                    v.as_u64()
                        .context("Invalid stimulus integer")?
                        .try_into()
                        .map_err(Into::into)
                })
                .collect()
        };
        let floats = |name: &str| -> Result<Vec<f32>> {
            array(name)?
                .iter()
                .map(|v| {
                    let n = v.as_f64().context("Invalid stimulus number")? as f32;
                    ensure!(n.is_finite(), "Non-finite projectile {name}");
                    Ok(n)
                })
                .collect()
        };
        Ok(Self {
            types: integers("StimTypeArray")?,
            amounts: floats("StimAmountArray")?,
            radii: floats("StimRadiusArray")?,
            impact_modes: integers("ProjectileImpactTypeArray")?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Delivery {
    /// REL 0x28834c: the selected target, without a radius query.
    Target,
    /// REL 0x2885a0: original radius and CsgPaintThinnerPersistent setting.
    Area {
        radius: f32,
        persistent: bool,
        projectile: u64,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct Emission {
    pub stimulus: u32,
    pub amount: f32,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub target: Option<u64>,
    pub owner: Option<u64>,
    pub tag: String,
    pub delivery: Delivery,
}
#[derive(Clone, Copy)]
pub struct Contact {
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub target: Option<u64>,
    /// The caller's r7 classification. Its component-query logic must run
    /// before dispatch; this is NOT simply target.is_some().
    pub target_flag: bool,
}
impl Contact {
    pub fn from_body(
        position: [f32; 3],
        direction: [f32; 3],
        target: Option<u64>,
        body: Option<TargetBody>,
    ) -> Self {
        Self {
            position,
            direction,
            target,
            target_flag: target.is_some() && body.is_some_and(TargetBody::target_flag),
        }
    }
}
pub struct Emitter {
    pub stimuli: Stimuli,
    /// ProjectileComponent +0x144, applied to amounts at 0x390d94.
    pub amount_scale: f32,
    pub projectile: u64,
    pub owner: Option<u64>,
    pub tag: String,
}
impl Emitter {
    /// Surface-effect gate in REL 0x3907d8..0x390904, after stimulus
    /// callbacks. Use the collision handler's original type count, but current
    /// amounts/scale: a receiver may have changed them during dispatch.
    /// This does not perform the subsequent brush-component/model lookup or
    /// decal creation at 0x316df4. Impact modes do not filter this second pass.
    pub fn requests_surface_effect(
        &self,
        original_count: usize,
        body: Option<TargetBody>,
        is_real: bool,
    ) -> Result<bool> {
        let mut has_paint_or_thinner = false;
        let mut maximum = 0.;
        for index in 0..original_count {
            let kind = *self
                .stimuli
                .types
                .get(index)
                .context("Stimulus type array shrank before surface-effect evaluation")?;
            if kind == 0 || kind == 1 {
                has_paint_or_thinner = true;
                let amount =
                    self.stimuli.amounts.get(index).copied().unwrap_or(0.) * self.amount_scale;
                // The native 'ble' does not branch on an unordered compare.
                if !(amount <= maximum) {
                    maximum = amount;
                }
            }
        }
        Ok(has_paint_or_thinner
            && body.is_none_or(TargetBody::allows_surface_effect)
            && is_real
            && !(maximum <= 0.))
    }
    /// Synchronous delivery permits a receiver to change the next entry or
    /// owner, just like native property queries between callbacks. Count and
    /// persistence are captured once. A callback failure stops immediately.
    pub fn dispatch(
        &mut self,
        contact: Contact,
        persistent: bool,
        mut deliver: impl FnMut(&mut Self, Emission) -> Result<()>,
    ) -> Result<()> {
        let count = self.stimuli.types.len();
        for index in 0..count {
            // Native initializes each optional getter's destination to zero.
            let radius = self.stimuli.radii.get(index).copied().unwrap_or(0.);
            let amount = self.stimuli.amounts.get(index).copied().unwrap_or(0.) * self.amount_scale;
            let mode = self.stimuli.impact_modes.get(index).copied().unwrap_or(0);
            let stimulus = *self
                .stimuli
                .types
                .get(index)
                .context("Stimulus type array shrank during synchronous dispatch")?;
            let direct = match mode {
                0 => false,
                1 if contact.target_flag => true,
                2 if !contact.target_flag => false,
                3 => contact.target_flag,
                _ => continue, // Retail switch deliberately skips this entry.
            };
            let emission = Emission {
                stimulus,
                amount,
                position: contact.position,
                direction: contact.direction,
                target: contact.target,
                owner: self.owner,
                tag: self.tag.clone(),
                delivery: if direct {
                    Delivery::Target
                } else {
                    Delivery::Area {
                        radius,
                        persistent,
                        projectile: self.projectile,
                    }
                },
            };
            deliver(self, emission)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn emitter() -> Emitter {
        Emitter {
            stimuli: Stimuli {
                types: vec![10, 11, 12, 13, 14],
                amounts: vec![0.5; 5],
                radii: vec![1.; 5],
                impact_modes: vec![0, 1, 2, 3, 99],
            },
            amount_scale: 2.,
            projectile: 9,
            owner: Some(7),
            tag: "brush".into(),
        }
    }
    fn contact(target_flag: bool) -> Contact {
        Contact {
            position: [1., 2., 3.],
            direction: [0., 1., 0.],
            target: Some(12),
            target_flag,
        }
    }
    #[test]
    fn authored_modes_select_direct_or_area_independently_of_target_presence() -> Result<()> {
        for flag in [false, true] {
            let mut emitted = Vec::new();
            emitter().dispatch(contact(flag), true, |_, e| {
                emitted.push(e);
                Ok(())
            })?;
            assert_eq!(
                emitted.iter().map(|e| e.stimulus).collect::<Vec<_>>(),
                if flag {
                    vec![10, 11, 13]
                } else {
                    vec![10, 12, 13]
                }
            );
            for e in emitted {
                assert_eq!(e.amount, 1.);
                assert_eq!(e.target, Some(12));
                assert_eq!(e.owner, Some(7));
                assert_eq!(
                    e.delivery,
                    if flag && e.stimulus != 10 {
                        Delivery::Target
                    } else {
                        Delivery::Area {
                            radius: 1.,
                            persistent: true,
                            projectile: 9,
                        }
                    }
                );
            }
        }
        Ok(())
    }
    #[test]
    fn body_lookup_uses_native_precedence_and_does_not_treat_every_entity_as_character(
    ) -> Result<()> {
        use TargetBody::*;
        let classes = [
            "PhysicsRigidBodyStaticComponent",
            "PhysicsRigidBodyCharacterComponent",
            "PhysicsRigidBodyPhantomShapeComponent",
            "PhysicsRigidBodyDynamicComponent",
        ];
        assert_eq!(TargetBody::find(classes), Some(Dynamic));
        assert_eq!(
            TargetBody::find(classes[..3].iter().copied()),
            Some(Phantom)
        );
        assert_eq!(TargetBody::find(classes[..2].iter().copied()), Some(Static));
        assert_eq!(TargetBody::find([classes[1]]), Some(Character));
        assert_eq!(TargetBody::find(["JPSTransformationComponent"]), None);
        for body in [
            None,
            Some(Dynamic),
            Some(Phantom),
            Some(Static),
            Some(Character),
        ] {
            let contact = Contact::from_body([0.; 3], [0., 1., 0.], Some(12), body);
            let mut delivered = Vec::new();
            emitter().dispatch(contact, false, |_, e| {
                delivered.push(e.stimulus);
                Ok(())
            })?;
            assert_eq!(
                delivered,
                if matches!(body, Some(Dynamic | Phantom | Character)) {
                    vec![10, 11, 13]
                } else {
                    vec![10, 12, 13]
                }
            );
        }
        assert!(!Contact::from_body([0.; 3], [0.; 3], None, Some(Character)).target_flag);
        assert!(!Phantom.allows_surface_effect());
        assert!(
            Dynamic.allows_surface_effect()
                && Static.allows_surface_effect()
                && Character.allows_surface_effect()
        );
        Ok(())
    }
    #[test]
    fn callbacks_affect_later_entries_without_extending_current_dispatch() -> Result<()> {
        let mut emitter = emitter();
        emitter.stimuli.types = vec![1, 0];
        emitter.stimuli.impact_modes.clear();
        emitter.stimuli.radii.clear();
        let mut received = Vec::new();
        emitter.dispatch(contact(false), false, |owner, e| {
            received.push(e);
            owner.amount_scale = 3.;
            owner.owner = Some(99);
            owner.tag = "changed".into();
            owner.stimuli.types.push(15);
            Ok(())
        })?;
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].amount, 1.);
        assert_eq!(received[1].amount, 1.5);
        assert_eq!(received[1].owner, Some(99));
        assert_eq!(received[1].tag, "changed");
        assert_eq!(
            received[1].delivery,
            Delivery::Area {
                radius: 0.,
                persistent: false,
                projectile: 9
            }
        );
        let mut calls = 0;
        assert!(emitter
            .dispatch(contact(false), false, |_, _| {
                calls += 1;
                anyhow::bail!("receiver failure")
            })
            .is_err());
        assert_eq!(calls, 1);
        Ok(())
    }
    #[test]
    fn surface_effect_uses_real_body_and_post_callback_amounts_not_delivery_modes() -> Result<()> {
        let mut emitter = emitter();
        emitter.stimuli.types = vec![1, 0, 9];
        emitter.stimuli.impact_modes = vec![99; 3];
        assert!(emitter.requests_surface_effect(3, Some(TargetBody::Static), true)?);
        assert!(!emitter.requests_surface_effect(3, Some(TargetBody::Static), false)?);
        assert!(!emitter.requests_surface_effect(3, Some(TargetBody::Phantom), true)?);
        emitter.stimuli.amounts = vec![0., -1., 100.];
        assert!(!emitter.requests_surface_effect(3, None, true)?);
        emitter.stimuli.impact_modes.clear();
        emitter.dispatch(contact(false), false, |owner, _| {
            owner.stimuli.amounts[0] = 2.;
            Ok(())
        })?;
        assert!(emitter.requests_surface_effect(3, None, true)?);
        emitter.amount_scale = f32::NAN;
        assert!(emitter.requests_surface_effect(3, None, true)?);
        assert!(emitter.requests_surface_effect(4, None, true).is_err());
        Ok(())
    }
}
