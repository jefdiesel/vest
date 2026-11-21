# Vest: the guide

Vest pays tokens out over time. Someone **paying out** locks tokens in a schedule for another
person; the person **receiving** claims them as they unlock. LEZ is the coin; Logos is the
network. Vest runs in Logos Basecamp and signs through your **Names Wallet**: you choose which
accounts Vest may see, and you approve every payment and claim in the Names app. Vest never sees
a key and has no signer of its own. (Another wallet that implements the Names Wallet SDK works
the same way; with more than one installed, Vest asks once which to use, and Settings can change
it.)

This guide follows the app. The same steps are on Vest's first screen, and one click away under
**Steps**, top right.

## Before you start

1. Install **Names** and **Vest** in Basecamp, and open Names once so it has a wallet.
2. Names Wallet must be on the **bridge test network** (in Names: Settings, Network). Vest says
   so plainly if it is on another one.
3. Open **Vest**. The first screen asks **What do you want to do?**: *Claim tokens sent to me*
   or *Pay someone over time*, then shows the five steps for that. Step 1 is **Connect Names
   Wallet**: Vest opens Names with the request, you tick the accounts you use for vesting and
   approve, and Names
   brings you back to Vest. You can take this back in Names at any time.

Amounts are in LEZ. Dates are set in a calendar: press **Starts**, **Cliff** or **Fully
unlocked** and pick the day, hour and minute in your local time.

---

## Receiving tokens

1. **Connect Names Wallet**
2. **Your schedules appear here (or paste the address someone sent you)**. A schedule someone made
   for you shows up once you add it: ask the person paying you for their **schedule code** (or their
   LNS name) and paste it under **Add a schedule**. An LNS name can't find a schedule yet, so the
   schedule code is what works. If you paste something else, Vest says why in plain words.
3. **See what you can claim now and when the rest unlocks**. Choose a schedule: Vest shows what you
   can claim now, what you have claimed, what is still locked, and when the rest unlocks. A
   cancelled schedule says when it
   was cancelled and what went back; what had unlocked before the cancel is still yours to claim.

   **From** shows who made the schedule. **Show** reveals the full address, **Copy** copies it, and
   **Open in Names** opens that name or address in Names Wallet when it is connected. The
   **Schedule code** row has **Copy schedule code**.
4. **Claim to your public account, or privately**. Press **Check claim**. Vest shows the amount and
   where it goes. A public claim pays your own account, in the open: anyone can see the amount and
   where it went.
5. **Approve it in Names Wallet**. Press **Claim in Names Wallet**; Vest opens Names, which shows
   exactly what will be signed, with the network fee. Approve it there; Names brings you back to
   Vest once the claim is sent. Vest shows "Waiting for you to
   approve the claim in Names Wallet." until you do, then the transaction, and what happens next.
   You can withdraw the request while it waits.

**Private claims.** Claims go to your public account for now: Names Wallet can't sign a private
claim for Vest yet (see *What Names Wallet can't do yet* below), so Vest doesn't offer one.

**They can hand it to someone else.** If the schedule was made transferable, you can hand what is
still to unlock to another account; Names Wallet 0.18 or later approves it for a LEZ schedule.

## Paying out

1. **Connect Names Wallet**
2. **Choose who, how much and the schedule** on **Pay out**:
   - **Evenly**: it unlocks a little every moment, evenly, from the start date to the end date.
   - **Cliff, then evenly**: nothing unlocks before the cliff date; at the cliff, everything
     earned so far unlocks at once; after that it keeps unlocking until the end date.
   - **Milestones**: each part unlocks when you (or the person you name) say a milestone was
     reached, not on a date.

   Example: 1,000 LEZ, nothing for a year, then unlocking evenly over the next three years: choose
   *Cliff, then evenly*, pick today in the calendar for **Starts**, one year on for **Cliff** and
   four years on for **Fully unlocked**.

   - **You can cancel it later**: until you make it permanent, you can stop it. What hasn't
     unlocked comes back to you; what has stays theirs. Making it permanent can't be undone.
   - **They can hand it to someone else**: the person receiving can pass the rest of the schedule
     to another account. This is fixed when you create it.
   - **More options**: someone else who may cancel, or who says when a milestone is reached (a
     board, a lawyer). By default that is you.
   - **Who receives it**: their address or LNS name. Vest looks the name up as you type and
     shows who it pays (`Pays alice · 2BhK…fGrD`), or says plainly that no one holds that name.
     With Names Wallet connected, the name is looked up through Names, and the schedule is made
     for the account Names found. Create won't go ahead with a name it can't find.
   - One person per schedule, in LEZ: Vest says so under the form (see below).
   Before Names opens, Vest reads the paying account's balance. If it cannot cover the schedule,
   Vest stops and says: "This schedule needs 1,000 LEZ, and the paying account holds 500 LEZ.
   Lower the amount, or add LEZ to the account first."
3. **Approve it in Names Wallet**
4. **Send them the schedule code**. Press **Copy schedule code** and send it to them. In their
   Vest they open **Receive**, paste it and press **Add schedule**. An LNS name cannot find a
   schedule yet, so share the code.
5. **Later:** on **Pay out**, under *Schedules you manage*: cancel a schedule, make it permanent,
   or unlock a milestone. Cancelling and making permanent each ask you to confirm, because neither
   can be undone.

**The receipt.** After a create, a card shows what was made: **Amount**, **From** (who pays),
**Receives** (who gets it), **Plan**, **Rules**, **Transaction** and **Schedule code**. Each row
with a full address has **Show** and **Copy**. The card ends with the same sharing line, a **Done**
button and **Copy schedule code**.

## What Names Wallet can't do yet

With Names Wallet 0.18 or later, Names Wallet approves every step of a **LEZ schedule**: creating,
public claims, cancelling, making permanent, unlocking a milestone and transferring. (Those steps
read the network's clock, another program's account, which Names Wallet's program calls carry
since 0.18; Names says "Reads the network clock" when it asks you.) Three things it can't
approve yet, so Vest doesn't offer them:

- **Token schedules** (paying out, or claiming, a token other than LEZ): the claim moves the
  token through the token program's accounts, which `propose_call` can't name.
- **A private claim**: it needs your public account to sign while a private account of yours
  receives, in one transaction; Names Wallet's private calls sign nothing public.
- **Several schedules in one transaction (batch create)**: Names Wallet 0.18 does not approve
  batch creates yet. Make one schedule per person.

The command line (`vesting`, below) still signs with its own wallet and does all three.

## When something goes wrong

Vest says what happened in one line under the page, and what to do. A few you may see:

- **"No wallet installed: install Names from Basecamp's app list."**: install Names, then press
  *Check again*.
- **"Names Wallet isn't installed, or isn't open. Install Names from Basecamp's app list, open it
  once, then come back here."**: press *Check again* once Names is open.
- **"Which wallet should Vest use?"**: more than one wallet is installed; choose one. Vest
  remembers it (Settings, *Use another wallet*, to change it).
- **"Names Wallet is on another network. Switch Names Wallet to the bridge test network (in Names:
  Settings, Network), then come back."**: change the network in Names.
- **"Names Wallet is busy."**: Vest waits and tries again a few times; nothing was sent twice.
- **"The network is busy. Trying again in a few seconds."** / **"Still busy. Try again in a
  minute."**: the network's public entry point limits how often one home can ask; Vest waits and
  retries for you. Nothing was sent twice.
- **"Nothing claimable yet"**: the line says when the next part unlocks.
- **"You declined the claim in Names Wallet. Nothing was sent."**: the request was rejected (or
  expired after 10 minutes); press **Check claim** again when you're ready.

- **"That address isn't a Vest schedule. Ask the person paying you for the schedule code (Vest
  shows it after they create it) and paste that here."** An LNS name in **Add a schedule** gets a
  plainer version: it is a person's name, not a schedule. Share the schedule code instead.

The exact details of a failure (for someone helping you) are under **Technical details**, under
the message.

## For developers

The command line (`vesting`, in `cli/`) does everything the app does, against a JSON file for
practice or the network with `--network` and `--wallet`; `README.md` has every command.
