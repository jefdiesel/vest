//! LEZ rc3 backend for the vesting SDK (feature `lez`).
//!
//! It talks to the vesting program in `program/` (crate `vesting-program`) through its own types,
//! so there is no second copy of the wire format. Reads decode the schedule actor's state, which
//! holds `vesting-core`'s `Schedule` verbatim. Writes send one real transaction per SDK
//! `store`/`submit` pair (one for a whole batch), signed by the caller the client names.
//!
//! Claims use the program's "as of" instruction at the time the client computed the claim (the
//! event's `at`), so the transaction pays exactly what the client showed and, being valid from
//! that time on, a private claim's proof never expires. A private destination is a one-time alias
//! of a private account of the wallet; the account kind always comes from the wallet.

#[cfg(feature = "lez")]
pub use backend_impl::{wallet_cli, LastTx, LezBackend};
#[cfg(feature = "lez")]
pub use vesting_program as program;

/// The public accounts a transaction touched, read by its hash (`getTransaction`) through
/// `rpc` (tools and measurements: finding the schedule account a Create made).
#[cfg(all(feature = "lez", feature = "guard"))]
#[doc(hidden)]
pub fn transaction_accounts(rpc: &str, hash: &str) -> Result<Vec<[u8; 32]>, String> {
    let request = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "getTransaction", "params": [hash] });
    let text = ureq::post(rpc)
        .set("Content-Type", "application/json")
        .send_string(&request.to_string())
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let answer: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let found = answer
        .get("result")
        .cloned()
        .ok_or_else(|| format!("no result: {text:.200}"))?;
    let (tx, _block): (common::transaction::LeeTransaction, u64) =
        serde_json::from_value(found).map_err(|e| e.to_string())?;
    match tx {
        common::transaction::LeeTransaction::Public(tx) => Ok(tx
            .affected_public_account_ids()
            .into_iter()
            .map(|id| id.into_value())
            .collect()),
        common::transaction::LeeTransaction::PrivacyPreserving(tx) => Ok(tx
            .affected_public_account_ids()
            .into_iter()
            .map(|id| id.into_value())
            .collect()),
    }
}

/// For the RPC relay (`rpc_guard`): the hash of a `sendTransaction`'s transaction, as
/// `getTransaction` takes it, so a refused write is looked up before it is ever resent.
#[cfg(all(feature = "lez", feature = "guard"))]
pub fn tx_hash_hook() -> Box<crate::rpc_guard::TxHash> {
    Box::new(|params: &serde_json::Value| {
        let tx = match params {
            serde_json::Value::Array(items) => items.first()?,
            serde_json::Value::Object(map) => map.get("tx")?,
            _ => return None,
        };
        let tx: common::transaction::LeeTransaction = serde_json::from_value(tx.clone()).ok()?;
        Some(serde_json::Value::String(tx.hash().to_string()))
    })
}

#[cfg(feature = "lez")]
mod backend_impl {
    use crate::{Backend, BackendError, FeeEstimate, FeePayer, ScheduleId};
    use lee::privacy_preserving_transaction::circuit::ProgramCatalog;
    use lee::program::Program;
    use lee_core::account::{AccountId, Actor};
    use lee_core::native_token::NATIVE_TOKEN_PROGRAM_ID;
    use sequencer_service_rpc::SequencerClient;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use vesting_core::{Event, Schedule, Timestamp};
    use vesting_program::{
        clock, schedule_address, Asset, Instruction, NewSchedule, ScheduleState, Sponsor,
    };
    use wallet::{
        config::{SequencerConnectionData, WalletConfigOverrides},
        program_facades::program_loader::ProgramLoader,
        utils::read_password,
        AccountIdentity, AccountMention, AsyncDelivery, DryRunReport, ExecutionFailureKind,
        WalletCore,
    };

    #[derive(serde::Serialize, serde::Deserialize, Default)]
    struct IdMap {
        next: ScheduleId,
        /// Local SDK id -> on-chain schedule account, hex.
        ids: BTreeMap<ScheduleId, String>,
        /// The nonce tag of this map's schedules (random per map, so two maps of the same creator
        /// never collide).
        #[serde(default)]
        tag: Option<String>,
    }

    /// What the last write did on chain.
    #[derive(Debug, Clone, Default)]
    pub struct LastTx {
        pub hash: String,
        pub block: u64,
        pub timestamp: u64,
    }

    /// A send's outcome: included on chain, or (dry run) the wallet's simulation report.
    enum Sent {
        Applied(LastTx),
        DryRun(Box<DryRunReport>),
    }

    pub struct LezBackend {
        runtime: tokio::runtime::Runtime,
        wallet_config: PathBuf,
        wallet_storage: PathBuf,
        password: String,
        sequencer: url::Url,
        program: AccountId,
        token_program: Option<AccountId>,
        sponsor: Option<Sponsor>,
        clock_elf: Option<PathBuf>,
        map_path: PathBuf,
        map: IdMap,
        pending: Option<(ScheduleId, Schedule)>,
        claim_target: Option<AccountId>,
        caller: Option<[u8; 32]>,
        dry_run: bool,
        /// The report of the last dry-run send.
        dry_report: Option<Box<DryRunReport>>,
        /// Deployed programs read for a transaction's catalog, kept 10 minutes: a claim summary
        /// and the claim after it would otherwise read every ELF segment twice.
        programs: std::sync::Mutex<BTreeMap<AccountId, (std::time::Instant, Program)>>,
        /// Schedule states read in the last 3 seconds (one refresh reads each once); forgotten
        /// after every transaction.
        reads: std::sync::Mutex<BTreeMap<AccountId, (std::time::Instant, Vec<u8>)>>,
        caching: bool,
        gas_limit: Option<u64>,
        last: Option<LastTx>,
    }

    impl LezBackend {
        pub fn open(
            sequencer: &str,
            wallet_dir: &Path,
            program: [u8; 32],
            clock_elf: Option<PathBuf>,
            map_path: &Path,
        ) -> Result<Self, BackendError> {
            Self::open_with_password(sequencer, wallet_dir, program, clock_elf, map_path, None)
        }

        /// As [`LezBackend::open`], with the wallet's password given (an app has no terminal to
        /// prompt on). With `None`, an encrypted wallet needs the LEZ wallet's password variable,
        /// and is refused otherwise instead of prompting.
        pub fn open_with_password(
            sequencer: &str,
            wallet_dir: &Path,
            program: [u8; 32],
            clock_elf: Option<PathBuf>,
            map_path: &Path,
            password: Option<String>,
        ) -> Result<Self, BackendError> {
            let sequencer = url::Url::parse(sequencer)
                .map_err(|e| BackendError::Io(format!("--network: {e}")))?;
            if !matches!(sequencer.scheme(), "http" | "https") {
                return Err(BackendError::Io("--network must be an http(s) URL".into()));
            }
            let wallet_storage = wallet_dir.join("storage.json");
            if !wallet_storage.exists() {
                return Err(BackendError::Io(format!(
                    "no wallet at {}",
                    wallet_dir.display()
                )));
            }
            let password = match password {
                Some(password) => password,
                None => {
                    let encrypted = wallet::storage::Storage::is_encrypted(&wallet_storage)
                        .map_err(|e| BackendError::Io(e.to_string()))?;
                    let from_env = std::env::var(wallet::PASSWORD_ENV_VAR)
                        .map(|p| !p.is_empty())
                        .unwrap_or(false);
                    if encrypted
                        && !from_env
                        && !std::io::IsTerminal::is_terminal(&std::io::stdin())
                    {
                        return Err(BackendError::Io(
                            "the wallet is encrypted: give its password".into(),
                        ));
                    }
                    read_password(&wallet_storage)
                        .map_err(|e| BackendError::Io(e.to_string()))?
                        .to_string()
                }
            };
            let mut map: IdMap = match std::fs::read(map_path) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|e| BackendError::Decode(e.to_string()))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => IdMap::default(),
                Err(error) => return Err(BackendError::Io(error.to_string())),
            };
            if map.tag.is_none() {
                map.tag = Some(hex::encode(&random32()?[..8]));
            }
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| BackendError::Io(e.to_string()))?;
            Ok(Self {
                runtime,
                wallet_config: wallet_dir.join("wallet_config.json"),
                wallet_storage,
                password,
                sequencer,
                program: AccountId::new(program),
                token_program: None,
                sponsor: None,
                clock_elf,
                map_path: map_path.to_path_buf(),
                map,
                pending: None,
                claim_target: None,
                caller: None,
                dry_run: false,
                dry_report: None,
                programs: Default::default(),
                reads: Default::default(),
                caching: true,
                gas_limit: None,
                last: None,
            })
        }

        /// Measurements: turn the read caches (program catalog, schedule states) off.
        #[doc(hidden)]
        pub fn set_caching(&mut self, on: bool) {
            self.caching = on;
            self.programs.lock().map(|mut p| p.clear()).ok();
            self.reads.lock().map(|mut r| r.clear()).ok();
        }

        /// The 0.4 token program for token schedules (`--token` other than zero).
        pub fn set_token_program(&mut self, program: [u8; 32]) {
            self.token_program = Some(AccountId::new(program));
        }

        /// Fee sponsorship of private claims for new token schedules.
        pub fn set_sponsor(&mut self, deposit: u128, per_claim: u128) {
            self.sponsor = Some(Sponsor { deposit, per_claim });
        }

        /// Simulate writes only; each reports the wallet's dry-run (cycles, bytes, fee) as an error.
        pub fn set_dry_run(&mut self, on: bool) {
            self.dry_run = on;
        }

        /// The execution gas a public transaction declares at most (`None`: the wallet's default).
        pub fn set_gas_limit(&mut self, limit: Option<u64>) {
            self.gas_limit = limit;
        }

        pub fn last_tx(&self) -> Option<&LastTx> {
            self.last.as_ref()
        }

        /// The on-chain account of a schedule id.
        pub fn address(&self, id: ScheduleId) -> Result<[u8; 32], BackendError> {
            Ok(self.address_of(id)?.into_value())
        }

        /// Record an existing schedule account under a local id (a beneficiary on another machine).
        /// An account already imported keeps its id; one that holds no schedule of this program
        /// is refused before it gets one.
        pub fn import(&mut self, account: [u8; 32]) -> Result<ScheduleId, BackendError> {
            if let Some(id) = self.id_of(&account) {
                return Ok(id);
            }
            let wallet = self.wallet()?;
            let found = self.chain_state(&wallet, AccountId::new(account))?;
            drop(wallet);
            if !found.is_some_and(|chain| chain.schedule.is_some()) {
                return Err(BackendError::Io(format!(
                    "{} is not a schedule of this vesting program",
                    hex::encode(account)
                )));
            }
            let id = self.next_id();
            self.map.ids.insert(id, hex::encode(account));
            self.persist()?;
            Ok(id)
        }

        /// The local id of an imported (or created) schedule account, when it has one.
        pub fn id_of(&self, account: &[u8; 32]) -> Option<ScheduleId> {
            let hex = hex::encode(account);
            self.map
                .ids
                .iter()
                .find(|(_, known)| **known == hex)
                .map(|(id, _)| *id)
        }

        fn wallet(&self) -> Result<WalletCore<SequencerClient>, BackendError> {
            let overrides = WalletConfigOverrides {
                sequencer: Some(SequencerConnectionData {
                    sequencer_addr: self.sequencer.clone(),
                    basic_auth: None,
                }),
                gas_limit: self.gas_limit,
                ..WalletConfigOverrides::default()
            };
            let mut wallet = WalletCore::new_update_chain(
                self.wallet_config.clone(),
                self.wallet_storage.clone(),
                Some(overrides),
                &self.password,
            )
            .map_err(|error| BackendError::Io(error.to_string()))?;
            wallet.set_dry_run(self.dry_run);
            Ok(wallet)
        }

        fn persist(&self) -> Result<(), BackendError> {
            let bytes = serde_json::to_vec_pretty(&self.map)
                .map_err(|e| BackendError::Decode(e.to_string()))?;
            if let Some(parent) = self.map_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| BackendError::Io(e.to_string()))?;
            }
            std::fs::write(&self.map_path, bytes).map_err(|e| BackendError::Io(e.to_string()))
        }

        fn nonce(&self, id: ScheduleId) -> [u8; 32] {
            let mut out = [0u8; 32];
            let tag = self.map.tag.as_deref().unwrap_or("sdk").as_bytes();
            let len = tag.len().min(24);
            out[..len].copy_from_slice(&tag[..len]);
            out[24..].copy_from_slice(&id.to_le_bytes());
            out
        }

        fn address_of(&self, id: ScheduleId) -> Result<AccountId, BackendError> {
            let hex = self.map.ids.get(&id).ok_or(BackendError::NotFound(id))?;
            let bytes: [u8; 32] = hex::decode(hex)
                .map_err(|e| BackendError::Decode(e.to_string()))?
                .try_into()
                .map_err(|_| BackendError::Decode("schedule address is not 32 bytes".into()))?;
            Ok(AccountId::new(bytes))
        }

        fn identity(wallet: &WalletCore<SequencerClient>, account: AccountId) -> AccountIdentity {
            if let Some(private) = wallet.resolve_private_account(account) {
                private
            } else if wallet.get_account_public_signing_key(account).is_some() {
                AccountIdentity::Public(account)
            } else {
                AccountIdentity::PublicNoSign(account)
            }
        }

        fn nosign(account: AccountId, program: AccountId) -> AccountMention {
            AccountIdentity::PublicNoSign(account).select_program_actor_state(program)
        }

        fn asset_of(&self, state: &Schedule) -> Result<Asset, BackendError> {
            if state.token == [0u8; 32] {
                return Ok(Asset::Native);
            }
            let program = self
                .token_program
                .ok_or_else(|| BackendError::Io("a token schedule needs --token-program".into()))?;
            Ok(Asset::Token {
                program,
                definition: AccountId::new(state.token),
            })
        }

        fn escrow_program(asset: Asset) -> AccountId {
            match asset {
                Asset::Native => NATIVE_TOKEN_PROGRAM_ID,
                Asset::Token { program, .. } => program,
            }
        }

        async fn public_state(
            wallet: &WalletCore<SequencerClient>,
            account: AccountId,
            program: AccountId,
        ) -> Result<Vec<u8>, BackendError> {
            let view = wallet
                .get_account_view(Actor::new(account, program))
                .await
                .map_err(|e| BackendError::Io(e.to_string()))?;
            Ok(view.map_or_else(Vec::new, |found| found.data.actor_state(program).to_vec()))
        }

        fn chain_state(
            &self,
            wallet: &WalletCore<SequencerClient>,
            address: AccountId,
        ) -> Result<Option<ScheduleState>, BackendError> {
            let fresh = self.reads.lock().ok().and_then(|reads| {
                reads
                    .get(&address)
                    .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(3))
                    .map(|(_, bytes)| bytes.clone())
            });
            let bytes = match fresh {
                Some(bytes) => bytes,
                None => {
                    let bytes =
                        self.runtime
                            .block_on(Self::public_state(wallet, address, self.program))?;
                    if let (true, Ok(mut reads)) = (self.caching, self.reads.lock()) {
                        reads.insert(address, (std::time::Instant::now(), bytes.clone()));
                    }
                    bytes
                }
            };
            if bytes.is_empty() {
                return Ok(None);
            }
            borsh::from_slice(&bytes)
                .map(Some)
                .map_err(|e| BackendError::Decode(format!("not a vesting schedule: {e}")))
        }

        async fn catalog(
            &self,
            wallet: &WalletCore<SequencerClient>,
            programs: &[AccountId],
        ) -> Result<ProgramCatalog, BackendError> {
            let cached = |id: &AccountId| {
                self.programs.lock().ok().and_then(|programs| {
                    programs
                        .get(id)
                        .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(600))
                        .map(|(_, program)| program.clone())
                })
            };
            let keep = |id: AccountId, program: &Program| {
                if let (true, Ok(mut programs)) = (self.caching, self.programs.lock()) {
                    programs.insert(id, (std::time::Instant::now(), program.clone()));
                }
            };
            let mut entries = Vec::new();
            for program in programs {
                let deployed = match cached(program) {
                    Some(deployed) => deployed,
                    None => {
                        let deployed = ProgramLoader(wallet)
                            .program(*program)
                            .await
                            .map_err(|e| BackendError::Io(e.to_string()))?
                            .ok_or_else(|| {
                                BackendError::Io(format!("program {program} is not deployed"))
                            })?;
                        keep(*program, &deployed);
                        deployed
                    }
                };
                entries.push((*program, deployed));
            }
            // The clock is a builtin: a proven transaction that reaches it needs its ELF too.
            let clock_id = clock::clock_account_id();
            let clock_program = match cached(&clock_id) {
                Some(program) => Some(program),
                None => ProgramLoader(wallet)
                    .program(clock_id)
                    .await
                    .ok()
                    .flatten()
                    .inspect(|p| keep(clock_id, p)),
            };
            let clock_program = match clock_program {
                Some(program) => Some(program),
                None => match &self.clock_elf {
                    Some(path) => {
                        let bytes =
                            std::fs::read(path).map_err(|e| BackendError::Io(e.to_string()))?;
                        Some(
                            Program::new(bytes.into())
                                .map_err(|e| BackendError::Io(e.to_string()))?,
                        )
                    }
                    None => None,
                },
            };
            if let Some(program) = clock_program {
                entries.push((clock_id, program));
            }
            Ok(entries.into_iter().collect())
        }

        async fn send(
            &self,
            accounts: Vec<AccountMention>,
            message: Vec<u8>,
            programs: Vec<AccountId>,
            fee_payer: Option<AccountId>,
        ) -> Result<Sent, BackendError> {
            let mut wallet = self.wallet()?;
            let catalog = self.catalog(&wallet, &programs).await?;
            let sent = wallet
                .send_tx_with_pre_check(
                    accounts,
                    0,
                    message,
                    &catalog,
                    None,
                    AsyncDelivery::default(),
                    fee_payer,
                    |_| Ok(()),
                )
                .await;
            let hash = match sent {
                Err(ExecutionFailureKind::DryRun(report)) => return Ok(Sent::DryRun(report)),
                Err(error) => return Err(BackendError::Io(error.to_string())),
                Ok((hash, _)) => hash,
            };
            let (transaction, block) = wallet
                .poll_transaction(hash)
                .await
                .map_err(|e| BackendError::Io(format!("{hash} not included: {e}")))?;
            let block = wallet
                .get_block(block)
                .await
                .map_err(|e| BackendError::Io(e.to_string()))?
                .ok_or_else(|| BackendError::Io("block missing".into()))?;
            let position = block
                .body
                .transactions
                .iter()
                .position(|t| t.hash() == hash);
            let reverted = position.is_some_and(|p| block.body.is_reverted(p));
            if let common::transaction::LeeTransaction::PrivacyPreserving(tx) = &transaction {
                let mut index = wallet
                    .storage_mut()
                    .key_chain_mut()
                    .build_latest_nullifier_index();
                wallet.storage_mut().key_chain_mut().follow_transaction(
                    &tx.message,
                    reverted,
                    false,
                    &mut index,
                );
                wallet
                    .store_persistent_data()
                    .map_err(|e| BackendError::Io(e.to_string()))?;
            }
            if reverted {
                return Err(BackendError::Io(format!(
                    "transaction {hash} was included in block {} but reverted",
                    block.header.block_id
                )));
            }
            eprintln!(
                "lez: tx {hash} applied in block {} (timestamp {})",
                block.header.block_id, block.header.timestamp
            );
            Ok(Sent::Applied(LastTx {
                hash: hash.to_string(),
                block: block.header.block_id,
                timestamp: block.header.timestamp,
            }))
        }

        fn programs_for(&self, asset: Asset) -> Vec<AccountId> {
            match asset {
                Asset::Native => vec![self.program],
                Asset::Token { program, .. } => vec![self.program, program],
            }
        }

        fn new_schedule(
            &self,
            id: ScheduleId,
            state: &Schedule,
        ) -> Result<NewSchedule, BackendError> {
            let asset = self.asset_of(state)?;
            Ok(NewSchedule {
                nonce: self.nonce(id),
                beneficiary: AccountId::new(state.beneficiary),
                asset,
                total: state.total,
                kind: state.kind.clone(),
                cancelable: Some(state.cancelable),
                transferable: Some(state.transferable),
                cancel_authority: Some(AccountId::new(state.cancel_authority)),
                milestone_authority: Some(AccountId::new(state.milestone_authority)),
                sponsor: match asset {
                    Asset::Token { .. } => self.sponsor,
                    Asset::Native => None,
                },
            })
        }

        /// One Create transaction for every item (all from the same creator).
        fn send_create(&mut self, items: &[(ScheduleId, Schedule)]) -> Result<(), BackendError> {
            let creator = AccountId::new(
                items
                    .first()
                    .ok_or_else(|| BackendError::Io("empty batch".into()))?
                    .1
                    .creator,
            );
            let wallet = self.wallet()?;
            let mut schedules = Vec::new();
            let mut accounts =
                vec![Self::identity(&wallet, creator).select_program_actor_state(self.program)];
            let mut seen_escrows = Vec::new();
            for (id, state) in items {
                if AccountId::new(state.creator) != creator {
                    return Err(BackendError::Io("a batch has one creator".into()));
                }
                let n = self.new_schedule(*id, state)?;
                let address = schedule_address(&self.program, &creator, &n.nonce);
                let escrow = Self::escrow_program(n.asset);
                if !seen_escrows.contains(&n.asset) {
                    accounts
                        .push(Self::identity(&wallet, creator).select_program_actor_state(escrow));
                    if let Asset::Token {
                        program,
                        definition,
                    } = n.asset
                    {
                        accounts.push(Self::nosign(definition, program));
                        if n.sponsor.is_some() {
                            accounts.push(
                                Self::identity(&wallet, creator)
                                    .select_program_actor_state(NATIVE_TOKEN_PROGRAM_ID),
                            );
                        }
                    }
                    seen_escrows.push(n.asset);
                }
                accounts.push(Self::nosign(address, self.program));
                accounts.push(Self::nosign(address, escrow));
                if n.sponsor.is_some() {
                    accounts.push(Self::nosign(address, NATIVE_TOKEN_PROGRAM_ID));
                }
                schedules.push(n);
            }
            accounts.push(Self::nosign(
                clock::CLOCK_PROGRAM_ACCOUNT_ID,
                clock::clock_account_id(),
            ));
            let mut programs = vec![self.program];
            for asset in &seen_escrows {
                if let Asset::Token { program, .. } = asset {
                    if !programs.contains(program) {
                        programs.push(*program);
                    }
                }
            }
            // A native and a sponsored token schedule both mention the creator's native LEZ (two
            // definitions of one token program its holding): the wallet refuses a repeated actor.
            dedup_mentions(&mut accounts);
            let message = borsh::to_vec(&Instruction::Create { schedules }).expect("borsh");
            drop(wallet);
            self.runtime_send(accounts, message, programs, None)?;
            if !self.dry_run {
                for (id, state) in items {
                    let address = schedule_address(
                        &self.program,
                        &AccountId::new(state.creator),
                        &self.nonce(*id),
                    );
                    self.map.ids.insert(*id, hex::encode(address.value()));
                }
                self.persist()?;
            }
            Ok(())
        }

        fn runtime_send(
            &mut self,
            accounts: Vec<AccountMention>,
            message: Vec<u8>,
            programs: Vec<AccountId>,
            fee_payer: Option<AccountId>,
        ) -> Result<(), BackendError> {
            match self
                .runtime
                .block_on(self.send(accounts, message, programs, fee_payer))?
            {
                Sent::Applied(last) => {
                    self.last = Some(last);
                    if let Ok(mut reads) = self.reads.lock() {
                        reads.clear();
                    }
                    Ok(())
                }
                // A dry run is not a write: the caller (the CLI's --dry-run) shows the report.
                Sent::DryRun(report) => {
                    let text = format!("dry run:\n{report}");
                    self.dry_report = Some(report);
                    Err(BackendError::Io(text))
                }
            }
        }

        /// The fee estimate of a dry-run report: who pays, and what they will have to pay with.
        fn fee_estimate(
            &self,
            report: &DryRunReport,
            id: ScheduleId,
            event: &Event,
            target: Option<AccountId>,
        ) -> Result<FeeEstimate, BackendError> {
            match report {
                DryRunReport::Public {
                    fee,
                    fee_at_head,
                    reserve_at_head,
                    revert_reason,
                    ..
                } => Ok(FeeEstimate {
                    fee: *fee_at_head,
                    reserve: *reserve_at_head,
                    payer: FeePayer::Caller,
                    available: Some(self.balances(fee.payer.into_value(), None)?.0),
                    revert_reason: revert_reason.clone(),
                }),
                DryRunReport::Private {
                    fee, fee_at_head, ..
                } => {
                    let payer = target.unwrap_or(fee.payer);
                    let held = self.balances(payer.into_value(), None)?.0;
                    // What the claim brings the payer before its fee is taken: the native LEZ it
                    // claims, or a token schedule's per-claim draw on the creator's float.
                    let brings = match event {
                        Event::Claimed { amount, .. } => {
                            let address = self.address_of(id)?;
                            let wallet = self.wallet()?;
                            let chain = self
                                .chain_state(&wallet, address)?
                                .ok_or(BackendError::NotFound(id))?;
                            match chain.asset {
                                Asset::Native => *amount,
                                Asset::Token { .. } => chain.per_claim.min(chain.float),
                            }
                        }
                        _ => 0,
                    };
                    Ok(FeeEstimate {
                        fee: *fee_at_head,
                        reserve: fee.reserve,
                        payer: FeePayer::ClaimTarget,
                        available: Some(held.saturating_add(brings)),
                        revert_reason: None,
                    })
                }
            }
        }

        /// The transaction for a transition of an existing schedule.
        fn send_op(
            &mut self,
            id: ScheduleId,
            state: &Schedule,
            event: &Event,
        ) -> Result<(), BackendError> {
            let address = self.address_of(id)?;
            let wallet = self.wallet()?;
            let chain = self
                .chain_state(&wallet, address)?
                .ok_or(BackendError::NotFound(id))?;
            let asset = chain.asset;
            let escrow = Self::escrow_program(asset);
            let caller = self
                .caller
                .take()
                .map(AccountId::new)
                .unwrap_or_else(|| AccountId::new(state.beneficiary));
            let mut accounts = vec![
                Self::identity(&wallet, caller).select_program_actor_state(self.program),
                Self::nosign(address, self.program),
                Self::nosign(address, escrow),
            ];
            let clock_mention =
                Self::nosign(clock::CLOCK_PROGRAM_ACCOUNT_ID, clock::clock_account_id());
            let mut fee_payer = None;
            let instruction = match event {
                Event::Claimed { to_private, at, .. } => {
                    // The queued target is used up by this claim either way, and only a private
                    // claim goes anywhere but the beneficiary's own account.
                    let queued = self.claim_target.take();
                    let target = match (*to_private, queued) {
                        (true, Some(target)) => target,
                        _ => AccountId::new(state.beneficiary),
                    };
                    let private = if *to_private {
                        wallet.resolve_private_account(target)
                    } else {
                        None
                    };
                    if *to_private && private.is_none() {
                        return Err(BackendError::Io(
                            "the claim target is not a private account of this wallet".into(),
                        ));
                    }
                    let destination = match private {
                        Some(identity) => {
                            let opening = random32()?;
                            let alias = target.blinded(&opening);
                            // A token claim initializes the alias's holding, which the account must
                            // authorize; a native claim only receives.
                            let authorizes = matches!(asset, Asset::Token { .. });
                            let mut mention = identity.clone().select_program_actor_state(escrow);
                            if !authorizes {
                                mention = mention.receiving();
                            }
                            mention.address_openings.insert(opening);
                            accounts.push(mention);
                            if let Asset::Token {
                                program,
                                definition,
                            } = asset
                            {
                                accounts.push(Self::nosign(definition, program));
                                let mut native =
                                    identity.select_program_actor_state(NATIVE_TOKEN_PROGRAM_ID);
                                native.address_openings.insert(opening);
                                accounts.push(native);
                                accounts.push(Self::nosign(address, NATIVE_TOKEN_PROGRAM_ID));
                            }
                            fee_payer = Some(target);
                            alias
                        }
                        None => {
                            accounts.push(
                                Self::identity(&wallet, target).select_program_actor_state(escrow),
                            );
                            target
                        }
                    };
                    Instruction::ClaimAsOf {
                        schedule: address,
                        to: destination,
                        to_private: *to_private,
                        as_of: *at,
                    }
                }
                Event::Cancelled { .. } => {
                    let creator = AccountId::new(state.creator);
                    accounts
                        .push(Self::identity(&wallet, creator).select_program_actor_state(escrow));
                    if chain.float > 0 {
                        accounts.push(Self::nosign(address, NATIVE_TOKEN_PROGRAM_ID));
                        accounts.push(
                            Self::identity(&wallet, creator)
                                .select_program_actor_state(NATIVE_TOKEN_PROGRAM_ID),
                        );
                    }
                    accounts.push(clock_mention);
                    Instruction::Cancel { schedule: address }
                }
                Event::MadeIrrevocable { .. } => {
                    accounts.push(clock_mention);
                    Instruction::MakeIrrevocable { schedule: address }
                }
                Event::MilestoneSignalled { index, .. } => {
                    accounts.push(clock_mention);
                    Instruction::SignalMilestone {
                        schedule: address,
                        index: u32::from(*index),
                    }
                }
                Event::BeneficiaryTransferred { new, .. } => {
                    accounts.push(clock_mention);
                    Instruction::TransferBeneficiary {
                        schedule: address,
                        new_beneficiary: AccountId::new(*new),
                    }
                }
                Event::Created { .. } => unreachable!("created schedules go through send_create"),
            };
            // A claim that empties the schedule returns the unused float to the creator.
            if matches!(event, Event::Claimed { .. }) && chain.float > 0 {
                accounts.push(Self::nosign(address, NATIVE_TOKEN_PROGRAM_ID));
                let creator = AccountId::new(state.creator);
                accounts.push(
                    Self::identity(&wallet, creator)
                        .without_signing()
                        .select_program_actor_state(NATIVE_TOKEN_PROGRAM_ID),
                );
            }
            dedup_mentions(&mut accounts);
            drop(wallet);
            let message = borsh::to_vec(&instruction).expect("borsh");
            self.runtime_send(accounts, message, self.programs_for(asset), fee_payer)
        }
    }

    impl LezBackend {
        /// Deploy a program ELF at a new header account of this wallet, paid by `payer`. Updatable
        /// (the header key, kept in this wallet, can replace it) unless `immutable`. Returns the
        /// program id and its image id.
        pub fn deploy(
            &mut self,
            elf: &[u8],
            payer: [u8; 32],
            immutable: bool,
        ) -> Result<([u8; 32], String), BackendError> {
            /// `program_loader_core::MAX_SEGMENT_DATA_LEN` at v0.4.0-rc3.
            const MAX_SEGMENT_DATA_LEN: usize = 96 * 1024;
            let program = Program::new(elf.to_vec().into())
                .map_err(|e| BackendError::Io(format!("bad ELF: {e}")))?;
            let image: String = program
                .id()
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .map(|b| format!("{b:02x}"))
                .collect();
            let user_elf_len = risc0_binfmt::ProgramBinary::decode(elf)
                .map_err(|e| BackendError::Io(format!("not a RISC Zero program binary: {e}")))?
                .user_elf
                .len();
            let mut wallet = self.wallet()?;
            let header = wallet.create_new_account_public(None).0;
            let segments: Vec<AccountId> =
                std::iter::repeat_with(|| wallet.create_new_account_public(None).0)
                    .take(user_elf_len.div_ceil(MAX_SEGMENT_DATA_LEN))
                    .collect();
            wallet
                .store_persistent_data()
                .map_err(|e| BackendError::Io(e.to_string()))?;
            eprintln!("deploying {} bytes ({user_elf_len} user ELF) in {} segment(s) at {header}, image {image}", elf.len(), segments.len());
            self.runtime
                .block_on(ProgramLoader(&wallet).deploy(
                    header,
                    &segments,
                    elf.to_vec(),
                    immutable,
                    Some(AccountId::new(payer)),
                ))
                .map_err(|e| BackendError::Io(format!("deploy: {e}")))?;
            let deployed = self
                .runtime
                .block_on(ProgramLoader(&wallet).program(header))
                .map_err(|e| BackendError::Io(e.to_string()))?
                .ok_or_else(|| BackendError::Io("nothing deployed at the header".into()))?;
            if deployed.id() != program.id() {
                return Err(BackendError::Io(
                    "the deployed image differs from the ELF".into(),
                ));
            }
            Ok((header.into_value(), image))
        }

        /// Token program helper: a new definition whose whole `supply` goes to `holder` (signs; it
        /// must hold no holding of another token). Returns the definition account (new key in the
        /// wallet).
        pub fn token_new(
            &mut self,
            token_program: [u8; 32],
            holder: [u8; 32],
            supply: u128,
            name: &str,
        ) -> Result<[u8; 32], BackendError> {
            let token_program = AccountId::new(token_program);
            let holder = AccountId::new(holder);
            let mut wallet = self.wallet()?;
            let definition = wallet.create_new_account_public(None).0;
            wallet
                .store_persistent_data()
                .map_err(|e| BackendError::Io(e.to_string()))?;
            let accounts = vec![
                AccountIdentity::Public(definition).select_program_actor_state(token_program),
                Self::identity(&wallet, holder).select_program_actor_state(token_program),
            ];
            drop(wallet);
            let message = borsh::to_vec(&token_core::Message::NewDefinition {
                name: name.to_string(),
                total_supply: supply,
                mint_authority: None,
                holding: holder,
            })
            .expect("borsh");
            self.runtime_send(accounts, message, vec![token_program], None)?;
            Ok(definition.into_value())
        }

        /// Token program helper: an empty holding of `definition` on `account` (a public signer, or
        /// a private account of the wallet, which then pays its own fee).
        pub fn token_init(
            &mut self,
            token_program: [u8; 32],
            definition: [u8; 32],
            account: [u8; 32],
        ) -> Result<(), BackendError> {
            let token_program = AccountId::new(token_program);
            let account = AccountId::new(account);
            let wallet = self.wallet()?;
            let who = Self::identity(&wallet, account);
            let fee_payer = who.is_private().then_some(account);
            let accounts = vec![
                who.select_program_actor_state(token_program),
                Self::nosign(AccountId::new(definition), token_program),
            ];
            drop(wallet);
            let message = borsh::to_vec(&token_core::Message::InitializeHolding {
                definition: AccountId::new(definition),
            })
            .expect("borsh");
            self.runtime_send(accounts, message, vec![token_program], fee_payer)
        }

        /// The wallet's accounts: `(account, private, native balance)`. Public accounts are the
        /// ones it can sign for; private ones are those it holds the keys of. A balance that
        /// cannot be read is reported as 0.
        pub fn wallet_accounts(&self) -> Result<Vec<([u8; 32], bool, u128)>, BackendError> {
            let wallet = self.wallet()?;
            let chain = wallet.storage().key_chain();
            let public: Vec<AccountId> = chain.public_account_ids().map(|(id, _)| id).collect();
            let private: Vec<AccountId> = chain.private_account_ids().map(|(id, _)| id).collect();
            drop(wallet);
            let mut out = Vec::new();
            for (ids, is_private) in [(public, false), (private, true)] {
                for id in ids {
                    let native = self.balances(id.into_value(), None).map_or(0, |(n, _)| n);
                    out.push((id.into_value(), is_private, native));
                }
            }
            Ok(out)
        }

        /// Native and token balances of a public account, or of a private account of the wallet.
        pub fn balances(
            &self,
            account: [u8; 32],
            token_program: Option<[u8; 32]>,
        ) -> Result<(u128, Option<u128>), BackendError> {
            let account = AccountId::new(account);
            let wallet = self.wallet()?;
            let token = |bytes: &[u8]| match token_core::TokenState::decode(bytes) {
                Some(token_core::TokenState::Holding(h)) => h.balance,
                _ => 0,
            };
            if let Some(private) = wallet.get_account_private(account) {
                let native = private
                    .data
                    .native_balance()
                    .map_err(|e| BackendError::Decode(e.to_string()))?;
                let t = token_program.map(|t| token(private.data.actor_state(AccountId::new(t))));
                return Ok((native, t));
            }
            let native_state = self.runtime.block_on(Self::public_state(
                &wallet,
                account,
                NATIVE_TOKEN_PROGRAM_ID,
            ))?;
            let native = lee_core::native_token::decode_balance(&native_state)
                .map_err(|e| BackendError::Decode(e.to_string()))?;
            let t = match token_program {
                Some(t) => Some(token(&self.runtime.block_on(Self::public_state(
                    &wallet,
                    account,
                    AccountId::new(t),
                ))?)),
                None => None,
            };
            Ok((native, t))
        }
    }

    /// The LEZ wallet's own command line (accounts, transfers, `program-loader deploy`, ...), on the
    /// wallet in `LEE_WALLET_HOME_DIR`, against `sequencer`.
    pub fn wallet_cli(sequencer: &str, args: Vec<String>) -> Result<(), String> {
        use clap::Parser as _;
        use wallet::cli::{execute_subcommand, Args};
        use wallet::utils::{fetch_config_path, fetch_persistent_storage_path};
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(async move {
            let overrides = WalletConfigOverrides {
                sequencer: Some(SequencerConnectionData {
                    sequencer_addr: url::Url::parse(sequencer).map_err(|e| e.to_string())?,
                    basic_auth: None,
                }),
                ..WalletConfigOverrides::default()
            };
            let Args {
                command,
                dry_run,
                allow_revert,
                gas_limit,
                max_fee,
                tip,
                fee_headroom,
                fee_payer,
                refund_to,
                ..
            } = Args::parse_from(std::iter::once("vesting wallet".to_owned()).chain(args));
            let config_path = fetch_config_path().map_err(|e| e.to_string())?;
            let storage_path = fetch_persistent_storage_path().map_err(|e| e.to_string())?;
            if let Some(parent) = config_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let command = command.ok_or("give a wallet command")?;
            let mut wallet = if storage_path.exists() {
                let password = read_password(&storage_path).map_err(|e| e.to_string())?;
                WalletCore::new_update_chain(config_path, storage_path, Some(overrides), &password)
                    .map_err(|e| e.to_string())?
            } else {
                let password = std::env::var(wallet::PASSWORD_ENV_VAR).unwrap_or_default();
                let (mut wallet, _mnemonic) = WalletCore::new_init_storage(
                    config_path,
                    storage_path,
                    Some(overrides),
                    &password,
                )
                .map_err(|e| e.to_string())?;
                wallet.start_at_chain_tip().await.ok();
                wallet.store_persistent_data().map_err(|e| e.to_string())?;
                println!("created a new wallet (recovery phrase not shown)");
                wallet
            };
            wallet.set_dry_run(dry_run);
            wallet.set_allow_revert(allow_revert);
            wallet.set_fee_params(wallet::FeeParams {
                gas_limit,
                max_fee,
                tip,
                headroom_blocks: fee_headroom,
                payer: fee_payer,
                refund_to,
            });
            match execute_subcommand(&mut wallet, command).await {
                Err(error) => match error.root_cause().downcast_ref() {
                    Some(ExecutionFailureKind::DryRun(report)) => {
                        println!("{report}");
                        Ok(())
                    }
                    _ => Err(format!("{error:#}")),
                },
                Ok(_) => Ok(()),
            }
        })
    }

    /// The wallet refuses an actor mentioned twice; keep the first mention of each actor.
    fn dedup_mentions(accounts: &mut Vec<AccountMention>) {
        let mut seen = Vec::new();
        accounts.retain(|m| {
            let key = (m.identity.account_id(), m.program_account_id);
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        });
    }

    fn random32() -> Result<[u8; 32], BackendError> {
        use std::io::Read as _;
        let mut out = [0u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut out))
            .map_err(|e| BackendError::Io(e.to_string()))?;
        Ok(out)
    }

    impl Backend for LezBackend {
        /// The chain's clock (the last block's timestamp).
        fn now(&self) -> Timestamp {
            self.runtime
                .block_on(async {
                    let wallet = self.wallet()?;
                    let state = Self::public_state(
                        &wallet,
                        clock::CLOCK_PROGRAM_ACCOUNT_ID,
                        clock::clock_account_id(),
                    )
                    .await?;
                    let (_, timestamp): (u64, u64) = borsh::from_slice(&state)
                        .map_err(|e| BackendError::Decode(e.to_string()))?;
                    Ok::<Timestamp, BackendError>(timestamp)
                })
                .unwrap_or_default()
        }

        fn load(&self, id: ScheduleId) -> Result<Schedule, BackendError> {
            let address = self.address_of(id)?;
            let wallet = self.wallet()?;
            let chain = self
                .chain_state(&wallet, address)?
                .ok_or(BackendError::NotFound(id))?;
            chain
                .schedule
                .ok_or_else(|| BackendError::Decode("schedule is being created".into()))
        }

        fn store(&mut self, id: ScheduleId, state: &Schedule) -> Result<(), BackendError> {
            self.pending = Some((id, state.clone()));
            Ok(())
        }

        fn next_id(&mut self) -> ScheduleId {
            let id = self.map.next;
            self.map.next += 1;
            let _ = self.persist();
            id
        }

        fn list(&self) -> Vec<(ScheduleId, Schedule)> {
            self.map
                .ids
                .keys()
                .filter_map(|id| self.load(*id).ok().map(|state| (*id, state)))
                .collect()
        }

        fn set_claim_target(&mut self, target: [u8; 32]) {
            self.claim_target = Some(AccountId::new(target));
        }

        fn set_caller(&mut self, caller: [u8; 32]) {
            self.caller = Some(caller);
        }

        fn submit(&mut self, event: &Event) -> Result<(), BackendError> {
            let (id, state) = self
                .pending
                .take()
                .ok_or_else(|| BackendError::Io("store() must precede submit()".into()))?;
            match event {
                Event::Created { .. } => self.send_create(&[(id, state)]),
                _ => self.send_op(id, &state, event),
            }
        }

        fn submit_batch(
            &mut self,
            items: &[(ScheduleId, Schedule, Event)],
        ) -> Result<(), BackendError> {
            let creates: Vec<(ScheduleId, Schedule)> =
                items.iter().map(|(id, s, _)| (*id, s.clone())).collect();
            self.send_create(&creates)
        }

        /// The wallet's dry run of the transaction `submit` would send (the same accounts, the
        /// same instruction, simulated at the head fee state; nothing is proven or sent).
        fn estimate_fee(
            &mut self,
            id: ScheduleId,
            state: &Schedule,
            event: &Event,
        ) -> Result<Option<FeeEstimate>, BackendError> {
            let target = self.claim_target;
            let was = self.dry_run;
            self.dry_run = true;
            self.dry_report = None;
            let sent = match event {
                Event::Created { .. } => self.send_create(&[(id, state.clone())]),
                _ => self.send_op(id, state, event),
            };
            self.dry_run = was;
            let report = match (self.dry_report.take(), sent) {
                (Some(report), _) => report,
                (None, Err(error)) => return Err(error),
                (None, Ok(())) => {
                    return Err(BackendError::Io("the dry run reported nothing".into()))
                }
            };
            self.fee_estimate(&report, id, event, target).map(Some)
        }
    }
}
