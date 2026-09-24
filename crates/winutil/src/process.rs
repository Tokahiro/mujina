//! Facts about other processes, obtained with the least access right that works.

use std::ffi::c_void;
use std::ops::ControlFlow;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::Packaging::Appx::GetPackageFamilyName;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess, OpenProcessToken,
    PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY, PROCESS_ACCESS_RIGHTS, PROCESS_INFORMATION,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    QueryFullProcessImageNameW, STARTUPINFOEXW, UpdateProcThreadAttribute,
};

use crate::error::Win32Error;
use crate::wide::{from_wide, to_wide};

const STILL_ACTIVE: u32 = 259;

fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedHandle> {
    // SAFETY: plain call; failure is reported as a null handle.
    let handle = unsafe { OpenProcess(access, 0, pid) };
    // SAFETY: a new handle of our own, which CloseHandle closes.
    (!handle.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(handle) })
}

pub fn is_elevated() -> bool {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo handle of the current process needs no closing; `token` is writable.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return false;
    }
    // SAFETY: on success `token` is a new handle of our own, which CloseHandle closes.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut length: u32 = 0;
    // SAFETY: valid token with query access; `elevation` is writable for the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenElevation,
            (&raw mut elevation).cast(),
            u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or(0),
            &raw mut length,
        )
    };
    ok != 0 && elevation.TokenIsElevated != 0
}

pub fn is_running(pid: u32) -> bool {
    let Some(process) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
        return false;
    };
    let mut code: u32 = 0;
    // SAFETY: valid process handle with query access; `code` is writable.
    let ok = unsafe { GetExitCodeProcess(process.as_raw_handle(), &raw mut code) };
    ok != 0 && code == STILL_ACTIVE
}

/// Whether the process has a package identity; `false` also when it cannot be opened.
pub fn is_packaged(pid: u32) -> bool {
    let Some(process) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
        return false;
    };
    let mut length: u32 = 0;
    // SAFETY: valid handle with query access; a null buffer with length 0 asks for the length,
    // which only a packaged process has.
    let status =
        unsafe { GetPackageFamilyName(process.as_raw_handle(), &raw mut length, null_mut()) };
    status == ERROR_INSUFFICIENT_BUFFER
}

pub fn image_name(pid: u32) -> Option<String> {
    image_name_of(&open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?)
}

/// Needs only `PROCESS_QUERY_LIMITED_INFORMATION`, not access to the process's memory.
pub fn image_path(pid: u32) -> Option<String> {
    image_path_of(&open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?)
}

/// Processes whose image path `matches`. Opens every process to read its path: for a button
/// press, not for polling. Processes that cannot be opened are left out.
pub fn running_from(mut matches: impl FnMut(&str) -> bool) -> Vec<u32> {
    let mut ids = Vec::new();
    walk_processes(|entry| {
        ids.push(entry.th32ProcessID);
        ControlFlow::Continue(())
    });
    ids.retain(|&pid| pid != 0 && image_path(pid).is_some_and(|path| matches(&path)));
    ids
}

fn image_name_of(process: &OwnedHandle) -> Option<String> {
    let path = image_path_of(process)?;
    path.rsplit(['\\', '/']).next().map(str::to_string)
}

/// `process` needs `PROCESS_QUERY_LIMITED_INFORMATION` or more.
fn image_path_of(process: &OwnedHandle) -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut length = u32::try_from(buffer.len()).ok()?;
    // SAFETY: valid process handle with query access; `buffer` is writable for `length` units.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process.as_raw_handle(),
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &raw mut length,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(from_wide(&buffer[..length as usize]))
}

/// Calls `visit` for each process until it breaks; for none if Windows takes no snapshot.
fn walk_processes(mut visit: impl FnMut(&PROCESSENTRY32W) -> ControlFlow<()>) {
    // SAFETY: plain call; failure is reported as INVALID_HANDLE_VALUE.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return;
    }
    // SAFETY: a new handle of our own, which CloseHandle closes.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    // SAFETY: PROCESSENTRY32W is plain data for which all-zero is a valid value.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(0);

    // SAFETY: valid snapshot handle; `entry` is writable and carries its own size.
    let mut more = unsafe { Process32FirstW(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    while more && visit(&entry).is_continue() {
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot.as_raw_handle(), &raw mut entry) } != 0;
    }
}

/// Lower-cased. For occasional diagnosis, not for polling.
pub fn running_image_names() -> Vec<String> {
    let mut names = Vec::new();
    walk_processes(|entry| {
        names.push(from_wide(&entry.szExeFile).to_lowercase());
        ControlFlow::Continue(())
    });
    names
}

/// Compares the image file name without case. For occasional lookup, not for polling.
pub fn pid_of_image(name: &str) -> Option<u32> {
    let mut found = None;
    walk_processes(|entry| {
        if from_wide(&entry.szExeFile).eq_ignore_ascii_case(name) {
            found = Some(entry.th32ProcessID);
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    found
}

/// A handle on another process that becomes signalled when the process exits.
#[derive(Debug)]
pub struct ProcessWatch {
    handle: OwnedHandle,
    pid: u32,
    since: Instant,
}

impl ProcessWatch {
    pub fn open(pid: u32) -> Option<Self> {
        let handle = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)?;
        Some(Self {
            handle,
            pid,
            since: Instant::now(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The raw handle, for wait functions that take an array of them. Valid while `self` is.
    pub fn as_raw(&self) -> *mut c_void {
        self.handle.as_raw_handle()
    }

    pub fn observed_for(&self) -> Duration {
        self.since.elapsed()
    }

    /// Still the watched process's name after it ended, as the handle keeps the id from reuse.
    pub fn image_name(&self) -> Option<String> {
        image_name_of(&self.handle)
    }

    pub fn exit_code(&self) -> Option<u32> {
        let mut code: u32 = 0;
        // SAFETY: valid process handle with query access; `code` is writable.
        let ok = unsafe { GetExitCodeProcess(self.as_raw(), &raw mut code) };
        (ok != 0 && code != STILL_ACTIVE).then_some(code)
    }
}

impl AsHandle for ProcessWatch {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

/// `PROCESS_CREATION_DESKTOP_APP_BREAKAWAY_ENABLE_PROCESS_TREE`: the child's own children start
/// outside the desktop app runtime (see UpdateProcThreadAttribute).
const DESKTOP_APP_BREAKAWAY_ENABLE_PROCESS_TREE: u32 = 0x01;

/// Starts `program` without waiting, so that what it starts is outside this process's package
/// and survives the package's removal. From a package only the child's children break away: a
/// child that [`crate::package::family_name`] still finds packaged should restart itself.
/// `arguments` must be plain words (no space or quote). The child starts in the system directory.
pub fn spawn_outside_package(program: &Path, arguments: &[&str]) -> Result<(), Win32Error> {
    const INVALID_PARAMETER: Win32Error = Win32Error {
        call: "CreateProcessW",
        code: ERROR_INVALID_PARAMETER,
    };
    let plain = |word: &&str| !word.is_empty() && !word.contains([' ', '\t', '"']);
    if !arguments.iter().all(plain) || program.to_string_lossy().contains('"') {
        return Err(INVALID_PARAMETER);
    }
    let mut command_line = format!("\"{}\"", program.display());
    for argument in arguments {
        command_line.push(' ');
        command_line.push_str(argument);
    }
    let mut command_line = to_wide(&command_line);
    let application = to_wide(&program.to_string_lossy());
    let directory = crate::library::system_directory()
        .map(|path| to_wide(&path.to_string_lossy()))
        .ok_or(INVALID_PARAMETER)?;

    let attributes = crate::package::family_name()
        .is_some()
        .then(AttributeList::desktop_app_breakaway)
        .transpose()?;

    // SAFETY: STARTUPINFOEXW is plain data for which all-zero is a valid value.
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or(0);
    let mut flags = CREATE_UNICODE_ENVIRONMENT;
    if let Some(list) = &attributes {
        startup.lpAttributeList = list.as_raw();
        flags |= EXTENDED_STARTUPINFO_PRESENT;
    }
    // SAFETY: PROCESS_INFORMATION is plain data for which all-zero is a valid value.
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: NUL-terminated strings outlive the call; the command line is writable, as required;
    // `startup` has its size and, when flagged, an initialised list that outlives the call.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            0,
            flags,
            null(),
            directory.as_ptr(),
            (&raw const startup).cast(),
            &raw mut process,
        )
    };
    if created == 0 {
        return Err(crate::error::last_error("CreateProcessW"));
    }
    // SAFETY: both handles are new and ours; nothing here waits for the child.
    unsafe {
        drop(OwnedHandle::from_raw_handle(process.hThread));
        drop(OwnedHandle::from_raw_handle(process.hProcess));
    }
    Ok(())
}

struct AttributeList {
    /// The list's memory, in pointer-sized units so that it is aligned for it.
    buffer: Vec<usize>,
    /// The attribute's value, which must outlive the list (UpdateProcThreadAttribute).
    _policy: Box<u32>,
}

impl AttributeList {
    fn desktop_app_breakaway() -> Result<Self, Win32Error> {
        let mut size: usize = 0;
        // SAFETY: a null list asks for the size; it then fails with ERROR_INSUFFICIENT_BUFFER.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &raw mut size) };
        let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>()).max(1)];
        // SAFETY: `buffer` is writable for at least `size` bytes, the size just asked for.
        let initialised = unsafe {
            InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), 1, 0, &raw mut size)
        };
        if initialised == 0 {
            return Err(crate::error::last_error(
                "InitializeProcThreadAttributeList",
            ));
        }
        let policy = Box::new(DESKTOP_APP_BREAKAWAY_ENABLE_PROCESS_TREE);
        // SAFETY: initialised list; the value is a DWORD, boxed so it lives as long as the list.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                buffer.as_mut_ptr().cast(),
                0,
                PROC_THREAD_ATTRIBUTE_DESKTOP_APP_POLICY as usize,
                (&raw const *policy).cast(),
                size_of::<u32>(),
                null_mut(),
                null(),
            )
        };
        let list = Self {
            buffer,
            _policy: policy,
        };
        if updated == 0 {
            return Err(crate::error::last_error("UpdateProcThreadAttribute"));
        }
        Ok(list)
    }

    fn as_raw(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_ptr().cast_mut().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: the list was initialised and is deleted exactly once, before its memory goes.
        unsafe { DeleteProcThreadAttributeList(self.as_raw()) };
    }
}

/// Every running process as `(process id, parent process id)`.
fn parentage() -> Vec<(u32, u32)> {
    let mut pairs = Vec::new();
    walk_processes(|entry| {
        pairs.push((entry.th32ProcessID, entry.th32ParentProcessID));
        ControlFlow::Continue(())
    });
    pairs
}

/// Excludes `root`; a chain ends where a process in between has exited. Not for polling.
pub fn descendants(root: u32) -> Vec<u32> {
    descendants_in(&parentage(), root)
}

fn descendants_in(pairs: &[(u32, u32)], root: u32) -> Vec<u32> {
    let mut found: Vec<u32> = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for &(pid, its_parent) in pairs {
            // Process ids are reused, so a stale parent id may close a loop.
            if its_parent == parent && pid != root && !found.contains(&pid) {
                found.push(pid);
                frontier.push(pid);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendants_follow_the_chain_and_survive_loops() {
        let pairs = [(2, 1), (3, 2), (4, 3), (5, 9), (1, 4)];
        let mut found = descendants_in(&pairs, 1);
        found.sort_unstable();
        assert_eq!(found, [2, 3, 4]);
        assert!(descendants_in(&pairs, 7).is_empty());
    }

    #[test]
    fn sees_the_current_process() {
        let pid = std::process::id();
        assert!(is_running(pid));
        let name = image_name(pid).unwrap();
        assert!(name.to_ascii_lowercase().ends_with(".exe"), "{name}");
        let watch = ProcessWatch::open(pid).unwrap();
        assert_eq!(
            watch.image_name(),
            Some(name),
            "the same, through the watch"
        );
    }

    #[test]
    fn the_process_list_contains_this_process() {
        let own = image_name(std::process::id()).unwrap().to_lowercase();
        assert!(running_image_names().contains(&own), "{own}");
    }

    #[test]
    fn only_plain_words_are_passed_on() {
        let program = Path::new(r"C:\no\such\program.exe");
        for arguments in [&["--log", "a b"][..], &["\"x\""], &[""]] {
            assert_eq!(
                spawn_outside_package(program, arguments).unwrap_err().code,
                ERROR_INVALID_PARAMETER,
                "{arguments:?}"
            );
        }
        // Plain words get as far as Windows, which finds no such program.
        assert_ne!(
            spawn_outside_package(program, &["--uninstall"])
                .unwrap_err()
                .code,
            ERROR_INVALID_PARAMETER
        );
    }

    #[test]
    fn pid_zero_is_not_a_running_process() {
        assert!(!is_running(0));
    }

    #[test]
    fn processes_are_found_by_where_their_program_lies() {
        let pid = std::process::id();
        let path = image_path(pid).unwrap();
        assert!(path.ends_with(&image_name(pid).unwrap()), "{path}");
        let folder = &path[..path.rfind('\\').unwrap()];
        let found = running_from(|image| image.starts_with(folder));
        assert!(found.contains(&pid), "{found:?}");
        assert!(running_from(|_| false).is_empty());
    }

    #[test]
    fn a_test_process_is_no_packaged_app() {
        assert!(!is_packaged(std::process::id()));
        assert!(!is_packaged(0), "not opened, so not known to be one");
    }

    #[test]
    fn a_walk_stops_where_it_is_told() {
        let mut all = 0;
        walk_processes(|_| {
            all += 1;
            ControlFlow::Continue(())
        });
        let mut until_break = 0;
        walk_processes(|_| {
            until_break += 1;
            ControlFlow::Break(())
        });
        assert!(all > 1, "{all}");
        assert_eq!(until_break, 1);

        let own = std::process::id();
        assert!(parentage().iter().any(|&(pid, _)| pid == own));
        let name = image_name(own).unwrap();
        // Another process with the same image may come first, e.g. a second test binary.
        assert!(pid_of_image(&name).is_some_and(is_running), "{name}");
    }

    #[test]
    fn a_watched_running_process_has_no_exit_code() {
        let watch = ProcessWatch::open(std::process::id()).unwrap();
        assert_eq!(watch.exit_code(), None);
        assert_eq!(watch.as_handle().as_raw_handle(), watch.as_raw());
        assert!(ProcessWatch::open(0).is_none());
    }
}
