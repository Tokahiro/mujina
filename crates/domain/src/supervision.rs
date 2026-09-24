//! What to do when the launcher goes away while the console experience is still on.

/// User-selectable reaction to the launcher exiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExitPolicy {
    #[default]
    RelaunchOnCrash,
    RelaunchAlways,
    /// Leave it to the user.
    Nothing,
}

impl ExitPolicy {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "relaunch_on_crash" => Some(Self::RelaunchOnCrash),
            "relaunch_always" => Some(Self::RelaunchAlways),
            "nothing" => Some(Self::Nothing),
            _ => None,
        }
    }
}

/// A launcher that dies sooner than this after watching began is not relaunched: it would only
/// loop.
pub const MIN_UPTIME_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherExit {
    /// `None` when the exit code could not be read.
    pub exit_code: Option<u32>,
    /// Since watching began, not since the process started.
    pub uptime_secs: u64,
}

pub const fn should_relaunch(
    policy: ExitPolicy,
    exit: LauncherExit,
    full_screen_active: bool,
) -> bool {
    if !full_screen_active || exit.uptime_secs < MIN_UPTIME_SECS {
        return false;
    }
    match policy {
        ExitPolicy::RelaunchAlways => true,
        ExitPolicy::RelaunchOnCrash => !matches!(exit.exit_code, Some(0)),
        ExitPolicy::Nothing => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn exit(exit_code: Option<u32>, uptime_secs: u64) -> LauncherExit {
        LauncherExit {
            exit_code,
            uptime_secs,
        }
    }

    #[test]
    fn never_relaunches_on_the_desktop() {
        assert!(!should_relaunch(
            ExitPolicy::RelaunchAlways,
            exit(Some(1), 600),
            false
        ));
    }

    #[test]
    fn crash_policy_distinguishes_clean_exits() {
        assert!(!should_relaunch(
            ExitPolicy::RelaunchOnCrash,
            exit(Some(0), 600),
            true
        ));
        assert!(should_relaunch(
            ExitPolicy::RelaunchOnCrash,
            exit(Some(0xC000_0005), 600),
            true
        ));
        assert!(should_relaunch(
            ExitPolicy::RelaunchOnCrash,
            exit(None, 600),
            true
        ));
    }

    #[test]
    fn a_launcher_that_dies_at_once_is_not_restarted_in_a_loop() {
        assert!(!should_relaunch(
            ExitPolicy::RelaunchAlways,
            exit(Some(1), 5),
            true
        ));
        assert!(should_relaunch(
            ExitPolicy::RelaunchAlways,
            exit(Some(1), MIN_UPTIME_SECS),
            true
        ));
    }

    #[test]
    fn explicit_policies() {
        assert!(should_relaunch(
            ExitPolicy::RelaunchAlways,
            exit(Some(0), 600),
            true
        ));
        assert!(!should_relaunch(
            ExitPolicy::Nothing,
            exit(Some(1), 600),
            true
        ));
    }
}
