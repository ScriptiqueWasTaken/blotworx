# Embedded runtime native-host hook

This contains only Ruffle core from revision
`45366370ed7d1a59c89aa3cf7b765c7a274407cd`. The other packages remain pinned
dependencies from the original repository. Applicable upstream licenses and
notices are retained in [ruffle-LICENSE.md](../licenses/ruffle-LICENSE.md).
No original game files are included.

Three source files differ: `external.rs`, AVM1 `fscommand.rs`, and AVM2
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

`tools/blotworx/vendor_ui_runtime.py` reproduces the source selection, manifest
path conversion and patch from the pinned upstream checkout. Its `--verify`
mode compares all admitted source files and the core manifest with that
checkout plus the three specified changes. Cargo's shared checkout is never
modified. `UPSTREAM.json` and the validation report record the content hashes.

The binary embeds the runtime and compiled builtins. Java is used while
building the upstream AVM2 builtin library, not by the running application.
