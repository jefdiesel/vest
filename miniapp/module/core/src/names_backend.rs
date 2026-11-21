//! Names Wallet as Vest's signer: the module reads schedules from the network itself (public
//! state, no wallet needed) and hands each write to the Names Wallet as a `propose_call`, which
//! the person approves in the Names app (docs/wallet/PROVIDER-SDK.md of the Names Wallet).
//!
//! What Names Wallet's `propose_call` can carry: one program, the chosen account's data for it,
//! other accounts' data for it, its own accounts (PDAs), LEZ balances and, since Names 0.18, the
//! network clock (another program's account, read only). A public claim of a native-LEZ schedule
//! fits (`ClaimAsOf`), and so do creating, cancelling, making irrevocable, signalling and
//! transferring a native-LEZ schedule, which read the clock (`create_call`, `op_call`). Token
//! schedules need the token program's accounts, and a private claim needs a public signer with a
//! private destination in one transaction: `propose_call` can't carry those, so this backend
//! refuses them with plain words.

use crate::backend::{ModuleBackend, WalletAccount};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vesting_core::{AccountId, Event, Schedule, Timestamp};
use vesting_program::{clock, schedule_seed, Asset, Instruction, NewSchedule, ScheduleState};
use vesting_sdk::names::{base58, RpcClient};
use vesting_sdk::{Backend, BackendError, ScheduleId};

/// What the module's direct writes answer with Names Wallet as the signer: those go through
/// Names Wallet as `propose_call`s (`create_call`, `claim_call`, `cancel_call`, ...), which the
/// person approves in the Names app.
pub const NOT_YET: &str = "With Names Wallet, Vest asks Names to sign each change: approve it in the Names app (create_call, claim_call, cancel_call, irrevocable_call, signal_call, transfer_call). To sign here instead, use a wallet folder (Settings, Advanced).";

/// What Names Wallet can't do for a token schedule yet.
const NO_TOKENS: &str = "Names Wallet can't sign for a token schedule yet: it reaches the token program's accounts, which Names Wallet's program calls don't carry. LEZ schedules work.";

/// An operation on an existing schedule that reads the clock, as Names Wallet signs it.
pub enum NamesOp {
    Cancel,
    MakeIrrevocable,
    SignalMilestone(u32),
    TransferBeneficiary(AccountId),
}

/// The network clock as Names Wallet 0.18's `propose_call` takes it: another program's account,
/// read only (`{"account", "program"}`).
pub fn clock_entry() -> Value {
    json!({
        "account": base58(&clock::CLOCK_PROGRAM_ACCOUNT_ID.into_value()),
        "program": base58(&clock::clock_account_id().into_value()),
    })
}

/// 32 random bytes: a new schedule's nonce.
pub fn random_nonce() -> Result<[u8; 32], String> {
    use std::io::Read as _;
    let mut out = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut out))
        .map_err(|e| format!("could not make a nonce: {e}"))?;
    Ok(out)
}

#[derive(Serialize, Deserialize, Default)]
struct IdMap {
    next: ScheduleId,
    /// Local id -> schedule account, hex.
    ids: BTreeMap<ScheduleId, String>,
}

pub struct NamesBackend {
    rpc: RpcClient,
    program: [u8; 32],
    map_path: PathBuf,
    map: IdMap,
    accounts: Vec<WalletAccount>,
}

fn io(error: impl ToString) -> BackendError {
    BackendError::Io(error.to_string())
}

fn lee(id: [u8; 32]) -> lee_core::account::AccountId {
    lee_core::account::AccountId::new(id)
}

impl NamesBackend {
    pub fn open(
        rpc: RpcClient,
        program: [u8; 32],
        map_path: &Path,
        accounts: Vec<WalletAccount>,
    ) -> Result<Self, BackendError> {
        let map = match std::fs::read(map_path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| BackendError::Decode(e.to_string()))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => IdMap::default(),
            Err(error) => return Err(io(error)),
        };
        Ok(Self {
            rpc,
            program,
            map_path: map_path.to_path_buf(),
            map,
            accounts,
        })
    }

    pub fn set_accounts(&mut self, accounts: Vec<WalletAccount>) {
        self.accounts = accounts;
    }

    fn persist(&self) -> Result<(), BackendError> {
        let bytes = serde_json::to_vec_pretty(&self.map)
            .map_err(|e| BackendError::Decode(e.to_string()))?;
        if let Some(parent) = self.map_path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&self.map_path, bytes).map_err(io)
    }

    fn address(&self, id: ScheduleId) -> Result<AccountId, BackendError> {
        let hex = self.map.ids.get(&id).ok_or(BackendError::NotFound(id))?;
        hex::decode(hex)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| BackendError::Decode("schedule address is not 32 bytes".into()))
    }

    /// The schedule account's whole state (asset, float, schedule).
    pub fn state_of(&self, address: &AccountId) -> Result<Option<ScheduleState>, BackendError> {
        let bytes = match self
            .rpc
            .get_actor_state(&base58(address), &base58(&self.program))
        {
            Ok(bytes) => bytes,
            // An account with nothing for this program.
            Err(error) if error.to_string().contains("no state for the program") => Vec::new(),
            Err(error) => return Err(io(error)),
        };
        if bytes.is_empty() {
            return Ok(None);
        }
        borsh::from_slice(&bytes)
            .map(Some)
            .map_err(|e| BackendError::Decode(format!("not a vesting schedule: {e}")))
    }

    /// The `propose_call` that claims, publicly, what schedule `id` has vested for `caller` (its
    /// beneficiary) at the network's time: `ClaimAsOf` with the schedule's data, its LEZ and the
    /// caller's LEZ. Native-LEZ schedules only.
    pub fn claim_call(&self, id: ScheduleId, caller: AccountId) -> Result<Value, String> {
        let address = self.address(id).map_err(|e| e.to_string())?;
        let state = self
            .state_of(&address)
            .map_err(|e| e.to_string())?
            .ok_or("this schedule isn't on the network")?;
        if !matches!(state.asset, Asset::Native) {
            return Err("Names Wallet can't claim a token schedule for Vest yet: the claim reaches the token program's accounts, which Names Wallet's program calls don't carry. LEZ schedules can be claimed.".into());
        }
        let schedule = state
            .schedule
            .ok_or("this schedule is still being created; try again in a moment")?;
        let now = self.now();
        if now == 0 {
            return Err("could not read the network's clock".into());
        }
        let (_, amount, event) =
            vesting_core::claim(&schedule, caller, now, false).map_err(|e| e.message())?;
        let Event::Claimed { at, .. } = event else {
            return Err("unexpected claim".into());
        };
        let instruction = Instruction::ClaimAsOf {
            schedule: lee(address),
            to: lee(caller),
            to_private: false,
            as_of: at,
        };
        let message = borsh::to_vec(&instruction).map_err(|e| e.to_string())?;
        let s = base58(&address);
        let accounts =
            json!([s, { "account": s, "balance": true }, { "account": "signer", "balance": true }]);
        Ok(json!({
            "program": base58(&self.program),
            "accounts_json": accounts.to_string(),
            "instruction_hex": hex::encode(message),
            "amount": amount.to_string(),
            "caller": format!("Public/{}", base58(&caller)),
            "schedule_address": hex::encode(address),
            "as_of": at,
        }))
    }
}

impl NamesBackend {
    /// The `propose_call` that creates one native-LEZ schedule from `params.creator` (the account
    /// the person must choose): `Create` with the creator's LEZ (the total moves out of it), the
    /// schedule's account (made, from its seed) and its LEZ, and the network clock. `nonce` picks
    /// the schedule's address. Checked here first with the network's time, as the program will.
    pub fn create_call(
        &self,
        params: vesting_core::CreateParams,
        nonce: [u8; 32],
    ) -> Result<Value, String> {
        if params.token != [0u8; 32] {
            return Err(NO_TOKENS.into());
        }
        let now = self.now();
        if now == 0 {
            return Err("could not read the network's clock".into());
        }
        vesting_core::create(params.clone(), now).map_err(|e| e.message())?;
        let creator = lee(params.creator);
        let program = lee(self.program);
        let seed = schedule_seed(&creator, &nonce);
        let address = lee_core::account::AccountId::for_public_pda(&program, &seed).into_value();
        if self.state_of(&address).map_err(|e| e.to_string())?.is_some() {
            return Err("a schedule with this nonce exists already; ask again".into());
        }
        let schedule = NewSchedule {
            nonce,
            beneficiary: lee(params.beneficiary),
            asset: Asset::Native,
            total: params.total,
            kind: params.kind,
            cancelable: params.cancelable,
            transferable: params.transferable,
            cancel_authority: params.cancel_authority.map(lee),
            milestone_authority: params.milestone_authority.map(lee),
            sponsor: None,
        };
        let message = borsh::to_vec(&Instruction::Create {
            schedules: vec![schedule],
        })
        .map_err(|e| e.to_string())?;
        let (s, seed) = (base58(&address), hex::encode(seed.as_ref()));
        let accounts = json!([
            { "account": "signer", "balance": true },
            { "account": s, "seed": seed },
            { "account": s, "seed": seed, "balance": true },
            clock_entry(),
        ]);
        Ok(json!({
            "program": base58(&self.program),
            "accounts_json": accounts.to_string(),
            "instruction_hex": hex::encode(message),
            "amount": params.total.to_string(),
            "caller": format!("Public/{}", base58(&params.creator)),
            "schedule_address": s,
            "schedule_address_hex": hex::encode(address),
        }))
    }

    /// The `propose_call` for `op` on native-LEZ schedule `id` by `caller` (the account the
    /// person must choose), checked first at the network's time as the program will run it. The
    /// schedule's data and LEZ, the creator's LEZ for a cancel's refund, and the clock.
    pub fn op_call(&self, id: ScheduleId, caller: AccountId, op: &NamesOp) -> Result<Value, String> {
        let address = self.address(id).map_err(|e| e.to_string())?;
        let state = self
            .state_of(&address)
            .map_err(|e| e.to_string())?
            .ok_or("this schedule isn't on the network")?;
        if !matches!(state.asset, Asset::Native) {
            return Err(NO_TOKENS.into());
        }
        if state.pending.is_some() {
            return Err("this schedule is still being changed; try again in a moment".into());
        }
        let schedule = state
            .schedule
            .ok_or("this schedule is still being created; try again in a moment")?;
        let now = self.now();
        if now == 0 {
            return Err("could not read the network's clock".into());
        }
        let schedule_id = lee(address);
        let s = base58(&address);
        let mut accounts = vec![json!(s), json!({ "account": s, "balance": true })];
        let mut amount = 0u128;
        let instruction = match op {
            NamesOp::Cancel => {
                let (_, refund, _) =
                    vesting_core::cancel(&schedule, caller, now).map_err(|e| e.message())?;
                amount = refund;
                // The refund goes to the creator: the chosen account's LEZ when it is the creator.
                accounts.push(if schedule.creator == caller {
                    json!({ "account": "signer", "balance": true })
                } else {
                    json!({ "account": base58(&schedule.creator), "balance": true })
                });
                Instruction::Cancel { schedule: schedule_id }
            }
            NamesOp::MakeIrrevocable => {
                vesting_core::make_irrevocable(&schedule, caller, now).map_err(|e| e.message())?;
                Instruction::MakeIrrevocable { schedule: schedule_id }
            }
            NamesOp::SignalMilestone(index) => {
                let (_, unlocked, _) =
                    vesting_core::signal_milestone(&schedule, caller, *index as usize, now)
                        .map_err(|e| e.message())?;
                amount = unlocked;
                Instruction::SignalMilestone { schedule: schedule_id, index: *index }
            }
            NamesOp::TransferBeneficiary(to) => {
                vesting_core::transfer_beneficiary(&schedule, caller, *to, now)
                    .map_err(|e| e.message())?;
                Instruction::TransferBeneficiary { schedule: schedule_id, new_beneficiary: lee(*to) }
            }
        };
        accounts.push(clock_entry());
        let message = borsh::to_vec(&instruction).map_err(|e| e.to_string())?;
        Ok(json!({
            "program": base58(&self.program),
            "accounts_json": Value::Array(accounts).to_string(),
            "instruction_hex": hex::encode(message),
            "amount": amount.to_string(),
            "caller": format!("Public/{}", base58(&caller)),
            "schedule_address": s,
        }))
    }
}

impl Backend for NamesBackend {
    /// The network's clock (the last block's timestamp); 0 when it can't be read.
    fn now(&self) -> Timestamp {
        let clock_account = clock::CLOCK_PROGRAM_ACCOUNT_ID.into_value();
        let clock_program = clock::clock_account_id().into_value();
        self.rpc
            .get_actor_state(&base58(&clock_account), &base58(&clock_program))
            .ok()
            .and_then(|bytes| borsh::from_slice::<(u64, u64)>(&bytes).ok())
            .map_or(0, |(_, timestamp)| timestamp)
    }

    fn load(&self, id: ScheduleId) -> Result<Schedule, BackendError> {
        let address = self.address(id)?;
        self.state_of(&address)?
            .ok_or(BackendError::NotFound(id))?
            .schedule
            .ok_or_else(|| BackendError::Decode("schedule is being created".into()))
    }

    fn store(&mut self, _id: ScheduleId, _state: &Schedule) -> Result<(), BackendError> {
        Err(io(NOT_YET))
    }

    fn next_id(&mut self) -> ScheduleId {
        let id = self.map.next;
        self.map.next += 1;
        id
    }

    fn list(&self) -> Vec<(ScheduleId, Schedule)> {
        self.map
            .ids
            .keys()
            .filter_map(|id| self.load(*id).ok().map(|state| (*id, state)))
            .collect()
    }

    fn submit(&mut self, _event: &Event) -> Result<(), BackendError> {
        Err(io(NOT_YET))
    }

    fn submit_batch(
        &mut self,
        _items: &[(ScheduleId, Schedule, Event)],
    ) -> Result<(), BackendError> {
        Err(io(NOT_YET))
    }
}

impl ModuleBackend for NamesBackend {
    fn accounts(&self) -> Result<Vec<WalletAccount>, String> {
        Ok(self.accounts.clone())
    }

    fn import_account(&mut self, account: AccountId) -> Result<ScheduleId, String> {
        let hex = hex::encode(account);
        if let Some((id, _)) = self.map.ids.iter().find(|(_, known)| **known == hex) {
            return Ok(*id);
        }
        match self.state_of(&account).map_err(|e| e.to_string())? {
            Some(state) if state.schedule.is_some() => {}
            _ => {
                return Err(format!(
                    "{} is not a schedule of this Vest program",
                    base58(&account)
                ))
            }
        }
        let id = self.next_id();
        self.map.ids.insert(id, hex);
        self.persist().map_err(|e| e.to_string())?;
        Ok(id)
    }

    fn schedule_address(&self, id: ScheduleId) -> Option<AccountId> {
        self.address(id).ok()
    }
}
