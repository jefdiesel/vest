# Vest viewer (static, read-only)

A single static page that shows vesting schedules read from the LEZ 0.4 sequencer. No build step,
no framework, no writes: it only calls `getAccountView`, `getEvents` (best effort), and the clock.

```sh
cd web/viewer && python3 -m http.server 8099
# open http://127.0.0.1:8099/
```

The sequencer may apply origin/CORS rules (the site explorer sends `Origin: https://nameswallet.app`);
serve from an allowed origin or use a local proxy if a page is blocked.

## Inputs

- **Schedule account** — decoded directly from its `(S, V)` actor state.
- **Creator + nonce** — derives `S = PDA(V, SHA256(SEED_TAG | creator | nonce))` and reads it. Useful
  when you know who created a schedule but not its address.
- **LNS name** (e.g. `jef05`) — resolved locally with the Names v5 resolver (Argon2id, ~1 s); only
  the derived name id goes to the sequencer. Uses the public account from the record.
- **Beneficiary address** — the program has no on-chain index keyed by beneficiary. Enter the
  schedule account or the creator + nonce; a beneficiary-only lookup needs an event-capable
  sequencer and is not guaranteed.

Program id is a field, defaulting to `2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2`. Change it if
the program is redeployed.

## What is shown

Kind, total, vested now, claimed, claimable, locked, next unlock, cancellation state,
transferability, creator/beneficiary, and the state's effective time. Amounts are `u128` decoded as
BigInt and displayed with thousands separators. Vesting uses the same floor maths as `vesting-core`:
linear `floor(total * (t - start) / (end - start))`, nothing before a cliff, milestone tranches
summed from the signalled bitmap, a high-water mark from `last_time`, and vested frozen at
cancellation. Event history (Created/Claimed/Cancelled/MilestoneSignalled/BeneficiaryTransferred/
MadeIrrevocable) is shown when the RPC serves `getEvents`; otherwise the page says so.

## Layout and tests

- `logic.mjs` — pure decoding, maths and presentation; no DOM or chain calls.
- `viewer.mjs` — DOM wiring and RPC reads.
- `logos-sdk/` — vendored read-only modules from logos-sdk-js (plus a small
  facade), so the page has no package install step.
- `test/` — `node --test test/logic.test.mjs` (9 tests) against recorded borsh fixtures in
  `test/fixtures/`; regenerate them with `node test/fixtures/generate.mjs`. No live chain needed.
