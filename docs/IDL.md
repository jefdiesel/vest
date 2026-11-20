# Vest — vesting program IDL

The interface of the vesting program for LEZ v0.4.0-rc3 (`program/src/lib.rs`, crate
`vesting-program`). Every rule is `vesting-core`'s (`core/`); the program only routes messages,
reads the clock, keeps state and moves tokens. The SDK's LEZ backend (`sdk/src/lez.rs`) and the
CLI use these types directly.

The machine-readable IDL is **`idl/vesting.json`**, in SPEL's IDL v2 (the LEZ 0.4 actor-model
format: messages with Borsh tags, state, events with selectors, types). It covers all seven
messages, the schedule state and the six events, and `idl-check` verifies it byte for byte
against the program's own Borsh encoding (`cargo test -p idl-check`). What SPEL does and does not
support for this program is in `docs/SPEL-GAP.md`.

## Deployments

| Chain | Program id (hex) | Image id | Updatable |
|---|---|---|---|
| Public rc3 demo chain (lez.nameswallet.app), 2026-11-20 | `1a0cd691626eed61f82296bfd9d89609453a8fa8c947994a70dd1cbaa661327d` | `2feaa8d38d50ef1e5ba5269a7bff6f9983928b4fc627c4d0780c2342d6008ab6` | yes (loader header flag 0) |

Token schedules use the 0.4 fungible token program of logos-blockchain/lez-programs (`cad71fc`,
`programs/token`), deployed wherever the token lives.

## Message model (LEZ 0.4 actors)

A program's state lives in *actors*: `(account, program)` pairs. Every operation is a **root call
to the caller's own actor `(X, V)`** of this program `V`; it must be authorized (X signed the
transaction, or a program holding X's PDA seed called it). That actor forwards an internal message
to the schedule actor `(S, V)`, which identifies the caller by `input.from` (set by the executor,
not by the caller). Operations other than an "as of" claim then read the clock: `(S, V)` stores the
request (`pending`), sends `ReadTimestamp` to the clock, and finishes in the clock's reply, which it
accepts only from `clock_actor()`. All of this is one transaction; any error reverts all of it.

## Accounts

| Account | Actors used | Derivation |
|---|---|---|
| Schedule `S` | `(S, V)`: [`ScheduleState`](#state); `(S, native)`: native escrow, or the token fee float; `(S, T)`: token escrow holding | `S = AccountId::for_public_pda(V, SHA256("/RFP017/vesting/schedule/v1" ‖ creator ‖ nonce))`; `nonce` is 32 bytes chosen by the creator (the SDK uses a random 8-byte map tag + the local id) |
| Caller `X` (creator, beneficiary, an authority) | `(X, V)` (no state), `(X, native)` or `(X, T)` when it pays | its own account; signs |
| Destination | `(to, native)` or `(to, T)` | a public account, or a one-time alias `P.blinded(o)` of a private account `P`, whose opening `o` stays in the claimant's proof |
| Clock | `(CLOCK_PROGRAM_ACCOUNT_ID, clock)` | LEZ builtin, `/LEZ/ClockProgramAccount/0000000` |
| Token definition | `(D, T)` | needed when a token holding is initialized |

Only deliveries from `(S, V)` carrying `S`'s seed are authorized on `S`: nobody holds a key for it.

## Types

| Name | Borsh | Meaning |
|---|---|---|
| `AccountId` | `[u8; 32]` | LEZ account id |
| `Amount` | `u128` | token amount |
| `Timestamp` | `u64` | milliseconds (the LEZ clock's unit) |
| `Asset` | enum: `0 Native`, `1 Token { program: AccountId, definition: AccountId }` | what the schedule escrows |
| `Sponsor` | `{ deposit: u128, per_claim: u128 }` | token schedules: native LEZ the creator deposits, and what each private claim draws |
| `ScheduleKind` | core enum: `0 CliffLinear { start, cliff, end }`, `1 Linear { start, end }`, `2 Milestone { tranches: Vec<u128> }` | |
| `NewSchedule` | `{ nonce: [u8;32], beneficiary, asset: Asset, total: u128, kind: ScheduleKind, cancelable: Option<bool>, transferable: Option<bool>, cancel_authority: Option<AccountId>, milestone_authority: Option<AccountId>, sponsor: Option<Sponsor> }` | `None` = the RFP defaults (cancelable, not transferable, authorities = creator) |

## Instructions

The root message is the borsh enum `Instruction`, sent to the caller's own actor `(X, V)`.

| Tag | Instruction | Caller `X` | Accounts besides `(X, V)` | Clock |
|---|---|---|---|---|
| 0 | `Create { schedules: Vec<NewSchedule> }` | creator (pays) | per schedule `(S, V)`, `(S, native|T)` (+ `(S, native)` with a sponsor); `(X, native|T)`; `(D, T)` for tokens | yes |
| 1 | `Claim { schedule, to, to_private: bool, granularity: u64 }` | beneficiary | `(S, V)`, `(S, escrow)`, destination | yes, rounded down to `granularity` ms (≥ 1) |
| 2 | `ClaimAsOf { schedule, to, to_private: bool, as_of: u64 }` | beneficiary | as `Claim` | no: pays what vested by `as_of`; the transaction is valid only in blocks with timestamp ≥ `as_of` |
| 3 | `Cancel { schedule }` | cancel authority | `(S, V)`, `(S, escrow)`, creator's escrow actor (refund) | yes, 1 ms (fixed) |
| 4 | `MakeIrrevocable { schedule }` | cancel authority or creator | `(S, V)` | yes |
| 5 | `SignalMilestone { schedule, index: u32 }` | milestone authority | `(S, V)` | yes |
| 6 | `TransferBeneficiary { schedule, new_beneficiary }` | beneficiary (transferable schedules) | `(S, V)` | yes |

Notes:
- **Create** sends, per schedule, the seeded `Create` to `(S, V)` (which admits the new account `S`
  and stores the schedule) and then the escrow transfer from the creator: a native `Transfer`, or
  `InitializeHolding` on `(S, T)` (seeded) + token `Transfer`, and the sponsor deposit. At most
  `MAX_BATCH` = 140 schedules (measured limit, see `docs/COSTS.md`).
- **Claims to a private account** (`to_private = true`) go to a one-time alias. For a token, the
  program first sends `InitializeHolding` to `(to, T)` (a no-op when the holding exists; the
  transaction authorizes the private account), then draws `min(per_claim, float)` native LEZ from
  `(S, native)` to the same alias, so a fresh private account can pay its in-proof fee.
- When nothing more can ever be claimed (claimed = total, or = vested at cancellation), the unused
  float goes back to the creator in the same transaction.
- **Cancel** refunds `total − vested_at_cancel` to the creator; what vested stays claimable.

## State

`(S, V)` holds `ScheduleState`:

`version u8 (= 1) · nonce [32] · asset Asset · schedule Option<vesting_core::Schedule> ·
float u128 · per_claim u128 · pending Option<Pending>`

`schedule` is `vesting-core`'s `Schedule` verbatim (layout in `docs/EVENTS.md`, "State layout"):
times, claimed, authorities, milestone bitmap and the cancel record (`at`, `vested_at_cancel`)
survive a cancel. It is `None` only inside the creating transaction, between the `Create` and the
clock's reply; `pending` is set and cleared within one transaction.

## Errors

A refused operation panics, which reverts the whole transaction; the wallet's simulation shows the
text before anything is sent. Core errors read `vesting error <code>: <message>` with the stable
codes of `core/src/error.rs`:

| Code | Name | Plain message (short form) |
| --- | --- | --- |
| 1 | `ZeroTotal` | Total must be greater than zero. |
| 2 | `InvalidTimes` | Check start, cliff and end. |
| 3 | `EmptyTranches` | A milestone schedule needs at least one tranche. |
| 4 | `TooManyTranches` | Reduce the tranche count. |
| 5 | `ZeroTranche` | Give the tranche a positive amount. |
| 6 | `TrancheSumMismatch` | Tranche amounts must sum to the total. |
| 7 | `BatchTooLarge` | Split the batch (more than `MAX_BATCH`). |
| 8 | `BatchItemFailed` | See the indexed item error (client side). |
| 9 | `NothingClaimable` | See the `WaitingFor` reason and the next unlock time. |
| 10 | `NotBeneficiary` | Only the current beneficiary may claim or transfer. |
| 11 | `NotCancelAuthority` | Only the cancellation authority may cancel or make irrevocable. |
| 12 | `NotMilestoneAuthority` | Only the milestone authority may signal. |
| 13 | `NotMilestoneSchedule` | Milestones need a milestone schedule. |
| 14 | `NotCancelable` | The schedule is irrevocable. |
| 15 | `AlreadyCancelled` | The schedule was already cancelled. |
| 16 | `AlreadyIrrevocable` | The change is one-way. |
| 17 | `CancelledSchedule` | Operation not allowed after cancellation. |
| 18 | `MilestoneAlreadySignalled` | A milestone cannot unlock twice. |
| 19 | `MilestoneOutOfRange` | Check the tranche count. |
| 20 | `NotTransferable` | Transferability was fixed at creation. |
| 21 | `SameBeneficiary` | Choose a different beneficiary. |
| 22 | `ArithmeticOverflow` | Check the amounts. |
| 23 | `VersionMismatch` | Unsupported state version. |
| 1000 | SDK `Backend` error | Backend/chain failure (client only). |
| 1001 | SDK `TargetNotPrivate` | Private claim target is not a private account of the wallet (client only). |

Program-level refusals without a core code: `vesting: <X> did not authorize this call` (an
unsigned root call, e.g. a forged clock reply sent to a schedule), `vesting: schedule <S> exists`,
`vesting: address does not match creator and nonce`, `vesting: a clock read is already pending`.

## Events

One `ProgramEvent` per transition, emitted by `(S, V)`: `selector` = the first 8 bytes of
`SHA256("vesting::<Variant>")`, `data` = borsh `VestingEvent { schedule: AccountId, event:
vesting_core::Event }`. Layouts and the storage caveat are in `docs/EVENTS.md`.

## Read-only queries

There are no view calls: clients read `(S, V)` (`getAccountView` / `getAccount`) and evaluate
`claimable` and `next_unlock` with `vesting-core` at the chain clock's time, as the SDK does
(`Client::claimable`, `Client::pre_claim_summary`).

## Known limit: the token fee float

`to_private` is the claimant's statement: the program cannot tell a one-time alias of a private
account from a public account the beneficiary controls (both are 32-byte ids, and the holding
initialization succeeds for any account the transaction authorizes). So a beneficiary can send a
"private" token claim to a public account they sign for and receive the `per_claim` draw as plain
LEZ, and can repeat it with small claims. The creator's loss is capped by the sponsor `deposit`,
which is meant for that beneficiary's fees anyway; size it as (expected private claims × fee). The
unused rest returns to the creator when the schedule is fully claimed. A sound check needs the
runtime to tell a program that a destination is private, which LEZ rc3 does not.
