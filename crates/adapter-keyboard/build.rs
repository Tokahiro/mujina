//! Lists every device profile in `profiles/devices/` for the crate to build in, so that a device
//! whose buttons are key chords is one file and no Rust edit. A profile that does not parse fails
//! the crate's tests, which name the file.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles/devices");
    // For a directory, Cargo scans all of it for modifications, a new file included.
    println!("cargo:rerun-if-changed={}", directory.display());
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => panic!("{}: {error}", directory.display()),
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    // By file name, so the order does not depend on the file system.
    files.sort();

    let mut code = String::from(
        "/// Every file in `profiles/devices/`, by name: the file name and its text.\n\
         pub(crate) const BUILTIN: &[(&str, &str)] = &[\n",
    );
    for path in &files {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(path) = path.to_str() else {
            panic!("{}: not a path Rust source can name", path.display());
        };
        // Debug formatting makes it a string literal, backslashes escaped. Written into a
        // String, which cannot fail.
        let _ = writeln!(code, "    ({name:?}, include_str!({path:?})),");
    }
    code.push_str("];\n");

    let Some(out) = std::env::var_os("OUT_DIR") else {
        panic!("OUT_DIR is not set");
    };
    let target = PathBuf::from(out).join("profiles.rs");
    if let Err(error) = std::fs::write(&target, code) {
        panic!("{}: {error}", target.display());
    }
}
