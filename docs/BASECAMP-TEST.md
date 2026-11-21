# Vest in Basecamp: install and click-through test

What this checks: the two Basecamp packages load, Vest connects to Names Wallet and comes back,
a public LEZ claim goes through Names Wallet, the payout box looks names up, and (with a wallet
folder) creating, cancelling, unlocking a milestone and a batch. Every write is a real transaction
on the public LEZ rc3 demo chain (`https://lez.nameswallet.app/`), with small amounts. Run it on a
test Basecamp (0.3.x) with nothing else at stake.

A recorded run (wallet-folder signing, 2026-11-20): `docs/live/basecamp-clickthrough-20261120.md`.

## 0. What you need

- The two packages built by `miniapp/module/stage.sh` + nix (`miniapp/README.md`):
  `vesting-<version>.lgx` (the core module) and `vesting_ui-<version>.lgx` (the app). Check their
  sha256 against the build's `SHA256SUMS`.
- **Names** installed, opened once, on the **bridge test network** (in Names: Settings, Network),
  with a public account holding a little LEZ (about 0.01 LEZ pays several fees).
- For **paying out** until Names Wallet can approve it: a LEZ rc3 wallet folder
  (`wallet_config.json` + `storage.json`) with a funded public account, made with the `vesting`
  CLI built `--features lez`:

  ```sh
  V="vesting --network https://lez.nameswallet.app/ --wallet <wallet folder>"
  $V wallet account new public      # prints Public/<base58>
  ```

- A schedule for your Names account to claim: make one with the wallet folder (step 5) or the CLI,
  with your Names account as the beneficiary and a start date in the past.

Amounts in the app are in LEZ; dates are `yyyy-mm-dd` (midnight UTC).

## 1. Install

1. Basecamp, Package Manager, **Install Local Package**: `vesting-<version>.lgx` first, then
   `vesting_ui-<version>.lgx`.
2. The sidebar shows Vest's icon (orange, a white **v**). Open **Vest**. Expected: the header
   "Vest · Pay tokens out over time, or claim what has vested to you · Runs on Bridge test
   network", then **What do you want to do?** with *Claim tokens sent to me* chosen and five
   numbered steps; step 1 **Connect Names Wallet** holds a **Connect** button. No red text.
3. If it says the Vest module isn't installed, the core module didn't load: stop and collect
   Basecamp's log.

## 2. Connect (Names Wallet and back)

1. Press **Connect**. Expected: Basecamp switches to **Names**, which shows "Vest wants to see
   your accounts" with your accounts to tick.
2. Tick your public account, **Approve**. Expected: Basecamp switches back to **Vest** by itself;
   step 1 has a tick, step 2 is current, and the Receive / Pay out / Settings tabs show.
3. Negative: Settings, **Disconnect Names Wallet**, then **Connect** and **Reject** in Names.
   Expected: back in Vest, "You declined the connection in Names Wallet. Nothing was sent." Connect
   again and approve.

## 3. Receive: a public claim through Names Wallet

1. **Receive**. Your schedules are listed (or "No schedules for your accounts yet": paste the
   schedule's address under **Add schedule**). Choose one.
2. The facts: Claimable now, Claimed, Still locked, Next, From. Press **Check claim**. Expected:
   "Checking…", then **Claim … LEZ?** with To (your account, public) and the fee line.
3. **Claim in Names Wallet**. Expected: Names opens with the call (the Vest program, its label from
   the Program Registry, the fee note); approve it. Vest shows "Waiting for you to approve…", then
   "Claimed … LEZ · tx …" and "What happens next: …"; Claimed goes up.
4. A schedule with nothing unlocked: **Check claim** says when the next part unlocks; nothing is
   sent.

## 4. The payout box

**Pay out**, *Who receives it*:

- a registered name with an account: `Pays <name> · <address>`;
- a name nobody registered: "No one holds the LNS name …" (red), and Create stops on it;
- an address: `Pays the address …`.

Typing in the amount, milestone and batch boxes goes in whole (each keystroke lands where it was
typed).

## 5. Paying out (wallet folder, for now)

With Names Wallet, **Create** is off and says why above the button. To pay out today: Settings,
**Advanced: a wallet folder instead of Names Wallet**, give the folder (and its password if it has
one), **Sign with this wallet folder**.

1. **Pay out**, **Evenly over time**, start yesterday, fully unlocked in two days, *Who receives it*
   your Names account, **How much** `0.001`, **Create schedule**. Expected: "Creating…", then
   "Created schedule N" and what to share with them.
2. **Milestones**: two milestones `0.0003` and `0.0007`, **Create schedule**. Under *Schedules you
   manage*: **Unlock milestone 1**; it disappears and that part becomes claimable.
3. **Cancel schedule** on the milestone schedule: the confirmation says it can't be undone;
   confirm. Expected: "Cancelled schedule N. … went back to the creator."
4. **Several people, one transaction**: two rows, **Create 2 schedules**: one transaction.

Switch back with Settings, **Use Names Wallet instead**.

## 6. What to send back

The result lines and tx hashes of steps 3 and 5, a screenshot of the Names approval in 3.3, and
anything red the steps don't expect. If a call never answers, Basecamp's log for the `vesting`
module.
