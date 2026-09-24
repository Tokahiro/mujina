//! The runner's host on the machine: the registry and the certificate store directly, the home
//! app through mujina-application's `HomeAppRegistration`, and PowerShell's `Add-AppxPackage` and
//! `Remove-AppxPackage` for the package. Every program is started by its full path, never looked
//! up by name, and data reaches PowerShell only through environment variables, never inside the
//! command text.

use std::ffi::OsStr;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use mujina_adapter_windows::agent_control;
use mujina_adapter_windows::checks;
use mujina_adapter_windows::home_registry::WindowsHomeAppRegistry;
use mujina_adapter_windows::identity::{self, FamilyIdentity};
use mujina_adapter_windows::paths;
use mujina_application::ports::HomeAppRegistry as _;
use mujina_application::register::{HomeAppRegistration, RegisterError, same_app};
use mujina_winutil::registry::{self, Hive};
use mujina_winutil::shell::{self, ElevationError};
use mujina_winutil::window::WindowHandle;
use mujina_winutil::{certstore, library, package, process, time};

use crate::cli::{self, ElevatedFailure};
use crate::journal::{self, FileJournal};
use crate::payload::{self, Payload};
use crate::plan::{Facts, Package, Version};
use crate::run::{
    CreatedFiles, ErrorKind, HomeApp, Journal, Packages, Probe, SignInCheck, StepError, Trust,
};

const DEVELOPER_MODE_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock";
const DEVELOPER_MODE_VALUE: &str = "AllowDevelopmentWithoutDevLicense";
/// Where Windows keeps its build number; `CurrentBuildNumber` is a string such as `26200`.
const VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The Run value of the check at sign-in is this, a dot and the family ([`run_value`]).
const RUN_VALUE_PREFIX: &str = "MujinaCleanup";
/// Mujina's own key under the current user: `Created` (below) and Setup's own subkey live in it.
const MUJINA_KEY: &str = r"Software\Mujina";
/// Every file Mujina made outside its own folders: one string value each, named after what the
/// file is for, holding its full path. Written only by whoever made the file, and only when it
/// did not exist before, so a file of the user's own is never listed.
const CREATED_KEY: &str = r"Software\Mujina\Created";
/// What the check at sign-in keeps between sign-ins, per family (`Setup\<family>`): how often
/// giving the home app back failed. A subkey of its own, since `Software\Mujina` goes as soon as
/// the created files are gone.
const SETUP_KEY: &str = r"Software\Mujina\Setup";
const ATTEMPTS_VALUE: &str = "CleanupAttempts";
/// No console window for the console programs.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The machine as the runner sees it, for the package attached to this executable.
pub struct WindowsHost {
    payload: Option<&'static Payload>,
    /// The family Setup works on: the attached package's.
    family: Option<String>,
    /// The window the administrator prompt belongs to.
    owner: Option<WindowHandle>,
    /// None when Setup runs with administrator rights and no `--log` names a file.
    journal: Option<FileJournal>,
    work: WorkFolder,
}

impl WindowsHost {
    /// `log`: the log, as [`log_file`] chose it, or none.
    pub fn new(log: Option<PathBuf>) -> Self {
        let payload = payload::attached().ok();
        let family = payload.map(|payload| payload.about.family.clone());
        Self {
            payload,
            family,
            owner: None,
            journal: log.map(|path| FileJournal::new(path, time::local_timestamp)),
            work: WorkFolder::new(),
        }
    }

    /// Asks for administrator rights in front of `owner`, the Setup window.
    pub fn with_owner(mut self, owner: Option<WindowHandle>) -> Self {
        self.owner = owner;
        self
    }

    pub fn log_path(&self) -> Option<&Path> {
        self.journal.as_ref().map(FileJournal::path)
    }

    fn family(&self) -> Result<&str, StepError> {
        self.family
            .as_deref()
            .ok_or_else(|| StepError::new(ErrorKind::NoPackage, no_package()))
    }

    fn payload(&self) -> Result<&'static Payload, StepError> {
        self.payload
            .ok_or_else(|| StepError::new(ErrorKind::NoPackage, no_package()))
    }

    fn registration<T>(
        &self,
        act: impl FnOnce(&HomeAppRegistration<'_>) -> Result<T, RegisterError>,
    ) -> Result<T, StepError> {
        let identity = FamilyIdentity::new(self.family()?);
        let registry = WindowsHomeAppRegistry;
        act(&HomeAppRegistration::new(&identity, &registry))
            .map_err(|error| StepError::new(ErrorKind::HomeApp, error.to_string()))
    }
}

/// Where the log goes: where `--log` says, else its usual place, but not with administrator
/// rights. The usual folder is the user's to change, and any program of theirs could make it a
/// link to a folder of the system's; there, creating the log, or renaming it once it is long,
/// would be done as administrator (SECURITY.md).
pub fn log_file(requested: Option<PathBuf>) -> Option<PathBuf> {
    requested.or_else(|| (!process::is_elevated()).then(default_log))
}

/// The log's usual place.
fn default_log() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
    journal::default_path(&local)
}

fn no_package() -> String {
    match payload::attached() {
        Ok(_) => "no package family".to_string(),
        Err(missing) => missing.to_string(),
    }
}

impl Probe for WindowsHost {
    fn facts(&self) -> Facts {
        let family = self.family.as_deref();
        let installed = family.and_then(|family| {
            package::installed_full_names(family)
                .iter()
                .filter_map(|name| Version::from_full_name(name))
                .max()
        });
        let home_app_is_this = family.is_some_and(|family| {
            WindowsHomeAppRegistry
                .current()
                .ok()
                .flatten()
                .is_some_and(|current| same_app(&current, &identity::app_user_model_id(family)))
        });
        Facts {
            os_build: registry::read_string(Hive::LocalMachine, VERSION_KEY, "CurrentBuildNumber")
                .ok()
                .flatten()
                .and_then(|build| build.trim().parse().ok()),
            dev_mode: checks::developer_mode(),
            cert_trusted: self
                .payload
                .is_some_and(|payload| certstore::contains(payload.cer)),
            installed,
            carried: self.payload.map(|payload| payload.about.version),
            home_app_is_this,
            agent_running: agent_control::agent_is_running(),
        }
    }
}

impl Trust for WindowsHost {
    /// The administrator part, all in one prompt: this program again, with `--elevated` and
    /// nothing else. It takes the certificate from its own file, not from anything written here.
    fn prepare(&self) -> Result<(), StepError> {
        self.payload()?;
        let program = std::env::current_exe()
            .map_err(|error| StepError::new(ErrorKind::ElevationFailed, error.to_string()))?;
        match shell::run_elevated(&program, cli::ELEVATED, self.owner) {
            Ok(0) => Ok(()),
            Ok(code) => Err(match ElevatedFailure::from_code(code) {
                Some(failure) => StepError::with_code(failure.kind(), code, format!("{failure:?}")),
                None => StepError::with_code(
                    ErrorKind::Preparing,
                    code,
                    "the administrator part ended unexpectedly",
                ),
            }),
            Err(ElevationError::Declined) => Err(StepError::new(
                ErrorKind::Declined,
                "the administrator prompt was declined",
            )),
            Err(ElevationError::Failed(error)) => Err(StepError::with_code(
                ErrorKind::ElevationFailed,
                error.code,
                error.to_string(),
            )),
        }
    }
}

impl Packages for WindowsHost {
    /// `Add-AppxPackage`, which ends the package's running processes (`-ForceApplicationShutdown`):
    /// the agent, if Mujina runs in Xbox mode now. It starts again the next time Xbox mode starts
    /// Mujina. A confirmed downgrade adds `-ForceUpdateFromAnyVersion`.
    fn add(&self, how: Package) -> Result<(), StepError> {
        let payload = self.payload()?;
        let msix = self
            .work
            .unpack(&payload.about.msix, payload.msix)
            .map_err(|error| StepError::new(ErrorKind::Other, error))?;
        let force = if matches!(how, Package::Downgrade { .. }) {
            " -ForceUpdateFromAnyVersion"
        } else {
            ""
        };
        let command = format!(
            "Add-AppxPackage -Path $env:MUJINA_MSIX -ForceApplicationShutdown{force} \
             -ErrorAction Stop"
        );
        powershell(&command, &[("MUJINA_MSIX", msix.as_os_str())])
            .map_err(|output| StepError::deployment(&output))
    }

    fn remove(&self) -> Result<(), StepError> {
        let family = self.family()?;
        // Without this Mujina there is nothing of this installer's to remove, and what comes after
        // (the listed files, the check at sign-in) may belong to another Mujina.
        if !package::is_installed(family) {
            return Err(StepError::new(
                ErrorKind::NotInstalled,
                format!("this Mujina ({family}) is not installed"),
            ));
        }
        powershell(
            "Get-AppxPackage | Where-Object PackageFamilyName -EQ $env:MUJINA_FAMILY \
             | Remove-AppxPackage -ErrorAction Stop",
            &[("MUJINA_FAMILY", OsStr::new(family))],
        )
        .map_err(|output| StepError::deployment(&output))
    }

    fn installed(&self) -> bool {
        self.family.as_deref().is_some_and(package::is_installed)
    }
}

// The methods themselves are tied to one registration's lifetime, which the registration built
// inside `registration` does not have.
#[allow(clippy::redundant_closure_for_method_calls)]
impl HomeApp for WindowsHost {
    fn make(&self) -> Result<(), StepError> {
        self.registration(|registration| registration.register())
            .map(drop)
    }

    fn give_back(&self) -> Result<(), StepError> {
        self.registration(|registration| registration.unregister())
            .map(drop)
    }
}

/// The Run value of the check at sign-in for `family`: one per family, so that a CI build beside
/// a release keeps its own.
fn run_value(family: &str) -> String {
    format!("{RUN_VALUE_PREFIX}.{family}")
}

/// Where the check at sign-in for `family` counts its failed attempts.
fn attempts_key(family: &str) -> String {
    format!(r"{SETUP_KEY}\{family}")
}

impl WindowsHost {
    fn failed(error: impl std::fmt::Display) -> StepError {
        StepError::new(ErrorKind::SignInCheck, error.to_string())
    }

    fn retained_copy(&self) -> Result<PathBuf, StepError> {
        paths::retained_setup(self.family()?).ok_or_else(|| Self::failed("LOCALAPPDATA is not set"))
    }
}

impl SignInCheck for WindowsHost {
    /// Copies this program to where it can stay, and has it look at every sign-in.
    fn arrange_check(&self) -> Result<(), StepError> {
        let family = self.family()?;
        let copy = self.retained_copy()?;
        let current = std::env::current_exe().map_err(Self::failed)?;
        if current != copy {
            if let Some(folder) = copy.parent() {
                std::fs::create_dir_all(folder).map_err(Self::failed)?;
            }
            std::fs::copy(&current, &copy)
                .map_err(|error| Self::failed(format!("{}: {error}", copy.display())))?;
        }
        // No family on the command line: the copy knows it from its package.
        registry::write_string(
            Hive::CurrentUser,
            RUN_KEY,
            &run_value(family),
            &format!("\"{}\" --cleanup", copy.display()),
        )
        .map_err(Self::failed)?;
        // A check that failed at earlier sign-ins starts counting again: this installation's
        // removal gets all its attempts. Should the count stay, the check only gives up sooner.
        let _ = registry::delete_key(Hive::CurrentUser, &attempts_key(family));
        Ok(())
    }

    fn remove_check(&self) -> Result<(), StepError> {
        let family = self.family()?;
        registry::delete_value(Hive::CurrentUser, RUN_KEY, &run_value(family))
            .map_err(Self::failed)?;
        registry::delete_key(Hive::CurrentUser, &attempts_key(family)).map_err(Self::failed)?;
        // Best effort: each goes only once nothing else is in it.
        let _ = registry::delete_key(Hive::CurrentUser, SETUP_KEY);
        let _ = registry::delete_key(Hive::CurrentUser, MUJINA_KEY);
        remove_copy(&self.retained_copy()?).map_err(Self::failed)
    }

    fn attempts(&self) -> u32 {
        self.family
            .as_deref()
            .and_then(|family| {
                registry::read_u32(Hive::CurrentUser, &attempts_key(family), ATTEMPTS_VALUE)
                    .ok()
                    .flatten()
            })
            .unwrap_or(0)
    }

    fn record_attempts(&self, attempts: u32) -> Result<(), StepError> {
        let key = attempts_key(self.family()?);
        registry::write_u32(Hive::CurrentUser, &key, ATTEMPTS_VALUE, attempts).map_err(Self::failed)
    }
}

impl CreatedFiles for WindowsHost {
    /// Deletes every file recorded under `CREATED_KEY`, as this user, then the record. A file
    /// that is gone already counts as deleted; if one cannot be deleted, the record stays, so
    /// that the check at sign-in tries again.
    fn forget(&self) -> Result<(), StepError> {
        let failed = |error: String| StepError::new(ErrorKind::Files, error);
        // Anything running as this user can add to the list. With administrator rights this
        // program must not delete what it names; the check at sign-in, which runs without them,
        // does.
        if process::is_elevated() {
            return Ok(());
        }
        let created = registry::string_values(Hive::CurrentUser, CREATED_KEY)
            .map_err(|error| failed(error.to_string()))?;
        let mut undeleted = Vec::new();
        for path in created.iter().filter_map(|(_, path)| recorded_file(path)) {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => undeleted.push(format!("{}: {error}", path.display())),
            }
        }
        if !undeleted.is_empty() {
            return Err(failed(undeleted.join("; ")));
        }
        registry::delete_key(Hive::CurrentUser, CREATED_KEY)
            .map_err(|error| failed(error.to_string()))?;
        // Its parent was created along with it. Best effort: a parent that still has other
        // subkeys (Setup's own, while the check at sign-in counts its attempts) stays.
        let _ = registry::delete_key(Hive::CurrentUser, MUJINA_KEY);
        Ok(())
    }

    fn remain(&self) -> bool {
        registry::string_values(Hive::CurrentUser, CREATED_KEY)
            .is_ok_and(|values| !values.is_empty())
    }
}

impl Journal for WindowsHost {
    fn note(&self, line: &str) {
        if let Some(journal) = &self.journal {
            journal.note(line);
        }
    }
}

/// `mujina-setup.exe --elevated`: what needs administrator rights. It starts no program and
/// trusts only the certificate attached to this file.
pub fn elevated() -> ExitCode {
    // A build without a package carries no certificate either: then it changes nothing.
    let Ok(payload) = payload::attached() else {
        return ExitCode::from(ElevatedFailure::Certificate.code());
    };
    if registry::write_u32(
        Hive::LocalMachine,
        DEVELOPER_MODE_KEY,
        DEVELOPER_MODE_VALUE,
        1,
    )
    .is_err()
    {
        return ExitCode::from(ElevatedFailure::DeveloperMode.code());
    }
    if certstore::add(payload.cer).is_err() {
        return ExitCode::from(ElevatedFailure::Certificate.code());
    }
    ExitCode::SUCCESS
}

/// The folders a copy of Setup leaves, innermost first, each to go only once empty: its own
/// (`Mujina\<family>`), and `Mujina` above it.
fn copy_folders(copy: &Path) -> Vec<&Path> {
    let folder = copy.parent();
    let parent = folder
        .and_then(Path::parent)
        .filter(|parent| parent.file_name().is_some_and(|name| name == "Mujina"));
    folder.into_iter().chain(parent).collect()
}

/// Removes the copy of Setup and its folders, where nothing else is in them. A running program
/// cannot delete itself, so when this is the copy, PowerShell waits for it to end, which may be
/// long after this step: Remove Mujina's window stays open until it is closed. The wait is
/// bounded in case the process ended before PowerShell looked and its ID went to another; a few
/// tries after it cover a file Windows still holds a moment after the end.
fn remove_copy(copy: &Path) -> Result<(), String> {
    let folders = copy_folders(copy);
    if std::env::current_exe().ok().as_deref() != Some(copy) {
        let _ = std::fs::remove_file(copy);
        for folder in folders {
            let _ = std::fs::remove_dir(folder);
        }
        return Ok(());
    }
    let folder = |index: usize| folders.get(index).map(|folder| folder.as_os_str());
    let system = library::system_directory().ok_or("the system directory was not found")?;
    Command::new(powershell_program()?)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Wait-Process -Id $env:MUJINA_PID -Timeout 3600 -ErrorAction SilentlyContinue; \
             foreach ($attempt in 1..10) { \
             Remove-Item -LiteralPath $env:MUJINA_COPY -Force -ErrorAction SilentlyContinue; \
             if (-not (Test-Path -LiteralPath $env:MUJINA_COPY)) { break }; \
             Start-Sleep -Milliseconds 500 }; \
             foreach ($folder in $env:MUJINA_FOLDER, $env:MUJINA_PARENT) { if ($folder) { \
             Remove-Item -LiteralPath $folder -ErrorAction SilentlyContinue } }",
        ])
        .env("MUJINA_PID", std::process::id().to_string())
        .env("MUJINA_COPY", copy)
        .env("MUJINA_FOLDER", folder(0).unwrap_or_default())
        .env("MUJINA_PARENT", folder(1).unwrap_or_default())
        // Not the folder it is about to remove.
        .current_dir(system)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(drop)
        .map_err(|error| format!("powershell.exe: {error}"))
}

/// A recorded path worth deleting: a full one. Anything else would depend on the folder this
/// program happens to run in.
fn recorded_file(path: &str) -> Option<&Path> {
    Some(Path::new(path)).filter(|path| path.is_absolute())
}

/// Windows PowerShell in the system directory, never one found next to this program.
fn powershell_program() -> Result<PathBuf, String> {
    let system = library::system_directory().ok_or("the system directory was not found")?;
    Ok(system.join(r"WindowsPowerShell\v1.0\powershell.exe"))
}

/// Runs a fixed PowerShell command; what varies comes in as environment variables. On failure,
/// what PowerShell said (its error stream, else its output).
fn powershell(command: &str, environment: &[(&str, &OsStr)]) -> Result<(), String> {
    // Not the folder this program was started from, usually Downloads: a child looks for DLLs in
    // its working folder too.
    let system = library::system_directory().ok_or("the system directory was not found")?;
    let output = Command::new(powershell_program()?)
        .args(["-NoProfile", "-NonInteractive", "-Command", command])
        .envs(environment.iter().copied())
        .current_dir(system)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("powershell.exe: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if text.is_empty() {
        text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    }
    Err(format!("powershell.exe failed: {text}"))
}

/// A folder of this process's own for the package it unpacks, removed with everything in it
/// when Setup ends, whatever happened.
struct WorkFolder(PathBuf);

impl WorkFolder {
    fn new() -> Self {
        // One folder per instance, not only per process: tests build several hosts in one
        // process, and dropping one must not remove the folder another is writing to.
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let instance = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!("mujina-setup-{}-{instance}", std::process::id())))
    }

    /// Writes `bytes` into the folder as `name`, a bare file name.
    fn unpack(&self, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.0)
            .map_err(|error| format!("{}: {error}", self.0.display()))?;
        let path = self.0.join(name);
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(path)
    }
}

impl Drop for WorkFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_full_paths_are_deleted() {
        let marker = r"C:\Program Files (x86)\Steam\.cef-enable-remote-debugging";
        assert_eq!(recorded_file(marker), Some(Path::new(marker)));
        assert_eq!(
            recorded_file(r"\\server\share\file"),
            Some(Path::new(r"\\server\share\file"))
        );
        assert_eq!(recorded_file(".cef-enable-remote-debugging"), None);
        assert_eq!(recorded_file(r"\Steam\file"), None);
        assert_eq!(recorded_file(r"C:file"), None);
        assert_eq!(recorded_file(""), None);
    }

    #[test]
    fn the_check_at_sign_in_is_one_run_value_per_family() {
        assert_eq!(
            run_value("Mujina_k2veznmcx4n98"),
            "MujinaCleanup.Mujina_k2veznmcx4n98"
        );
    }

    #[test]
    fn a_copy_leaves_its_folders_up_to_mujina_s() {
        let local = Path::new(r"C:\Users\A\AppData\Local");
        let copy = local.join(r"Mujina\Mujina_k2veznmcx4n98\mujina-setup.exe");
        assert_eq!(
            copy_folders(&copy),
            [
                local.join(r"Mujina\Mujina_k2veznmcx4n98").as_path(),
                local.join("Mujina").as_path()
            ]
        );
    }

    #[test]
    fn helper_programs_come_from_the_system_directory() {
        let powershell = powershell_program().unwrap();
        assert!(powershell.is_absolute());
        assert!(powershell.ends_with(r"WindowsPowerShell\v1.0\powershell.exe"));
    }

    #[test]
    fn the_work_folder_is_this_process_s_and_goes_with_it() {
        let folder = WorkFolder::new();
        let path = folder.unpack("test.msix", b"not a package").unwrap();
        assert!(path.is_file());
        assert!(
            path.to_string_lossy()
                .contains(&std::process::id().to_string())
        );
        drop(folder);
        assert!(!path.exists());
    }

    #[test]
    fn dropping_one_work_folder_leaves_another_of_the_same_process() {
        let one = WorkFolder::new();
        let other = WorkFolder::new();
        let kept = other.unpack("test.msix", b"not a package").unwrap();
        drop(one);
        assert!(kept.is_file());
    }

    /// Read-only: the test binary carries no package, so its host has no family and reports
    /// nothing installed and nothing carried.
    #[test]
    fn a_build_without_a_package_finds_nothing_of_its_own() {
        let host = WindowsHost::new(Some(std::env::temp_dir().join("mujina-setup-test.log")));
        let facts = host.facts();
        assert_eq!(facts.carried, None);
        assert_eq!(facts.installed, None);
        assert!(!facts.cert_trusted);
        assert!(
            facts.os_build.is_some_and(|build| build > 10000),
            "{facts:?}"
        );
        assert!(!host.installed());
        assert_eq!(host.prepare().unwrap_err().kind, ErrorKind::NoPackage);
        assert_eq!(host.remove().unwrap_err().kind, ErrorKind::NoPackage);
    }
}
