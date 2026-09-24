//! Builds the MSIX package, unsigned: `packaging/sign.ps1` signs it in a job that runs no cargo
//! (docs/signing.md).
//!
//! - `MSIX_PUBLISHER`: the manifest's publisher (default `CN=Mujina Dev`). It must equal the
//!   signing certificate's subject and never change between releases: the package family
//!   derives from it.
//! - `MSIX_REVISION`: fourth component of the package version (default `0`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{TaskResult, workspace};

const BINARIES: [&str; 3] = ["mujina.exe", "mujinactl.exe", "mujina-settings.exe"];

#[derive(Debug, PartialEq, Eq)]
struct Options {
    build: bool,
    timings: bool,
}

impl Options {
    /// Refuses what it does not know, so a mistyped option in CI fails instead of being ignored.
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Options {
            build: true,
            timings: false,
        };
        for argument in arguments {
            match argument.as_str() {
                "--no-build" => options.build = false,
                "--timings" => options.timings = true,
                other => return Err(format!("package: unknown option {other}")),
            }
        }
        Ok(options)
    }

    fn cargo_build(&self) -> Vec<&'static str> {
        let mut arguments = vec![
            "build",
            "--locked",
            "--release",
            "-p",
            "mujina-app",
            "-p",
            "mujina-settings-gui",
        ];
        if self.timings {
            arguments.push("--timings");
        }
        arguments
    }
}

pub fn run(arguments: &[String]) -> TaskResult {
    let options = Options::parse(arguments)?;
    let msix = build(&options)?;
    println!("unsigned {} (packaging/sign.ps1 signs it)", msix.display());
    Ok(())
}

/// Builds the package and returns its path; `build_binaries` runs cargo first.
pub fn unsigned(build_binaries: bool) -> Result<PathBuf, String> {
    build(&Options {
        build: build_binaries,
        timings: false,
    })
}

fn build(options: &Options) -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Err("packaging needs the Windows SDK and only works on Windows".to_string());
    }

    let root = workspace::root();
    let release = root.join("target").join("release");
    let out = root.join("target").join("package");
    let layout = out.join("layout");

    if options.build {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        run_tool(
            Command::new(cargo)
                .args(options.cargo_build())
                .current_dir(&root),
        )?;
    }

    if layout.exists() {
        fs::remove_dir_all(&layout).map_err(|error| format!("{}: {error}", layout.display()))?;
    }
    fs::create_dir_all(&layout).map_err(|error| format!("{}: {error}", layout.display()))?;

    for binary in BINARIES {
        copy(&release.join(binary), &layout.join(binary))?;
    }
    let packaging = root.join("packaging");
    copy(
        &packaging.join("CustomCapability.SCCD"),
        &layout.join("CustomCapability.SCCD"),
    )?;
    copy_dir(&packaging.join("Assets"), &layout.join("Assets"))?;
    copy_dir(&packaging.join("Public"), &layout.join("Public"))?;

    let version = workspace::version()?;
    let revision = env_or("MSIX_REVISION", "0");
    let publisher = env_or("MSIX_PUBLISHER", "CN=Mujina Dev");
    let package_version = format!("{version}.{revision}");
    let template = packaging.join("AppxManifest.xml.in");
    let manifest = fs::read_to_string(&template)
        .map_err(|error| format!("{}: {error}", template.display()))?
        .replace("@VERSION@", &package_version)
        .replace("@PUBLISHER@", &xml_escape(&publisher));
    fs::write(layout.join("AppxManifest.xml"), manifest)
        .map_err(|error| format!("AppxManifest.xml: {error}"))?;

    // resources.pri: only through it does Windows find the logo's other sizes and forms, such as
    // the taskbar's unplated 24 px one.
    let makepri = sdk_tool("makepri.exe")?;
    let config = out.join("priconfig.xml");
    run_tool(
        Command::new(&makepri)
            .args(["createconfig", "/cf"])
            .arg(&config)
            .args(["/dq", "en-US", "/pv", "10.0.0", "/o"]),
    )?;
    run_tool(
        Command::new(&makepri)
            .args(["new", "/pr"])
            .arg(&layout)
            .arg("/cf")
            .arg(&config)
            .arg("/mn")
            .arg(layout.join("AppxManifest.xml"))
            .arg("/of")
            .arg(layout.join("resources.pri"))
            .arg("/o"),
    )?;

    let msix = out.join(format!("Mujina_{package_version}_x64.msix"));
    run_tool(
        Command::new(sdk_tool("makeappx.exe")?)
            .args(["pack", "/o", "/d"])
            .arg(&layout)
            .arg("/p")
            .arg(&msix),
    )?;
    Ok(msix)
}

/// An environment variable, treating "set but empty" (how CI passes an undefined variable) as
/// unset.
pub(crate) fn env_or(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn run_tool(command: &mut Command) -> TaskResult {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .map_err(|error| format!("{program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed with {status}"))
    }
}

fn copy(from: &Path, to: &Path) -> TaskResult {
    fs::copy(from, to)
        .map(drop)
        .map_err(|error| format!("{}: {error}", from.display()))
}

fn copy_dir(from: &Path, to: &Path) -> TaskResult {
    fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", from.display()))?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            copy(&entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Finds a tool in the newest installed Windows SDK.
fn sdk_tool(name: &str) -> Result<PathBuf, String> {
    let program_files = std::env::var("ProgramFiles(x86)")
        .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string());
    let bin = Path::new(&program_files)
        .join("Windows Kits")
        .join("10")
        .join("bin");

    let mut candidates: Vec<(Vec<u32>, PathBuf)> = fs::read_dir(&bin)
        .map_err(|error| format!("Windows SDK not found at {}: {error}", bin.display()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version = parse_version(&entry.file_name().to_string_lossy())?;
            let tool = entry.path().join("x64").join(name);
            tool.is_file().then_some((version, tool))
        })
        .collect();
    candidates.sort();
    candidates.pop().map(|(_, tool)| tool).ok_or_else(|| {
        format!(
            "{name} not found in any Windows SDK under {}",
            bin.display()
        )
    })
}

fn parse_version(text: &str) -> Option<Vec<u32>> {
    text.split('.').map(|part| part.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_versions_sort_numerically() {
        assert!(parse_version("10.0.26100.0") > parse_version("10.0.9999.0"));
        assert_eq!(parse_version("x64"), None);
    }

    #[test]
    fn publisher_is_escaped_for_the_manifest() {
        assert_eq!(xml_escape(r#"CN="A&B""#), "CN=&quot;A&amp;B&quot;");
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter()
            .map(|argument| (*argument).to_string())
            .collect()
    }

    #[test]
    fn options_are_parsed_and_typos_refused() {
        let plain = Options {
            build: true,
            timings: false,
        };
        assert_eq!(Options::parse(&[]), Ok(plain));
        assert_eq!(
            Options::parse(&strings(&["--timings", "--no-build"])),
            Ok(Options {
                build: false,
                timings: true,
            })
        );
        assert!(Options::parse(&strings(&["--timing"])).is_err());
    }

    #[test]
    fn the_build_is_locked_and_timed_only_on_request() {
        let plain = Options {
            build: true,
            timings: false,
        }
        .cargo_build();
        assert!(plain.contains(&"--locked"));
        assert!(!plain.contains(&"--timings"));
        let timed = Options {
            build: true,
            timings: true,
        }
        .cargo_build();
        assert!(timed.contains(&"--timings"));
    }
}
