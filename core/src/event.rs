//! Borsh event records for every state transition. See docs/EVENTS.md.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::types::{AccountId, Amount, ScheduleKind, Timestamp, TokenId};

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Created {
        creator: AccountId,
        beneficiary: AccountId,
        token: TokenId,
        total: Amount,
        kind: ScheduleKind,
        cancelable: bool,
        transferable: bool,
        cancel_authority: AccountId,
        milestone_authority: AccountId,
        /// Effective time of the creation.
        at: Timestamp,
    },
    Claimed {
        amount: Amount,
        to_private: bool,
        claimed_total: Amount,
        /// Effective time of the claim.
        at: Timestamp,
    },
    Cancelled {
        vested_at_cancel: Amount,
        refund: Amount,
        /// Effective time of the cancellation.
        at: Timestamp,
    },
    MilestoneSignalled {
        index: u16,
        amount: Amount,
        /// Effective time of the signal.
        at: Timestamp,
    },
    BeneficiaryTransferred {
        old: AccountId,
        new: AccountId,
        /// Effective time of the transfer.
        at: Timestamp,
    },
    MadeIrrevocable {
        /// Effective time of the change.
        at: Timestamp,
    },
}
