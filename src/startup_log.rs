use std::{fs, io::Write, path::PathBuf};

pub fn install() -> Option<PathBuf> {
    let path = dirs::data_local_dir()?
        .join("Blotworx Engine")
        .join("blotworx.log");
    let file = (|| {
        fs::create_dir_all(path.parent()?).ok()?;
        fs::File::create(&path).ok()
    })()?;
    let mut logger = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,blotworx=info,blotworx_phase=info"),
    );
    logger.target(env_logger::Target::Pipe(Box::new(file)));
    logger.try_init().ok()?;
    let previous = std::panic::take_hook();
    let crash_path = path.clone();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(&crash_path) {
            let _ = writeln!(
                file,
                "{info}\n{}",
                std::backtrace::Backtrace::force_capture()
            );
        }
        previous(info);
    }));
    log::info!(
        "Blotworx {} on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    Some(path)
}
