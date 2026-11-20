use ruffle_core::events::{KeyDescriptor, KeyLocation, LogicalKey, MouseButton, NamedKey, PhysicalKey, PlayerEvent};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PausePage { Main, Map, Quest, Pin }

impl PausePage {
    fn name(self) -> &'static str {
        match self { Self::Main => "Main", Self::Map => "Map", Self::Quest => "Quest", Self::Pin => "Pin" }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PauseOptions { Closed, Loading, Open }

impl PauseOptions {
    fn active(self) -> bool { self != Self::Closed }
}

enum PauseCommand {
    Page(PausePage), QuestRow(usize), QuestInfo, Options, Resume, Quit, Sound(String), MusicVolume(i32), SfxVolume(i32), LeftHanded, ResetOptions,
}

impl PauseCommand {
    fn from_movie(command: &str, args: &str) -> Option<Self> {
        let volume = |value: &str| {
            let value = value.trim().strip_prefix('*').unwrap_or(value).trim().parse::<f64>().ok()?;
            value.is_finite().then(|| value.clamp(0., 100.) as i32)
        };
        Some(match (command, args) {
            ("PauseMenu", "Main") => Self::Page(PausePage::Main),
            ("PauseMenu", "Map") => Self::Page(PausePage::Map),
            ("PauseMenuQuest", "Menu") => Self::Page(PausePage::Quest),
            ("PauseMenuQuest", "QuestInfo") => Self::QuestInfo,
            ("PauseMenuQuest", name) => Self::QuestRow(name.strip_prefix("questitem")?.parse::<usize>().ok()?.checked_sub(1)?),
            ("PauseMenuPin", "Menu") => Self::Page(PausePage::Pin),
            ("PauseMenuPick", "OptionsButton") => Self::Options,
            ("PauseMenuPick", "Exit") => Self::Resume,
            ("PauseMenuPick", "QuitButton") => Self::Quit,
            ("OptionsMenuMusic", value) => Self::MusicVolume(volume(value)?),
            ("OptionsMenuSFX", value) => Self::SfxVolume(volume(value)?),
            ("OptionsMenuPick", "LeftHanded") => Self::LeftHanded,
            ("OptionsMenuPick", "Reset") => Self::ResetOptions,
            ("SFX", event) if event.starts_with("Play_") || event.starts_with("Set_") => Self::Sound(event.into()),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum PauseKey {
    Confirm, Back, PreviousPage, NextPage, Up, Down, Left, Right,
}

impl PauseKey {
    fn forward(self, player: &mut ruffle_core::Player) {
        use NamedKey as N;
        use PhysicalKey as P;
        let (physical_key, logical_key) = match self {
            Self::Confirm => (P::Enter, N::Enter),
            Self::Back => (P::Escape, N::Escape),
            Self::PreviousPage => (P::PageUp, N::PageUp),
            Self::NextPage => (P::PageDown, N::PageDown),
            Self::Up => (P::ArrowUp, N::ArrowUp),
            Self::Down => (P::ArrowDown, N::ArrowDown),
            Self::Left => (P::ArrowLeft, N::ArrowLeft),
            Self::Right => (P::ArrowRight, N::ArrowRight),
        };
        let key = KeyDescriptor { physical_key, logical_key: LogicalKey::Named(logical_key), key_location: KeyLocation::Standard };
        player.handle_event(PlayerEvent::KeyDown { key });
        player.handle_event(PlayerEvent::KeyUp { key });
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PauseInput {
    Key(PauseKey),
    Pointer { position: egui::Pos2, pressed: Option<bool> },
}

impl PauseInput {
    fn forward(&self, player: &mut ruffle_core::Player, rect: egui::Rect) {
        match *self {
            Self::Key(key) => key.forward(player),
            Self::Pointer { position, pressed } => {
                let x = f64::from((position.x - rect.left()) / rect.width()) * 853.;
                let y = f64::from((position.y - rect.top()) / rect.height()) * 480.;
                let button = MouseButton::Left;
                player.handle_event(match pressed {
                    Some(true) => PlayerEvent::MouseDown { x, y, button, index: None },
                    Some(false) => PlayerEvent::MouseUp { x, y, button },
                    None => PlayerEvent::MouseMove { x, y },
                });
            }
        }
    }
}

pub(crate) struct PausePointer {
    pub position: [f32; 2],
    stick_mode: bool,
    clicking: bool,
}

impl Default for PausePointer {
    fn default() -> Self { Self { position: [0.5, 0.63], stick_mode: false, clicking: false } }
}

impl PausePointer {
    pub(crate) fn input(&mut self, ui: &egui::Ui, rect: egui::Rect, pad: super::input::Pad,
        edges: u16, elapsed: f32) -> Vec<PauseInput>
    {
        let (mut events, focused) = pause_input(ui, edges & !0x1000);
        if !focused { self.clicking = false; return events; }
        if edges & 15 != 0 { self.stick_mode = false; }
        if let Some(pointer) = events.iter().rev().find_map(|event| match *event {
            PauseInput::Pointer { position, .. } if rect.contains(position) => Some(position),
            _ => None,
        }) {
            self.stick_mode = false;
            self.position = [(pointer.x - rect.left()) / rect.width().max(1.), (pointer.y - rect.top()) / rect.height().max(1.)];
        }
        if pad.right != [0.; 2] {
            let step = elapsed.clamp(0., 0.1);
            self.stick_mode = true;
            self.position[0] = (self.position[0] + pad.right[0] * step).clamp(0.03, 0.97);
            self.position[1] = (self.position[1] - pad.right[1] * step).clamp(0.07, 0.93);
        }
        let clicking = self.stick_mode && pad.buttons & 0x1000 != 0;
        if clicking != self.clicking {
            let position = egui::pos2(rect.left() + self.position[0] * rect.width(), rect.top() + self.position[1] * rect.height());
            events.push(PauseInput::Pointer { position, pressed: Some(clicking) });
            self.clicking = clicking;
        } else if !self.stick_mode && edges & 0x1000 != 0 {
            events.push(PauseInput::Key(PauseKey::Confirm));
        }
        events
    }
}

fn pause_input(ui: &egui::Ui, pad: u16) -> (Vec<PauseInput>, bool) {
    use egui::Key as K;
    use PauseKey as P;
    ui.input(|input| {
        if !input.focused { return (Vec::new(), false); }
        let mut events = input.events.iter().filter_map(|event| match *event {
            egui::Event::Key { key, pressed: true, repeat: false, .. } => {
                let key = match key {
                    K::Enter | K::Space => P::Confirm,
                    K::Escape | K::Backspace => P::Back,
                    K::PageUp => P::PreviousPage,
                    K::PageDown => P::NextPage,
                    K::ArrowUp => P::Up, K::ArrowDown => P::Down,
                    K::ArrowLeft => P::Left, K::ArrowRight => P::Right,
                    _ => return None,
                };
                Some(PauseInput::Key(key))
            }
            egui::Event::PointerMoved(position) => Some(PauseInput::Pointer { position, pressed: None }),
            egui::Event::PointerButton { pos: position, button: egui::PointerButton::Primary, pressed, .. } =>
                Some(PauseInput::Pointer { position, pressed: Some(pressed) }),
            egui::Event::PointerButton { button: egui::PointerButton::Secondary, pressed: true, .. } =>
                Some(PauseInput::Key(P::Back)),
            _ => None,
        }).collect::<Vec<_>>();
        for (bit, key) in [(0x2000,P::Back), (0x100,P::PreviousPage),
            (0x200,P::NextPage), (1,P::Up), (2,P::Down), (4,P::Left), (8,P::Right)] {
            if pad & bit != 0 { events.push(PauseInput::Key(key)); }
        }
        (events, true)
    })
}

#[derive(Default)]
struct PauseOutcome {
    resume: bool,
    quit: bool,
    sounds: Vec<String>,
    music_volume: Option<i32>,
    sfx_volume: Option<i32>,
    toggle_left_handed: bool,
    reset_options: bool,
}

#[derive(Default)]
struct PauseLists {
    pins: Vec<PauseRow>,
    quests: Vec<PauseRow>,
    pin_counts: [f64; 6],
}

struct PauseRow {
    name: String,
    description: String,
    icon: String,
    state: u16,
    character: String,
    area: String,
}

impl PauseLists {
    fn from_world(world: &super::assets::World) -> Self {
        let pins = world.collectibles.entries.iter().filter(|entry| entry.kind == "Pin")
            .map(|entry| PauseRow { name: format!("{}_Name", entry.name), description: format!("{}_Description", entry.name),
                icon: entry.icon.clone(), state: u16::from(entry.count > 0), character: String::new(), area: String::new() }).collect();
        let quests = world.quests.pause_quests().map(|quest| PauseRow {
            name: quest.name.to_owned(), description: quest.description.to_owned(),
            icon: quest.icon.to_owned(), state: u16::from(quest.status <= 4) * quest.status,
            character: quest.character.to_owned(), area: quest.area.to_owned(),
        }).collect();
        let mut pin_counts = [0.; 6];
        for (i, (name, setting)) in [("Gold_Pin", "GoldPinCount"), ("Silver_Pin", "SilverPinCount"), ("Bronze_Pin", "BronzePinCount")].into_iter().enumerate() {
            let owned = world.collectibles.entries.iter().find(|entry| entry.name == name).map_or(0, |entry| entry.count);
            pin_counts[2 * i] = f64::from(owned);
            pin_counts[2 * i + 1] = world.runtime_settings.get(setting).and_then(|text| text.parse().ok()).unwrap_or(0.);
        }
        Self { pins, quests, pin_counts }
    }

    fn publish(&self, player: &mut ruffle_core::Player, page: PausePage, images: &super::ui_images::Images, root: &Path) {
        let (prefix, rows) = match page {
            PausePage::Pin => {
                write(player, "_global.PINCOUNT", Value::List(self.pin_counts.map(Value::Number).into()));
                ("PIN", &self.pins)
            }
            PausePage::Quest => ("QUEST", &self.quests),
            _ => return,
        };
        let label = |key: &str| Value::String(images.pause_label(key).unwrap_or(key).to_owned());
        let icon = |name: &str| {
            let url = hud_icon_url(root, name, PORTRAIT_SUFFIX);
            Value::String(url.map_or_else(String::new, |url| url.to_string()))
        };
        for (field, values) in [
            ("NAMES", rows.iter().map(|row| label(&row.name)).collect()),
            ("INFO", rows.iter().map(|row| label(&row.description)).collect()),
            ("STATE", rows.iter().map(|row| Value::Number(f64::from(row.state))).collect()),
            ("TEXTURE", rows.iter().map(|row| icon(&row.icon)).collect()),
        ] {
            write(player, &format!("_global.{prefix}{field}"), Value::List(values));
        }
        write(player, &format!("_global.{prefix}TOTAL"), Value::Number(rows.len() as f64));
        if page == PausePage::Quest {
            let counts = [0, 1, 2].map(|state| {
                Value::Number(rows.iter().filter(|row| row.state == state).count() as f64)
            });
            write(player, "_global.QUESTCOUNT", Value::List(counts.into()));
        }
    }
}

fn hud_icon_url(root: &Path, name: &str, suffix: &str) -> Option<url::Url> {
    if name.is_empty() { return None; }
    let path = if name.contains(['/', '\\']) {
        name.replace('\\', "/")
    } else {
        let name = name.to_ascii_lowercase();
        let name = name.trim_start_matches("ui_icon_");
        let name = name.strip_suffix(".nif_wii").or_else(|| name.strip_suffix(".nif")).unwrap_or(name);
        let name = name.strip_suffix("_tex").unwrap_or(name);
        format!("HUDElements/Icons/ui_icon_{name}_tex.nif")
    };
    let path = super::resource_path::canonical(&path).ok()?;
    url::Url::from_file_path(root.join(format!("{path}{suffix}"))).ok()
}
fn pause_movie(player: &mut ruffle_core::Player, host: &Host, method: &str, kind: &str, page: &str) -> Result<()> {
    super::ui_calls::call(method, kind, super::ui_calls::Value::String(page), &mut NativeView { player, host })
}

fn close_pause_options(player: &mut ruffle_core::Player, host: &Host, options: &mut PauseOptions) -> Result<()> {
    if matches!(read(player, "_root.OPTIONS.m_TVMenuUp"), Value::Bool(true) | Value::Number(1.)) {
        pause_movie(player, host, "Close", "Menu", "TV")?;
    }
    pause_movie(player, host, "Close", "Menu", "Options")?;
    write(player, "_root.OPTIONS._visible", Value::Bool(false));
    write(player, "_root.PAUSE_MENU_MAIN._visible", Value::Bool(true));
    *options = PauseOptions::Closed;
    Ok(())
}

fn begin_pause(player: &mut ruffle_core::Player, host: &Host, frame: &HudFrameInput) -> Result<()> {
    // The authored pages use Key.getCode via these globals. Bind host input
    // to the same handlers that process the original Wii controls.
    for (name, code) in [("aKey", 13.), ("bKey", 27.), ("plusKey", 34.), ("minusKey", 33.),
        ("upKey", 38.), ("downKey", 40.), ("leftKey", 37.), ("rightKey", 39.)] {
        write(player, &format!("_global.{name}"), Value::Number(code));
    }
    write(player, "_global.IsGamePaused", Value::Bool(true));
    write(player, "_global.PINSANDFILMS", Value::List(frame.pause_collectibles.map(Value::Number).into()));
    if let Some(map) = &frame.pause_map {
        write(player, "_global.PAUSE_MENU_MAP", Value::String(map.map.clone()));
        write(player, "_global.MAPTYPE", Value::String(map.map_type.clone()));
    }
    pause_movie(player, host, "PauseDropdownCounter", "Pause", "true")?;
    pause_movie(player, host, "Open", "Pause", "Main")
}

fn apply_pause_commands(
    player: &mut ruffle_core::Player, host: &Host, page: &mut PausePage,
    options: &mut PauseOptions, input: &HudFrameInput, paused: bool, commands: Vec<PauseCommand>,
    images: &super::ui_images::Images, root: &Path,
) -> Result<PauseOutcome> {
    let mut result = PauseOutcome::default();
    let mut quest_row = None;
    for command in commands {
        match command {
            PauseCommand::Sound(event) => result.sounds.push(event),
            _ if !paused => {}
            PauseCommand::Page(next) if *page != next && !options.active() => {
                if let Some(lists) = &input.pause_lists { lists.publish(player, next, images, root); }
                pause_movie(player, host, "Close", "Pause", page.name())?;
                pause_movie(player, host, "Open", "Pause", next.name())?;
                *page = next;
            }
            PauseCommand::QuestRow(row) if *page == PausePage::Quest => {
                quest_row = Some(row);
            }
            PauseCommand::QuestInfo if *page == PausePage::Quest => {
                let scroll = match read(player, "_root.PAUSE_MENU_QUESTS.PauseMC.QuestMenu.sbQuests.position") {
                    Value::Number(value) if value.is_finite() && value >= 0. => value as usize,
                    _ => 0,
                };
                if let Some(row) = quest_row.and_then(|row| input.pause_lists.as_ref()?.quests.get(scroll.saturating_add(row))) {
                    let text = |key: &str| Value::String(images.pause_label(key).unwrap_or(key).to_owned());
                    let mut details = vec![text(&row.name), text(&row.description), text(&row.character), text(&row.area)];
                    details.resize(8, Value::Number(0.));
                    write(player, "_global.QUESTDETAILS", Value::List(details));
                    pause_movie(player, host, "QuestCommand", "UpdateQuestInfo", "")?;
                }
            }
            PauseCommand::Options if !options.active() => {
                write_pause_settings(player, input.pause_settings);
                pause_movie(player, host, "Open", "Menu", "Options")?;
                *options = PauseOptions::Loading;
            }
            PauseCommand::Resume if !options.active() => result.resume = true,
            PauseCommand::Quit => result.quit = true,
            PauseCommand::MusicVolume(volume) => result.music_volume = Some(volume),
            PauseCommand::SfxVolume(volume) => result.sfx_volume = Some(volume),
            PauseCommand::LeftHanded => result.toggle_left_handed = !result.toggle_left_handed,
            PauseCommand::ResetOptions if options.active() => {
                write_pause_settings(player, input.pause_defaults);
                pause_movie(player, host, "OptionsMenu", "Reset", "")?;
                result.reset_options = true;
            }
            _ => {}
        }
    }
    // Keep the main page visible until the asynchronous submenu can draw.
    if options.active() {
        let loaded = matches!(read(player, "_root.OPTIONS._url"), Value::String(url) if url.to_ascii_lowercase().ends_with("/options.gfx"));
        if loaded && *options == PauseOptions::Loading
            && matches!(read(player, "_root.OPTIONS._visible"), Value::Bool(true) | Value::Number(1.))
        {
            write(player, "_root.PAUSE_MENU_MAIN._visible", Value::Bool(false));
            *options = PauseOptions::Open;
        }
        else if !loaded && *options == PauseOptions::Open && matches!(read(player, "_global.CurrentInputHandlerOwnerID"),
            Value::String(id) if id == "PAUSE_MENU_MAIN")
        {
            write(player, "_root.PAUSE_MENU_MAIN._visible", Value::Bool(true));
            *options = PauseOptions::Closed;
        }
    }
    Ok(result)
}

fn write_pause_settings(player: &mut ruffle_core::Player, settings: [i32; 3]) {
    write(player, "_global.OPTION_SETTINGS", Value::List(settings.map(|value| Value::Number(f64::from(value))).into()));
}

fn format_pause_fields(player: &mut ruffle_core::Player, page: PausePage, options: bool,
    images: &super::ui_images::Images) -> bool
{
    let mut changed = false;
    if page == PausePage::Map {
        let path = "_root.PAUSE_MENU_MAP.PauseMC.Map.MapName.text";
        if let Value::String(value) = read(player, path) {
            if let Some(localized) = images.pause_label(&value).filter(|&text| text != value) {
                write(player, path, Value::String(localized.to_owned()));
                changed = true;
            }
        }
    }
    let mut strip_marker = |path: &str| {
        if let Value::String(text) = read(player, path) {
            if let Some(value) = text.strip_prefix('*') {
                write(player, path, Value::String(value.to_owned()));
                changed = true;
            }
        }
    };
    if page == PausePage::Main {
        for field in ["Tickets", "Pins", "Films"] {
            strip_marker(&format!("_root.PAUSE_MENU_MAIN.PauseMC.MainPage.Counters.{field}.text"));
        }
    }
    if options {
        for field in ["SFX_Volume", "Music_Volume"] {
            strip_marker(&format!("_root.OPTIONS.OptionsMC.Contents.{field}.displayValue.text"));
        }
        for (field, key, fallback) in [
            ("LeftHanded", "UI_LeftHanded", "Left-Handed"),
            ("SFX_Volume", "UI_SFX", "Sound Effects"),
            ("Music_Volume", "UI_Music", "Music"),
            ("Reset", "UI_ResetToDefault", "Reset to Default"),
            ("TV", "UI_TVSettings", "TV Settings"),
        ] {
            let path = format!("_root.OPTIONS.OptionsMC.Contents.{field}.label");
            if matches!(read(player, &path), Value::String(value) if value == key) {
                write(player, &path, Value::String(images.pause_label(key).unwrap_or(fallback).to_owned()));
                changed = true;
            }
        }
    }
    if page == PausePage::Main {
        for (button, key, fallback) in [("OptionsButton", "UI_Options", "Options"), ("QuitButton", "UI_Quit", "Quit")] {
            let path = format!("_root.PAUSE_MENU_MAIN.PauseMC.MainPage.{button}.label");
            if matches!(read(player, &path), Value::String(value) if value == key) {
                write(player, &path, Value::String(images.pause_label(key).unwrap_or(fallback).to_owned()));
                changed = true;
            }
        }
    }
    changed
}
