//! Compiles `ui/` with the `lang/` translations bundled, and embeds Mujina's icon.

#[cfg(windows)]
#[path = "../../packaging/icon_resource.rs"]
mod icon_resource;

fn main() {
    // Windows only: elsewhere the crate is a stub and `slint-build` is not a dependency.
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        compile_ui();
    }
}

#[cfg(windows)]
fn compile_ui() {
    icon_resource::embed(std::path::Path::new("../../packaging/mujina.ico"));
    println!("cargo:rerun-if-changed=lang");
    // Shared with Mujina Setup: ui/ at the repository root, imported as "@mujina/...".
    let shared = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui");
    let libraries = std::collections::HashMap::from([("mujina".to_string(), shared)]);
    // Translations: lang/<language>/LC_MESSAGES/mujina-settings-gui.po.
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_bundled_translations("lang")
        .with_library_paths(libraries);
    if let Err(error) = slint_build::compile_with_config("ui/main.slint", config) {
        panic!("ui/main.slint: {error}");
    }
}
