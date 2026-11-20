//! Deferred application transition destination, EM1 REL2f5a4 ->2a360.
//! globalscripts.lua's LoadLevel(entity,path) calls TransitionToLevel(path).
//! The application copies into one 1024-byte C buffer; a later call replaces
//! the pending destination. The Lua callback returns no values.
#[derive(Default, Debug)]
pub struct Pending {
    destination: Option<String>,
    clean: bool,
}
impl Pending {
    pub fn request(&mut self, path: &str) -> Result<(), &'static str> {
        self.schedule(path, false)
    }
    pub(super) fn request_clean(&mut self, path: &str) -> Result<(), &'static str> {
        self.schedule(path, true)
    }
    fn schedule(&mut self, path: &str, clean: bool) -> Result<(), &'static str> {
        let path = path.split('\0').next().unwrap_or("");
        if path.is_empty() {
            return Ok(());
        }
        // Native strncpy copies at most 1023 bytes before its explicit NUL.
        let path = path
            .get(..path.len().min(1023))
            .ok_or("Level destination truncates a UTF-8 character")?
            .replace('\\', "/");
        if path.starts_with('/') || path.contains(':') || path.split('/').any(|s| s == "..") {
            return Err("Level destination must be a relative game asset path");
        }
        self.destination = Some(path);
        self.clean = clean;
        Ok(())
    }
    pub fn take(&mut self) -> Option<(String, bool)> {
        self.destination.take().map(|path| (path, self.clean))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_single_slot_c_string_and_last_request() {
        let mut pending = Pending::default();
        pending
            .request("Levels\\DBC_Start_1st.Level\0ignored")
            .unwrap();
        pending.request("").unwrap();
        assert_eq!(
            pending.take().as_ref().map(|(path, _)| path.as_str()),
            Some("Levels/DBC_Start_1st.Level")
        );
        assert_eq!(pending.take(), None);
        pending.request("Levels/GV_ZoneB_Start.level").unwrap();
        pending.request("MeanStreet_V1.Level").unwrap();
        assert_eq!(pending.take().as_ref().map(|(path, _)| path.as_str()), Some("MeanStreet_V1.Level"));
    }
    #[test]
    fn later_transition_selects_its_own_clean_mode() -> Result<(), &'static str> {
        let mut pending = Pending::default();
        pending.request_clean("Levels/Main_Menu.level")?;
        pending.request("Levels/DBC_Start_1st.Level")?;
        assert_eq!(pending.take(), Some(("Levels/DBC_Start_1st.Level".into(), false)));
        pending.request_clean("Levels/Main_Menu.level")?;
        assert_eq!(pending.take(), Some(("Levels/Main_Menu.level".into(), true)));
        Ok(())
    }

    #[test]
    fn unsafe_paths_do_not_replace_valid_destination() {
        let mut pending = Pending::default();
        pending.request("Levels/DBC_Start_1st.Level").unwrap();
        for path in [
            "../outside.level",
            "Levels/../../outside.level",
            "D:\\outside.level",
            "/outside.level",
        ] {
            assert!(pending.request(path).is_err());
        }
        assert_eq!(
            pending.take().as_ref().map(|(path, _)| path.as_str()),
            Some("Levels/DBC_Start_1st.Level")
        );
    }
    #[test]
    fn native_destination_buffer_has_1023_payload_bytes() {
        let mut pending = Pending::default();
        pending.request(&"a".repeat(1100)).unwrap();
        assert_eq!(pending.take().unwrap().0.len(), 1023);
    }
}
