// Read the LEZ clock (block id + timestamp) through the single RPC door.
// ClockAccountData is `{ block_id: u64, timestamp: u64 }` (LEZ rc3 clock_core).
// SPDX-License-Identifier: MIT OR Apache-2.0

import { builtinProgram, CLOCK_PROGRAM } from './accounts.js';
import { decodeBytes, Reader } from './util.js';

export { CLOCK_PROGRAM };
export const clockAccountId = () => builtinProgram('clock');

/** Read the clock. `client` is rpc.Client (or any object with `call`). */
export async function read(client) {
  const account = await clockAccountId();
  const value = await client.call('getAccountView', [account]);
  const bytes = decodeBytes(value, 'clock account view');
  if (bytes.length < 16) throw new Error('clock state is truncated');
  const reader = new Reader(bytes);
  const clock = { blockId: reader.u64(), timestamp: reader.u64() };
  reader.expectEnd();
  return clock;
}

export const nowMs = async (client) => (await read(client)).timestamp;

export function toUtc(milliseconds) {
  const date = new Date(milliseconds);
  return Number.isNaN(date.getTime()) ? `${milliseconds} ms (out of range)` : date.toISOString();
}

/** A private claim "as of" a named time; rejects a time in the chain's future. */
export class AsOfClaim {
  constructor(at) { this.at = BigInt(at); }
  validate(chainNow) {
    if (this.at > BigInt(chainNow)) {
      throw new Error(`as-of time ${this.at} is in the future (clock reads ${chainNow})`);
    }
    return this;
  }
}
