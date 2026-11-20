//! Full lifecycle API for modules built on the vesting program.

use crate::backend::{Backend, ScheduleId};
use crate::error::SdkError;
use vesting_core::{
    cancel, claim, claimable, create, create_batch, make_irrevocable, next_unlock,
    signal_milestone, transfer_beneficiary, AccountId, Amount, CreateParams, Event, Schedule,
    Timestamp,
};

/// Placeholder until the program publishes a real fee estimate (RFP Usability 4).
pub const ESTIMATED_FEE_PLACEHOLDER: Amount = 0;

/// Account kinds, modelled locally until LEZ exposes them in a shared crate.
///
/// Important: for now the caller declares the kind, so `claim_private` only
/// checks what the caller says. When the real LEZ backend lands, the kind must
/// come from the wallet (a private account the wallet actually holds), not
/// from a flag supplied by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    Public,
    Private,
}

/// A destination account together with its kind. As with [`AccountKind`], the
/// kind is caller-supplied today and must come from the wallet once the LEZ
/// backend is real.
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

/// What a module shows the user before asking them to confirm a claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreClaimSummary {
    pub schedule: ScheduleId,
    pub claimable: Amount,
    pub estimated_fee: Amount,
    pub next_unlock: Option<Timestamp>,
    pub destination_kind: AccountKind,
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

    /// Pre-claim summary for either destination kind (RFP Usability 4).
    pub fn pre_claim_summary(
        &self,
        id: ScheduleId,
        destination_kind: AccountKind,
    ) -> Result<PreClaimSummary, SdkError> {
        let state = self.schedule(id)?;
        Ok(PreClaimSummary {
            schedule: id,
            claimable: state.claimable_at(self.now()),
            estimated_fee: ESTIMATED_FEE_PLACEHOLDER,
            next_unlock: next_unlock(&state, self.now()),
            destination_kind,
        })
    }

    /// Claim to whichever destination the core event flag describes.
    pub fn claim(
        &mut self,
        id: ScheduleId,
        caller: AccountId,
        to_private: bool,
    ) -> Result<(Schedule, Amount, Event), SdkError> {
        let state = self.schedule(id)?;
        let (next, _local, event) = claim(&state, caller, self.now(), to_private)?;
        self.backend.set_caller(caller);
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
        self.backend.set_claim_target(target.id);
        self.claim(id, caller, true)
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
