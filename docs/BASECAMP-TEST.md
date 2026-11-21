# Vesting in Basecamp: install and click-through test

What this checks: the two Basecamp packages load, the app talks to the real module, and the
module talks to the chain through a LEZ wallet: settings and the chain-id check, the wallet's
accounts, creating a schedule, the pre-claim summary with the simulated fee, a public claim, the
private claim's fee check and disclosure, cancel, milestone signal, a batch, LNS names, import.

Run it on a test Basecamp (0.3.x) with nothing else at stake. Every write below is a real
transaction on the public LEZ rc3 demo chain (`https://lez.nameswallet.app/`), with small amounts.

## 0. What you need

- The two packages built by `miniapp/module/stage.sh` + nix (see `miniapp/README.md`):
  `vesting-0.3.0.lgx` (the core module) and `vesting_ui-0.3.0.lgx` (the app). Check their sha256
  against the build's `SHA256SUMS` before installing.
- A LEZ rc3 wallet folder (`wallet_config.json` + `storage.json`) with two public accounts, `A`
  (creator) and `B` (beneficiary), and one private account `P`, all on the demo chain. With the
  `vesting` CLI (built `--features lez`):

  ```sh
  V="vesting --network https://lez.nameswallet.app/ --wallet <wallet folder>"
  $V wallet account new public      # A: prints Public/<base58>
  $V wallet account new public      # B
  $V wallet account new private     # P: prints Private/<base58>
  ```

- Native LEZ on `A` (pays the creates and the cancel/signal transactions) and on `B` (pays its
  public claim's fee): about 0.01 LEZ each is plenty. Send it from a funded wallet with
  `$V wallet auth-transfer send --from Public/<funded> --to Public/<A or B> --amount 10000000`.
- Optional: an LNS name registered on the demo chain whose account is `B` (for step 7).

Amounts in the app are in the smallest unit (9 decimals: `1000000` is 0.001 LEZ). Dates are
`yyyy-mm-dd` (UTC midnight).

## 1. Install

1. Basecamp, install from file: `vesting-0.3.0.lgx` first, then `vesting_ui-0.3.0.lgx`.
2. Open **Vesting**. Expected: no red text; a card "Connect a wallet to start" with **Open
   settings**. If it instead says the Vesting module isn't installed, the core module didn't
   load: stop and collect Basecamp's log.

## 2. Settings and the chain check

1. **Settings** tab. The facts show the built-in network: name "Bridge test network (LEZ rc3
   demo chain)", RPC `https://lez.nameswallet.app/`, chain id `2da7f7df…5374`, vesting program
   `2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2`, token program
   `FCvRhHFsraY3pBKajWtvC1QCsvXw7MCeYBbNgMnjNzqz`, LNS registry
   `6Yxj5k8AiuBSuMAYVQFGrYKjJGUSSaWWgoB9D9XXN3gA`.
2. Type the wallet folder (and its password if it is encrypted), **Save and connect**. Expected:
   the button says "Connecting…", then the status turns green "Connected: Bridge test network …"
   and Result says "Connected to …".
3. Negative: type chain id `00…00` (64 zeros) in the overrides, **Save and connect**. Expected: red
   "Saved, but not connected: https://lez.nameswallet.app/ serves chain 2da7f7df…, not 0000…".
   Clear the field and **Save and connect** again (green).

## 3. Creator view: create

1. **Create** tab. The wallet's public accounts are listed with balances; choose `A`.
2. Cliff + linear is preselected: choose **Linear**. Token `LEZ`. Start date: yesterday. End date:
   tomorrow + 1 day. Beneficiary: `B`'s base58. Total amount `1000000`.
3. **Create schedule**. Expected: "Creating…", then Result "Created schedule 0. tx …… in block N".
   "Schedules you manage" lists `0 · linear · 0.001 LEZ` with **Cancel** and **Make irrevocable**.
4. Milestone: choose **Milestone**, beneficiary `B`, tranches `300000` and `700000` (+ Add tranche),
   **Create schedule**. Expected: schedule 1, with **Signal tranche 1** and **Signal tranche 2**.

## 4. Recipient view: summary, public claim

1. **Your schedules** tab. Expected: positions `0 · linear · LEZ` and `1 · milestone · LEZ`; choose 0.
   The facts show the schedule account, beneficiary, total, locked, claimed 0, chain time; "2. Claim"
   shows "Claimable now: …" above 0 (about a third of it, by the dates).
2. **Public account** is chosen; "Pays" names `B`. Press **Check claim**. Expected: "Checking the fee…",
   then the **Claim …** panel: the claimable amount, an **Estimated fee** above 0 (the
   simulated fee of this very claim), destination `B (public)`, and a green line "Your public
   account can pay the estimated fee of …". No privacy text.
3. **Claim …**. Expected: "Claiming…", then Result "Claimed … LEZ to … (public) · tx …
   in block N"; Claimed in the facts goes up and Claimable drops to (nearly) 0.
4. Choose position 1 (milestone, nothing signalled) and **Claim**. Expected: no confirmation;
   Result "Nothing claimable yet. Next unlock: tranche 1 — signalled by the creator."

## 5. Private claim: disclosure and fee check (do not confirm)

1. Position 0, **Private account**. The wallet's private account `P` is listed with its balance;
   choose it. **Claim**.
2. Expected: the confirmation shows destination `P (private)`, the privacy disclosure in amber
   ("Visible on-chain: the claim amount, the beneficiary address, the vesting schedule address.
   Not traceable: which private account receives the tokens, any later movement of those
   tokens."), the estimated fee of the private claim, and the fee check: green "The private
   account (paid from what the claim brings it) can pay …" when the claimable amount covers the
   fee, or red "… can't pay the estimated fee …" with **Claim …** disabled when it doesn't.
3. Press **Cancel** (Result: "Claim not sent."). Confirming would prove the claim on this machine
   (about 10 minutes with a real proof on this chain); only do it on purpose.

## 6. Creator actions

1. **Create** tab, schedule 1: **Signal tranche 1**. Expected: "Signalling…", then "Signalled
   milestone 1 on 1. Unlocked 0.0003 LEZ." and the button disappears. Signal it again is no
   longer offered; schedule 1 is now claimable on the Recipient tab (claim it publicly as in 4).
2. Schedule 0: **Make irrevocable**. Expected: "Made schedule 0 irrevocable."; its row says
   "irrevocable" and Cancel is gone.
3. Schedule 1: **Cancel**. Expected: "Cancelled schedule 1. Refunded 0.0007 LEZ to the creator."
   and the row says CANCELLED. What was vested before the cancel stays claimable by `B`.

## 7. Batch, names, import

1. **Create**, **Several people, one transaction**, Linear, dates as in 3, two rows: `B` with
   `200000`, and (if you have one) `B`'s LNS name with `300000`; **Create 2 schedules**. Expected:
   Result "<name> resolves to <B's base58>. Created 2 schedules. tx … in block N" (one transaction).
   A mistyped name fails with "unknown LNS name '…'" and nothing is created.
2. **Your schedules**, Add a schedule: paste the schedule account of a schedule made elsewhere for `B` (for
   example by the CLI); **Import**. Expected: "Imported the schedule as N." and it is listed.
   An account that is not a schedule of this program is refused with a plain error.

## 8. What to send back

The Result lines and tx hashes of steps 3, 4, 6 and 7, screenshots of the two confirmations (4.2
and 5.2), and anything red that the steps don't expect. If a call never answers, Basecamp's log
for the `vesting` module.
