//! Reliability tests R1–R4 from the RFP-017 proposal.
//!
//! They drive the SDK's in-memory `Backend` through `Client`, the same shape as
//! `sdk/tests/client.rs`. The in-memory ledger stores schedules and events, so
//! "escrow balance" here means a schedule's unclaimed remainder, `total -
//! claimed`. The program's account derivation and instruction encoding are
//! tested in `program/tests/derivation.rs` (R3, second half).

use std::cell::Cell;
use std::collections::BTreeMap;

use vesting_core::*;
use vesting_sdk::*;

#[derive(Default)]
struct MemoryBackend {
    /// A `Cell` lets a test move the clock through the `&Backend` that
    /// `Client::backend()` hands back.
    now: Cell<Timestamp>,
    next_id: ScheduleId,
    schedules: BTreeMap<ScheduleId, Schedule>,
    events: Vec<Event>,
}

impl MemoryBackend {
    fn new(now: Timestamp) -> Self {
        Self {
            now: Cell::new(now),
            ..Self::default()
        }
    }

    fn set_now(&self, now: Timestamp) {
        self.now.set(now);
    }
}

impl Backend for MemoryBackend {
    fn now(&self) -> Timestamp {
        self.now.get()
    }
    fn load(&self, id: ScheduleId) -> Result<Schedule, BackendError> {
        self.schedules
            .get(&id)
            .cloned()
            .ok_or(BackendError::NotFound(id))
    }
    fn store(&mut self, id: ScheduleId, schedule: &Schedule) -> Result<(), BackendError> {
        self.schedules.insert(id, schedule.clone());
        Ok(())
    }
    fn next_id(&mut self) -> ScheduleId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    fn list(&self) -> Vec<(ScheduleId, Schedule)> {
        self.schedules
            .iter()
            .map(|(id, state)| (*id, state.clone()))
            .collect()
    }
    fn submit(&mut self, event: &Event) -> Result<(), BackendError> {
        self.events.push(event.clone());
        Ok(())
    }
}

const A: AccountId = [1u8; 32]; // creator and default cancel/milestone authority
const B: AccountId = [2u8; 32]; // beneficiary
const C: AccountId = [3u8; 32]; // another account
const D: AccountId = [4u8; 32]; // stranger
const TOKEN: [u8; 32] = [9u8; 32];

fn linear(total: Amount, start: Timestamp, end: Timestamp) -> CreateParams {
    CreateParams::linear(A, B, TOKEN, total, start, end)
}

fn milestone(tranches: Vec<Amount>) -> CreateParams {
    let total = tranches.iter().sum();
    CreateParams {
        kind: ScheduleKind::Milestone { tranches },
        ..linear(total, 0, 1)
    }
}

/// Tokens still held in escrow for a schedule: everything not yet paid out.
fn escrow(state: &Schedule) -> Amount {
    state.total - state.claimed
}

// ── R1 ──────────────────────────────────────────────────────────────────────────────

#[test]
fn r1_failed_or_refused_claim_changes_nothing_and_a_retry_succeeds() {
    let mut client = Client::new(MemoryBackend::new(0));
    let (id, created, _) = client.create(linear(1_000, 0, 1_000)).unwrap();
    let before = client.show(id).unwrap();
    let events_before = client.backend().events.len();
    assert_eq!(escrow(&before), 1_000);
    assert_eq!(before.claimed, 0);
    assert_eq!(events_before, 1); // only Created

    // Failed: nothing has vested yet at t = 0. Deterministic error, no state change.
    let failed = client.claim_public(id, B).unwrap_err();
    assert_eq!(failed.code(), 9);
    assert_eq!(
        failed,
        SdkError::Core(VestingError::NothingClaimable {
            next_unlock: Some(1),
            reason: WaitingFor::Accrual { at: Some(1) },
        })
    );
    assert_eq!(client.show(id).unwrap(), before);
    assert_eq!(client.backend().events.len(), events_before);

    // Refused (authorisation): a stranger is not the beneficiary.
    let refused = client.claim_public(id, D).unwrap_err();
    assert_eq!(refused, SdkError::Core(VestingError::NotBeneficiary));
    assert_eq!(refused.code(), 10);
    assert_eq!(client.show(id).unwrap(), before);
    assert_eq!(client.backend().events.len(), events_before);

    // Refused (SDK validation): a private claim with a public target.
    let refused = client
        .claim_private(id, B, AccountRef::public(C))
        .unwrap_err();
    assert_eq!(refused.code(), 1001);
    assert_eq!(client.show(id).unwrap(), before);
    assert_eq!(client.backend().events.len(), events_before);

    // Retry the same claim once something has vested: it succeeds, exactly once.
    client.backend().set_now(500);
    let (state, amount, event) = client.claim_public(id, B).unwrap();
    assert_eq!(amount, 500);
    assert_eq!(state.claimed, 500);
    assert_eq!(state.total, created.total);
    assert_eq!(escrow(&state), 500);
    assert_eq!(
        event,
        Event::Claimed {
            amount: 500,
            to_private: false,
            claimed_total: 500,
            at: 500,
        }
    );
    assert_eq!(client.backend().events.len(), events_before + 1);
}

// ── R2 ──────────────────────────────────────────────────────────────────────────────

#[test]
fn r2_cancel_is_all_or_nothing_and_refund_plus_vested_equals_escrow() {
    let mut client = Client::new(MemoryBackend::new(500));
    let (id, created, _) = client.create(linear(1_000, 0, 1_000)).unwrap();
    let before = client.show(id).unwrap();
    let events_before = client.backend().events.len();

    // A failed cancel (wrong caller) changes nothing.
    let refused = client.cancel(id, B).unwrap_err();
    assert_eq!(refused, SdkError::Core(VestingError::NotCancelAuthority));
    assert_eq!(refused.code(), 11);
    assert_eq!(client.show(id).unwrap(), before);
    assert_eq!(client.backend().events.len(), events_before);

    // A cancel on an irrevocable schedule is also refused with no change.
    let (irrevocable_id, _, _) = client
        .create(CreateParams {
            cancelable: Some(false),
            ..linear(500, 0, 1_000)
        })
        .unwrap();
    let irrevocable_before = client.show(irrevocable_id).unwrap();
    let irrevocable_events = client.backend().events.len();
    assert_eq!(client.cancel(irrevocable_id, A).unwrap_err().code(), 14);
    assert_eq!(client.show(irrevocable_id).unwrap(), irrevocable_before);
    assert_eq!(client.backend().events.len(), irrevocable_events);

    // A successful cancel: vested part + refund = the whole escrow.
    let (cancelled, refund, event) = client.cancel(id, A).unwrap();
    assert_eq!(
        event,
        Event::Cancelled {
            vested_at_cancel: 500,
            refund: 500,
            at: 500,
        }
    );
    assert_eq!(refund, 500);
    assert!(cancelled.cancelled.is_some());
    match event {
        Event::Cancelled {
            vested_at_cancel,
            refund,
            ..
        } => assert_eq!(vested_at_cancel + refund, created.total),
        _ => unreachable!(),
    }

    // Cancelling twice is refused and changes nothing further.
    let after_cancel = client.show(id).unwrap();
    let events_after_cancel = client.backend().events.len();
    assert_eq!(client.cancel(id, A).unwrap_err().code(), 15);
    assert_eq!(client.show(id).unwrap(), after_cancel);
    assert_eq!(client.backend().events.len(), events_after_cancel);

    // The beneficiary's vested part stays claimable. Refund + claimed = escrow.
    // (`escrow()` is total - claimed; it does not subtract the creator's refund,
    // so after the cancel it still includes the 500 the creator will get back.)
    let (claimed, amount, _) = client.claim_public(id, B).unwrap();
    assert_eq!(amount, 500);
    assert_eq!(claimed.claimed, 500);
    assert_eq!(claimed.claimed + refund, created.total);
    assert_eq!(client.claimable(id).unwrap(), 0); // beneficiary is fully paid
}

// ── R3 ──────────────────────────────────────────────────────────────────────────────

#[test]
fn r3_interleaved_claims_on_two_schedules_do_not_cross_effect() {
    let mut client = Client::new(MemoryBackend::new(250));
    let (id0, first, _) = client.create(linear(1_000, 0, 1_000)).unwrap();
    let (id1, second, _) = client.create(linear(2_000, 0, 1_000)).unwrap();
    assert_ne!(id0, id1);
    let mut state0 = client.show(id0).unwrap();
    let mut state1 = client.show(id1).unwrap();
    assert_eq!(state0, first);
    assert_eq!(state1, second);

    // Interleave claims. After each one the other schedule is unchanged.
    let (next, amount, _) = client.claim_public(id0, B).unwrap();
    assert_eq!(amount, 250);
    assert_eq!(client.show(id1).unwrap(), state1);
    state0 = next;

    let (next, amount, _) = client.claim_public(id1, B).unwrap();
    assert_eq!(amount, 500);
    assert_eq!(client.show(id0).unwrap(), state0);
    state1 = next;

    client.backend().set_now(500);
    let (next, amount, _) = client.claim_public(id0, B).unwrap();
    assert_eq!(amount, 250);
    assert_eq!(client.show(id1).unwrap(), state1);
    state0 = next;

    let (next, amount, _) = client.claim_public(id1, B).unwrap();
    assert_eq!(amount, 500);
    assert_eq!(client.show(id0).unwrap(), state0);
    state1 = next;

    client.backend().set_now(1_000);
    let (next, amount, _) = client.claim_public(id0, B).unwrap();
    assert_eq!(amount, 500);
    assert_eq!(client.show(id1).unwrap(), state1);
    state0 = next;

    let (next, amount, _) = client.claim_public(id1, B).unwrap();
    assert_eq!(amount, 1_000);
    assert_eq!(client.show(id0).unwrap(), state0);
    state1 = next;

    // Each schedule paid only from its own escrow.
    assert_eq!(state0.claimed, 1_000);
    assert_eq!(state1.claimed, 2_000);
    assert_eq!(state0.total, first.total);
    assert_eq!(state1.total, second.total);
    assert_eq!(state0.beneficiary, B);
    assert_eq!(state1.beneficiary, B);
    assert_eq!(escrow(&state0), 0);
    assert_eq!(escrow(&state1), 0);
    // Two Created and six Claimed events, and nothing else.
    assert_eq!(client.backend().events.len(), 8);
    assert_eq!(client.claim_public(id0, B).unwrap_err().code(), 9);
    assert_eq!(client.claim_public(id1, B).unwrap_err().code(), 9);
}

// `r3_schedule_derivation_never_collides_for_one_creator` lives in
// `program/tests/derivation.rs`: it tests the program's own derivation and encoders.

// ── R4 ──────────────────────────────────────────────────────────────────────────────

#[test]
fn r4_signalling_a_milestone_twice_is_a_deterministic_error_and_never_unlocks_twice() {
    let mut client = Client::new(MemoryBackend::new(0));
    let (id, _, _) = client.create(milestone(vec![40, 60])).unwrap();

    // The first signal unlocks exactly one tranche.
    let (once, amount, event) = client.signal_milestone(id, A, 0).unwrap();
    assert_eq!(amount, 40);
    assert_eq!(
        event,
        Event::MilestoneSignalled {
            index: 0,
            amount: 40,
            at: 0,
        }
    );
    assert_eq!(client.claimable(id).unwrap(), 40);

    // Repeating the signal on the same state gives the identical error each time.
    let first_error = signal_milestone(&once, A, 0, 0).unwrap_err();
    let second_error = signal_milestone(&once, A, 0, 0).unwrap_err();
    assert_eq!(first_error, second_error);
    assert_eq!(
        first_error,
        VestingError::MilestoneAlreadySignalled { index: 0 }
    );
    assert_eq!(first_error.code(), 18);
    assert!(first_error.message().contains("twice"));

    // Through the Client it stores nothing new: no event, no state change, no extra unlock.
    let before = client.show(id).unwrap();
    let events_before = client.backend().events.len();
    assert_eq!(client.signal_milestone(id, A, 0).unwrap_err().code(), 18);
    assert_eq!(client.show(id).unwrap(), before);
    assert_eq!(client.backend().events.len(), events_before);
    assert_eq!(client.claimable(id).unwrap(), 40); // still 40, not 80

    // The other tranche still signals; re-signalling index 0 is refused even then.
    let (both, amount, _) = client.signal_milestone(id, A, 1).unwrap();
    assert_eq!(amount, 60);
    assert_eq!(client.claimable(id).unwrap(), 100);
    assert_eq!(client.signal_milestone(id, A, 0).unwrap_err().code(), 18);
    assert_eq!(client.claimable(id).unwrap(), 100);
    assert_eq!(client.show(id).unwrap(), both);

    // One claim pays the sum once; any further claim has nothing left.
    let (claimed, amount, _) = client.claim_public(id, B).unwrap();
    assert_eq!(amount, 100);
    assert_eq!(claimed.claimed, 100);
    assert_eq!(escrow(&claimed), 0);
    assert_eq!(client.claim_public(id, B).unwrap_err().code(), 9);
}
