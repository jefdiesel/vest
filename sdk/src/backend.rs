//! The storage/chain boundary shared by the SDK and its consumers.
//!
//! A real LEZ backend implements this trait when the program exists. Until
//! then the CLI supplies a JSON-file implementation and tests supply an
//! in-memory one.

use vesting_core::{Event, Schedule, Timestamp};

/// Backend-local handle for a schedule. The chain backend will map this to a
/// program account address.
pub type ScheduleId = u64;

#[derive(Debug)]
pub enum BackendError {
    Io(String),
    Decode(String),
    NotFound(ScheduleId),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Io(message) => write!(f, "backend I/O error: {}", message),
            BackendError::Decode(message) => write!(f, "backend data is corrupt: {}", message),
            BackendError::NotFound(id) => write!(f, "no schedule with id {}", id),
        }
    }
}

impl std::error::Error for BackendError {}

/// Everything the SDK needs from a ledger. Kept deliberately small.
pub trait Backend {
    /// Current time in milliseconds, or an override for testing.
    fn now(&self) -> Timestamp;
    fn load(&self, id: ScheduleId) -> Result<Schedule, BackendError>;
    fn store(&mut self, id: ScheduleId, schedule: &Schedule) -> Result<(), BackendError>;
    fn next_id(&mut self) -> ScheduleId;
    fn list(&self) -> Vec<(ScheduleId, Schedule)>;
    /// Record an event from a successful state transition.
    fn submit(&mut self, event: &Event) -> Result<(), BackendError>;

    /// Destination account for the next claim. The LEZ backend needs it to build a one-time
    /// blinded alias; other backends ignore it. Account *kind* is never taken from the caller —
    /// LEZ asks the wallet. Defaults to a no-op so existing backends need no change.
    fn set_claim_target(&mut self, _target: [u8; 32]) {}

    /// The account acting in the next transition (the transaction's signer on LEZ). The client
    /// sets it before every operation; other backends ignore it.
    fn set_caller(&mut self, _caller: [u8; 32]) {}

    /// Store and submit several new schedules. LEZ sends them as one transaction (RFP
    /// Functionality 5); the default stores and submits them one by one.
    fn submit_batch(
        &mut self,
        items: &[(ScheduleId, Schedule, Event)],
    ) -> Result<(), BackendError> {
        for (id, state, event) in items {
            self.store(*id, state)?;
            self.submit(event)?;
        }
        Ok(())
    }
}
