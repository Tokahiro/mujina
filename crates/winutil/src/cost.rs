//! What this process has cost so far.

use std::time::Duration;

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// CPU time and memory of the current process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessCost {
    /// Kernel plus user time of all threads, living and dead.
    pub cpu: Duration,
    pub peak_working_set_bytes: u64,
}

fn duration(time: FILETIME) -> Duration {
    // A FILETIME counts 100-nanosecond intervals.
    let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Duration::from_nanos(ticks.saturating_mul(100))
}

/// `None` only if Windows refuses the queries, which it has no reason to for the own process.
pub fn of_this_process() -> Option<ProcessCost> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: the pseudo-handle of the current process is always valid; all four out parameters
    // are writable.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    };
    if ok == 0 {
        return None;
    }

    // SAFETY: PROCESS_MEMORY_COUNTERS is plain data for which all-zero is a valid value.
    let mut memory: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    memory.cb = size;
    // SAFETY: valid pseudo-handle; `memory` is writable for the stated size.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut memory, size) };
    if ok == 0 {
        return None;
    }

    Some(ProcessCost {
        cpu: duration(kernel) + duration(user),
        peak_working_set_bytes: memory.PeakWorkingSetSize as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_test_has_cost_something() {
        let cost = of_this_process().unwrap();
        assert!(cost.peak_working_set_bytes > 0);
    }

    #[test]
    fn filetime_is_hundreds_of_nanoseconds() {
        let one_second = FILETIME {
            dwLowDateTime: 10_000_000,
            dwHighDateTime: 0,
        };
        assert_eq!(duration(one_second), Duration::from_secs(1));
    }
}
