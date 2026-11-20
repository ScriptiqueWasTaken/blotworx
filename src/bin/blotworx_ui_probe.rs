use anyhow::{Context, Result};
fn main() -> Result<()> {
    let _ = env_logger::try_init();
    let mut args = std::env::args_os().skip(1);
    let root=args.next().context("Usage: blotworx_ui_probe DATA/files movie/path.gfx report.json [aspect [capture.png [level.level]]]")?;
    let movie = args
        .next()
        .context("Missing virtual movie path")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("Movie path must be UTF-8"))?;
    let report = args.next().context("Missing report path")?;
    let aspect = if let Some(value) = args.next() {
        value
            .into_string()
            .map_err(|_| anyhow::anyhow!("Invalid aspect encoding"))?
            .parse::<f32>()?
    } else {
        4. / 3.
    };
    let capture = args.next();
    let level = args
        .next()
        .map(|s| {
            s.into_string()
                .map_err(|_| anyhow::anyhow!("Level must be UTF-8"))
        })
        .transpose()?;
    anyhow::ensure!(args.next().is_none(), "Unexpected probe arguments");
    blotworx::blotworx::ui_movie::probe_with_player_level(
        std::path::Path::new(&root),
        &movie,
        std::path::Path::new(&report),
        aspect,
        capture.as_deref().map(std::path::Path::new),
        level.as_deref(),
    )
}
