# Vest — SPEL and the vesting IDL — what exists, what is missing

Read-only survey of `logos-co/spel`
(2026-11-20). SPEL **can** describe this program's interface: use the `refactor/lez-v0.4.x`
branch's **IDL v2**, which models LEZ 0.4 actors by messages, state and events. `idl/vesting.json`
is written in that format and checked against the real Borsh encoding by `idl-check`.

## What SPEL supports today

- **Default branch `main`** (`README.md:5-7`, `docs/reference/idl.md`) is the *retired*
  instruction-based framework: `#[lez_program]`, `spel init`, scaffolding, and IDL **v1** (top-level
  `instructions`, `accounts`, `args`, with `discriminator`, `execution`, `variant`). It targets the
  older LEZ account-list model, not the 0.4 actor model. Its IDL v1 has no way to express our
  message tags, the actor state, or the event selectors; it is the wrong format for this program.
- **Branch `origin/refactor/lez-v0.4.x`** (not merged to `main`) is explicitly the LEZ 0.4 actor
  toolchain. Its `README.md` says: "The client toolchain for programs on LEZ's actor model... On
  LEZ's actor model a program is written against `lee_core` directly: an actor is an (account,
  program) pair... SPEL no longer provides a guest framework; the `#[lez_program]` macros, `spel
  init` scaffolding and instruction-style transactions were retired". Its
  `docs/reference/idl.md` defines IDL **v2**: `version: "2"`, `messages` with positional Borsh
  `tag`s, `state`, `events` with `sha256("<name>::<event>")[..8]` selectors, and a self-contained
  `types` array. The CLI on that branch refuses a v1 IDL.
- Generation: `spel generate-idl <CRATE_DIR> --message <ENUM> [--state <TYPE>] [--events <MODULE>]`
  (`docs/reference/cli.md:281-302`). It parses the core crate with `syn`, needs no annotations,
  and reads types across path dependencies. Golden files: `spel-framework-core/tests/fixtures/
  golden/{token,nft}.json` on that branch.

## Why `idl/vesting.json` is hand-written rather than generated

1. `spel generate-idl` parses a crate with `syn` and needs the message enum, state type and event
   module in one core crate. The program's messages are in `program/` (`Instruction`, `NewSchedule`,
   `Op`, `Pending`, `ScheduleState`, `VestingEvent`), but its events and schedule state are
   `vesting-core`'s types (`Event`, `Schedule`), and the generator's event rule expects one event
   type per selector rather than a wrapper carrying a core enum. A generated file would need
   hand edits anyway.
2. The 0.4 branch's `spel-client-gen` is excluded from its workspace with the comment "Parked until
   `spel send` works on LEZ's actor model: it generates clients for instruction account lists,
   which actor programs no longer have" (`Cargo.toml` on `refactor/lez-v0.4.x`). So even a
   generated IDL cannot yet produce the typed client a full SPEL integration would want.

The hand-written IDL is therefore checked against the program itself: `idl-check` builds every
message, both shapes of the state and every event with the program's own types, re-encodes the
same values from the IDL's types with an independent small Borsh encoder, and asserts the bytes
are identical; it also checks the message tags and each event's selector against the program's.

## Exact gaps to raise upstream (draft issue — NOT posted)

> **Title:** Please land/mark the LEZ 0.4 (actor-model) IDL v2 work on `main`
>
> We are an RFP-017 team writing an IDL for a vesting program on LEZ testnet 0.4 (`bafd07be`,
> actor model). The default `main` branch documents only the retired `#[lez_program]` /
> instruction-style IDL v1, which cannot express actor messages, state or event selectors. The
> `refactor/lez-v0.4.x` branch has IDL v2 and the right generator, but it is not on `main` and its
> `spel-client-gen` is parked.
>
> Questions:
> 1. Is `refactor/lez-v0.4.x` the supported branch for 0.4/actor programs, and what is its
>    stability commitment? Could its IDL v2 docs be merged to `main` and the v1 docs marked legacy?
> 2. `spel generate-idl` requires the message enum in a core crate in the same dependency graph.
>    Can it generate from a crate that only re-exports the enum (our events wrap
>    another crate's enum), or from a `--message-path <file>`?
> 3. `spel-client-gen` is excluded from that branch. What replaces typed client generation for
>    actor messages, and is there a timeline?
> 4. The `events` selector rule `sha256("<namespace>::<event>")[..8]` is documented; is there a
>    helper the generator/CLI exposes to compute and verify it from a program's `Event` enum?
>
> Our current workaround is a hand-authored v2 IDL plus a byte-level test against the program's
> Borsh encoding. References: `docs/reference/idl.md` on `main` vs `refactor/lez-v0.4.x`;
> the parked `spel-client-gen` note in that branch's `Cargo.toml`.
