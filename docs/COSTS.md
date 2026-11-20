# Vest — costs on LEZ v0.4.0-rc3

Measured 2026-11-20 with `scripts/measure.sh` (the CLI's `--dry-run`, which runs the wallet's own
simulation and prints its report) on a standalone rc3 sequencer on the Mac mini, against the
program built from this branch. The figures are the wallet's: "metered cycles" is the execution of
the public part, which the transaction declares as gas (cycles + 25%), and the fee is at the base
fees of the test chain (8 per unit of execution and of storage gas; the public rc3 chain showed
the same base fees on the same day). LEZ has no separate "compute unit": gas is metered zkVM
cycles.

## Per operation

| Operation | Cycles (public part) | Wire bytes | Declared gas | Fee at head |
|---|---:|---:|---:|---:|
| Create, native, 1 schedule (linear) | 76,132 | 828 | 95,165 | 615,680 |
| Create, native, milestone (2 tranches) | 77,655 | 848 | 97,069 | 628,024 |
| Create, token (0.4 token program) | 141,118 | 956 | 176,398 | 1,136,592 |
| Create, token, with fee sponsor | 143,188 | 1,116 | 178,985 | 1,154,432 |
| Claim to a public account, native | 41,538 | 664 | 51,923 | 337,616 |
| Claim to a public account, milestone | 40,496 | 664 | 50,620 | 329,280 |
| Claim to a public account, token | 68,874 | 792 | 86,093 | 557,328 |
| Claim to a private account, native | 41,538 + proof | – | – | 5,402,920 |
| Claim to a private account, token (holding init + sponsor draw) | 70,660 + proof | – | – | 5,635,896 |
| Cancel, native | 73,102 | 687 | 91,378 | 590,312 |
| Cancel, token | 101,716 | 815 | 127,145 | 820,248 |
| Signal milestone | 69,901 | 627 | 87,377 | 564,224 |
| Transfer beneficiary | 72,090 | 655 | 90,113 | 581,960 |
| Make irrevocable | 69,576 | 623 | 86,970 | 561,592 |

Notes:
- Cancel, signal, transfer, make irrevocable and create read the clock (a call to the clock and
  its reply), which is most of the difference to a claim, which is sent "as of" a time and reads no
  clock.
- A private claim is a privacy-preserving transaction: the cycles above are its public part; the
  private part is proven by the claimant. With real proofs on the Mac mini (local prover, 4
  threads) a native private claim of the earlier spike program (same flow, smaller program) took
  458 s and 725 s on the public rc3 chain (peak footprint about 9.4 GB). This program's private
  claims have run with dev-mode receipts only so far; a token private claim (holding
  initialisation + sponsor draw) has not been proven for real yet. The fee of a private transaction is paid inside the proof by the private account
  (from what it receives for native LEZ; from the creator-funded sponsor float for a token).

## Batch create

One create transaction carries up to `MAX_BATCH` = 140 schedules (`core/src/types.rs`).

| Schedules | Native cycles | Native bytes | Token cycles | Token bytes |
|---:|---:|---:|---:|---:|
| 1 | 76,132 | 828 | 141,118 | 956 |
| 8 | 549,482 | 2,907 | 1,070,731 | 3,483 |
| 16 | 1,093,590 | 5,283 | – | 6,371 |
| 64 | 4,357,551 | 19,539 | 8,526,962 | 23,699 |
| 74 | | | 9,859,240 (applies) | 27,309 |
| 76 | | | over the cap | 28,031 |
| 128 | 8,709,135 | 38,547 | | |
| 146 | 9,940,791 (applies) | 43,893 | | |
| 148 | over the cap | 44,487 | | |

- Sent for real on the throwaway rc3 chain with `--gas-limit 10000000`: 140 native schedules in
  one transaction (`74e962fe…`, block 776) and 72 token schedules in one (`88f5f327…`, block 777).
- The binding limit is the 10,000,000-cycle execution cap of a public transaction (about 68,000
  cycles per native schedule and 133,000 per token schedule), not the transaction's size (44 KB at
  146 native schedules). Hence `MAX_BATCH = 140` for native schedules with a margin; a token batch
  stops near 74 and the wallet refuses a larger one in simulation, before anything is sent or paid.
- The wallet declares at most 2,000,000 gas by default, enough for about 29 native or 14 token
  schedules: larger batches need `--gas-limit 10000000`. A 10M-gas create costs about 80,000,000
  in fees at base fee 8.
