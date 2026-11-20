//! FXComponent's native named-start protocol, REL31b9cc..31bc08.
//! The backend owns real ChildComponent/template/particle instances; this
//! protocol never replaces them with a generic visual burst or a success no-op.
use anyhow::{ensure, Result};
pub type Pose = [f32; 13]; // NiTransform: row-major rotation, translation, scale.
///31b414 ->203a80 installs c0734/c06d4: case-folded ASCII djb2
/// and case-insensitive comparison. The retail locale's uppercase table
///8064be20 maps A..Z to a..z. Non-ASCII locale behavior is not guessed.
pub fn lookup_key(name: &str) -> Result<String> {
    let name = name.split('\0').next().unwrap_or("");
    ensure!(
        name.is_ascii(),
        "Native FX lookup requires the original locale for non-ASCII names"
    );
    Ok(name.to_ascii_lowercase())
}
pub fn lookup_hash(name: &str) -> Result<u32> {
    Ok(lookup_key(name)?.bytes().fold(0x1505u32, |hash, byte| {
        hash.wrapping_mul(33).wrapping_add(byte.into())
    }))
}
pub trait Backend {
    fn child_factory(&mut self) -> Result<Option<usize>>;
    fn existing_child(&mut self, factory: usize, name: &str) -> Result<Option<usize>>;
    fn particle_controller(&mut self, entity: usize) -> Result<Option<usize>>;
    fn set_pose(&mut self, entity: usize, pose: &Pose) -> Result<()>;
    fn restart_particles(&mut self, controller: usize) -> Result<()>;
    fn retain_child(&mut self, entity: usize) -> Result<()>;
    fn template(&mut self, name: &str) -> Result<Option<usize>>;
    fn template_lifetime(&mut self, template: usize) -> Result<Option<f32>>;
    fn create_child(
        &mut self,
        factory: usize,
        template: usize,
        fire_and_forget: bool,
    ) -> Result<Option<usize>>;
}
pub fn start(
    backend: &mut impl Backend,
    name: &str,
    pose: Option<&Pose>,
    fire_and_forget: bool,
) -> Result<Option<usize>> {
    let Some(factory) = backend.child_factory()? else {
        return Ok(None);
    };
    if !fire_and_forget {
        if let Some(entity) = backend.existing_child(factory, name)? {
            if let Some(pose) = pose {
                backend.set_pose(entity, pose)?;
            }
            if let Some(controller) = backend.particle_controller(entity)? {
                backend.restart_particles(controller)?;
                backend.retain_child(entity)?;
                return Ok(Some(entity));
            }
        }
    }
    let Some(template) = backend.template(name)? else {
        return Ok(None);
    };
    if fire_and_forget
        && !backend
            .template_lifetime(template)?
            .is_some_and(|value| value > 0.)
    {
        return Ok(None);
    }
    let Some(entity) = backend.create_child(factory, template, fire_and_forget)? else {
        return Ok(None);
    };
    if let Some(pose) = pose {
        backend.set_pose(entity, pose)?;
    }
    if backend.particle_controller(entity)?.is_none() {
        // Creation is already published. Native does not roll it back merely
        // because the resulting entity has no particle-controller component.
        return Ok(None);
    }
    if !fire_and_forget {
        backend.retain_child(entity)?;
    }
    Ok(Some(entity))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Trace {
        calls: Vec<&'static str>,
        factory: bool,
        existing: bool,
        template: bool,
        life: Option<f32>,
        particle: bool,
        existing_particle: Option<bool>,
        create: bool,
        created: bool,
        fail_pose: bool,
    }
    impl Default for Trace {
        fn default() -> Self {
            Self {
                calls: vec![],
                factory: true,
                existing: false,
                template: true,
                life: Some(1.5),
                particle: true,
                existing_particle: None,
                create: true,
                created: false,
                fail_pose: false,
            }
        }
    }
    impl Backend for Trace {
        fn child_factory(&mut self) -> Result<Option<usize>> {
            self.calls.push("factory");
            Ok(self.factory.then_some(1))
        }
        fn existing_child(&mut self, f: usize, _: &str) -> Result<Option<usize>> {
            assert_eq!(f, 1);
            self.calls.push("existing");
            Ok(self.existing.then_some(2))
        }
        fn particle_controller(&mut self, e: usize) -> Result<Option<usize>> {
            assert!([2, 4].contains(&e));
            self.calls.push("particles");
            Ok((if e == 2 {
                self.existing_particle.unwrap_or(self.particle)
            } else {
                self.particle
            })
            .then_some(5))
        }
        fn set_pose(&mut self, e: usize, p: &Pose) -> Result<()> {
            assert!([2, 4].contains(&e));
            assert_eq!(p, &[7.; 13]);
            self.calls.push("pose");
            if self.fail_pose {
                anyhow::bail!("Expected pose failure")
            };
            Ok(())
        }
        fn restart_particles(&mut self, p: usize) -> Result<()> {
            assert_eq!(p, 5);
            self.calls.push("restart");
            Ok(())
        }
        fn retain_child(&mut self, e: usize) -> Result<()> {
            assert!([2, 4].contains(&e));
            self.calls.push("retain");
            Ok(())
        }
        fn template(&mut self, _: &str) -> Result<Option<usize>> {
            self.calls.push("template");
            Ok(self.template.then_some(3))
        }
        fn template_lifetime(&mut self, t: usize) -> Result<Option<f32>> {
            assert_eq!(t, 3);
            self.calls.push("lifetime");
            Ok(self.life)
        }
        fn create_child(&mut self, f: usize, t: usize, _: bool) -> Result<Option<usize>> {
            assert_eq!((f, t), (1, 3));
            self.calls.push("create");
            self.created = self.create;
            Ok(self.create.then_some(4))
        }
    }
    #[test]
    fn original_control_flow_fixture_matches_backend_order_and_partial_creation() -> Result<()> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/named_effect.json"
        ))?;
        let cases = data["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 44);
        for case in cases {
            let c = &case["input"];
            let boolean = |key: &str| c[key].as_bool().unwrap();
            let mut backend = Trace {
                factory: boolean("factory"),
                existing: boolean("existing"),
                existing_particle: Some(boolean("existing_particle")),
                template: boolean("template") && boolean("template_nonnull"),
                life: c["life"].as_f64().map(|v| v as f32),
                create: boolean("create"),
                particle: boolean("new_particle"),
                ..Default::default()
            };
            let result = start(
                &mut backend,
                "effect",
                boolean("pose").then_some(&[7.; 13]),
                boolean("fire_and_forget"),
            )?;
            assert_eq!(
                result.unwrap_or(0),
                case["result"]["entity"].as_u64().unwrap() as usize,
                "{c}"
            );
            assert_eq!(
                backend.created,
                case["result"]["created"].as_bool().unwrap(),
                "{c}"
            );
            assert_eq!(
                backend.calls,
                case["result"]["trace"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect::<Vec<_>>(),
                "{c}"
            );
        }
        Ok(())
    }
    #[test]
    fn ascii_lookup_preserves_native_case_folding_and_c_string_termination() -> Result<()> {
        assert_eq!(
            lookup_key("GremlinVariant4TeleportOutFX")?,
            "gremlinvariant4teleportoutfx"
        );
        assert_eq!(
            lookup_hash("GremlinVariant4TeleportOutFX")?,
            lookup_hash("gremlinvariant4teleportoutfx\0ignored")?
        );
        assert_eq!(lookup_hash("")?, 5381);
        assert!(lookup_key("é").is_err());
        Ok(())
    }
    #[test]
    fn reuse_repositions_before_restart_and_retains_without_template_lookup() -> Result<()> {
        let mut trace = Trace {
            existing: true,
            ..Default::default()
        };
        assert_eq!(
            start(&mut trace, "effect", Some(&[7.; 13]), false)?,
            Some(2)
        );
        assert_eq!(
            trace.calls,
            [
                "factory",
                "existing",
                "pose",
                "particles",
                "restart",
                "retain"
            ]
        );
        assert!(!trace.created);
        Ok(())
    }
    #[test]
    fn fire_and_forget_requires_positive_lifetime_but_never_reuses_or_retains() -> Result<()> {
        for life in [None, Some(-1.), Some(-0.), Some(0.), Some(f32::NAN)] {
            let mut trace = Trace {
                existing: true,
                life,
                ..Default::default()
            };
            assert_eq!(start(&mut trace, "effect", None, true)?, None);
            assert_eq!(trace.calls, ["factory", "template", "lifetime"]);
            assert!(!trace.created);
        }
        let mut trace = Trace {
            existing: true,
            ..Default::default()
        };
        assert_eq!(start(&mut trace, "effect", Some(&[7.; 13]), true)?, Some(4));
        assert_eq!(
            trace.calls,
            [
                "factory",
                "template",
                "lifetime",
                "create",
                "pose",
                "particles"
            ]
        );
        Ok(())
    }
    #[test]
    fn missing_factory_or_template_declines_without_creation() -> Result<()> {
        let mut trace = Trace {
            factory: false,
            ..Default::default()
        };
        assert_eq!(start(&mut trace, "effect", None, true)?, None);
        assert_eq!(trace.calls, ["factory"]);
        let mut trace = Trace {
            template: false,
            ..Default::default()
        };
        assert_eq!(start(&mut trace, "effect", None, true)?, None);
        assert_eq!(trace.calls, ["factory", "template"]);
        Ok(())
    }
    #[test]
    fn missing_particle_component_and_later_failure_do_not_undo_creation() -> Result<()> {
        let mut trace = Trace {
            particle: false,
            ..Default::default()
        };
        assert_eq!(start(&mut trace, "effect", None, true)?, None);
        assert!(trace.created);
        assert_eq!(
            trace.calls,
            ["factory", "template", "lifetime", "create", "particles"]
        );
        let mut trace = Trace {
            fail_pose: true,
            ..Default::default()
        };
        assert!(start(&mut trace, "effect", Some(&[7.; 13]), true).is_err());
        assert!(trace.created);
        assert_eq!(trace.calls.last(), Some(&"pose"));
        Ok(())
    }
    #[test]
    fn nonparticle_existing_child_falls_through_after_its_pose_write() -> Result<()> {
        let mut trace = Trace {
            existing: true,
            particle: false,
            ..Default::default()
        };
        assert_eq!(start(&mut trace, "effect", Some(&[7.; 13]), false)?, None);
        assert_eq!(
            trace.calls,
            [
                "factory",
                "existing",
                "pose",
                "particles",
                "template",
                "create",
                "pose",
                "particles"
            ]
        );
        assert!(trace.created);
        Ok(())
    }
}
