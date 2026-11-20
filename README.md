# Blotworx Engine

Blotworx is an experimental Rust runtime for loading local Epic Mickey game data. It builds independently of BlueThinner. The project has no game installation, extracted game assets, or decompiled Lua source in Git.

## Build and run

The current development target is 64-bit Windows. Install Rust and the Visual Studio C++ build tools. Cargo builds the bundled native dependencies. The optional embedded movie UI features are experimental and require a JDK.

```powershell
cargo build --locked --bin blotworx
cargo run --locked --bin blotworx -- --help
cargo run --locked --bin blotworx -- --data "C:/Games/EM1/DATA" --level "Levels/EB_VillageTest.level"
cargo run --locked --bin blotworx -- --data "C:/Games/EM2" --level "Levels/YensidsLab.lvlc" --shared-playtest
```

Use your own extracted EM1 data or the EM2 PC installation. An existing BlueThinner import can be passed directly. Omitting `--data` tries the existing BlueThinner build cache. This is a development runtime. The EM2 startup camera and several original material inputs remain incomplete.

## Layout

- `src/blotworx/` contains the runtime, simulation, rendering integration, and platform compatibility.
- `src/render_support/` contains the shared scene, texture, material, and viewport code used by the runtime.
- `crates/asset-formats/` contains the parser snapshot required by this checkout.
- `third_party/` contains native dependencies and the optional embedded UI runtime. See [dependency provenance](third_party/README.md).
- `tools/blotworx/fixtures/` contains reference data used by the Rust tests. [Coverage notes](docs/blotworx-coverage.json) track work in progress.

The extracted runtime reads the game scripts supplied by the game installation. BlueThinner's reviewed script snapshot and its embedded GV cleanup override are excluded. Those script-specific paths may therefore behave differently from an existing BlueThinner build.

The build includes HavokLib under GPL-3.0. Individual third-party libraries and the format parser retain their own licenses.
