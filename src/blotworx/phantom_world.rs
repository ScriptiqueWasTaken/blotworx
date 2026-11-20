//! EM1 PhysicsPhantomComponent membership guards, REL377be8/377c44.
//! The caller supplies reconstructed world/physics-system handles and actual physics
//! operations. Initialization REL377abc clears byte+55 after configuration.
use anyhow::Result;
#[derive(Clone, Copy, Debug)]
pub struct Membership {
    pub added: bool,
}
impl Membership {
    /// Post-configuration state, not the allocation constructor state.
    pub fn configured() -> Self {
        Self { added: false }
    }
    pub fn attach(
        &mut self,
        world: Option<usize>,
        system: Option<usize>,
        mut add: impl FnMut(usize, usize) -> Result<()>,
    ) -> Result<bool> {
        let (Some(world), Some(system)) = (world, system) else {
            return Ok(false);
        };
        if self.added {
            return Ok(false);
        };
        add(world, system)?;
        self.added = true;
        Ok(true)
    }
    pub fn detach(
        &mut self,
        world: Option<usize>,
        system: Option<usize>,
        mut remove: impl FnMut(usize, usize) -> Result<()>,
    ) -> Result<bool> {
        let (Some(world), Some(system)) = (world, system) else {
            return Ok(false);
        };
        if !self.added {
            return Ok(false);
        };
        remove(world, system)?;
        self.added = false;
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_membership_guards_and_callback_order() -> Result<()> {
        let mut membership = Membership { added: false };
        let mut calls = Vec::new();
        assert!(!membership.attach(None, Some(2), |_, _| panic!("No world"))?);
        assert!(!membership.attach(Some(1), None, |_, _| panic!("No phantom"))?);
        assert!(membership.attach(Some(1), Some(2), |w, p| {
            calls.push((w, p));
            Ok(())
        })?);
        assert!(!membership.attach(Some(1), Some(2), |_, _| panic!("Already added"))?);
        assert!(!membership.detach(None, Some(2), |_, _| panic!("No world"))?);
        assert!(membership.added);
        assert!(membership.detach(Some(1), Some(2), |w, p| {
            calls.push((w, p));
            Ok(())
        })?);
        assert!(!membership.detach(Some(1), Some(2), |_, _| panic!("Already removed"))?);
        assert_eq!(calls, [(1, 2), (1, 2)]);
        Ok(())
    }
    #[test]
    fn unsupported_physics_callbacks_never_commit_membership() -> Result<()> {
        let mut membership = Membership { added: false };
        assert!(membership
            .attach(Some(1), Some(2), |_, _| anyhow::bail!("Missing world add"))
            .is_err());
        assert!(!membership.added);
        membership.added = true;
        assert!(membership
            .detach(Some(1), Some(2), |_, _| anyhow::bail!(
                "Missing world removal"
            ))
            .is_err());
        assert!(membership.added);
        Ok(())
    }
}
