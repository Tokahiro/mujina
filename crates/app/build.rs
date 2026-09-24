//! Gives `mujina.exe` and `mujinactl.exe` Mujina's icon.

#[path = "../../packaging/icon_resource.rs"]
mod icon_resource;

fn main() {
    // The executables exist on Windows only.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    icon_resource::embed(std::path::Path::new("../../packaging/mujina.ico"));
}
