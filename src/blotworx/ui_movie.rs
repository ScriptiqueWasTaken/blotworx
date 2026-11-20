//! Embedded AVM1 movie compatibility and the original-asset gameplay HUD.
//! Image/font handling, native host services and update-phase/render parity
//! remain individually validated compatibility requirements.
use anyhow::{ensure, Context, Result};
use ruffle_core::{
    backend::{
        log::LogBackend,
        navigator::{
            self, ErrorResponse, NavigationMethod, NavigatorBackend, NullExecutor,
            NullNavigatorBackend, OwnedFuture, Request, SuccessResponse,
        },
    },
    external::{ExternalInterfaceProvider, FsCommandProvider, Value},
    tag_utils::SwfMovieData,
    FloatDuration, PlayerBuilder,
};
use std::{borrow::Cow, cell::RefCell, rc::Rc};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use swf::avm1::{
    types::{Action, DefineFunction, Push, Value as StackValue},
    write::Writer,
};

const METHODS: [&str; 24] = [
    "Init",
    "ShowHud",
    "HideHud",
    "ShowHudFor2D",
    "HideHudFor2D",
    "ShowReticles",
    "HideReticles",
    "Show",
    "Hide",
    "Close",
    "Open",
    "SkipMovieUpdate",
    "DropdownCounterCommand",
    "GenericDialogCommand",
    "UpdateVariable",
    "Update",
    "Cursor",
    "PauseDropdownCounter",
    "PauseMenuCommand",
    "OptionsMenu",
    "QuestCommand",
    "SetPopupText",
    "DisplayPopup",
    "HidePopup",
];
const GETTER: &str = "__bwx_read_property";
const SETTER: &str = "__bwx_write_property";
const CAN_SCROLL_DIALOGUE: &str = "__bwx_can_scroll_dialogue";
const SCROLL_DIALOGUE: &str = "__bwx_scroll_dialogue";
const LOAD_DIALOGUE_PORTRAIT: &str = "__bwx_load_dialogue_portrait";
const LOAD_POPUP_ICON: &str = "__bwx_load_popup_icon";
const PORTRAIT_SUFFIX: &str = ".bwx-portrait.png";
const POPUP_ICON_SUFFIX: &str = ".bwx-popup.png";

#[derive(Default)]
struct Observation {
    callbacks: BTreeSet<String>,
    traces: Vec<String>,
    warnings: Vec<String>,
    unhandled: Vec<serde_json::Value>,
    fetches: Vec<serde_json::Value>,
    aliases: Vec<serde_json::Value>,
    synchronous_fs_commands: Vec<serde_json::Value>,
    native_initialization: Arc<super::ui_initialization::State>,
    player: Option<Arc<PlayerData>>,
    native_updates: Vec<serde_json::Value>,
    native_errors: Vec<String>,
    #[cfg(feature = "blotworx-ui-render")]
    ticket_dings: Option<Vec<(i32, i32)>>,
    #[cfg(feature = "blotworx-ui-render")]
    ticket_closed: bool,
    #[cfg(feature = "blotworx-ui-render")]
    popup_done: bool,
    #[cfg(feature = "blotworx-ui-render")]
    pause_commands: Option<Vec<PauseCommand>>,
    effects: Option<Arc<Mutex<super::ui_effects::Runtime>>>,
    effect_events: Vec<serde_json::Value>,
}
/// Resolved source components for the component scenario. Runtime mutations
/// and restored saves are not represented; this is not a gameplay-state owner.
struct PlayerData {
    health: Option<std::collections::BTreeMap<String, i32>>,
    inventory: Option<std::collections::BTreeMap<String, f32>>,
    level: String,
    entity: String,
}
impl PlayerData {
    fn load(root: &Path, level: &str) -> Result<Self> {
        let world = super::assets::load(root, level, &egui::Context::default())?;
        let entity = world
            .entities
            .get(world.player_entity)
            .context("Imported world has no player")?;
        let mut health = None;
        let mut inventory = None;
        for c in &entity.components {
            if c.class_name == "HealthComponent" {
                ensure!(health.is_none(), "Duplicate player health component");
                let mut values = std::collections::BTreeMap::new();
                for name in ["Lives", "MaxHealth", "Health"] {
                    let value = c
                        .properties
                        .iter()
                        .find(|p| p.name == name)
                        .with_context(|| format!("Player {name} missing"))?
                        .value
                        .as_i64()
                        .with_context(|| format!("Player {name} is not integer"))?;
                    values.insert(name.into(), i32::try_from(value)?);
                }
                health = Some(values);
            }
            if c.class_name == "InventoryComponent" {
                ensure!(inventory.is_none(), "Duplicate player inventory component");
                let mut values = std::collections::BTreeMap::new();
                for name in ["PaintMax", "Paint", "ThinnerMax", "Thinner", "ETicket"] {
                    let value = c
                        .properties
                        .iter()
                        .find(|p| p.name == name)
                        .with_context(|| format!("Player {name} missing"))?
                        .value
                        .as_f64()
                        .with_context(|| format!("Player {name} is not float"))?
                        as f32;
                    ensure!(value.is_finite(), "Nonfinite player {name}");
                    values.insert(name.into(), value);
                }
                inventory = Some(values);
            }
        }
        Ok(Self {
            health,
            inventory,
            level: level.into(),
            entity: entity.name.clone(),
        })
    }
}
#[derive(Clone)]
struct Host(Arc<Mutex<Observation>>);
impl LogBackend for Host {
    fn avm_trace(&self, message: &str) {
        self.0.lock().unwrap().traces.push(message.into());
    }
    fn avm_warning(&self, message: &str) {
        self.0.lock().unwrap().warnings.push(message.into());
    }
}
impl ExternalInterfaceProvider for Host {
    fn on_fs_command(
        &self,
        context: &mut ruffle_core::context::UpdateContext<'_>,
        command: &str,
        args: &str,
    ) -> Option<bool> {
        #[cfg(feature = "blotworx-ui-render")]
        if command == "PopupDisplayDone" {
            self.0.lock().unwrap().popup_done = true;
            return Some(true);
        }
        #[cfg(feature = "blotworx-ui-render")]
        if let Some(request) = PauseCommand::from_movie(command, args) {
            if let Some(commands) = &mut self.0.lock().unwrap().pause_commands {
                commands.push(request);
                return Some(true);
            }
        }
        #[cfg(feature = "blotworx-ui-render")]
        if command == "DropdownCounterClosed" {
            self.0.lock().unwrap().ticket_closed = true;
            return Some(true);
        }
        #[cfg(feature = "blotworx-ui-render")]
        if command == "DropdownCounterTicketUpDing" {
            if let Some(pending) = &mut self.0.lock().unwrap().ticket_dings {
                if let Some((previous, next)) = args.split_once(':').and_then(|(previous, next)| {
                    Some((previous.parse().ok()?, next.parse().ok()?))
                }) {
                    pending.push((previous, next));
                    return Some(true);
                }
                return Some(false);
            }
        }
        if super::ui_effects::recognizes(command) {
            let effects = self.0.lock().unwrap().effects.clone();
            let parsed = super::ui_effects::parse(command, args);
            let result = match (&parsed, effects) {
                (Ok(parsed), Some(runtime)) => runtime
                    .lock()
                    .unwrap()
                    .command(parsed)
                    .map(|outcome| format!("{outcome:?}")),
                (Ok(_), None) => Err(anyhow::anyhow!(
                    "Native HUD effect resources are not admitted"
                )),
                (Err(error), _) => Err(anyhow::anyhow!("HUD effect command admission: {error:#}")),
            };
            self.0.lock().unwrap().effect_events.push(serde_json::json!({"command":command,"arguments":args,"parsed":parsed.as_ref().ok(),"result":result.as_ref().ok(),"error":result.as_ref().err().map(|e|format!("{e:#}"))}));
            if result.is_ok() {
                return Some(true);
            }
            // Preserve explicit unsupported-command diagnostics; resource
            // preparation alone must not acknowledge actual render effects.
            return None;
        }
        // Reenter in the original callback phase, without reacquiring Player
        // or holding the observation mutex across any original movie code.
        let callback = context
            .external_interface
            .get_callback(&format!("__bwx_{GETTER}"));
        let frame = callback.map(|callback| {
            format!(
                "{:?}",
                callback.call(
                    context,
                    "__bwx_fscommand_frame",
                    [Value::String("_root._currentframe".into())]
                )
            )
        });
        struct Services<'a, 'gc> {
            context: &'a mut ruffle_core::context::UpdateContext<'gc>,
            host: &'a Host,
            player: Option<Arc<PlayerData>>,
        }
        impl super::ui_calls::Movie for Services<'_, '_> {
            fn available(&mut self) -> Result<bool> {
                Ok(true)
            }
            fn invoke(&mut self, call: super::ui_calls::Invocation<'_>) -> Result<bool> {
                let name = format!("__bwx_{}", call.method);
                let callback = self
                    .context
                    .external_interface
                    .get_callback(&name)
                    .with_context(|| format!("Unavailable movie method {}", call.method))?;
                let args = call.arguments.iter().map(|v| match v {
                    super::ui_calls::Value::String(v) => Value::String((*v).into()),
                    super::ui_calls::Value::Number(v) => Value::Number(*v),
                    super::ui_calls::Value::BooleanByte(v) => Value::Bool(*v != 0),
                });
                callback.call(self.context, &name, args);
                self.host.0.lock().unwrap().native_updates.push(serde_json::json!({"method":call.method,"arguments":format!("{:?}",call.arguments),"phase":"synchronous-fscommand"}));
                Ok(true)
            }
        }
        impl super::ui_initialization::Backend for Services<'_, '_> {
            type Entity = ();
            type Owner = super::ui_initialization::Component;
            fn player(&mut self) -> Result<Option<()>> {
                Ok(self.player.as_ref().map(|_| ()))
            }
            fn component(&mut self, _: (), kind: Self::Owner) -> Result<Option<Self::Owner>> {
                let p = self.player.as_ref().context("No player")?;
                Ok(match kind {
                    Self::Owner::Health => p.health.is_some(),
                    Self::Owner::Inventory => p.inventory.is_some(),
                }
                .then_some(kind))
            }
            fn integer(&mut self, _: Self::Owner, name: &str) -> Result<i32> {
                self.player
                    .as_ref()
                    .and_then(|p| p.health.as_ref())
                    .and_then(|p| p.get(name))
                    .copied()
                    .with_context(|| format!("Unavailable native health field {name}"))
            }
            fn float(&mut self, _: Self::Owner, name: &str) -> Result<f32> {
                self.player
                    .as_ref()
                    .and_then(|p| p.inventory.as_ref())
                    .and_then(|p| p.get(name))
                    .copied()
                    .with_context(|| format!("Unavailable native inventory field {name}"))
            }
        }
        let (state, player) = {
            let o = self.0.lock().unwrap();
            (o.native_initialization.clone(), o.player.clone())
        };
        let player_present = player.is_some();
        match state.command(
            command,
            &mut Services {
                context,
                host: self,
                player,
            },
        ) {
            Ok(true) => {
                self.0.lock().unwrap().synchronous_fs_commands.push(serde_json::json!({"name":command,"arguments":args,"root_frame_during_callback":frame,"native_initialized":state.initialized(),"player_present":player_present}));
                Some(true)
            }
            Ok(false) => None,
            Err(error) => {
                self.0
                    .lock()
                    .unwrap()
                    .native_errors
                    .push(format!("{error:#}"));
                Some(false)
            }
        }
    }
    fn call_method(
        &self,
        _: &mut ruffle_core::context::UpdateContext<'_>,
        name: &str,
        args: &[Value],
    ) -> Value {
        self.0.lock().unwrap().unhandled.push(serde_json::json!({"kind":"ExternalInterface","name":name,"arguments":format!("{args:?}")}));
        Value::Undefined
    }
    fn on_callback_available(&self, name: &str) {
        self.0.lock().unwrap().callbacks.insert(name.into());
    }
    fn get_id(&self) -> Option<String> {
        Some("BlotworxHudProbe".into())
    }
}
impl FsCommandProvider for Host {
    fn on_fs_command(&self, command: &str, args: &str) -> bool {
        #[cfg(feature = "blotworx-ui-render")]
        if command == "PopupDisplayDone" {
            self.0.lock().unwrap().popup_done = true;
            return true;
        }
        self.0
            .lock()
            .unwrap()
            .unhandled
            .push(serde_json::json!({"kind":"FSCommand","name":command,"arguments":args}));
        false
    }
}
/// A read-only local provider with explicit fetch diagnostics. The base is the
/// parent movie directory, not DATA/files; authored loadClip calls use bare
/// names such as StatusBar.gfx. No failed request is recorded as loaded.
struct Files {
    inner: NullNavigatorBackend,
    root: PathBuf,
    host: Host,
    images: Rc<RefCell<super::ui_images::Images>>,
    resources: Rc<RefCell<super::resource_store::Store>>,
}
struct MovieResponse {
    url: String,
    bytes: Option<Vec<u8>>,
    length: u64,
}
impl SuccessResponse for MovieResponse {
    fn url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.url)
    }
    fn set_url(&mut self, url: String) {
        self.url = url;
    }
    fn body(mut self: Box<Self>) -> OwnedFuture<Vec<u8>, ruffle_core::loader::Error> {
        navigator::async_return(self.bytes.take().ok_or_else(|| {
            ruffle_core::loader::Error::FetchError("Movie response body already consumed".into())
        }))
    }
    fn text_encoding(&self) -> Option<&'static encoding_rs::Encoding> {
        None
    }
    fn status(&self) -> u16 {
        200
    }
    fn redirected(&self) -> bool {
        false
    }
    fn next_chunk(&mut self) -> OwnedFuture<Option<Vec<u8>>, ruffle_core::loader::Error> {
        navigator::async_return(Ok(self.bytes.take()))
    }
    fn expected_length(&self) -> Result<Option<u64>, ruffle_core::loader::Error> {
        Ok(Some(self.length))
    }
}
fn movie_resource_path(root: &Path, path: &Path) -> Result<String> {
    // Canonicalize the deepest existing ancestor so an archive member can
    // have a virtual suffix while symlinks still stay inside the game root.
    let (ancestor, resolved) = path.ancestors()
        .find_map(|ancestor| ancestor.canonicalize().ok().map(|resolved| (ancestor, resolved)))
        .context("Movie path has no resolvable ancestor")?;
    let prefix = resolved.strip_prefix(root).context("Movie reference escapes the game root")?;
    let relative = prefix.join(path.strip_prefix(ancestor)?);
    super::resource_path::canonical(&relative.to_string_lossy().replace('\\', "/"))
}

fn read_movie_resource(resources: &mut super::resource_store::Store, path: &str)
    -> Result<epic_mickey_lib_rs::packfile::VirtualFile>
{
    // Each page admits its pack when the authored container requests it.
    // This also admits the textures referenced by that page.
    let pack = match path.rsplit('/').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "pausemenu_mainpage.gfx" | "options.gfx" => Some("pausemenu_main.pak"),
        "pausemenu_map.gfx" => Some("pausemenu_map.pak"),
        "pausemenu_pinmenu.gfx" => Some("pausemenu_pins.pak"),
        "pausemenu_questmenu.gfx" => Some("pausemenu_quests.pak"),
        _ => None,
    };
    if let Some(pack) = pack {
        resources.admit_packs(&["pausemenu_base.pak".into(), pack.into()])?;
    }
    resources.read(path)
}
impl NavigatorBackend for Files {
    fn navigate_to_url(
        &self,
        url: &str,
        target: &str,
        _: Option<(
            NavigationMethod,
            ruffle_core::indexmap::IndexMap<String, String>,
        )>,
    ) {
        self.host
            .0
            .lock()
            .unwrap()
            .unhandled
            .push(serde_json::json!({"kind":"Navigation","url":url,"target":target}));
    }
    fn fetch(&self, mut request: Request) -> OwnedFuture<Box<dyn SuccessResponse>, ErrorResponse> {
        let requested = request.url().to_owned();
        let resolved = self.resolve_url(&requested).ok();
        let target = resolved.as_ref().and_then(|url| url.to_file_path().ok())
            .and_then(|path| movie_resource_path(&self.root, &path).ok().map(|relative| (path, relative)));
        let Some((local_path, relative)) = target else {
            self.host.0.lock().unwrap().fetches.push(serde_json::json!({"url":requested,"result":"rejected","reason":"Reference outside the selected game root"}));
            return navigator::async_return(Err(navigator::create_fetch_error(
                &requested, "Reference outside the selected game root",
            )));
        };
        let url = resolved.unwrap().to_string();
        let host = self.host.clone();
        let popup_icon = relative.strip_suffix(POPUP_ICON_SUFFIX);
        if let Some(texture) = popup_icon.or_else(|| relative.strip_suffix(PORTRAIT_SUFFIX)) {
            let bytes = (|| -> Result<Vec<u8>> {
                let file = self.resources.borrow_mut().read(texture)?;
                let (rgba, width, height) = crate::render_support::nif::decode_image_bytes(&file.data)
                    .with_context(|| format!("Decode authored portrait {}", file.path))?;
                let buffer = image::RgbaImage::from_raw(width, height, rgba)
                    .context("Invalid portrait RGBA length")?;
                // The authored popup scales its 4x4 placeholder to 74x74.
                // Load the replacement at that size using the same slot.
                let buffer = if popup_icon.is_some() {
                    image::imageops::resize(&buffer, 74, 74, image::imageops::FilterType::Triangle)
                } else { buffer };
                let mut png = std::io::Cursor::new(Vec::new());
                buffer.write_to(&mut png, image::ImageFormat::Png)?;
                Ok(png.into_inner())
            })();
            return match bytes {
                Ok(bytes) => {
                    let length = bytes.len() as u64;
                    host.0.lock().unwrap().fetches.push(serde_json::json!({"url":requested,"result":"opened","bytes":length}));
                    navigator::async_return(Ok(Box::new(MovieResponse {
                        url, bytes: Some(bytes), length,
                    }) as Box<dyn SuccessResponse>))
                }
                Err(error) => {
                    host.0.lock().unwrap().fetches.push(serde_json::json!({"url":requested,"result":"failed","error":format!("{error:#}")}));
                    navigator::async_return(Err(navigator::create_fetch_error(&url, &format!("{error:#}"))))
                }
            };
        }
        if local_path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gfx")) {
            let images = self.images.clone();
            let resources = self.resources.clone();
            return Box::pin(async move {
                let adapted = (|| -> Result<Vec<u8>> {
                    let mut resources = resources.borrow_mut();
                    let file = read_movie_resource(&mut resources, &relative)?;
                    let mut movie = crate::gfx::GfxFile::load(&file.data)?;
                    images.borrow_mut().adapt(&mut movie, &file.path, &mut resources)?;
                    movie.save()
                })();
                let bytes = match adapted {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        host.0.lock().unwrap().fetches.push(serde_json::json!({"url":requested,"resolved":url,"result":"failed","error":format!("{error:#}")}));
                        return Err(navigator::create_fetch_error(&url, &format!("{error:#}")));
                    }
                };
                let length = bytes.len() as u64;
                host.0.lock().unwrap().fetches.push(serde_json::json!({"url":requested,"resolved":url,"result":"opened","bytes":length}));
                Ok(Box::new(MovieResponse { url, bytes: Some(bytes), length }) as Box<dyn SuccessResponse>)
            });
        }
        request.set_url(url);
        let future = self.inner.fetch(request);
        Box::pin(async move {
            let result = future.await;
            let event = match &result {
                Ok(response) => {
                    serde_json::json!({"url":requested,"resolved":response.url(),"result":"opened","bytes":response.expected_length().ok().flatten()})
                }
                Err(error) => {
                    serde_json::json!({"url":requested,"result":"failed","error":error.error.to_string()})
                }
            };
            host.0.lock().unwrap().fetches.push(event);
            result
        })
    }
    fn resolve_url(&self, url: &str) -> Result<url::Url, url::ParseError> {
        let original = self.inner.resolve_url(url)?;
        if let Ok(path) = original.to_file_path() {
            // Cooked ImportAssets retains authoring .swf names. The owned
            // corpus supplies the same-named .gfx movie instead. Do not rewrite
            // an existing file or invent an empty library when neither exists.
            if !path.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("swf"))
            {
                let cooked = path.with_extension("gfx");
                if movie_resource_path(&self.root, &cooked).is_ok() {
                    if let Ok(mut resolved) = url::Url::from_file_path(cooked) {
                        resolved.set_query(original.query());
                        resolved.set_fragment(original.fragment());
                        self.host.0.lock().unwrap().aliases.push(
                            serde_json::json!({"requested":url,"resolved":resolved.as_str()}),
                        );
                        return Ok(resolved);
                    }
                }
            }
        }
        Ok(original)
    }
    fn spawn_future(&mut self, future: OwnedFuture<(), ruffle_core::loader::Error>) {
        self.inner.spawn_future(future);
    }
    fn pre_process_url(&self, url: url::Url) -> url::Url {
        self.inner.pre_process_url(url)
    }
    fn connect_socket(
        &mut self,
        host: String,
        port: u16,
        timeout: Duration,
        handle: ruffle_core::socket::SocketHandle,
        receiver: async_channel::Receiver<Vec<u8>>,
        sender: async_channel::Sender<ruffle_core::socket::SocketAction>,
    ) {
        self.host
            .0
            .lock()
            .unwrap()
            .unhandled
            .push(serde_json::json!({"kind":"Socket","host":host,"port":port}));
        self.inner
            .connect_socket(host, port, timeout, handle, receiver, sender);
    }
}
fn push(writer: &mut Writer<&mut Vec<u8>>, value: StackValue<'_>) -> Result<()> {
    writer.write_action(&Action::Push(Push {
        values: vec![value],
    }))?;
    Ok(())
}
fn string(writer: &mut Writer<&mut Vec<u8>>, value: &str) -> Result<()> {
    push(writer, StackValue::Str(swf::SwfStr::from_utf8_str(value)))
}
fn variable(writer: &mut Writer<&mut Vec<u8>>, name: &str) -> Result<()> {
    string(writer, name)?;
    writer.write_action(&Action::GetVariable)?;
    Ok(())
}
/// Scaleform's host Invoke API is exposed through Ruffle's public external
/// callback interface. The in-memory adapter registers existing functions;
/// their original bodies and display timeline are not rewritten.
fn bridge(version: u8) -> Result<Vec<u8>> {
    bridge_with_methods(version, &[])
}

fn bridge_with_methods(version: u8, methods: &[&str]) -> Result<Vec<u8>> {
    let mut getter = Vec::new();
    {
        let mut w = Writer::new(&mut getter, version);
        variable(&mut w, "path")?;
        w.write_action(&Action::GetVariable)?;
        w.write_action(&Action::Return)?;
    }
    let mut bytes = Vec::new();
    let mut w = Writer::new(&mut bytes, version);
    w.write_action(&Action::DefineFunction(DefineFunction {
        name: swf::SwfStr::from_utf8_str(GETTER),
        params: vec![swf::SwfStr::from_utf8_str("path")],
        actions: &getter,
    }))?;
    let mut setter = Vec::new();
    {
        let mut w = Writer::new(&mut setter, version);
        variable(&mut w, "path")?;
        variable(&mut w, "value")?;
        w.write_action(&Action::SetVariable)?;
    }
    w.write_action(&Action::DefineFunction(DefineFunction {
        name: swf::SwfStr::from_utf8_str(SETTER),
        params: vec![swf::SwfStr::from_utf8_str("path"), swf::SwfStr::from_utf8_str("value")],
        actions: &setter,
    }))?;
    for (callback, method) in [(CAN_SCROLL_DIALOGUE, "CanScrollText"), (SCROLL_DIALOGUE, "ScrollText")] {
        let mut actions = Vec::new();
        let mut child = Writer::new(&mut actions, version);
        push(&mut child, StackValue::Int(0))?;
        variable(&mut child, "_root.GENERIC_DIALOG")?;
        string(&mut child, method)?;
        child.write_action(&Action::CallMethod)?;
        child.write_action(&Action::Return)?;
        w.write_action(&Action::DefineFunction(DefineFunction {
            name: swf::SwfStr::from_utf8_str(callback), params: Vec::new(), actions: &actions,
        }))?;
    }
    for (name, slot) in [
        (LOAD_DIALOGUE_PORTRAIT, "_root.GENERIC_DIALOG.Portrait"),
        (LOAD_POPUP_ICON, "_root.POPUP.Popup.icon"),
    ] {
        let mut bitmap = Vec::new();
        let mut child = Writer::new(&mut bitmap, version);
        for axis in ["_xscale", "_yscale"] {
            variable(&mut child, slot)?;
            string(&mut child, axis)?;
            push(&mut child, StackValue::Int(100))?;
            child.write_action(&Action::SetMember)?;
        }
        variable(&mut child, "url")?;
        push(&mut child, StackValue::Int(1))?;
        variable(&mut child, slot)?;
        string(&mut child, "loadMovie")?;
        child.write_action(&Action::CallMethod)?;
        child.write_action(&Action::Return)?;
        w.write_action(&Action::DefineFunction(DefineFunction {
            name: swf::SwfStr::from_utf8_str(name),
            params: vec![swf::SwfStr::from_utf8_str("url")], actions: &bitmap,
        }))?;
    }
    for method in METHODS.into_iter().chain(methods.iter().copied()).chain([GETTER, SETTER, CAN_SCROLL_DIALOGUE, SCROLL_DIALOGUE, LOAD_DIALOGUE_PORTRAIT, LOAD_POPUP_ICON]) {
        // Arguments are popped by AVM1 in source order: name, this, method.
        variable(&mut w, "this")?;
        string(&mut w, method)?;
        w.write_action(&Action::GetMember)?;
        variable(&mut w, "this")?;
        string(&mut w, &format!("__bwx_{method}"))?;
        push(&mut w, StackValue::Int(3))?;
        variable(&mut w, "flash")?;
        string(&mut w, "external")?;
        w.write_action(&Action::GetMember)?;
        string(&mut w, "ExternalInterface")?;
        w.write_action(&Action::GetMember)?;
        string(&mut w, "addCallback")?;
        w.write_action(&Action::CallMethod)?;
        w.write_action(&Action::Pop)?;
    }
    w.write_action(&Action::End)?;
    Ok(bytes)
}
fn read(player: &mut ruffle_core::Player, path: &str) -> Value {
    player.call_internal_interface(&format!("__bwx_{GETTER}"), [Value::String(path.into())])
}
fn write(player: &mut ruffle_core::Player, path: &str, value: Value) {
    player.call_internal_interface(&format!("__bwx_{SETTER}"), [Value::String(path.into()), value]);
}
fn snapshot(player: &mut ruffle_core::Player) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    for name in [
        "_global.HIDDEN",
        "_global.SHOW_AMMO",
        "_global.CursorVisible",
        "_root.STATUS._visible",
        "_global.HEALTH",
        "_global.MAX_HEALTH",
        "_root.STATUS.prevTotalPips",
        "_root.SKETCH_SELECT._visible",
        "_root.AMMO_BARS._visible",
        "_root.RETICLES._visible",
        "_root.STATUS._url",
        "_root.SKETCH_SELECT._url",
        "_root.AMMO_BARS._url",
        "_root.RETICLES._url",
        "_root.RETICLES.Cursor._x",
        "_root.RETICLES.Cursor._y",
        "_root.RETICLES.Cursor._visible",
        "_root.RETICLES.Reticle_Menu._visible",
        "_root.TICKETS._visible",
        "_root.SKIP_MOVIE._visible",
        "_root.BLACK._visible",
        "_root.TICKETS.g_TMDropdownMC.NumberMC.Numbers.text",
        "_root.GENERIC_DIALOG.dialogBox.scroll",
        "_root.GENERIC_DIALOG._visible",
        "_root.GENERIC_DIALOG.dialogBox.text",
        "_root.GENERIC_DIALOG.dialogBox.maxscroll",
        "_root.GENERIC_DIALOG.Portrait._url",
        "_root.GENERIC_DIALOG.Portrait._width",
        "_root.GENERIC_DIALOG.Portrait._visible",
        "_root.PAUSE_MENU_MAIN._url",
        "_root.PAUSE_MENU_MAIN._framesloaded",
        "_root.PAUSE_MENU_MAIN._visible",
        "_root.POPUP.Popup._totalframes",
        "_root.POPUP.Popup.textClips.title.text",
        "_root.POPUP.Popup.textClips.description.text",
        "_root.POPUP.Popup.icon._url",
        "_root.POPUP._visible",
        "_global.INIT_MOVIE_MAX",
        "_root.STATUS._framesloaded",
        "_root.STATUS._totalframes",
        "_global.X_RES",
        "_global.Y_RES",
        "_global.SAFE_RANGE_X",
        "_global.SAFE_RANGE_Y",
    ] {
        result.insert(
            name.into(),
            serde_json::json!(format!("{:?}", read(player, name))),
        );
    }
    serde_json::Value::Object(result)
}
struct NativeView<'a> {
    player: &'a mut ruffle_core::Player,
    host: &'a Host,
}
impl super::ui_calls::Movie for NativeView<'_> {
    // This adapter borrows an existing, successfully created movie view. A
    // null manager/view is represented by the caller not creating an adapter.
    fn available(&mut self) -> Result<bool> {
        Ok(true)
    }
    fn invoke(&mut self, call: super::ui_calls::Invocation<'_>) -> Result<bool> {
        let callback = format!("__bwx_{}", call.method);
        if !self.host.0.lock().unwrap().callbacks.contains(&callback) {
            self.host
                .0
                .lock()
                .unwrap()
                .unhandled
                .push(serde_json::json!({"kind":"UnavailableMovieMethod","name":call.method}));
            // Retain the native wrapper's ignored false result, but diagnose
            // the missing method instead of reporting successful execution.
            return Ok(false);
        }
        let arguments = call.arguments.iter().map(|v| match v {
            super::ui_calls::Value::String(v) => Value::String((*v).into()),
            super::ui_calls::Value::Number(v) => Value::Number(*v),
            super::ui_calls::Value::BooleanByte(v) => Value::Bool(*v != 0),
        });
        self.player.call_internal_interface(&callback, arguments);
        Ok(true)
    }
}
struct Presentation<'a>(NativeView<'a>);
impl super::input_presentation::Backend for Presentation<'_> {
    fn message(&mut self, _: &mut super::input_presentation::State, name: &str) -> Result<()> {
        super::ui_calls::invoke(name, &[], &mut self.0)
    }
    fn menu(
        &mut self,
        _: &mut super::input_presentation::State,
        command: &str,
        menu: &str,
        reason: &str,
    ) -> Result<()> {
        super::ui_calls::call(
            command,
            menu,
            super::ui_calls::Value::String(reason),
            &mut self.0,
        )
    }
    fn flush(&mut self, _: &mut super::input_presentation::State) -> Result<()> {
        anyhow::bail!("Native2104a4 flush service is not connected to movie ownership")
    }
    fn cursor(&mut self, _: &mut super::input_presentation::State, _: bool) -> Result<()> {
        anyhow::bail!("Native218c10 cursor service is not connected to input ownership")
    }
    fn global_event(&mut self, _: &mut super::input_presentation::State, _: u16) -> Result<()> {
        anyhow::bail!("Native cutscene global-event receivers are not connected")
    }
}
fn lock(player: &Arc<Mutex<ruffle_core::Player>>) -> Result<MutexGuard<'_, ruffle_core::Player>> {
    player
        .lock()
        .map_err(|_| anyhow::anyhow!("Movie player lock poisoned"))
}

/// Diagnostic entry point; does not install an incomplete runtime into World.
pub fn probe(root: &Path, path: &str, report: &Path, aspect: f32) -> Result<()> {
    probe_with_capture(root, path, report, aspect, None)
}
pub fn probe_with_capture(
    root: &Path,
    path: &str,
    report: &Path,
    aspect: f32,
    capture: Option<&Path>,
) -> Result<()> {
    probe_with_player_level(root, path, report, aspect, capture, None)
}
pub fn probe_with_player_level(
    root: &Path,
    path: &str,
    report: &Path,
    aspect: f32,
    capture: Option<&Path>,
    level: Option<&str>,
) -> Result<()> {
    let root = root.canonicalize()?;
    let output = if report.exists() {
        report.canonicalize()?
    } else {
        let parent = report
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        parent
            .canonicalize()?
            .join(report.file_name().context("Report has no filename")?)
    };
    ensure!(
        !output.starts_with(&root),
        "Movie reports must be outside the immutable game root"
    );
    if let Some(capture) = capture {
        let parent = capture
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let resolved = if capture.exists() {
            capture.canonicalize()?
        } else {
            parent
                .canonicalize()?
                .join(capture.file_name().context("Capture has no filename")?)
        };
        ensure!(
            !resolved.starts_with(&root),
            "Movie captures must be outside the immutable game root"
        );
        ensure!(
            resolved != output,
            "Movie capture and report must be separate files"
        );
        #[cfg(not(feature = "blotworx-ui-render"))]
        anyhow::bail!("HUD captures require the blotworx-ui-render feature");
    }
    let mut resources = super::resource_store::Store::new(&root, &["HUDTextures.pak".into()])?;
    let file = read_movie_resource(&mut resources, path)?;
    let original_hash = ring::digest::digest(&ring::digest::SHA256, &file.data)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut movie = crate::gfx::GfxFile::load(&file.data)?;
    let mut images = super::ui_images::Images::default();
    images.inspect_unverified_blends = capture.is_none();
    images.adapt(&mut movie, &file.path, &mut resources)?;
    let images = Rc::new(RefCell::new(images));
    let resources = Rc::new(RefCell::new(resources));
    ensure!(
        !file
            .data
            .windows(GETTER.len())
            .any(|w| w == GETTER.as_bytes()),
        "Movie conflicts with the private host adapter name"
    );
    let extension_tags = movie
        .tags
        .iter()
        .filter(|t| t.code >= 1000)
        .map(|t| t.code)
        .collect::<BTreeSet<_>>();
    let first = movie
        .tags
        .iter()
        .position(|t| t.code == 1)
        .context("Movie has no initial display frame")?;
    movie.tags.insert(
        first,
        crate::gfx::GfxTag {
            code: 12,
            body: bridge(movie.version)?,
            long_length: true,
        },
    );
    let bytes = movie.save()?;
    let url = url::Url::from_file_path(root.canonicalize()?.join(&file.path))
        .map_err(|_| anyhow::anyhow!("Invalid movie file URL"))?;
    let movie_data = SwfMovieData::from_data(&bytes, url.to_string(), None, None)?;
    let host = Host(Arc::new(Mutex::new(Observation::default())));
    host.0.lock().unwrap().effects = Some(Arc::new(Mutex::new(super::ui_effects::Runtime::load(
        &root,
    )?)));
    if let Some(level) = level {
        host.0.lock().unwrap().player = Some(Arc::new(PlayerData::load(&root, level)?));
    }
    let mut executor = NullExecutor::new();
    let parent = root
        .join(&file.path)
        .parent()
        .context("Movie has no parent directory")?
        .to_owned();
    let navigator = Files {
        inner: NullNavigatorBackend::with_base_path(&parent, &executor)?,
        root: root.clone(),
        host: host.clone(),
        images: images.clone(),
        resources: resources.clone(),
    };
    let builder = PlayerBuilder::new()
        .with_movie(movie_data)
        .with_autoplay(true)
        .with_log(host.clone())
        .with_external_interface(Box::new(host.clone()))
        .with_fs_commands(Box::new(host.clone()))
        .with_navigator(navigator);
    #[cfg(feature = "blotworx-ui-render")]
    let builder = if capture.is_some() {
        let renderer = ruffle_render_wgpu::backend::WgpuRenderBackend::for_offscreen(
            (853, 480),
            wgpu::Backends::PRIMARY,
            wgpu::PowerPreference::HighPerformance,
        )
        .map_err(|e| anyhow::anyhow!("Create HUD renderer: {e}"))?;
        builder.with_renderer(renderer)
    } else {
        builder
    };
    let handle = builder.build();
    let mut ticks = 0;
    // A bounded observation scenario, not an update/event discard policy.
    for _ in 0..120 {
        lock(&handle)?.tick(FloatDuration::from_secs(1. / 30.));
        // Loader futures acquire the player themselves. Never poll them while
        // holding its mutex, including during initial child movie admission.
        executor.run();
        ticks += 1;
        if host
            .0
            .lock()
            .unwrap()
            .callbacks
            .contains(&format!("__bwx_{GETTER}"))
        {
            break;
        }
    }
    let getter_available = host
        .0
        .lock()
        .unwrap()
        .callbacks
        .contains(&format!("__bwx_{GETTER}"));
    let mut observations = Vec::new();
    let mut hud_values_verified = false;
    let mut initialization_values_verified = false;
    if getter_available {
        let mut player = lock(&handle)?;
        for name in [
            "_root._visible",
            "_root._currentframe",
            "_root._totalframes",
        ] {
            observations.push(
                serde_json::json!({"property":name,"value":format!("{:?}",read(&mut player,name))}),
            );
        }
        // REL20cb2c..20cf94 invokes Init with640x480 and Boolean(false)
        // for the non-wide (>1.6) aspect branch. This probe exercises that
        // explicit scenario; viewport/renderer equivalence is not established.
        let init = "__bwx_Init";
        ensure!(
            host.0.lock().unwrap().callbacks.contains(init),
            "Original root has no Init function"
        );
        let args = super::ui_calls::initialization_arguments(aspect)?;
        super::ui_calls::invoke(
            "Init",
            &args,
            &mut NativeView {
                player: &mut player,
                host: &host,
            },
        )?;
        drop(player);
        for _ in 0..30 {
            lock(&handle)?.tick(FloatDuration::from_secs(1. / 30.));
            executor.run();
            ticks += 1;
        }
        let mut player = lock(&handle)?;
        let super::ui_calls::Value::Number(width) = args[0] else {
            unreachable!()
        };
        initialization_values_verified = read(&mut player, "_global.X_RES") == Value::Number(width)
            && read(&mut player, "_global.Y_RES") == Value::Number(480.);
        observations.push(serde_json::json!({"invoked":"Init","arguments":[serde_json::json!(width),serde_json::json!(480),serde_json::json!(false)],"after":snapshot(&mut player)}));
        let mut hud_checks = Vec::new();
        let mut input = super::input_presentation::State::default();
        for method in METHODS {
            let callback = format!("__bwx_{method}");
            if !host.0.lock().unwrap().callbacks.contains(&callback) {
                continue;
            }
            if [
                "Init",
                "Close",
                "Open",
                "UpdateVariable",
                "Update",
                "Show",
                "Hide",
            ]
            .contains(&method)
            {
                continue;
            }
            if ["ShowHud", "HideHud", "ShowHudFor2D", "HideHudFor2D"].contains(&method) {
                input.mode_2d = u8::from(method.ends_with("For2D"));
                input.set_hud(
                    method.starts_with("Show"),
                    false,
                    &mut Presentation(NativeView {
                        player: &mut player,
                        host: &host,
                    }),
                )?;
            } else {
                super::ui_calls::invoke(
                    method,
                    &[],
                    &mut NativeView {
                        player: &mut player,
                        host: &host,
                    },
                )?;
            }
            let after = snapshot(&mut player);
            if ["ShowHud", "HideHud"].contains(&method) {
                let hidden = method == "HideHud";
                let mut matches = read(&mut player, "_global.HIDDEN") == Value::Bool(hidden);
                for name in [
                    "_root.STATUS._visible",
                    "_root.SKETCH_SELECT._visible",
                    "_root.AMMO_BARS._visible",
                ] {
                    matches &= read(&mut player, name) == Value::Bool(!hidden);
                }
                hud_checks.push(matches);
            }
            observations.push(
                serde_json::json!({"invoked":method,"native_hud_byte":input.hud,"after":after}),
            );
        }
        hud_values_verified = hud_checks.len() == 2 && hud_checks.into_iter().all(|v| v);
    }
    #[cfg(feature = "blotworx-ui-render")]
    if let Some(capture) = capture {
        let mut player = lock(&handle)?;
        super::ui_calls::invoke(
            "ShowHud",
            &[],
            &mut NativeView {
                player: &mut player,
                host: &host,
            },
        )?;
        player.set_viewport_dimensions(ruffle_render::backend::ViewportDimensions {
            width: 853,
            height: 480,
            scale_factor: 1.,
        });
        player.render();
        let backend = (player.renderer() as &dyn std::any::Any)
            .downcast_ref::<ruffle_render_wgpu::backend::WgpuRenderBackend<
                ruffle_render_wgpu::target::TextureTarget,
            >>()
            .context("HUD capture renderer unavailable")?;
        backend
            .capture_frame()
            .context("HUD renderer produced no frame")?
            .save(capture)?;
    }
    let seen = host.0.lock().unwrap();
    let fetches_verified =
        !seen.fetches.is_empty() && seen.fetches.iter().all(|v| v["result"] == "opened");
    let initialization_signal = seen.synchronous_fs_commands.iter().any(|v| {
        v["name"] == "HUD_INITIALIZED" && v["root_frame_during_callback"] == "Number(1.0)"
    });
    let output = serde_json::json!({
        "scope":"Embedded AVM1 execution and optional partial component rendering. Game host bindings and original UI parity remain unvalidated.",
        "movie":file.path,"original_sha256":original_hash,"adapter_in_memory_only":true,
        "ruffle_revision":"45366370ed7d1a59c89aa3cf7b765c7a274407cd","ticks":ticks,
        "getter_available":getter_available,"hud_values_verified":hud_values_verified,"initialization_values_verified":initialization_values_verified,
        "initialization_aspect_bits":aspect.to_bits(),"child_movie_fetches_verified":fetches_verified,"initialization_signal_observed":initialization_signal,
        "callbacks":seen.callbacks,"observations":observations,
        "traces":seen.traces,"warnings":seen.warnings,"unhandled_host_calls":seen.unhandled,"child_movie_fetches":seen.fetches,"cooked_import_aliases":seen.aliases,
        "synchronous_fs_commands":seen.synchronous_fs_commands,"native_initialization_player_connected":seen.player.is_some(),
        "native_player_update_calls":seen.native_updates,"native_host_errors":seen.native_errors,
        "hud_effect_commands":seen.effect_events,"hud_effect_resources":seen.effects.as_ref().map(|r|r.lock().unwrap().resources.summary()),
        "observed_host_bindings_complete":seen.unhandled.is_empty() && seen.native_errors.is_empty(),
        "player_source":seen.player.as_ref().map(|p|serde_json::json!({"scope":"Resolved source component snapshot from loaded world; runtime changes and restored saves are not represented","level":p.level,"entity":p.entity,"health":p.health,"inventory":p.inventory})),
        "scaleform_extension_tags":extension_tags,"scaleform_images_and_fonts_validated":false,
        "external_images":images.borrow().observations,"rendered_component_capture":capture,
        "gameplay_connected":false,"rendering_validated":false,
        "resource_reads":resources.borrow().reads,"loose_reads":resources.borrow().loose_reads,
    });
    std::fs::write(report, serde_json::to_vec_pretty(&output)?)?;
    ensure!(
        getter_available,
        "Movie did not execute the host adapter; report saved to {}",
        report.display()
    );
    ensure!(
        seen.native_errors.is_empty(),
        "Native HUD host services failed; report saved to {}",
        report.display()
    );
    ensure!(
        fetches_verified,
        "Required child movie fetches failed; report saved to {}",
        report.display()
    );
    ensure!(
        initialization_values_verified && initialization_signal,
        "Original movie initialization did not settle correctly; report saved to {}",
        report.display()
    );
    ensure!(hud_values_verified,"Original HUD function effects did not match their authored property writes; report saved to {}",report.display());
    ensure!(
        seen.unhandled.is_empty(),
        "Observed native HUD commands are unsupported; partial capture and report saved to {}",
        report.display()
    );
    Ok(())
}

include!("ui_gameplay.rs");
#[cfg(feature = "blotworx-ui-render")]
mod title { include!("ui_title.rs"); }
#[cfg(feature = "blotworx-ui-render")]
pub(super) use title::{Menu, Title};
