//! Every state transition emits exactly one event, from the schedule actor, whose selector names it
//! and whose data is the `vesting-core` event (RFP Functionality 6). The handler runs natively here,
//! message by message as the executor would deliver them (the clock's replies included).
//!
//! Why not read them back from a chain: an rc3 sequencer drops the events of the blocks it produces
//! (`lez/sequencer/core/src/lib.rs` at bafd07be: `Ok((_events, tx_reverted))` in the produce path,
//! `events: Vec::new()` in its storage update) and stores a block's events when the block comes
//! back through the Bedrock channel. A standalone sequencer's Bedrock is a mock, so only genesis
//! events are stored (observed in `e2e/tests/chain.rs`: none past block 1 over 135 blocks).

use borsh::BorshDeserialize;
use lee_core::{
    account::{AccountId, Actor, ActorState},
    program::{ReceiveInput, Response},
};
use vesting_core::{Event, ScheduleKind};
use vesting_program::{
    clock, handle, schedule_address, selector, Asset, Instruction, Internal, NewSchedule, Op,
    VestingEvent,
};

const V: AccountId = AccountId::new([0xAA; 32]);
const CREATOR: AccountId = AccountId::new([1; 32]);
const BENE: AccountId = AccountId::new([2; 32]);
const BENE2: AccountId = AccountId::new([3; 32]);
const NONCE: [u8; 32] = [9; 32];

fn deliver(
    to: AccountId,
    from: Option<Actor>,
    authorized: bool,
    pre: &[u8],
    message: Vec<u8>,
) -> Response {
    handle(&ReceiveInput {
        receiver: Actor::new(to, V),
        from,
        is_authorized: authorized,
        pre_state: ActorState::from(pre.to_vec()),
        message,
    })
}

fn state_of(r: &Response, pre: &[u8]) -> Vec<u8> {
    r.post_state
        .clone()
        .map_or_else(|| pre.to_vec(), ActorState::into_inner)
}

fn one_event(r: &Response, schedule: AccountId) -> Event {
    assert_eq!(r.events.len(), 1, "one event per transition");
    let e = &r.events[0];
    let decoded = VestingEvent::try_from_slice(&e.data).expect("a vesting event");
    assert_eq!(decoded.schedule, schedule);
    assert_eq!(e.selector, selector(&decoded.event));
    decoded.event
}

/// Runs an operation that reads the clock: the forwarded message, then the clock's reply.
fn at_clock(
    s: AccountId,
    state: &[u8],
    caller: AccountId,
    op: Op,
    now: u64,
) -> (Vec<u8>, Response) {
    let r = deliver(
        s,
        Some(Actor::new(caller, V)),
        false,
        state,
        borsh::to_vec(&Internal::AtClock { op, granularity: 1 }).unwrap(),
    );
    assert!(
        r.events.is_empty(),
        "nothing happens before the clock answers"
    );
    assert_eq!(r.sync_messages.last().unwrap().to, clock::clock_actor());
    let pending = state_of(&r, state);
    let reply = deliver(
        s,
        Some(clock::clock_actor()),
        false,
        &pending,
        borsh::to_vec(&clock::Reply::Timestamp(now)).unwrap(),
    );
    (state_of(&reply, &pending), reply)
}

fn create(kind: ScheduleKind, total: u128, transferable: bool) -> (AccountId, Vec<u8>, Event) {
    let s = schedule_address(&V, &CREATOR, &NONCE);
    let params = NewSchedule {
        nonce: NONCE,
        beneficiary: BENE,
        asset: Asset::Native,
        total,
        kind,
        cancelable: None,
        transferable: Some(transferable),
        cancel_authority: None,
        milestone_authority: None,
        sponsor: None,
    };
    // The creator's own actor forwards a seeded Create and the escrow transfer.
    let r = deliver(
        CREATOR,
        None,
        true,
        &[],
        borsh::to_vec(&Instruction::Create {
            schedules: vec![params.clone()],
        })
        .unwrap(),
    );
    assert_eq!(
        r.sync_messages.len(),
        2,
        "the schedule's Create, then the escrow transfer"
    );
    let r = deliver(
        s,
        Some(Actor::new(CREATOR, V)),
        true,
        &[],
        borsh::to_vec(&Internal::Create(params)).unwrap(),
    );
    let pending = state_of(&r, &[]);
    let reply = deliver(
        s,
        Some(clock::clock_actor()),
        false,
        &pending,
        borsh::to_vec(&clock::Reply::Timestamp(1_000)).unwrap(),
    );
    let e = one_event(&reply, s);
    (s, state_of(&reply, &pending), e)
}

#[test]
fn create_claim_cancel_emit_their_events() {
    let (s, state, e) = create(
        ScheduleKind::Linear {
            start: 1_000,
            end: 11_000,
        },
        10_000,
        false,
    );
    assert!(matches!(
        e,
        Event::Created {
            total: 10_000,
            at: 1_000,
            ..
        }
    ));

    // A claim "as of" 6000 pays 5000 and is valid only from 6000.
    let r = deliver(
        s,
        Some(Actor::new(BENE, V)),
        false,
        &state,
        borsh::to_vec(&Internal::ClaimAsOf {
            to: BENE,
            to_private: false,
            as_of: 6_000,
        })
        .unwrap(),
    );
    assert!(matches!(
        one_event(&r, s),
        Event::Claimed {
            amount: 5_000,
            at: 6_000,
            to_private: false,
            ..
        }
    ));
    assert_eq!(r.validity.timestamps.start(), Some(6_000));
    let state = state_of(&r, &state);

    // A clock claim at 8000 pays the next 2000.
    let (state, r) = at_clock(
        s,
        &state,
        BENE,
        Op::Claim {
            to: BENE,
            to_private: true,
        },
        8_000,
    );
    assert!(matches!(
        one_event(&r, s),
        Event::Claimed {
            amount: 2_000,
            to_private: true,
            ..
        }
    ));

    // Cancel at 9000: 8000 vested, 2000 refunded.
    let (_, r) = at_clock(s, &state, CREATOR, Op::Cancel, 9_000);
    assert!(matches!(
        one_event(&r, s),
        Event::Cancelled {
            vested_at_cancel: 8_000,
            refund: 2_000,
            at: 9_000
        }
    ));
}

#[test]
fn milestone_irrevocable_transfer_emit_their_events() {
    let (s, state, _) = create(
        ScheduleKind::Milestone {
            tranches: vec![3, 2],
        },
        5,
        true,
    );
    let (state, r) = at_clock(s, &state, CREATOR, Op::SignalMilestone { index: 0 }, 2_000);
    assert!(matches!(
        one_event(&r, s),
        Event::MilestoneSignalled {
            index: 0,
            amount: 3,
            ..
        }
    ));
    let (state, r) = at_clock(s, &state, CREATOR, Op::MakeIrrevocable, 3_000);
    assert!(matches!(
        one_event(&r, s),
        Event::MadeIrrevocable { at: 3_000 }
    ));
    let (_, r) = at_clock(
        s,
        &state,
        BENE,
        Op::TransferBeneficiary {
            new_beneficiary: BENE2,
        },
        4_000,
    );
    assert!(matches!(
        one_event(&r, s),
        Event::BeneficiaryTransferred { .. }
    ));
}

#[test]
#[should_panic(expected = "vesting error 18")]
fn a_repeated_milestone_signal_is_refused() {
    let (s, state, _) = create(
        ScheduleKind::Milestone {
            tranches: vec![3, 2],
        },
        5,
        false,
    );
    let (state, _) = at_clock(s, &state, CREATOR, Op::SignalMilestone { index: 0 }, 2_000);
    let _ = at_clock(s, &state, CREATOR, Op::SignalMilestone { index: 0 }, 2_500);
}

#[test]
#[should_panic(expected = "did not authorize")]
fn a_forged_clock_reply_as_a_root_call_is_refused() {
    let (s, state, _) = create(
        ScheduleKind::Linear {
            start: 1_000,
            end: 11_000,
        },
        10_000,
        false,
    );
    // A root call (no sender) carrying a clock reply is not a clock reply: it is an instruction,
    // which the schedule account (a PDA nobody can sign for) never authorizes.
    let _ = deliver(
        s,
        None,
        false,
        &state,
        borsh::to_vec(&clock::Reply::Timestamp(u64::MAX / 2)).unwrap(),
    );
}

#[test]
#[should_panic(expected = "vesting error 10")]
fn only_the_beneficiary_claims() {
    let (s, state, _) = create(
        ScheduleKind::Linear {
            start: 1_000,
            end: 11_000,
        },
        10_000,
        false,
    );
    let _ = deliver(
        s,
        Some(Actor::new(BENE2, V)),
        false,
        &state,
        borsh::to_vec(&Internal::ClaimAsOf {
            to: BENE2,
            to_private: false,
            as_of: 6_000,
        })
        .unwrap(),
    );
}
