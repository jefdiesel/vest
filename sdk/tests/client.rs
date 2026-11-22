//! SDK tests against an in-memory backend.

use std::collections::BTreeMap;
use vesting_core::*;
use vesting_sdk::*;

#[derive(Default)]
struct MemoryBackend {
    now: Timestamp,
    next_id: ScheduleId,
    schedules: BTreeMap<ScheduleId, Schedule>,
    events: Vec<Event>,
    /// What `estimate_fee` answers (`None`: no fees, like a local ledger).
    fee: Option<FeeEstimate>,
    /// What the last `estimate_fee` was asked to simulate.
    estimated: Vec<(ScheduleId, Event, Option<[u8; 32]>)>,
    caller: Option<[u8; 32]>,
    target: Option<[u8; 32]>,
}

impl MemoryBackend {
    fn new(now: Timestamp) -> Self {
        Self {
            now,
            ..Self::default()
        }
    }
}

impl Backend for MemoryBackend {
    fn now(&self) -> Timestamp {
        self.now
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
    fn set_caller(&mut self, caller: [u8; 32]) {
        self.caller = Some(caller);
    }
    fn set_claim_target(&mut self, target: [u8; 32]) {
        self.target = Some(target);
    }
    fn estimate_fee(
        &mut self,
        id: ScheduleId,
        _state: &Schedule,
        event: &Event,
    ) -> Result<Option<FeeEstimate>, BackendError> {
        self.estimated.push((id, event.clone(), self.target.take()));
        Ok(self.fee.clone())
    }
}

const A: AccountId = [1u8; 32];
const B: AccountId = [2u8; 32];
const C: AccountId = [3u8; 32];

fn linear_params(total: Amount, start: Timestamp, end: Timestamp) -> CreateParams {
    CreateParams::linear(A, B, [9u8; 32], total, start, end)
}

#[test]
fn full_linear_lifecycle_through_sdk() {
    let mut client = Client::new(MemoryBackend::new(500));
    let (id, state, event) = client.create(linear_params(1_000, 0, 1_000)).unwrap();
    assert_eq!(id, 0);
    assert_eq!(state.total, 1_000);
    assert!(matches!(event, Event::Created { at: 500, .. }));

    assert_eq!(client.claimable(id).unwrap(), 500);
    let summary = client
        .pre_claim_summary(id, B, AccountRef::public(B))
        .unwrap();
    assert_eq!(summary.claimable, 500);
    // A local ledger charges no fee: the estimate is 0 and there is nothing to check.
    assert_eq!(summary.estimated_fee, 0);
    assert_eq!(summary.fee, None);
    assert_eq!(summary.fee_covered, None);
    assert_eq!(summary.destination_kind, AccountKind::Public);
    assert!(summary.next_unlock.is_some());

    let (next, amount, event) = client.claim_public(id, B).unwrap();
    assert_eq!(amount, 500);
    assert_eq!(next.claimed, 500);
    assert!(matches!(
        event,
        Event::Claimed {
            to_private: false,
            ..
        }
    ));

    let (cancelled, refund, _) = client.cancel(id, A).unwrap();
    assert_eq!(refund, 500);
    // Cancellation froze vested at 500, which was already claimed.
    let error = client.claim_public(id, B).unwrap_err();
    assert_eq!(
        error,
        SdkError::Core(VestingError::NothingClaimable {
            next_unlock: None,
            reason: WaitingFor::CancelledAndFullyClaimed,
        })
    );
    assert_eq!(client.show(id).unwrap(), cancelled);

    assert_eq!(client.list().len(), 1);
    assert_eq!(client.backend().events.len(), 3);
}

#[test]
fn claim_private_refuses_a_public_target() {
    let mut client = Client::new(MemoryBackend::new(500));
    let (id, _, _) = client.create(linear_params(1_000, 0, 1_000)).unwrap();

    let summary = client
        .pre_claim_summary(id, B, AccountRef::private(C))
        .unwrap();
    assert_eq!(summary.destination_kind, AccountKind::Private);

    let error = client
        .claim_private(id, B, AccountRef::public(C))
        .unwrap_err();
    assert_eq!(error.code(), 1001);
    assert!(error.message().contains("public account"));
    // Validation failed before any state change.
    assert_eq!(client.claimable(id).unwrap(), 500);
    assert_eq!(client.backend().events.len(), 1); // only the create event

    let (_, amount, event) = client.claim_private(id, B, AccountRef::private(C)).unwrap();
    assert_eq!(amount, 500);
    assert!(matches!(
        event,
        Event::Claimed {
            to_private: true,
            ..
        }
    ));
}

#[test]
fn milestone_lifecycle_through_sdk() {
    let mut client = Client::new(MemoryBackend::new(0));
    let params = CreateParams {
        kind: ScheduleKind::Milestone {
            tranches: vec![40, 60],
        },
        ..linear_params(100, 0, 1)
    };
    let (id, _, _) = client.create(params).unwrap();

    let error = client.claim_public(id, B).unwrap_err();
    assert_eq!(
        error,
        SdkError::Core(VestingError::NothingClaimable {
            next_unlock: None,
            reason: WaitingFor::Milestone,
        })
    );
    let summary = client
        .pre_claim_summary(id, B, AccountRef::public(B))
        .unwrap();
    assert_eq!(summary.claimable, 0);
    assert_eq!(summary.next_unlock, None);
    // Nothing claimable: nothing to simulate.
    assert!(client.backend().estimated.is_empty());

    let (_, amount, _) = client.signal_milestone(id, A, 0).unwrap();
    assert_eq!(amount, 40);
    assert_eq!(client.claim_public(id, B).unwrap().1, 40);
    assert_eq!(client.signal_milestone(id, A, 0).unwrap_err().code(), 18);
    assert_eq!(client.signal_milestone(id, A, 1).unwrap().1, 60);
    assert_eq!(client.claim_public(id, B).unwrap().1, 60);
}

#[test]
fn batch_create_irrevocable_and_transfer_through_sdk() {
    let mut client = Client::new(MemoryBackend::new(5));
    let params = vec![
        linear_params(10, 0, 10),
        CreateParams {
            transferable: Some(true),
            ..linear_params(20, 0, 10)
        },
    ];
    let created = client.create_batch(params).unwrap();
    assert_eq!(created.len(), 2);
    assert_eq!(client.list().len(), 2);

    let (_, _) = client.make_irrevocable(1, A).unwrap();
    assert_eq!(client.cancel(1, A).unwrap_err().code(), 14);

    assert_eq!(client.transfer_beneficiary(0, B, C).unwrap_err().code(), 20);
    let (_, event) = client.transfer_beneficiary(1, B, C).unwrap();
    assert!(matches!(event, Event::BeneficiaryTransferred { new, .. } if new == C));
    assert_eq!(client.claim_public(1, B).unwrap_err().code(), 10);
    assert_eq!(client.claim_public(1, C).unwrap().1, 10);
}

fn chain_fee(fee: Amount, available: Option<Amount>, payer: FeePayer) -> FeeEstimate {
    FeeEstimate {
        fee,
        reserve: fee * 2,
        payer,
        available,
        revert_reason: None,
    }
}

#[test]
fn the_summary_carries_the_simulated_fee_and_sends_nothing() {
    let mut backend = MemoryBackend::new(500);
    backend.fee = Some(chain_fee(1_234, Some(10_000), FeePayer::Caller));
    let mut client = Client::new(backend);
    let (id, _, _) = client.create(linear_params(1_000, 0, 1_000)).unwrap();

    let summary = client
        .pre_claim_summary(id, B, AccountRef::public(B))
        .unwrap();
    assert_eq!(summary.claimable, 500);
    assert_eq!(summary.estimated_fee, 1_234);
    assert_eq!(summary.fee_covered, Some(true));
    assert_eq!(summary.fee.as_ref().unwrap().payer, FeePayer::Caller);
    // The very claim was simulated: schedule 0, by B, for the 500 vested now, to a public account.
    let (sim_id, event, target) = client.backend().estimated[0].clone();
    assert_eq!(sim_id, id);
    assert_eq!(target, None);
    assert!(matches!(
        event,
        Event::Claimed {
            amount: 500,
            to_private: false,
            ..
        }
    ));
    // Nothing was stored or submitted.
    assert_eq!(client.backend().events.len(), 1);
    assert_eq!(client.claimable(id).unwrap(), 500);
}

#[test]
fn a_private_summary_simulates_the_private_claim_and_checks_the_payer() {
    let mut backend = MemoryBackend::new(500);
    // The private target would hold less than the fee: the check says so before any claim.
    backend.fee = Some(chain_fee(900, Some(899), FeePayer::ClaimTarget));
    let mut client = Client::new(backend);
    let (id, _, _) = client.create(linear_params(1_000, 0, 1_000)).unwrap();

    let summary = client
        .pre_claim_summary(id, B, AccountRef::private(C))
        .unwrap();
    assert_eq!(summary.estimated_fee, 900);
    assert_eq!(summary.fee_covered, Some(false));
    let (_, event, target) = client.backend().estimated[0].clone();
    assert_eq!(target, Some(C));
    assert!(matches!(
        event,
        Event::Claimed {
            to_private: true,
            ..
        }
    ));

    // A caller who may not claim gets the claim's own error, not a fee.
    let error = client
        .pre_claim_summary(id, C, AccountRef::public(C))
        .unwrap_err();
    assert_eq!(error.code(), 10);
}

#[test]
fn a_refused_claim_leaves_no_private_target_behind() {
    // Before the cliff nothing is claimable: the private claim is refused, and the backend must
    // not keep its target for the next (public) claim.
    let mut client = Client::new(MemoryBackend::new(100));
    let params = CreateParams {
        kind: ScheduleKind::CliffLinear {
            start: 0,
            cliff: 500,
            end: 1_000,
        },
        ..linear_params(1_000, 0, 1_000)
    };
    let (id, _, _) = client.create(params).unwrap();
    assert_eq!(
        client
            .claim_private(id, B, AccountRef::private(C))
            .unwrap_err()
            .code(),
        9
    );
    assert_eq!(client.backend().target, None);
    // A caller who may not claim: the summary refuses before naming a target.
    let mut backend = MemoryBackend::new(500);
    backend.fee = Some(chain_fee(1, Some(10), FeePayer::ClaimTarget));
    let mut client = Client::new(backend);
    let (id, _, _) = client.create(linear_params(1_000, 0, 1_000)).unwrap();
    assert_eq!(
        client
            .pre_claim_summary(id, C, AccountRef::private(C))
            .unwrap_err()
            .code(),
        10
    );
    assert_eq!(client.backend().target, None);
    // A valid private claim still names its target.
    client.claim_private(id, B, AccountRef::private(C)).unwrap();
    assert_eq!(client.backend().target, Some(C));
}

#[test]
fn a_public_payer_needs_the_reserve_a_private_one_the_fee() {
    // Public: admission holds the reserve (2 000 here), so 1 500 is not enough for a 1 000 fee.
    let mut backend = MemoryBackend::new(500);
    backend.fee = Some(chain_fee(1_000, Some(1_500), FeePayer::Caller));
    let mut client = Client::new(backend);
    let (id, _, _) = client.create(linear_params(1_000, 0, 1_000)).unwrap();
    let summary = client
        .pre_claim_summary(id, B, AccountRef::public(B))
        .unwrap();
    assert_eq!(summary.fee_covered, Some(false));
    assert_eq!(summary.fee.as_ref().unwrap().needed(), 2_000);
    // Private: LEZ clamps the reserve to the payer's balance; the fee is what must be there.
    let estimate = chain_fee(1_000, Some(1_500), FeePayer::ClaimTarget);
    assert_eq!(estimate.needed(), 1_000);
    assert_eq!(estimate.covered(), Some(true));
    assert_eq!(chain_fee(1_000, None, FeePayer::Caller).covered(), None);
}
