//! PlayerPickup initialization, REL389e80..389f98 /389fbc..38a044.
use anyhow::Result;

#[derive(Default, Debug)]
pub struct State {
    pub physics_component: Option<usize>,
    pub phantom: Option<usize>,
    pub listener: Option<usize>,
}
pub trait Initialize {
    fn physics_component(&mut self) -> Result<Option<usize>>;
    /// Native component +4 reference decrement/destruction and increment.
    fn release_component(&mut self, component: usize) -> Result<()>;
    fn retain_component(&mut self, component: usize) -> Result<()>;
    fn phantom(&mut self, component: usize) -> Result<Option<usize>>;
    /// Original Havok referenced-object retain80159c74.
    fn retain_phantom(&mut self, phantom: usize) -> Result<()>;
    /// Allocate12 bytes, size12/refcount1, vtable504780/interface504794.
    /// Allocation failure is unsupported, never a fabricated null listener.
    fn allocate_listener(&mut self) -> Result<usize>;
    fn register(&mut self, phantom: usize, listener: usize) -> Result<()>;
    fn position(&mut self) -> Result<[f32; 4]>;
}
/// Component ownership/global reference monitor remain actual world callbacks.
/// Phantom retention/listener allocation/registration use shared owned records.
pub trait ComponentBindings: super::hk_reference::Backend {
    fn physics_component(&mut self) -> Result<Option<usize>>;
    fn release_component(&mut self, component: usize) -> Result<()>;
    fn retain_component(&mut self, component: usize) -> Result<()>;
    fn phantom(&mut self, component: usize) -> Result<Option<usize>>;
    fn position(&mut self) -> Result<[f32; 4]>;
}
pub struct SharedInitialize<'a, B> {
    pub objects: &'a mut super::phantom_objects::Objects,
    pub bindings: &'a mut B,
}
impl<B: ComponentBindings> Initialize for SharedInitialize<'_, B> {
    fn physics_component(&mut self) -> Result<Option<usize>> {
        self.bindings.physics_component()
    }
    fn release_component(&mut self, c: usize) -> Result<()> {
        self.bindings.release_component(c)
    }
    fn retain_component(&mut self, c: usize) -> Result<()> {
        self.bindings.retain_component(c)
    }
    fn phantom(&mut self, c: usize) -> Result<Option<usize>> {
        self.bindings.phantom(c)
    }
    fn retain_phantom(&mut self, p: usize) -> Result<()> {
        self.objects.retain_phantom(p, self.bindings)
    }
    fn allocate_listener(&mut self) -> Result<usize> {
        self.objects.allocate_pickup_listener()
    }
    fn register(&mut self, p: usize, l: usize) -> Result<()> {
        use anyhow::Context;
        anyhow::ensure!(
            self.objects.listeners.contains_key(&l),
            "Unknown pickup listener allocation"
        );
        self.objects
            .phantoms
            .get_mut(&p)
            .context("Unknown pickup phantom allocation")?
            .overlaps
            .register(Some(l));
        Ok(())
    }
    fn position(&mut self) -> Result<[f32; 4]> {
        self.bindings.position()
    }
}
pub trait Teardown {
    /// REL143d48 must run before reading pickup-owned fields.
    fn base(&mut self, state: &mut State) -> Result<()>;
    fn unregister(&mut self, phantom: usize, listener: Option<usize>) -> Result<()>;
    fn release_phantom(&mut self, phantom: usize) -> Result<()>;
    fn release_listener(&mut self, listener: usize) -> Result<()>;
    fn release_component(&mut self, component: usize) -> Result<()>;
}
/// REL3885ec..3886e8. Clear each field after its successful release, not before.
pub fn teardown(state: &mut State, backend: &mut impl Teardown) -> Result<()> {
    backend.base(state)?;
    if let Some(phantom) = state.phantom {
        backend.unregister(phantom, state.listener)?;
        backend.release_phantom(phantom)?;
        state.phantom = None;
    }
    if let Some(listener) = state.listener {
        backend.release_listener(listener)?;
        state.listener = None;
    }
    if let Some(component) = state.physics_component {
        backend.release_component(component)?;
        state.physics_component = None;
    }
    Ok(())
}
pub fn initialize(
    state: &mut State,
    bob: &mut super::pickup::Bob,
    backend: &mut impl Initialize,
) -> Result<()> {
    let component = backend.physics_component()?;
    if state.physics_component != component {
        if let Some(previous) = state.physics_component {
            backend.release_component(previous)?;
        }
        state.physics_component = component;
        if let Some(component) = component {
            backend.retain_component(component)?;
        }
    }
    if let Some(component) = state.physics_component {
        let phantom = backend.phantom(component)?;
        state.phantom = phantom;
        if let Some(phantom) = phantom {
            backend.retain_phantom(phantom)?;
            let listener = backend.allocate_listener()?;
            state.listener = Some(listener);
            backend.register(phantom, listener)?;
        }
    }
    bob.base_height = backend.position()?[1];
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initialization_retains_and_registers_actual_shared_records() -> Result<()> {
        struct Bindings {
            phantom: usize,
            calls: Vec<&'static str>,
            fail: bool,
        }
        impl super::super::hk_reference::Backend for Bindings {
            fn enter(&mut self) -> Result<()> {
                self.calls.push("enter");
                Ok(())
            }
            fn leave(&mut self) -> Result<()> {
                self.calls.push("leave");
                Ok(())
            }
            fn destroy(&mut self, _: &mut super::super::hk_reference::Header) -> Result<()> {
                panic!("Retain cannot destroy")
            }
        }
        impl ComponentBindings for Bindings {
            fn physics_component(&mut self) -> Result<Option<usize>> {
                self.calls.push("component");
                Ok(Some(40))
            }
            fn release_component(&mut self, _: usize) -> Result<()> {
                panic!("No old component")
            }
            fn retain_component(&mut self, c: usize) -> Result<()> {
                assert_eq!(c, 40);
                self.calls.push("retain component");
                Ok(())
            }
            fn phantom(&mut self, c: usize) -> Result<Option<usize>> {
                assert_eq!(c, 40);
                self.calls.push("phantom");
                Ok(Some(self.phantom))
            }
            fn position(&mut self) -> Result<[f32; 4]> {
                self.calls.push("position");
                if self.fail {
                    anyhow::bail!("Position unavailable")
                };
                Ok([1., 7., 3., 0.])
            }
        }
        let mut objects = super::super::phantom_objects::Objects::default();
        let phantom = objects.allocate_size_phantom([0; 8], 0x18001)?;
        let mut bindings = Bindings {
            phantom,
            calls: vec![],
            fail: false,
        };
        let mut state = State::default();
        let mut bob = super::super::pickup::Bob {
            phase: 2.,
            base_height: 3.,
        };
        initialize(
            &mut state,
            &mut bob,
            &mut SharedInitialize {
                objects: &mut objects,
                bindings: &mut bindings,
            },
        )?;
        assert_eq!(
            bindings.calls,
            [
                "component",
                "retain component",
                "phantom",
                "enter",
                "leave",
                "position"
            ]
        );
        assert_eq!(objects.phantoms[&phantom].header.references, 2);
        let listener = state.listener.unwrap();
        assert_ne!(listener, phantom);
        assert_eq!(
            objects.listeners[&listener],
            super::super::hk_reference::Header {
                memory_size: 12,
                references: 1
            }
        );
        assert_eq!(
            objects.phantoms[&phantom].overlaps.listeners,
            vec![Some(listener)]
        );
        assert_eq!(
            (state.physics_component, state.phantom),
            (Some(40), Some(phantom))
        );
        assert_eq!((bob.phase, bob.base_height), (2., 7.));
        // Failure after registration leaves earlier native publications intact.
        let mut next = State::default();
        bindings.fail = true;
        assert!(initialize(
            &mut next,
            &mut bob,
            &mut SharedInitialize {
                objects: &mut objects,
                bindings: &mut bindings
            }
        )
        .is_err());
        assert_eq!(objects.phantoms[&phantom].header.references, 3);
        assert_eq!(
            objects.phantoms[&phantom].overlaps.listeners,
            vec![Some(listener), next.listener]
        );
        assert_eq!((bob.phase, bob.base_height), (2., 7.));
        Ok(())
    }
    #[test]
    fn teardown_unregisters_before_release_and_preserves_state_on_failure() -> Result<()> {
        struct Backend {
            calls: Vec<&'static str>,
            fail: bool,
        }
        impl Teardown for Backend {
            fn base(&mut self, _: &mut State) -> Result<()> {
                self.calls.push("base");
                Ok(())
            }
            fn unregister(&mut self, p: usize, l: Option<usize>) -> Result<()> {
                assert_eq!((p, l), (3, Some(4)));
                self.calls.push("unregister");
                Ok(())
            }
            fn release_phantom(&mut self, p: usize) -> Result<()> {
                assert_eq!(p, 3);
                self.calls.push("phantom");
                Ok(())
            }
            fn release_listener(&mut self, l: usize) -> Result<()> {
                assert_eq!(l, 4);
                self.calls.push("listener");
                if self.fail {
                    anyhow::bail!("Missing release backend")
                };
                Ok(())
            }
            fn release_component(&mut self, c: usize) -> Result<()> {
                assert_eq!(c, 2);
                self.calls.push("component");
                Ok(())
            }
        }
        let mut state = State {
            physics_component: Some(2),
            phantom: Some(3),
            listener: Some(4),
        };
        let mut backend = Backend {
            calls: vec![],
            fail: false,
        };
        teardown(&mut state, &mut backend)?;
        assert_eq!(
            backend.calls,
            vec!["base", "unregister", "phantom", "listener", "component"]
        );
        assert_eq!(
            (state.physics_component, state.phantom, state.listener),
            (None, None, None)
        );
        state = State {
            physics_component: Some(2),
            phantom: Some(3),
            listener: Some(4),
        };
        backend.fail = true;
        backend.calls.clear();
        assert!(teardown(&mut state, &mut backend).is_err());
        assert_eq!(
            (state.physics_component, state.phantom, state.listener),
            (Some(2), None, Some(4))
        );
        assert_eq!(
            backend.calls,
            vec!["base", "unregister", "phantom", "listener"]
        );
        Ok(())
    }
    #[test]
    fn initialization_preserves_ownership_registration_and_baseline_order() -> Result<()> {
        struct Backend(Vec<&'static str>);
        impl Initialize for Backend {
            fn physics_component(&mut self) -> Result<Option<usize>> {
                self.0.push("query");
                Ok(Some(2))
            }
            fn release_component(&mut self, c: usize) -> Result<()> {
                assert_eq!(c, 1);
                self.0.push("release");
                Ok(())
            }
            fn retain_component(&mut self, c: usize) -> Result<()> {
                assert_eq!(c, 2);
                self.0.push("retain");
                Ok(())
            }
            fn phantom(&mut self, c: usize) -> Result<Option<usize>> {
                assert_eq!(c, 2);
                self.0.push("phantom");
                Ok(Some(3))
            }
            fn retain_phantom(&mut self, p: usize) -> Result<()> {
                assert_eq!(p, 3);
                self.0.push("retain phantom");
                Ok(())
            }
            fn allocate_listener(&mut self) -> Result<usize> {
                self.0.push("allocate");
                Ok(4)
            }
            fn register(&mut self, p: usize, l: usize) -> Result<()> {
                assert_eq!((p, l), (3, 4));
                self.0.push("register");
                Ok(())
            }
            fn position(&mut self) -> Result<[f32; 4]> {
                self.0.push("position");
                Ok([1., 7., 3., 0.])
            }
        }
        let mut state = State {
            physics_component: Some(1),
            ..Default::default()
        };
        let mut bob = super::super::pickup::Bob::default();
        let mut backend = Backend(vec![]);
        initialize(&mut state, &mut bob, &mut backend)?;
        assert_eq!(
            backend.0,
            vec![
                "query",
                "release",
                "retain",
                "phantom",
                "retain phantom",
                "allocate",
                "register",
                "position"
            ]
        );
        assert_eq!(
            (state.physics_component, state.phantom, state.listener),
            (Some(2), Some(3), Some(4))
        );
        assert_eq!(bob.base_height, 7.);
        assert_eq!(bob.phase, 0.);
        Ok(())
    }
}
