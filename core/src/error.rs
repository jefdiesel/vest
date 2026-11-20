//! Errors with stable numeric codes and actionable, plain-English messages.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::types::Timestamp;
use std::fmt;

/// Why a claim found nothing to claim. Kept borsh-stable so a client can
/// branch on the reason instead of parsing the message text.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitingFor {
    /// Still before the cliff; the whole accrued amount unlocks at `at`.
    Cliff { at: Timestamp },
    /// Time-based vesting is accruing; `at` is the next growth in claimable
    /// amount, or `None` if it is still accruing but no unit has grown yet.
    Accrual { at: Option<Timestamp> },
    /// A milestone tranche has not been signalled yet.
    Milestone,
    /// All vested tokens have been claimed.
    FullyClaimed,
    /// Cancelled, and everything vested at cancellation has been claimed.
    CancelledAndFullyClaimed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VestingError {
    ZeroTotal,
    InvalidTimes,
    EmptyTranches,
    TooManyTranches,
    ZeroTranche {
        index: usize,
    },
    TrancheSumMismatch {
        expected: u128,
        got: u128,
    },
    BatchTooLarge {
        len: usize,
    },
    BatchItemFailed {
        index: usize,
        source: Box<VestingError>,
    },
    NothingClaimable {
        next_unlock: Option<Timestamp>,
        reason: WaitingFor,
    },
    NotBeneficiary,
    NotCancelAuthority,
    NotMilestoneAuthority,
    NotMilestoneSchedule,
    NotCancelable,
    AlreadyCancelled,
    AlreadyIrrevocable,
    CancelledSchedule,
    MilestoneAlreadySignalled {
        index: usize,
    },
    MilestoneOutOfRange {
        index: usize,
    },
    NotTransferable,
    SameBeneficiary,
    ArithmeticOverflow,
    VersionMismatch {
        expected: u8,
        got: u8,
    },
}

impl VestingError {
    /// Stable numeric code. Codes never change so clients can match on them.
    pub fn code(&self) -> u32 {
        match self {
            VestingError::ZeroTotal => 1,
            VestingError::InvalidTimes => 2,
            VestingError::EmptyTranches => 3,
            VestingError::TooManyTranches => 4,
            VestingError::ZeroTranche { .. } => 5,
            VestingError::TrancheSumMismatch { .. } => 6,
            VestingError::BatchTooLarge { .. } => 7,
            VestingError::BatchItemFailed { .. } => 8,
            VestingError::NothingClaimable { .. } => 9,
            VestingError::NotBeneficiary => 10,
            VestingError::NotCancelAuthority => 11,
            VestingError::NotMilestoneAuthority => 12,
            VestingError::NotMilestoneSchedule => 13,
            VestingError::NotCancelable => 14,
            VestingError::AlreadyCancelled => 15,
            VestingError::AlreadyIrrevocable => 16,
            VestingError::CancelledSchedule => 17,
            VestingError::MilestoneAlreadySignalled { .. } => 18,
            VestingError::MilestoneOutOfRange { .. } => 19,
            VestingError::NotTransferable => 20,
            VestingError::SameBeneficiary => 21,
            VestingError::ArithmeticOverflow => 22,
            VestingError::VersionMismatch { .. } => 23,
        }
    }

    /// A one-line message telling the user what happened and what to do.
    pub fn message(&self) -> String {
        match self {
            VestingError::ZeroTotal => "The total amount must be greater than zero.".into(),
            VestingError::InvalidTimes => {
                "Invalid schedule times: check start, cliff and end (start <= cliff < end).".into()
            }
            VestingError::EmptyTranches => {
                "A milestone schedule needs at least one tranche.".into()
            }
            VestingError::TooManyTranches { .. } => {
                "Too many milestone tranches; reduce the count.".into()
            }
            VestingError::ZeroTranche { index } => {
                format!("Tranche {} is zero; give it a positive amount.", index)
            }
            VestingError::TrancheSumMismatch { expected, got } => format!(
                "Tranche amounts sum to {}, but the total is {}. Make them match.",
                got, expected
            ),
            VestingError::BatchTooLarge { len } => format!(
                "Batch has {} schedules; the maximum is {}. Split the batch.",
                len,
                crate::types::MAX_BATCH
            ),
            VestingError::BatchItemFailed { index, source } => format!(
                "Schedule {} in the batch failed: {}",
                index,
                source.message()
            ),
            VestingError::NothingClaimable {
                next_unlock: _,
                reason,
            } => match reason {
                WaitingFor::Cliff { at } => format!(
                    "Nothing is claimable yet: the cliff is at {} ms. The whole accrued amount unlocks then.",
                    at
                ),
                WaitingFor::Accrual { at: Some(at) } => {
                    format!("Nothing is claimable yet; the next unlock is at {} ms.", at)
                }
                WaitingFor::Accrual { at: None } => {
                    "Nothing is claimable yet, but tokens are still accruing over time.".into()
                }
                WaitingFor::Milestone => {
                    "Nothing is claimable yet: waiting for the milestone authority to signal the next tranche.".into()
                }
                WaitingFor::FullyClaimed => {
                    "Everything that has vested has already been claimed from this schedule.".into()
                }
                WaitingFor::CancelledAndFullyClaimed => {
                    "The schedule was cancelled and everything vested at cancellation has been claimed.".into()
                }
            },
            VestingError::NotBeneficiary => {
                "Only the current beneficiary may claim or transfer this position.".into()
            }
            VestingError::NotCancelAuthority => {
                "Only the cancellation authority may cancel this schedule or make it irrevocable."
                    .into()
            }
            VestingError::NotMilestoneAuthority => {
                "Only the milestone authority may signal milestones on this schedule.".into()
            }
            VestingError::NotMilestoneSchedule => {
                "Milestones can only be signalled on a milestone schedule.".into()
            }
            VestingError::NotCancelable => {
                "This schedule is irrevocable and cannot be cancelled.".into()
            }
            VestingError::AlreadyCancelled => "This schedule has already been cancelled.".into(),
            VestingError::AlreadyIrrevocable => {
                "This schedule is already irrevocable; the change is one-way.".into()
            }
            VestingError::CancelledSchedule => {
                "This operation is not allowed on a cancelled schedule.".into()
            }
            VestingError::MilestoneAlreadySignalled { index } => format!(
                "Milestone {} was already signalled; it cannot unlock twice.",
                index
            ),
            VestingError::MilestoneOutOfRange { index } => format!(
                "Milestone {} does not exist; check the tranche count.",
                index
            ),
            VestingError::NotTransferable => {
                "This position is not transferable; it was fixed at creation.".into()
            }
            VestingError::SameBeneficiary => {
                "The new beneficiary must be different from the current one.".into()
            }
            VestingError::ArithmeticOverflow => {
                "An amount overflowed; check the schedule total and tranche amounts.".into()
            }
            VestingError::VersionMismatch { expected, got } => format!(
                "Unsupported schedule version {}; expected {}.",
                got, expected
            ),
        }
    }
}

impl fmt::Display for VestingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for VestingError {}
