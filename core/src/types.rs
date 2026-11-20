//! Plain types shared with the future on-chain program.

use borsh::{BorshDeserialize, BorshSerialize};

use crate::math::mul_div_floor;

/// An account address. Maps to the LEZ account id.
pub type AccountId = [u8; 32];
/// A token amount.
pub type Amount = u128;
/// Milliseconds, like the LEZ clock.
pub type Timestamp = u64;
/// The token definition's account.
pub type TokenId = [u8; 32];

/// Maximum number of milestone tranches in one schedule. Bounded so that the
/// signalled bitmap in the state stays small.
pub const MAX_TRANCHES: usize = 32;

/// The most schedules one create transaction may carry. Measured on LEZ v0.4.0-rc3 (2026-11-20,
/// docs/COSTS.md): the binding limit is the 10,000,000-cycle execution cap of a public
/// transaction, not its size (146 native schedules simulate in 9.94M cycles and 44 KB; 148 exceed
/// the cap). 140 leaves a margin for heavier kinds. Token schedules cost about twice as much, so a
/// token batch tops out near 74; the wallet's simulation refuses a larger one before sending it.
/// Above about 29 native (14 token) schedules, send with a gas limit above the wallet's 2,000,000
/// default (`--gas-limit 10000000`).
pub const MAX_BATCH: usize = 140;

/// Current borsh layout version of [`Schedule`].
pub const STATE_VERSION: u8 = 1;

/// The three schedule kinds listed in RFP Functionality 1.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum ScheduleKind {
    /// Nothing before `cliff`; at `cliff` the amount accrued since `start`
    /// unlocks at once, then linear release to `end`. `cliff == start` is
    /// allowed (that is plain linear with a cliff marker).
    CliffLinear {
        start: Timestamp,
        cliff: Timestamp,
        end: Timestamp,
    },
    /// Accrues continuously from `start` to `end`.
    Linear { start: Timestamp, end: Timestamp },
    /// The milestone authority signals tranche `i`; each signal unlocks that
    /// fixed amount. The tranches must sum to the schedule total.
    Milestone { tranches: Vec<Amount> },
}

impl ScheduleKind {
    /// Number of milestone tranches, or 0 for a time-based schedule.
    pub fn tranche_count(&self) -> usize {
        match self {
            ScheduleKind::Milestone { tranches } => tranches.len(),
            _ => 0,
        }
    }
}

/// Recorded when a cancelable schedule is cancelled.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct CancelRecord {
    /// Effective time of the cancellation (after the high-water mark).
    pub at: Timestamp,
    /// Vested amount frozen at cancellation. Vested-but-unclaimed tokens stay
    /// claimable by the beneficiary after cancellation.
    pub vested_at_cancel: Amount,
}

/// The full on-chain state of one vesting schedule.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    pub creator: AccountId,
    pub beneficiary: AccountId,
    pub token: TokenId,
    pub total: Amount,
    pub claimed: Amount,
    pub kind: ScheduleKind,
    pub cancelable: bool,
    pub transferable: bool,
    /// Defaults to `creator`. Soft requirement in the RFP.
    pub cancel_authority: AccountId,
    /// Defaults to `creator`. Soft requirement in the RFP.
    pub milestone_authority: AccountId,
    /// Signalled-milestones bitmap: one bit per tranche, packed into u64 words.
    pub signalled: Vec<u64>,
    pub cancelled: Option<CancelRecord>,
    /// High-water mark for time: canonical time can go backwards, so every
    /// operation uses `max(t, last_time)` and stores the result.
    pub last_time: Timestamp,
    pub version: u8,
}

impl Schedule {
    /// Apply the high-water mark to a supplied time.
    pub fn effective_time(&self, t: Timestamp) -> Timestamp {
        t.max(self.last_time)
    }

    /// Is milestone tranche `index` signalled?
    pub fn is_signalled(&self, index: usize) -> bool {
        let word = index / 64;
        let bit = index % 64;
        self.signalled
            .get(word)
            .map(|w| w & (1u64 << bit) != 0)
            .unwrap_or(false)
    }

    /// Mark milestone tranche `index` as signalled.
    pub fn set_signalled(&mut self, index: usize) {
        let word = index / 64;
        let bit = index % 64;
        if self.signalled.len() <= word {
            self.signalled.resize(word + 1, 0);
        }
        self.signalled[word] |= 1u64 << bit;
    }

    /// Sum of the amounts of signalled milestone tranches.
    pub fn signalled_total(&self) -> Amount {
        match &self.kind {
            ScheduleKind::Milestone { tranches } => tranches
                .iter()
                .enumerate()
                .filter(|(i, _)| self.is_signalled(*i))
                .map(|(_, amount)| *amount)
                .sum(),
            _ => 0,
        }
    }

    /// Amount vested at time `t`. For a cancelled schedule this is frozen at
    /// the cancellation record; otherwise the high-water mark is applied.
    pub fn vested_at(&self, t: Timestamp) -> Amount {
        if let Some(record) = &self.cancelled {
            return record.vested_at_cancel;
        }
        let t = self.effective_time(t);
        match &self.kind {
            ScheduleKind::Linear { start, end } => Self::linear_vested(self.total, *start, *end, t),
            ScheduleKind::CliffLinear { start, cliff, end } => {
                if t < *cliff {
                    0
                } else {
                    Self::linear_vested(self.total, *start, *end, t)
                }
            }
            ScheduleKind::Milestone { .. } => self.signalled_total(),
        }
    }

    fn linear_vested(total: Amount, start: Timestamp, end: Timestamp, t: Timestamp) -> Amount {
        if t <= start {
            return 0;
        }
        if t >= end {
            return total;
        }
        mul_div_floor(total, t - start, end - start)
    }

    /// Amount currently claimable at time `t`.
    pub fn claimable_at(&self, t: Timestamp) -> Amount {
        self.vested_at(t).saturating_sub(self.claimed)
    }

    /// Smallest time at or after `t` at which something becomes claimable, or
    /// `None` if nothing ever will. Milestone schedules always return `None`:
    /// their unlocks are driven by the milestone authority, not by time.
    pub fn next_unlock_after(&self, t: Timestamp) -> Option<Timestamp> {
        if self.cancelled.is_some() {
            return None;
        }
        if matches!(self.kind, ScheduleKind::Milestone { .. }) {
            return None;
        }
        if self.claimed >= self.total {
            return None;
        }
        let t = self.effective_time(t);
        let end = match &self.kind {
            ScheduleKind::Linear { end, .. } => *end,
            ScheduleKind::CliffLinear { end, .. } => *end,
            ScheduleKind::Milestone { .. } => unreachable!(),
        };
        if t >= end {
            return None;
        }

        // `claimable_at` is monotone in t, so binary search for the first time
        // at which vested exceeds what has already been claimed.
        let mut lo = t.saturating_add(1);
        let mut hi = end;
        if lo > hi {
            return None;
        }
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.claimable_at(mid) > 0 {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }
        Some(lo)
    }
}

/// Parameters supplied by the creator. `None` means "use the RFP default".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateParams {
    pub creator: AccountId,
    pub beneficiary: AccountId,
    pub token: TokenId,
    pub total: Amount,
    pub kind: ScheduleKind,
    /// `None` defaults to `true` (RFP Functionality 3).
    pub cancelable: Option<bool>,
    /// `None` defaults to `false`. Fixed at creation, never changes.
    pub transferable: Option<bool>,
    /// `None` defaults to `creator`.
    pub cancel_authority: Option<AccountId>,
    /// `None` defaults to `creator`.
    pub milestone_authority: Option<AccountId>,
}

impl CreateParams {
    /// A simple linear schedule with all RFP defaults applied.
    pub fn linear(
        creator: AccountId,
        beneficiary: AccountId,
        token: TokenId,
        total: Amount,
        start: Timestamp,
        end: Timestamp,
    ) -> Self {
        Self {
            creator,
            beneficiary,
            token,
            total,
            kind: ScheduleKind::Linear { start, end },
            cancelable: None,
            transferable: None,
            cancel_authority: None,
            milestone_authority: None,
        }
    }
}
