//! Compiles the UI and embeds the icon. `packaging/attach-payload.ps1` adds the package (ADR-0015).

#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
#[path = "../../packaging/icon_resource.rs"]
mod icon_resource;

fn main() {
    // `slint-build` exists only on a Windows host; CARGO_CFG_WINDOWS checks the target.
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        compile_ui();
        harden_imports();
    }
}

#[cfg(windows)]
fn compile_ui() {
    icon_resource::embed(Path::new("../../packaging/mujina.ico"));

    println!("cargo:rerun-if-changed=lang");
    // The UI shared with Settings, imported as "@mujina/...".
    let shared = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui");
    let libraries = std::collections::HashMap::from([("mujina".to_string(), shared)]);
    // Texts in lang/<language>/LC_MESSAGES/mujina-setup.po; the window selects the language.
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_bundled_translations("lang")
        .with_library_paths(libraries);
    if let Err(error) = slint_build::compile_with_config("ui/setup.slint", config) {
        panic!("ui/setup.slint: {error}");
    }
}

/// Has Windows look for the imported DLLs in System32 only, never in the folder the program was
/// started from, where a planted one could wait. `main` does the same for DLLs loaded later.
#[cfg(windows)]
fn harden_imports() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // 0x800 is LOAD_LIBRARY_SEARCH_SYSTEM32.
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
}
