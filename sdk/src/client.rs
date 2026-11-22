//! Full lifecycle API for modules built on the vesting program.

use crate::backend::{Backend, FeeEstimate, ScheduleId};
use crate::error::SdkError;
use vesting_core::{
    cancel, claim, claimable, create, create_batch, make_irrevocable, next_unlock,
    signal_milestone, transfer_beneficiary, AccountId, Amount, CreateParams, Event, Schedule,
    Timestamp,
};

/// Account kinds, modelled locally until LEZ exposes them in a shared crate.
///
/// The kind a caller declares is only a first check: `claim_private` refuses a target declared
/// public before anything is sent, and `LezBackend` then asks the wallet whether the target is a
/// private account it holds, refusing otherwise. A flag cannot make an account private.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    Public,
    Private,
}

/// A destination account together with its declared kind (see [`AccountKind`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountRef {
    pub id: AccountId,
    pub kind: AccountKind,
}

impl AccountRef {
    pub fn public(id: AccountId) -> Self {
        Self {
            id,
            kind: AccountKind::Public,
        }
    }

    pub fn private(id: AccountId) -> Self {
        Self {
            id,
            kind: AccountKind::Private,
        }
    }
}

/// What a module shows the user before asking them to confirm a claim (RFP Usability 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreClaimSummary {
    pub schedule: ScheduleId,
    pub claimable: Amount,
    /// The simulated fee (native LEZ). 0 when nothing is claimable or the backend charges none.
    pub estimated_fee: Amount,
    /// The backend's simulation of this claim; `None` when nothing is claimable now or the
    /// backend charges no fees (a local ledger).
    pub fee: Option<FeeEstimate>,
    /// Whether the fee's payer can pay it, when the backend can tell (the private path's gas
    /// check): `Some(false)` means the claim would be refused for its fee.
    pub fee_covered: Option<bool>,
    pub next_unlock: Option<Timestamp>,
    pub destination_kind: AccountKind,
}

/// The pre-claim summary of schedule `id` for `caller` claiming to `destination` (the
/// beneficiary's own account for a public claim, a private account for a private one), at the
/// backend's time. When something is claimable it simulates the very claim transaction to get
/// its fee (`Backend::estimate_fee`); nothing is sent. A private destination declared public is
/// refused as `claim_private` refuses it.
pub fn pre_claim_summary<B: Backend>(
    backend: &mut B,
    id: ScheduleId,
    caller: AccountId,
    destination: AccountRef,
) -> Result<PreClaimSummary, SdkError> {
    let now = backend.now();
    let state = backend
        .load(id)
        .map_err(|error| SdkError::Backend(error.to_string()))?;
    let claimable_now = state.claimable_at(now);
    let to_private = destination.kind == AccountKind::Private;
    let mut fee = None;
    if claimable_now > 0 {
        // The claim the client would send, as `Client::claim` builds it. A claim the rules refuse
        // (a caller who is not the beneficiary) is the same error the claim itself would give.
        let (next, _amount, event) = claim(&state, caller, now, to_private)?;
        // Only once the claim is valid: a target left behind by a refused claim would steer the
        // backend's next claim.
        if to_private {
            backend.set_claim_target(destination.id);
        }
        backend.set_caller(caller);
        fee = backend
            .estimate_fee(id, &next, &event)
            .map_err(|error| SdkError::Backend(error.to_string()))?;
    }
    let fee_covered = fee.as_ref().and_then(FeeEstimate::covered);
    Ok(PreClaimSummary {
        schedule: id,
        claimable: claimable_now,
        estimated_fee: fee.as_ref().map_or(0, |estimate| estimate.fee),
        fee,
        fee_covered,
        next_unlock: next_unlock(&state, now),
        destination_kind: destination.kind,
    })
}

pub struct Client<B: Backend> {
    backend: B,
}

impl<B: Backend> Client<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn now(&self) -> Timestamp {
        self.backend.now()
    }

    fn schedule(&self, id: ScheduleId) -> Result<Schedule, SdkError> {
        self.backend
            .load(id)
            .map_err(|error| SdkError::Backend(error.to_string()))
    }

    fn record(&mut self, id: ScheduleId, state: &Schedule, event: Event) -> Result<(), SdkError> {
        self.backend
            .store(id, state)
            .map_err(|error| SdkError::Backend(error.to_string()))?;
        self.backend
            .submit(&event)
            .map_err(|error| SdkError::Backend(error.to_string()))
    }

    /// Create one schedule (RFP Usability 1).
    pub fn create(
        &mut self,
        params: CreateParams,
    ) -> Result<(ScheduleId, Schedule, Event), SdkError> {
        let (state, event) = create(params, self.now())?;
        let id = self.backend.next_id();
        self.backend.set_caller(state.creator);
        self.record(id, &state, event.clone())?;
        Ok((id, state, event))
    }

    /// Create a batch of schedules (RFP Functionality 5).
    pub fn create_batch(
        &mut self,
        params: Vec<CreateParams>,
    ) -> Result<Vec<(ScheduleId, Schedule, Event)>, SdkError> {
        let (states, events) = create_batch(params, self.now())?;
        let mut created = Vec::with_capacity(states.len());
        for (state, event) in states.into_iter().zip(events) {
            let id = self.backend.next_id();
            created.push((id, state, event));
        }
        if let Some((_, first, _)) = created.first() {
            self.backend.set_caller(first.creator);
        }
        self.backend
            .submit_batch(&created)
            .map_err(|error| SdkError::Backend(error.to_string()))?;
        Ok(created)
    }

    pub fn show(&self, id: ScheduleId) -> Result<Schedule, SdkError> {
        self.schedule(id)
    }

    pub fn list(&self) -> Vec<(ScheduleId, Schedule)> {
        self.backend.list()
    }

    pub fn claimable(&self, id: ScheduleId) -> Result<Amount, SdkError> {
        Ok(claimable(&self.schedule(id)?, self.now()))
    }

    /// Pre-claim summary with the simulated fee (RFP Usability 4); see [`pre_claim_summary`].
    /// A public claim always pays the beneficiary: pass `AccountRef::public(beneficiary)`.
    pub fn pre_claim_summary(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
        destination: AccountRef,
    ) -> Result<PreClaimSummary, SdkError> {
        pre_claim_summary(&mut self.backend, id, caller, destination)
    }

    /// The backend, mutably (backend-specific calls: import, balances, ...).
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Claim to whichever destination the core event flag describes.
    pub fn claim(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
        to_private: bool,
    ) -> Result<(Schedule, Amount, Event), SdkError> {
        self.claim_to(id, caller, to_private, None)
    }

    /// `claim`, telling the backend the private `target` only once the claim is valid (a target
    /// left behind by a refused claim must never steer a later one).
    fn claim_to(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
        to_private: bool,
        target: Option<AccountId>,
    ) -> Result<(Schedule, Amount, Event), SdkError> {
        let state = self.schedule(id)?;
        let (next, _local, event) = claim(&state, caller, self.now(), to_private)?;
        self.backend.set_caller(caller);
        if let Some(target) = target {
            self.backend.set_claim_target(target);
        }
        self.record(id, &next, event.clone())?;
        // What the ledger did, which on LEZ is what the chain paid.
        let after = self.schedule(id)?;
        let amount = after.claimed.saturating_sub(state.claimed);
        Ok((after, amount, event))
    }

    /// Claim to a public account.
    pub fn claim_public(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
    ) -> Result<(Schedule, Amount, Event), SdkError> {
        self.claim(id, caller, false)
    }

    /// Claim to a private account, refusing a target that is not private
    /// (RFP Privacy 3). Nothing is recorded when validation fails.
    pub fn claim_private(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
        target: AccountRef,
    ) -> Result<(Schedule, Amount, Event), SdkError> {
        if target.kind != AccountKind::Private {
            return Err(SdkError::TargetNotPrivate { account: target.id });
        }
        // The LEZ backend sends to a one-time alias of this account (and checks with the wallet
        // that it is private); other backends ignore the target.
        self.claim_to(id, caller, true, Some(target.id))
    }

    pub fn cancel(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
    ) -> Result<(Schedule, Amount, Event), SdkError> {
        let state = self.schedule(id)?;
        let (next, _local, event) = cancel(&state, caller, self.now())?;
        self.backend.set_caller(caller);
        self.record(id, &next, event.clone())?;
        let after = self.schedule(id)?;
        let refund = after.cancelled.as_ref().map_or(0, |record| {
            state.total.saturating_sub(record.vested_at_cancel)
        });
        Ok((after, refund, event))
    }

    pub fn make_irrevocable(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
    ) -> Result<(Schedule, Event), SdkError> {
        let state = self.schedule(id)?;
        let (next, event) = make_irrevocable(&state, caller, self.now())?;
        self.backend.set_caller(caller);
        self.record(id, &next, event.clone())?;
        Ok((next, event))
    }

    pub fn signal_milestone(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
        index: usize,
    ) -> Result<(Schedule, Amount, Event), SdkError> {
        let state = self.schedule(id)?;
        let (next, amount, event) = signal_milestone(&state, caller, index, self.now())?;
        self.backend.set_caller(caller);
        self.record(id, &next, event.clone())?;
        Ok((next, amount, event))
    }

    pub fn transfer_beneficiary(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
        new_beneficiary: AccountId,
    ) -> Result<(Schedule, Event), SdkError> {
        let state = self.schedule(id)?;
        let (next, event) = transfer_beneficiary(&state, caller, new_beneficiary, self.now())?;
        self.backend.set_caller(caller);
        self.record(id, &next, event.clone())?;
        Ok((next, event))
    }
}
