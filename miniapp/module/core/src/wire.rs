//! JSON <-> core/SDK types for the module's C ABI.
//!
//! Amounts are decimal strings (u128 cannot be a JSON number), account IDs and the token ID are
//! 32-byte hex (answers) or hex/base58 (inputs), and times are JSON numbers (milliseconds).

use crate::display;
use serde_json::{json, Value};
use vesting_core::{AccountId, Amount, CreateParams, Schedule, ScheduleKind, Timestamp};
use vesting_sdk::ScheduleId;

/// Parse a 32-byte account ID, or a token ID: 64 hex characters, or base58 with or without a
/// `Public/` / `Private/` prefix. (Names are resolved before this, in `Module::resolve_fields`.)
pub fn account(value: Option<&Value>, field: &str) -> Result<AccountId, String> {
    let text = value
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field} must be an account (64 hex or base58)"))?;
    vesting_sdk::names::parse_address(text)
        .ok_or_else(|| format!("{field}: '{text}' is not a 32-byte account (64 hex or base58)"))
}

/// Parse a u128 amount from a decimal string (or a JSON integer).
pub fn amount(value: Option<&Value>, field: &str) -> Result<Amount, String> {
    match value {
        Some(Value::String(text)) => text.parse::<u128>().map_err(|e| format!("{field}: {e}")),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(|n| n as Amount)
            .ok_or_else(|| format!("{field} must be a decimal string")),
        _ => Err(format!("{field} is required (a decimal string)")),
    }
}

fn timestamp(value: Option<&Value>, field: &str) -> Result<Timestamp, String> {
    value
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{field} must be an integer timestamp"))
}

/// Parse a schedule kind: `{"type":"linear","start":..,"end":..}`,
/// `{"type":"cliff-linear","start":..,"cliff":..,"end":..}` or
/// `{"type":"milestone","tranches":["40","60"]}`.
pub fn kind(value: Option<&Value>) -> Result<ScheduleKind, String> {
    let v = value.ok_or_else(|| "kind is required".to_string())?;
    let kind_type = v
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "kind.type is required".to_string())?;
    match kind_type {
        "linear" => Ok(ScheduleKind::Linear {
            start: timestamp(v.get("start"), "kind.start")?,
            end: timestamp(v.get("end"), "kind.end")?,
        }),
        "cliff-linear" => Ok(ScheduleKind::CliffLinear {
            start: timestamp(v.get("start"), "kind.start")?,
            cliff: timestamp(v.get("cliff"), "kind.cliff")?,
            end: timestamp(v.get("end"), "kind.end")?,
        }),
        "milestone" => {
            let rows = v
                .get("tranches")
                .and_then(Value::as_array)
                .ok_or_else(|| "kind.tranches must be an array".to_string())?;
            let tranches = rows
                .iter()
                .enumerate()
                .map(|(i, row)| amount(Some(row), &format!("kind.tranches[{i}]")))
                .collect::<Result<Vec<_>, _>>()?;
            if tranches.is_empty() {
                return Err("kind.tranches must not be empty".to_string());
            }
            Ok(ScheduleKind::Milestone { tranches })
        }
        other => Err(format!("unknown kind.type: {other}")),
    }
}

/// Parse a JSON object into `CreateParams` (the `create` call's input).
pub fn create_params(v: &Value) -> Result<CreateParams, String> {
    let optional_account = |field: &str| -> Result<Option<AccountId>, String> {
        match v.get(field) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => Ok(Some(account(Some(value), field)?)),
        }
    };
    Ok(CreateParams {
        creator: account(v.get("creator"), "creator")?,
        beneficiary: account(v.get("beneficiary"), "beneficiary")?,
        token: account(v.get("token"), "token")?,
        total: amount(v.get("total"), "total")?,
        kind: kind(v.get("kind"))?,
        cancelable: v.get("cancelable").and_then(Value::as_bool),
        transferable: v.get("transferable").and_then(Value::as_bool),
        cancel_authority: optional_account("cancel_authority")?,
        milestone_authority: optional_account("milestone_authority")?,
    })
}

/// Render one schedule as JSON for the `list`/`create` answers. Display strings are computed
/// here from the core maths; the raw decimal strings are kept alongside them.
pub fn schedule_json(id: ScheduleId, s: &Schedule, now: Timestamp) -> Value {
    let claimable = s.claimable_at(now);
    let locked = s.total.saturating_sub(s.vested_at(now));
    let (kind, signalled) = match &s.kind {
        ScheduleKind::Linear { start, end } => (
            json!({ "type": "linear", "start": start, "end": end }),
            Vec::<bool>::new(),
        ),
        ScheduleKind::CliffLinear { start, cliff, end } => (
            json!({ "type": "cliff-linear", "start": start, "cliff": cliff, "end": end }),
            Vec::<bool>::new(),
        ),
        ScheduleKind::Milestone { tranches } => (
            json!({
                "type": "milestone",
                "tranches": tranches.iter().map(ToString::to_string).collect::<Vec<_>>(),
            }),
            (0..tranches.len())
                .map(|i| s.is_signalled(i))
                .collect::<Vec<_>>(),
        ),
    };
    json!({
        "id": id,
        "creator": hex::encode(s.creator),
        "beneficiary": hex::encode(s.beneficiary),
        "token": hex::encode(s.token),
        "total": s.total.to_string(),
        "total_display": display::amount(s.total),
        "claimed": s.claimed.to_string(),
        "claimed_display": display::amount(s.claimed),
        "claimable": claimable.to_string(),
        "claimable_display": display::amount(claimable),
        "locked": locked.to_string(),
        "locked_display": display::amount(locked),
        "kind": kind,
        "signalled": signalled,
        "cancelable": s.cancelable,
        "transferable": s.transferable,
        "cancel_authority": hex::encode(s.cancel_authority),
        "milestone_authority": hex::encode(s.milestone_authority),
        "cancelled": s.cancelled.is_some(),
        // After a cancel: when, what had vested then (still the beneficiary's to claim) and
        // what went back to the creator.
        "cancelled_at": s.cancelled.as_ref().map(|c| c.at),
        "vested_at_cancel": s.cancelled.as_ref().map(|c| c.vested_at_cancel.to_string()),
        "refund": s.cancelled.as_ref().map(|c| s.total.saturating_sub(c.vested_at_cancel).to_string()),
        "refund_display": s.cancelled.as_ref().map(|c| display::amount(s.total.saturating_sub(c.vested_at_cancel))),
        "last_time": s.last_time,
        "version": s.version,
    })
}
