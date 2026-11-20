//! Havok-compatible referenced-object header operations, DOL80159c74/80159cf4.
use anyhow::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub memory_size: u16,
    pub references: i16,
}
pub trait Backend {
    /// Original global synchronization/ownership monitor branch is required.
    fn enter(&mut self) -> Result<()>;
    fn leave(&mut self) -> Result<()>;
    /// Virtual+8 with deleting flag1; header references already zero.
    fn destroy(&mut self, header: &mut Header) -> Result<()>;
}
/// Owned runtime's shared Havok reference monitor. Final destruction needs its
/// actual type backend; this adapter is used by construction/retain paths.
pub struct Monitor<'a> {
    pub mutex: &'a std::sync::Mutex<()>,
    pub held: Option<std::sync::MutexGuard<'a, ()>>,
}
impl Backend for Monitor<'_> {
    fn enter(&mut self) -> Result<()> {
        anyhow::ensure!(self.held.is_none(), "Havok reference monitor already held");
        self.held = Some(
            self.mutex
                .lock()
                .map_err(|_| anyhow::anyhow!("Havok reference monitor poisoned"))?,
        );
        Ok(())
    }
    fn leave(&mut self) -> Result<()> {
        anyhow::ensure!(
            self.held.take().is_some(),
            "Havok reference monitor not held"
        );
        Ok(())
    }
    fn destroy(&mut self, _: &mut Header) -> Result<()> {
        anyhow::bail!("Referenced-object destruction requires the actual type backend")
    }
}
pub fn retain(header: &mut Header, backend: &mut impl Backend) -> Result<()> {
    if header.memory_size == 0 {
        return Ok(());
    }
    backend.enter()?;
    header.references = header.references.wrapping_add(1);
    backend.leave()
}
pub fn release(header: &mut Header, backend: &mut impl Backend) -> Result<()> {
    if header.memory_size == 0 {
        return Ok(());
    }
    if header.references == 1 {
        header.references = 0;
        return backend.destroy(header);
    }
    backend.enter()?;
    header.references = header.references.wrapping_sub(1);
    backend.leave()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_operations_preserve_size_gate_signed_wrap_and_final_release() -> Result<()> {
        struct Hooks(Vec<&'static str>);
        impl Backend for Hooks {
            fn enter(&mut self) -> Result<()> {
                self.0.push("enter");
                Ok(())
            }
            fn leave(&mut self) -> Result<()> {
                self.0.push("leave");
                Ok(())
            }
            fn destroy(&mut self, h: &mut Header) -> Result<()> {
                assert_eq!(h.references, 0);
                self.0.push("destroy");
                Ok(())
            }
        }
        let mut hooks = Hooks(vec![]);
        let mut header = Header {
            memory_size: 0,
            references: 1,
        };
        retain(&mut header, &mut hooks)?;
        release(&mut header, &mut hooks)?;
        assert_eq!(header.references, 1);
        assert!(hooks.0.is_empty());
        header.memory_size = 12;
        retain(&mut header, &mut hooks)?;
        assert_eq!(header.references, 2);
        release(&mut header, &mut hooks)?;
        release(&mut header, &mut hooks)?;
        assert_eq!(hooks.0, vec!["enter", "leave", "enter", "leave", "destroy"]);
        hooks.0.clear();
        header.references = i16::MAX;
        retain(&mut header, &mut hooks)?;
        assert_eq!(header.references, i16::MIN);
        release(&mut header, &mut hooks)?;
        assert_eq!(header.references, i16::MAX);
        header.references = 0;
        release(&mut header, &mut hooks)?;
        assert_eq!(header.references, -1);
        assert!(!hooks.0.contains(&"destroy"));
        Ok(())
    }
}
