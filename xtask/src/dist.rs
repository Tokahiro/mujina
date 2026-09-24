//! `cargo xtask dist`: the release's files, built on a developer's machine the way the release
//! workflow builds them (ADR-0015), with a certificate the developer supplies.
//!
//! 1. The package, unsigned (`cargo xtask package`), and Mujina Setup without it, in the release
//!    profile.
//! 2. `packaging/sign.ps1` signs the package and writes its certificate.
//! 3. `packaging/attach-payload.ps1` attaches both to Mujina Setup.
//! 4. `packaging/sign.ps1` signs Mujina Setup.
//! 5. `SHA256SUMS.txt`, and Mujina Setup's `--about` as a last check.
//!
//! Everything lands in `target/dist`: `Mujina-Setup-v<version>.exe`, the package, `Mujina.cer`
//! and `SHA256SUMS.txt`, as a release publishes them. The workflows run the same two scripts,
//! with no cargo in the job that holds the key; here it is one machine, the developer's.
//!
//! Options:
//! - `--pfx <file>`: the certificate to sign with (required). Its password comes from
//!   `SIGNING_PFX_PASSWORD`. signtool leaves a copy of its private key in the user's key store:
//!   use a development certificate (docs/signing.md), never the release one.
//! - `--no-build`: package and attach what target/release already holds.
//!
//! `MSIX_PUBLISHER` is read as by `cargo xtask package` and must be the certificate's subject.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::package::{self, env_or, run_tool};
use crate::{TaskResult, workspace};

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    pfx: PathBuf,
    build: bool,
}

impl Options {
    fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut pfx = None;
        let mut build = true;
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--pfx" => {
                    let file = arguments.next().ok_or("dist: --pfx needs a file")?;
                    pfx = Some(PathBuf::from(file));
                }
                "--no-build" => build = false,
                other => return Err(format!("dist: unknown option {other}")),
            }
        }
        let pfx = pfx.ok_or("dist: --pfx <file> is required (docs/signing.md)")?;
        Ok(Self { pfx, build })
    }
}

pub fn run(arguments: &[String]) -> TaskResult {
    let options = Options::parse(arguments)?;
    if !cfg!(windows) {
        return Err("dist needs the Windows SDK and only works on Windows".to_string());
    }
    if std::env::var_os("SIGNING_PFX_PASSWORD").is_none() {
        return Err("dist: set SIGNING_PFX_PASSWORD to the password of the --pfx file".into());
    }
    let pfx = fs::canonicalize(&options.pfx)
        .map_err(|error| format!("{}: {error}", options.pfx.display()))?;
    let root = workspace::root();
    let publisher = env_or("MSIX_PUBLISHER", "CN=Mujina Dev");
    let version = workspace::version()?;

    let built = package::unsigned(options.build)?;
    if options.build {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        run_tool(
            Command::new(cargo)
                .args(["build", "--locked", "--release", "-p", "mujina-setup"])
                .current_dir(&root),
        )?;
    }

    let out = root.join("target").join("dist");
    if out.exists() {
        fs::remove_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    }
    fs::create_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    let msix = out.join(built.file_name().ok_or("the package has no file name")?);
    fs::copy(&built, &msix).map_err(|error| format!("{}: {error}", built.display()))?;
    let cer = out.join("Mujina.cer");
    let setup = out.join(format!("Mujina-Setup-v{version}.exe"));
    let scripts = root.join("packaging");

    let sign = |file: &Path, certificate: Option<&Path>| {
        let mut command = powershell(&scripts.join("sign.ps1"));
        command
            .arg("-Path")
            .arg(file)
            .arg("-PfxFile")
            .arg(&pfx)
            .arg("-Subject")
            .arg(&publisher);
        if let Some(certificate) = certificate {
            command.arg("-Certificate").arg(certificate);
        }
        run_tool(&mut command)
    };
    sign(&msix, Some(&cer))?;
    run_tool(
        powershell(&scripts.join("attach-payload.ps1"))
            .arg("-Setup")
            .arg(root.join("target").join("release").join("mujina-setup.exe"))
            .arg("-Msix")
            .arg(&msix)
            .arg("-Certificate")
            .arg(&cer)
            .arg("-Output")
            .arg(&setup),
    )?;
    sign(&setup, None)?;

    // What a release's SHA256SUMS.txt holds, as sha256sum writes it.
    let mut sums = String::new();
    for file in [&setup, &msix, &cer] {
        let hash = sha256(file)?;
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let _ = writeln!(sums, "{hash}  {name}");
    }
    fs::write(out.join("SHA256SUMS.txt"), sums)
        .map_err(|error| format!("SHA256SUMS.txt: {error}"))?;

    // Mujina Setup says what it carries: on this machine it may run.
    let about = Command::new(&setup)
        .arg("--about")
        .output()
        .map_err(|error| format!("{}: {error}", setup.display()))?;
    print!("{}", String::from_utf8_lossy(&about.stdout));
    if !about.status.success() {
        return Err(format!("{} does not read its package", setup.display()));
    }
    println!("release-shaped files in {}", out.display());
    Ok(())
}

/// A script of `packaging/`, run by PowerShell 7 where it is installed, else by Windows
/// PowerShell from the system directory. Both run the scripts; the execution policy is set aside
/// for this one process, as it is on GitHub's runners.
fn powershell(script: &Path) -> Command {
    let program = if Command::new("pwsh").arg("-Version").output().is_ok() {
        PathBuf::from("pwsh")
    } else {
        windows_powershell()
    };
    let mut command = Command::new(program);
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script);
    command
}

/// The SHA-256 of `file`, in lower-case hex, as Windows' `Get-FileHash` computes it.
fn sha256(file: &Path) -> Result<String, String> {
    let output = Command::new(windows_powershell())
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-FileHash -Algorithm SHA256 -LiteralPath $env:MUJINA_FILE).Hash.ToLowerInvariant()",
        ])
        .env("MUJINA_FILE", file)
        .output()
        .map_err(|error| format!("Get-FileHash: {error}"))?;
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && hash.len() == 64 {
        Ok(hash)
    } else {
        Err(format!("Get-FileHash {} failed", file.display()))
    }
}

/// Windows PowerShell, from the system directory.
fn windows_powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_certificate_is_required_and_typos_are_refused() {
        assert_eq!(
            Options::parse(&strings(&["--pfx", "dev.pfx"])),
            Ok(Options {
                pfx: PathBuf::from("dev.pfx"),
                build: true,
            })
        );
        assert_eq!(
            Options::parse(&strings(&["--no-build", "--pfx", "dev.pfx"])),
            Ok(Options {
                pfx: PathBuf::from("dev.pfx"),
                build: false,
            })
        );
        assert!(Options::parse(&[]).is_err());
        assert!(Options::parse(&strings(&["--pfx"])).is_err());
        assert!(Options::parse(&strings(&["--pfx", "dev.pfx", "--sign"])).is_err());
    }
}
