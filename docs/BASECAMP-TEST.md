# Vest in Basecamp: install and click-through test

What this checks: the two Basecamp packages load, Vest connects to Names Wallet and comes back,
a public LEZ claim goes through Names Wallet, the payout box looks names up through Names, and
creating, cancelling and unlocking a milestone through Names Wallet (0.18 or later). Vest has no
signer of its own in Basecamp. Every write is a real transaction
on the public LEZ rc3 demo chain (`https://lez.nameswallet.app/`), with small amounts. Run it on a
test Basecamp (0.3.x) with nothing else at stake.

A recorded run of an older Vest (wallet-folder signing, removed since, 2026-11-20):
`docs/live/basecamp-clickthrough-20261120.md`.

## 0. What you need

- The two packages built by `miniapp/module/stage.sh` + nix (`miniapp/README.md`):
  `vesting-<version>.lgx` (the core module) and `vesting_ui-<version>.lgx` (the app). Check their
  sha256 against the build's `SHA256SUMS`.
- **Names** installed, opened once, on the **bridge test network** (in Names: Settings, Network),
  with a public account holding a little LEZ (about 0.01 LEZ pays several fees).
- **Names 0.18 or later** for paying out (it approves calls that read the network clock).
- A schedule for your Names account to claim: make one in step 5, or with the `vesting` CLI
  (built `--features lez`, its own wallet folder), with your Names account as the beneficiary and
  a start date in the past.

Amounts in the app are in LEZ; dates are picked in a calendar (day, hour and minute in your local
time), not typed as `yyyy-mm-dd`.

## 1. Install

1. Basecamp, Package Manager, **Install Local Package**: `vesting-<version>.lgx` first, then
   `vesting_ui-<version>.lgx`.
2. The sidebar shows Vest's icon (orange, a white **v**). Open **Vest**. Expected: the header
   "Vest · Pay tokens out over time, or claim what has unlocked for you · Runs on Bridge test
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
   **schedule code** under **Add schedule**). Choose one.
2. The facts: Claimable now, Claimed, Still locked, Next, From. On **From**, **Show** reveals the
   full address, **Copy** copies it and **Open in Names** opens it in Names Wallet. The **Schedule
   code** row has **Copy schedule code**. Press **Check claim**. Expected:
   "Checking…", then **Claim … LEZ?** with To (your account, public) and the fee line.
3. **Claim in Names Wallet**. Expected: Names opens with the call (the Vest program, its label from
   the Program Registry, the fee note); approve it. Vest shows "Waiting for you to approve…", then
   "Claimed … LEZ · tx …" and "What happens next: …"; Claimed goes up.
4. A schedule with nothing unlocked: **Check claim** says when the next part unlocks; nothing is
   sent.

4. Paste an LNS name under **Add schedule** instead of a schedule code. Expected: a plain reason,
   "... is a person's LNS name, not a schedule. Ask them for the schedule code (Vest shows it
   after they create it) and paste that here." An address that is not a schedule gets the same
   plain kind of answer.

## 4. The payout box

**Pay out**, *Who receives it*:

- a registered name with an account: `Pays <name> · <address>`;
- a name nobody registered: "No one holds the LNS name …" (red), and Create stops on it;
- an address: `Pays the address …`.

Typing in the amount and milestone boxes goes in whole (each keystroke lands where it was typed).

## 5. Paying out (through Names Wallet)

With Names older than 0.18, **Create** is off and says why above the button. Each step below
opens Names with the call; choose the account it names and approve; Names brings you back.

1. **Pay out**, **Evenly**, pick yesterday in the calendar for **Starts** and two days on for
   **Fully unlocked**, *Who receives it* your Names account, **How much** `0.001`, **Create
   schedule**. Before Names opens, Vest checks the paying account's balance; if it cannot cover
   `0.001` LEZ, the expected line is "This schedule needs … LEZ, and the paying account holds …
   LEZ. Lower the amount, or add LEZ to the account first.", and no wallet opens. Otherwise
   expect "In Names Wallet, choose … and approve.", then "Created schedule N · … · tx …".
   The **receipt** card opens after the create: **Amount**, **From**, **Receives**, **Plan**,
   **Rules**, **Transaction** and **Schedule code**, with **Show** / **Copy** on rows that have a
   full address, the line "Send them the schedule code (Copy schedule code, below): in their Vest
   they open Receive, paste it and press Add schedule.", a **Done** button and **Copy schedule
   code**. Press **Copy schedule code** and check the clipboard holds the full code.
2. **Milestones**: two milestones `0.0003` and `0.0007`, **Create schedule**. Under *Schedules you
   manage*: **Unlock milestone 1**; it disappears and that part becomes claimable.
3. **Cancel schedule** on the milestone schedule: the confirmation says it can't be undone;
   confirm and approve in Names. Expected: "Cancelled schedule N. … went back to the creator."
4. The form offers no batch, no other token and no private claim, and says so under it ("One
   person per schedule, in LEZ…").
5. Settings: **Signing** says Vest signs through Names Wallet and names the wallet in use; there
   is no wallet folder anywhere. A Vest that had a wallet folder saved before switches to Names
   Wallet by itself.

## 6. What to send back

The result lines and tx hashes of steps 3 and 5, a screenshot of the Names approval in 3.3, and
anything red the steps don't expect. If a call never answers, Basecamp's log for the `vesting`
module.
