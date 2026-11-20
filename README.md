# Blotworx Engine

Blotworx is an experimental Rust runtime for loading local Epic Mickey game data. It builds independently of BlueThinner. The project has no game installation, extracted game assets, or decompiled Lua source in Git.

## Public demo

Download the Windows demo and open **Blotworx.exe**. On first launch, supply your Epic Mickey Wii disc image. Blotworx imports it locally and opens the original title screen. Use the same save slot to resume from its last checkpoint. [Releases](https://github.com/Bloticle/blotworx/releases) · [Setup and compatibility](docs/PUBLIC_DEMO.md)

## Build and run

The current development target is 64-bit Windows. Install Rust and the Visual Studio C++ build tools. Cargo builds the bundled native dependencies. The default build includes the EM1 gameplay HUD renderer. It loads the HUD movie and textures from your game installation.

```powershell
cargo run --locked
```

Import a Wii disc image or choose an existing extracted game folder. Open **F10 → Game** to change levels and **F10 → Tools** to unlock preview abilities for the session. In the EM2 PC preview, **F6** toggles the shared controller.

The EM2 startup camera and several material inputs are still incomplete. For automated captures and validation, use `cargo run --locked -- diagnose --help`.

For interactive debugging, use the standard `dev` build shown above. It keeps debug assertions and optimizes the engine and its dependencies across all levels. Avoid reusing executables built with an `opt-level=0` override for playtesting. Press F3 in EM1 to view runtime frame timings.

## Layout

- `src/blotworx/` contains the runtime, simulation, rendering integration, and platform compatibility.
- `src/render_support/` contains the shared scene, texture, material, and viewport code used by the runtime.
- `crates/asset-formats/` contains the parser snapshot required by this checkout.
- `third_party/` contains native dependencies and the optional embedded UI runtime. See [dependency provenance](third_party/README.md).
- `tools/blotworx/fixtures/` contains reference data used by the Rust tests. [Coverage notes](docs/blotworx-coverage.json) track work in progress.

The extracted runtime reads the game scripts supplied by the game installation. BlueThinner's reviewed script snapshot and its embedded GV cleanup override are excluded. Those script-specific paths may therefore behave differently from an existing BlueThinner build.

The build includes HavokLib under GPL-3.0. Individual third-party libraries and the format parser retain their own licenses.
