# Embedded runtime native-host hook

This contains only Ruffle core from revision
`45366370ed7d1a59c89aa3cf7b765c7a274407cd`. The other packages remain pinned
dependencies from the original repository. Applicable upstream licenses and
notices are retained in [ruffle-LICENSE.md](../licenses/ruffle-LICENSE.md).
No original game files are included.

The native host hook changes `external.rs`, AVM1 `fscommand.rs`, and AVM2
`flash/system.rs`. The additive `ExternalInterfaceProvider::on_fs_command`
hook receives the active `UpdateContext` and returns `Option<bool>`. `None`
uses the existing FSCommand provider. `Some` supplies its handled result.
The provider is cloned before invocation so the host can synchronously call
registered original movie functions without aliasing the interface or taking
the Player mutex again. Original AVM instructions and dispatch timing remain
unchanged.

The native Epic Mickey initialization callback invokes the movie immediately.
Deferring it until after `tick` would change ordering. This hook replaces that
architectural blocker; it does not implement missing game host services.

The display patch in `core/src/display_object/movie_clip.rs` renders authored
`DefineScalingGrid` clips with separate edge and center slices. It uses the
existing movie geometry and mask commands. `UPSTREAM.json` records the pinned
revision and the hashes of the core tree and display patch file. Cargo's shared
checkout is unchanged.

The binary embeds the runtime and compiled builtins. Java is used while
building the upstream AVM2 builtin library, not by the running application.
