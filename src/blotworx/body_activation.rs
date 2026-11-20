//! Component activation orchestration, REL 0x38330c..0x383598.
//! Backends must implement the actual motion conversion and world operations;
//! none have defaults that could silently stand in for Havok semantics.
use anyhow::Result;
use anyhow::{ensure, Context};

/// Concrete native record lifecycle before hkpEntity world admission. A body
/// can already have Box3D query shapes; those are not native island membership.
/// DOL802b59fc..5a10 proves activate has no writes when body+8 is null.
pub struct PreAdmission<'a> {
    pub bodies: &'a mut [super::dynamic_body::PlacedBody],
    pub component: &'a mut epic_mickey_lib_rs::scene_file::Component,
    pub gravity_factor_word: u32,
}
/// Box3D compatibility world backend. Native policy/order is retained; solver
/// islands, quality-specific agents and deferred world operations are not yet
/// Havok-equivalent. Used only outside a simulation/world lock.
pub struct Admitted<'a> {
    pub records: PreAdmission<'a>,
    pub world: &'a mut super::physics::Collision,
    pub materials: &'a [super::body_material::Coefficients],
}
impl Admitted<'_> {
    fn publish(&mut self, index: usize) -> Result<()> {
        let body = self.records.body(index)?;
        if matches!(body.motion.current.motion_type(), 2 | 3 | 6 | 7) {
            body.admit_native_mass(self.world)?;
        }
        self.world.set_simulation_body(
            &body.shapes,
            &body.motion.current,
            *self
                .materials
                .get(index)
                .context("Missing admitted body material")?,
            true,
        )
    }
}
impl System for Admitted<'_> {
    fn body_count(&self) -> usize {
        self.records.body_count()
    }
    fn motion_type(&self, index: usize) -> Result<u8> {
        self.records.motion_type(index)
    }
    fn has_stored_motion(&self, index: usize) -> Result<bool> {
        self.records.has_stored_motion(index)
    }
    fn set_active(&mut self, index: usize, value: bool) -> Result<()> {
        self.records.set_active(index, value)
    }
    fn cache_velocities(&mut self, index: usize) -> Result<()> {
        self.records.cache_velocities(index)
    }
    fn cached_linear(&self, index: usize) -> Result<[f32; 4]> {
        self.records.cached_linear(index)
    }
    fn cached_angular(&self, index: usize) -> Result<[f32; 4]> {
        self.records.cached_angular(index)
    }
    fn activate(&mut self, index: usize) -> Result<()> {
        let shape = *self
            .records
            .body(index)?
            .shapes
            .first()
            .context("Admitted body has no shape")?;
        self.world.wake_simulation_body(shape)
    }
    fn set_linear(&mut self, index: usize, value: [f32; 4]) -> Result<()> {
        self.records.set_linear(index, value)?;
        self.publish(index)
    }
    fn set_angular(&mut self, index: usize, value: [f32; 4]) -> Result<()> {
        self.records.set_angular(index, value)?;
        self.publish(index)
    }
    fn set_mover(&mut self, value: bool) -> Result<()> {
        self.records.set_mover(value)
    }
    fn request_motion(&mut self, index: usize, mode: u8) -> Result<()> {
        self.records.request_motion(index, mode)?;
        self.publish(index)
    }
    fn set_quality(&mut self, index: usize, quality: u8) -> Result<()> {
        self.records.set_quality(index, quality)
    }
    fn in_world(&self, index: usize) -> Result<bool> {
        let shape = *self
            .records
            .body(index)?
            .shapes
            .first()
            .context("Admitted body has no shape")?;
        let owner = self
            .world
            .shape_owner(shape)
            .context("Admitted body has no owner")?;
        self.world.owner_in_world(owner)
    }
    fn update_filter(&mut self, _: usize) -> Result<()> {
        anyhow::bail!("Quality-specific world filter refresh requires agent compatibility")
    }
}
impl PreAdmission<'_> {
    fn body(&self, index: usize) -> Result<&super::dynamic_body::PlacedBody> {
        self.bodies.get(index).context("Unknown activation body")
    }
    fn body_mut(&mut self, index: usize) -> Result<&mut super::dynamic_body::PlacedBody> {
        self.bodies
            .get_mut(index)
            .context("Unknown activation body")
    }
}
impl System for PreAdmission<'_> {
    fn body_count(&self) -> usize {
        self.bodies.len()
    }
    fn motion_type(&self, index: usize) -> Result<u8> {
        Ok(self.body(index)?.motion.current.motion_type())
    }
    fn has_stored_motion(&self, index: usize) -> Result<bool> {
        Ok(self.body(index)?.motion.has_stored_dynamic())
    }
    fn set_active(&mut self, index: usize, active: bool) -> Result<()> {
        self.body_mut(index)?.set_activation_active(active);
        Ok(())
    }
    fn cache_velocities(&mut self, index: usize) -> Result<()> {
        self.body_mut(index)?.cache_activation_velocities()
    }
    fn cached_linear(&self, index: usize) -> Result<[f32; 4]> {
        Ok(self.body(index)?.cached_activation_velocities()?.0)
    }
    fn cached_angular(&self, index: usize) -> Result<[f32; 4]> {
        Ok(self.body(index)?.cached_activation_velocities()?.1)
    }
    fn activate(&mut self, index: usize) -> Result<()> {
        self.body(index)?;
        Ok(())
    }
    fn set_linear(&mut self, index: usize, value: [f32; 4]) -> Result<()> {
        self.body_mut(index)?.motion.set_linear_velocity(value)
    }
    fn set_angular(&mut self, index: usize, value: [f32; 4]) -> Result<()> {
        self.body_mut(index)?.motion.set_angular_velocity(value)
    }
    fn set_mover(&mut self, value: bool) -> Result<()> {
        ensure!(
            self.component.class_name == "PhysicsRigidBodyDynamicComponent",
            "Unresolved derived activation setter"
        );
        let property = self
            .component
            .properties
            .iter_mut()
            .find(|p| p.name == "Is Mover")
            .context("Unresolved Is Mover")?;
        ensure!(property.value.is_boolean(), "Malformed Is Mover");
        property.value = serde_json::json!(value);
        Ok(())
    }
    fn request_motion(&mut self, index: usize, mode: u8) -> Result<()> {
        let gravity = self.gravity_factor_word;
        self.body_mut(index)?.motion.replace(mode, gravity)?;
        Ok(())
    }
    fn set_quality(&mut self, index: usize, quality: u8) -> Result<()> {
        self.body_mut(index)?.motion.quality = quality;
        Ok(())
    }
    fn in_world(&self, index: usize) -> Result<bool> {
        self.body(index)?;
        Ok(false)
    }
    fn update_filter(&mut self, _: usize) -> Result<()> {
        anyhow::bail!("Pre-admission body cannot update an admitted-world filter")
    }
}

pub trait System {
    fn body_count(&self) -> usize;
    fn motion_type(&self, body: usize) -> Result<u8>;
    /// Native helper exposes stored motion only for types 4 and 5.
    fn has_stored_motion(&self, body: usize) -> Result<bool>;
    /// A missing optional user-data record makes this flag write a no-op.
    fn set_active(&mut self, body: usize, active: bool) -> Result<()>;
    /// Unlike the flag write, caching/restoring requires body user data.
    fn cache_velocities(&mut self, body: usize) -> Result<()>;
    fn cached_linear(&self, body: usize) -> Result<[f32; 4]>;
    fn cached_angular(&self, body: usize) -> Result<[f32; 4]>;
    fn activate(&mut self, body: usize) -> Result<()>;
    fn set_linear(&mut self, body: usize, value: [f32; 4]) -> Result<()>;
    fn set_angular(&mut self, body: usize, value: [f32; 4]) -> Result<()>;
    /// Component property write, repeated for each converted body.
    fn set_mover(&mut self, value: bool) -> Result<()>;
    /// DOL 0x802b7380 with activation=1 and filter-update mode=0.
    /// Request 1 must retain/restore the concrete inertia model.
    /// If world+0x94 is nonzero, enqueue operation type 4 instead of immediately
    /// converting. The component continues its remaining operations meanwhile.
    /// A type 4/5 body without stored dynamic motion rejects a dynamic request
    /// without changing mode (0x802d6438..0x802d6450).
    fn request_motion(&mut self, body: usize, mode: u8) -> Result<()>;
    fn set_quality(&mut self, body: usize, quality: u8) -> Result<()>;
    fn in_world(&self, body: usize) -> Result<bool>;
    /// DOL 0x802dacd0 with update mode=0 and collection mode=1.
    fn update_filter(&mut self, body: usize) -> Result<()>;
}

/// Executes in authored physics-system order. Errors abort at the failing
/// operation; earlier world changes cannot be rolled back or blindly retried.
/// A missing component physics system is represented by no call, not invented
/// bodies. Component+0x68 holds the transformation's parent entity relationship.
pub fn apply(
    system: &mut impl System,
    requested_dynamic: bool,
    has_parent: bool,
    update_quality: bool,
) -> Result<()> {
    if requested_dynamic && !has_parent {
        for body in 0..system.body_count() {
            system.set_active(body, true)?;
            let restore = system.has_stored_motion(body)?;
            system.set_mover(false)?;
            system.request_motion(body, 1)?;
            if restore {
                let linear = system.cached_linear(body)?;
                system.activate(body)?;
                system.set_linear(body, linear)?;
                let angular = system.cached_angular(body)?;
                system.activate(body)?;
                system.set_angular(body, angular)?;
            }
        }
    } else {
        for body in 0..system.body_count() {
            if !matches!(system.motion_type(body)?, 4 | 5) {
                system.set_active(body, false)?;
                system.cache_velocities(body)?;
                system.activate(body)?;
                system.set_linear(body, [0.; 4])?;
                system.activate(body)?;
                system.set_angular(body, [0.; 4])?;
                system.set_mover(true)?;
                system.request_motion(body, 4)?;
            }
            // This is outside the motion-type guard in the original.
            if update_quality {
                system.set_quality(body, 9)?;
                if system.in_world(body)? {
                    system.update_filter(body)?;
                }
            }
        }
    }
    Ok(())
}

/// REL 0x383160..0x3832e4: parent changes are a separate lifecycle path.
/// Read Is Real / Is Painted per body, as the original does. There is no
/// activation-cache save/restore, Is Mover write, or quality override here.
/// The required final callback performs base 0x37f000: compare/store the parent
/// relationship and, for a changed non-null parent, force the pose update.
pub fn change_parent<S: System>(
    system: &mut S,
    parent_present: bool,
    mut real_or_painted: impl FnMut(&mut S) -> Result<bool>,
    finish_parent: impl FnOnce(&mut S) -> Result<()>,
) -> Result<()> {
    for body in 0..system.body_count() {
        if !parent_present && real_or_painted(system)? {
            system.request_motion(body, 1)?;
            system.set_active(body, true)?;
        } else if !matches!(system.motion_type(body)?, 4 | 5) {
            system.activate(body)?;
            system.set_linear(body, [0.; 4])?;
            system.activate(body)?;
            system.set_angular(body, [0.; 4])?;
            system.request_motion(body, 4)?;
            system.set_active(body, false)?;
        }
    }
    finish_parent(system)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Body {
        mode: u8,
        stored: bool,
        world: bool,
        linear: [f32; 4],
        angular: [f32; 4],
        cache: ([f32; 4], [f32; 4]),
    }
    struct Mock {
        bodies: Vec<Body>,
        events: Vec<String>,
        fail: Option<String>,
    }
    impl Mock {
        fn event(&mut self, event: String) -> Result<()> {
            self.events.push(event.clone());
            anyhow::ensure!(
                self.fail.as_ref() != Some(&event),
                "injected operation failure"
            );
            Ok(())
        }
        fn new(modes: &[u8]) -> Self {
            Self {
                bodies: modes
                    .iter()
                    .map(|&mode| Body {
                        mode,
                        stored: false,
                        world: true,
                        linear: [1., 2., 3., 7.],
                        angular: [4., 5., 6., 8.],
                        cache: ([0.; 4], [0.; 4]),
                    })
                    .collect(),
                events: vec![],
                fail: None,
            }
        }
    }
    impl System for Mock {
        fn body_count(&self) -> usize {
            self.bodies.len()
        }
        fn motion_type(&self, b: usize) -> Result<u8> {
            Ok(self.bodies[b].mode)
        }
        fn has_stored_motion(&self, b: usize) -> Result<bool> {
            Ok(matches!(self.bodies[b].mode, 4 | 5) && self.bodies[b].stored)
        }
        fn set_active(&mut self, b: usize, v: bool) -> Result<()> {
            self.event(format!("{b}:active:{v}"))
        }
        fn cache_velocities(&mut self, b: usize) -> Result<()> {
            self.event(format!("{b}:cache"))?;
            let body = &mut self.bodies[b];
            body.cache = (body.linear, body.angular);
            Ok(())
        }
        fn cached_linear(&self, b: usize) -> Result<[f32; 4]> {
            Ok(self.bodies[b].cache.0)
        }
        fn cached_angular(&self, b: usize) -> Result<[f32; 4]> {
            Ok(self.bodies[b].cache.1)
        }
        fn activate(&mut self, b: usize) -> Result<()> {
            self.event(format!("{b}:activate"))
        }
        fn set_linear(&mut self, b: usize, v: [f32; 4]) -> Result<()> {
            self.event(format!("{b}:linear"))?;
            self.bodies[b].linear = v;
            Ok(())
        }
        fn set_angular(&mut self, b: usize, v: [f32; 4]) -> Result<()> {
            self.event(format!("{b}:angular"))?;
            self.bodies[b].angular = v;
            Ok(())
        }
        fn set_mover(&mut self, v: bool) -> Result<()> {
            self.event(format!("mover:{v}"))
        }
        fn request_motion(&mut self, b: usize, m: u8) -> Result<()> {
            self.event(format!("{b}:motion:{m}"))?;
            self.bodies[b].mode = m;
            Ok(())
        }
        fn set_quality(&mut self, b: usize, q: u8) -> Result<()> {
            self.event(format!("{b}:quality:{q}"))
        }
        fn in_world(&self, b: usize) -> Result<bool> {
            Ok(self.bodies[b].world)
        }
        fn update_filter(&mut self, b: usize) -> Result<()> {
            self.event(format!("{b}:filter"))
        }
    }
    #[test]
    fn attaching_parent_converts_before_publishing_relation_without_caching() {
        let mut s = Mock::new(&[2, 4, 5]);
        change_parent(
            &mut s,
            true,
            |_| panic!("Attached branch must not read real/painted"),
            |s| s.event("parent:attached".into()),
        )
        .unwrap();
        assert_eq!(
            s.events,
            [
                "0:activate",
                "0:linear",
                "0:activate",
                "0:angular",
                "0:motion:4",
                "0:active:false",
                "parent:attached"
            ]
        );
        assert_eq!(s.bodies[0].cache, ([0.; 4], [0.; 4]));
        assert_eq!(s.bodies[1].linear, [1., 2., 3., 7.]);
    }
    #[test]
    fn detaching_parent_rechecks_flags_and_does_not_restore_activation_cache() {
        let mut s = Mock::new(&[4, 2]);
        s.bodies[0].stored = true;
        s.bodies[0].cache = ([9.; 4], [8.; 4]);
        let mut reads = 0;
        change_parent(
            &mut s,
            false,
            |_| {
                reads += 1;
                Ok(reads == 1)
            },
            |s| s.event("parent:cleared".into()),
        )
        .unwrap();
        assert_eq!(reads, 2);
        assert_eq!(
            s.events,
            [
                "0:motion:1",
                "0:active:true",
                "1:activate",
                "1:linear",
                "1:activate",
                "1:angular",
                "1:motion:4",
                "1:active:false",
                "parent:cleared"
            ]
        );
        assert_eq!(s.bodies[0].linear, [1., 2., 3., 7.]);
    }
    #[test]
    fn caches_before_zeroing_and_updates_existing_fixed_bodies() {
        let mut s = Mock::new(&[2, 4, 5]);
        s.bodies[2].world = false;
        apply(&mut s, true, true, true).unwrap();
        assert_eq!(
            s.events,
            [
                "0:active:false",
                "0:cache",
                "0:activate",
                "0:linear",
                "0:activate",
                "0:angular",
                "mover:true",
                "0:motion:4",
                "0:quality:9",
                "0:filter",
                "1:quality:9",
                "1:filter",
                "2:quality:9"
            ]
        );
        assert_eq!(s.bodies[0].cache, ([1., 2., 3., 7.], [4., 5., 6., 8.]));
        assert_eq!(s.bodies[0].linear, [0.; 4]);
        assert_eq!(s.bodies[1].linear, [1., 2., 3., 7.]);
    }
    #[test]
    fn restores_after_conversion_and_never_updates_quality_on_dynamic_branch() {
        let mut s = Mock::new(&[4, 5, 2]);
        s.bodies[0].stored = true;
        s.bodies[0].cache = ([9., 8., 7., 6.], [5., 4., 3., 2.]);
        apply(&mut s, true, false, true).unwrap();
        assert_eq!(
            s.events,
            [
                "0:active:true",
                "mover:false",
                "0:motion:1",
                "0:activate",
                "0:linear",
                "0:activate",
                "0:angular",
                "1:active:true",
                "mover:false",
                "1:motion:1",
                "2:active:true",
                "mover:false",
                "2:motion:1"
            ]
        );
        assert_eq!(s.bodies[0].linear, [9., 8., 7., 6.]);
        assert_eq!(s.bodies[0].angular, [5., 4., 3., 2.]);
    }
    #[test]
    fn failure_does_not_advance_to_later_bodies_or_undo_earlier_writes() {
        let mut s = Mock::new(&[2, 2]);
        s.fail = Some("0:angular".into());
        assert!(apply(&mut s, false, false, true).is_err());
        assert_eq!(
            s.events,
            [
                "0:active:false",
                "0:cache",
                "0:activate",
                "0:linear",
                "0:activate",
                "0:angular"
            ]
        );
        assert_eq!(s.bodies[0].linear, [0.; 4]);
        assert_eq!(s.bodies[0].mode, 2);
        assert_eq!(s.bodies[1].linear, [1., 2., 3., 7.]);
    }
}
