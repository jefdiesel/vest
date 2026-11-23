# Vest

Privacy-preserving token vesting for the Logos Execution Zone (LEZ 0.4), built for Logos
[RFP-017](docs/RFP-017.md). A creator locks native LEZ or a 0.4 token for a beneficiary on a
cliff + linear, fully linear or milestone schedule; the beneficiary claims what has vested to a
public account or to a private account the creator never learns.

Licence: MIT OR Apache-2.0.

## What is here

| Part | What it does |
| --- | --- |
| `core/` (`vesting-core`) | Every vesting rule: schedule maths, state, all transitions as pure functions, stable error codes, borsh events. No LEZ dependency. |
| `program/` (`vesting-program`) | The LEZ v0.4.0-rc3 program. It routes messages, reads the clock, stores the core `Schedule` and moves native LEZ or 0.4 tokens from a per-schedule escrow; one event per transition. |
| `methods/` | Guest builds (risc0): the program, and the 0.4 token program (logos-blockchain/lez-programs `cad71fc`) for tests and deployments. |
| `sdk/` (`vesting-sdk`) | `Client` with the full lifecycle and the `Backend` trait, including the pre-claim summary with the claim's simulated fee; feature `lez` adds `LezBackend`, which talks to the program through a LEZ wallet (public and private claims, dry runs, deploy, token helpers); feature `names` the LNS resolver. |
| `cli/` (`vesting-cli`) | The `vesting` binary: a JSON-file backend for trying the rules offline, the chain with `--network`/`--wallet` (feature `lez`); LNS names accepted for every account. |
| `idl/`, `idl-check/` | The SPEL v2 IDL of the program, checked byte for byte against the program's own encoding. |
| `miniapp/` | Basecamp packages: the `vesting` core module (`miniapp/module`, Rust over the SDK on the chain) and the `vesting_ui` app (QML). |
| `web/viewer/` | A static, read-only page that reads a schedule from the chain and shows vested, claimable, locked and the next unlock. |
| `e2e/`, `scripts/` | End to end on an in-process standalone rc3 sequencer driving `scripts/e2e.sh` (CI: `.github/workflows/e2e.yml`); measurement and public-chain smoke scripts. |
| `vendor/logos-sdk/` | The `lns`, `rpc` and `accounts` crates of logos-sdk, vendored for LNS name resolution. |

Documentation: [program IDL](docs/IDL.md) · [events](docs/EVENTS.md) · [costs and batch limit](docs/COSTS.md) ·
[reliability tests](docs/RELIABILITY-TESTS.md) · [SPEL notes](docs/SPEL-GAP.md) ·
[the user guide](docs/USER-GUIDE.md) · [Basecamp click-through test](docs/BASECAMP-TEST.md) · [RFP-017 requirements → evidence](docs/REQUIREMENTS-MAP.md).

## Build and test

As CI's unit job runs them (no risc0 toolchain needed):

```sh
cargo fmt --all -- --check
cargo test -p vesting-core -p vesting-sdk -p vesting-cli -p vesting-program -p idl-check \
  -p vesting-module-core -p logos-lns -p logos-rpc -p logos-accounts
node --test web/viewer/test/logic.test.mjs miniapp/logic.test.mjs reference/faucet-miniapp/logic.test.mjs
```

The program's guests and the LEZ backend need the risc0 toolchain (`rzup install rust`,
`rzup install cargo-risczero 3.0.5`):

```sh
cargo build --release -p vesting-cli -p vesting-methods --features vesting-cli/lez
# guests: target/riscv-guest/vesting-methods/{vesting_guest,token_guest}/riscv32im-risc0-zkvm-elf/release/*.bin
```

End to end, as CI runs it (needs an rc3 `sequencer_service` built with `--features standalone`
and its `networks/local.json`; dev-mode proofs):

```sh
SEQ_BIN=... LOCAL_JSON=... VESTING_BIN=target/release/vesting VESTING_ELF=... TOKEN_ELF=... \
  RISC0_DEV_MODE=1 cargo test --release -p vesting-e2e -- --ignored --nocapture
```

## Trying the rules offline (JSON backend)

Without `--network`/`--wallet` the CLI keeps schedules and events in a JSON state file and moves
no tokens: it is for trying the rules. `--state <path>` picks the file
(default `vesting-state.json`) and `--now <milliseconds>` overrides the clock so you can
reproduce cliff dates and unlocks. Accounts are 32-byte addresses written as 64 hex characters
(or base58), or LNS names (below).

Below, `ALICE` is the creator/cancel authority, `BOB` the beneficiary, `TOKEN` the token
definition and `PRIV` a private (shielded) destination account:

```sh
ALICE=0101010101010101010101010101010101010101010101010101010101010101
BOB=0202020202020202020202020202020202020202020202020202020202020202
TOKEN=0303030303030303030303030303030303030303030303030303030303030303
PRIV=0404040404040404040404040404040404040404040404040404040404040404
STATE=./vesting-state.json
V="vesting --state $STATE"     # or: cargo run -p vesting-cli --bin vesting -- ...
```

Times are Unix milliseconds. The CLI prints amounts as decimal numbers and dates in UTC.

### 1. Create a schedule

Three schedule kinds are supported.

```sh
# Fully linear: 1,000,000 tokens from 2026-01-01 to 2027-01-01.
$V --now 1767225600000 create \
  --creator $ALICE --beneficiary $BOB --token $TOKEN \
  --total 1000000 --kind linear \
  --start 1767225600000 --end 1798761600000

# Cliff + linear: 1-year cliff, 3-year linear release.
$V --now 1767225600000 create \
  --creator $ALICE --beneficiary $BOB --token $TOKEN \
  --total 1000000 --kind cliff-linear \
  --start 1767225600000 --cliff 1798761600000 --end 1893456000000

# Milestone: two tranches of 25% and 75%.
$V create \
  --creator $ALICE --beneficiary $BOB --token $TOKEN \
  --total 1000000 --kind milestone \
  --tranche 250000 --tranche 750000
```

Options: `--cancelable true|false` (default `true`), `--transferable true|false` (default
`false`), `--cancel-authority <account>`, `--milestone-authority <account>`. A cancelable
schedule can later be made irrevocable, but never the reverse.

Batch creation takes a JSON file (amounts are strings):

```json
[
  {"creator":"<64 hex>","beneficiary":"<64 hex>","token":"<64 hex>",
   "total":"1000","kind":{"type":"linear","start":0,"end":1000}},
  {"creator":"<64 hex>","beneficiary":"<64 hex>","token":"<64 hex>",
   "total":"100","kind":{"type":"milestone","tranches":["40","60"]}}
]
```

```sh
$V create-batch batch.json
```

### 2. Inspect a schedule

Schedules are numbered from 0 in the order they are created.

```sh
$V --now 1775000000000 show 0
$V --now 1775000000000 claimable 0
$V list --creator $ALICE
$V list --beneficiary $BOB
```

`claimable` prints the amount available now and the next unlock, for example
`next unlock in 3 days (2027-01-01 00:00 UTC)`. A milestone schedule instead says
`next unlock: awaiting a milestone signal`.

### 3. Claim

```sh
# Public claim: everything currently vested goes to a public account.
$V --now 1775000000000 claim 0

# Private claim: prints the RFP privacy disclosure and asks for confirmation.
$V --now 1775000000000 claim 0 --to-private $PRIV
$V --now 1775000000000 claim 0 --to-private $PRIV --yes   # non-interactive
```

The disclosure states what is visible on-chain (claim amount, beneficiary address, vesting
schedule address) and what is not (the destination private account and subsequent movements).
Declining the prompt submits no claim. An empty claim returns a clear error with the reason —
for example `the cliff is at ...`, `waiting for the milestone authority`, or
`everything vested has already been claimed`.

Mutating commands default to the natural caller (beneficiary for `claim` and `transfer`, cancel
authority for `cancel` and `make-irrevocable`, milestone authority for signalling). Override with
`--as-account <account>` to test authorisation failures.

### 4. Cancel, signal, transfer

```sh
$V --now 1775000000000 cancel 0            # refunds unvested tokens to the creator
$V signal-milestone 0 0                    # unlock tranche 0
$V --now 1775000000000 make-irrevocable 0
$V transfer 0 $BOB                         # only if --transferable true was set at creation
```

After cancellation, tokens vested before the cancel remain claimable by the beneficiary;
unvested tokens go back to the creator.

## Using the SDK from a module

```rust,ignore
use vesting_sdk::{AccountKind, AccountRef, Client};

let mut client = Client::new(my_backend); // implements vesting_sdk::Backend
let (id, _, _) = client.create(params)?;
let summary = client.pre_claim_summary(id, AccountKind::Private)?;
client.claim_private(id, beneficiary, AccountRef::private(shielded_account))?;
```

`claim_private` refuses a public target with `SdkError::TargetNotPrivate` (code 1001) before any
state change. `pre_claim_summary(id, caller, destination)` simulates the very claim the client
would send (`Backend::estimate_fee`; on LEZ the wallet's dry run, nothing proven or sent) and
returns its fee, who pays it (the caller's public account, or for a private claim the private
destination, from what the claim brings it) and `fee_covered`, whether that payer will hold
enough. A local ledger charges no fee (`fee: None`). The CLI prints it with
`vesting summary <id> [--to-private <account>]` and refuses a private claim whose destination
could not pay.

## LNS names instead of addresses

Every account argument accepts either a 64-hex address or a logos-names v5 name: `creator`,
`beneficiary`, `token`, cancel/milestone authorities, `--as-account`, `--to-private`, transfer
targets, `list` filters and batch rows (`"beneficiary": "alice"`). Names are resolved at the
moment of the action through the `lns` module of `logos-sdk` (vendored in `vendor/logos-sdk/`,
used through `vesting_sdk::names` by the CLI and the Basecamp module); the resolved address is
what goes on chain. Addresses may be 64 hex characters or base58 (`Public/…` too). No on-chain
change is needed.

```sh
$V --lns <lns-program> --lns-rpc https://lez.nameswallet.app/ create \
  --creator alice --beneficiary bob --token $TOKEN \
  --total 1000 --kind linear --start 0 --end 1000
```

Before the schedule is sent, the CLI prints `alice resolves to <address>` for every name, so a
mistyped name cannot silently pay a stranger. Unknown names, expired names and names with no
owner fail with a one-line error and nothing is sent. There is no default LNS program id: pass
`--lns` explicitly. If `--network` is set, `--lns-rpc` defaults to that URL.

`show` and `list` display a name as `alice (5T7Az…)` when the name was seen earlier in this CLI.
The mapping is kept in a local name book (`--names`, default `vesting-names.json`); it is only a
display convenience, never chain state.


## On LEZ (the `lez` feature)

```sh
cargo build --release -p vesting-cli -p vesting-methods --features vesting-cli/lez
V="target/release/vesting --network http://127.0.0.1:3290 --wallet ~/my-wallet"
$V wallet account new public                       # the LEZ wallet's own command line
$V deploy --elf target/riscv-guest/vesting-methods/vesting_guest/riscv32im-risc0-zkvm-elf/release/vesting_guest.bin --payer <hex>
P="--program <program hex> --lez-map ids.json"     # add --token-program <hex> for token schedules
$V $P create --creator <hex> --beneficiary <hex> --token 00..00 --total 1000000 --kind linear --start <ms> --end <ms>
$V $P claim 0 --yes                                # public; --to-private <private account hex> for private
$V $P cancel 0 --as-account <creator hex>
$V $P summary 0 --to-private <private account hex> # claimable, the simulated fee, the fee check
$V $P --dry-run claim 0 --yes                      # the wallet's full report: cycles, bytes, fee
```

`scripts/e2e.sh` exercises every operation this way; `scripts/pub-smoke.sh` is the public-chain
smoke. Deployments are updatable (the header key stays in the deploying wallet) unless
`--immutable` is given.

### Deployments

| Chain | Program | Image | Notes |
|---|---|---|---|
| Public rc3 demo chain (lez.nameswallet.app), 2026-11-20 | `1a0cd691626eed61f82296bfd9d89609453a8fa8c947994a70dd1cbaa661327d` | `2feaa8d38d50ef1e5ba5269a7bff6f9983928b4fc627c4d0780c2342d6008ab6` | updatable; native-LEZ smoke (`docs/live/pub-smoke-program-20261120.txt`) and token / real-proof private token claim / batch run (`docs/live/pub-full-20261120.txt`) passed |

Events: every transition emits one `ProgramEvent` (selector = first 8 bytes of
`SHA256("vesting::<Variant>")`, data = borsh `VestingEvent { schedule, event }` with the core
event); `program/tests/events.rs` checks them. An rc3 sequencer stores events only for blocks that
come back through Bedrock, so a standalone chain (including the public demo chain) serves none.
Details in [`docs/EVENTS.md`](docs/EVENTS.md).

## Mini-app and viewer

- `miniapp/`: two Basecamp packages, the `vesting` core module and the `vesting_ui` app. Vest
  signs only through the person's wallet, with the **Names Wallet SDK** (Names, or any wallet
  that implements the SDK): it opens the wallet with each request and the wallet brings the
  person back once they answer. The first screen asks what you came to do (claim, or pay out)
  and walks through five steps; the payout box looks LNS names up as you type. With Names Wallet
  0.18 or later it also approves creating, cancelling, making permanent, unlocking a milestone
  and transferring a LEZ schedule. Batch creates, token schedules and private claims aren't
  offered in the app until the wallet can approve them (the CLI does them with its own wallet).
  Build and module calls:
  `miniapp/README.md`; for people:
  `docs/USER-GUIDE.md`; install and click-through: `docs/BASECAMP-TEST.md`.
- `web/viewer/`: `cd web/viewer && python3 -m http.server 8099`, then open the page and give a
  schedule account (or a creator and nonce, or an LNS name). Defaults to the public deployment.

## Known limits

- The token fee float of private token claims can be drawn by a beneficiary as plain LEZ (the
  program cannot tell a private destination from a public one on rc3); the creator's loss is
  capped by the deposit. Details in `docs/IDL.md`.
- A standalone rc3 sequencer (including the public demo chain) stores no program events: events
  are read through Bedrock-backed indexers (`docs/EVENTS.md`).
- Schedule ids in the CLI and SDK are local; a beneficiary on another machine imports the
  schedule account (`vesting import <account>`).
- In Basecamp the module answers one call at a time, and a private claim is proven on the
  machine (minutes with a real proof): other calls wait for it.
