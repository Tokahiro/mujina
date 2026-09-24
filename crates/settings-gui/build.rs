//! Compiles the user interface in `ui/` into Rust, with the texts in `lang/` built in, and gives
//! the executable Mujina's icon.

#[cfg(windows)]
#[path = "../../packaging/icon_resource.rs"]
mod icon_resource;

fn main() {
    // The app exists on Windows only; elsewhere the crate builds to a stub. `slint-build` is a
    // build-dependency only on a Windows host, so the UI compiler is not even built elsewhere.
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        compile_ui();
    }
}

#[cfg(windows)]
fn compile_ui() {
    icon_resource::embed(std::path::Path::new("../../packaging/mujina.ico"));
    println!("cargo:rerun-if-changed=lang");
    // What Settings and Setup share (colours, icons, the mark, the switch, the fonts) is in
    // ui/ at the top of the repository, imported as "@mujina/...".
    let shared = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui");
    let libraries = std::collections::HashMap::from([("mujina".to_string(), shared)]);
    // Texts in lang/<language>/LC_MESSAGES/mujina-settings-gui.po; Slint picks the language
    // Windows shows.
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent-dark".into())
        .with_bundled_translations("lang")
        .with_library_paths(libraries);
    if let Err(error) = slint_build::compile_with_config("ui/main.slint", config) {
        panic!("ui/main.slint: {error}");
    }
}
