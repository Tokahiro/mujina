/// The part this process plays; decides how much of a launcher integration is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Short-lived: bring the launcher up and leave.
    Home,
    /// Resident: additionally runs the launcher's background integrations.
    Agent,
    /// Setup and diagnosis.
    Tool,
}
