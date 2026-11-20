mod common;

use borsh::BorshDeserialize;
use common::*;
use vesting_core::*;

#[test]
fn create_defaults_and_rejects_bad_params() {
    let (state, event) = create(CreateParams::linear(A, B, TOKEN, 1_000, 100, 200), 50).unwrap();
    assert!(state.cancelable, "cancelable defaults to true");
    assert!(!state.transferable, "transferable defaults to false");
    assert_eq!(state.cancel_authority, A);
    assert_eq!(state.milestone_authority, A);
    assert_eq!(state.claimed, 0);
    assert_eq!(state.last_time, 50);
    assert_eq!(state.version, STATE_VERSION);
    assert_eq!(
        event,
        Event::Created {
            creator: A,
            beneficiary: B,
            token: TOKEN,
            total: 1_000,
            kind: ScheduleKind::Linear {
                start: 100,
                end: 200
            },
            cancelable: true,
            transferable: false,
            cancel_authority: A,
            milestone_authority: A,
            at: 50,
        }
    );

    let bad = CreateParams::linear(A, B, TOKEN, 0, 100, 200);
    assert_eq!(create(bad, 0).unwrap_err().code(), 1); // ZeroTotal
    let bad = CreateParams::linear(A, B, TOKEN, 1, 200, 200);
    assert_eq!(create(bad, 0).unwrap_err().code(), 2); // InvalidTimes
    let bad = CreateParams {
        kind: ScheduleKind::CliffLinear {
            start: 10,
            cliff: 5,
            end: 20,
        },
        ..CreateParams::linear(A, B, TOKEN, 1, 10, 20)
    };
    assert_eq!(create(bad, 0).unwrap_err().code(), 2);
}

#[test]
fn linear_vesting_is_exact_and_monotone() {
    let state = linear(1_000, 1_000, 2_000);
    assert_eq!(state.vested_at(999), 0);
    assert_eq!(state.vested_at(1_000), 0);
    assert_eq!(state.vested_at(1_001), 1); // floor(1000/1000)
    assert_eq!(state.vested_at(1_500), 500);
    assert_eq!(state.vested_at(1_999), 999);
    assert_eq!(state.vested_at(2_000), 1_000);
    assert_eq!(state.vested_at(u64::MAX), 1_000);

    let mut previous = 0;
    for t in 0..2_100 {
        let v = state.vested_at(t);
        assert!(v >= previous, "vested went down at t={}", t);
        previous = v;
    }
    // The largest possible total and time window must not overflow.
    let huge = linear(u128::MAX, 0, u64::MAX);
    assert_eq!(huge.vested_at(u64::MAX), u128::MAX);
    // (2^128-1) / (2^64-1) == 2^64+1, so this is exact.
    assert_eq!(
        huge.vested_at(u64::MAX / 2),
        (1u128 << 127) - (1u128 << 63) - 1
    );
}

#[test]
fn cliff_linear_waits_then_unlocks_at_cliff() {
    let state = cliff_linear(1_200, 100, 400, 1_000);
    assert_eq!(state.vested_at(399), 0);
    assert_eq!(state.vested_at(400), 400); // 1200 * 300 / 900, floor
    assert_eq!(state.vested_at(700), 800);
    assert_eq!(state.vested_at(1_000), 1_200);

    let plain = cliff_linear(1_000, 100, 100, 1_100);
    assert_eq!(plain.vested_at(100), 0);
    assert_eq!(plain.vested_at(600), 500);
}

#[test]
fn claim_requires_beneficiary_and_reports_next_unlock() {
    let state = linear(1_000, 1_000, 2_000);
    assert_eq!(claim(&state, A, 1_500, false).unwrap_err().code(), 10);

    let err = claim(&state, B, 1_000, false).unwrap_err();
    assert_eq!(err.code(), 9); // NothingClaimable
    assert_eq!(next_unlock(&state, 1_000), Some(1_001));
    assert_eq!(
        err,
        VestingError::NothingClaimable {
            next_unlock: Some(1_001),
            reason: WaitingFor::Accrual { at: Some(1_001) },
        }
    );

    let (next, amount, event) = claim(&state, B, 1_500, false).unwrap();
    assert_eq!(amount, 500);
    assert_eq!(next.claimed, 500);
    assert_eq!(
        event,
        Event::Claimed {
            amount: 500,
            to_private: false,
            claimed_total: 500,
            at: 1_500,
        }
    );
    check_conservation(&next, 1_500);

    let (next2, amount2, _) = claim(&next, B, 2_000, false).unwrap();
    assert_eq!(amount2, 500);
    assert_eq!(next2.claimed, 1_000);
    let err = claim(&next2, B, u64::MAX, false).unwrap_err();
    assert_eq!(err.code(), 9);
    assert_eq!(
        err,
        VestingError::NothingClaimable {
            next_unlock: None,
            reason: WaitingFor::FullyClaimed,
        }
    );
    assert_eq!(next_unlock(&next2, u64::MAX), None);
}

#[test]
fn private_claim_is_recorded_in_the_event() {
    let state = linear(100, 0, 10);
    let (_, _, event) = claim(&state, B, 5, true).unwrap();
    assert_eq!(
        event,
        Event::Claimed {
            amount: 50,
            to_private: true,
            claimed_total: 50,
            at: 5,
        }
    );
}

#[test]
fn milestone_next_unlock_is_none_with_a_reason() {
    let state = milestone(vec![40, 60]);
    assert_eq!(claimable(&state, 1_000), 0);
    assert_eq!(next_unlock(&state, 1_000), None);
    let err = claim(&state, B, 1_000, false).unwrap_err();
    assert!(err.message().contains("milestone authority"));
    assert_eq!(err.code(), 9);
    assert_eq!(
        err,
        VestingError::NothingClaimable {
            next_unlock: None,
            reason: WaitingFor::Milestone,
        }
    );
}

#[test]
fn nothing_claimable_reasons_are_plain_words() {
    // Before the cliff the whole accrued amount is waiting at the cliff.
    let cliff_state = cliff_linear(1_000, 100, 500, 1_000);
    let err = claim(&cliff_state, B, 200, false).unwrap_err();
    assert_eq!(
        err,
        VestingError::NothingClaimable {
            next_unlock: Some(500),
            reason: WaitingFor::Cliff { at: 500 },
        }
    );
    assert!(err.message().contains("cliff"));

    // After the cliff, time-based accrual has a next growth time.
    let accrual = linear(3, 0, 1_000);
    let err = claim(&accrual, B, 0, false).unwrap_err();
    assert_eq!(
        err,
        VestingError::NothingClaimable {
            next_unlock: Some(334),
            reason: WaitingFor::Accrual { at: Some(334) },
        }
    );
    assert!(err.message().contains("334"));

    // Cancelled, with everything vested at cancellation already claimed.
    let state = linear(100, 0, 10);
    let (cancelled, _, _) = cancel(&state, A, 5).unwrap();
    let (claimed, _, _) = claim(&cancelled, B, 5, false).unwrap();
    let err = claim(&claimed, B, 10, false).unwrap_err();
    assert_eq!(
        err,
        VestingError::NothingClaimable {
            next_unlock: None,
            reason: WaitingFor::CancelledAndFullyClaimed,
        }
    );
    assert!(err.message().contains("cancelled"));
}

#[test]
fn cancel_refunds_unvested_and_keeps_vested_claimable() {
    let state = linear(1_000, 1_000, 2_000);
    assert_eq!(cancel(&state, B, 1_500,).unwrap_err().code(), 11);

    let (cancelled, refund, event) = cancel(&state, A, 1_500).unwrap();
    assert_eq!(refund, 500);
    assert_eq!(
        event,
        Event::Cancelled {
            vested_at_cancel: 500,
            refund: 500,
            at: 1_500,
        }
    );
    assert_eq!(cancelled.cancelled.as_ref().unwrap().vested_at_cancel, 500);
    // Time past the end cannot make newly vested tokens appear after cancel.
    assert_eq!(cancelled.vested_at(u64::MAX), 500);
    assert_eq!(claimable(&cancelled, u64::MAX), 500);

    let (after, amount, _) = claim(&cancelled, B, u64::MAX, false).unwrap();
    assert_eq!(amount, 500);
    assert_eq!(after.claimed, 500);
    assert_eq!(claim(&after, B, u64::MAX, false).unwrap_err().code(), 9);
    check_conservation(&after, u64::MAX);
}

#[test]
fn cancel_twice_and_irrevocable_are_rejected() {
    let state = linear(100, 0, 10);
    let (cancelled, _, _) = cancel(&state, A, 5).unwrap();
    assert_eq!(cancel(&cancelled, A, 6).unwrap_err().code(), 15); // AlreadyCancelled

    let (irrevocable, event) = make_irrevocable(&state, A, 4).unwrap();
    assert_eq!(event, Event::MadeIrrevocable { at: 4 });
    assert!(!irrevocable.cancelable);
    assert_eq!(irrevocable.last_time, 4);
    assert_eq!(make_irrevocable(&irrevocable, A, 5).unwrap_err().code(), 16);
    assert_eq!(cancel(&irrevocable, A, 5).unwrap_err().code(), 14); // NotCancelable
    assert_eq!(make_irrevocable(&state, B, 5).unwrap_err().code(), 11);

    // A separately nominated cancel authority works.
    let custom = CreateParams {
        cancel_authority: Some(C),
        ..CreateParams::linear(A, B, TOKEN, 100, 0, 10)
    };
    let (custom, _) = create(custom, 0).unwrap();
    assert_eq!(cancel(&custom, C, 5).unwrap().1, 50);
}

#[test]
fn milestone_is_idempotent_and_authorised() {
    let state = milestone(vec![40, 60]);
    assert_eq!(signal_milestone(&state, B, 0, 1).unwrap_err().code(), 12);
    assert_eq!(signal_milestone(&state, A, 2, 1).unwrap_err().code(), 19);
    assert_eq!(
        signal_milestone(&linear(100, 0, 10), A, 0, 1)
            .unwrap_err()
            .code(),
        13
    );

    let (signalled, amount, event) = signal_milestone(&state, A, 0, 1).unwrap();
    assert_eq!(amount, 40);
    assert_eq!(
        event,
        Event::MilestoneSignalled {
            index: 0,
            amount: 40,
            at: 1,
        }
    );
    assert_eq!(claimable(&signalled, 1), 40);
    let err = signal_milestone(&signalled, A, 0, 2).unwrap_err();
    assert_eq!(err.code(), 18);
    assert!(err.message().contains("twice"));

    let (signalled2, _, _) = signal_milestone(&signalled, A, 1, 3).unwrap();
    assert_eq!(claimable(&signalled2, 3), 100);
    let (claimed, amount, _) = claim(&signalled2, B, 3, false).unwrap();
    assert_eq!(amount, 100);
    check_conservation(&claimed, 3);

    let (cancelled, _, _) = cancel(&state, A, 1).unwrap();
    assert_eq!(
        signal_milestone(&cancelled, A, 0, 2).unwrap_err().code(),
        17
    );
}

#[test]
fn transfer_is_frozen_at_creation_and_authorised() {
    let state = linear(100, 0, 10);
    assert_eq!(
        transfer_beneficiary(&state, B, C, 1).unwrap_err().code(),
        20
    ); // NotTransferable

    let params = CreateParams {
        transferable: Some(true),
        ..CreateParams::linear(A, B, TOKEN, 100, 0, 10)
    };
    let (transferable, _) = create(params, 0).unwrap();
    assert_eq!(
        transfer_beneficiary(&transferable, A, C, 1)
            .unwrap_err()
            .code(),
        10
    ); // NotBeneficiary
    assert_eq!(
        transfer_beneficiary(&transferable, B, B, 1)
            .unwrap_err()
            .code(),
        21
    ); // SameBeneficiary

    let (moved, event) = transfer_beneficiary(&transferable, B, C, 1).unwrap();
    assert_eq!(
        event,
        Event::BeneficiaryTransferred {
            old: B,
            new: C,
            at: 1,
        }
    );
    assert_eq!(moved.beneficiary, C);
    assert_eq!(claim(&moved, B, 5, false).unwrap_err().code(), 10);
    assert_eq!(claim(&moved, C, 5, false).unwrap().1, 50);
    // Transferability itself cannot be changed.
    assert!(transferable.transferable);
}

#[test]
fn time_backwards_uses_a_high_water_mark() {
    let fresh = linear(1_000, 1_000, 2_000);
    assert_eq!(claimable(&fresh, 1_100), 100);

    let (claimed, amount, _) = claim(&fresh, B, 1_800, false).unwrap();
    assert_eq!(amount, 800);
    assert_eq!(claimed.last_time, 1_800);

    // A fresh state at 1_100 would have 100 claimable, but this state has
    // already claimed up to 1_800: going backwards must not rewind the clock.
    assert_eq!(claimable(&claimed, 1_100), 0);
    assert_eq!(claim(&claimed, B, 1_100, false).unwrap_err().code(), 9);

    // Cancelling with a past time also uses the high-water mark.
    let (cancelled, refund, _) = cancel(&fresh, A, 1_100).unwrap();
    assert_eq!(refund, 900);
    assert_eq!(cancelled.cancelled.unwrap().at, 1_100);
    let (cancelled2, refund2, _) = cancel(&claimed, A, 1_100).unwrap();
    assert_eq!(refund2, 200);
    assert_eq!(cancelled2.cancelled.unwrap().at, 1_800);
}

#[test]
fn failed_operations_leave_no_new_state() {
    let state = linear(100, 0, 10);
    let ms = milestone(vec![40, 60]);
    // A stranger is refused by every operation, with the exact error.
    assert_eq!(
        claim(&state, D, 5, false).unwrap_err(),
        VestingError::NotBeneficiary
    );
    assert_eq!(
        cancel(&state, D, 5).unwrap_err(),
        VestingError::NotCancelAuthority
    );
    assert_eq!(
        make_irrevocable(&state, D, 5).unwrap_err(),
        VestingError::NotCancelAuthority
    );
    assert_eq!(
        signal_milestone(&ms, D, 0, 1).unwrap_err(),
        VestingError::NotMilestoneAuthority
    );
    assert_eq!(
        transfer_beneficiary(&state, D, C, 5).unwrap_err(),
        VestingError::NotBeneficiary
    );
    // The beneficiary is not an authority for cancel or milestone signalling.
    assert_eq!(
        cancel(&state, B, 5).unwrap_err(),
        VestingError::NotCancelAuthority
    );
    assert_eq!(
        signal_milestone(&ms, B, 0, 1).unwrap_err(),
        VestingError::NotMilestoneAuthority
    );
    assert_eq!(
        make_irrevocable(&state, B, 5).unwrap_err(),
        VestingError::NotCancelAuthority
    );

    // Every borrowed state is untouched and still usable.
    assert_eq!(state, linear(100, 0, 10));
    assert_eq!(ms, milestone(vec![40, 60]));
    // A failed claim did not consume vested tokens.
    assert_eq!(claim(&state, B, 5, false).unwrap().1, 50);
}

#[test]
fn independent_schedules_do_not_share_state() {
    let first = linear(1_000, 0, 10);
    let second = linear(500, 0, 10);
    let (claimed_first, amount_first, _) = claim(&first, B, 5, false).unwrap();
    let (claimed_second, amount_second, _) = claim(&second, B, 5, true).unwrap();
    assert_eq!(amount_first, 500);
    assert_eq!(amount_second, 250);
    assert_eq!(claimed_first.claimed, 500);
    assert_eq!(claimed_second.claimed, 250);
    // Neither schedule's state was changed by the other's claim.
    assert_eq!(first, linear(1_000, 0, 10));
    assert_eq!(second, linear(500, 0, 10));
    check_conservation(&claimed_first, 10);
    check_conservation(&claimed_second, 10);
}

#[test]
fn batch_reports_the_first_bad_index() {
    let good = CreateParams::linear(A, B, TOKEN, 100, 0, 10);
    let bad = CreateParams::linear(A, B, TOKEN, 0, 0, 10);
    let err = create_batch(vec![good.clone(), good.clone(), bad, good.clone()], 0).unwrap_err();
    match err {
        VestingError::BatchItemFailed { index, source } => {
            assert_eq!(index, 2);
            assert_eq!(source.code(), 1);
        }
        other => panic!("wrong error: {:?}", other),
    }
    let (states, events) = create_batch(vec![good.clone(), good.clone()], 7).unwrap();
    assert_eq!(states.len(), 2);
    assert_eq!(events.len(), 2);
    assert!(states.iter().all(|s| s.last_time == 7));
    let too_many = vec![good; MAX_BATCH + 1];
    assert_eq!(create_batch(too_many, 0).unwrap_err().code(), 7);
}

#[test]
fn borsh_roundtrips_state_and_events() {
    let state = milestone(vec![40, 60]);
    let (signalled, _, _) = signal_milestone(&state, A, 0, 1).unwrap();
    let bytes = borsh::to_vec(&signalled).unwrap();
    let back = Schedule::try_from_slice(&bytes).unwrap();
    assert_eq!(back, signalled);

    let events = vec![
        Event::Created {
            creator: A,
            beneficiary: B,
            token: TOKEN,
            total: 100,
            kind: ScheduleKind::Milestone {
                tranches: vec![40, 60],
            },
            cancelable: true,
            transferable: false,
            cancel_authority: A,
            milestone_authority: A,
            at: 0,
        },
        Event::Claimed {
            amount: 40,
            to_private: true,
            claimed_total: 40,
            at: 5,
        },
        Event::Cancelled {
            vested_at_cancel: 40,
            refund: 60,
            at: 6,
        },
        Event::MilestoneSignalled {
            index: 1,
            amount: 60,
            at: 7,
        },
        Event::BeneficiaryTransferred {
            old: B,
            new: C,
            at: 8,
        },
        Event::MadeIrrevocable { at: 9 },
    ];
    for event in events {
        let bytes = borsh::to_vec(&event).unwrap();
        assert_eq!(Event::try_from_slice(&bytes).unwrap(), event);
    }
}

#[test]
fn version_is_checked_on_every_mutation() {
    let mut state = linear(100, 0, 10);
    state.version = 99;
    assert_eq!(claim(&state, B, 5, false).unwrap_err().code(), 23);
    assert_eq!(cancel(&state, A, 5).unwrap_err().code(), 23);
    assert_eq!(make_irrevocable(&state, A, 5).unwrap_err().code(), 23);
    assert_eq!(signal_milestone(&state, A, 0, 5).unwrap_err().code(), 23);
    assert_eq!(
        transfer_beneficiary(&state, B, C, 5).unwrap_err().code(),
        23
    );
}

#[test]
fn error_codes_are_stable_and_messages_are_actionable() {
    let err = VestingError::MilestoneAlreadySignalled { index: 3 };
    assert_eq!(err.code(), 18);
    assert_eq!(
        err.code(),
        VestingError::MilestoneAlreadySignalled { index: 9 }.code()
    );
    assert!(!err.message().is_empty());
    // Every code is unique.
    let errors = vec![
        VestingError::ZeroTotal,
        VestingError::InvalidTimes,
        VestingError::EmptyTranches,
        VestingError::TooManyTranches,
        VestingError::ZeroTranche { index: 0 },
        VestingError::TrancheSumMismatch {
            expected: 1,
            got: 2,
        },
        VestingError::BatchTooLarge { len: 1 },
        VestingError::BatchItemFailed {
            index: 0,
            source: Box::new(VestingError::ZeroTotal),
        },
        VestingError::NothingClaimable {
            next_unlock: None,
            reason: WaitingFor::Accrual { at: None },
        },
        VestingError::NotBeneficiary,
        VestingError::NotCancelAuthority,
        VestingError::NotMilestoneAuthority,
        VestingError::NotMilestoneSchedule,
        VestingError::NotCancelable,
        VestingError::AlreadyCancelled,
        VestingError::AlreadyIrrevocable,
        VestingError::CancelledSchedule,
        VestingError::MilestoneAlreadySignalled { index: 0 },
        VestingError::MilestoneOutOfRange { index: 0 },
        VestingError::NotTransferable,
        VestingError::SameBeneficiary,
        VestingError::ArithmeticOverflow,
        VestingError::VersionMismatch {
            expected: 1,
            got: 2,
        },
    ];
    let mut codes: Vec<u32> = errors.iter().map(|e| e.code()).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), errors.len());
}
