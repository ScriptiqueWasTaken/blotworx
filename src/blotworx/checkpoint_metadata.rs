//! Level-resource names28f968/28fb1c/28fde0 and checkpoint header2bd60..2be80.
//! These are live owners, not a complete save or gameplay restoration path.
use anyhow::{ensure, Context, Result};

const WIDTH: usize = 60;
const OFFSETS: [usize; 4] = [0x10, 0x4c, 0x88, 0xc4];

/// Exact lookup/write order in34e60..352dc, header298..2b0 inclusive.
pub const GLOBAL_NAMES: [&str; 25] = [
    "FR_TotalReels",
    "UsherUnlockReceived",
    "MS_SketchAnvilCount",
    "MS_SketchTVCount",
    "MS_SketchWatchCount",
    "HavePin_Wasteland",
    "HavePin_PiratesOfTheWasteland",
    "HavePin_GremlinVillage",
    "HavePin_MeanStreets",
    "HavePin_Tomorrowland",
    "HavePin_LonesomeManor",
    "Have_ConceptArt_15_Mickeys_Mistake",
    "Have_ConceptArt_20_Bog_Easy",
    "GotWalletLarge",
    "MS_wallet1",
    "MS_wallet2",
    "MS_ArtUnlock1",
    "MS_ArtUnlock2",
    "MS_ArtUnlock3",
    "MS_ArtUnlock4",
    "MS_ArtUnlock5",
    "MS_ArtUnlock6",
    "MS_ArtUnlock7",
    "MS_ArtUnlock8",
    "MS_ArtUnlock9",
];
#[derive(Clone, Debug, PartialEq)]
pub struct Globals(pub [u8; 25]);
impl Globals {
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<i16>) -> Result<Self> {
        let mut fields = [0; 25];
        let mut scratch = None;
        for (output, name) in fields.iter_mut().zip(GLOBAL_NAMES) {
            //2091a4 leaves the reused destination unchanged on lookup miss.
            // Retain its prior value, but never fabricate the initially
            // uninitialized stack word if the first lookup itself fails.
            if let Some(value) = lookup(name) {
                scratch = Some(value);
            }
            *output = scratch.with_context(|| {
                format!("First checkpoint global {name} leaves native scratch uninitialized")
            })? as u8;
        }
        Ok(Self(fields))
    }
    pub fn from_header(header: &[u8]) -> Result<Self> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint-global header"
        );
        Ok(Self(header[0x298..0x2b1].try_into()?))
    }
    pub fn write_header(&self, header: &mut [u8]) -> Result<()> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint-global header"
        );
        header[0x298..0x2b1].copy_from_slice(&self.0);
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"values":GLOBAL_NAMES.into_iter().zip(self.0).collect::<std::collections::BTreeMap<_,_>>(),
            "checkpoint_integrated":false})
    }
}

///353f0: ETicketMax as signed integer, then SketchMax slots2,0,1 as raw floats.
#[derive(Clone, Debug, PartialEq)]
pub struct InventoryLimits {
    pub maximum_tickets: i32,
    pub sketch_maxima_bits: [u32; 3],
}
impl InventoryLimits {
    pub fn capture(maximum_tickets: f32, sketch_maxima: &[f32]) -> Result<Self> {
        ensure!(sketch_maxima.len()>=3,"Checkpoint SketchMax slots0..2 are not owned; native getter leaves destination unchanged");
        Ok(Self {
            maximum_tickets: super::inventory_commands::checked_fctiwz(maximum_tickets)?,
            sketch_maxima_bits: [2, 0, 1].map(|index| sketch_maxima[index].to_bits()),
        })
    }
    pub fn from_header(header: &[u8]) -> Result<Self> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint-inventory header"
        );
        Ok(Self {
            maximum_tickets: i32::from_be_bytes(header[0x310..0x314].try_into()?),
            sketch_maxima_bits: [0x314, 0x318, 0x31c]
                .map(|offset| u32::from_be_bytes(header[offset..offset + 4].try_into().unwrap())),
        })
    }
    pub fn write_header(&self, header: &mut [u8]) -> Result<()> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint-inventory header"
        );
        header[0x310..0x314].copy_from_slice(&self.maximum_tickets.to_be_bytes());
        for (offset, value) in [0x314, 0x318, 0x31c]
            .into_iter()
            .zip(self.sketch_maxima_bits)
        {
            header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"maximum_tickets":self.maximum_tickets,"sketch_source_indices":[2,0,1],
            "sketch_maxima_bits":self.sketch_maxima_bits,"sketch_maxima":self.sketch_maxima_bits.map(f32::from_bits),
            "checkpoint_integrated":false})
    }
}

fn c_string(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len())]
}
/// Native28f3dc removes the final backslash prefix, then the final slash prefix.
fn basename(bytes: &[u8]) -> &[u8] {
    let bytes = c_string(bytes);
    let bytes = &bytes[bytes.iter().rposition(|&b| b == b'\\').map_or(0, |i| i + 1)..];
    &bytes[bytes.iter().rposition(|&b| b == b'/').map_or(0, |i| i + 1)..]
}
fn part_name(value: Option<&str>) -> Option<Vec<u8>> {
    let value = value
        .map(|s| c_string(s.as_bytes()))
        .filter(|s| !s.is_empty())?;
    // Descriptor lines use native ASCII lowercasing2901ec.28fde0 copies the
    // basename into260 bytes and then truncates at the FIRST '.', not the last.
    let value = basename(value);
    let value = &value[..value.len().min(259)];
    let value = &value[..value.iter().position(|&b| b == b'.').unwrap_or(value.len())];
    Some(value.to_ascii_lowercase())
}

#[derive(Clone, Debug)]
pub struct Level {
    level: Vec<u8>,
    parts: [Option<Vec<u8>>; 2],
}
impl Level {
    pub fn replace_part(&mut self,slot:usize,name:&str)->Result<()> {
        let part=self.parts.get_mut(slot).context("Invalid level part slot")?;
        *part=Some(c_string(name.as_bytes()).to_vec());Ok(())
    }
    pub fn bootstrap(level: &str, initial: Option<&str>, interior: Option<&str>) -> Self {
        //28fb50..28fb70 snprintf260("levels/%s", basename), then28fdb0.
        let mut path = b"levels/".to_vec();
        path.extend_from_slice(basename(level.as_bytes()));
        path.truncate(259);
        Self {
            level: path,
            parts: [part_name(initial), part_name(interior)],
        }
    }
    /// Write the three currently owned resource names. The checkpoint callback
    /// label is supplied separately by its actual request, never guessed here.
    pub fn write_header(&self, header: &mut [u8]) -> Result<()> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated level-resource save header"
        );
        for (offset, source) in OFFSETS[..3].iter().zip([
            self.level.as_slice(),
            self.parts[0].as_deref().unwrap_or_default(),
            self.parts[1].as_deref().unwrap_or_default(),
        ]) {
            write_name(header, *offset, source);
        }
        Ok(())
    }
    pub fn write_checkpoint_names(&self, header: &mut [u8], callback: &[u8]) -> Result<()> {
        write_names(
            header,
            [
                Some(&self.level),
                self.parts[0].as_deref(),
                self.parts[1].as_deref(),
                Some(callback),
            ],
        )
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"level":String::from_utf8_lossy(&self.level),
            "parts":self.parts.each_ref().map(|part|part.as_ref().map(|s|String::from_utf8_lossy(s))),
            "part_replacement_implemented":false,"checkpoint_integrated":false})
    }
}
/// strlen+strncpy(min(strlen+1,60)), then destination[59]=0. Because its
/// count includes only one terminator, short writes do not clear the tail.
fn write_name(header: &mut [u8], offset: usize, source: &[u8]) {
    let source = c_string(source);
    let length = source.len().min(WIDTH);
    header[offset..offset + length].copy_from_slice(&source[..length]);
    if length < WIDTH {
        header[offset + length] = 0;
    }
    header[offset + WIDTH - 1] = 0;
}
pub fn write_names(header: &mut [u8], names: [Option<&[u8]>; 4]) -> Result<()> {
    ensure!(
        header.len() >= super::save_game::HEADER_SIZE,
        "Truncated checkpoint-name header"
    );
    ensure!(
        names[0].is_some() && names[3].is_some(),
        "Missing required checkpoint name source"
    );
    for (offset, source) in OFFSETS.into_iter().zip(names) {
        write_name(header, offset, source.unwrap_or_default());
    }
    Ok(())
}

/// Coordinator2be80..2bfa0: GameState map58 and current player inventory,
/// plus ordinary CLB totals. Playtime13c and OS timestamp140 are NOT owned here.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub map_label: Vec<u8>,
    pub tickets: i32,
    pub pins: i32,
    pub film_reels: i32,
}
impl Summary {
    pub fn capture(
        map: &str,
        tickets: f32,
        collectibles: &super::collectibles::State,
    ) -> Result<Self> {
        Ok(Self {
            map_label: c_string(map.as_bytes()).to_vec(),
            tickets: super::inventory_commands::checked_fctiwz(tickets)?,
            pins: collectibles
                .totals("Pin")
                .0
                .wrapping_add(collectibles.totals("Pin_Inc").0),
            film_reels: collectibles.totals("Film_Reel").0,
        })
    }
    pub fn from_header(header: &[u8]) -> Result<Self> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint-summary header"
        );
        let map = &header[0x100..0x13c];
        let end = map
            .iter()
            .position(|&byte| byte == 0)
            .context("Unterminated checkpoint map label")?;
        let word = |offset| i32::from_be_bytes(header[offset..offset + 4].try_into().unwrap());
        Ok(Self {
            map_label: map[..end].to_vec(),
            tickets: word(0x148),
            pins: word(0x14c),
            film_reels: word(0x150),
        })
    }
    pub fn write_header(&self, header: &mut [u8]) -> Result<()> {
        ensure!(
            header.len() >= super::save_game::HEADER_SIZE,
            "Truncated checkpoint-summary header"
        );
        //2beac uses snprintf60("%s"), unlike the preceding four name fields.
        // DOL80017540..80017564 terminates at min(length,59), leaving the
        // remaining tail untouched. There is no unconditional byte59 clear.
        let source = c_string(&self.map_label);
        let length = source.len().min(59);
        header[0x100..0x100 + length].copy_from_slice(&source[..length]);
        header[0x100 + length] = 0;
        for (offset, value) in [
            (0x148, self.tickets),
            (0x14c, self.pins),
            (0x150, self.film_reels),
        ] {
            header[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        Ok(())
    }
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"map":String::from_utf8_lossy(&self.map_label),"tickets":self.tickets,
            "pins":self.pins,"film_reels":self.film_reels,"clock_fields_captured":false,"checkpoint_integrated":false})
    }
}
impl super::assets::World {
    pub fn checkpoint_inventory_limits(&self) -> Result<InventoryLimits> {
        let player = self
            .entities
            .get(self.player_entity)
            .context("Checkpoint player is not owned")?;
        let mut components = player
            .components
            .iter()
            .filter(|component| component.class_name == "InventoryComponent");
        let inventory = components
            .next()
            .context("Checkpoint player has no inventory component")?;
        ensure!(
            components.next().is_none(),
            "Checkpoint inventory selection is ambiguous"
        );
        let field = |name: &str| -> Result<&epic_mickey_lib_rs::scene_file::Property> {
            let mut fields = inventory
                .properties
                .iter()
                .filter(|property| property.name == name);
            let property = fields
                .next()
                .with_context(|| format!("Checkpoint inventory {name} missing"))?;
            ensure!(
                fields.next().is_none() && property.class_name == "Float",
                "Invalid checkpoint inventory {name}"
            );
            Ok(property)
        };
        let maximum = field("ETicketMax")?
            .value
            .as_f64()
            .context("ETicketMax is not a float")? as f32;
        let values = field("SketchMax")?
            .value
            .as_array()
            .context("SketchMax is not an array")?;
        ensure!(values.len() >= 3, "Checkpoint SketchMax slots0..2 missing");
        let mut maxima = [0.; 3];
        for (output, value) in maxima.iter_mut().zip(values) {
            *output = value
                .as_f64()
                .context("Checkpoint SketchMax is not a float")? as f32;
        }
        InventoryLimits::capture(maximum, &maxima)
    }
    pub fn checkpoint_summary(&self) -> Result<Summary> {
        let player = self
            .entities
            .get(self.player_entity)
            .context("Checkpoint player is not owned")?;
        let mut components = player
            .components
            .iter()
            .filter(|component| component.class_name == "InventoryComponent");
        let inventory = components
            .next()
            .context("Checkpoint player has no inventory component")?;
        ensure!(
            components.next().is_none(),
            "Checkpoint inventory selection is ambiguous"
        );
        //3452dc fetches ETicket only; do not require unrelated sketch arrays,
        // ETicketMax, or the retained import-time authored_inventory snapshot.
        let mut fields = inventory
            .properties
            .iter()
            .filter(|property| property.name == "ETicket");
        let field = fields
            .next()
            .context("Live ETicket missing; native destination is uninitialized")?;
        ensure!(
            fields.next().is_none() && field.class_name == "Float",
            "Invalid live ETicket property"
        );
        let tickets = field
            .value
            .as_f64()
            .context("Live ETicket is not a float")? as f32;
        Summary::capture(&self.map_info.map, tickets, &self.collectibles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn globals_and_inventory_limits_preserve_native_width_order_and_other_owners() -> Result<()> {
        let values = GLOBAL_NAMES
            .into_iter()
            .enumerate()
            .map(|(i, name)| (name, (i as i16) * 257 - 512))
            .collect::<std::collections::BTreeMap<_, _>>();
        let globals = Globals::capture(|name| values.get(name).copied())?;
        assert_eq!(globals.0, std::array::from_fn(|i| i as u8));
        let missing = Globals::capture(|name| {
            if name == GLOBAL_NAMES[7] {
                None
            } else {
                values.get(name).copied()
            }
        })?;
        assert_eq!(missing.0[7], missing.0[6]);
        assert_eq!(missing.0[8], 8);
        assert!(Globals::capture(|name| if name == GLOBAL_NAMES[0] {
            None
        } else {
            values.get(name).copied()
        })
        .is_err());
        let limits = InventoryLimits::capture(-12.9, &[1.25, -0., f32::from_bits(0x7fc01234)])?;
        assert_eq!(limits.maximum_tickets, -12);
        assert_eq!(
            limits.sketch_maxima_bits,
            [0x7fc01234, 1.25f32.to_bits(), 0x80000000]
        );
        let mut header = vec![0xa5; 0x400];
        let before = header.clone();
        globals.write_header(&mut header)?;
        limits.write_header(&mut header)?;
        assert_eq!(Globals::from_header(&header)?, globals);
        assert_eq!(InventoryLimits::from_header(&header)?, limits);
        for i in 0..header.len() {
            if !matches!(i,0x298..=0x2b0|0x310..=0x31f) {
                assert_eq!(header[i], before[i], "Unowned byte {i:x}");
            }
        }
        assert!(InventoryLimits::capture(1., &[1., 2.]).is_err());
        assert!(InventoryLimits::capture(f32::INFINITY, &[0.; 3]).is_err());
        assert!(globals.write_header(&mut header[..0x31f]).is_err());
        assert!(limits.write_header(&mut header[..0x31f]).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original files in BLOTWORX_TEST_DATA"]
    fn live_header_globals_and_inventory_limits_follow_actual_gameplay_owners() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        world.run_callback_fixture(
            "SetGlobal('FR_TotalReels',-1,true); SetGlobal('MS_ArtUnlock9',257,true)",
        )?;
        let inventory = world.entities[world.player_entity]
            .components
            .iter_mut()
            .find(|c| c.class_name == "InventoryComponent")
            .unwrap();
        inventory
            .properties
            .iter_mut()
            .find(|p| p.name == "ETicketMax")
            .unwrap()
            .value = serde_json::json!(1337.75);
        inventory
            .properties
            .iter_mut()
            .find(|p| p.name == "SketchMax")
            .unwrap()
            .value = serde_json::json!([3.25, 4.5, -0.0]);
        let mut header = vec![0xa5; 0x400];
        super::super::save_game::initialize_header(&mut header)?;
        world.write_save_header_owners(&mut header)?;
        let flags = Globals::from_header(&header)?;
        assert_eq!(flags.0[0], 255);
        assert_eq!(flags.0[24], 1);
        let limits = InventoryLimits::from_header(&header)?;
        assert_eq!(limits.maximum_tickets, 1337);
        assert_eq!(
            limits.sketch_maxima_bits,
            [0x80000000, 3.25f32.to_bits(), 4.5f32.to_bits()]
        );
        // Full native coordinator/restore remains unbound; no terminal LAST is created.
        assert!(super::super::save_game::Archive::parse(&header).is_err());
        Ok(())
    }
    #[test]
    fn summary_uses_exact_clb_types_wrapping_counts_and_native_ticket_conversion() -> Result<()> {
        let entries = [
            ("Pin", i32::MAX),
            ("Pin_Inc", 2),
            ("Pin", -4),
            ("Film_Reel", 7),
            ("Film_Reel", -2),
            ("pin", 400),
            ("Quest_Object", 9),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (kind, count))| super::super::collectibles::Entry {
            kind: kind.into(),
            name: i.to_string(),
            icon: String::new(),
            count,
        })
        .collect();
        let collectibles = super::super::collectibles::State::new(entries);
        let snapshot = Summary::capture("MAP_TEST\0ignored", -65.75, &collectibles)?;
        assert_eq!(snapshot.map_label, b"MAP_TEST");
        assert_eq!(snapshot.tickets, -65);
        assert_eq!(snapshot.pins, i32::MAX.wrapping_add(2).wrapping_sub(4));
        assert_eq!(snapshot.film_reels, 5);
        for value in [f32::NAN, f32::INFINITY, 2147483648.] {
            assert!(Summary::capture("map", value, &collectibles).is_err());
        }
        Ok(())
    }
    #[test]
    fn summary_header_preserves_clocks_reserved_bytes_and_short_map_tail() -> Result<()> {
        for map in [
            vec![],
            b"MAP_GV_ZONEC".to_vec(),
            "é".repeat(40).into_bytes(),
            vec![b'X'; 59],
            vec![b'X'; 60],
        ] {
            let mut header = vec![0xa5; 0x400];
            let before = header.clone();
            let value = Summary {
                map_label: map.clone(),
                tickets: 65,
                pins: -3,
                film_reels: i32::MIN,
            };
            value.write_header(&mut header)?;
            let roundtrip = Summary::from_header(&header)?;
            assert_eq!(roundtrip.map_label, map[..map.len().min(59)]);
            assert_eq!(roundtrip.tickets, 65);
            assert_eq!(roundtrip.pins, -3);
            assert_eq!(roundtrip.film_reels, i32::MIN);
            if map.len() < 59 {
                assert_eq!(
                    header[0x13b], 0xa5,
                    "snprintf did not clear the short map tail"
                );
            }
            for i in 0..header.len() {
                if !matches!(i,0x100..=0x13b|0x148..=0x153) {
                    assert_eq!(header[i], before[i], "Unowned byte {i:x}");
                }
            }
        }
        let mut short = vec![0xa5; 0x31f];
        let before = short.clone();
        assert!(Summary {
            map_label: vec![],
            tickets: 0,
            pins: 0,
            film_reels: 0
        }
        .write_header(&mut short)
        .is_err());
        assert_eq!(short, before);
        Ok(())
    }
    #[test]
    #[ignore = "requires original files and native save in BLOTWORX_TEST_DATA/BLOTWORX_TEST_SAVE"]
    fn native_summary_agrees_with_native_mick_and_clb_owners_without_editing_save() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let original = std::fs::read(&path)?;
        let archive = super::super::save_game::Archive::parse(&original)?;
        let metadata = Summary::from_header(&original)?;
        let inventory =
            super::super::save_player::Inventory::decode(&archive.section(*b"MICK")?.payload)?;
        let (mut collectibles, _) =
            super::super::assets::collectible_owners_from_game(std::path::Path::new(&root))?;
        collectibles.restore_save_header(&original, true)?;
        assert_eq!(metadata.tickets, inventory.tickets);
        assert_eq!(
            metadata.pins,
            collectibles
                .totals("Pin")
                .0
                .wrapping_add(collectibles.totals("Pin_Inc").0)
        );
        assert_eq!(metadata.film_reels, collectibles.totals("Film_Reel").0);
        let mut rewritten = original.clone();
        metadata.write_header(&mut rewritten)?;
        assert_eq!(rewritten, original);
        assert_eq!(std::fs::read(path)?, original);
        Ok(())
    }
    #[test]
    #[ignore = "requires original files in BLOTWORX_TEST_DATA"]
    fn live_summary_observes_gameplay_map_collectibles_and_current_inventory() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let film = world
            .collectibles
            .entries
            .iter()
            .find(|e| e.kind == "Film_Reel")
            .context("No film reel")?
            .name
            .clone();
        let pin = world
            .collectibles
            .entries
            .iter()
            .find(|e| e.kind == "Pin_Inc")
            .context("No incremental pin")?
            .name
            .clone();
        world.run_callback_fixture(&format!("SetMap(GetPlayer(),'BWX_TEST_MAP','3D'); GiveCollectible({film:?}); IncrementCollectibleCount({film:?}); GiveCollectible({pin:?})"))?;
        let inventory = world.entities[world.player_entity]
            .components
            .iter_mut()
            .find(|c| c.class_name == "InventoryComponent")
            .unwrap();
        inventory
            .properties
            .iter_mut()
            .find(|p| p.name == "ETicket")
            .unwrap()
            .value = serde_json::json!(65.75);
        let snapshot = world.checkpoint_summary()?;
        assert_eq!(snapshot.map_label, b"BWX_TEST_MAP");
        assert_eq!(snapshot.tickets, 65);
        assert_eq!(snapshot.pins, 1);
        assert_eq!(snapshot.film_reels, 2);
        let mut header = vec![0xa5; 0x400];
        super::super::save_game::initialize_header(&mut header)?;
        let clock = header[0x13c..0x148].to_vec();
        world.write_save_header_owners(&mut header)?;
        assert_eq!(Summary::from_header(&header)?, snapshot);
        assert_eq!(&header[0x13c..0x148], &clock);
        let inventory = world.entities[world.player_entity]
            .components
            .iter_mut()
            .find(|c| c.class_name == "InventoryComponent")
            .unwrap();
        inventory.properties.retain(|p| p.name != "ETicket");
        assert!(world.checkpoint_summary().is_err());
        Ok(())
    }
    #[test]
    fn names_match_original_instruction_reference_and_preserve_other_bytes() -> Result<()> {
        // tools/blotworx/checkpoint_names_reference.py executes the original
        //2bd60..2be80 against these inputs and the same full allocation pattern.
        let cases: [([Option<Vec<u8>>; 4], &str); 3] = [
            (
                [
                    Some(b"levels/GV_ZoneA_Start.level".to_vec()),
                    Some(b"stream_0".to_vec()),
                    Some(b"stream_asiaairlockinterior".to_vec()),
                    Some(b"GV_ZoneA_LoadCheckPoint".to_vec()),
                ],
                "9db650197fc8795477240f64ec06104e37e0d985db0ee3dbd3764c3717b6ef7b",
            ),
            (
                [
                    Some(b"levels/Test.level".to_vec()),
                    None,
                    None,
                    Some(vec![]),
                ],
                "9434de041c8d2d486e8d75a1e8ecbf95ca504d27d45360cf1146d2fe1aa6a086",
            ),
            (
                [
                    Some(vec![b'L'; 60]),
                    Some(vec![b'I'; 59]),
                    Some("é".repeat(40).into_bytes()),
                    Some(b"checkpoint\0ignored".to_vec()),
                ],
                "2840b0d72a40cc06dc084506ec37bf895c28ffbf6aa60116b0feb156887ac1e1",
            ),
        ];
        for (names, hash) in cases {
            let mut header = (0..0x400)
                .map(|i| ((i * 37 + 11) & 255) as u8)
                .collect::<Vec<_>>();
            let before = header.clone();
            write_names(&mut header, names.each_ref().map(|s| s.as_deref()))?;
            let actual = ring::digest::digest(&ring::digest::SHA256, &header)
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(actual, hash);
            for i in 0..header.len() {
                if !(0x10..0x100).contains(&i) {
                    assert_eq!(header[i], before[i]);
                }
            }
            assert_eq!(
                header[0x2c4], before[0x2c4],
                "Name writes cannot change the new-playthrough marker"
            );
        }
        let mut header = vec![0xa5; super::super::save_game::HEADER_SIZE];
        let before = header.clone();
        assert!(write_names(&mut header, [None; 4]).is_err());
        assert_eq!(header, before);
        assert!(write_names(&mut header[..0x31f], [Some(b"x"); 4]).is_err());
        assert_eq!(header, before);
        Ok(())
    }
    #[test]
    fn resource_names_keep_native_path_and_first_dot_rules() -> Result<()> {
        let level = Level::bootstrap(
            "D:\\game/Levels/GV_ZoneA_Start.level",
            Some("GV\\STREAM_0.part"),
            Some("GV/Airlock.extra.part"),
        );
        assert_eq!(level.level, b"levels/GV_ZoneA_Start.level");
        assert_eq!(
            level.parts,
            [Some(b"stream_0".to_vec()), Some(b"airlock".to_vec())]
        );
        let level = Level::bootstrap(&"L".repeat(400), Some(""), None);
        assert_eq!(level.level.len(), 259);
        assert_eq!(level.parts, [None, None]);
        let mut header = vec![0xa5; 0x400];
        let before = header.clone();
        level.write_header(&mut header)?;
        assert_eq!(
            &header[0xc4..],
            &before[0xc4..],
            "Resource publication invented a checkpoint label"
        );
        assert_eq!(header[0x4c], 0);
        assert_eq!(header[0x4d], 0xa5);
        Ok(())
    }
    #[test]
    #[ignore = "requires original EM1 files in BLOTWORX_TEST_DATA"]
    fn actual_level_resource_names_are_owned_before_checkpoint() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA")
            .ok_or_else(|| anyhow::anyhow!("Set BLOTWORX_TEST_DATA"))?;
        let mut world = super::super::assets::load(
            std::path::Path::new(&root),
            "Levels/GV_ZoneA_Start.level",
            &egui::Context::default(),
        )?;
        let names = world.level_resource.summary();
        assert_eq!(names["level"], "levels/GV_ZoneA_Start.level");
        assert_eq!(
            names["parts"],
            serde_json::json!(["stream_1", "stream_atob_airlock"])
        );
        // Resource identity must survive diagnostics/report mutation.
        world.report.level = "incorrect diagnostic level".into();
        world.report.scenes.clear();
        let mut header = vec![0xa5; 0x400];
        super::super::save_game::initialize_header(&mut header)?;
        header[0x2c4] = 0xa5;
        world.write_save_header_owners(&mut header)?;
        for (index, value) in [
            names["level"].as_str(),
            names["parts"][0].as_str(),
            names["parts"][1].as_str(),
        ]
        .into_iter()
        .enumerate()
        {
            let actual = c_string(&header[OFFSETS[index]..OFFSETS[index] + WIDTH]);
            assert_eq!(actual, value.unwrap_or_default().as_bytes());
        }
        assert_eq!(header[0xc4], 0xa5);
        assert_eq!(header[0x2c4], 0xa5);
        Ok(())
    }
}
