#![allow(dead_code)]

use vesting_core::*;

pub const A: AccountId = [1u8; 32];
pub const B: AccountId = [2u8; 32];
pub const C: AccountId = [3u8; 32];
pub const D: AccountId = [4u8; 32];
pub const TOKEN: TokenId = [9u8; 32];

pub fn linear(total: Amount, start: Timestamp, end: Timestamp) -> Schedule {
    create(CreateParams::linear(A, B, TOKEN, total, start, end), start)
        .unwrap()
        .0
}

pub fn cliff_linear(total: Amount, start: Timestamp, cliff: Timestamp, end: Timestamp) -> Schedule {
    create(
        CreateParams {
            kind: ScheduleKind::CliffLinear { start, cliff, end },
            ..CreateParams::linear(A, B, TOKEN, total, start, end)
        },
        start,
    )
    .unwrap()
    .0
}

pub fn milestone(tranches: Vec<Amount>) -> Schedule {
    let total: Amount = tranches.iter().sum();
    create(
        CreateParams {
            kind: ScheduleKind::Milestone { tranches },
            ..CreateParams::linear(A, B, TOKEN, total, 0, 1)
        },
        0,
    )
    .unwrap()
    .0
}

/// claimed + refund + still claimable + still locked must equal the total.
pub fn check_conservation(state: &Schedule, t: Timestamp) {
    let total = state.total;
    let claimed = state.claimed;
    let refund = match &state.cancelled {
        Some(rec) => total - rec.vested_at_cancel,
        None => 0,
    };
    let still_claimable = state.claimable_at(t);
    let still_locked = match &state.cancelled {
        Some(_) => 0,
        None => total - state.vested_at(t),
    };
    assert_eq!(
        claimed + refund + still_claimable + still_locked,
        total,
        "conservation broken for {:?} at t={}",
        state,
        t
    );
    assert!(claimed <= state.vested_at(t), "claimed more than vested");
    assert!(state.vested_at(t) <= total, "vested more than total");
}
