//! Parent assignment protocol, REL3538a8..353a64. Backend owns typed handle
//! storage, admission, detach synchronization and component notifications.
use anyhow::Result;
pub trait Backend {
    fn flags_20(&mut self) -> Result<u8>;
    fn current_handle(&mut self) -> Result<u32>;
    // Requested storage is reread after callbacks, matching native r30 loads.
    fn requested_handle(&mut self) -> Result<u32>;
    fn admit(&mut self) -> Result<bool>; //353ee0, receives original request holder
    fn detach(&mut self) -> Result<()>; //353dc4
    fn assign(&mut self) -> Result<()>; //12455c, original request holder
    fn parent_transform(&mut self, handle: u32) -> Result<usize>;
    fn own_entity(&mut self) -> Result<Option<usize>>;
    fn entity_handle(&mut self, entity: usize) -> Result<u32>;
    fn register_child(&mut self, parent: usize, child: u32) -> Result<()>; //3536b8
    fn component_3764(&mut self, parent: u32) -> Result<Option<usize>>;
    fn attach_312bf4(&mut self, component: usize, entity: Option<usize>) -> Result<()>;
    fn update_353be4(&mut self, flag: u32) -> Result<bool>;
    fn notify_298d24(&mut self, parent: u32, entity: Option<usize>) -> Result<()>;
}
fn register(backend: &mut impl Backend, handle: u32) -> Result<()> {
    let parent = backend.parent_transform(handle)?;
    let entity = backend.own_entity()?;
    let child = match entity {
        Some(e) => backend.entity_handle(e)?,
        None => 0,
    };
    backend.register_child(parent, child)
}
pub fn set(backend: &mut impl Backend) -> Result<()> {
    if backend.flags_20()? & 8 != 0 {
        backend.assign()?;
        let handle = backend.requested_handle()?;
        // Locked native path queries even a null parent; don't silently skip it.
        return register(backend, handle);
    }
    let old = backend.current_handle()?;
    if old == backend.requested_handle()? {
        return Ok(());
    }
    if !backend.admit()? {
        return Ok(());
    }
    if old != 0 {
        backend.detach()?;
    }
    backend.assign()?;
    let handle = backend.requested_handle()?;
    if handle != 0 {
        register(backend, handle)?;
    }
    let handle = backend.requested_handle()?;
    if let Some(component) = backend.component_3764(handle)? {
        let entity = backend.own_entity()?;
        backend.attach_312bf4(component, entity)?;
    }
    if backend.update_353be4(0)? {
        let entity = backend.own_entity()?;
        let handle = backend.requested_handle()?;
        backend.notify_298d24(handle, entity)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    struct State {
        flags: u8,
        old: u32,
        request: u32,
        admit: bool,
        fail: bool,
        calls: Vec<String>,
    }
    impl Backend for State {
        fn flags_20(&mut self) -> Result<u8> {
            self.calls.push("flags".into());
            Ok(self.flags)
        }
        fn current_handle(&mut self) -> Result<u32> {
            self.calls.push("current".into());
            Ok(self.old)
        }
        fn requested_handle(&mut self) -> Result<u32> {
            self.calls.push("requested".into());
            Ok(self.request)
        }
        fn admit(&mut self) -> Result<bool> {
            self.calls.push("admit".into());
            Ok(self.admit)
        }
        fn detach(&mut self) -> Result<()> {
            self.calls.push("detach".into());
            Ok(())
        }
        fn assign(&mut self) -> Result<()> {
            self.calls.push("assign".into());
            self.old = self.request;
            Ok(())
        }
        fn parent_transform(&mut self, h: u32) -> Result<usize> {
            self.calls.push(format!("parent {h}"));
            if self.fail {
                anyhow::bail!("Missing parent transform")
            };
            Ok(100)
        }
        fn own_entity(&mut self) -> Result<Option<usize>> {
            self.calls.push("entity".into());
            Ok(Some(7))
        }
        fn entity_handle(&mut self, e: usize) -> Result<u32> {
            assert_eq!(e, 7);
            self.calls.push("entity handle".into());
            Ok(70)
        }
        fn register_child(&mut self, p: usize, c: u32) -> Result<()> {
            assert_eq!((p, c), (100, 70));
            self.calls.push("register".into());
            self.request = 3;
            Ok(())
        }
        fn component_3764(&mut self, p: u32) -> Result<Option<usize>> {
            self.calls.push(format!("component {p}"));
            Ok(Some(200))
        }
        fn attach_312bf4(&mut self, c: usize, e: Option<usize>) -> Result<()> {
            assert_eq!((c, e), (200, Some(7)));
            self.calls.push("attach".into());
            Ok(())
        }
        fn update_353be4(&mut self, f: u32) -> Result<bool> {
            assert_eq!(f, 0);
            self.calls.push("update".into());
            self.request = 4;
            Ok(true)
        }
        fn notify_298d24(&mut self, p: u32, e: Option<usize>) -> Result<()> {
            assert_eq!((p, e), (4, Some(7)));
            self.calls.push("notify".into());
            Ok(())
        }
    }
    #[test]
    fn native_parent_branch_order_and_fresh_request() -> Result<()> {
        let mut s = State {
            flags: 0,
            old: 1,
            request: 2,
            admit: true,
            fail: false,
            calls: vec![],
        };
        set(&mut s)?;
        assert_eq!(
            s.calls,
            [
                "flags",
                "current",
                "requested",
                "admit",
                "detach",
                "assign",
                "requested",
                "parent 2",
                "entity",
                "entity handle",
                "register",
                "requested",
                "component 3",
                "entity",
                "attach",
                "update",
                "entity",
                "requested",
                "notify"
            ]
        );
        s.calls.clear();
        s.request = s.old;
        set(&mut s)?;
        assert_eq!(s.calls, ["flags", "current", "requested"]);
        s.calls.clear();
        s.request = 5;
        s.admit = false;
        set(&mut s)?;
        assert_eq!(s.calls, ["flags", "current", "requested", "admit"]);
        s.calls.clear();
        s.flags = 8;
        s.request = 0;
        s.fail = true;
        assert!(set(&mut s).is_err());
        assert_eq!(s.old, 0);
        assert_eq!(s.calls, ["flags", "assign", "requested", "parent 0"]);
        Ok(())
    }
}
