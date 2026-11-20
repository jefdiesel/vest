// Regenerates the recorded borsh fixtures with a tiny writer. No chain access.
// Run: node web/viewer/test/fixtures/generate.mjs
import { writeFileSync } from 'node:fs';

const u8 = (n) => Uint8Array.of(n & 0xff);
const u16 = (n) => { const b = new Uint8Array(2); new DataView(b.buffer).setUint16(0, n, true); return b; };
const u32 = (n) => { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, n, true); return b; };
const u64 = (n) => { const b = new Uint8Array(8); new DataView(b.buffer).setBigUint64(0, BigInt(n), true); return b; };
const u128 = (n) => {
  const b = new Uint8Array(16);
  new DataView(b.buffer).setBigUint64(0, BigInt(n) & 0xffffffffffffffffn, true);
  new DataView(b.buffer).setBigUint64(8, BigInt(n) >> 64n, true);
  return b;
};
const concat = (...parts) => new Uint8Array(parts.flatMap((part) => Array.from(part)));
const vec = (...items) => concat(u32(items.length), ...items);
const option = (value) => value ? concat(u8(1), value) : u8(0);
const fill = (n, byte) => new Uint8Array(n).fill(byte);
const b64 = (bytes) => Buffer.from(bytes).toString('base64');

const nativeAsset = u8(0);
const kind = {
  cliff: concat(u8(0), u64(0), u64(400), u64(1000)),
  linear: concat(u8(1), u64(0), u64(1000)),
  milestone: concat(u8(2), vec(u128(400), u128(600))),
};

function schedule({ total, claimed, kindBytes, signalled = [], cancelled = null, lastTime = 0, creator = 1, beneficiary = 2 }) {
  return concat(
    fill(32, creator), fill(32, beneficiary), fill(32, 0),
    u128(total), u128(claimed), kindBytes,
    u8(1), u8(0), // cancelable, transferable
    fill(32, creator), fill(32, creator), // authorities
    vec(...signalled.map((word) => u64(word))),
    option(cancelled),
    u64(lastTime), u8(1),
  );
}

function state(inner, { nonce = 1, asset = nativeAsset, float = 0, perClaim = 0 } = {}) {
  return concat(u8(1), fill(32, nonce), asset, option(inner), u128(float), u128(perClaim), u8(0));
}

const cliffState = state(schedule({ total: 1000, claimed: 100, kindBytes: kind.cliff }));
const linearState = state(schedule({ total: 1000, claimed: 0, kindBytes: kind.linear, lastTime: 500 }));
const milestoneState = state(schedule({ total: 1000, claimed: 0, kindBytes: kind.milestone, signalled: [1] }));
const cancelledState = state(schedule({
  total: 1000, claimed: 250, kindBytes: kind.cliff,
  cancelled: concat(u64(600), u128(500)),
}));

// A Claimed event record: schedule account (32 bytes of 9) + core::Event variant 1.
const claimedEvent = concat(fill(32, 9), u8(1), u128(200), u8(1), u128(400), u64(700));

writeFileSync(new URL('./state.json', import.meta.url), JSON.stringify({
  cliff: b64(cliffState), linear: b64(linearState), milestone: b64(milestoneState),
  cancelled: b64(cancelledState), event: b64(claimedEvent),
}, null, 2) + '\n');
console.log('fixtures written');
