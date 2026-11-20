// Account helpers: ids from public keys, base58 formatting, PDA derivation and known programs.
// Follows lee_core::account (LEZ rc3) and the explorer's decode.js. Zero runtime dependencies.
// SPDX-License-Identifier: MIT OR Apache-2.0

import { base58Decode, base58Encode, concat, fromHex, pad32, sha256, utf8 } from './util.js';

/** AccountId [0; 32]: the native coin program. */
export const NATIVE_PROGRAM = '11111111111111111111111111111111';
/** AccountId [0xFE; 32]: the program loader. */
export const LOADER_PROGRAM = 'JAQ5MVHbCkSYRzXunsrNuM2m1LS859PGveHfoYPAmcvZ';
export const CLOCK_PROGRAM = '2hwaRW8sMnyKLLRbDRw8k68tiv9bnfq98oxcdtcp157Z';
export const REGISTRY_PROGRAM = '7sGWX1JTYoD2GFs69sWdX9PqJzhpcdk8ouZBu3BYeY82';

const PUBLIC_ID_PREFIX = pad32('/LEE/v0.3/AccountId/Public/');
const PDA_PREFIX = pad32('/LEE/v0.2/AccountId/PDA/');

export const formatAccount = (bytes) => base58Encode(bytes);

export function parseAccount(text) {
  const bytes = base58Decode(String(text).trim());
  if (!bytes || bytes.length !== 32) throw new Error('an account id is 32 base58 bytes');
  return bytes;
}

export const isAccount = (text) => {
  try { parseAccount(text); return true; } catch { return false; }
};

/** The public account id of a signer's 32-byte public key (lee signature/public_key.rs). */
export async function accountOfKey(publicKey) {
  const key = typeof publicKey === 'string' ? fromHex(publicKey) : publicKey;
  if (key.length !== 32) throw new Error('a public key is 32 bytes');
  return base58Encode(await sha256(PUBLIC_ID_PREFIX, key));
}

/** SPEL compute_pda seed helper: UTF-8 zero-padded to 32 bytes. */
export const seedStr = (text) => pad32(text);

/** sha256 of concatenated 32-byte seed components. */
export async function combine(parts) {
  if (!parts.length) throw new Error('a PDA needs at least one component');
  for (const part of parts) if (part.length !== 32) throw new Error('a PDA seed part is not 32 bytes');
  return sha256(...parts);
}

/** `sha256(PDA_PREFIX ‖ program ‖ seed)` → base58 account id (for_public_pda). */
export async function derivePda(program, seed) {
  const programBytes = typeof program === 'string' ? parseAccount(program) : program;
  const seedBytes = typeof seed === 'string' ? utf8.encode(seed) : seed;
  if (seedBytes.length !== 32) throw new Error('a PDA seed is 32 bytes');
  return base58Encode(await sha256(PDA_PREFIX, programBytes, seedBytes));
}

/** The built-in program id from a name: sha256('/LEE-BuiltinProgram/v1/AccountId' ‖ name). */
export async function builtinProgram(name) {
  const prefix = pad32('/LEE-BuiltinProgram/v1/AccountId');
  return base58Encode(await sha256(prefix, utf8.encode(name)));
}

/** Short display form, keeping the prefix and suffix. */
export function shortAccount(text, size = 6) {
  const value = String(text);
  return value.length <= size * 2 + 1 ? value : `${value.slice(0, size)}…${value.slice(-4)}`;
}

export { concat };
