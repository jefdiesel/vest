//! The storage/chain boundary shared by the SDK and its consumers.
//!
//! `LezBackend` (feature `lez`) implements it on the chain; the CLI and the Basecamp module's
//! tests supply a JSON-file implementation, and the SDK's tests an in-memory one.

use vesting_core::{Amount, Event, Schedule, Timestamp};

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

/// Who pays a transaction's fee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeePayer {
    /// The signing caller's public account (every public transaction).
    Caller,
    /// The private destination of a private claim: a native claim pays from what it receives,
    /// a token claim from the native LEZ its schedule's fee float sends with it.
    ClaimTarget,
}

/// What a transition would cost on chain, from a simulation that sends nothing (the wallet's
/// dry run, as `vesting --dry-run` prints it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeeEstimate {
    /// The fee at the chain's current base fees, in native LEZ (smallest unit).
    pub fee: Amount,
    /// What the payer must be able to pay for the transaction to be admitted (at least `fee`;
    /// the surplus of a public reserve is not charged).
    pub reserve: Amount,
    pub payer: FeePayer,
    /// Native LEZ the payer will have to pay with, when the backend can tell: a public caller's
    /// balance; for a private claim, the target's balance plus what the claim itself brings
    /// (the claimed native LEZ, or a token schedule's per-claim float).
    pub available: Option<Amount>,
    /// Why the simulated action would revert (a reverted public action still pays its fee).
    pub revert_reason: Option<String>,
}

impl FeeEstimate {
    /// What the payer must have for the transaction to be admitted: a public payer the reserve
    /// (admission holds the fee at the gas limit, at least `fee`); a private claim's payer the
    /// fee (LEZ clamps a private reserve to the payer's balance).
    pub fn needed(&self) -> Amount {
        match self.payer {
            FeePayer::Caller => self.reserve.max(self.fee),
            FeePayer::ClaimTarget => self.fee,
        }
    }

    /// Whether the payer can pay, when the backend can tell.
    pub fn covered(&self) -> Option<bool> {
        self.available.map(|have| have >= self.needed())
    }
}

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

    /// Simulate the transaction that `submit` would send for `event` (the new `state` of schedule
    /// `id`, signed by the caller last given to `set_caller`) and report its fee, sending nothing.
    /// `Ok(None)`: this backend charges no fees (a local ledger).
    fn estimate_fee(
        &mut self,
        _id: ScheduleId,
        _state: &Schedule,
        _event: &Event,
    ) -> Result<Option<FeeEstimate>, BackendError> {
        Ok(None)
    }

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
