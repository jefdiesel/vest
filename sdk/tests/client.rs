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
    let summary = client.pre_claim_summary(id, AccountKind::Public).unwrap();
    assert_eq!(summary.claimable, 500);
    assert_eq!(summary.estimated_fee, ESTIMATED_FEE_PLACEHOLDER);
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

    let summary = client.pre_claim_summary(id, AccountKind::Private).unwrap();
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
    let summary = client.pre_claim_summary(id, AccountKind::Public).unwrap();
    assert_eq!(summary.claimable, 0);
    assert_eq!(summary.next_unlock, None);

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
