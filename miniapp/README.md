# Vest mini-app (Basecamp)

A UI-only mini-app for LEZ vesting, with a **mock backend**. It covers the recipient and
creator views, the pre-claim confirmation (estimated fee, privacy disclosure, private-account
gas check), and immediate button feedback. No chain calls, wallet calls, or network access
happen yet: the backend holds three fixed sample schedules. Each backend slot maps to exactly
one SDK `Client` call, so the real vesting module can replace the mock without changing the
view (see `module/README.md`).

**Status:** the pure logic is tested with `node`; the Basecamp/nix build and live load are
**untested** (no Basecamp or nix in this environment). Steps marked **UNTESTED** below were
adapted from the working faucet mini-app in `../reference/faucet-miniapp/`.

| | |
|---|---|
| Basecamp name | `vesting_ui` |
| Needs | Basecamp 0.3.1 with the `Logos.Theme` module |
| View | `src/qml/Main.qml` |
| Logic | `src/qml/logic.mjs` (all formatting, vesting maths, validation, next-unlock text, confirmation) |
| Backend | `src/vesting_ui.rep` + `src/vesting_ui_backend.{h,cpp}` (mock, SDK-mapped slots) |
| Real module contract | `module/README.md` — one line per call |
| Data | three sample schedules, two mock destinations, mock fee of `2 LEZ` |

## What it does

**Recipient tab.** Pick one of your positions and see its kind, total, locked and claimed
amounts, the amount claimable now, and the next unlock (a date plus "in 3 days" for the
sample cliff). Choose a **public** or **private** destination from the mock accounts, then
press **Claim**.

**Pre-claim confirmation.** Before any claim, a confirmation panel shows the claimable amount
and the estimated fee. For a **private** claim it also shows the privacy disclosure — visible
on-chain: claim amount, beneficiary address, schedule address; not traceable: the destination
private account and later movements — and checks that the private account can pay the fee.
The sample private account has zero gas, so the confirmation shows a clear error and disables
**Confirm claim** until that is resolved. Every button switches at once to a busy label and a
disabled state ("Claiming…", "Creating…", "Cancelling…", "Signalling…"), then ends in a plain
result line in the **Result** panel. A claim attempted with nothing vested shows
"Nothing claimable yet. …" with the next unlock instead of a confirmation.

**Creator tab.** Shared settings (kind, token, dates, cancelable/transferable flags, cancel and
milestone authorities) plus either **one recipient** or a **batch** form with add/remove rows,
one token and one submit. Batch rows take a beneficiary, an amount and (for milestone kinds)
comma-separated tranches; `logic.mjs` validates every row before the single `createBatch` call.
**My schedules** lists the schedules with **Cancel**, **Make irrevocable**, one **Signal
tranche N** button per un-signalled milestone, and a **Transfer** action with a new-beneficiary
field for transferable positions.

Known **LNS names** are shown beside addresses, and a name may be typed wherever an account is
expected (beneficiary, batch recipients, destination, transfer target). They come from a small mock
`resolveName` seam in the module until the SDK's names module lands.

The **Effective time** fact in the recipient view shows what `backend.now()` (SDK
`Client::now()`) returns; all display maths use that time rather than the wall clock, so the
view shows the module's high-water-mark time.

## The backend interface

Each `.rep` slot is one SDK call: `list`, `now`, `preClaimSummary`, `create`, `createBatch`,
`claimPublic`, `claimPrivate`, `cancel`, `makeIrrevocable`, `signal`, `transfer`. Each returns
JSON text with `{ok, error}` plus call data. Amounts are decimal strings, times are epoch ms
and schedule IDs are numbers. Caller account, destination accounts and gas balances are
module/wallet-supplied placeholders in the mock, not SDK calls. Every amount comes with a
ready `*_display` string computed in the module; the view only displays those strings. The
full mapping is in `module/README.md`; the view calls `list()` and `now()` on load and after
each change.

## Build and test

```sh
node --test miniapp/logic.test.mjs   # TESTED: formatting, vesting maths, validation,
                                  # next-unlock, fee/privacy checks, result lines, QML hygiene
```

Result (2026-11-20): `node --test miniapp/logic.test.mjs` — **58 tests, 58 pass**. This includes
the fixed linear and cliff cases copied from `core/tests/unit.rs` (including the `u128::MAX`
and `u64::MAX` cases), so the display maths cannot drift from `vesting-core`: exact
`floor(total*(t-start)/(end-start))`, nothing before the cliff, amounts as decimal strings
with BigInt arithmetic on Node and a decimal-limb fallback in QML. Qt's JavaScript Host
Environment doc says QML implements ECMAScript 7 (ES2016), so BigInt (ES2020) is unavailable
there; the fallback is required, and the tests assert it is identical to the BigInt path.

```sh
# UNTESTED here (no nix in this environment). On the Mac mini, as for the reference:
cd miniapp && nix build .#lgx-portable --accept-flake-config
```

## Load it in Basecamp (UNTESTED)

Adapted from `../reference/faucet-miniapp/README.md`; not run here.

1. Build `logos-vesting_ui-module.lgx` with the nix command above.
2. In Basecamp, install the built `.lgx` (module `vesting_ui`), or add this `miniapp/`
   directory as a local module through the Basecamp module installer.
3. Open the **Vesting** app. Confirm the three sample schedules appear in both tabs.
4. To load from a git repo instead, point Basecamp at this repository's `miniapp/` directory;
   the flake and `metadata.json` are at its root, as the reference expects.

**Not tested inside a running Basecamp:** the backend slot was exercised only through the
QML data flow (and its logic through Node); the C++ backend was not compiled here (no
`LOGOS_MODULE_BUILDER_ROOT` / nix), and the QML was not rendered offscreen.

## Known limits / open questions

- All data is mock and resets when the app restarts; no LEZ clock, chain, or wallet access.
- The estimated fee comes from `preClaimSummary` (the SDK's placeholder is `0`; the mock uses
  `2`). Private-account gas balances (`0` and `500`) and the destination accounts are mock
  module/wallet values; the real module supplies them and re-checks the fee before submitting.
- The privacy disclosure names what is visible/hidden per RFP-017, but the mock does not make
  a private claim or produce a shielded destination.
- Only the beneficiary address is public at creation (as the RFP specifies); the mini-app does
  not offer commitment-based anonymous beneficiaries (deferred to a future RFP).
- Vesting maths for display matches `vesting-core` exactly and is pinned by fixed-case tests.
  The high-water-mark problem is resolved by displaying and using the module's effective time
  from `now()`; the mock itself does not track `last_time`, but the view shows whatever the
  real module returns.
- The core program's cancel/milestone results (and its event schema) are not surfaced by the
  mock; only the display state is.
