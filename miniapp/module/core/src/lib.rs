//! The Vest Basecamp module, as plain Rust: every call takes and returns JSON text.
//!
//! `../rust-lib` is the Logos module itself (the `VestingModule` trait the builder turns into
//! a Basecamp plugin); each of its methods is one call of [`Module`] here, so everything the
//! mini-app does is tested in this crate without Basecamp.
//!
//! Each schedule call is one `vesting_sdk::Client` call. The acting account is never taken from
//! the view: it is the schedule's own beneficiary (claim, transfer), cancel authority (cancel,
//! make irrevocable) or milestone authority (signal), and the LEZ wallet must hold its key.
//! Accounts may be typed as hex, base58 or an LNS name; names are resolved through the LNS
//! registry of the network (as the CLI does) and the answer says what each name resolved to.
//!
//! Answers are `{"ok":true,...}` or `{"ok":false,"error":"...","code":<n>?}` with the SDK's own
//! message and code. Amounts are decimal strings with a ready `*_display` string next to them.

mod backend;
pub mod display;
mod names_backend;
pub mod network;
mod wire;

pub use backend::{ModuleBackend, WalletAccount};

use network::Network;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use vesting_core::{AccountId, Schedule};
use vesting_sdk::names::{self, NameBook, RpcClient};
use vesting_sdk::{AccountKind, AccountRef, Client, FeePayer, PreClaimSummary, SdkError};

pub use names_backend::NOT_YET as NAMES_NOT_YET;

/// Who signs: the person's Names Wallet (the default in the app), or a wallet folder this module
/// opens itself (Settings, Advanced; the JSON test ledger in builds without `lez`).
pub enum AppBackend {
    #[cfg(feature = "lez")]
    Wallet(vesting_sdk::LezBackend),
    #[cfg(not(feature = "lez"))]
    Wallet(backend::JsonFileBackend),
    Names(names_backend::NamesBackend),
}

macro_rules! each {
    ($self:expr, $b:ident => $e:expr) => {
        match $self {
            AppBackend::Wallet($b) => $e,
            AppBackend::Names($b) => $e,
        }
    };
}

impl vesting_sdk::Backend for AppBackend {
    fn now(&self) -> vesting_core::Timestamp {
        each!(self, b => b.now())
    }
    fn load(&self, id: u64) -> Result<Schedule, vesting_sdk::BackendError> {
        each!(self, b => b.load(id))
    }
    fn store(&mut self, id: u64, s: &Schedule) -> Result<(), vesting_sdk::BackendError> {
        each!(self, b => b.store(id, s))
    }
    fn next_id(&mut self) -> u64 {
        each!(self, b => b.next_id())
    }
    fn list(&self) -> Vec<(u64, Schedule)> {
        each!(self, b => b.list())
    }
    fn submit(&mut self, e: &vesting_core::Event) -> Result<(), vesting_sdk::BackendError> {
        each!(self, b => b.submit(e))
    }
    fn set_claim_target(&mut self, t: [u8; 32]) {
        each!(self, b => b.set_claim_target(t))
    }
    fn set_caller(&mut self, c: [u8; 32]) {
        each!(self, b => b.set_caller(c))
    }
    fn estimate_fee(
        &mut self,
        id: u64,
        s: &Schedule,
        e: &vesting_core::Event,
    ) -> Result<Option<vesting_sdk::FeeEstimate>, vesting_sdk::BackendError> {
        each!(self, b => b.estimate_fee(id, s, e))
    }
    fn submit_batch(
        &mut self,
        items: &[(u64, Schedule, vesting_core::Event)],
    ) -> Result<(), vesting_sdk::BackendError> {
        each!(self, b => b.submit_batch(items))
    }
}

impl ModuleBackend for AppBackend {
    fn accounts(&self) -> Result<Vec<WalletAccount>, String> {
        each!(self, b => b.accounts())
    }
    fn import_account(&mut self, a: AccountId) -> Result<u64, String> {
        each!(self, b => b.import_account(a))
    }
    fn schedule_address(&self, id: u64) -> Option<AccountId> {
        each!(self, b => b.schedule_address(id))
    }
}

/// The signer when the settings name none: Names Wallet in the app; the JSON ledger in tests.
#[cfg(feature = "lez")]
const DEFAULT_SIGNER: &str = "names";
#[cfg(not(feature = "lez"))]
const DEFAULT_SIGNER: &str = "wallet";

/// Settings, saved as `settings.json` in the module's data folder. Empty strings mean "use
/// the network's value" (or "none"). The wallet password is never saved.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Who signs: "names" (Names Wallet) or "wallet" (the wallet folder below). Empty: the default.
    pub signer: String,
    /// A LEZ rc3 wallet folder (`wallet_config.json` + `storage.json`), for signer "wallet".
    pub wallet_dir: String,
    /// A `names-network` file; empty: the built-in profile (the public rc3 demo chain).
    pub network_file: String,
    /// Overrides of single network fields.
    pub rpc: String,
    pub chain_id: String,
    pub program: String,
    pub token_program: String,
    pub lns_registry: String,
    /// The clock program's ELF, if the chain doesn't serve it (private claims need it).
    pub clock_elf: String,
    pub decimals: Option<u8>,
    /// Token schedules: the creator's fee float for private claims (decimal strings).
    pub sponsor_deposit: String,
    pub sponsor_per_claim: String,
    /// The accounts (Names Wallet's `Public/<id>`) whose person has finished the first-run
    /// guide: the app folds its guides away for them.
    pub guide_done: Vec<String>,
    /// Tests only (the JSON backend): ledger file, fixed clock and the "wallet" accounts.
    pub state_path: String,
    pub now: Option<u64>,
    pub accounts: Vec<TestAccount>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestAccount {
    pub id: String,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub balance: String,
}

pub struct Module {
    dir: PathBuf,
    settings: Settings,
    password: Option<String>,
    network: Option<Network>,
    client: Option<Client<AppBackend>>,
    /// Why the backend is not open, when it isn't.
    status: String,
    book: NameBook,
    /// LNS lookups; tests put a mock here.
    lns_client: Option<RpcClient>,
    /// The local relay in front of a public RPC: pacing, retries, plain errors.
    guard: Option<vesting_sdk::rpc_guard::RpcGuard>,
    /// The wallet's accounts as last read, kept a few seconds so one refresh reads them once.
    accounts_cache: Option<(std::time::Instant, Vec<WalletAccount>)>,
    /// Tests: put the relay in front of a local RPC too, with this policy.
    guard_local: Option<vesting_sdk::rpc_guard::Policy>,
    caching: bool,
    /// The accounts Names Wallet gave this app (`granted_accounts` + `account_balances`).
    names_accounts: Vec<WalletAccount>,
    /// Tests: the chain reads of the Names signer go here instead of the network.
    chain_client: Option<RpcClient>,
}

fn ok(mut value: Value) -> String {
    value["ok"] = json!(true);
    value.to_string()
}

/// A failure answer in plain words: the RPC relay's words when a network problem is behind it
/// (busy, unreachable), never a response body; the raw detail (status and at most 200
/// characters, tags removed) goes in `detail`, for Technical details.
fn fail(message: impl Into<String>) -> String {
    let (words, detail) = vesting_sdk::rpc_guard::plain_error(&message.into());
    let mut answer = json!({ "ok": false, "error": words });
    if let Some(detail) = detail {
        answer["detail"] = json!(detail);
    }
    answer.to_string()
}

fn sdk_fail(error: SdkError) -> String {
    let mut answer: Value = serde_json::from_str(&fail(error.message())).unwrap_or_default();
    answer["code"] = json!(error.code());
    answer.to_string()
}

fn b58(account: &AccountId) -> String {
    names::base58(account)
}

/// A schedule id or tranche index as the module's interface takes it: decimal text (Basecamp's
/// call checker refuses a QML number for an integer parameter, so every id travels as a string).
pub fn parse_index(field: &str, text: &str) -> Result<u64, String> {
    text.trim()
        .parse::<u64>()
        .map_err(|_| format!("{field} must be a whole number, got '{text}'"))
}

/// The failure answer for input the module refused before any call.
pub fn bad_input(message: impl Into<String>) -> String {
    fail(message)
}

/// Run `body`, turning an `Err(text)` into a failure answer.
fn answer(body: impl FnOnce() -> Result<String, String>) -> String {
    body().unwrap_or_else(fail)
}

impl Module {
    /// A module keeping its settings, id map and name book in `dir` (Basecamp's persistence
    /// folder for this module). Nothing is opened until the first call that needs the chain.
    pub fn new(dir: &Path) -> Self {
        let settings = std::fs::read(dir.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let book = NameBook::open(&dir.join("vesting-names.json")).unwrap_or_default();
        Self {
            dir: dir.to_path_buf(),
            settings,
            password: None,
            network: None,
            client: None,
            status: "not opened yet".into(),
            book,
            lns_client: None,
            guard: None,
            accounts_cache: None,
            guard_local: None,
            caching: true,
            names_accounts: Vec::new(),
            chain_client: None,
        }
    }

    /// Measurements: turn the module's and the backend's read caches off (`false`) or on.
    #[doc(hidden)]
    pub fn set_caching(&mut self, on: bool) {
        self.caching = on;
        self.accounts_cache = None;
        #[cfg(feature = "lez")]
        if let Some(AppBackend::Wallet(backend)) = self.client.as_mut().map(|c| c.backend_mut()) {
            backend.set_caching(on);
        }
    }

    /// Measurements: what the relay sent, per method, and how many were refused.
    #[doc(hidden)]
    pub fn network_requests(&self) -> Option<(std::collections::BTreeMap<String, u64>, u64)> {
        self.guard.as_ref().map(|g| g.stats())
    }

    /// Tests: the Names signer reads the chain through this client (used at the next open).
    pub fn set_chain_client(&mut self, client: RpcClient) {
        self.chain_client = Some(client);
    }

    /// Tests: route even a local RPC through the relay, with `policy`.
    pub fn guard_local_rpc(&mut self, policy: vesting_sdk::rpc_guard::Policy) {
        self.guard_local = Some(policy);
    }

    /// Tests: answer LNS lookups from this client instead of the network's RPC.
    pub fn set_lns_client(&mut self, client: RpcClient) {
        self.lns_client = Some(client);
    }

    fn effective_network(&self) -> Result<Network, String> {
        let s = &self.settings;
        let mut net = if s.network_file.trim().is_empty() {
            network::built_in()
        } else {
            let path = s.network_file.trim();
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            network::parse(&text, path)?
        };
        let id = |text: &str, field: &str| -> Result<Option<AccountId>, String> {
            if text.trim().is_empty() {
                return Ok(None);
            }
            names::parse_address(text)
                .map(Some)
                .ok_or_else(|| format!("{field}: '{text}' is not a 32-byte account"))
        };
        if !s.rpc.trim().is_empty() {
            net.rpc = s.rpc.trim().to_string();
            // Another RPC is another chain unless a chain id says otherwise.
            net.chain_id = String::new();
        }
        if !s.chain_id.trim().is_empty() {
            net.chain_id = s.chain_id.trim().to_lowercase();
        }
        if let Some(program) = id(&s.program, "program")? {
            net.program = Some(program);
        }
        if let Some(program) = id(&s.token_program, "token_program")? {
            net.token_program = Some(program);
        }
        if let Some(program) = id(&s.lns_registry, "lns_registry")? {
            net.lns_registry = Some(program);
        }
        Ok(net)
    }

    /// Where requests for `rpc` go: through the relay for a public RPC (started once per RPC),
    /// straight to a local one.
    fn rpc_url(&mut self, rpc: &str) -> Result<String, String> {
        if network::is_loopback(rpc) && self.guard_local.is_none() {
            return Ok(rpc.to_string());
        }
        if let Some(guard) = &self.guard {
            if guard.upstream() == rpc {
                return Ok(guard.url().to_string());
            }
        }
        #[cfg(feature = "lez")]
        let hook = Some(vesting_sdk::lez::tx_hash_hook());
        #[cfg(not(feature = "lez"))]
        let hook = None;
        let guard = vesting_sdk::rpc_guard::RpcGuard::start(
            rpc,
            self.guard_local.unwrap_or_default(),
            hook,
        )
        .map_err(|e| format!("could not start the network relay: {e}"))?;
        let url = guard.url().to_string();
        self.guard = Some(guard);
        Ok(url)
    }

    /// "names" or "wallet".
    pub fn signer(&self) -> &str {
        match self.settings.signer.trim() {
            "" => DEFAULT_SIGNER,
            other => other,
        }
    }

    fn names_mode(&self) -> bool {
        self.signer() == "names"
    }

    /// The chain check: the RPC must serve the network's chain (a local RPC may go unchecked).
    fn check_chain(&mut self, net: &Network) -> Result<(), String> {
        if net.rpc.is_empty() {
            return Err("no RPC: set one, or a network file".into());
        }
        if net.chain_id.is_empty() {
            if !network::is_loopback(&net.rpc) {
                return Err(format!(
                    "{} is not a local RPC: give its chain id (or a network file)",
                    net.rpc
                ));
            }
            return Ok(());
        }
        let via = self.rpc_url(&net.rpc)?;
        let served = network::channel_id(&names::http_client(&via))?;
        if served != net.chain_id {
            return Err(format!(
                "{} serves chain {served}, not {}: check the RPC and the network",
                net.rpc, net.chain_id
            ));
        }
        Ok(())
    }

    /// Open the backend from the settings (the chain check first).
    fn open(&mut self) -> Result<(), String> {
        let net = self.effective_network()?;
        if let Some(decimals) = self.settings.decimals {
            display::set_decimals(decimals);
        }
        if self.names_mode() {
            self.check_chain(&net)?;
            let program = net
                .program
                .ok_or("this network names no vesting program: set one in Settings")?;
            let reads = match self.chain_client.take() {
                Some(client) => client,
                None => names::http_client(&self.rpc_url(&net.rpc)?),
            };
            let backend = names_backend::NamesBackend::open(
                reads,
                program,
                &self.dir.join("vesting-names-ids.json"),
                self.names_accounts.clone(),
            )
            .map_err(|e| e.to_string())?;
            self.network = Some(net);
            self.client = Some(Client::new(AppBackend::Names(backend)));
            self.status = "ready".into();
            return Ok(());
        }
        #[cfg(feature = "lez")]
        let client = {
            use std::path::PathBuf;
            self.check_chain(&net)?;
            let wallet_dir = self.settings.wallet_dir.trim().to_string();
            if wallet_dir.is_empty() {
                return Err("set the wallet folder in Settings, Advanced".into());
            }
            let program = net
                .program
                .ok_or("this network names no vesting program: set one in Settings")?;
            let clock_elf = Some(self.settings.clock_elf.trim())
                .filter(|p| !p.is_empty())
                .map(PathBuf::from);
            let via = self.rpc_url(&net.rpc)?;
            let mut backend = vesting_sdk::LezBackend::open_with_password(
                &via,
                Path::new(&wallet_dir),
                program,
                clock_elf,
                &self.dir.join("vesting-lez-ids.json"),
                self.password.clone(),
            )
            .map_err(|e| e.to_string())?;
            if let Some(token_program) = net.token_program {
                backend.set_token_program(token_program);
            }
            let amount = |text: &str| text.trim().parse::<u128>().ok();
            if let (Some(deposit), Some(per_claim)) = (
                amount(&self.settings.sponsor_deposit),
                amount(&self.settings.sponsor_per_claim),
            ) {
                backend.set_sponsor(deposit, per_claim);
            }
            Client::new(AppBackend::Wallet(backend))
        };
        #[cfg(not(feature = "lez"))]
        let client = {
            let path = if self.settings.state_path.trim().is_empty() {
                self.dir.join("vesting-state.json")
            } else {
                PathBuf::from(self.settings.state_path.trim())
            };
            let mut accounts = Vec::new();
            for account in &self.settings.accounts {
                accounts.push(WalletAccount {
                    id: names::parse_address(&account.id)
                        .ok_or_else(|| format!("accounts: '{}' is not an account", account.id))?,
                    private: account.private,
                    balance: account.balance.parse().unwrap_or(0),
                });
            }
            Client::new(AppBackend::Wallet(
                backend::JsonFileBackend::open(&path, self.settings.now, accounts)
                    .map_err(|e| e.to_string())?,
            ))
        };
        self.network = Some(net);
        self.client = Some(client);
        self.status = "ready".into();
        Ok(())
    }

    fn client(&mut self) -> Result<&mut Client<AppBackend>, String> {
        if self.client.is_none() {
            if let Err(error) = self.open() {
                self.status = error.clone();
                return Err(format!("Not connected: {error}"));
            }
        }
        Ok(self.client.as_mut().expect("opened"))
    }

    fn save_book(&self) {
        let _ = self.book.save(&self.dir.join("vesting-names.json"));
    }

    /// Resolve an account typed by a person: hex, base58 or an LNS name.
    fn resolve(&mut self, text: &str) -> Result<(AccountId, Option<String>), String> {
        if let Some(account) = names::parse_address(text) {
            return Ok((account, None));
        }
        let net = match &self.network {
            Some(net) => net.clone(),
            None => self.effective_network()?,
        };
        let via = if self.lns_client.is_none() {
            self.rpc_url(&net.rpc)?
        } else {
            String::new()
        };
        let built;
        let client: &RpcClient = match &self.lns_client {
            Some(client) => client,
            None => {
                built = names::http_client(&via);
                &built
            }
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let resolved = names::resolve(Some(client), net.lns_registry, &mut self.book, text, now)
            .map_err(|e| e.to_string())?;
        self.save_book();
        Ok(resolved)
    }

    /// Replace every account field of a create row by its address; returns what names resolved to.
    fn resolve_fields(&mut self, row: &mut Value, resolved: &mut Vec<Value>) -> Result<(), String> {
        for field in [
            "creator",
            "beneficiary",
            "token",
            "cancel_authority",
            "milestone_authority",
        ] {
            let Some(text) = row.get(field).and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            let text = text.trim();
            let account = if field == "token"
                && (text.is_empty() || text.eq_ignore_ascii_case("lez") || text == "native")
            {
                [0u8; 32]
            } else if text.is_empty() && field.ends_with("authority") {
                row[field] = Value::Null;
                continue;
            } else {
                let (account, name) = self.resolve(text).map_err(|e| format!("{field}: {e}"))?;
                if let Some(name) = name {
                    resolved.push(
                        json!({ "name": name, "account": hex::encode(account), "base58": b58(&account) }),
                    );
                }
                account
            };
            row[field] = json!(hex::encode(account));
        }
        Ok(())
    }

    fn wallet_accounts(&mut self) -> Result<Vec<WalletAccount>, String> {
        if let Some((at, accounts)) = &self.accounts_cache {
            if at.elapsed() < std::time::Duration::from_secs(10) {
                return Ok(accounts.clone());
            }
        }
        let accounts = self.client()?.backend().accounts()?;
        if self.caching {
            self.accounts_cache = Some((std::time::Instant::now(), accounts.clone()));
        }
        Ok(accounts)
    }

    /// After a write: balances and roles may have changed.
    fn forget_reads(&mut self) {
        self.accounts_cache = None;
    }

    fn tx(&self) -> Value {
        #[cfg(feature = "lez")]
        if let Some(AppBackend::Wallet(backend)) = self.client.as_ref().map(|c| c.backend()) {
            if let Some(last) = backend.last_tx() {
                return json!({ "hash": last.hash, "block": last.block });
            }
        }
        Value::Null
    }

    fn schedule(&mut self, id: u64) -> Result<Schedule, String> {
        self.client()?.show(id).map_err(|e| e.message())
    }

    // ── Calls ───────────────────────────────────────────────────────────────────────────

    /// The settings (password left out), the network in use and whether it is open.
    pub fn settings(&mut self) -> String {
        let network = self.effective_network();
        let mut settings = serde_json::to_value(&self.settings).unwrap_or_default();
        if let Some(map) = settings.as_object_mut() {
            map.remove("accounts");
        }
        let net = match network {
            Ok(net) => json!({
                "name": net.name, "source": net.source, "rpc": net.rpc, "chain_id": net.chain_id,
                "program": net.program.map(|p| b58(&p)),
                "token_program": net.token_program.map(|p| b58(&p)),
                "lns_registry": net.lns_registry.map(|p| b58(&p)),
            }),
            Err(error) => json!({ "error": error }),
        };
        ok(json!({
            "settings": settings,
            "network": net,
            "connected": self.client.is_some(),
            "status": self.status,
            "has_password": self.password.is_some(),
            "signer": self.signer(),
            "chain": cfg!(feature = "lez"),
            "data_dir": self.dir.display().to_string(),
            "network_requests": self.guard.as_ref().map(|g| {
                let (sent, refused) = g.stats();
                json!({ "by_method": sent, "total": sent.values().sum::<u64>(), "refused": refused })
            }),
        }))
    }

    /// Change settings (a JSON object of `Settings` fields, plus `password`, kept in memory only),
    /// save them and reconnect. Answers as `settings`, with the connection's error if any.
    pub fn configure(&mut self, input: &str) -> String {
        let patch: Value = match serde_json::from_str(input) {
            Ok(value) => value,
            Err(e) => return fail(format!("invalid JSON: {e}")),
        };
        let mut merged = serde_json::to_value(&self.settings).unwrap_or_default();
        if let (Some(target), Some(source)) = (merged.as_object_mut(), patch.as_object()) {
            for (key, value) in source {
                if key == "password" {
                    self.password = value.as_str().filter(|p| !p.is_empty()).map(str::to_string);
                } else {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
        match serde_json::from_value::<Settings>(merged) {
            Ok(settings) => self.settings = settings,
            Err(e) => return fail(format!("settings: {e}")),
        }
        if let Err(e) = std::fs::create_dir_all(&self.dir).and_then(|_| {
            std::fs::write(
                self.dir.join("settings.json"),
                serde_json::to_vec_pretty(&self.settings).unwrap_or_default(),
            )
        }) {
            return fail(format!("could not save the settings: {e}"));
        }
        self.client = None;
        self.network = None;
        match self.open() {
            Ok(()) => self.settings(),
            Err(error) => {
                self.status = error.clone();
                let mut answer: Value = serde_json::from_str(&self.settings()).unwrap_or_default();
                answer["ok"] = json!(false);
                answer["error"] = json!(format!("Saved, but not connected: {error}"));
                answer.to_string()
            }
        }
    }

    /// The wallet's accounts with their native balances.
    pub fn accounts(&mut self) -> String {
        answer(|| {
            let accounts = self.wallet_accounts()?;
            let rows: Vec<Value> = accounts
                .iter()
                .map(|a| {
                    json!({
                        "id": hex::encode(a.id),
                        "base58": b58(&a.id),
                        "private": a.private,
                        "balance": a.balance.to_string(),
                        "balance_display": display::amount(a.balance),
                        "name": self.book.label(&a.id),
                    })
                })
                .collect();
            Ok(ok(json!({ "accounts": rows })))
        })
    }

    /// `Client::list()`: every schedule this module knows, with which roles the wallet holds.
    pub fn list(&mut self) -> String {
        answer(|| {
            let mine: Vec<AccountId> = self.wallet_accounts()?.iter().map(|a| a.id).collect();
            let client = self.client()?;
            let now = client.now();
            let schedules: Vec<(u64, Schedule, Option<AccountId>)> = client
                .list()
                .into_iter()
                .map(|(id, state)| {
                    let address = client.backend().schedule_address(id);
                    (id, state, address)
                })
                .collect();
            let mut labels = serde_json::Map::new();
            let rows: Vec<Value> = schedules
                .iter()
                .map(|(id, state, address)| {
                    for account in [
                        state.creator,
                        state.beneficiary,
                        state.cancel_authority,
                        state.milestone_authority,
                    ] {
                        if let Some(name) = self.book.label(&account) {
                            labels.insert(hex::encode(account), json!(name));
                        }
                    }
                    let mut row = wire::schedule_json(*id, state, now);
                    row["address"] = json!(address.map(hex::encode));
                    row["mine"] = json!({
                        "beneficiary": mine.contains(&state.beneficiary),
                        "creator": mine.contains(&state.creator),
                        "cancel_authority": mine.contains(&state.cancel_authority),
                        "milestone_authority": mine.contains(&state.milestone_authority),
                    });
                    row
                })
                .collect();
            Ok(ok(
                json!({ "time": now, "schedules": rows, "names": labels }),
            ))
        })
    }

    /// `Client::now()`: the time all display maths use (the chain's clock on LEZ).
    pub fn now(&mut self) -> String {
        answer(|| {
            let now = self.client()?.now();
            if now == 0 {
                return Err("could not read the chain's clock".into());
            }
            Ok(ok(json!({ "time": now })))
        })
    }

    /// `Client::pre_claim_summary`: claimable now, the simulated fee of the claim the
    /// beneficiary would send, and whether its payer can pay it. `private_target` empty: a
    /// public claim.
    pub fn pre_claim_summary(&mut self, id: u64, private_target: &str) -> String {
        answer(|| {
            let summary = self.summary(id, private_target)?;
            let fee = summary.fee.as_ref();
            let amount = |value: Option<u128>| value.map(|v| v.to_string());
            let shown = |value: Option<u128>| value.map(display::amount);
            Ok(ok(json!({ "summary": {
                "schedule": summary.schedule,
                "claimable": summary.claimable.to_string(),
                "claimable_display": display::amount(summary.claimable),
                "estimated_fee": summary.estimated_fee.to_string(),
                "estimated_fee_display": display::amount(summary.estimated_fee),
                "fee_simulated": fee.is_some(),
                "fee_payer": fee.map(|f| match f.payer {
                    FeePayer::Caller => "caller",
                    FeePayer::ClaimTarget => "claim_target",
                }),
                "fee_reserve": amount(fee.map(|f| f.reserve)),
                "fee_reserve_display": shown(fee.map(|f| f.reserve)),
                "fee_available": amount(fee.and_then(|f| f.available)),
                "fee_available_display": shown(fee.and_then(|f| f.available)),
                "fee_covered": summary.fee_covered,
                "revert_reason": fee.and_then(|f| f.revert_reason.clone()),
                "next_unlock": summary.next_unlock,
                "destination_kind": if summary.destination_kind == AccountKind::Private { "private" } else { "public" },
            }})))
        })
    }

    fn summary(&mut self, id: u64, private_target: &str) -> Result<PreClaimSummary, String> {
        let state = self.schedule(id)?;
        let destination = if private_target.trim().is_empty() {
            AccountRef::public(state.beneficiary)
        } else {
            let (target, _) = self.resolve(private_target.trim())?;
            AccountRef::private(target)
        };
        self.client()?
            .pre_claim_summary(id, state.beneficiary, destination)
            .map_err(|e| e.message())
    }

    /// `Client::create`. Input: one schedule (`creator`, `beneficiary`, `token` ("LEZ" or a
    /// token definition), `total`, `kind`, flags, authorities); accounts may be names.
    pub fn create(&mut self, input: &str) -> String {
        self.forget_reads();
        if self.names_mode() {
            return fail(NAMES_NOT_YET);
        }
        answer(|| {
            let mut row: Value =
                serde_json::from_str(input).map_err(|e| format!("invalid JSON: {e}"))?;
            let mut resolved = Vec::new();
            self.resolve_fields(&mut row, &mut resolved)?;
            let params = wire::create_params(&row)?;
            let client = self.client()?;
            match client.create(params) {
                Ok((id, state, _)) => {
                    let now = client.now();
                    Ok(ok(json!({
                        "id": id, "schedule": wire::schedule_json(id, &state, now),
                        "resolved": resolved, "tx": self.tx(),
                    })))
                }
                Err(error) => Ok(sdk_fail(error)),
            }
        })
    }

    /// `Client::create_batch`: a JSON array of schedules (one creator), one transaction.
    pub fn create_batch(&mut self, input: &str) -> String {
        self.forget_reads();
        if self.names_mode() {
            return fail(NAMES_NOT_YET);
        }
        answer(|| {
            let value: Value =
                serde_json::from_str(input).map_err(|e| format!("invalid JSON: {e}"))?;
            let rows = value
                .as_array()
                .or_else(|| value.get("params").and_then(Value::as_array))
                .ok_or("the batch must be a JSON array of schedules")?
                .clone();
            let mut resolved = Vec::new();
            let mut params = Vec::new();
            for (index, mut row) in rows.into_iter().enumerate() {
                self.resolve_fields(&mut row, &mut resolved)
                    .map_err(|e| format!("row {}: {e}", index + 1))?;
                params.push(
                    wire::create_params(&row).map_err(|e| format!("row {}: {e}", index + 1))?,
                );
            }
            match self.client()?.create_batch(params) {
                Ok(created) => {
                    let ids: Vec<u64> = created.iter().map(|(id, _, _)| *id).collect();
                    Ok(ok(
                        json!({ "ids": ids, "resolved": resolved, "tx": self.tx() }),
                    ))
                }
                Err(error) => Ok(sdk_fail(error)),
            }
        })
    }

    fn claimed(&self, next: &Schedule, amount: u128) -> String {
        ok(json!({
            "amount": amount.to_string(), "amount_display": display::amount(amount),
            "claimed_total": next.claimed.to_string(),
            "claimed_total_display": display::amount(next.claimed),
            "tx": self.tx(),
        }))
    }

    /// `Client::claim_public`, by the schedule's beneficiary.
    pub fn claim_public(&mut self, id: u64) -> String {
        self.forget_reads();
        if self.names_mode() {
            return fail(NAMES_NOT_YET);
        }
        answer(|| {
            let caller = self.schedule(id)?.beneficiary;
            match self.client()?.claim_public(id, caller) {
                Ok((next, amount, _)) => Ok(self.claimed(&next, amount)),
                Err(error) => Ok(sdk_fail(error)),
            }
        })
    }

    /// `Client::claim_private` to `target`, by the schedule's beneficiary. The target's kind
    /// comes from the wallet (a private account it holds), never from the view, and the claim's
    /// fee is simulated first: a claim whose private destination couldn't pay is not sent.
    pub fn claim_private(&mut self, id: u64, target: &str) -> String {
        self.forget_reads();
        if self.names_mode() {
            return fail(NAMES_NOT_YET);
        }
        answer(|| {
            let caller = self.schedule(id)?.beneficiary;
            let (target, _) = self.resolve(target.trim())?;
            let held_private = self
                .wallet_accounts()?
                .iter()
                .any(|a| a.private && a.id == target);
            let destination = AccountRef {
                id: target,
                kind: if held_private {
                    AccountKind::Private
                } else {
                    AccountKind::Public
                },
            };
            if held_private {
                let summary = self
                    .client()?
                    .pre_claim_summary(id, caller, destination)
                    .map_err(|e| e.message())?;
                if summary.fee_covered == Some(false) {
                    return Err(format!(
                        "The private account could not pay the claim's fee ({}); nothing was sent.",
                        display::amount(summary.estimated_fee)
                    ));
                }
            }
            match self.client()?.claim_private(id, caller, destination) {
                Ok((next, amount, _)) => Ok(self.claimed(&next, amount)),
                Err(error) => Ok(sdk_fail(error)),
            }
        })
    }

    /// `Client::cancel`, by the schedule's cancel authority.
    pub fn cancel(&mut self, id: u64) -> String {
        self.forget_reads();
        if self.names_mode() {
            return fail(NAMES_NOT_YET);
        }
        answer(|| {
            let caller = self.schedule(id)?.cancel_authority;
            match self.client()?.cancel(id, caller) {
                Ok((_, refund, _)) => Ok(ok(json!({
                    "refund": refund.to_string(), "refund_display": display::amount(refund),
                    "tx": self.tx(),
                }))),
                Err(error) => Ok(sdk_fail(error)),
            }
        })
    }

    /// `Client::make_irrevocable`, by the schedule's cancel authority.
    pub fn make_irrevocable(&mut self, id: u64) -> String {
        self.forget_reads();
        if self.names_mode() {
            return fail(NAMES_NOT_YET);
        }
        answer(|| {
            let caller = self.schedule(id)?.cancel_authority;
            match self.client()?.make_irrevocable(id, caller) {
                Ok(_) => Ok(ok(json!({ "tx": self.tx() }))),
                Err(error) => Ok(sdk_fail(error)),
            }
        })
    }

    /// `Client::signal_milestone`, by the schedule's milestone authority.
    pub fn signal_milestone(&mut self, id: u64, index: u64) -> String {
        self.forget_reads();
        if self.names_mode() {
            return fail(NAMES_NOT_YET);
        }
        answer(|| {
            let caller = self.schedule(id)?.milestone_authority;
            match self.client()?.signal_milestone(id, caller, index as usize) {
                Ok((_, amount, _)) => Ok(ok(json!({
                    "amount": amount.to_string(), "amount_display": display::amount(amount),
                    "tx": self.tx(),
                }))),
                Err(error) => Ok(sdk_fail(error)),
            }
        })
    }

    /// `Client::transfer_beneficiary`, by the schedule's beneficiary.
    pub fn transfer_beneficiary(&mut self, id: u64, new_beneficiary: &str) -> String {
        self.forget_reads();
        if self.names_mode() {
            return fail(NAMES_NOT_YET);
        }
        answer(|| {
            let caller = self.schedule(id)?.beneficiary;
            let (to, name) = self.resolve(new_beneficiary.trim())?;
            match self.client()?.transfer_beneficiary(id, caller, to) {
                Ok(_) => Ok(ok(json!({
                    "beneficiary": hex::encode(to), "name": name, "tx": self.tx(),
                }))),
                Err(error) => Ok(sdk_fail(error)),
            }
        })
    }

    /// Give an on-chain schedule account (made elsewhere) a local id, so its beneficiary sees it.
    pub fn import_schedule(&mut self, account: &str) -> String {
        self.forget_reads();
        answer(|| {
            let (account, _) = self.resolve(account.trim())?;
            let id = self.client()?.backend_mut().import_account(account)?;
            Ok(ok(json!({ "id": id })))
        })
    }

    /// The accounts Names Wallet gave this app, as the app read them there: a JSON array of
    /// `{account: "Public/<id>" | "Private/<id>", account_kind, balance}` (`granted_accounts`
    /// with `account_balances`). They decide which schedules are "yours" and who claims.
    pub fn set_accounts(&mut self, input: &str) -> String {
        answer(|| {
            let rows: Vec<Value> =
                serde_json::from_str(input).map_err(|e| format!("invalid JSON: {e}"))?;
            let mut accounts = Vec::new();
            for row in rows {
                let text = row.get("account").and_then(Value::as_str).unwrap_or("");
                let id = names::parse_address(text)
                    .ok_or_else(|| format!("'{text}' is not an account"))?;
                let private = text.starts_with("Private/")
                    || row.get("account_kind").and_then(Value::as_str) == Some("private");
                let balance = row
                    .get("balance")
                    .and_then(Value::as_str)
                    .and_then(|b| b.parse().ok())
                    .unwrap_or(0);
                accounts.push(WalletAccount {
                    id,
                    private,
                    balance,
                });
            }
            self.names_accounts = accounts.clone();
            self.accounts_cache = None;
            if let Some(AppBackend::Names(backend)) = self.client.as_mut().map(|c| c.backend_mut())
            {
                backend.set_accounts(accounts.clone());
            }
            Ok(ok(json!({ "accounts": accounts.len() })))
        })
    }

    /// The Names Wallet `propose_call` arguments that claim schedule `id` publicly for its
    /// beneficiary, one of the accounts Names gave: `{program, accounts_json, instruction_hex,
    /// summary, amount, amount_display, caller}`. Names Wallet signs it once the person approves.
    pub fn claim_call(&mut self, id: u64) -> String {
        answer(|| {
            if !self.names_mode() {
                return Err(
                    "claim_call is for Names Wallet; this module signs with a wallet folder".into(),
                );
            }
            let schedule = self.schedule(id)?;
            let mine = self.wallet_accounts()?;
            if !mine
                .iter()
                .any(|a| !a.private && a.id == schedule.beneficiary)
            {
                return Err("this schedule pays an account Names Wallet hasn't shared with Vest: connect that account in Names Wallet".into());
            }
            let Some(AppBackend::Names(backend)) = self.client.as_ref().map(|c| c.backend()) else {
                return Err("not connected".into());
            };
            let mut call = backend.claim_call(id, schedule.beneficiary)?;
            let amount: u128 = call["amount"].as_str().unwrap_or("0").parse().unwrap_or(0);
            call["amount_display"] = json!(display::amount(amount));
            call["summary"] = json!(format!(
                "Claim {} from Vest schedule {} to this account",
                display::amount(amount),
                id
            ));
            Ok(ok(call))
        })
    }

    /// Names Wallet only: the first of `candidates` that is a public account Names Wallet shared
    /// with Vest (the account the person must then choose in the Names prompt).
    fn names_caller(&mut self, candidates: &[AccountId], role: &str) -> Result<AccountId, String> {
        let mine = self.wallet_accounts()?;
        candidates
            .iter()
            .copied()
            .find(|c| mine.iter().any(|a| !a.private && a.id == *c))
            .ok_or_else(|| {
                format!("this needs the schedule's {role}, an account Names Wallet hasn't shared with Vest: connect that account in Names Wallet")
            })
    }

    fn names_backend(&mut self) -> Result<&names_backend::NamesBackend, String> {
        match self.client()?.backend() {
            AppBackend::Names(backend) => Ok(backend),
            #[allow(unreachable_patterns)]
            _ => Err("not connected to Names Wallet".into()),
        }
    }

    /// The Names Wallet `propose_call` arguments that create one native-LEZ schedule (the same
    /// JSON object as `create`; accounts may be names): `{program, accounts_json,
    /// instruction_hex, summary, amount, amount_display, caller, schedule_address,
    /// schedule_address_hex, resolved}`. `caller` (the creator) is the account to choose in the
    /// Names prompt. Once the request is `done`, `import_schedule(schedule_address)` gives the
    /// schedule its local id. Needs Names Wallet 0.18 (it reads the network clock).
    pub fn create_call(&mut self, input: &str) -> String {
        self.forget_reads();
        answer(|| {
            if !self.names_mode() {
                return Err(
                    "create_call is for Names Wallet; this module signs with a wallet folder"
                        .into(),
                );
            }
            let mut row: Value =
                serde_json::from_str(input).map_err(|e| format!("invalid JSON: {e}"))?;
            let mut resolved = Vec::new();
            self.resolve_fields(&mut row, &mut resolved)?;
            let params = wire::create_params(&row)?;
            self.names_caller(&[params.creator], "creator")?;
            let (total, beneficiary) = (params.total, params.beneficiary);
            let nonce = names_backend::random_nonce()?;
            let mut call = self.names_backend()?.create_call(params, nonce)?;
            let to = resolved
                .iter()
                .find(|r| r["account"] == hex::encode(beneficiary))
                .and_then(|r| r["name"].as_str().map(str::to_string))
                .unwrap_or_else(|| b58(&beneficiary));
            call["amount_display"] = json!(display::amount(total));
            call["summary"] = json!(format!(
                "Create a Vest schedule: {} from this account, vesting to {to}",
                display::amount(total)
            ));
            call["resolved"] = json!(resolved);
            Ok(ok(call))
        })
    }

    fn op_call(
        &mut self,
        id: u64,
        op: names_backend::NamesOp,
        candidates: impl FnOnce(&Schedule) -> Vec<AccountId>,
        role: &str,
        summary: impl FnOnce(&str) -> String,
    ) -> String {
        self.forget_reads();
        answer(|| {
            if !self.names_mode() {
                return Err(
                    "this call is for Names Wallet; this module signs with a wallet folder".into(),
                );
            }
            let schedule = self.schedule(id)?;
            let caller = self.names_caller(&candidates(&schedule), role)?;
            let mut call = self.names_backend()?.op_call(id, caller, &op)?;
            let amount: u128 = call["amount"].as_str().unwrap_or("0").parse().unwrap_or(0);
            let shown = display::amount(amount);
            call["amount_display"] = json!(shown);
            call["summary"] = json!(summary(&shown));
            Ok(ok(call))
        })
    }

    /// `propose_call` arguments that cancel schedule `id` (its cancel authority signs; what has
    /// not vested goes back to the creator, `amount`). Reply as `create_call`'s, without the
    /// schedule fields beyond `schedule_address`.
    pub fn cancel_call(&mut self, id: u64) -> String {
        self.op_call(
            id,
            names_backend::NamesOp::Cancel,
            |s| vec![s.cancel_authority],
            "cancel authority",
            |amount| {
                format!(
                    "Cancel Vest schedule {id}: {amount} not yet vested goes back to its creator"
                )
            },
        )
    }

    /// `propose_call` arguments that make schedule `id` irrevocable (its cancel authority or
    /// creator signs).
    pub fn irrevocable_call(&mut self, id: u64) -> String {
        self.op_call(
            id,
            names_backend::NamesOp::MakeIrrevocable,
            |s| vec![s.cancel_authority, s.creator],
            "cancel authority or creator",
            |_| {
                format!("Make Vest schedule {id} irrevocable: it can never be cancelled after this")
            },
        )
    }

    /// `propose_call` arguments that signal milestone tranche `index` of schedule `id` (its
    /// milestone authority signs; `amount` is what the tranche unlocks).
    pub fn signal_call(&mut self, id: u64, index: u64) -> String {
        let Ok(tranche) = u32::try_from(index) else {
            return fail("index: too large");
        };
        self.op_call(id, names_backend::NamesOp::SignalMilestone(tranche), |s| vec![s.milestone_authority], "milestone authority", |amount| {
            format!("Signal milestone {index} of Vest schedule {id}: unlocks {amount} for its beneficiary")
        })
    }

    /// `propose_call` arguments that move schedule `id` to another beneficiary (hex, base58 or a
    /// name; its beneficiary signs, and the schedule must be transferable).
    pub fn transfer_call(&mut self, id: u64, new_beneficiary: &str) -> String {
        let (to, name) = match self.resolve(new_beneficiary.trim()) {
            Ok(found) => found,
            Err(error) => return fail(error),
        };
        let shown = name.unwrap_or_else(|| b58(&to));
        self.op_call(
            id,
            names_backend::NamesOp::TransferBeneficiary(to),
            |s| vec![s.beneficiary],
            "beneficiary",
            |_| format!("Move Vest schedule {id} to a new beneficiary, {shown}"),
        )
    }

    /// Resolve an LNS name (or check an address): `{name, account, base58}`.
    pub fn resolve_name(&mut self, text: &str) -> String {
        answer(|| {
            let (account, name) = self.resolve(text.trim())?;
            Ok(ok(json!({
                "name": name, "account": hex::encode(account), "base58": b58(&account),
            })))
        })
    }
}

#[cfg(all(test, not(feature = "lez")))]
mod tests;
