// Logos Names (LNS v5) read SDK: derive a name's identifier/key locally, resolve names and deeds
// over LEZ JSON-RPC, open sealed records with WebCrypto. Ported from the Names resolver package
// (@lns/resolver, the logos-names resolver), read-only.
// The only external piece is vendored @noble/hashes Argon2id (MIT, see vendor/argon2id.js).
// SPDX-License-Identifier: MIT OR Apache-2.0

import { argon2idAsync } from './argon2id.js';
import { base58Decode, base58Encode, concat, fromHex, pad32, Reader, sha256, toHex, utf8 } from './util.js';
import { derivePda } from './accounts.js';

const DERIVE_SALT = 'logos-names/v5';
// Argon2id v0x13: 64 MiB, 3 passes, 1 lane, 32-byte output. ~0.5 s in pure JS, by design.
const DERIVE_PARAMS = { t: 3, m: 65536, p: 1, dkLen: 32, version: 0x13, asyncTick: 50 };
const ID_DOMAIN = 'logos-names/v5/id';
const KEY_DOMAIN = 'logos-names/v5/key';
const NAME_SEED_DOMAIN = 'logos-names/v5/name/';
const PAGE_SEED_DOMAIN = 'logos-names/v5/page/';
const DEED_DEFINITION_DOMAIN = 'lns-deed/v1/definition/';
const DEED_HOLDING_DOMAIN = 'lns-deed/v1/holding/';
const DEED_NFT_PROGRAM = 'CjrUpegdA1RjqDN43T8P19kAkwnD1quAkNkER8k3dw9G';
const DEED_VERSION = 7;
const ASSOCIATED_DATA = { record: 'logos-names/v5/record', page: 'logos-names/v5/page' };
const NONCE_LENGTH = 12;
const NAME_PATTERN = /^(?!-)[a-z0-9-]{1,32}(?<!-)$/;
export const GRACE_MS = 2_592_000_000; // 30 days
export const LEGACY_SUFFIX = '.lez';
export const FORWARD_KEY = 'forward';
export { DEED_NFT_PROGRAM };

export function normalizeName(input) {
  const lowered = String(input).trim().replace(/[A-Z]/g, (letter) => letter.toLowerCase());
  let bare = lowered.startsWith('lns:') ? lowered.slice(4) : lowered;
  if (bare.startsWith('@')) bare = bare.slice(1);
  return bare.endsWith(LEGACY_SUFFIX) ? bare.slice(0, -LEGACY_SUFFIX.length) : bare;
}
export const isValidName = (name) => typeof name === 'string' && NAME_PATTERN.test(name);

/** The chain-only identifier (id) and the sealing key (key). The key is never sent anywhere. */
export async function deriveName(name) {
  if (!isValidName(name)) throw new Error(`invalid name: ${name}`);
  const master = await argon2idAsync(utf8.encode(name), utf8.encode(DERIVE_SALT), DERIVE_PARAMS);
  return {
    id: await sha256(utf8.encode(ID_DOMAIN), master),
    key: await sha256(utf8.encode(KEY_DOMAIN), master),
  };
}

async function pdaFromSeed(program, seed) {
  if (seed.length !== 32) throw new Error('a name seed is 32 bytes');
  return derivePda(program, seed);
}
async function derivedAddress(program, domain, id) {
  if (!(id instanceof Uint8Array) || id.length !== 32) throw new Error('a name id is 32 bytes');
  return pdaFromSeed(program, await sha256(utf8.encode(domain), id));
}
export const recordAddress = (program, id) => derivedAddress(program, NAME_SEED_DOMAIN, id);
export const pageAddress = (program, id) => derivedAddress(program, PAGE_SEED_DOMAIN, id);

export async function deedAccounts(program, id, ownerTag, nonce) {
  const nonceBytes = fromHex(nonce);
  if (nonceBytes.length !== 32) throw new Error('a deed nonce is 32 bytes');
  const definitionSeed = await sha256(utf8.encode(DEED_DEFINITION_DOMAIN), id, nonceBytes);
  const definition = await pdaFromSeed(program, definitionSeed);
  const holdingSeed = await sha256(
    utf8.encode(DEED_HOLDING_DOMAIN),
    base58Decode(definition),
    fromHex(ownerTag),
  );
  return { definition, holding: await pdaFromSeed(program, holdingSeed), nonce };
}

/** AES-GCM seal/open with a name's key, bound to its kind. WebCrypto only. */
async function sealingKey(key, usage) {
  return globalThis.crypto.subtle.importKey('raw', key, 'AES-GCM', false, [usage]);
}
function sealingParams(nonce, kind) {
  const associated = ASSOCIATED_DATA[kind];
  if (!associated) throw new Error(`unknown kind of sealed data: ${kind}`);
  return { name: 'AES-GCM', iv: nonce, additionalData: utf8.encode(associated) };
}
export async function seal(key, plaintext, kind, nonce = null) {
  if (!ASSOCIATED_DATA[kind]) throw new Error(`unknown kind of sealed data: ${kind}`);
  const iv = nonce ?? globalThis.crypto.getRandomValues(new Uint8Array(NONCE_LENGTH));
  if (iv.length !== NONCE_LENGTH) throw new Error('a nonce is 12 bytes');
  const params = sealingParams(iv, kind);
  const encrypted = await globalThis.crypto.subtle.encrypt(params, await sealingKey(key, 'encrypt'), plaintext);
  return concat(iv, new Uint8Array(encrypted));
}

export async function openSealed(key, sealed, kind) {
  if (sealed.length < NONCE_LENGTH + 16) return null;
  try {
    const params = sealingParams(sealed.subarray(0, NONCE_LENGTH), kind);
    const opened = await globalThis.crypto.subtle.decrypt(
      params, await sealingKey(key, 'decrypt'), sealed.subarray(NONCE_LENGTH),
    );
    return new Uint8Array(opened);
  } catch { return null; }
}

export async function ownerTag(id, accountId) {
  return toHex(await sha256(utf8.encode('logos-names/v5/owner'), id, base58Decode(accountId)));
}

export function decodeNameRecord(bytes) {
  const reader = new Reader(bytes);
  const version = reader.u8();
  return {
    version,
    ownerTag: toHex(reader.take(32)),
    expires: reader.u64(),
    sealed: reader.bytes(),
    claimed: version >= 6 ? reader.bool() : false,
    deed: version >= DEED_VERSION
      ? (() => {
        const tag = reader.u8();
        if (tag === 0) return null;
        if (tag !== 1) throw new Error("the record's deed is unreadable");
        return { definition: reader.id(), nonce: toHex(reader.take(32)) };
      })()
      : null,
  };
}
export function decodePrimaryName(bytes) {
  const reader = new Reader(bytes);
  return { version: reader.u8(), name: reader.string() };
}
export function decodePage(bytes) {
  const reader = new Reader(bytes);
  return { version: reader.u8(), sealed: reader.bytes() };
}
export function decodeProfile(bytes) {
  const reader = new Reader(bytes);
  const profile = {
    name: reader.string(),
    account: reader.option(() => reader.id()),
    npk: reader.option(() => toHex(reader.take(32))),
    vpk: toHex(reader.bytes()) || null,
    texts: {},
  };
  const count = reader.u32();
  for (let i = 0; i < count; i += 1) profile.texts[reader.string()] = reader.string();
  return profile;
}
export function decodePageContent(bytes) {
  const reader = new Reader(bytes);
  return { contentType: reader.string(), body: reader.bytes() };
}

/** The page for `name`, or null when unregistered, has no page, or does not open with its key. */
export async function resolvePage(client, program, name, now = Date.now()) {
  const { id, key } = await deriveName(name);
  if ((await readRecord(client, program, id, now)) === null) return null;
  const bytes = await readActorState(client, program, await pageAddress(program, id));
  if (bytes.length === 0) return null;
  const opened = await openSealed(key, decodePage(bytes).sealed, 'page');
  return opened ? decodePageContent(opened) : null;
}

/** Read one actor's state bytes under a program through the rpc.Client (LEZ getAccountView). */
export async function readActorState(client, program, accountId) {
  const value = await client.call('getAccountView', [{ account_id: accountId, program_account_id: program }]);
  const states = value?.data?.actor_states ?? value?.data?.shards ?? {};
  const bytes = states[program];
  return bytes ? Uint8Array.from(bytes) : new Uint8Array();
}
export const readShard = readActorState;

/** The name `accountId` is shown as, or null. A reverse record alone is not enough: the forward
 * record must point back at the same account (or the owner tag must match). */
export async function reverseLookup(client, program, accountId, now = Date.now()) {
  const bytes = await readActorState(client, program, accountId);
  if (bytes.length === 0) return null;
  const { name } = decodePrimaryName(bytes);
  if (!isValidName(name)) return null;
  const { id, key } = await deriveName(name);
  const record = await readRecord(client, program, id, now);
  if (record === null) return null;
  if (record.ownerTag === (await ownerTag(id, accountId))) return name;
  const profile = await openSealed(key, record.sealed, 'record');
  const decoded = profile ? decodeProfile(profile) : null;
  return decoded && decoded.name === name && decoded.account === accountId ? name : null;
}

async function readRecord(client, program, id, now) {
  const bytes = await readActorState(client, program, await recordAddress(program, id));
  if (bytes.length === 0) return null;
  const record = decodeNameRecord(bytes);
  return now >= record.expires + GRACE_MS ? null : record;
}

/** What `name` resolves to at `now`, or null when unregistered/past grace. */
export async function resolveName(client, program, name, now = Date.now()) {
  const { id, key } = await deriveName(name);
  const record = await readRecord(client, program, id, now);
  if (record === null) return null;
  const deed = record.deed === null
    ? null
    : await deedAccounts(program, id, record.ownerTag, record.deed.nonce);
  const expires = record.expires >= Number.MAX_SAFE_INTEGER ? null : record.expires;
  const held = { name, expires, lapsed: expires !== null && expires <= now };
  const profile = await openSealed(key, record.sealed, 'record');
  const decoded = profile ? decodeProfile(profile) : null;
  if (!decoded || decoded.name !== name) return { ...held, account: null, npk: null, vpk: null, texts: {}, deed };
  return { ...held, account: decoded.account, npk: decoded.npk, vpk: decoded.vpk, texts: decoded.texts, deed };
}

export function forwardOf(record) {
  const value = record?.texts?.[FORWARD_KEY];
  if (typeof value !== 'string') return null;
  const target = normalizeName(value);
  return isValidName(target) && target !== record.name ? target : null;
}

export async function resolveUsing(client, program, name, now = Date.now()) {
  const record = await resolveName(client, program, name, now);
  const target = forwardOf(record);
  if (target === null) return record;
  const forwarded = await resolveName(client, program, target, now);
  if (forwarded === null) throw new Error(`${name} points to ${target}, which isn't registered`);
  return { ...forwarded, forwardedFrom: name };
}

/** Public-facing view: never exposes private receiving keys. */
export function publicView(record) {
  if (!record) return null;
  const { name, expires, lapsed, account, npk, vpk, texts } = record;
  return { name, expires, lapsed, payablePrivately: Boolean(npk && vpk), publicAccount: account, texts };
}

/** Whether the deed's holding account really holds the name's NFT master/copy. */
export async function readDeed(client, deed) {
  const fallback = { held: false, kind: null, definition: deed.definition };
  const bytes = await readActorState(client, DEED_NFT_PROGRAM, deed.holding);
  if (bytes.length === 0) return fallback;
  try {
    const reader = new Reader(bytes);
    const tag = reader.u8();
    if (tag !== 1 && tag !== 2) return fallback;
    const definition = reader.id();
    const held = tag === 1 ? reader.take(16).some((byte) => byte !== 0) : reader.bool();
    const kind = tag === 1 ? 'master' : 'copy';
    reader.expectEnd();
    if (definition !== deed.definition) return { held: false, kind: null, definition };
    return { held, kind: held ? kind : null, definition };
  } catch { return fallback; }
}
