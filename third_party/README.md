# Dependencies built from source

- Box3D and MojoShader retain their local revision and patch notes in their respective directories.
- HavokLib is based on `ef5d5c6f5ab8d7f32cb4e8983300b8db61e4dc6b` with the local spline sampling change, under its bundled GPL-3.0 license. The crate compiles those sources directly. It does not link the proprietary Havok SDK.
- Spike is based on `6064cee803c17b3f17da2bb1f42211ab3c1c20db` and contains the headers and support sources used by HavokLib. Its bundled components retain their notices.
- The format parser is based on `7171e1cf646393a7a92635f0aaf17b479b15e9ac` with the local parser and bridge changes. See its own license.
- The experimental Ruffle core snapshot has its revision and patch details in `ruffle/UPSTREAM.json`.
