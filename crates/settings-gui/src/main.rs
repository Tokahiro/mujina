//! A window onto `mujinactl`, in its own process so the agent never pays for it (ADR-0011).

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod capture;
#[cfg(windows)]
mod feedback;
#[cfg(windows)]
mod form;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod nav;
#[cfg(windows)]
mod pad;
#[cfg(windows)]
mod rows;
#[cfg(windows)]
mod setup;
#[cfg(windows)]
mod status;
#[cfg(windows)]
mod texts;

// Generated code follows Slint's conventions, not this workspace's lints.
#[cfg(windows)]
#[allow(
    unused,
    unsafe_op_in_unsafe_fn,
    clippy::all,
    clippy::pedantic,
    clippy::undocumented_unsafe_blocks
)]
mod ui {
    slint::include_modules!();
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    match app::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("Mujina Settings only runs on Windows.");
    std::process::ExitCode::FAILURE
}

#[cfg(all(test, windows))]
mod tests {
    use std::path::Path;

    fn sources(dir: &Path) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                found.extend(sources(&path));
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                found.push((path.display().to_string(), text));
            }
        }
        found
    }

    fn identifier(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn after_crate<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
        text.match_indices(name)
            .filter(|(at, _)| !text[..*at].ends_with(identifier))
            .map(|(at, _)| &text[at + name.len()..])
            .filter(|rest| !rest.starts_with(identifier))
            .collect()
    }

    /// arch-check sees crates, not modules. Keeps the app from being a second composition root.
    #[test]
    fn mujina_app_is_reached_through_its_tool_module_only() {
        // Written in two, so that this file does not name the crate itself.
        let crate_name = concat!("mujina", "_app");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let files = sources(&src);
        assert!(files.len() > 1);
        for (file, text) in &files {
            for rest in after_crate(text, crate_name) {
                let through_tool = rest
                    .strip_prefix("::tool")
                    .is_some_and(|rest| !rest.starts_with(identifier));
                assert!(
                    through_tool,
                    "{file} names {crate_name}{}",
                    rest.lines().next().unwrap_or_default()
                );
            }
        }
    }

    #[test]
    fn a_crate_is_told_from_one_whose_name_starts_the_same() {
        let text = "use house_boat::Oar;\nuse house::rooms;\nhouse::tool::x()";
        assert_eq!(
            after_crate(text, "house"),
            ["::rooms;\nhouse::tool::x()", "::tool::x()"]
        );
    }
}
