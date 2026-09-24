//! Project automation, run as `cargo xtask <task>`.

mod arch;
mod changelog;
mod dist;
mod i18n;
mod package;
mod scaffold;
mod workspace;

use std::process::ExitCode;

const USAGE: &str = "\
cargo xtask <TASK> [--help]

TASKS:
    arch-check                                  Verify that crate dependencies point inward only
    i18n-check                                  Verify that every text shown has its translations
    version-check [TAG]                         Verify that a release tag (default: $GITHUB_REF_NAME) matches Cargo.toml
    package [--no-build] [--timings]            Build the unsigned MSIX package into target/package (Windows only)
    dist --pfx <FILE> [--no-build]              Build, sign and attach what a release publishes into target/dist (Windows only)
    release-notes [TAG]                         Print the CHANGELOG.md entry of a release tag (default: $GITHUB_REF_NAME)
    new-launcher <ID> [--name <NAME>]           Make a launcher's crate, crates/adapter-<ID>, and list it (docs/new-launcher.md)
    new-device <ID> [--name <NAME>]             Make a device's crate, crates/adapter-<ID>, and list it (docs/new-device.md)
    new-device <ID> --profile [--name <NAME>]   Make a key-chord device's profile, profiles/devices/<ID>.toml
";

type TaskResult = Result<(), String>;

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let task = arguments.next();
    let rest: Vec<String> = arguments.collect();

    if rest.iter().any(|arg| arg == "--help" || arg == "-h")
        && let Some(help) = task.as_deref().and_then(help)
    {
        print!("{help}");
        return ExitCode::SUCCESS;
    }

    let result = match task.as_deref() {
        Some("arch-check") => arch::check(),
        Some("i18n-check") => i18n::check(),
        Some("version-check") => workspace::version_check(rest.first().map(String::as_str)),
        Some("package") => package::run(&rest),
        Some("dist") => dist::run(&rest),
        Some("release-notes") => changelog::release_notes(rest.first().map(String::as_str)),
        Some("new-launcher") => scaffold::new_launcher(&rest),
        Some("new-device") => scaffold::new_device(&rest),
        Some("--help" | "-h") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// `task`'s lines of the usage, and for the skeletons how an id is spelt; `None` for no task.
fn help(task: &str) -> Option<String> {
    let lines: Vec<&str> = USAGE
        .lines()
        .map(str::trim_start)
        .filter(|line| line.split_whitespace().next() == Some(task))
        .collect();
    if lines.is_empty() {
        return None;
    }
    let mut help = String::new();
    for line in lines {
        help.push_str("cargo xtask ");
        help.push_str(line);
        help.push('\n');
    }
    help.push_str(&scaffold::id_help(task).unwrap_or_default());
    Some(help)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_s_help_is_its_usage() {
        let device = help("new-device").unwrap();
        assert!(
            device.starts_with("cargo xtask new-device <ID> [--name <NAME>]"),
            "{device}"
        );
        assert!(
            device.contains("new-device <ID> --profile [--name <NAME>]"),
            "{device}"
        );
        assert!(device.contains("<ID>: lower case letters"), "{device}");
        assert!(!device.contains("new-launcher"), "{device}");
        assert!(help("arch-check").unwrap().contains("point inward"));
        assert_eq!(help("nothing"), None);
    }
}
