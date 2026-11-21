//! The module's two backends behind one small trait.
//!
//! - feature `lez`: the SDK's `LezBackend`, a LEZ rc3 wallet and RPC from the settings. This is
//!   what the Basecamp package builds.
//! - without it: `JsonFileBackend`, the CLI's JSON-file ledger (schedules and events as hex borsh),
//!   with a fixed clock and a list of "wallet" accounts from the settings. It moves nothing and
//!   exists for the module's tests.

use vesting_core::AccountId;
use vesting_sdk::{Backend, ScheduleId};

/// One account of the connected wallet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalletAccount {
    pub id: AccountId,
    pub private: bool,
    /// Native LEZ balance (smallest unit).
    pub balance: u128,
}

/// What the module needs beyond the SDK's `Backend`.
pub trait ModuleBackend: Backend {
    /// The wallet's accounts (signing public accounts and held private accounts).
    fn accounts(&self) -> Result<Vec<WalletAccount>, String>;
    /// Give an existing on-chain schedule account a local id.
    fn import_account(&mut self, account: AccountId) -> Result<ScheduleId, String>;
    /// The on-chain account of a schedule id, when there is one.
    fn schedule_address(&self, id: ScheduleId) -> Option<AccountId>;
    /// Whether schedule account `account` already has a local id (`false` when unknown).
    fn knows(&self, _account: &AccountId) -> bool {
        false
    }
}

#[cfg(feature = "lez")]
impl ModuleBackend for vesting_sdk::LezBackend {
    fn accounts(&self) -> Result<Vec<WalletAccount>, String> {
        Ok(self
            .wallet_accounts()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|(id, private, balance)| WalletAccount {
                id,
                private,
                balance,
            })
            .collect())
    }

    fn import_account(&mut self, account: AccountId) -> Result<ScheduleId, String> {
        // Refuse an account that holds no schedule of this program before it gets an id.
        let id = vesting_sdk::LezBackend::import(self, account).map_err(|e| e.to_string())?;
        if let Err(error) = self.load(id) {
            return Err(format!(
                "{} is not a schedule of this vesting program: {error}",
                hex::encode(account)
            ));
        }
        Ok(id)
    }

    fn schedule_address(&self, id: ScheduleId) -> Option<AccountId> {
        vesting_sdk::LezBackend::address(self, id).ok()
    }
}

#[cfg(not(feature = "lez"))]
pub use json::JsonFileBackend;

#[cfg(not(feature = "lez"))]
mod json {
    use super::{ModuleBackend, WalletAccount};
    use borsh::BorshDeserialize;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use vesting_core::{AccountId, Event, Schedule, Timestamp};
    use vesting_sdk::{Backend, BackendError, ScheduleId};

    #[derive(Serialize, Deserialize, Default)]
    struct StateFile {
        next_id: ScheduleId,
        /// id -> hex(borsh(Schedule))
        schedules: BTreeMap<ScheduleId, String>,
        /// hex(borsh(Event)), in submission order
        events: Vec<String>,
    }

    /// The CLI's JSON-file ledger, for tests: no chain, no fees, a fixed clock.
    pub struct JsonFileBackend {
        path: PathBuf,
        now: Timestamp,
        state: StateFile,
        accounts: Vec<WalletAccount>,
    }

    impl JsonFileBackend {
        pub fn open(
            path: &Path,
            now_override: Option<Timestamp>,
            accounts: Vec<WalletAccount>,
        ) -> Result<Self, BackendError> {
            let state = match fs::read(path) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|error| BackendError::Decode(error.to_string()))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => StateFile::default(),
                Err(error) => return Err(BackendError::Io(error.to_string())),
            };
            let now = now_override.unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as Timestamp
            });
            Ok(Self {
                path: path.to_path_buf(),
                now,
                state,
                accounts,
            })
        }

        fn flush(&self) -> Result<(), BackendError> {
            let bytes = serde_json::to_vec_pretty(&self.state)
                .map_err(|error| BackendError::Decode(error.to_string()))?;
            fs::write(&self.path, bytes).map_err(|error| BackendError::Io(error.to_string()))
        }
    }

    impl Backend for JsonFileBackend {
        fn now(&self) -> Timestamp {
            self.now
        }

        fn load(&self, id: ScheduleId) -> Result<Schedule, BackendError> {
            let encoded = self
                .state
                .schedules
                .get(&id)
                .ok_or(BackendError::NotFound(id))?;
            let bytes = hex::decode(encoded).map_err(|e| BackendError::Decode(e.to_string()))?;
            Schedule::try_from_slice(&bytes).map_err(|e| BackendError::Decode(e.to_string()))
        }

        fn store(&mut self, id: ScheduleId, schedule: &Schedule) -> Result<(), BackendError> {
            let bytes = borsh::to_vec(schedule).map_err(|e| BackendError::Decode(e.to_string()))?;
            self.state.schedules.insert(id, hex::encode(bytes));
            self.flush()
        }

        fn next_id(&mut self) -> ScheduleId {
            let id = self.state.next_id;
            self.state.next_id = id + 1;
            id
        }

        fn list(&self) -> Vec<(ScheduleId, Schedule)> {
            self.state
                .schedules
                .iter()
                .filter_map(|(id, encoded)| {
                    let bytes = hex::decode(encoded).ok()?;
                    let schedule = Schedule::try_from_slice(&bytes).ok()?;
                    Some((*id, schedule))
                })
                .collect()
        }

        fn submit(&mut self, event: &Event) -> Result<(), BackendError> {
            let bytes = borsh::to_vec(event).map_err(|e| BackendError::Decode(e.to_string()))?;
            self.state.events.push(hex::encode(bytes));
            self.flush()
        }
    }

    impl ModuleBackend for JsonFileBackend {
        fn accounts(&self) -> Result<Vec<WalletAccount>, String> {
            Ok(self.accounts.clone())
        }

        fn import_account(&mut self, _account: AccountId) -> Result<ScheduleId, String> {
            Err("importing a schedule account needs the chain (the lez feature)".into())
        }

        fn schedule_address(&self, _id: ScheduleId) -> Option<AccountId> {
            None
        }
    }
}
