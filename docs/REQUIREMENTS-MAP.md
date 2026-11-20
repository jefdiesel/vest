# Vest — RFP-017 hard requirements → evidence

Every hard requirement of [RFP-017](RFP-017.md), with the test or run that shows it and an honest
status. Three kinds of evidence:

- **unit**: a test in this repository (`cargo test`, `node --test`); counts at the end.
- **e2e**: `scripts/e2e.sh` against a LEZ v0.4.0-rc3 standalone sequencer with dev-mode receipts,
  run by `e2e/tests/chain.rs` (in-process sequencer, also CI) and on a throwaway chain; results
  in `docs/live/e2e-inprocess-20261120.txt`.
- **public**: the public rc3 demo chain (lez.nameswallet.app), program
  `1a0cd691626eed61f82296bfd9d89609453a8fa8c947994a70dd1cbaa661327d`, results in
  `docs/live/pub-smoke-program-20261120.txt` (tx hashes and blocks).

The public chain is a standalone rc3 sequencer, not "LEZ testnet 0.4": the official testnet does not
run 0.4 yet. Where the RFP says testnet, the public demo chain is the closest available evidence.

## Functionality

| # | Requirement | Status | Evidence |
|---|---|---|---|
| F1 | Escrow against cliff+linear, fully linear and milestone schedules | met | core: `cliff_linear_waits_then_unlocks_at_cliff`, `linear_vesting_is_exact_and_monotone`, milestone tests; program escrows native LEZ in `(S, native)` or a 0.4 token holding `(S, T)`; e2e: create-linear, create-cliff, create-milestone, create-token; public: create-linear, create-cliff, create-milestone |
| F2 | Beneficiary named at creation; claim to public or private; creator does not learn the private account | met (private: dev-mode receipts in the new program; a real proof ran with the spike program) | e2e: claim-public, claim-private-native into a fresh private account, claim-token-private into a fresh account with no holding and no LEZ; the destination is a one-time alias `P.blinded(o)`, the opening stays in the claimant's proof. Real proofs (725 s, 458 s) of private claims ran on the public chain with the spike program of the same flow; a real-proof private claim of this program on the public chain is prepared and waits for funds |
| F3 | Cancelable by default; one-way irrevocable; unvested refunded; vested stays claimable | met | core: `create_defaults_and_rejects_bad_params`, `cancel_refunds_unvested_and_keeps_vested_claimable`, `cancel_twice_and_irrevocable_are_rejected`; program `events.rs`; e2e: cancel, cancel-again (refused), claim-after-cancel, make-irrevocable, cancel-irrevocable (refused); public: cancel (refund 254,378), claim-after-cancel, make-irrevocable, cancel-irrevocable refused |
| F4 | Transferability fixed at creation | met | core: `transfer_is_frozen_at_creation_and_authorised`; e2e and public: transfer, then the old beneficiary refused (e2e) and the new one claims |
| F5 | Batch creation up to the documented maximum | met | one `Create` transaction carries many schedules; `MAX_BATCH = 140`, measured on rc3 (`docs/COSTS.md`): the 10M-cycle cap binds at 146 native / 74 token; sent for real: 140 native (block 776) and 72 token (block 777) schedules in one transaction each on a throwaway rc3 chain; e2e: create-batch |
| F6 | An event for each state transition, schema documented | met for emission; read-back limited by rc3 | program `tests/events.rs` (one event per transition with selector and payload, all six kinds); schema in `docs/EVENTS.md` and `idl/vesting.json`. An rc3 standalone sequencer stores no program events (it drops the events of the blocks it produces); `e2e/tests/chain.rs` reports this. Reading them back needs a Bedrock-backed indexer (`getEvents`) |

## Usability

| # | Requirement | Status | Evidence |
|---|---|---|---|
| U1 | SDK for Logos modules: full lifecycle, public and private claims | met | `sdk/src/client.rs` (`create`, `create_batch`, `claimable`, `pre_claim_summary`, `claim_public`, `claim_private`, `cancel`, `make_irrevocable`, `signal_milestone`, `transfer_beneficiary`); `LezBackend` (feature `lez`) on chain; unit: `sdk/tests/client.rs`, `sdk/tests/reliability.rs`; e2e drives every operation through it |
| U2 | Mini-app: recipient and creator views; local build; loadable in Basecamp | partly met | `miniapp/` QML views and the Rust module over the SDK (`lez` feature builds); `node --test miniapp/logic.test.mjs` 63 pass, module tests 10 pass. **Not done:** the Basecamp/nix packaging and a live load in Basecamp are untested; the module's name map is still a mock |
| U3 | CLI: create, claim, cancel, signal milestone, query claimable | met | `vesting` commands; unit: `cli/tests/lifecycle.rs`; e2e and public runs use only the CLI |
| U4 | Pre-claim summary with fee; private path checks gas | partly met | CLI and mini-app show claimable and next unlock; the CLI's `--dry-run` prints the wallet's real fee. The SDK's `PreClaimSummary.estimated_fee` is still a placeholder (0). For native LEZ a fresh private account pays its fee from the claim; for tokens, from the creator's fee float. The mini-app's gas check runs on its mock data |
| U5 | Privacy disclosure before each private claim | met (CLI); mini-app on mock data | `cli/src/render.rs::privacy_disclosure`, printed before every private claim with a y/N prompt; unit: `private_claim_needs_confirmation_and_prints_disclosure`; mini-app logic tests |
| U6 | IDL with SPEL | met (hand-written v2) | `idl/vesting.json` (SPEL IDL v2: seven messages, state, six events); `idl-check` byte-checks it against the program's encoding (4 tests). Not generated by `spel generate-idl`: `docs/SPEL-GAP.md` |
| U7 | Clear failed-claim errors, with the next unlock time | met | core error codes and `WaitingFor`; the CLI prints e.g. "Nothing is claimable yet: the cliff is at 1795194435000 ms ... (code 9)" and a status line; e2e: claim-before-cliff, claim-before-signal; public: claim-before-cliff |

## Reliability

| # | Requirement | Status | Evidence |
|---|---|---|---|
| R1 | A failed claim consumes nothing; retry works | met | unit R1 (`sdk/tests/reliability.rs`); on chain a refused claim panics and the whole transaction reverts: a pre-cliff claim sent anyway was included and reverted with the escrow untouched (spike program, same mechanism) |
| R2 | Cancellation atomic | met | unit R2; program `events.rs` cancel path; e2e cancel then cancel-again refused with state unchanged |
| R3 | Concurrent claims on independent schedules do not corrupt shared state | met (no shared state) | each schedule is its own account `S`; unit R3 (two interleaved schedules) and `program/tests/derivation.rs` (no account collisions); two real transactions in one block were not specifically tested |
| R4 | Milestone signalling idempotent, deterministic error | met | unit R4 and `a_repeated_milestone_signal_is_refused`; e2e and public: signal-0-again refused (code 18) |

## Performance

| # | Requirement | Status | Evidence |
|---|---|---|---|
| P1 | A claim completes in one transaction | met | public and e2e claims are single transactions (tx hashes in `docs/live/`); a clock claim reads the clock and pays in the same transaction |
| P2 | CU cost of each operation on testnet 0.4 | met on rc3 (not testnet) | `docs/COSTS.md`: cycles, bytes and fees of create, claim (public/private, native/token), cancel, signal, transfer, make irrevocable, and the batch curve, measured on an rc3 sequencer |

## Supportability

| # | Requirement | Status | Evidence |
|---|---|---|---|
| S1 | Deployed and tested on LEZ testnet 0.4 | partly met | deployed (updatable) on the public rc3 demo chain and smoke-tested (native public operations, all pass). Token schedules, a batch and a real-proof private claim of this program on that chain are still to run. LEZ testnet 0.4 itself is not live |
| S2 | E2E against a standalone sequencer, in CI, green on the default branch | met locally; CI not yet run | `e2e/tests/chain.rs` passes locally (34+ steps); `.github/workflows/e2e.yml` runs it; the repository has not been pushed, so CI has not run |
| S3 | Every hard requirement has a test | met, with the limits stated in this map | this table |
| S4 | README with deployment, addresses, CLI and mini-app usage | met | `README.md` |

## Privacy

| # | Requirement | Status | Evidence |
|---|---|---|---|
| PR1 | SDK and mini-app support public and private claims | met | SDK `claim_public` / `claim_private` (target handed to the backend, alias built from the wallet's private account); mini-app views |
| PR2 | Disclosure before each private claim | met (CLI); mini-app on mock data | as U5 |
| PR3 | The SDK checks the target is a private account | met | `Client::claim_private` refuses a public `AccountRef` (code 1001, `claim_private_refuses_a_public_target`); the LEZ backend asks the wallet whether the target is a private account it holds and refuses otherwise |

## Soft requirements

| Requirement | Status | Evidence |
|---|---|---|
| Separate cancellation authority | met | e2e: the creator's cancel refused (code 11), the authority's succeeds, refund to the creator |
| Separate milestone authority | met | e2e: the creator's signal refused (code 12), the authority's succeeds |

## Test counts (2026-11-20)

- `cargo test --workspace --exclude vesting-e2e --exclude vesting_guest --exclude token_guest`:
  72 passed, 0 failed (vesting-core 26, vesting-program 7, vesting-sdk 8, vesting-cli 10,
  idl-check 4, vendored logos-sdk crates 17).
- Mini-app module: `cargo test --manifest-path miniapp/module/rust-lib/Cargo.toml` 10 passed.
- `node --test`: viewer 9, mini-app 63, reference faucet mini-app 1 — all pass.
- `e2e/tests/chain.rs` (in-process rc3 sequencer, dev-mode): passes, every step of `scripts/e2e.sh`.

## Known limits

- The token fee float can be drawn as plain LEZ by a beneficiary who claims "privately" to a public
  account; the creator's loss is capped by the deposit (`docs/IDL.md`).
- Events cannot be read back from a standalone rc3 sequencer (`docs/EVENTS.md`).
- Schedule ids are local to a CLI/SDK map; another machine imports the schedule account.
