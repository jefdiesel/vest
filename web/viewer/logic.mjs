// Pure logic for the vesting viewer: borsh decoding of on-chain state/events, the vesting maths
// (BigInt, floor, identical to vesting-core), next unlock, and input classification.
// No DOM, no chain calls. Node-testable against recorded fixtures.
// SPDX-License-Identifier: MIT OR Apache-2.0

import { Reader, base58Decode, base58Encode, pad32, sha256, utf8 } from './logos-sdk/util.js';

export const DEFAULT_PROGRAM = '2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2';
const SEED_TAG = utf8.encode('/RFP017/vesting/schedule/v1');
const PDA_PREFIX = pad32('/LEE/v0.2/AccountId/PDA/');
const MAX_TRANCHES = 64;

/** S = PDA(V, SHA256(SEED_TAG | creator | nonce)) — program/src/lib.rs schedule_address. */
export async function scheduleAddress(program, creator, nonce) {
  const programBytes = base58Decode(program);
  const creatorBytes = base58Decode(creator);
  if (!programBytes || programBytes.length !== 32) throw new Error('bad program id');
  if (!creatorBytes || creatorBytes.length !== 32) throw new Error('bad creator account');
  const nonceBytes = typeof nonce === 'string' ? utf8.encode(nonce).subarray(0, 32) : nonce;
  const nonce32 = new Uint8Array(32);
  nonce32.set(nonceBytes.subarray(0, 32));
  const seed = await sha256(SEED_TAG, creatorBytes, nonce32);
  return base58Encode(await sha256(PDA_PREFIX, programBytes, seed));
}

// ── borsh decoding (program/src/lib.rs + core/src/types.rs) ──────────────────────────

function decodeAsset(reader) {
  const tag = reader.u8();
  if (tag === 0) return { kind: 'native' };
  if (tag === 1) return { kind: 'token', program: reader.id(), definition: reader.hex32() };
  throw new Error(`unknown asset tag ${tag}`);
}

function decodeKind(reader) {
  const tag = reader.u8();
  if (tag === 0) return { kind: 'cliff-linear', start: reader.u64(), cliff: reader.u64(), end: reader.u64() };
  if (tag === 1) return { kind: 'linear', start: reader.u64(), end: reader.u64() };
  if (tag === 2) return { kind: 'milestone', tranches: reader.vec(() => reader.u128(), MAX_TRANCHES) };
  throw new Error(`unknown schedule kind ${tag}`);
}

function decodeSchedule(reader) {
  return {
    creator: reader.id(),
    beneficiary: reader.id(),
    token: reader.hex32(),
    total: reader.u128(),
    claimed: reader.u128(),
    ...decodeKind(reader),
    cancelable: reader.bool(),
    transferable: reader.bool(),
    cancelAuthority: reader.id(),
    milestoneAuthority: reader.id(),
    signalled: reader.vec(() => BigInt(reader.u64()), 4),
    cancelled: reader.option(() => ({ at: BigInt(reader.u64()), vestedAtCancel: reader.u128() })),
    lastTime: BigInt(reader.u64()),
    version: reader.u8(),
  };
}

function skipNewSchedule(reader) {
  reader.take(32); // nonce
  reader.take(32); // beneficiary
  decodeAsset(reader);
  reader.u128(); // total
  decodeKind(reader);
  reader.option(() => reader.bool()); // cancelable
  reader.option(() => reader.bool()); // transferable
  reader.option(() => reader.take(32)); // cancel authority
  reader.option(() => reader.take(32)); // milestone authority
  reader.option(() => { reader.u128(); reader.u128(); }); // sponsor
}

function skipOp(reader) {
  const tag = reader.u8();
  if (tag === 0) { reader.take(32); reader.bool(); return; } // Claim
  if (tag === 1 || tag === 2) return; // Cancel / MakeIrrevocable
  if (tag === 3) { reader.u32(); return; } // SignalMilestone
  if (tag === 4) { reader.take(32); return; } // TransferBeneficiary
  throw new Error(`unknown op tag ${tag}`);
}

function skipPending(reader) {
  const tag = reader.u8();
  if (tag === 0) { reader.take(32); skipNewSchedule(reader); return; } // Create
  if (tag === 1) { reader.take(32); skipOp(reader); return; } // Op
  throw new Error(`unknown pending tag ${tag}`);
}

/** Decode the whole state at (S, V): ScheduleState from program/src/lib.rs. */
export function decodeScheduleState(bytes) {
  const reader = new Reader(bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes));
  const version = reader.u8();
  const nonce = reader.hex32();
  const asset = decodeAsset(reader);
  const schedule = reader.option(() => decodeSchedule(reader));
  const float = reader.u128();
  const perClaim = reader.u128();
  const hasPending = reader.u8();
  if (hasPending > 1) throw new Error('bad pending tag');
  if (hasPending) skipPending(reader);
  reader.expectEnd();
  return { version, nonce, asset, schedule, float, perClaim };
}

function decodeCoreEvent(reader) {
  const tag = reader.u8();
  if (tag === 0) {
    const value = {
      name: 'Created', creator: reader.id(), beneficiary: reader.id(), token: reader.hex32(),
      total: reader.u128(), ...decodeKind(reader), cancelable: reader.bool(), transferable: reader.bool(),
      cancelAuthority: reader.id(), milestoneAuthority: reader.id(), at: reader.u64(),
    };
    return value;
  }
  if (tag === 1) return { name: 'Claimed', amount: reader.u128(), toPrivate: reader.bool(), claimedTotal: reader.u128(), at: reader.u64() };
  if (tag === 2) return { name: 'Cancelled', vestedAtCancel: reader.u128(), refund: reader.u128(), at: reader.u64() };
  if (tag === 3) {
    const index = Number(reader.u8()) | (Number(reader.u8()) << 8); // u16 LE
    return { name: 'MilestoneSignalled', index, amount: reader.u128(), at: reader.u64() };
  }
  if (tag === 4) return { name: 'BeneficiaryTransferred', old: reader.id(), new: reader.id(), at: reader.u64() };
  if (tag === 5) return { name: 'MadeIrrevocable', at: reader.u64() };
  throw new Error(`unknown event tag ${tag}`);
}

/** A VestingEvent record from a LEZ event log: schedule account + core::Event. */
export function decodeVestingEvent(bytes) {
  const reader = new Reader(bytes instanceof Uint8Array ? bytes : Uint8Array.from(bytes));
  const schedule = reader.id();
  const event = decodeCoreEvent(reader);
  reader.expectEnd();
  return { schedule, event };
}

// ── vesting maths: exact BigInt floor, same as vesting-core ──────────────────────────

function linearVested(total, start, end, at) {
  if (at <= start) return 0n;
  if (at >= end) return total;
  return (total * (at - start)) / (end - start);
}

export function vestedAt(schedule, now) {
  if (!schedule) return 0n;
  if (schedule.cancelled) return schedule.cancelled.vestedAtCancel;
  const at = BigInt(now) > schedule.lastTime ? BigInt(now) : schedule.lastTime;
  if (schedule.kind === 'milestone') {
    let vested = 0n;
    schedule.tranches.forEach((amount, index) => {
      const word = Math.floor(index / 64), bit = BigInt(index % 64);
      if ((schedule.signalled[word] ?? 0n) & (1n << bit)) vested += amount;
    });
    return vested;
  }
  if (schedule.kind === 'cliff-linear' && at < BigInt(schedule.cliff)) return 0n;
  return linearVested(schedule.total, BigInt(schedule.start), BigInt(schedule.end), at);
}

export function claimableAt(schedule, now) {
  const vested = vestedAt(schedule, now);
  return vested > schedule.claimed ? vested - schedule.claimed : 0n;
}

export function lockedAt(schedule, now) {
  const vested = vestedAt(schedule, now);
  return vested < schedule.total ? schedule.total - vested : 0n;
}

/** Next unlock: { at: bigint|null, text } — null when nothing more will unlock. */
export function nextUnlock(schedule, now) {
  if (!schedule || schedule.cancelled) return { at: null, text: 'cancelled — nothing further vests' };
  if (schedule.kind === 'milestone') {
    for (let index = 0; index < schedule.tranches.length; index += 1) {
      const word = Math.floor(index / 64), bit = BigInt(index % 64);
      if (!((schedule.signalled[word] ?? 0n) & (1n << bit))) {
        return { at: null, text: `milestone ${index + 1} awaits the authority's signal` };
      }
    }
    return { at: null, text: 'all milestones signalled' };
  }
  const nowBig = BigInt(now);
  if (schedule.kind === 'cliff-linear' && nowBig < BigInt(schedule.cliff)) {
    return { at: BigInt(schedule.cliff), text: 'cliff' };
  }
  if (nowBig >= BigInt(schedule.end)) return { at: null, text: 'fully vested' };
  // claimable_at is monotone: binary search the first time it becomes positive (as core does).
  let low = nowBig + 1n, high = BigInt(schedule.end);
  while (low < high) {
    const mid = low + (high - low) / 2n;
    if (claimableAt(schedule, mid) > 0n) high = mid; else low = mid + 1n;
  }
  return { at: low, text: 'linear accrual' };
}

// ── presentation ─────────────────────────────────────────────────────────────────────

export function grouped(value) {
  const text = String(value);
  return /^\d+$/.test(text) ? text.replace(/\B(?=(\d{3})+(?!\d))/g, ',') : text;
}

export function amountText(value) { return `${grouped(value)} LEZ`; }

export function describeSchedule(schedule, now) {
  if (!schedule) return null;
  const next = nextUnlock(schedule, now);
  return {
    kind: schedule.kind,
    total: amountText(schedule.total),
    vested: amountText(vestedAt(schedule, now)),
    claimed: amountText(schedule.claimed),
    claimable: amountText(claimableAt(schedule, now)),
    locked: amountText(lockedAt(schedule, now)),
    nextUnlock: next.at === null ? next.text : `${next.text} at ${new Date(Number(next.at)).toISOString()}`,
    cancelled: schedule.cancelled ? `yes at ${new Date(Number(schedule.cancelled.at)).toISOString()} (${amountText(schedule.cancelled.vestedAtCancel)} still claimable)` : 'no',
    transferable: schedule.transferable ? 'yes' : 'no',
    creator: schedule.creator,
    beneficiary: schedule.beneficiary,
    lastTime: new Date(Number(schedule.lastTime)).toISOString(),
  };
}

/** What the address box means. A name is lowercase letters/digits/hyphens, like LNS. */
export function classifyInput(text) {
  const value = String(text ?? '').trim();
  if (/^[a-z0-9-]{1,32}$/.test(value)) return { type: 'name', value };
  if (base58Decode(value)?.length === 32) return { type: 'account', value };
  throw new Error('enter a 32-byte account id (base58) or an LNS name');
}
