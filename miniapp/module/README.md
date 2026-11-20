# Real module contract for the `vesting_ui` view

The C++ in `../src/vesting_ui_backend.cpp` is a mock. To make this UI real, a Basecamp module
named `vesting_ui` must implement the slots in `../src/vesting_ui.rep`. **Each slot is exactly
one SDK call** to `Client` in `sdk/src/client.rs`; no slot should
combine, reimplement or skip an SDK method.

| `.rep` slot | SDK call | What the view uses it for |
|---|---|---|
| `list()` | `Client::list()` | All schedules visible to the wallet; fills both tabs |
| `now()` | `Client::now()` | The effective time for all display maths, shown as "Effective time" |
| `preClaimSummary(id, isPrivate)` | `Client::pre_claim_summary(id, AccountKind)` | Confirmation: claimable amount, estimated fee, next unlock |
| `create(params)` | `Client::create(CreateParams)` | Single-schedule form |
| `createBatch(params)` | `Client::create_batch(Vec<CreateParams>)` | Batch form: one submit, N schedules |
| `claimPublic(id)` | `Client::claim_public(id, caller)` | Public claim |
| `claimPrivate(id, target)` | `Client::claim_private(id, caller, AccountRef)` | Private claim; the SDK rejects a non-private target |
| `cancel(id)` | `Client::cancel(id, caller)` | Cancels a cancelable schedule |
| `makeIrrevocable(id)` | `Client::make_irrevocable(id, caller)` | One-way cancelable → non-cancelable |
| `signal(id, index)` | `Client::signal_milestone(id, caller, index)` | Signals one milestone tranche |
| `transfer(id, newBeneficiary)` | `Client::transfer_beneficiary(id, caller, newBeneficiary)` | Transfers a transferable position |

## Values the module supplies (not SDK calls)

- **Caller / beneficiary account** for `claim*`, `cancel`, `makeIrrevocable`, `signal` and
  `transfer`: the connected wallet account, obtained through the wallet provider SDK, never
  from the view. The mock uses a fixed account.
- **Private-account gas balance** for the fee check: the module/wallet supplies it; the SDK
  has no balance method. The view only calls `logic.mjs::canPayFee(gas, fee)` and shows the
  error. The mock uses fixed `0` / `500` values.
- **Estimated fee**: from `preClaimSummary().estimated_fee` (the SDK currently returns
  `ESTIMATED_FEE_PLACEHOLDER = 0`; the real program's estimate replaces it). The view never
  hardcodes a fee for the confirmation.
- **Public and private destination accounts**: from the wallet provider SDK in the real app;
  the mock lists one of each in `Main.qml`.

## Errors

Every slot returns JSON `{"ok": false, "error": "..."}` on failure. The view shows `error`
as a plain result line. SDK errors should be passed through with their actionable message
(for example `NothingClaimable` with the next unlock, `TargetNotPrivate`,
`MilestoneAlreadySignalled`), not collapsed to a generic failure.

## Out of scope for this module

Batch transaction size is bounded by the LEZ transaction limit; the `createBatch` slot should
document the maximum batch length it accepts. The view does not enforce who may cancel or
signal — that is on chain, and the UI just displays the error.

## Amounts: formatted in the module, displayed in QML

Every amount is returned twice: the exact decimal string (u128, no float) and a ready
`*_display` string with the token's decimals and symbol — for example
`{"claimable":"94138000","claimable_display":"0.094138 LEZ"}`. `vesting_init` accepts
`{"decimals":9}` (LEZ default). The module computes all of it with `vesting-core`; the QML
view only shows the `*_display` strings and keeps its decimal code solely for parsing what a
person types. Fields: `total_display`, `claimed_display`, `claimable_display`, `locked_display`
on each scheduled item; `claimable_display` and `estimated_fee_display` in the summary;
`amount_display`/`claimed_total_display` on claims, `refund_display` on cancel, and
`amount_display` on signal.

## What exists now, and what is left

**Done (builds on the laptop):** `rust-lib/` is a standalone crate exposing the eleven calls
as `extern "C" fn(const char*) -> char*` (JSON in, JSON out, free with `vesting_free`), plus
`vesting_init` for module setup. It calls `vesting-sdk::Client` directly. The default
`json-file-backend` feature stores state in the CLI's JSON/borsh format; `--no-default-features`
uses an in-memory backend. Eight cargo tests call the C ABI with JSON and check the JSON out:

```sh
cargo test --manifest-path miniapp/module/rust-lib/Cargo.toml
```

**Live LEZ backend (feature `lez`):** the crate can use `vesting_sdk::LezBackend` instead of the
JSON/memory backend. Default features keep the JSON backend for host tests; build with
`--features lez` and pass settings in `vesting_init`'s JSON: `rpc_url` (http(s) sequencer),
`wallet_dir` (rc3 wallet with `wallet_config.json` + `storage.json`), `program` (32-byte hex
account; the public deployment is `1a0cd691626eed61f82296bfd9d89609453a8fa8c947994a70dd1cbaa661327d`),
optional `clock_elf`, `lez_map` (id→schedule map, default `vesting-lez-ids.json`),
`token_program` (32-byte hex, for token schedules) and `sponsor_deposit` + `sponsor_per_claim`
(decimal strings: the fee float for private token claims). The chain supplies time;
`now`/`state_path` are ignored in this mode. The 11 SDK-mapped calls are unchanged; a private
claim goes to a one-time alias of the private account, which the wallet must hold.

**Name seam (module helpers, not SDK calls):** `vesting_names()` returns
`{"ok":true,"names":{"alice":"<account>",…}}` and `vesting_resolve_name({"name":"alice"})`
resolves one name. The module's map is still the mock seeded with alice/bob/carol; the CLI
resolves real LNS names (`cli/src/names.rs`, vendored `logos-lns`), and moving that resolver into
the module is open work. The QML shows names beside addresses and accepts a name
wherever an account is typed.

**Left to do (needs nix / the module builder, so not done here):**

1. A Qt/C++ wrapper module (`CMakeLists.txt`, `metadata.json`, `flake.nix`, and the `.rep`
   scaffolding from the builder) that declares the eleven slots in `../src/vesting_ui.rep`
   and forwards each to the matching `vesting_*` C export as JSON text.
2. Map wallet account IDs (base58 in the current mock) to the 32-byte hex IDs the C ABI takes,
   and supply the connected caller account to `claim*`, `cancel`, `makeIrrevocable`, `signal`
   and `transfer` from the wallet provider SDK, not from the view.
3. Generate the module's `.lidl` / provider scaffold with the builder revision the reference
   Names module uses, then package with `nix build .#lgx-portable` on the Mac mini.
4. Done: the `lez` feature uses the SDK's on-chain backend for the real program (see above).
5. Confirm Qt QML's JavaScript engine has no BigInt (Qt docs say ES2016) and keep the decimal
   fallback in `../src/qml/logic.mjs` when packaging.
