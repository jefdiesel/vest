import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { base58Encode, fromBase64 } from '../logos-sdk/util.js';
import {
  DEFAULT_PROGRAM, claimableAt, classifyInput, decodeScheduleState, decodeVestingEvent,
  describeSchedule, grouped, lockedAt, nextUnlock, scheduleAddress, vestedAt,
} from '../logic.mjs';

const fixture = JSON.parse(readFileSync(new URL('./fixtures/state.json', import.meta.url), 'utf8'));
const state = (name) => decodeScheduleState(fromBase64(fixture[name]));

test('decodes ScheduleState and the inner core::Schedule', () => {
  const cliff = state('cliff');
  assert.equal(cliff.version, 1);
  assert.equal(cliff.asset.kind, 'native');
  assert.equal(cliff.schedule.kind, 'cliff-linear');
  assert.equal(cliff.schedule.total, 1000n);
  assert.equal(cliff.schedule.claimed, 100n);
  assert.equal(cliff.schedule.cliff, 400);
  assert.equal(cliff.schedule.end, 1000);
  assert.equal(cliff.float, 0n);
  assert.equal(cliff.perClaim, 0n);
  assert.equal(state('milestone').schedule.kind, 'milestone');
  assert.deepEqual(state('milestone').schedule.tranches, [400n, 600n]);
});

test('vested/claimable/locked use BigInt floor maths matching vesting-core', () => {
  const cliff = state('cliff').schedule;
  assert.equal(vestedAt(cliff, 200), 0n); // nothing before the cliff
  assert.equal(vestedAt(cliff, 400), 400n); // the cliff lump
  assert.equal(vestedAt(cliff, 700), 700n);
  assert.equal(vestedAt(cliff, 1000), 1000n);
  assert.equal(vestedAt(cliff, 5000), 1000n);
  assert.equal(claimableAt(cliff, 700), 600n);
  assert.equal(lockedAt(cliff, 700), 300n);
});

test('the high-water mark in state is respected', () => {
  const linear = state('linear').schedule;
  assert.equal(linear.lastTime, 500n);
  assert.equal(vestedAt(linear, 100), 500n);
  assert.equal(vestedAt(linear, 700), 700n);
});

test('milestone vesting sums only signalled tranches', () => {
  const milestone = state('milestone').schedule;
  assert.equal(vestedAt(milestone, 0), 400n);
  assert.equal(claimableAt(milestone, 0), 400n);
});

test('cancellation freezes vested tokens', () => {
  const cancelled = state('cancelled').schedule;
  assert.equal(cancelled.cancelled.vestedAtCancel, 500n);
  assert.equal(vestedAt(cancelled, 9999), 500n);
  assert.equal(claimableAt(cancelled, 9999), 250n); // 500 vested - 250 claimed
});

test('next unlock text and time', () => {
  const cliff = state('cliff').schedule;
  assert.deepEqual(nextUnlock(cliff, 200), { at: 400n, text: 'cliff' });
  assert.equal(nextUnlock(cliff, 400).at, 401n);
  assert.match(nextUnlock(state('milestone').schedule, 0).text, /milestone 2/);
  assert.equal(nextUnlock(state('cancelled').schedule, 0).at, null);
  assert.equal(describeSchedule(cliff, 700).claimable, '600 LEZ');
  assert.match(describeSchedule(cliff, 700).cancelled, /^no$/);
});

test('decodes a VestingEvent', () => {
  const record = decodeVestingEvent(fromBase64(fixture.event));
  assert.equal(record.schedule, base58Encode(new Uint8Array(32).fill(9)));
  assert.deepEqual(record.event, {
    name: 'Claimed', amount: 200n, toPrivate: true, claimedTotal: 400n, at: 700,
  });
});

test('PDA derivation has a fixed vector', async () => {
  const creator = base58Encode(new Uint8Array(32).fill(1));
  assert.equal(DEFAULT_PROGRAM, '2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2');
  assert.equal(await scheduleAddress(DEFAULT_PROGRAM, creator, 's'), 'DatwjdUNZmUVRzAUvNvUqZ4ar5apmER3GKDdc8B1QqDQ');
});

test('input classification and amount formatting', () => {
  assert.deepEqual(classifyInput('  jef05 '), { type: 'name', value: 'jef05' });
  assert.equal(classifyInput(base58Encode(new Uint8Array(32).fill(1))).type, 'account');
  assert.throws(() => classifyInput('!!'), /account id/);
  assert.equal(grouped(1234567n), '1,234,567');
});
