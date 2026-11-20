# Vest — reliability tests (RFP-017)

The tests below are in `sdk/tests/reliability.rs`, except the derivation half of R3, which is in
`program/tests/derivation.rs`. The SDK tests drive an in-memory `Backend` through `Client`, the
same shape as `sdk/tests/client.rs`; the program test uses the program's own derivation and
encoders, which the LEZ backend also uses. On chain, the same properties ran in
`scripts/e2e.sh` (dev-mode receipts) and the public smoke (see `docs/REQUIREMENTS-MAP.md`).

| Row | Test | What it proves |
| --- | --- | --- |
| R1 | `r1_failed_or_refused_claim_changes_nothing_and_a_retry_succeeds` | A claim that fails (nothing vested), is refused (wrong caller), or is rejected by SDK validation (public target for a private claim) stores no new state and emits no event: `total`, `claimed`, the escrow remainder and the event count are unchanged. The same claim by the beneficiary then succeeds once, moving the right amount. |
| R2 | `r2_cancel_is_all_or_nothing_and_refund_plus_vested_equals_escrow` | A cancel by the wrong caller, or on an irrevocable schedule, changes nothing. A successful cancel emits exactly one `Cancelled` event whose `vested_at_cancel + refund` equals the whole escrow; a second cancel is refused with no further change. The beneficiary's vested part stays claimable and `claimed + refund` equals the original escrow. |
| R3 | `r3_interleaved_claims_on_two_schedules_do_not_cross_effect` | Claims on two schedules of different totals, interleaved at three clock times, pay only from their own schedule: after each claim the other schedule's state is byte-for-byte unchanged. Both finish fully claimed with the expected totals and only `Created`/`Claimed` events. |
| R3 | `r3_schedule_derivation_never_collides_for_one_creator` (program crate) | For one creator and program, 256 successive nonces, seeds and PDA accounts are all distinct and deterministic; the same nonce under another creator or program gives another account. A single `Create` instruction carrying 64 schedules wires 64 distinct nonces into the bytes. |
| R4 | `r4_signalling_a_milestone_twice_is_a_deterministic_error_and_never_unlocks_twice` | Signalling index 0 twice on the same state returns an identical `MilestoneAlreadySignalled` (code 18) error; through the Client it stores no state or event and claimable stays at 40, not 80. After the second tranche is signalled, re-signalling index 0 is still refused, and one claim pays the full 100 exactly once. |

## Limits

- The in-memory ledger stores schedules and events, not token balances.
  "Escrow balance" in these tests means a schedule's unclaimed remainder,
  `schedule.total - schedule.claimed` (see `escrow()` in the test file). The
  helper does not subtract a creator's cancel refund, so R2 checks the refund
  against the event's `vested_at_cancel` instead. No real tokens move.
- Transaction rollback is owned by the chain: a refused operation panics in the program and the
  whole transaction reverts. These tests show that a failed or refused operation stores no new
  state and emits no event in the SDK; on chain, the e2e run's refusals (pre-cliff claim, repeated
  signal, cancel of an irrevocable schedule, a non-beneficiary claim) left state unchanged, and a
  claim sent anyway before the cliff was included and reverted with the escrow untouched.
- R3 on-chain concurrency (two real transactions in one block) is not shown.
  Only state isolation through the SDK and the pure derivation functions are
  covered.
- `LezBackend` (`sdk/src/lez.rs`) is exercised by `e2e/tests/chain.rs` against a sequencer, not
  here.
