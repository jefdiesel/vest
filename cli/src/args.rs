//! Command-line argument definitions.
//!
//! Account arguments are raw text: a 64-hex address or an LNS name. They are
//! resolved at the moment of the action in `names.rs`.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::str::FromStr;

/// An account address or an LNS name. Validation happens during resolution.
#[derive(Clone, Debug)]
pub struct AccountArg(pub String);

impl FromStr for AccountArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.trim().is_empty() {
            return Err("an account or name cannot be empty".to_string());
        }
        Ok(AccountArg(text.to_string()))
    }
}

impl AccountArg {
    pub fn text(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum KindArg {
    Linear,
    CliffLinear,
    Milestone,
}

#[derive(Parser, Debug)]
#[command(
    name = "vesting",
    about = "Create and manage RFP-017 vesting schedules"
)]
pub struct Cli {
    /// JSON file holding the in-memory ledger.
    #[arg(long, global = true, default_value = "vesting-state.json")]
    pub state: PathBuf,

    /// Use this time (milliseconds) instead of the system clock.
    #[arg(long, global = true)]
    pub now: Option<u64>,

    /// LEZ sequencer URL. With --wallet, selects the real LEZ backend; JSON stays default.
    #[arg(long, global = true)]
    pub network: Option<String>,

    /// Wallet directory holding wallet_config.json and storage.json (for --network).
    #[arg(long, global = true)]
    pub wallet: Option<PathBuf>,

    /// Deployed vesting program account (required with --network).
    #[arg(long, global = true)]
    pub program: Option<AccountArg>,

    /// Clock ELF for private claims on LEZ (the clock is a builtin program).
    #[arg(long, global = true)]
    pub clock_elf: Option<PathBuf>,

    /// The 0.4 token program, for token schedules (`--token` other than zero) on LEZ.
    #[arg(long, global = true)]
    pub token_program: Option<AccountArg>,

    /// Token schedules on LEZ: native LEZ the creator deposits to sponsor private-claim fees.
    #[arg(long, global = true, default_value_t = 0)]
    pub sponsor_deposit: u128,

    /// Token schedules on LEZ: native LEZ paid to each private claim from that deposit.
    #[arg(long, global = true, default_value_t = 0)]
    pub sponsor_per_claim: u128,

    /// LEZ: the most execution gas a public transaction may declare (the wallet's default is
    /// 2,000,000; LEZ rc3's cap is 10,000,000). Large batches need more than the default.
    #[arg(long, global = true)]
    pub gas_limit: Option<u64>,

    /// LEZ: simulate the transaction and print the wallet's report (cycles, bytes, fee).
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Local file mapping SDK ids to on-chain schedule accounts (LEZ backend).
    #[arg(long, global = true, default_value = "vesting-lez-ids.json")]
    pub lez_map: PathBuf,

    /// Deployed LNS program account, needed only when an argument is a name.
    #[arg(long, global = true)]
    pub lns: Option<AccountArg>,

    /// RPC endpoint for LNS lookups (defaults to --network when present).
    #[arg(long, global = true)]
    pub lns_rpc: Option<String>,

    /// Local name book for reverse lookups in show/list/events.
    #[arg(long, global = true, default_value = "vesting-names.json")]
    pub names: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create one vesting schedule.
    Create {
        #[arg(long)]
        creator: AccountArg,
        #[arg(long)]
        beneficiary: AccountArg,
        #[arg(long)]
        token: AccountArg,
        #[arg(long)]
        total: u128,
        #[arg(long, value_enum)]
        kind: KindArg,
        #[arg(long)]
        start: Option<u64>,
        #[arg(long)]
        cliff: Option<u64>,
        #[arg(long)]
        end: Option<u64>,
        #[arg(long = "tranche")]
        tranches: Vec<u128>,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        cancelable: bool,
        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        transferable: bool,
        #[arg(long)]
        cancel_authority: Option<AccountArg>,
        #[arg(long)]
        milestone_authority: Option<AccountArg>,
    },

    /// Create several schedules from a JSON file.
    CreateBatch {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },

    /// Show how much is claimable now, and when the next unlock is.
    Claimable { schedule: u64 },

    /// Claim everything currently vested.
    Claim {
        schedule: u64,
        /// Claim to this private account instead of a public one.
        #[arg(long)]
        to_private: Option<AccountArg>,
        /// Skip the privacy confirmation prompt.
        #[arg(long)]
        yes: bool,
        /// Act as this account (defaults to the beneficiary).
        #[arg(long)]
        as_account: Option<AccountArg>,
    },

    /// Cancel a schedule and refund the unvested balance.
    Cancel {
        schedule: u64,
        #[arg(long)]
        as_account: Option<AccountArg>,
    },

    /// Make a cancelable schedule irrevocable.
    MakeIrrevocable {
        schedule: u64,
        #[arg(long)]
        as_account: Option<AccountArg>,
    },

    /// Signal one milestone tranche.
    SignalMilestone {
        schedule: u64,
        index: usize,
        #[arg(long)]
        as_account: Option<AccountArg>,
    },

    /// Transfer the beneficiary rights to another account.
    Transfer {
        schedule: u64,
        new_beneficiary: AccountArg,
        #[arg(long)]
        as_account: Option<AccountArg>,
    },

    /// Show the full state of one schedule.
    Show { schedule: u64 },

    /// LEZ: the wallet's own command line (accounts, transfers, `program-loader deploy`, ...).
    Wallet {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// LEZ: deploy a program ELF (updatable unless --immutable); prints its id and image id.
    Deploy {
        #[arg(long)]
        elf: PathBuf,
        #[arg(long)]
        payer: AccountArg,
        #[arg(long)]
        immutable: bool,
    },

    /// LEZ: a new token of `--token-program` with its whole supply in `--holder` (dev and tests).
    TokenNew {
        #[arg(long)]
        holder: AccountArg,
        #[arg(long)]
        supply: u128,
        #[arg(long, default_value = "Vesting test token")]
        name: String,
    },

    /// LEZ: an empty holding of `--definition` on `--account` (public signer or private account).
    TokenInit {
        #[arg(long)]
        definition: AccountArg,
        #[arg(long)]
        account: AccountArg,
    },

    /// LEZ: native (and with --token-program, token) balance of an account.
    Balance { account: AccountArg },

    /// LEZ: the on-chain account of a schedule id.
    Address { schedule: u64 },

    /// LEZ: give an existing on-chain schedule account a local id.
    Import { account: AccountArg },

    /// List all schedules, optionally filtered by creator or beneficiary.
    List {
        #[arg(long)]
        creator: Option<AccountArg>,
        #[arg(long)]
        beneficiary: Option<AccountArg>,
    },
}
