//! Cooked references can retain source-folder `..` segments. Resolve them
//! lexically before archive lookup; never allow a loose read outside the root.
use anyhow::{ensure, Context, Result};

pub fn canonical(path: &str) -> Result<String> {
    let path = path.split('\0').next().unwrap_or("").replace('\\', "/");
    ensure!(
        !path.starts_with('/') && !path.contains(':'),
        "Resource must be a relative game path"
    );
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().context("Resource path escapes the game root")?;
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooked_parent_segments_resolve_inside_root() -> Result<()> {
        assert_eq!(canonical("Palettes/Prefabs/Pickups/Level Pickups/../../../../Environments/_Shared/Scripts/LevelPickups.lua")?,
            "Environments/_Shared/Scripts/LevelPickups.lua");
        assert_eq!(
            canonical("./Scripts\\Sub\\..\\Child.lua")?,
            "Scripts/Child.lua"
        );
        for path in [
            "../outside.lua",
            "Scripts/../../outside.lua",
            "D:/outside.lua",
            "/outside.lua",
        ] {
            assert!(canonical(path).is_err(), "{path}");
        }
        Ok(())
    }
}
