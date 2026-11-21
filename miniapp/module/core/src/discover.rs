//! Schedules arrive by themselves: the module finds them by reading blocks.
//!
//! The chain has no lookup by beneficiary, so `discover` reads the blocks after the last one it
//! read (`getBlockRange`, base64 borsh `common::block::Block`), keeps every account the Vest
//! program touched (an actor whose program account is Vest's: compared as 32 bytes, never as
//! text, since the chain's JSON writes program ids in base58), and reads each of those accounts
//! (`getAccountView`). An account holding a schedule is remembered with its last decoded state;
//! one whose current beneficiary is one of the wallet's accounts is imported, so `list()` shows
//! it. A schedule touched again later is read again: a transferable position can move to you,
//! or away from you.
//!
//! Paced: one RPC call at a time, at least [`GAP`] apart (the public door allows 50 per 10 s per
//! address, shared with the person's browser). Bounded: at most `max_blocks` blocks and
//! [`BUDGET`] of wall time per call, so a call stays inside the app's call timeout and the
//! module (one call at a time) answers the person's own presses soon; the app calls again until
//! `done`. The state lives in `vesting-discover.json` in the module's folder, keyed by chain id
//! and program: another chain or program starts over from the program's first block.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use vesting_core::{AccountId, Schedule, Timestamp};
use vesting_program::{Asset, ScheduleState};
use vesting_sdk::names::{base58, RpcClient};

pub const STATE_FILE: &str = "vesting-discover.json";
const FORMAT: &str = "vest-discover/1";
/// Blocks per call when the app gives none.
pub const DEFAULT_MAX_BLOCKS: u64 = 2000;
/// The most blocks one call reads, whatever it is asked.
pub const MAX_BLOCKS_CAP: u64 = 10_000;
/// Blocks per `getBlockRange`.
pub const RANGE: u64 = 100;
/// Between two RPC calls of a scan.
pub const GAP: Duration = Duration::from_millis(1000);
/// Schedules one call imports (each reads the account once more); the rest wait for the next call.
pub const IMPORTS_PER_CALL: usize = 5;
/// Wall time one call may spend reading before it answers (the app calls again).
pub const BUDGET: Duration = Duration::from_secs(10);

/// What the scan remembers of one schedule account.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Known {
    /// hex(borsh(Schedule)) as last read.
    pub schedule: String,
    /// Native LEZ (the zero token), not a token schedule.
    pub native: bool,
    /// The block whose transaction made the scan read it last.
    pub block: u64,
}

impl Known {
    pub fn decoded(&self) -> Option<Schedule> {
        let bytes = hex::decode(&self.schedule).ok()?;
        borsh::from_slice(&bytes).ok()
    }
}

/// The scan state, saved after every block range.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ScanState {
    pub format: String,
    pub chain_id: String,
    /// The Vest program, base58.
    pub program: String,
    /// Where this chain's scan started.
    pub floor: u64,
    /// The last block read; `None` before the first.
    pub last_block: Option<u64>,
    /// The chain's tip at the last call.
    pub tip: u64,
    /// Every account the Vest program touched (base58) -> `[block, transaction hash]`.
    pub touched: BTreeMap<String, Vec<(u64, String)>>,
    /// Accounts touched in blocks already read whose view is still to read (a call that ran out
    /// of time or met an RPC error); the next call reads them first.
    pub pending: Vec<String>,
    /// Schedule accounts (base58) with their last decoded state.
    pub schedules: BTreeMap<String, Known>,
    /// Schedules this scan imported (base58), so it never imports one twice.
    pub imported: BTreeSet<String>,
}

impl ScanState {
    pub fn fresh(chain_id: &str, program: &AccountId, floor: u64) -> Self {
        Self {
            format: FORMAT.into(),
            chain_id: chain_id.into(),
            program: base58(program),
            floor,
            ..Self::default()
        }
    }

    pub fn path(dir: &Path) -> PathBuf {
        dir.join(STATE_FILE)
    }

    /// The saved state, or `None` (no file, or one this build can't read).
    pub fn load(dir: &Path) -> Option<Self> {
        let bytes = std::fs::read(Self::path(dir)).ok()?;
        let state: Self = serde_json::from_slice(&bytes).ok()?;
        (state.format == FORMAT).then_some(state)
    }

    /// Written to a temporary name, then renamed: never half a file.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not save the scan: {e}"))?;
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("{STATE_FILE}.tmp"));
        std::fs::write(&tmp, bytes).map_err(|e| format!("could not save the scan: {e}"))?;
        std::fs::rename(&tmp, Self::path(dir)).map_err(|e| format!("could not save the scan: {e}"))
    }

    /// The first block the next call reads.
    pub fn next_block(&self) -> u64 {
        self.last_block.map_or(self.floor, |b| b + 1).max(self.floor)
    }

    pub fn touch(&mut self, account: &str, block: u64, tx: &str) {
        let seen = self.touched.entry(account.to_string()).or_default();
        if !seen.iter().any(|(b, t)| *b == block && t == tx) {
            seen.push((block, tx.to_string()));
        }
        if !self.pending.iter().any(|p| p == account) {
            self.pending.push(account.to_string());
        }
    }
}

/// The accounts the Vest program `program` touched in `block`, each with the transaction's hash
/// (hex), in the block's order: the actors whose program account is Vest's.
pub fn vest_accounts(block: &common::block::Block, program: &AccountId) -> Vec<(AccountId, String)> {
    let mut out = Vec::new();
    for tx in &block.body.transactions {
        let context = match tx {
            common::transaction::LeeTransaction::Public(t) => &t.message.public_context,
            common::transaction::LeeTransaction::PrivacyPreserving(t) => &t.message.public_context,
        };
        let hash = hex::encode(tx.hash().0);
        for actor in &context.actors {
            if actor.program_account_id.value() == program {
                let account = *actor.account_id.value();
                if !out.iter().any(|(a, h): &(AccountId, String)| *a == account && *h == hash) {
                    out.push((account, hash.clone()));
                }
            }
        }
    }
    out
}

/// A `getBlockRange` answer: a list of base64 borsh blocks.
pub fn decode_blocks(answer: &Value) -> Result<Vec<common::block::Block>, String> {
    let list = answer
        .as_array()
        .ok_or("getBlockRange: expected a list of blocks")?;
    list.iter()
        .map(|b| {
            serde_json::from_value::<common::block::Block>(b.clone())
                .map_err(|e| format!("getBlockRange: a block this build can't read: {e}"))
        })
        .collect()
}

/// The Vest state of `account`, `None` when it holds none (a creator's or a payee's own account,
/// or nothing at all). Text that isn't a Vest state counts as none.
pub fn read_state(
    client: &RpcClient,
    account: &AccountId,
    program: &AccountId,
) -> Result<Option<ScheduleState>, String> {
    let bytes = match client.get_actor_state(&base58(account), &base58(program)) {
        Ok(bytes) => bytes,
        Err(error) if error.to_string().contains("no state for the program") => Vec::new(),
        Err(error) => return Err(format!("getAccountView: {error}")),
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    Ok(borsh::from_slice::<ScheduleState>(&bytes).ok())
}

/// One RPC call at a time, at least `gap` after the previous one.
pub struct Pacer {
    pub gap: Duration,
    last: Option<Instant>,
}

impl Pacer {
    pub fn new(gap: Duration) -> Self {
        Self { gap, last: None }
    }

    /// Sleep until `gap` has passed since the previous call ended.
    pub fn wait(&mut self) {
        if let Some(last) = self.last {
            let since = last.elapsed();
            if since < self.gap {
                std::thread::sleep(self.gap - since);
            }
        }
    }

    /// A call just ended.
    pub fn mark(&mut self) {
        self.last = Some(Instant::now());
    }

    pub fn call(&mut self, client: &RpcClient, method: &str, params: Value) -> Result<Value, String> {
        self.wait();
        let answer = client.call(method, params);
        self.mark();
        answer.map_err(|e| format!("{method}: {e}"))
    }
}

/// `max_blocks` as the app sends it: empty is the default, at most [`MAX_BLOCKS_CAP`].
pub fn parse_max_blocks(text: &str) -> Result<u64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(DEFAULT_MAX_BLOCKS);
    }
    match text.parse::<u64>() {
        Ok(0) | Err(_) => Err(format!(
            "max_blocks must be a whole number of blocks above 0, got '{text}'"
        )),
        Ok(n) => Ok(n.min(MAX_BLOCKS_CAP)),
    }
}

/// `unvested` (nothing claimable now, more to come), `claimable`, or `closed` (all claimed, or
/// cancelled with nothing left), at `now`.
pub fn status(schedule: &Schedule, now: Timestamp) -> &'static str {
    if schedule.claimable_at(now) > 0 {
        "claimable"
    } else if schedule.cancelled.is_some() || schedule.claimed >= schedule.total {
        "closed"
    } else {
        "unvested"
    }
}

/// One schedule as `incoming()` answers it (the same field meanings as `list()`).
pub fn incoming_row(address: &AccountId, known: &Known, now: Timestamp) -> Option<Value> {
    let s = known.decoded()?;
    let claimable = s.claimable_at(now);
    Some(json!({
        "address": hex::encode(address),
        "address_base58": base58(address),
        "creator": hex::encode(s.creator),
        "creator_base58": base58(&s.creator),
        "beneficiary": hex::encode(s.beneficiary),
        "beneficiary_base58": base58(&s.beneficiary),
        "native": known.native,
        "total": s.total.to_string(),
        "total_display": crate::display::amount(s.total),
        "claimed": s.claimed.to_string(),
        "claimed_display": crate::display::amount(s.claimed),
        "claimable": claimable.to_string(),
        "claimable_display": crate::display::amount(claimable),
        "next_unlock_ms": s.next_unlock_after(now),
        "status": status(&s, now),
    }))
}

pub fn known_of(state: &ScheduleState, block: u64) -> Option<Known> {
    let schedule = state.schedule.as_ref()?;
    Some(Known {
        schedule: hex::encode(borsh::to_vec(schedule).ok()?),
        native: matches!(state.asset, Asset::Native),
        block,
    })
}

pub fn parse_b58(text: &str) -> Option<AccountId> {
    vesting_sdk::names::parse_address(text)
}
