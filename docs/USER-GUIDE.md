# Vest: the guide

Vest pays tokens out over time. Someone **paying out** locks tokens in a schedule for another
person; the person **receiving** claims them as they unlock. LEZ is the coin; Logos is the
network. Vest runs in Logos Basecamp and signs through your **Names Wallet**: you choose which
accounts Vest may see, and you approve every payment and claim in the Names app. Vest never sees
a key.

This guide follows the app. The same steps are on Vest's first screen, and one click away under
**Steps**, top right.

## Before you start

1. Install **Names** and **Vest** in Basecamp, and open Names once so it has a wallet.
2. Names Wallet must be on the **bridge test network** (in Names: Settings, Network). Vest says
   so plainly if it is on another one.
3. Open **Vest**. The first screen asks **What do you want to do?**: *Claim tokens sent to me*
   or *Pay someone over time*, then shows the five steps for that. Step 1 is **Connect**: Vest
   opens Names with the request, you tick the accounts you use for vesting and approve, and Names
   brings you back to Vest. You can take this back in Names at any time.

Amounts are in LEZ (or, for another token, in that token's smallest unit). Dates are written
`yyyy-mm-dd` and mean midnight UTC.

---

## Receiving tokens

1. **Connect Names Wallet.**
2. **Your schedules appear.** A schedule someone made for you shows up once you add it: ask the
   person paying you for the schedule's address (or their LNS name) and paste it under **Add a
   schedule**.
3. **See what you can claim.** Choose a schedule: Vest shows what you can claim now, what you
   have claimed, what is still locked, and when the rest unlocks. A cancelled schedule says when it
   was cancelled and what went back; what had unlocked before the cancel is still yours to claim.
4. **Claim.** Press **Check claim**. Vest shows the amount and where it goes. A public claim pays
   your own account, in the open: anyone can see the amount and where it went.
5. **Approve it in Names Wallet.** Press **Claim in Names Wallet**; Vest opens Names, which shows
   exactly what will be signed, with the network fee. Approve it there; Names brings you back to
   Vest once the claim is sent. Vest shows "Waiting for you to
   approve the claim in Names Wallet" until you do, then the transaction, and what happens next.
   You can withdraw the request while it waits.

**Private claims.** A private claim pays a private account of yours: the amount is public, but
where the tokens go after that stays hidden. It is proven on your computer and takes a few
minutes. Names Wallet can't sign a private claim for Vest yet (see *What Names Wallet can't do yet*
below); with a wallet folder (Settings, Advanced) Vest offers it.

**Transferable schedules.** If the schedule was made transferable, you can hand what is still to
unlock to another account (with a wallet folder, for now).

## Paying out

1. **Connect Names Wallet.**
2. **Choose who, how much and the schedule** on **Pay out**:
   - **Evenly over time**: it unlocks a little every moment, evenly, from the start date to the
     end date.
   - **Cliff, then evenly**: nothing unlocks before the cliff date; at the cliff, everything
     earned so far unlocks at once; after that it keeps unlocking until the end date.
   - **Milestones**: each part unlocks when you (or the person you name) say a milestone was
     reached, not on a date.

   Example: 1,000 LEZ, nothing for a year, then unlocking evenly over the next three years: choose
   *Cliff, then evenly*, start today, cliff in one year, fully unlocked in four.

   - **You can cancel it later**: until you make it permanent, you can stop it. What hasn't
     unlocked comes back to you; what has stays theirs. Making it permanent can't be undone.
   - **They can hand it to someone else**: the person receiving can pass the rest of the schedule
     to another account. This is fixed when you create it.
   - **More options**: another token; and someone else who may cancel, or who says when a
     milestone is reached (a board, a lawyer). By default that is you.
   - **Who receives it**: their address or LNS name. Vest looks the name up as you type and
     shows who it pays (`Pays alice · 2BhK…fGrD`), or says plainly that no one holds that name.
     Create won't go ahead with a name it can't find.
   - **Several people, one transaction**: one row per person, up to 140.
3. **Approve it in Names Wallet.**
4. **Share the schedule's address or your LNS name** with the person receiving, so they can add
   it in Vest.
5. **Later:** on **Pay out**, under *Schedules you manage*: cancel a schedule, make it permanent,
   or unlock a milestone. Cancelling and making permanent each ask you to confirm, because neither
   can be undone.

## What Names Wallet can't do yet

With Names Wallet 0.18 or later, Names Wallet approves every step of a **LEZ schedule**: creating,
public claims, cancelling, making permanent, unlocking a milestone and transferring. (Those steps
read the network's clock, another program's account, which Names Wallet's program calls carry
since 0.18; Names says "Reads the network clock" when it asks you.) Two things still need more
from Names Wallet's program calls than they carry:

- **Claiming a token schedule**: the claim moves the token through the token program's accounts,
  which `propose_call` can't name.
- **A private claim**: it needs your public account to sign while a private account of yours
  receives, in one transaction; Names Wallet's private calls sign nothing public.

For those, Vest can sign itself with a **wallet folder** (Settings, Advanced): a LEZ wallet made
with the LEZ wallet's command line (`wallet_config.json` and `storage.json`). Everything above
then works from Vest, and the same steps apply without the Names approvals.

## When something goes wrong

Vest says what happened in one line under the page, and what to do. A few you may see:

- **"Names Wallet isn't installed, or isn't open"**: install Names, open it once, then press
  *Check again*.
- **"Switch Names Wallet to the bridge test network"**: Names Wallet is on another network.
- **"The network is busy" / "Still busy. Try again in a minute."**: the network's public entry
  point limits how often one home can ask; Vest waits and retries for you. Nothing was sent twice.
- **"Nothing claimable yet"**: the line says when the next part unlocks.
- **"You declined the claim in Names Wallet. Nothing was sent."**: the request was rejected (or
  expired after 10 minutes); press **Check claim** again when you're ready.

The exact details of a failure (for someone helping you) are under **Technical details**, under
the message.

## For developers

The command line (`vesting`, in `cli/`) does everything the app does, against a JSON file for
practice or the network with `--network` and `--wallet`; `README.md` has every command.
