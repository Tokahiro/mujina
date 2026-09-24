//! Which part Mujina plays in this process.

/// What the process is going to be; decides how much of a launcher integration is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Short-lived: bring the launcher up and leave.
    Home,
    /// Resident: additionally runs the launcher's background integrations.
    Agent,
    /// Setup and diagnosis.
    Tool,
}
