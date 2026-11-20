//! Ordinary CLB owner, REL110ffc/111124/11125c/1126d8/111978.
//! Ownership is connected to gameplay; UI request delivery and complete
//! checkpoint orchestration remain unfinished and are reported by World.
use anyhow::{ensure, Result};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Entry {
    pub kind: String,
    pub name: String,
    pub icon: String,
    pub count: i32,
}
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum Route {
    CollectibleQueue,
    UiEventManager,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Notification {
    pub route: Route,
    pub event: &'static str,
    pub title_key: String,
    pub icon: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_key: Option<String>,
}
impl Notification {
    pub fn new(route: Route, event: &'static str, name: &str, icon: &str) -> Result<Self> {
        // Native112228 copies at most1016 bytes, then appends _Name.
        let prefix = &name.as_bytes()[..name.len().min(0x3f8)];
        Ok(Self {
            route,
            event,
            title_key: format!("{}_Name", std::str::from_utf8(prefix)?),
            icon: icon.into(),
            description_key: None,
        })
    }
}
#[derive(Clone, Debug)]
pub struct State {
    pub entries: Vec<Entry>,
    names: BTreeMap<String, usize>,
    pub notifications: Vec<Notification>,
}
impl State {
    pub fn new(entries: Vec<Entry>) -> Self {
        let names = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.name.clone(), i))
            .collect();
        Self {
            entries,
            names,
            notifications: Vec::new(),
        }
    }
    pub fn has(&self, name: &str) -> bool {
        self.names
            .get(name)
            .is_some_and(|&i| self.entries[i].count > 0)
    }
    /// Native11125c changes boolean ownership without reducing an existing
    /// positive count. Notification is independent of whether the value changed.
    pub fn set_owned(
        &mut self,
        name: &str,
        owned: bool,
        notify: bool,
        immediate: bool,
    ) -> Result<()> {
        let Some(&index) = self.names.get(name) else {
            return Ok(());
        };
        let e = &mut self.entries[index];
        if owned && e.count <= 0 {
            e.count = 1;
        } else if !owned && e.count > 0 {
            e.count = 0;
        }
        if notify {
            self.notify(
                index,
                if immediate {
                    Route::UiEventManager
                } else {
                    Route::CollectibleQueue
                },
            )?;
        }
        Ok(())
    }
    /// Native1126d8 increments exactly once, with wrapping GPR arithmetic.
    pub fn increment(&mut self, name: &str) -> Result<()> {
        let Some(&index) = self.names.get(name) else {
            return Ok(());
        };
        self.entries[index].count = self.entries[index].count.wrapping_add(1);
        self.notify(index, Route::UiEventManager)
    }
    ///112800/1126a0: raw named count access; no ownership normalization/UI.
    pub(super) fn named_count(&self,name:&str)->i32 {self.names.get(name).map_or(0,|&i|self.entries[i].count)}
    pub(super) fn set_named_count(&mut self,name:&str,count:i32) {if let Some(&i)=self.names.get(name) {self.entries[i].count=count;}}
    fn notify(&mut self, index: usize, route: Route) -> Result<()> {
        let e = &self.entries[index];
        self.notifications.push(Notification::new(
            route,
            if e.kind == "Film_Reel" {
                "UI_NewFilmReceived"
            } else {
                "UI_NewItemReceived"
            },
            &e.name,
            &e.icon,
        )?);
        Ok(())
    }
    /// Native112834 sums all matching counts and separately counts matching
    /// definitions, including unowned and negative-count records.
    pub fn totals(&self, kind: &str) -> (i32, u32) {
        self.entries
            .iter()
            .filter(|e| e.kind == kind)
            .fold((0i32, 0u32), |(sum, n), e| {
                (sum.wrapping_add(e.count), n + 1)
            })
    }
    fn validate_header(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() >= super::save_game::HEADER_SIZE
                && bytes.starts_with(b"GAME")
                && bytes[4..8] == 16u32.to_be_bytes(),
            "Expected GAME v16 header"
        );
        ensure!(
            self.entries.len() <= 0xc0,
            "Collectibles exceed native GAME header allocation"
        );
        Ok(())
    }
    /// COLL2 has no section payload. Native1119c4 writes low bytes at GAME+1d8.
    pub fn write_save_header(&self, bytes: &mut [u8]) -> Result<()> {
        self.validate_header(bytes)?;
        for (byte, entry) in bytes[0x1d8..].iter_mut().zip(&self.entries) {
            *byte = entry.count as u8;
        }
        Ok(())
    }
    /// Native111a18 sign-extends each stored byte. Its selective mode preserves
    /// Quest_Object counts; do not infer that mode from a missing/empty section.
    pub fn restore_save_header(&mut self, bytes: &[u8], include_quest_objects: bool) -> Result<()> {
        self.validate_header(bytes)?;
        for (entry, &byte) in self.entries.iter_mut().zip(&bytes[0x1d8..]) {
            if include_quest_objects || entry.kind != "Quest_Object" {
                entry.count = i32::from(byte as i8);
            }
        }
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"defined":self.entries.len(),"owned":self.entries.iter().filter(|e|e.count>0).count(),
            "counts":self.entries.iter().filter(|e|e.count!=0).map(|e|serde_json::json!({"name":e.name,"kind":e.kind,"count":e.count})).collect::<Vec<_>>(),
            "notifications_pending_delivery":self.notifications})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> State {
        State::new(
            [
                ("Film_Reel", "FR_Test"),
                ("Quest_Object", "Bell"),
                ("Pin_Inc", "Bronze_Pin"),
            ]
            .into_iter()
            .map(|(kind, name)| Entry {
                kind: kind.into(),
                name: name.into(),
                icon: "icon.nif".into(),
                count: 0,
            })
            .collect(),
        )
    }
    fn header() -> Vec<u8> {
        let mut b = vec![0xa4; super::super::save_game::HEADER_SIZE];
        b[..4].copy_from_slice(b"GAME");
        b[4..8].copy_from_slice(&16u32.to_be_bytes());
        b
    }
    #[test]
    fn ownership_counts_and_notification_routes_follow_native_receivers() -> Result<()> {
        let mut s = state();
        assert!(!s.has("FR_Test"));
        for _ in 0..2 {
            s.set_owned("FR_Test", true, true, false)?;
        }
        assert_eq!(s.entries[0].count, 1);
        assert_eq!(s.notifications.len(), 2);
        assert_eq!(s.notifications[0].route, Route::CollectibleQueue);
        assert_eq!(s.notifications[0].event, "UI_NewFilmReceived");
        assert_eq!(s.notifications[0].title_key, "FR_Test_Name");
        s.increment("FR_Test")?;
        assert_eq!(s.entries[0].count, 2);
        assert_eq!(s.notifications[2].route, Route::UiEventManager);
        s.increment("Bell")?;
        assert_eq!(s.notifications[3].event, "UI_NewItemReceived");
        assert!(!s.has("fr_test"));
        s.increment("unknown")?;
        assert_eq!(s.notifications.len(), 4);
        s.entries[2].count = i32::MAX;
        s.increment("Bronze_Pin")?;
        assert_eq!(s.entries[2].count, i32::MIN);
        assert!(!s.has("Bronze_Pin"));
        s.set_owned("Bronze_Pin", false, false, false)?;
        assert_eq!(s.entries[2].count, i32::MIN);
        s.set_owned("Bronze_Pin", true, false, false)?;
        assert_eq!(s.entries[2].count, 1);
        assert_eq!(s.totals("Film_Reel"), (2, 1));
        assert_eq!(s.totals("unknown"), (0, 0));
        Ok(())
    }
    #[test]
    fn header_bytes_are_signed_on_restore_selective_and_other_owners_survive() -> Result<()> {
        let mut s = state();
        let mut b = header();
        let before = b.clone();
        s.entries[0].count = 255;
        s.entries[1].count = 2;
        s.entries[2].count = 128;
        s.write_save_header(&mut b)?;
        assert_eq!(&b[0x1d8..0x1db], &[255, 2, 128]);
        assert_eq!(b[..0x1d8], before[..0x1d8]);
        assert_eq!(b[0x1db..], before[0x1db..]);
        s.entries[1].count = 17;
        s.restore_save_header(&b, false)?;
        assert_eq!(
            s.entries.iter().map(|e| e.count).collect::<Vec<_>>(),
            [-1, 17, -128]
        );
        s.restore_save_header(&b, true)?;
        assert_eq!(s.entries[1].count, 2);
        assert!(!s.has("FR_Test"));
        assert!(s.has("Bell"));
        assert!(s.restore_save_header(&b[..0x319], true).is_err());
        Ok(())
    }
}
