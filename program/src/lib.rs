//! The RFP-017 vesting program for LEZ v0.4.0-rc3: a thin shell around `vesting-core`, which holds
//! every rule. This crate only routes messages, reads the clock, keeps state and moves tokens.
//!
//! # Accounts
//!
//! A schedule lives at `S = PDA(V, SHA256(SEED_TAG | creator | nonce))`. Its state is this
//! program's actor `(S, V)` ([`ScheduleState`]). Its escrow is another program's actor on the same
//! account: `(S, native)` for native LEZ, or the token holding `(S, T)` for a token of the 0.4
//! token program `T`. For a token schedule `(S, native)` holds the optional fee float. Only
//! deliveries from `(S, V)` that carry the seed are authorized on S.
//!
//! # Messages
//!
//! Every operation is a root call to the caller's own actor `(X, V)` ([`Instruction`]), which must
//! be authorized (X signed, or a program holding X's seed called it). It forwards an [`Internal`]
//! message to `(S, V)`, which takes the caller from `input.from` (set by the executor). `(S, V)`
//! stashes the request ([`Pending`]), reads the clock, and runs the `vesting-core` operation in the
//! clock's reply, accepted only from `clock_actor()`. `ClaimAsOf` needs no clock: the caller names
//! `t`, the program runs the core claim at `t` and makes the transaction valid only in blocks with
//! timestamp >= `t` (a private claim's proof then never expires). Any core error panics, which
//! reverts the whole transaction.
//!
//! # Private claims of a token
//!
//! The destination is a one-time alias of the private account, which the transaction authorizes.
//! The program initializes its holding (a no-op when it exists) before the transfer, and pays the
//! schedule's per-claim fee sponsorship (native LEZ from the float) to the same alias, so a fresh
//! private account can pay its in-proof fee.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{
    account::{AccountId, Actor},
    native_token,
    program::{PdaSeed, ProgramEvent, ReceiveInput, Response, SyncMessage},
    Timestamp,
};
use vesting_core as core;

pub mod clock {
    //! The clock's interface, vendored from LEZ rc3 `lez/programs/clock/core` (same borsh layout).
    use super::*;
    pub const CLOCK_PROGRAM_ACCOUNT_ID: AccountId =
        AccountId::new(*b"/LEZ/ClockProgramAccount/0000000");
    #[derive(Debug, Clone, Copy, BorshSerialize, BorshDeserialize)]
    pub enum Message {
        Tick { timestamp: Timestamp, block_id: u64 },
        ReadBlockId { granularity: u64 },
        ReadTimestamp { granularity: Timestamp },
    }
    #[derive(Debug, Clone, Copy, BorshSerialize, BorshDeserialize)]
    pub enum Reply {
        BlockId(u64),
        Timestamp(Timestamp),
    }
    #[must_use]
    pub fn clock_account_id() -> AccountId {
        AccountId::from_builtin_program_name(b"clock")
    }
    #[must_use]
    pub fn clock_actor() -> Actor {
        Actor::new(CLOCK_PROGRAM_ACCOUNT_ID, clock_account_id())
    }
}

/// What a schedule escrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Asset {
    Native,
    /// A token of the 0.4 token program `program` (logos-blockchain/lez-programs).
    Token {
        program: AccountId,
        definition: AccountId,
    },
}

/// Fee sponsorship of private token claims, funded by the creator at creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Sponsor {
    /// Native LEZ the creator moves into `(S, native)` at creation.
    pub deposit: u128,
    /// Native LEZ paid to the claim's one-time alias on each private token claim.
    pub per_claim: u128,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NewSchedule {
    pub nonce: [u8; 32],
    pub beneficiary: AccountId,
    pub asset: Asset,
    pub total: u128,
    pub kind: core::ScheduleKind,
    /// `None`: the RFP defaults (cancelable, not transferable, authorities = creator).
    pub cancelable: Option<bool>,
    pub transferable: Option<bool>,
    pub cancel_authority: Option<AccountId>,
    pub milestone_authority: Option<AccountId>,
    /// Token schedules only.
    pub sponsor: Option<Sponsor>,
}

/// Root calls to the caller's own actor `(X, V)`. Append-only.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Instruction {
    Create {
        schedules: Vec<NewSchedule>,
    },
    /// Read the clock (rounded down to `granularity` ms) and pay what vested to `to`.
    Claim {
        schedule: AccountId,
        to: AccountId,
        to_private: bool,
        granularity: Timestamp,
    },
    /// Pay what vested by `as_of`; valid only in blocks with timestamp >= `as_of`.
    ClaimAsOf {
        schedule: AccountId,
        to: AccountId,
        to_private: bool,
        as_of: Timestamp,
    },
    Cancel {
        schedule: AccountId,
    },
    MakeIrrevocable {
        schedule: AccountId,
    },
    SignalMilestone {
        schedule: AccountId,
        index: u32,
    },
    TransferBeneficiary {
        schedule: AccountId,
        new_beneficiary: AccountId,
    },
}

/// Operations forwarded from a caller's actor to the schedule actor.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Op {
    Claim { to: AccountId, to_private: bool },
    Cancel,
    MakeIrrevocable,
    SignalMilestone { index: u32 },
    TransferBeneficiary { new_beneficiary: AccountId },
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Internal {
    Create(NewSchedule),
    /// Run `op` at the clock's time, read at `granularity` ms.
    AtClock {
        op: Op,
        granularity: Timestamp,
    },
    /// Run a claim at the caller's `as_of`.
    ClaimAsOf {
        to: AccountId,
        to_private: bool,
        as_of: Timestamp,
    },
}

/// A clock read in flight; set and cleared inside one transaction.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Pending {
    Create {
        creator: AccountId,
        params: NewSchedule,
    },
    Op {
        caller: AccountId,
        op: Op,
    },
}

pub const STATE_VERSION: u8 = 1;

/// The state at `(S, V)`: `vesting-core`'s schedule verbatim, plus what the chain needs.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ScheduleState {
    pub version: u8,
    pub nonce: [u8; 32],
    pub asset: Asset,
    /// `None` only while the creating clock read is in flight.
    pub schedule: Option<core::Schedule>,
    /// Remaining fee float (token schedules) and what each private claim draws.
    pub float: u128,
    pub per_claim: u128,
    pub pending: Option<Pending>,
}

/// Events: `vesting-core`'s event for each transition, with the schedule it happened to.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct VestingEvent {
    pub schedule: AccountId,
    pub event: core::Event,
}

pub const SEED_TAG: &[u8] = b"/RFP017/vesting/schedule/v1";
/// Read granularity for every operation but a clock claim, fixed here: a coarse one would round
/// "now" down, and a cancel would then take back tokens that had already vested.
pub const FIXED_GRANULARITY: Timestamp = 1;

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    use risc0_zkvm::sha::{Impl, Sha256 as _};
    Impl::hash_bytes(&parts.concat())
        .as_bytes()
        .try_into()
        .expect("32 bytes")
}

#[must_use]
pub fn schedule_seed(creator: &AccountId, nonce: &[u8; 32]) -> PdaSeed {
    PdaSeed::new(sha256(&[SEED_TAG, creator.as_ref(), nonce]))
}

#[must_use]
pub fn schedule_address(program: &AccountId, creator: &AccountId, nonce: &[u8; 32]) -> AccountId {
    AccountId::for_public_pda(program, &schedule_seed(creator, nonce))
}

/// `vesting::<Variant>`: the first 8 bytes of its SHA-256.
#[must_use]
pub fn selector(e: &core::Event) -> [u8; 8] {
    let name = match e {
        core::Event::Created { .. } => "vesting::Created",
        core::Event::Claimed { .. } => "vesting::Claimed",
        core::Event::Cancelled { .. } => "vesting::Cancelled",
        core::Event::MilestoneSignalled { .. } => "vesting::MilestoneSignalled",
        core::Event::BeneficiaryTransferred { .. } => "vesting::BeneficiaryTransferred",
        core::Event::MadeIrrevocable { .. } => "vesting::MadeIrrevocable",
    };
    let mut out = [0u8; 8];
    out.copy_from_slice(&sha256(&[name.as_bytes()])[..8]);
    out
}

fn event(schedule: AccountId, e: core::Event) -> ProgramEvent {
    ProgramEvent {
        selector: selector(&e),
        data: borsh::to_vec(&VestingEvent { schedule, event: e }).expect("borsh"),
    }
}

fn id(a: &AccountId) -> core::AccountId {
    *a.value()
}

fn fail(e: &core::VestingError) -> ! {
    panic!("vesting error {}: {e}", e.code())
}

/// The guest entry point (`actor_entrypoint!(raw ..)`): dispatch on the sender before decoding.
pub fn handle(input: &ReceiveInput) -> Response {
    if input.from == Some(clock::clock_actor()) {
        return on_clock(input);
    }
    if input.from_own_program() {
        return on_internal(input);
    }
    on_instruction(input)
}

fn on_instruction(input: &ReceiveInput) -> Response {
    assert!(
        input.is_authorized,
        "vesting: {} did not authorize this call",
        input.receiver.account_id
    );
    let me = input.receiver.account_id;
    let program = input.receiver.program_account_id;
    let to = |s: AccountId, m: &Internal| SyncMessage::new(Actor::new(s, program), m);
    let at_clock = |s: AccountId, op: Op, granularity: Timestamp| {
        Response::keep_state().send(to(s, &Internal::AtClock { op, granularity }))
    };
    match borsh::from_slice::<Instruction>(&input.message).expect("vesting: not an instruction") {
        Instruction::Create { schedules } => {
            assert!(!schedules.is_empty(), "vesting: empty batch");
            assert!(
                schedules.len() <= core::MAX_BATCH,
                "vesting error 7: at most {} schedules per transaction",
                core::MAX_BATCH
            );
            let mut r = Response::keep_state();
            for n in schedules {
                let seed = schedule_seed(&me, &n.nonce);
                let s = AccountId::for_public_pda(&program, &seed);
                let (asset, total, sponsor) = (n.asset, n.total, n.sponsor);
                // The seeded message admits S and writes the schedule before anything credits it.
                r = r.send(to(s, &Internal::Create(n)).with_pda_seeds([seed]));
                r = match asset {
                    Asset::Native => {
                        assert!(
                            sponsor.is_none(),
                            "vesting: a fee sponsor is for token schedules"
                        );
                        r.send_sync(
                            Actor::native_balance(me),
                            &native_token::Message::Transfer {
                                to: s,
                                amount: total,
                            },
                        )
                    }
                    Asset::Token {
                        program: t,
                        definition,
                    } => {
                        r = r
                            .send(
                                SyncMessage::new(
                                    Actor::new(s, t),
                                    &token_core::Message::InitializeHolding { definition },
                                )
                                .with_pda_seeds([seed]),
                            )
                            .send_sync(
                                Actor::new(me, t),
                                &token_core::Message::Transfer {
                                    to: s,
                                    amount: total,
                                },
                            );
                        match sponsor {
                            Some(sp) if sp.deposit > 0 => r.send_sync(
                                Actor::native_balance(me),
                                &native_token::Message::Transfer {
                                    to: s,
                                    amount: sp.deposit,
                                },
                            ),
                            _ => r,
                        }
                    }
                };
            }
            r
        }
        Instruction::Claim {
            schedule,
            to: dest,
            to_private,
            granularity,
        } => at_clock(
            schedule,
            Op::Claim {
                to: dest,
                to_private,
            },
            granularity.max(1),
        ),
        Instruction::ClaimAsOf {
            schedule,
            to: dest,
            to_private,
            as_of,
        } => Response::keep_state().send(to(
            schedule,
            &Internal::ClaimAsOf {
                to: dest,
                to_private,
                as_of,
            },
        )),
        Instruction::Cancel { schedule } => at_clock(schedule, Op::Cancel, FIXED_GRANULARITY),
        Instruction::MakeIrrevocable { schedule } => {
            at_clock(schedule, Op::MakeIrrevocable, FIXED_GRANULARITY)
        }
        Instruction::SignalMilestone { schedule, index } => {
            at_clock(schedule, Op::SignalMilestone { index }, FIXED_GRANULARITY)
        }
        Instruction::TransferBeneficiary {
            schedule,
            new_beneficiary,
        } => at_clock(
            schedule,
            Op::TransferBeneficiary { new_beneficiary },
            FIXED_GRANULARITY,
        ),
    }
}

fn load(input: &ReceiveInput) -> ScheduleState {
    assert!(
        !input.pre_state.is_empty(),
        "vesting: {} is not a schedule",
        input.receiver.account_id
    );
    let s: ScheduleState = borsh::from_slice(&input.pre_state).expect("vesting: schedule state");
    assert_eq!(s.version, STATE_VERSION, "vesting: state version");
    s
}

fn save(s: &ScheduleState) -> Response {
    Response::set_state(borsh::to_vec(s).expect("borsh"))
}

fn read_clock(s: &ScheduleState, granularity: Timestamp) -> Response {
    save(s).send_sync(
        clock::clock_actor(),
        &clock::Message::ReadTimestamp { granularity },
    )
}

fn on_internal(input: &ReceiveInput) -> Response {
    let me = input.receiver.account_id;
    let caller = input
        .from
        .expect("an internal message has a sender")
        .account_id;
    match borsh::from_slice::<Internal>(&input.message).expect("vesting: internal message") {
        Internal::Create(params) => {
            assert!(input.pre_state.is_empty(), "vesting: schedule {me} exists");
            assert_eq!(
                me,
                schedule_address(&input.receiver.program_account_id, &caller, &params.nonce),
                "vesting: address does not match creator and nonce"
            );
            let (float, per_claim) = params
                .sponsor
                .map_or((0, 0), |sp| (sp.deposit, sp.per_claim));
            let s = ScheduleState {
                version: STATE_VERSION,
                nonce: params.nonce,
                asset: params.asset,
                schedule: None,
                float,
                per_claim,
                pending: Some(Pending::Create {
                    creator: caller,
                    params,
                }),
            };
            read_clock(&s, FIXED_GRANULARITY)
        }
        Internal::AtClock { op, granularity } => {
            let mut s = load(input);
            assert!(
                s.pending.is_none(),
                "vesting: a clock read is already pending"
            );
            s.pending = Some(Pending::Op { caller, op });
            read_clock(&s, granularity)
        }
        Internal::ClaimAsOf {
            to,
            to_private,
            as_of,
        } => {
            let s = load(input);
            assert!(s.pending.is_none(), "vesting: a clock read is pending");
            run_op(me, s, caller, Op::Claim { to, to_private }, as_of).timestamp_window(as_of..)
        }
    }
}

fn on_clock(input: &ReceiveInput) -> Response {
    let me = input.receiver.account_id;
    let clock::Reply::Timestamp(now) =
        borsh::from_slice(&input.message).expect("vesting: clock reply")
    else {
        panic!("vesting: reads only the timestamp");
    };
    let mut s = load(input);
    match s
        .pending
        .take()
        .expect("vesting: a clock reply with no read pending")
    {
        Pending::Create { creator, params } => {
            let p = core::CreateParams {
                creator: id(&creator),
                beneficiary: id(&params.beneficiary),
                token: match params.asset {
                    Asset::Native => id(&native_token::NATIVE_TOKEN_PROGRAM_ID),
                    Asset::Token { definition, .. } => id(&definition),
                },
                total: params.total,
                kind: params.kind,
                cancelable: params.cancelable,
                transferable: params.transferable,
                cancel_authority: params.cancel_authority.as_ref().map(id),
                milestone_authority: params.milestone_authority.as_ref().map(id),
            };
            let (state, e) = core::create(p, now).unwrap_or_else(|e| fail(&e));
            s.schedule = Some(state);
            save(&s).event(event(me, e))
        }
        Pending::Op { caller, op } => run_op(me, s, caller, op, now),
    }
}

/// Runs one core operation at time `t` and makes its payments.
fn run_op(
    me: AccountId,
    mut s: ScheduleState,
    caller: AccountId,
    op: Op,
    t: Timestamp,
) -> Response {
    let state = s.schedule.take().expect("vesting: schedule not created");
    let creator = AccountId::new(state.creator);
    let seed = schedule_seed(&creator, &s.nonce);
    let mut sends: Vec<SyncMessage> = Vec::new();
    let (next, e) = match op {
        Op::Claim { to, to_private } => {
            let (next, amount, e) =
                core::claim(&state, id(&caller), t, to_private).unwrap_or_else(|e| fail(&e));
            match s.asset {
                Asset::Native => sends.push(native_token::custody_transfer(me, seed, to, amount)),
                Asset::Token {
                    program,
                    definition,
                } => {
                    if to_private {
                        // The alias's holding (a no-op when it exists): the transaction authorizes
                        // the private account, the definition answers through the alias.
                        sends.push(SyncMessage::new(
                            Actor::new(to, program),
                            &token_core::Message::InitializeHolding { definition },
                        ));
                        let draw = s.per_claim.min(s.float);
                        if draw > 0 {
                            s.float -= draw;
                            sends.push(native_token::custody_transfer(me, seed, to, draw));
                        }
                    }
                    sends.push(
                        SyncMessage::new(
                            Actor::new(me, program),
                            &token_core::Message::Transfer { to, amount },
                        )
                        .with_pda_seeds([seed]),
                    );
                }
            }
            (next, e)
        }
        Op::Cancel => {
            let (next, refund, e) =
                core::cancel(&state, id(&caller), t).unwrap_or_else(|e| fail(&e));
            if refund > 0 {
                sends.push(pay(me, seed, s.asset, creator, refund));
            }
            (next, e)
        }
        Op::MakeIrrevocable => {
            core::make_irrevocable(&state, id(&caller), t).unwrap_or_else(|e| fail(&e))
        }
        Op::SignalMilestone { index } => {
            let (next, _amount, e) = core::signal_milestone(&state, id(&caller), index as usize, t)
                .unwrap_or_else(|e| fail(&e));
            (next, e)
        }
        Op::TransferBeneficiary { new_beneficiary } => {
            core::transfer_beneficiary(&state, id(&caller), id(&new_beneficiary), t)
                .unwrap_or_else(|e| fail(&e))
        }
    };
    // Once nothing more can ever be claimed, the unused float goes back to the creator.
    let claimable_ever = next
        .cancelled
        .as_ref()
        .map_or(next.total, |c| c.vested_at_cancel);
    let done = next.claimed >= claimable_ever;
    if done && s.float > 0 {
        sends.push(native_token::custody_transfer(me, seed, creator, s.float));
        s.float = 0;
    }
    s.schedule = Some(next);
    let mut r = save(&s).event(event(me, e));
    for m in sends {
        r = r.send(m);
    }
    r
}

fn pay(me: AccountId, seed: PdaSeed, asset: Asset, to: AccountId, amount: u128) -> SyncMessage {
    match asset {
        Asset::Native => native_token::custody_transfer(me, seed, to, amount),
        Asset::Token { program, .. } => SyncMessage::new(
            Actor::new(me, program),
            &token_core::Message::Transfer { to, amount },
        )
        .with_pda_seeds([seed]),
    }
}
