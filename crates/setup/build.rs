//! Compiles the user interface and embeds the icon.
//!
//! The package the installer carries is not built in: `packaging/attach-payload.ps1` adds it to
//! the finished executable (ADR-0015), so that the job that signs it runs no compiler. A build
//! from cargo alone is an installer that says it carries no package; `--about` says which it is.

#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
#[path = "../../packaging/icon_resource.rs"]
mod icon_resource;

fn main() {
    // `slint-build` is a build-dependency only on a Windows host, so the UI compiler is not even
    // built elsewhere.
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
    // What Settings and Setup share (colours, icons, the mark, the switch, the fonts) is in
    // ui/ at the top of the repository, imported as "@mujina/...".
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

/// The DLLs the executable imports are looked for in the system directory only, not first in
/// the folder it was started from, where a planted one could wait (a Downloads folder, say).
/// Windows 10 1607 and later honour it; `main` does the same for DLLs loaded later.
#[cfg(windows)]
fn harden_imports() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // 0x800 is LOAD_LIBRARY_SEARCH_SYSTEM32.
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
}
