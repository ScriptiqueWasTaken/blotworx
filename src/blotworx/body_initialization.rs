//! Ordered property portion of the retail rigid-body loader, REL 382970..382e40.
//! Resource loading, initial velocity, welding/user data and world installation
//! surround this portion and must not be silently replaced by this function.
use super::{
    body_material::{Coefficients, Property},
    native_hk_transform::Transform,
    native_motion_record::MotionRecord,
    shape_bounds::ShapeGraph,
};
use anyhow::Result;

/// Live property/lifecycle access. Reads cannot be hoisted out of the body loop:
/// e.g. the first body's Mass publication changes subsequent bodies' inputs.
pub trait Context {
    fn float(&mut self, name: &str) -> Result<f32>;
    fn point(&mut self, name: &str) -> Result<[f32; 3]>;
    fn boolean(&mut self, name: &str) -> Result<bool>;
    fn publish_mass(&mut self, value: f32) -> Result<()>;
    fn config_float(&mut self, name: &str, default: &str) -> Result<f32>;
    /// REL 382a3c..382a50: resolve +7c wrapper through 37fc4c; when true set
    /// component byte +a8. Its lifecycle meaning remains with the component.
    fn material_finished(&mut self, motion: &mut MotionRecord) -> Result<()>;
    fn entity_transform(&mut self) -> Result<Transform>;
    /// REL 382cec: component virtual +38c (base dynamic target is 37d3b4).
    /// Other subclasses must dispatch their own implementation.
    fn placement_finished(&mut self, motion: &mut MotionRecord) -> Result<()>;
}

/// Execute on an owned body's CURRENT motion, after its velocity stage and
/// before welding/user-data creation. The graph is owned once by its placed
/// physics system, not copied per call. Errors preserve preceding side effects.
pub fn apply(
    motion: &mut MotionRecord,
    material: &mut Coefficients,
    bounds: &mut ShapeGraph,
    root: usize,
    context: &mut impl Context,
) -> Result<()> {
    // Both callbacks use the same live context sequentially, never concurrently.
    {
        let context = std::cell::RefCell::new(&mut *context);
        material.initialize(
            |property| {
                context.borrow_mut().float(match property {
                    Property::Friction => "Friction",
                    Property::Restitution => "Restitution",
                })
            },
            |name, default| context.borrow_mut().config_float(name, default),
        )?;
    }
    context.material_finished(motion)?;
    let center = if context.boolean("Should Change Center Of Mass")? {
        Some(context.point("Center Of Mass")?)
    } else {
        None
    };
    {
        let context = std::cell::RefCell::new(&mut *context);
        let identity = [
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ];
        motion.initialize_mass_properties(
            center,
            |_| bounds.aabb(root, &identity, 0.),
            || context.borrow_mut().float("Mass"),
            |value, _| context.borrow_mut().publish_mass(value),
            || context.borrow_mut().point("Restrict Rotation"),
        )?;
    }
    let transform = super::native_hk_transform::initial_body(
        &context.entity_transform()?,
        &motion.swept_state().transform,
    )?;
    motion.set_transform(transform)?;
    context.placement_finished(motion)?;
    motion.initialize_motion_tuning(|name| context.float(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Properties {
        mass: f32,
        events: Vec<String>,
        fail: bool,
    }
    impl Context for Properties {
        fn float(&mut self, name: &str) -> Result<f32> {
            self.events.push(name.into());
            Ok(match name {
                "Mass" => self.mass,
                "Friction" => -1.,
                "Restitution" => 0.5,
                "Linear Damping" => 0.25,
                _ => 0.,
            })
        }
        fn point(&mut self, name: &str) -> Result<[f32; 3]> {
            self.events.push(name.into());
            Ok(if name == "Center Of Mass" {
                [1., 0., 0.]
            } else {
                [0.; 3]
            })
        }
        fn boolean(&mut self, name: &str) -> Result<bool> {
            self.events.push(name.into());
            Ok(true)
        }
        fn publish_mass(&mut self, value: f32) -> Result<()> {
            self.events.push("publish mass".into());
            self.mass = value;
            Ok(())
        }
        fn config_float(&mut self, name: &str, default: &str) -> Result<f32> {
            self.events.push(name.into());
            assert_eq!(default, "1.0");
            Ok(0.75)
        }
        fn material_finished(&mut self, motion: &mut MotionRecord) -> Result<()> {
            self.events.push("material hook".into());
            assert_eq!(motion.swept_state().local_center, [0.; 4]);
            Ok(())
        }
        fn entity_transform(&mut self) -> Result<Transform> {
            self.events.push("entity pose".into());
            Ok(super::super::native_hk_transform::from_entity(
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                [10., 0., 0.],
            ))
        }
        fn placement_finished(&mut self, motion: &mut MotionRecord) -> Result<()> {
            self.events.push("placement hook".into());
            assert_eq!(motion.swept_state().transform[12], 12.);
            assert_eq!(motion.swept_state().center[0], 13.);
            if self.fail {
                anyhow::bail!("Placement hook failed");
            }
            Ok(())
        }
    }
    fn body(mass: f32) -> MotionRecord {
        let mut value = MotionRecord([0; 0x120]);
        value.0[8] = 3;
        for (at, v) in [(0xc0, 1f32), (0xc4, 1.), (0xc8, 1.), (0xcc, 1. / mass)] {
            value.0[at..at + 4].copy_from_slice(&v.to_be_bytes());
        }
        value
            .set_transform(super::super::native_hk_transform::from_entity(
                [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                [2., 0., 0.],
            ))
            .unwrap();
        value
    }
    #[test]
    fn properties_run_in_native_order_before_and_after_placement() {
        let mut bounds = ShapeGraph::default();
        let root = bounds
            .insert(super::super::shape_bounds::Shape::Box(
                super::super::shape_bounds::BoxShape {
                    half: [1.; 3],
                    radius: 0.,
                },
            ))
            .unwrap();
        let mut context = Properties {
            mass: 0.,
            events: vec![],
            fail: false,
        };
        let mut material = Coefficients {
            friction: 0.,
            restitution: 0.,
        };
        let mut first = body(8.);
        apply(&mut first, &mut material, &mut bounds, root, &mut context).unwrap();
        assert_eq!(
            context.events,
            [
                "Friction",
                "DefaultFrictionForAllObjects",
                "Restitution",
                "material hook",
                "Should Change Center Of Mass",
                "Center Of Mass",
                "Mass",
                "publish mass",
                "Restrict Rotation",
                "entity pose",
                "placement hook",
                "Linear Damping",
                "Angular Damping",
                "Gravity Factor",
                "Max Linear Velocity",
                "Max Angular Velocity"
            ]
        );
        assert_eq!(
            material,
            Coefficients {
                friction: 0.75,
                restitution: 0.5
            }
        );
        let mut second = body(16.);
        context.events.clear();
        apply(&mut second, &mut material, &mut bounds, root, &mut context).unwrap();
        assert_eq!(second.mass(), 8.);
        assert!(!context.events.iter().any(|e| e == "publish mass"));
        context.fail = true;
        context.events.clear();
        let mut failed = body(4.);
        assert!(apply(&mut failed, &mut material, &mut bounds, root, &mut context).is_err());
        assert_eq!(failed.swept_state().center[0], 13.);
        assert_eq!(context.events.last().unwrap(), "placement hook");
        assert_eq!(&failed.0[0xb4..0xb8], &[0; 4]);
    }
}
