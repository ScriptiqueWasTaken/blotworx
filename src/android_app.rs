use android_activity::AndroidApp;
use anyhow::{anyhow, Context, Result};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::OnceLock,
};

thread_local! { static ACTIVITY: RefCell<Option<AndroidApp>> = const { RefCell::new(None) }; }
static STORAGE: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn storage() -> Option<PathBuf> {
    STORAGE.get().cloned()
}

pub(crate) fn choose_disc() -> Result<()> {
    use jni::{objects::JObject, refs::Global, JavaVM};
    ACTIVITY.with_borrow(|app| {
        let app = app.as_ref().context("Android activity unavailable")?;
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        vm.attach_current_thread(|env| -> jni::errors::Result<()> {
            let activity = app.activity_as_ptr() as jni::sys::jobject;
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&activity)? };
            env.call_method(
                activity.as_ref(),
                jni::jni_str!("chooseDiscImage"),
                jni::jni_sig!("()V"),
                &[],
            )?;
            Ok(())
        })?;
        Ok(())
    })
}

pub(crate) fn take_disc() -> Option<Result<PathBuf>> {
    let marker = storage()?.join("disc-selection");
    let message = match std::fs::read_to_string(&marker) {
        Ok(message) => message,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => return Some(Err(error.into())),
    };
    if let Err(error) = std::fs::remove_file(marker) {
        return Some(Err(error.into()));
    }
    Some(match message.split_once('\n') {
        Some(("image", path)) => Ok(PathBuf::from(path)),
        Some(("error", error)) => Err(anyhow!(error.to_owned())),
        _ => Err(anyhow!("Unrecognized disc selection")),
    })
}

#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    let _ = env_logger::try_init();
    if let Some(path) = app.internal_data_path() {
        let _ = STORAGE.set(path);
    }
    ACTIVITY.with_borrow_mut(|activity| *activity = Some(app.clone()));
    if let Err(error) = crate::blotworx::run_platform(Some(app)) {
        log::error!("Blotworx Android startup failed: {error:#}");
    }
    ACTIVITY.with_borrow_mut(|activity| *activity = None);
}

// Desktop path dialogs are disabled in this cross-compilation probe. The public
// Android setup will use the Storage Access Framework to import a disc URI.
pub(crate) struct FileDialog;
impl FileDialog {
    pub fn new() -> Self {
        Self
    }
    pub fn add_filter(self, _: &str, _: &[&str]) -> Self {
        self
    }
    pub fn set_directory(self, _: impl AsRef<Path>) -> Self {
        self
    }
    pub fn set_file_name(self, _: &str) -> Self {
        self
    }
    pub fn pick_file(self) -> Option<PathBuf> {
        None
    }
    pub fn pick_folder(self) -> Option<PathBuf> {
        None
    }
    pub fn save_file(self) -> Option<PathBuf> {
        None
    }
}

static GAMEPAD: std::sync::Mutex<(u16, [f32; 6])> = std::sync::Mutex::new((0, [0.; 6]));

pub(crate) fn gamepad() -> (u16, [f32; 6]) {
    *GAMEPAD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[no_mangle]
pub extern "system" fn Java_dev_bloticle_blotworx_BlotworxActivity_updateGamepad(
    _env: *mut jni::sys::JNIEnv,
    _activity_class: jni::sys::jclass,
    buttons: jni::sys::jint,
    left_x: jni::sys::jfloat,
    left_y: jni::sys::jfloat,
    right_x: jni::sys::jfloat,
    right_y: jni::sys::jfloat,
    left_trigger: jni::sys::jfloat,
    right_trigger: jni::sys::jfloat,
) {
    let values = [
        left_x,
        left_y,
        right_x,
        right_y,
        left_trigger,
        right_trigger,
    ]
    .map(|value| if value.is_finite() { value } else { 0. });
    *GAMEPAD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = (buttons as u16, values);
}
