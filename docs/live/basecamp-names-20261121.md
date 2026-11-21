# Basecamp click-through through Names Wallet, 21 November 2026

Jef's own Basecamp (0.3.2), checked by the conductor; no transaction was sent. Packages installed: `vesting` core module
0.3.3 sha256 `688a39ed9fcd5d8f8f912159e264c891a2d3f7673ddc4344212bd666cff2d224`, and the
`vesting_ui` app 0.3.4 sha256 `f9c409e61353db8dee925b4764933a165892043bbac3126aacd523974070b786`.
Network shown in the app: **Bridge test network**.

This run covers the first screen, connecting through Names Wallet, and the payout box's name
lookup. It does not cover sending a claim, creating a schedule, cancelling, milestones, transfer or
batch; those keep their earlier evidence (`docs/live/basecamp-clickthrough-20261120.md`,
`docs/BASECAMP-TEST.md`).

| Step | Result |
|---|---|
| Install both packages | installed; the module and app load; the sidebar icon is orange with a white lowercase **v** |
| First screen | "What do you want to do?" with *Claim tokens sent to me* and *Pay someone over time*, then five numbered steps; step 1 **Connect Names Wallet** holds a **Connect** button. Header says the app runs on **Bridge test network**. No red text |
| Connected | Vest was already connected through Names Wallet (Jef approved it in Names earlier): step 1 ticked, the Receive / Pay out / Settings tabs shown. Names did not bring the person back to Vest on its own in 0.3.4; that comes with 0.3.5 |
| Payout box: type `jef04` | "Pays jef04 · CCVd76…pKnk" — a registered name with an account |
| Payout box: type `jef` | "No one holds the LNS name jef." — not registered on this chain; red, Create stops on it |
| Payout box: type `jefdiesel` | registered but its record has no account; no pay line. Known Names bug, being fixed there; not a Vest defect |
| Amount box: type `1000` | the whole value went in, each keystroke where it was typed (the one-character bug was fixed in 0.3.2) |
| Create in Names mode | the Pay out tab says "Paying out with Names Wallet: not yet" and why; **Create** is off. A wallet folder (Settings, Advanced) still signs creates, cancels and milestones |

Noticed, not fixed here: in Names mode Create waits on Names 0.18; the `jefdiesel` case is a Names
record issue. Neither is a change Vest can make.

**0.3.5: pending.** The next build makes Names open with the request and bring the person back once
they answer. This run used 0.3.3 / 0.3.4; re-check the connect step when 0.3.5 is installed.
