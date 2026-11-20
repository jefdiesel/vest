//! Plain-words output for the CLI.

use chrono::{DateTime, Utc};
use vesting_core::{
    next_unlock, AccountId, Amount, Schedule, ScheduleKind, Timestamp, VestingError, WaitingFor,
};

/// A UTC timestamp such as `2027-01-01 00:00 UTC`.
pub fn utc(milliseconds: Timestamp) -> String {
    match DateTime::<Utc>::from_timestamp_millis(milliseconds as i64) {
        Some(date) => date.format("%Y-%m-%d %H:%M UTC").to_string(),
        None => format!("{} ms (outside the supported date range)", milliseconds),
    }
}

pub fn human_duration(from: Timestamp, to: Timestamp) -> String {
    let seconds = to.saturating_sub(from) / 1_000;
    if seconds < 60 {
        return format!("{} seconds", seconds);
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{} minutes", minutes);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{} hours", hours);
    }
    format!("{} days", hours / 24)
}

/// One line describing the next unlock, using the words from the RFP.
pub fn next_unlock_line(state: &Schedule, now: Timestamp) -> String {
    if state.cancelled.is_some() {
        return "next unlock: none (the schedule was cancelled)".to_string();
    }
    if matches!(state.kind, ScheduleKind::Milestone { .. }) && state.claimed < state.total {
        return "next unlock: awaiting a milestone signal".to_string();
    }
    match next_unlock(state, now) {
        Some(at) => format!("next unlock in {} ({})", human_duration(now, at), utc(at)),
        None if state.claimable_at(now) > 0 => format!(
            "next unlock: none (fully vested; {} not yet claimed)",
            state.claimable_at(now)
        ),
        None => "next unlock: none (everything vested has been claimed)".to_string(),
    }
}

pub fn schedule_kind_line(state: &Schedule) -> String {
    match &state.kind {
        ScheduleKind::Linear { start, end } => {
            format!("type: linear from {} to {}", utc(*start), utc(*end))
        }
        ScheduleKind::CliffLinear { start, cliff, end } => format!(
            "type: cliff + linear, start {}, cliff {}, end {}",
            utc(*start),
            utc(*cliff),
            utc(*end)
        ),
        ScheduleKind::Milestone { tranches } => format!(
            "type: milestone, {} tranches: {}",
            tranches.len(),
            tranches
                .iter()
                .map(|amount| amount.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub fn show(
    id: u64,
    state: &Schedule,
    now: Timestamp,
    display: impl Fn(&AccountId) -> String,
) -> String {
    let mut lines = vec![
        format!("schedule {}", id),
        schedule_kind_line(state),
        format!("token: {}", display(&state.token)),
        format!("creator: {}", display(&state.creator)),
        format!("beneficiary: {}", display(&state.beneficiary)),
        format!("total: {}", state.total),
        format!("claimed: {}", state.claimed),
        format!("claimable now: {}", state.claimable_at(now)),
        next_unlock_line(state, now),
        format!(
            "cancelable: {}",
            if state.cancelable {
                "yes"
            } else {
                "no, irrevocable"
            }
        ),
        format!(
            "transferable: {}",
            if state.transferable { "yes" } else { "no" }
        ),
    ];
    if let Some(record) = &state.cancelled {
        lines.push(format!(
            "cancelled at {}; vested at cancellation {}, refund {}",
            utc(record.at),
            record.vested_at_cancel,
            state.total - record.vested_at_cancel
        ));
    }
    lines.join("\n")
}

pub fn error(error: &VestingError) -> String {
    format!("error: {} (code {})", error.message(), error.code())
}

pub fn waiting_reason(reason: &WaitingFor) -> String {
    match reason {
        WaitingFor::Cliff { at } => format!("waiting for the cliff at {}", utc(*at)),
        WaitingFor::Accrual { at: Some(at) } => {
            format!("waiting for accrual; next unlock at {}", utc(*at))
        }
        WaitingFor::Accrual { at: None } => "waiting for accrual".to_string(),
        WaitingFor::Milestone => "waiting for a milestone signal".to_string(),
        WaitingFor::FullyClaimed => "everything vested has been claimed".to_string(),
        WaitingFor::CancelledAndFullyClaimed => {
            "cancelled and everything vested at cancellation has been claimed".to_string()
        }
    }
}

/// The RFP Privacy disclosure, printed before every private claim.
pub fn privacy_disclosure(amount: Amount, beneficiary: &str, id: u64, destination: &str) -> String {
    format!(
        "Privacy disclosure — claim to a private account\n\
         Claiming {} from schedule {}.\n\
         Visible on-chain: the claim amount ({}), the beneficiary address ({}), and the vesting schedule address.\n\
         Not traceable: the destination private account ({}) and any subsequent movements of the claimed tokens.",
        amount, id, amount, beneficiary, destination
    )
}
