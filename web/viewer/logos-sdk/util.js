// Shared byte/encoding/crypto helpers. Zero runtime dependencies: base58, hex and the
// minimal Borsh reader are implemented here; SHA-256 uses WebCrypto (browser + Node 20+).
// SPDX-License-Identifier: MIT OR Apache-2.0

const ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
export const utf8 = new TextEncoder();

export function base58Encode(bytes) {
  let n = 0n;
  for (const b of bytes) n = (n << 8n) | BigInt(b);
  let s = '';
  while (n > 0n) { s = ALPHABET[Number(n % 58n)] + s; n /= 58n; }
  for (const b of bytes) { if (b !== 0) break; s = '1' + s; }
  return s;
}

/** base58 → bytes, or null when a character is not in the alphabet. */
export function base58Decode(text) {
  let n = 0n;
  for (const ch of String(text)) {
    const v = ALPHABET.indexOf(ch);
    if (v < 0) return null;
    n = n * 58n + BigInt(v);
  }
  const bytes = [];
  while (n > 0n) { bytes.unshift(Number(n & 0xffn)); n >>= 8n; }
  for (const ch of String(text)) { if (ch !== '1') break; bytes.unshift(0); }
  return Uint8Array.from(bytes);
}

export const toHex = (bytes) => Array.from(bytes, (x) => x.toString(16).padStart(2, '0')).join('');

export function fromHex(text) {
  const clean = String(text).replace(/^0x/, '');
  if (clean.length % 2) throw new Error('hex string has an odd length');
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    const byte = Number.parseInt(clean.slice(i * 2, i * 2 + 2), 16);
    if (Number.isNaN(byte)) throw new Error('bad hex string');
    out[i] = byte;
  }
  return out;
}

export function fromBase64(text) {
  const binary = globalThis.atob(text);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) out[i] = binary.charCodeAt(i);
  return out;
}

export function toBase64(bytes) {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return globalThis.btoa(binary);
}

function subtle() {
  if (!globalThis.crypto?.subtle) {
    throw new Error('WebCrypto is required (Node 20+ or a browser with crypto.subtle)');
  }
  return globalThis.crypto.subtle;
}

export async function sha256(...parts) {
  const all = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let offset = 0;
  for (const part of parts) { all.set(part, offset); offset += part.length; }
  return new Uint8Array(await subtle().digest('SHA-256', all));
}

export function concat(...parts) {
  const out = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let offset = 0;
  for (const part of parts) { out.set(part, offset); offset += part.length; }
  return out;
}

/** UTF-8 text right-padded with zeroes to 32 bytes (LEZ's pad32 string seed). */
export function pad32(text) {
  const bytes = new Uint8Array(32);
  bytes.set(utf8.encode(text).subarray(0, 32));
  return bytes;
}

/** A little-endian unsigned integer from a byte array (u128 → BigInt). */
export function leUint(bytes, offset = 0, length = bytes.length) {
  let value = 0n;
  for (let i = offset + length - 1; i >= offset; i -= 1) value = (value << 8n) | BigInt(bytes[i]);
  return value;
}

/** getAccountView replies carry bytes as base64 text or as an array of byte numbers. */
export function decodeBytes(value, what = 'account view') {
  if (typeof value === 'string') return fromBase64(value);
  if (Array.isArray(value)) {
    if (value.some((byte) => !Number.isInteger(byte) || byte < 0 || byte > 255)) {
      throw new Error(`bad bytes in ${what}`);
    }
    return Uint8Array.from(value);
  }
  if (value instanceof Uint8Array) return value;
  throw new Error(`unexpected ${what} shape`);
}

/** Minimal Borsh reader. Fails closed: short data, unknown tags and trailing bytes throw. */
export class Reader {
  constructor(bytes) { this.b = bytes; this.offset = 0; }
  take(length) {
    if (!Number.isInteger(length) || length < 0 || length > this.b.length - this.offset) {
      throw new Error('the data is shorter than its layout');
    }
    const slice = this.b.subarray(this.offset, this.offset + length);
    this.offset += length;
    return slice;
  }
  u8() { return this.take(1)[0]; }
  u32() { const [a, b, c, d] = this.take(4); return (a | (b << 8) | (c << 16) | (d << 24)) >>> 0; }
  u64() { const low = this.u32(), high = this.u32(); return high * 2 ** 32 + low; }
  u128() { return Array.from(this.take(16)).reduceRight((v, x) => (v << 8n) | BigInt(x), 0n); }
  bool() { const v = this.u8(); if (v > 1) throw new Error('bad boolean'); return v === 1; }
  bytes() { return this.take(this.u32()); }
  string() { return new TextDecoder('utf-8', { fatal: true }).decode(this.bytes()); }
  id() { return base58Encode(this.take(32)); }
  hex32() { return toHex(this.take(32)); }
  option(read) { const tag = this.u8(); if (tag > 1) throw new Error('bad option tag'); return tag ? read() : null; }
  vec(read, max = 1 << 20) {
    const count = this.u32();
    if (count > max) throw new Error('a list is too long');
    const out = [];
    for (let i = 0; i < count; i += 1) out.push(read());
    return out;
  }
  get rest() { return this.b.length - this.offset; }
  expectEnd() { if (this.rest) throw new Error(`${this.rest} bytes too many`); }
}
