//! CLI for RFP-017 vesting, driving an in-memory (JSON-file) backend.

mod args;
mod backend;
mod batch;
mod names;
mod render;

use args::{AccountArg, Cli, Command, KindArg};
use backend::{Backend, JsonFileBackend, ScheduleId};
use clap::Parser;
use logos_rpc::{Client, HttpTransport};
use names::{NameBook, NameError};
use std::fs;
use std::io::{self, Write};
use std::sync::Arc;
use vesting_core::{
    cancel, claim, create, create_batch, make_irrevocable, signal_milestone, transfer_beneficiary,
    CreateParams, Event, Schedule, ScheduleKind, VestingError,
};
#[cfg(feature = "lez")]
use vesting_sdk::LezBackend;

fn main() {
    let cli = Cli::parse();
    match execute(&cli) {
        Ok(lines) => {
            for line in lines {
                println!("{}", line);
            }
        }
        Err(message) => {
            eprintln!("{}", message);
            std::process::exit(1);
        }
    }
}

fn build_kind(
    kind: KindArg,
    start: Option<u64>,
    cliff: Option<u64>,
    end: Option<u64>,
    tranches: &[u128],
) -> Result<ScheduleKind, String> {
    match kind {
        KindArg::Linear => Ok(ScheduleKind::Linear {
            start: start.ok_or("--start is required for a linear schedule")?,
            end: end.ok_or("--end is required for a linear schedule")?,
        }),
        KindArg::CliffLinear => Ok(ScheduleKind::CliffLinear {
            start: start.ok_or("--start is required for a cliff-linear schedule")?,
            cliff: cliff.ok_or("--cliff is required for a cliff-linear schedule")?,
            end: end.ok_or("--end is required for a cliff-linear schedule")?,
        }),
        KindArg::Milestone => {
            if tranches.is_empty() {
                return Err("at least one --tranche is required for a milestone schedule".into());
            }
            Ok(ScheduleKind::Milestone {
                tranches: tranches.to_vec(),
            })
        }
    }
}

fn core_error(error: VestingError) -> String {
    render::error(&error)
}

/// LNS configuration and the local reverse-lookup book for one command.
struct NamesCtx {
    client: Option<Client>,
    lns: Option<vesting_core::AccountId>,
    book: NameBook,
    path: std::path::PathBuf,
    now: u64,
}

impl NamesCtx {
    fn from_cli(cli: &Cli) -> Result<Self, String> {
        let book = NameBook::open(&cli.names)?;
        let (client, lns) = match (&cli.lns, cli.lns_rpc.as_deref().or(cli.network.as_deref())) {
            (Some(arg), Some(url)) => {
                let id = parse_address(arg.text())?;
                (
                    Some(Client::new(Arc::new(HttpTransport::new(url)))),
                    Some(id),
                )
            }
            (Some(_), None) => {
                return Err("--lns needs --lns-rpc <url> or --network <url>".into());
            }
            (None, _) => (None, None),
        };
        Ok(Self {
            client,
            lns,
            book,
            path: cli.names.clone(),
            now: cli.now.unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64
            }),
        })
    }

    fn resolve_text(&mut self, text: &str) -> Result<vesting_core::AccountId, String> {
        let client = self.client.as_ref();
        let lns = self.lns;
        let (id, name) = names::resolve(client, lns, &mut self.book, text, self.now)
            .map_err(|error: NameError| error.to_string())?;
        // Print before the action so a mistyped name cannot silently pay a stranger.
        if let Some(name) = name {
            println!("{} resolves to {}", name, names::base58(&id));
        }
        Ok(id)
    }

    fn resolve(&mut self, arg: &AccountArg) -> Result<vesting_core::AccountId, String> {
        self.resolve_text(arg.text())
    }

    fn display(&self, account: &vesting_core::AccountId) -> String {
        self.book.display(account)
    }

    fn save(&self) -> Result<(), String> {
        self.book.save(&self.path)
    }
}

fn parse_address(text: &str) -> Result<vesting_core::AccountId, String> {
    let bytes = hex::decode(text).map_err(|_| format!("bad LNS program address '{}'", text))?;
    if bytes.len() != 32 {
        return Err(format!("bad LNS program address '{}'", text));
    }
    let mut account = [0u8; 32];
    account.copy_from_slice(&bytes);
    Ok(account)
}

/// `--network` + `--wallet` select the LEZ backend; the JSON file stays the default.
fn execute(cli: &Cli) -> Result<Vec<String>, String> {
    let mut ctx = NamesCtx::from_cli(cli)?;
    let result = execute_with(cli, &mut ctx);
    // Keep name → address mappings even when an action was refused: resolving
    // a name never sends anything by itself.
    ctx.save()?;
    result
}

fn execute_with(cli: &Cli, ctx: &mut NamesCtx) -> Result<Vec<String>, String> {
    if cli.network.is_some() || cli.wallet.is_some() {
        let (Some(network), Some(wallet)) = (&cli.network, &cli.wallet) else {
            return Err("--network and --wallet must be given together".into());
        };
        #[cfg(feature = "lez")]
        if let Command::Wallet { args } = &cli.command {
            std::env::set_var("LEE_WALLET_HOME_DIR", wallet);
            vesting_sdk::lez::wallet_cli(network, args.clone())?;
            return Ok(Vec::new());
        }
        let needs_program = !matches!(
            cli.command,
            Command::Deploy { .. }
                | Command::TokenNew { .. }
                | Command::TokenInit { .. }
                | Command::Balance { .. }
        );
        let program = match &cli.program {
            Some(account) => ctx.resolve(account)?,
            None if !needs_program => [0u8; 32],
            None => return Err("--program <account> is required with --network".into()),
        };
        #[cfg(feature = "lez")]
        {
            let mut backend = LezBackend::open(
                network,
                wallet,
                program,
                cli.clock_elf.clone(),
                &cli.lez_map,
            )
            .map_err(|error| error.to_string())?;
            let token_program = match &cli.token_program {
                Some(account) => Some(ctx.resolve(account)?),
                None => None,
            };
            if let Some(token_program) = token_program {
                backend.set_token_program(token_program);
            }
            if cli.sponsor_deposit > 0 {
                backend.set_sponsor(cli.sponsor_deposit, cli.sponsor_per_claim);
            }
            backend.set_dry_run(cli.dry_run);
            backend.set_gas_limit(cli.gas_limit);
            match &cli.command {
                Command::Deploy {
                    elf,
                    payer,
                    immutable,
                } => {
                    let bytes =
                        fs::read(elf).map_err(|error| format!("{}: {}", elf.display(), error))?;
                    let (id, image) = backend
                        .deploy(&bytes, ctx.resolve(payer)?, *immutable)
                        .map_err(|error| error.to_string())?;
                    return Ok(vec![
                        format!("program {}", hex::encode(id)),
                        format!("image {}", image),
                        format!(
                            "{}",
                            if *immutable {
                                "immutable"
                            } else {
                                "updatable: the header key in this wallet can replace it"
                            }
                        ),
                    ]);
                }
                Command::TokenNew {
                    holder,
                    supply,
                    name,
                } => {
                    let program = token_program.ok_or("--token-program is required")?;
                    let definition = backend
                        .token_new(program, ctx.resolve(holder)?, *supply, name)
                        .map_err(|error| error.to_string())?;
                    return Ok(vec![format!("definition {}", hex::encode(definition))]);
                }
                Command::TokenInit {
                    definition,
                    account,
                } => {
                    let program = token_program.ok_or("--token-program is required")?;
                    let (definition, account) = (ctx.resolve(definition)?, ctx.resolve(account)?);
                    backend
                        .token_init(program, definition, account)
                        .map_err(|error| error.to_string())?;
                    return Ok(vec![format!(
                        "holding of {} on {}",
                        hex::encode(definition),
                        ctx.display(&account)
                    )]);
                }
                Command::Balance { account } => {
                    let (native, token) = backend
                        .balances(ctx.resolve(account)?, token_program)
                        .map_err(|error| error.to_string())?;
                    return Ok(vec![match token {
                        Some(token) => format!("native {} token {}", native, token),
                        None => format!("native {}", native),
                    }]);
                }
                Command::Address { schedule } => {
                    let address = backend
                        .address(*schedule)
                        .map_err(|error| error.to_string())?;
                    return Ok(vec![hex::encode(address)]);
                }
                _ => {}
            }
            if let Command::Import { account } = &cli.command {
                let account = ctx.resolve(account)?;
                let id = backend.import(account).map_err(|error| error.to_string())?;
                return Ok(vec![format!(
                    "imported schedule {} as {}",
                    hex::encode(account),
                    id
                )]);
            }
            // On LEZ the time comes from the chain's clock; --now applies to the JSON backend.
            let now = backend.now();
            run(&mut backend, cli, now, ctx)
        }
        #[cfg(not(feature = "lez"))]
        {
            let _ = (network, wallet, program, ctx);
            Err("this CLI was built without the `lez` feature: rebuild with --features lez".into())
        }
    } else {
        let mut backend =
            JsonFileBackend::open(&cli.state, cli.now).map_err(|error| error.to_string())?;
        let now = backend.now();
        run(&mut backend, cli, now, ctx)
    }
}

fn load<B: Backend>(backend: &B, id: ScheduleId) -> Result<Schedule, String> {
    backend.load(id).map_err(|error| error.to_string())
}

fn save_result<B: Backend>(
    backend: &mut B,
    caller: vesting_core::AccountId,
    id: ScheduleId,
    state: &Schedule,
    event: &Event,
) -> Result<(), String> {
    backend.set_caller(caller);
    backend
        .store(id, state)
        .map_err(|error| error.to_string())?;
    backend.submit(event).map_err(|error| error.to_string())
}

fn confirm(disclosure: &str) -> Result<bool, String> {
    println!("{}", disclosure);
    print!("Proceed? [y/N] ");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut answer = String::new();
    match io::stdin().read_line(&mut answer) {
        Ok(0) => Ok(false),
        Ok(_) => Ok(matches!(
            answer.trim().to_ascii_lowercase().as_str(),
            "y" | "yes"
        )),
        Err(error) => Err(error.to_string()),
    }
}

fn claim_failure(error: VestingError) -> String {
    let mut lines = vec![render::error(&error)];
    if let VestingError::NothingClaimable { reason, .. } = &error {
        lines.push(format!("status: {}", render::waiting_reason(reason)));
    }
    lines.join("\n")
}

fn run<B: Backend>(
    backend: &mut B,
    cli: &Cli,
    now: u64,
    ctx: &mut NamesCtx,
) -> Result<Vec<String>, String> {
    match &cli.command {
        Command::Create {
            creator,
            beneficiary,
            token,
            total,
            kind,
            start,
            cliff,
            end,
            tranches,
            cancelable,
            transferable,
            cancel_authority,
            milestone_authority,
        } => {
            let schedule_kind = build_kind(*kind, *start, *cliff, *end, tranches)?;
            let params = CreateParams {
                creator: ctx.resolve(creator)?,
                beneficiary: ctx.resolve(beneficiary)?,
                token: ctx.resolve(token)?,
                total: *total,
                kind: schedule_kind,
                cancelable: Some(*cancelable),
                transferable: Some(*transferable),
                cancel_authority: match cancel_authority {
                    Some(account) => Some(ctx.resolve(account)?),
                    None => None,
                },
                milestone_authority: match milestone_authority {
                    Some(account) => Some(ctx.resolve(account)?),
                    None => None,
                },
            };
            let (state, event) = create(params, now).map_err(core_error)?;
            let id = backend.next_id();
            save_result(backend, state.creator, id, &state, &event)?;
            let state = load(backend, id).unwrap_or(state);
            Ok(vec![
                format!("created schedule {}", id),
                render::show(id, &state, now, |id| ctx.display(id)),
            ])
        }

        Command::CreateBatch { file } => {
            let bytes = fs::read(file).map_err(|error| format!("batch file: {}", error))?;
            let items: Vec<batch::BatchParams> =
                serde_json::from_slice(&bytes).map_err(|error| format!("batch file: {}", error))?;
            let mut params = Vec::with_capacity(items.len());
            for item in items {
                params.push(item.into_params_with(&mut |text| ctx.resolve_text(text))?);
            }
            let (states, events) = create_batch(params, now).map_err(core_error)?;
            let items: Vec<(ScheduleId, Schedule, Event)> = states
                .into_iter()
                .zip(events)
                .map(|(state, event)| (backend.next_id(), state, event))
                .collect();
            if let Some((_, first, _)) = items.first() {
                backend.set_caller(first.creator);
            }
            // One transaction on LEZ (RFP Functionality 5).
            backend
                .submit_batch(&items)
                .map_err(|error| error.to_string())?;
            Ok(items
                .iter()
                .map(|(id, _, _)| format!("created schedule {}", id))
                .collect())
        }

        Command::Claimable { schedule } => {
            let state = load(backend, *schedule)?;
            let mut lines = vec![format!(
                "claimable now: {} of {}",
                state.claimable_at(now),
                state.total
            )];
            lines.push(render::next_unlock_line(&state, now));
            Ok(lines)
        }

        Command::Claim {
            schedule,
            to_private,
            yes,
            as_account,
        } => {
            let state = load(backend, *schedule)?;
            let caller = match as_account {
                Some(account) => ctx.resolve(account)?,
                None => state.beneficiary,
            };
            let (to_private, destination) = match to_private {
                Some(account) => {
                    // The LEZ backend asks the wallet whether this account is private; a flag
                    // cannot make an account private. This only names the destination.
                    let target = ctx.resolve(account)?;
                    backend.set_claim_target(target);
                    (true, ctx.display(&target))
                }
                None => (false, "(public account)".to_string()),
            };
            if to_private {
                let disclosure = render::privacy_disclosure(
                    state.claimable_at(now),
                    &ctx.display(&state.beneficiary),
                    *schedule,
                    &destination,
                );
                if !*yes && !confirm(&disclosure)? {
                    return Ok(vec!["cancelled: no claim was submitted".to_string()]);
                }
                if *yes {
                    println!("{}", disclosure);
                }
            }
            let (next, _local, event) =
                claim(&state, caller, now, to_private).map_err(claim_failure)?;
            save_result(backend, caller, *schedule, &next, &event)?;
            // What the ledger (on LEZ, the chain) recorded, not the local estimate.
            let next = load(backend, *schedule)?;
            let amount = next.claimed.saturating_sub(state.claimed);
            Ok(vec![
                format!("claimed {}", amount),
                format!("claimed in total: {}", next.claimed),
                if to_private {
                    format!("destination: {} (private)", destination)
                } else {
                    "destination: public account".to_string()
                },
            ])
        }

        Command::Cancel {
            schedule,
            as_account,
        } => {
            let state = load(backend, *schedule)?;
            let caller = match as_account {
                Some(account) => ctx.resolve(account)?,
                None => state.cancel_authority,
            };
            let (next, _local, event) = cancel(&state, caller, now).map_err(core_error)?;
            save_result(backend, caller, *schedule, &next, &event)?;
            let next = load(backend, *schedule)?;
            let refund = next.cancelled.as_ref().map_or(0, |record| {
                state.total.saturating_sub(record.vested_at_cancel)
            });
            Ok(vec![
                format!("cancelled schedule {}", schedule),
                format!("refunded {} to the creator", refund),
                format!(
                    "{} remains claimable by the beneficiary",
                    next.claimable_at(now)
                ),
            ])
        }

        Command::MakeIrrevocable {
            schedule,
            as_account,
        } => {
            let state = load(backend, *schedule)?;
            let caller = match as_account {
                Some(account) => ctx.resolve(account)?,
                None => state.cancel_authority,
            };
            let (next, event) = make_irrevocable(&state, caller, now).map_err(core_error)?;
            save_result(backend, caller, *schedule, &next, &event)?;
            Ok(vec![format!("schedule {} is now irrevocable", schedule)])
        }

        Command::SignalMilestone {
            schedule,
            index,
            as_account,
        } => {
            let state = load(backend, *schedule)?;
            let caller = match as_account {
                Some(account) => ctx.resolve(account)?,
                None => state.milestone_authority,
            };
            let (next, amount, event) =
                signal_milestone(&state, caller, *index, now).map_err(core_error)?;
            save_result(backend, caller, *schedule, &next, &event)?;
            Ok(vec![format!(
                "signalled milestone {}: unlocked {}",
                index, amount
            )])
        }

        Command::Transfer {
            schedule,
            new_beneficiary,
            as_account,
        } => {
            let state = load(backend, *schedule)?;
            let caller = match as_account {
                Some(account) => ctx.resolve(account)?,
                None => state.beneficiary,
            };
            let (next, event) =
                transfer_beneficiary(&state, caller, ctx.resolve(new_beneficiary)?, now)
                    .map_err(core_error)?;
            save_result(backend, caller, *schedule, &next, &event)?;
            Ok(vec![format!(
                "beneficiary of schedule {} is now {}",
                schedule,
                ctx.display(&next.beneficiary)
            )])
        }

        Command::Import { .. }
        | Command::Deploy { .. }
        | Command::Wallet { .. }
        | Command::TokenNew { .. }
        | Command::TokenInit { .. }
        | Command::Balance { .. }
        | Command::Address { .. } => {
            Err("this command needs the LEZ backend (--network, --wallet)".into())
        }

        Command::Show { schedule } => {
            let state = load(backend, *schedule)?;
            Ok(vec![render::show(*schedule, &state, now, |id| {
                ctx.display(id)
            })])
        }

        Command::List {
            creator,
            beneficiary,
        } => {
            let creator = match creator {
                Some(arg) => Some(ctx.resolve(arg)?),
                None => None,
            };
            let beneficiary = match beneficiary {
                Some(arg) => Some(ctx.resolve(arg)?),
                None => None,
            };
            let mut lines = Vec::new();
            for (id, state) in backend.list() {
                if let Some(wanted) = creator {
                    if state.creator != wanted {
                        continue;
                    }
                }
                if let Some(wanted) = beneficiary {
                    if state.beneficiary != wanted {
                        continue;
                    }
                }
                lines.push(render::show(id, &state, now, |id| ctx.display(id)));
            }
            if lines.is_empty() {
                lines.push("no schedules".to_string());
            }
            Ok(lines)
        }
    }
}
