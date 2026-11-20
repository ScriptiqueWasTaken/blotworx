//! Part resource records29474c; WRLD selection295248/295cf0.
//! Initial level loading28fc90..28fdc0 ignores inactive options for BOTH parts.
use super::scene_loading::Requests;
use anyhow::Result;
use std::collections::BTreeSet;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Resource {
    pub name: String,
    pub flags: u32,
    pub kind: u32,
}
/// Native2904a8 tokenizes a lowercased part line;29474c retains gsa0/bnk1.
/// Other extensions are not retained by that native resource-list constructor.
pub fn part_records(text: &str, force_active: bool) -> Vec<Resource> {
    let mut seen = BTreeSet::new();
    text.split('\0')
        .next()
        .unwrap_or("")
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with(['#', ';']) || line.starts_with("//") {
                return None;
            }
            let mut words = line.split_ascii_whitespace();
            let name = words.next()?.to_ascii_lowercase();
            let kind = if name.ends_with(".gsa") {
                0
            } else if name.ends_with(".bnk") {
                1
            } else {
                return None;
            };
            let inactive = words.any(|word| {
                word.eq_ignore_ascii_case("-i") || word.eq_ignore_ascii_case("-inactive")
            });
            //2948d0..29493c keeps the first (interned name, type) in this
            // part array, independently of flags. The other part is separate.
            if !seen.insert((name.clone(), kind)) {
                return None;
            }
            Some(Resource {
                name,
                flags: if !force_active && inactive {
                    0x80000000
                } else {
                    0
                },
                kind,
            })
        })
        .collect()
}
#[derive(Default, Debug, serde::Serialize)]
pub struct State {
    pub shared: Option<String>,
    pub parts: [Vec<Resource>; 2],
    active: BTreeSet<String>,
    available: BTreeSet<String>,
    loading: Option<String>,
}
pub(super) fn scene_name(path: &str) -> String {
    let path = path
        .split('\0')
        .next()
        .unwrap_or("")
        .replace('\\', "/")
        .to_ascii_lowercase();
    path.strip_suffix(".bin")
        .map(|stem| format!("{stem}.gsa"))
        .unwrap_or(path)
}
///1808f0 queries the scene-manager basename key; resource paths and queued
///filenames retain their separate full identities.
fn active_name(active: &BTreeSet<String>, path: &str) -> bool {
    active.contains(super::scene_key::key(path).as_ref())
}

impl State {
    /// Replace one descriptor slot, retaining shared and other-slot resources.
    /// Return outgoing scenes so the World withdraws their live registrations.
    pub fn replace_part(&mut self,slot:usize,records:Vec<Resource>)->Vec<String> {
        let old=std::mem::replace(&mut self.parts[slot],records);
        old.into_iter().filter(|resource| resource.kind==0
            && self.shared.as_deref()!=Some(resource.name.as_str())
            && !self.parts.iter().flatten().any(|other|other.kind==0 && other.name==resource.name))
            .map(|resource|resource.name).collect()
    }
    pub fn bootstrap(shared: Option<&str>, parts: &[String; 2]) -> Self {
        //28fbc4 uses2901ec for the .level too: DOL800249fc lowercases
        // each descriptor line before its key/value callback.
        Self {
            shared: shared.map(|name| name.split('\0').next().unwrap_or("").to_ascii_lowercase()),
            parts: parts.each_ref().map(|part| part_records(part, true)),
            ..Default::default()
        }
    }
    /// Publication succeeds only after source/component/physics construction.
    /// The current importer is synchronous; there is no active async loader
    /// when Lua can request a checkpoint. Unloading is not implemented here.
    pub fn published(&mut self, path: &str) {
        let name = scene_name(path);
        self.available.insert(name.clone());
        self.active.insert(super::scene_key::key(path).into_owned());
    }
    pub fn deactivated(&mut self, path: &str) {
        self.active.remove(super::scene_key::key(path).as_ref());
    }
    pub fn is_active(&self, path: &str) -> bool {
        active_name(&self.active,path)
    }
    fn eligible(&self, resource: &Resource, pending: &Requests) -> bool {
        //295248: active entity-manager scene, current loader, queued exact
        // interned name, otherwise resource absence plus nonnegative flags.
        active_name(&self.active, &resource.name)
            || self.loading.as_deref() == Some(resource.name.as_str())
            || pending.names().any(|name| name == resource.name.as_str())
            || (!self.available.contains(&resource.name) && resource.flags & 0x80000000 == 0)
    }
    /// Shared is prepended when the descriptor key exists. Traverse initial
    /// then interior records, preserving first occurrence and exact identity.
    pub fn save_names(&self, pending: &Requests) -> Vec<String> {
        let mut names = self.shared.iter().cloned().collect::<Vec<_>>();
        for resource in self.parts.iter().flatten() {
            if resource.kind != 0 || names.contains(&resource.name) {
                continue;
            }
            if self.eligible(resource, pending) {
                names.push(resource.name.clone());
            }
        }
        names
    }
    pub fn save_payload(&self, pending: &Requests) -> Result<Vec<u8>> {
        super::save_world::encode(&self.save_names(pending))
    }
    pub fn summary(&self, pending: &Requests) -> serde_json::Value {
        let serialization=match self.save_payload(pending).and_then(|payload| {
            Ok(serde_json::json!({"payload_bytes":payload.len(),"record_count":super::save_game::records(&payload)?.len()}))
        }) {Ok(value)=>value,Err(error)=>serde_json::json!({"error":format!("{error:#}")})};
        serde_json::json!({"owner":self,"save_names":self.save_names(pending),"pending":pending.names().collect::<Vec<_>>(),"serialization":serialization,
            "bootstrap_ignores_inactive_flags":true,"part_replacement_implemented":false,
            "runtime_scene_visibility_switching_connected":true,"storage_unloading_implemented":false,
            "unloading_implemented":false,"native_loader_timing_verified":false,"checkpoint_integrated":false})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_scene_group_keys_are_separate_from_resource_filenames() {
        let mut state = State::bootstrap(None, &[String::new(), String::new()]);
        state.parts[0] = part_records("Scenes/Area.Variant.gsa -i\nOther/Area.Variant.gsa -i\nOther.gsa -i", false);
        state.published("Scenes\\AREA.VARIANT.BIN");
        assert!(state.is_active("AREA.VARIANT.GSA"));
        assert!(!state.is_active("area.variant")); // Last suffix is treated as an extension.
        assert!(state.is_active("Other/Area.Variant.gsa"));
        assert!(!state.is_active("area.gsa"));
        assert_eq!(state.save_names(&Requests::default()), ["scenes/area.variant.gsa", "other/area.variant.gsa"]);
        assert!(state.available.contains("scenes/area.variant.gsa"));
        assert!(!state.available.contains("other/area.variant.gsa"));
        state.deactivated("AREA.VARIANT.GSA");
        assert!(!state.is_active("Scenes/Area.Variant.bin"));
        assert!(state.save_names(&Requests::default()).is_empty());
        assert!(state.available.contains("scenes/area.variant.gsa"));
    }
    #[test]
    fn part_replacement_keeps_shared_and_other_slot_resources() {
        let mut s=State::bootstrap(Some("shared.gsa"),&["shared.gsa\nold.gsa\nairlock.gsa".into(),"airlock.gsa".into()]);
        assert_eq!(s.replace_part(0,part_records("next.gsa",false)),["old.gsa"]);
        assert_eq!(s.parts[0][0].name,"next.gsa");assert_eq!(s.parts[1][0].name,"airlock.gsa");
    }
    #[test]
    fn bootstrap_override_preserves_both_part_lists_and_streaming_inactive_flags() {
        let text="# comment\r\nWorld/A.gsa -i\nworld/B.GSA -INACTIVE\nglobal.bnk\nWORLD/A.GSA\n// comment\nother.txt\n";
        let streaming = part_records(text, false);
        assert_eq!(streaming.len(), 3);
        assert_eq!(
            streaming
                .iter()
                .map(|record| (record.kind, record.flags))
                .collect::<Vec<_>>(),
            [(0, 0x80000000), (0, 0x80000000), (1, 0)]
        );
        let state = State::bootstrap(Some("World/SHARED.gsa"), &[text.into(), text.into()]);
        assert!(state.parts.iter().flatten().all(|record| record.flags == 0));
        assert_eq!(
            state.save_names(&Requests::default()),
            ["world/shared.gsa", "world/a.gsa", "world/b.gsa"]
        );
    }
    #[test]
    fn native_selection_distinguishes_active_loading_pending_available_and_inactive() -> Result<()>
    {
        let mut state = State::bootstrap(Some("shared.gsa"), &[String::new(), String::new()]);
        state.parts[0] = part_records(
            "a.gsa -i\nb.gsa -i\nc.gsa -i\nd.gsa -i\nd.gsa\ne.gsa\nf.gsa\na.gsa\nshared.gsa\na.bnk",
            false,
        );
        state.parts[1] = part_records("b.gsa\ng.gsa", false);
        state.published("a.bin");
        state.loading = Some("b.gsa".into());
        let mut pending = Requests::default();
        pending.request("c.gsa");
        state.available.insert("e.gsa".into());
        assert_eq!(
            state.save_names(&pending),
            ["shared.gsa", "a.gsa", "b.gsa", "c.gsa", "f.gsa", "g.gsa"]
        );
        assert_eq!(
            super::super::save_world::decode(&state.save_payload(&pending)?)?,
            state.save_names(&pending)
        );
        pending.cancel("c.gsa");
        assert!(!state
            .save_names(&pending)
            .iter()
            .any(|name| name == "c.gsa"));
        let mut changed_case = Requests::default();
        changed_case.request("C.gsa");
        assert!(!state
            .save_names(&changed_case)
            .iter()
            .any(|name| name == "c.gsa"));
        Ok(())
    }
    #[test]
    fn streamed_deactivation_removes_saved_scene_until_reactivated() -> Result<()> {
        let mut state = State::bootstrap(Some("shared.gsa"), &["area.gsa".into(), String::new()]);
        state.published("area.bin");
        assert_eq!(
            state.save_names(&Requests::default()),
            ["shared.gsa", "area.gsa"]
        );
        state.deactivated("AREA.GSA");
        assert_eq!(state.save_names(&Requests::default()), ["shared.gsa"]);
        state.published("area.gsa");
        assert_eq!(
            state.save_names(&Requests::default()),
            ["shared.gsa", "area.gsa"]
        );
        Ok(())
    }
    #[test]
    fn streamed_world_save_uses_part_order_and_excludes_unlisted_active_scenes() -> Result<()> {
        let mut state = State::bootstrap(Some("shared.gsa"), &[String::new(), String::new()]);
        state.parts[0]=part_records("zonea.gsa\nairlock.gsa",false);
        state.parts[1]=part_records("zoneb.gsa",false);
        state.published("shared.gsa");
        state.published("zonea.bin");
        state.published("zoneb.bin");
        state.published("airlock.bin");
        state.deactivated("zonea.gsa");
        state.published("zoneb.bin");
        state.published("unlisted.bin");
        assert_eq!(
            state.save_names(&Requests::default()),
            ["shared.gsa", "airlock.gsa", "zoneb.gsa"]
        );
        state.deactivated("airlock.bin");
        assert_eq!(
            state.save_names(&Requests::default()),
            ["shared.gsa", "zoneb.gsa"]
        );
        Ok(())
    }
    #[test]
    fn world_scene_list_matches_original_two_part_enumeration()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/world_scene_list.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let input=&case["input"];
            let names=|key:&str|input[key].as_array().unwrap().iter().map(|v|v.as_str().unwrap().to_owned()).collect::<BTreeSet<_>>();
            let mut state=State::default();
            state.shared=input["shared"].as_str().map(str::to_owned);
            state.active=names("active").iter().map(|s|super::super::scene_key::key(s).into_owned()).collect();state.available=names("available");
            state.loading=input["loading"].as_str().map(str::to_owned);
            for (slot,part) in input["parts"].as_array().unwrap().iter().enumerate() {
                state.parts[slot]=part.as_array().unwrap().iter().map(|r|Resource {
                    name:r["name"].as_str().unwrap().to_owned(),kind:r["kind"].as_u64().unwrap() as u32,
                    flags:r["flags"].as_u64().unwrap() as u32,
                }).collect();
            }
            let mut pending=Requests::default();
            for name in input["pending"].as_array().unwrap() {pending.request(name.as_str().unwrap());}
            assert_eq!(serde_json::to_value(state.save_names(&pending))?,case["names"],"{input}");
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn original_gv_scene_owner_keeps_part_order_separate_from_import_report() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_DATA"))?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let expected = [
            "environments/thepark/fantasyland/smallworld/gsa/gv_shared_za.gsa",
            "environments/thepark/fantasyland/smallworld/gsa/gremlinvillage_zonea.gsa",
            "environments/thepark/fantasyland/smallworld/gsa/gv_zonea_airlock.gsa",
        ];
        assert_eq!(
            world.scene_resources.save_names(&world.scene_requests),
            expected
        );
        assert_eq!(
            world.scene_resources.parts[0]
                .iter()
                .filter(|record| record.kind == 0)
                .count(),
            1
        );
        assert_eq!(
            world.scene_resources.parts[1]
                .iter()
                .filter(|record| record.kind == 0)
                .count(),
            1
        );
        assert!(world
            .scene_resources
            .parts
            .iter()
            .flatten()
            .all(|record| record.flags == 0));
        assert_eq!(
            super::super::save_world::decode(
                &world.scene_resources.save_payload(&world.scene_requests)?
            )?,
            expected
        );
        world.report.scenes.clear();
        assert_eq!(
            world.scene_resources.save_names(&world.scene_requests),
            expected
        );
        let zone = expected[1];
        world.scene_resources.parts[0][0].flags = 0x80000000;
        world.scene_resources.deactivated(zone);
        assert_eq!(
            world.scene_resources.save_names(&world.scene_requests),
            [expected[0], expected[2]]
        );
        world.scene_requests.request(zone);
        assert_eq!(
            world.scene_resources.save_names(&world.scene_requests),
            expected
        );
        Ok(())
    }
}
