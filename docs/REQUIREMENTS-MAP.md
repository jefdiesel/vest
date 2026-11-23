# Vest — RFP-017 hard requirements → evidence

Every hard requirement of [RFP-017](RFP-017.md), with the test or run that shows it and an honest
status. Three kinds of evidence:

- **unit**: a test in this repository (`cargo test`, `node --test`); counts at the end.
- **e2e**: `scripts/e2e.sh` against a LEZ v0.4.0-rc3 standalone sequencer with dev-mode receipts,
  run by `e2e/tests/chain.rs` (in-process sequencer, also CI) and on a throwaway chain; results
  in `docs/live/e2e-inprocess-20261120.txt` (40 steps) and `docs/live/e2e-inprocess-20261120-fee.txt` (43 steps, with the fee summaries).
- **public**: our own public rc3 test chain (`lez.nameswallet.app`, not a Logos network), program
  `1a0cd691626eed61f82296bfd9d89609453a8fa8c947994a70dd1cbaa661327d`, results in
  `docs/live/pub-smoke-program-20261120.txt` (native public lifecycle) and
  `docs/live/pub-full-20261120.txt` (token schedules, real-proof private token claim, batch of 8, cancel and float return).

The public chain is a standalone rc3 sequencer that we operate, not "LEZ testnet 0.4": the official testnet does not
run 0.4 yet. Where the RFP says testnet, those rows remain open until Logos launches testnet 0.4.

## Functionality

| # | Requirement | Status | Evidence |
|---|---|---|---|
| F1 | Escrow against cliff+linear, fully linear and milestone schedules | met | core: `cliff_linear_waits_then_unlocks_at_cliff`, `linear_vesting_is_exact_and_monotone`, milestone tests; program escrows native LEZ in `(S, native)` or a 0.4 token holding `(S, T)`; e2e: create-linear, create-cliff, create-milestone, create-token; public: create-linear, create-cliff, create-milestone. Names Wallet 0.18 path: a create built by the module's `create_call`, sent through the Names module's send path with the names command-line tool (sent with the names command-line tool), tx `3a3e784bec51bfcb0bd12f9f4f83e407a0f6ae99da5a14811ead997b51f32ba8`, block 43382, schedule `Hu2UvypkMBX9G5RfmXsJMKYUKvBdE8LLyZhFUUFUgcps` with state and a 1000-unit escrow read back; a second create `be52f9c35a79e027ebb5c565a5726066019ff74915ee311a39f80caff8121959`, block 43390 |
| F2 | Beneficiary named at creation; claim to public or private; creator does not learn the private account | met | e2e: claim-public, claim-private-native into a fresh private account, claim-token-private into a fresh account with no holding and no LEZ (dev-mode receipts); the destination is a one-time alias `P.blinded(o)`, the opening stays in the claimant's proof. An earlier spike program with the same flow ran two private native claims with real proofs (`2d950584…` block 37996, `19eb619b…` block 38267). On this program, a private token claim into a fresh account (no holding, no LEZ) ran with a real proof on our public rc3 chain: tx `c5d78291…`, block 39066, ~10 minutes on a Mac mini with 4 threads (600 s between the previous step in block 39026 and the claim, proving and inclusion together; `docs/live/pub-full-20261120.txt`). This program's private native claim has run with dev-mode receipts only so far |
| F3 | Cancelable by default; one-way irrevocable; unvested refunded; vested stays claimable | met | core: `create_defaults_and_rejects_bad_params`, `cancel_refunds_unvested_and_keeps_vested_claimable`, `cancel_twice_and_irrevocable_are_rejected`; program `events.rs`; e2e: cancel, cancel-again (refused), claim-after-cancel, make-irrevocable, cancel-irrevocable (refused); public: cancel (refund 254,378), claim-after-cancel, make-irrevocable, cancel-irrevocable refused by the CLI before sending. Names Wallet 0.18 path: a cancel built by `cancel_call`, sent through the Names module's send path with the names command-line tool, tx `8db6c07259a875b5e2dcf9992997ef0257191ca25cc608c5e91fc04fa66e988e`, block 43393 — refund to the creator; the 4 vested units stayed in escrow |
| F4 | Transferability fixed at creation | met | core: `transfer_is_frozen_at_creation_and_authorised`; e2e and public: transfer, then the old beneficiary refused (e2e) and the new one claims |
| F5 | Batch creation up to the documented maximum | met | one `Create` transaction carries many schedules; `MAX_BATCH = 140`, measured on rc3 (`docs/COSTS.md`): the 10M-cycle cap binds at 146 native / 74 token; sent for real on a throwaway rc3 chain: 140 native (block 776) and 72 token (block 777) schedules in one transaction each (both require a raised gas limit); on our public rc3 chain: 8 native schedules (`8083542a…`, block 39067); e2e: create-batch |
| F6 | An event for each state transition, schema documented | met for emission; read-back limited by rc3 | program `tests/events.rs` (one event per transition with selector and payload, all six kinds); schema in `docs/EVENTS.md` and `idl/vesting.json`. An rc3 standalone sequencer stores no program events (it drops the events of the blocks it produces); `e2e/tests/chain.rs` reports this. Reading them back needs a Bedrock-backed indexer (`getEvents`) |

## Usability

| # | Requirement | Status | Evidence |
|---|---|---|---|
| U1 | SDK for Logos modules: full lifecycle, public and private claims | met | `sdk/src/client.rs` (`create`, `create_batch`, `claimable`, `pre_claim_summary`, `claim_public`, `claim_private`, `cancel`, `make_irrevocable`, `signal_milestone`, `transfer_beneficiary`); `LezBackend` (feature `lez`) carried the e2e and public CLI runs (the CLI calls `core` and `LezBackend` directly, not `Client`; `Client` is covered by `sdk/tests/client.rs` and `sdk/tests/reliability.rs`). A Basecamp-app SDK sits next to it: `sdk/qml/VestSDK.mjs`, with `sdk/qml/README.md` and 17 tests in `sdk/qml/VestSDK.test.mjs` (not yet in CI; refuses private claims until wallet support lands). It signs through any wallet that implements the Names Wallet SDK |
| U2 | Mini-app: recipient and creator views; local build; loadable in Basecamp | partly | two Basecamp packages built with `logos-module-builder` (`miniapp/README.md`): the `vesting` core module (`mkLogosModule`, Rust over the SDK's `LezBackend`, settings from a network file or the built-in profile, chain-id check, real LNS lookups, the wallet's accounts) and the `vesting_ui` app (`mkLogosQmlModule`, QML calling the module). Unit: `vesting-module-core` tests, `node --test miniapp/logic.test.mjs` 212 tests; offscreen renders of `Main.qml` against a stand-in module, 31 scenarios, no QML error (`miniapp/tools/render.py`, not in CI); packages built with nix (`lgx-portable`, darwin-arm64; latest built on the Mini are `vesting_ui` 0.6.3 / `vesting` 0.5.3). Live on a test Basecamp (2026-11-21): `vesting` 0.4.0 / `vesting_ui` 0.5.8 installed and ran a Names-mode Create (`5f1fdfa2…`, block 43932). **Open (milestone 4):** downloadable release assets (Linux and macOS), git/catalog install in Basecamp, token schedules, batch creation, and private claims |
| U3 | CLI: create, claim, cancel, signal milestone, query claimable | met | `vesting` commands; unit: `cli/tests/lifecycle.rs`; e2e and public runs use only the CLI |
| U4 | Pre-claim summary with fee; private path checks gas | partly | `Client::pre_claim_summary` simulates the very claim (`Backend::estimate_fee`; `LezBackend`: the wallet's dry run, nothing proven or sent) and returns the fee at the head base fees, the reserve, who pays (the caller's public account; for a private claim the private destination, from the native LEZ it claims or a token schedule's per-claim float) and `fee_covered`. The CLI (`vesting summary`, and before every private claim, which is refused when the destination could not pay) uses it. Unit: `the_summary_carries_the_simulated_fee_and_sends_nothing`, `a_private_summary_simulates_the_private_claim_and_checks_the_payer`; e2e: summary-public, summary-private-native, summary-token-private, all PASS (fees 341,088 / 5,406,392 / 5,637,880; `docs/live/e2e-inprocess-20261120-fee.txt`). **Open (milestone 4):** the Basecamp mini-app shows the claim amount and defers the fee figure to Names Wallet's approval prompt; it does not yet call `pre_claim_summary` or run a private gas check |
| U5 | Privacy disclosure before each private claim | not met (CLI has it; mini-app does not yet) | `cli/src/render.rs::privacy_disclosure`, printed before every CLI private claim with a y/N prompt; unit: `private_claim_needs_confirmation_and_prints_disclosure`. **Open (milestone 4):** the Basecamp mini-app does not yet support private claims and has no private-claim disclosure UI |
| U6 | IDL with SPEL | partly (hand-written v2) | `idl/vesting.json` (hand-written in the v2 format of SPEL's unmerged `refactor/lez-v0.4.x` branch: seven messages, state, six events); `idl-check` byte-checks it against the program's encoding (4 tests). Not yet generated by `spel generate-idl`: `docs/SPEL-GAP.md` |
| U7 | Clear failed-claim errors, with the next unlock time | met | core error codes and `WaitingFor`; the CLI prints e.g. "Nothing is claimable yet: the cliff is at 1795194435000 ms ... (code 9)" and a status line; e2e: claim-before-cliff, claim-before-signal; public: claim-before-cliff (client-side pre-check) |

## Reliability

| # | Requirement | Status | Evidence |
|---|---|---|---|
| R1 | A failed claim consumes nothing; retry works | met | unit R1 (`sdk/tests/reliability.rs`), program test `only_the_beneficiary_claims`; on chain a refused claim panics and the whole transaction reverts (the transaction still pays its fee; a pre-cliff claim sent anyway reverted with the escrow untouched on the spike program, same mechanism) |
| R2 | Cancellation atomic | met | unit R2; program `events.rs` cancel path; e2e cancel then cancel-again refused with state unchanged |
| R3 | Concurrent claims on independent schedules do not corrupt shared state | met (no shared state) | each schedule is its own account `S`; unit R3 (two interleaved schedules) and `program/tests/derivation.rs` (no account collisions); two real transactions in one block were not specifically tested |
| R4 | Milestone signalling idempotent, deterministic error | met | unit R4 and `a_repeated_milestone_signal_is_refused`; program test expects `vesting error 18`; e2e and public: signal-0-again refused (code 18, client-side pre-check) |

## Performance

| # | Requirement | Status | Evidence |
|---|---|---|---|
| P1 | A claim completes in one transaction | met | public and e2e claims are single transactions (tx hashes in `docs/live/`); a clock claim reads the clock and pays in the same transaction (a public token claim requires the beneficiary's token holding to be initialised beforehand) |
| P2 | CU cost of each operation on testnet 0.4 | partly (standalone rc3, not testnet 0.4) | `docs/COSTS.md`: cycles, bytes and fees of create, claim (public/private, native/token), cancel, signal, transfer, make irrevocable, and the batch curve, measured on a standalone rc3 sequencer. To be re-measured on Logos testnet 0.4 (milestone 2) |

## Supportability

| # | Requirement | Status | Evidence |
|---|---|---|---|
| S1 | Deployed and tested on LEZ testnet 0.4 | not met | deployed (updatable) and run on our own public rc3 test chain (`docs/live/pub-smoke-program-20261120.txt`, `docs/live/pub-full-20261120.txt`). Logos testnet 0.4 is not live yet (milestone 2) |
| S2 | E2E against a standalone sequencer, in CI, green on the default branch | met | `.github/workflows/e2e.yml` and the unit workflow are green on `main` (`e2e/tests/chain.rs` drives `scripts/e2e.sh` on an in-process standalone rc3 sequencer with dev-mode receipts) |
| S3 | Every hard requirement has a test | partly | cargo unit/program/SDK/CLI/module/idl-check suites and 222 Node tests (viewer, mini-app, reference faucet) pass in CI. Open: tests for mini-app U4/U5 once built, adding `sdk/qml/VestSDK.test.mjs` and `miniapp/tools/render.py` to CI, R2 partial-failure and R3 same-block concurrency tests, and P2 on testnet 0.4 |
| S4 | README with deployment, addresses, CLI and mini-app usage | partly | `README.md` covers offline JSON and on-chain CLI usage plus the rc3 deployment address; mini-app steps are in `docs/USER-GUIDE.md`. Open (milestone 4): release/install assets, token program address and funding steps in `README.md`, and testnet 0.4 addresses |

## Privacy

| # | Requirement | Status | Evidence |
|---|---|---|---|
| PR1 | SDK and mini-app support public and private claims | partly | SDK (`claim_public` / `claim_private`) and CLI support both public and private claims; `LezBackend` ran both in e2e and a real-proof private token claim on our rc3 chain. **Open (milestone 4):** the Basecamp mini-app currently offers public LEZ claims only |
| PR2 | Disclosure before each private claim | not met (CLI has it; mini-app does not yet) | as U5 |
| PR3 | The SDK checks the target is a private account | met | `Client::claim_private` refuses a public `AccountRef` (code 1001, `claim_private_refuses_a_public_target`); `LezBackend` also checks with the wallet that the target is a private account it holds; `vesting-module-core` declares the kind from the wallet's own accounts (`a_private_claim_needs_a_private_account_of_the_wallet`) |

## Soft requirements

| Requirement | Status | Evidence |
|---|---|---|
| Separate cancellation authority | met | e2e (standalone chain in CI): the creator's cancel refused (code 11), the authority's succeeds, refund to the creator |
| Separate milestone authority | met | e2e (standalone chain in CI): the creator's signal refused (code 12), the authority's succeeds |

## Test counts (2026-11-23)

- Unit & e2e CI workflows on `main` (`.github/workflows/`): green.
- `node --test web/viewer/test/logic.test.mjs miniapp/logic.test.mjs reference/faucet-miniapp/logic.test.mjs`: **222 passed, 0 failed** (viewer 9, mini-app 212, reference faucet mini-app 1).
- `sdk/qml/VestSDK.test.mjs`: 17 tests (local; not yet in CI).
- `e2e/tests/chain.rs` (in-process rc3 sequencer, dev-mode): passes in CI; `scripts/e2e.sh` 43 steps PASS (`docs/live/e2e-inprocess-20261120-fee.txt`).

## Known limits

- **Upgrade key:** the test deployment on our rc3 chain is updatable; whoever holds that key controls the program and its escrows. Whether the reviewed program is made immutable or governed is agreed with Logos in milestone 3.
- **Token programs:** a schedule records the token program its creator chose, and the clients do not yet restrict it to the canonical one; use token schedules only with the canonical program until milestone 3 closes this.
- **Proof system:** LEZ rc3 pins RISC Zero 3.0.5, inside the range of critical advisory GHSA-g6qg-9j8p-7j87; private transactions on rc3 should not carry value until LEZ updates.
- **Mini-app:** private claims, fee/gas check, privacy disclosure, token schedules and batch creation wait on wallet signing support (milestone 4).
- The token fee float of private token claims can be drawn as plain LEZ by a beneficiary who claims "privately" to a public account; the creator's loss is capped by the deposit (`docs/IDL.md`).
- Events cannot be read back from a standalone rc3 sequencer (`docs/EVENTS.md`).
- Schedule ids are local to a CLI/SDK map; another machine imports the schedule account.
