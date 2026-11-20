# Vest — events (RFP-017 Functionality 6)

Every state transition of the vesting program emits one LEZ `ProgramEvent` from the schedule
actor `(S, V)` (`program/src/lib.rs`, `Response::event`):

| Part | Value |
|---|---|
| emitter | the vesting program's id (`TransactionEvent.account_id`) |
| `selector` | first 8 bytes of `SHA256("vesting::<Variant>")`, e.g. `vesting::Claimed` |
| `data` | borsh `VestingEvent { schedule: AccountId [32], event: vesting_core::Event }` |

So `data` is the 32-byte schedule account followed by the core event below (tag byte included).
The core events are defined in `core/src/event.rs`; `borsh_roundtrips_state_and_events` in
`core/tests/unit.rs` checks every variant, and `program/tests/events.rs` checks that each
transition emits exactly one event with the right selector and payload. Every event carries `at`,
the effective time (`max(t, last_time)`) of the transition.

**Where to read them.** Clients read events through the indexer's `getEvents` (filter by program
id and selector) or the sequencer FFI's `sequencer_ffi_query_events`; the sequencer's JSON-RPC
serves none. On LEZ v0.4.0-rc3 a sequencer stores a block's events only for blocks that come back
through the Bedrock channel: it drops the events of the blocks it produces itself
(`lez/sequencer/core/src/lib.rs` at `bafd07be`, `Ok((_events, tx_reverted))` in the produce path).
A standalone sequencer (a mock Bedrock, as on the public demo chain) therefore stores no program
events past genesis; on a chain with Bedrock (the testnet) they are available once the block is
finalized.

## Borsh byte layout (used throughout)

| Type | Bytes | Encoding |
| --- | --- | --- |
| enum variant tag | 1 | `u8`, the index in declaration order |
| `AccountId`, `TokenId` | 32 | raw bytes |
| `Amount` (`u128`) | 16 | little endian |
| `Timestamp` (`u64`) | 8 | little endian |
| `u16` | 2 | little endian |
| `bool` | 1 | `0` = false, `1` = true |
| `Vec<T>` | 4 + n × size(T) | `u32` little-endian element count, then elements |
| `Option<T>` | 1 + size(T) | `u8` tag (`0` = None, `1` = Some), then payload |

All integers are little endian, as required by borsh 1.x.

## Event variants

### 0 — `Created`

| Field | Type | Meaning |
| --- | --- | --- |
| `creator` | `AccountId` | Account that created and funded the schedule. |
| `beneficiary` | `AccountId` | Account holding the beneficiary rights at creation. |
| `token` | `TokenId` | Token definition account held in escrow. |
| `total` | `Amount` | Total locked amount. |
| `kind` | `ScheduleKind` | Start/cliff/end, or the milestone tranche amounts. |
| `cancelable` | `bool` | Whether the schedule may be cancelled (default `true`). |
| `transferable` | `bool` | Whether the position may be transferred (default `false`). |
| `cancel_authority` | `AccountId` | Who may cancel / make irrevocable. |
| `milestone_authority` | `AccountId` | Who may signal milestones. |
| `at` | `Timestamp` | Effective creation time. |

`kind` is itself a borsh enum: 1 tag byte plus 24 bytes for `CliffLinear`
(three `u64`s), 16 bytes for `Linear` (two `u64`s), or `4 + n × 16` bytes for
`Milestone` (`Vec<u128>`). Together with the other fields this makes the event
**204 bytes** for `Linear`, **212 bytes** for `CliffLinear`, and `192 + 16 × n`
bytes for `Milestone` (704 bytes at the 32-tranche maximum). A dashboard can
rebuild the whole schedule from this event alone.

### 1 — `Claimed`

| Field | Type | Meaning |
| --- | --- | --- |
| `amount` | `Amount` | Tokens released by this claim. |
| `to_private` | `bool` | `true` when claimed to a private (shielded) account. |
| `claimed_total` | `Amount` | Running total claimed from this schedule after the claim. |
| `at` | `Timestamp` | Effective time of the claim. |

Total size: 1 + 16 + 1 + 16 + 8 = **42 bytes**.

Observers can see the claim amount and timestamp, but not which private
account received the tokens when `to_private` is `true`.

### 2 — `Cancelled`

| Field | Type | Meaning |
| --- | --- | --- |
| `vested_at_cancel` | `Amount` | Vested amount frozen at cancellation; still claimable by the beneficiary. |
| `refund` | `Amount` | Unvested amount returned to the creator (`total - vested_at_cancel`). |
| `at` | `Timestamp` | Effective time of the cancellation. |

Total size: 1 + 16 + 16 + 8 = **41 bytes**.

### 3 — `MilestoneSignalled`

| Field | Type | Meaning |
| --- | --- | --- |
| `index` | `u16` | Index of the tranche that was signalled. |
| `amount` | `Amount` | Fixed amount unlocked by this tranche. |
| `at` | `Timestamp` | Effective time of the signal. |

Total size: 1 + 2 + 16 + 8 = **27 bytes**.

### 4 — `BeneficiaryTransferred`

| Field | Type | Meaning |
| --- | --- | --- |
| `old` | `AccountId` | Previous beneficiary. |
| `new` | `AccountId` | New beneficiary. |
| `at` | `Timestamp` | Effective time of the transfer. |

Total size: 1 + 32 + 32 + 8 = **73 bytes**.

### 5 — `MadeIrrevocable`

| Field | Type | Meaning |
| --- | --- | --- |
| `at` | `Timestamp` | Effective time of the change. |

Total size: 1 + 8 = **9 bytes**.

## State layout (for reference)

`Schedule` in `core/src/types.rs` is borsh-serialised as:

`creator [32] · beneficiary [32] · token [32] · total [16] · claimed [16] ·
kind [1 tag + payload] · cancelable [1] · transferable [1] ·
cancel_authority [32] · milestone_authority [32] · signalled [4 + words × 8] ·
cancelled [1 + optional 24] · last_time [8] · version [1]`

The `kind` payload is 24 bytes for `CliffLinear` (three `u64`s), 16 bytes for
`Linear` (two `u64`s), and `4 + n × 16` bytes for `Milestone` (`Vec<u128>`).
`signalled` is a packed bitmap: one bit per milestone tranche in 64-bit words.
`cancelled` is one tag byte plus 24 bytes when `Some` (`at` 8 + `vested_at_cancel` 16).
