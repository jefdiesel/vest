//! State transitions as pure functions.
//!
//! Every operation borrows the old state and returns a new one, so an `Err`
//! can never leave a partially modified state behind. The on-chain program
//! owns atomicity; these functions only describe the rules.

use crate::error::{VestingError, WaitingFor};
use crate::event::Event;
use crate::types::{
    AccountId, Amount, CancelRecord, CreateParams, Schedule, ScheduleKind, Timestamp, MAX_BATCH,
    MAX_TRANCHES, STATE_VERSION,
};

/// Validate every creation rule. Returns the packed signalled bitmap length.
fn validate(params: &CreateParams) -> Result<usize, VestingError> {
    if params.total == 0 {
        return Err(VestingError::ZeroTotal);
    }
    match &params.kind {
        ScheduleKind::Linear { start, end } => {
            if start >= end {
                return Err(VestingError::InvalidTimes);
            }
            Ok(0)
        }
        ScheduleKind::CliffLinear { start, cliff, end } => {
            if start > cliff || cliff >= end {
                return Err(VestingError::InvalidTimes);
            }
            Ok(0)
        }
        ScheduleKind::Milestone { tranches } => {
            if tranches.is_empty() {
                return Err(VestingError::EmptyTranches);
            }
            if tranches.len() > MAX_TRANCHES {
                return Err(VestingError::TooManyTranches);
            }
            let mut sum: Amount = 0;
            for (index, amount) in tranches.iter().enumerate() {
                if *amount == 0 {
                    return Err(VestingError::ZeroTranche { index });
                }
                sum = sum
                    .checked_add(*amount)
                    .ok_or(VestingError::ArithmeticOverflow)?;
            }
            if sum != params.total {
                return Err(VestingError::TrancheSumMismatch {
                    expected: params.total,
                    got: sum,
                });
            }
            Ok((tranches.len() + 63) / 64)
        }
    }
}

fn check_version(state: &Schedule) -> Result<(), VestingError> {
    if state.version != STATE_VERSION {
        return Err(VestingError::VersionMismatch {
            expected: STATE_VERSION,
            got: state.version,
        });
    }
    Ok(())
}

/// Create one schedule (RFP Functionality 1–4).
pub fn create(params: CreateParams, t: Timestamp) -> Result<(Schedule, Event), VestingError> {
    let words = validate(&params)?;
    let creator = params.creator;
    let cancelable = params.cancelable.unwrap_or(true);
    let transferable = params.transferable.unwrap_or(false);

    let state = Schedule {
        creator,
        beneficiary: params.beneficiary,
        token: params.token,
        total: params.total,
        claimed: 0,
        kind: params.kind,
        cancelable,
        transferable,
        cancel_authority: params.cancel_authority.unwrap_or(creator),
        milestone_authority: params.milestone_authority.unwrap_or(creator),
        signalled: vec![0u64; words],
        cancelled: None,
        last_time: t,
        version: STATE_VERSION,
    };

    let event = Event::Created {
        creator,
        beneficiary: state.beneficiary,
        token: state.token,
        total: state.total,
        kind: state.kind.clone(),
        cancelable,
        transferable,
        cancel_authority: state.cancel_authority,
        milestone_authority: state.milestone_authority,
        at: t,
    };
    Ok((state, event))
}

/// Create a batch of schedules (RFP Functionality 5).
///
/// Returns all states and events, or the first error together with its index
/// in the input batch. `MAX_BATCH` is the measured LEZ rc3 limit (see its note).
pub fn create_batch(
    params: Vec<CreateParams>,
    t: Timestamp,
) -> Result<(Vec<Schedule>, Vec<Event>), VestingError> {
    if params.len() > MAX_BATCH {
        return Err(VestingError::BatchTooLarge { len: params.len() });
    }
    let mut states = Vec::with_capacity(params.len());
    let mut events = Vec::with_capacity(params.len());
    for (index, item) in params.into_iter().enumerate() {
        match create(item, t) {
            Ok((state, event)) => {
                states.push(state);
                events.push(event);
            }
            Err(source) => {
                return Err(VestingError::BatchItemFailed {
                    index,
                    source: Box::new(source),
                })
            }
        }
    }
    Ok((states, events))
}

/// Amount claimable now (RFP Usability 1, 7).
pub fn claimable(state: &Schedule, t: Timestamp) -> Amount {
    state.claimable_at(t)
}

/// Next time the claimable amount grows, or `None` (RFP Usability 7).
pub fn next_unlock(state: &Schedule, t: Timestamp) -> Option<Timestamp> {
    state.next_unlock_after(t)
}

/// Why a claim found nothing, expressed as a stable reason a client can branch
/// on. `t` is already the effective (high-water) time.
fn claim_block_reason(
    state: &Schedule,
    t: Timestamp,
    next_unlock: Option<Timestamp>,
) -> WaitingFor {
    if state.cancelled.is_some() {
        return WaitingFor::CancelledAndFullyClaimed;
    }
    if state.claimed >= state.total {
        return WaitingFor::FullyClaimed;
    }
    match &state.kind {
        ScheduleKind::Milestone { .. } => WaitingFor::Milestone,
        ScheduleKind::CliffLinear { cliff, .. } if t < *cliff && next_unlock == Some(*cliff) => {
            WaitingFor::Cliff { at: *cliff }
        }
        _ => WaitingFor::Accrual { at: next_unlock },
    }
}

/// Claim everything currently vested (RFP Functionality 2, Usability 7,
/// Privacy 1). `to_private` is recorded in the event; the program decides how
/// to route the tokens.
pub fn claim(
    state: &Schedule,
    caller: AccountId,
    t: Timestamp,
    to_private: bool,
) -> Result<(Schedule, Amount, Event), VestingError> {
    check_version(state)?;
    if caller != state.beneficiary {
        return Err(VestingError::NotBeneficiary);
    }
    let t = state.effective_time(t);
    let amount = state.claimable_at(t);
    if amount == 0 {
        let next_unlock = state.next_unlock_after(t);
        return Err(VestingError::NothingClaimable {
            next_unlock,
            reason: claim_block_reason(state, t, next_unlock),
        });
    }

    let mut next = state.clone();
    next.claimed = next
        .claimed
        .checked_add(amount)
        .ok_or(VestingError::ArithmeticOverflow)?;
    next.last_time = t;
    let event = Event::Claimed {
        amount,
        to_private,
        claimed_total: next.claimed,
        at: t,
    };
    Ok((next, amount, event))
}

/// Cancel a cancelable schedule and refund the unvested balance (RFP
/// Functionality 3). Vested-but-unclaimed tokens remain claimable.
pub fn cancel(
    state: &Schedule,
    caller: AccountId,
    t: Timestamp,
) -> Result<(Schedule, Amount, Event), VestingError> {
    check_version(state)?;
    if caller != state.cancel_authority {
        return Err(VestingError::NotCancelAuthority);
    }
    if !state.cancelable {
        return Err(VestingError::NotCancelable);
    }
    if state.cancelled.is_some() {
        return Err(VestingError::AlreadyCancelled);
    }
    let t = state.effective_time(t);
    let vested = state.vested_at(t);
    let refund = state
        .total
        .checked_sub(vested)
        .ok_or(VestingError::ArithmeticOverflow)?;

    let mut next = state.clone();
    next.cancelled = Some(CancelRecord {
        at: t,
        vested_at_cancel: vested,
    });
    next.last_time = t;
    let event = Event::Cancelled {
        vested_at_cancel: vested,
        refund,
        at: t,
    };
    Ok((next, refund, event))
}

/// Make a cancelable schedule irrevocable. One-way by design; doing it twice
/// is an error, not a silent no-op (RFP Design Rationale, Cancellation
/// semantics). Soft requirement: the creator may act as well as the cancel
/// authority.
pub fn make_irrevocable(
    state: &Schedule,
    caller: AccountId,
    t: Timestamp,
) -> Result<(Schedule, Event), VestingError> {
    check_version(state)?;
    if caller != state.cancel_authority && caller != state.creator {
        return Err(VestingError::NotCancelAuthority);
    }
    if state.cancelled.is_some() {
        return Err(VestingError::CancelledSchedule);
    }
    if !state.cancelable {
        return Err(VestingError::AlreadyIrrevocable);
    }
    let t = state.effective_time(t);
    let mut next = state.clone();
    next.cancelable = false;
    next.last_time = t;
    Ok((next, Event::MadeIrrevocable { at: t }))
}

/// Signal one milestone tranche (RFP Functionality 1, Reliability 4).
/// Signalling the same index twice is a deterministic error and never unlocks
/// twice.
pub fn signal_milestone(
    state: &Schedule,
    caller: AccountId,
    index: usize,
    t: Timestamp,
) -> Result<(Schedule, Amount, Event), VestingError> {
    check_version(state)?;
    if caller != state.milestone_authority {
        return Err(VestingError::NotMilestoneAuthority);
    }
    if state.cancelled.is_some() {
        return Err(VestingError::CancelledSchedule);
    }
    let tranches = match &state.kind {
        ScheduleKind::Milestone { tranches } => tranches,
        _ => return Err(VestingError::NotMilestoneSchedule),
    };
    if index >= tranches.len() {
        return Err(VestingError::MilestoneOutOfRange { index });
    }
    if state.is_signalled(index) {
        return Err(VestingError::MilestoneAlreadySignalled { index });
    }
    let amount = tranches[index];

    let t = state.effective_time(t);
    let mut next = state.clone();
    next.set_signalled(index);
    next.last_time = t;
    let event = Event::MilestoneSignalled {
        index: index as u16,
        amount,
        at: t,
    };
    Ok((next, amount, event))
}

/// Transfer the beneficiary rights, if the position was created transferable
/// (RFP Functionality 4).
pub fn transfer_beneficiary(
    state: &Schedule,
    caller: AccountId,
    new_beneficiary: AccountId,
    t: Timestamp,
) -> Result<(Schedule, Event), VestingError> {
    check_version(state)?;
    if caller != state.beneficiary {
        return Err(VestingError::NotBeneficiary);
    }
    if !state.transferable {
        return Err(VestingError::NotTransferable);
    }
    if new_beneficiary == state.beneficiary {
        return Err(VestingError::SameBeneficiary);
    }
    let old = state.beneficiary;

    let t = state.effective_time(t);
    let mut next = state.clone();
    next.beneficiary = new_beneficiary;
    next.last_time = t;
    let event = Event::BeneficiaryTransferred {
        old,
        new: new_beneficiary,
        at: t,
    };
    Ok((next, event))
}
