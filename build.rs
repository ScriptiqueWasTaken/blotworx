fn main() {
    println!("cargo:rerun-if-changed=third_party/mojoshader");
    println!("cargo:rerun-if-changed=src/blotworx/d3d9_translate_bridge.cpp");
    let mut shaders = cc::Build::new();
    shaders
        .cpp(true)
        .std("c++17")
        .opt_level(2)
        .warnings(false)
        .include("third_party/mojoshader")
        .define("MOJOSHADER_NO_VERSION_INCLUDE", None)
        .define("_CRT_SECURE_NO_WARNINGS", None)
        .file("src/blotworx/d3d9_translate_bridge.cpp")
        .file("third_party/mojoshader/mojoshader.c")
        .file("third_party/mojoshader/mojoshader_common.c")
        .file("third_party/mojoshader/profiles/mojoshader_profile_common.c")
        .file("third_party/mojoshader/profiles/mojoshader_profile_glsl.c");
    for profile in [
        "D3D", "BYTECODE", "HLSL", "ARB1", "ARB1_NV", "METAL", "SPIRV", "GLSPIRV",
    ] {
        shaders.define(&format!("SUPPORT_PROFILE_{profile}"), Some("0"));
    }
    shaders.compile("blotworx_mojoshader");
    println!("cargo:rerun-if-changed=third_party/box3d");
    println!("cargo:rerun-if-changed=src/blotworx/box3d_bridge.c");
    let mut build = cc::Build::new();
    build
        .include("third_party/box3d/include")
        .include("third_party/box3d/src")
        .std("c17")
        .opt_level(2)
        .define("NDEBUG", None)
        .define("BOX3D_DISABLE_AVX2", None)
        .warnings(false)
        .file("src/blotworx/box3d_bridge.c");
    let mut sources: Vec<_> = std::fs::read_dir("third_party/box3d/src")
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|s| s == "c"))
        .collect();
    sources.sort();
    for source in sources {
        build.file(source);
    }
    if build.get_compiler().is_like_msvc() {
        build.flag("/fp:precise");
    } else {
        build.flag_if_supported("-ffp-contract=off");
    }
    build.compile("blotworx_box3d");
    if std::env::var("CARGO_CFG_UNIX").is_ok() {
        println!("cargo:rustc-link-lib=m");
    }
}
