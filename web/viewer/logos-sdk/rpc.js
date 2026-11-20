// The single RPC door: fetch-based JSON-RPC for LEZ, with retries, 429 pacing and batch fallback.
// Works in browsers and Node 20+ (fetch, AbortController). Zero runtime dependencies.
// SPDX-License-Identifier: MIT OR Apache-2.0

const RETRYABLE_CODES = new Set([-32005]); // LEZ's rate-limited code (Names Wallet rpc.rs)
const BATCH_FALLBACK_STATUS = new Set([400, 403, 405, 413]);
const DEFAULT_BACKOFF_MS = [2000, 4000, 10000];

export class RpcError extends Error {
  constructor(message, { code = null, status = null, retryAfterMs = null } = {}) {
    super(message);
    this.name = 'RpcError';
    this.code = code;
    this.status = status;
    this.retryAfterMs = retryAfterMs;
  }
  get retryable() {
    return this.status === 429 || RETRYABLE_CODES.has(this.code) || this.status >= 500;
  }
}

function retryAfter(header) {
  if (!header) return null;
  const seconds = Number(header);
  if (Number.isFinite(seconds)) return Math.max(0, seconds * 1000);
  const date = Date.parse(header);
  return Number.isNaN(date) ? null : Math.max(0, date - Date.now());
}

function isLoopback(url) {
  try {
    const host = new URL(url).hostname.toLowerCase();
    return host === 'localhost' || host === '127.0.0.1' || host === '[::1]' || host === '::1';
  } catch { return false; }
}

/** A fetch-based LEZ sequencer client. Inject `fetch`/`sleep`/`now` for deterministic tests. */
export class Client {
  constructor({
    url,
    fetch: fetchImpl = globalThis.fetch?.bind(globalThis),
    timeoutMs = 20_000,
    retries = 3,
    ratePerSec = 4,
    burst = 8,
    sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    now = () => Date.now(),
  } = {}) {
    if (!url) throw new Error('rpc Client needs a url');
    if (!fetchImpl) throw new Error('fetch is required (Node 20+ or a browser)');
    this.url = url;
    this.fetch = fetchImpl;
    this.timeoutMs = timeoutMs;
    this.retries = retries;
    this.ratePerSec = ratePerSec;
    this.burst = burst;
    this.sleep = sleep;
    this.now = now;
    this.local = isLoopback(url);
    this.tokens = burst;
    this.lastRefill = now();
  }

  async _pace() {
    if (this.local) return; // Names Wallet and the registry SDK both skip loopback
    const current = this.now();
    const elapsed = Math.max(0, current - this.lastRefill);
    this.tokens = Math.min(this.burst, this.tokens + (elapsed / 1000) * this.ratePerSec);
    this.lastRefill = current;
    if (this.tokens < 1) {
      await this.sleep(Math.ceil(((1 - this.tokens) / this.ratePerSec) * 1000));
      this.tokens = 1;
      this.lastRefill = this.now();
    }
    this.tokens -= 1;
  }

  async _post(body) {
    await this._pace();
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const response = await this.fetch(this.url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
        redirect: 'error',
        referrerPolicy: 'no-referrer',
        signal: controller.signal,
      });
      const header = response.headers?.get?.('retry-after') ?? null;
      if (response.status === 429) {
        throw new RpcError('rate limited by the sequencer', { status: 429, retryAfterMs: retryAfter(header) });
      }
      let reply;
      try { reply = await response.json(); } catch { throw new RpcError('bad JSON from the sequencer', { status: response.status }); }
      if (reply.error) {
        throw new RpcError(`${body.method ?? 'batch'}: ${reply.error.message ?? JSON.stringify(reply.error)}`, {
          code: reply.error.code ?? null,
          status: response.status,
        });
      }
      if (!response.ok) throw new RpcError(`sequencer returned HTTP ${response.status}`, { status: response.status });
      return reply.result;
    } catch (error) {
      if (error?.name === 'AbortError') throw new RpcError('the sequencer did not answer in time');
      throw error;
    } finally {
      clearTimeout(timer);
    }
  }

  /** One JSON-RPC call with bounded retries; honours Retry-After and typed backoff. */
  async call(method, params = []) {
    let last;
    for (let attempt = 0; attempt <= this.retries; attempt += 1) {
      try {
        return await this._post({ jsonrpc: '2.0', id: 1, method, params });
      } catch (error) {
        last = error;
        if (attempt === this.retries || !(error instanceof RpcError) || !error.retryable) break;
        const wait = error.retryAfterMs ?? DEFAULT_BACKOFF_MS[Math.min(attempt, DEFAULT_BACKOFF_MS.length - 1)];
        await this.sleep(wait);
      }
    }
    throw last;
  }

  /** Batch of [method, params] pairs; falls back to single calls on endpoints that refuse batches. */
  async batch(calls) {
    if (!Array.isArray(calls) || calls.length === 0) return [];
    try {
      const body = calls.map(([method, params], index) => ({ jsonrpc: '2.0', id: index + 1, method, params }));
      const results = await this._post(body);
      if (!Array.isArray(results)) throw new RpcError('batch reply is not an array');
      return results;
    } catch (error) {
      if (!(error instanceof RpcError) || !BATCH_FALLBACK_STATUS.has(error.status)) throw error;
      const out = [];
      for (const [method, params] of calls) out.push(await this.call(method, params));
      return out;
    }
  }

  getAccount(account) { return this.call('getAccount', [account]); }
  getAccountView(account, program) {
    return this.call('getAccountView', [{ account_id: account, program_account_id: program }]);
  }
  getBalance(account) {
    return this.call('getAccountBalance', [account]).then((value) => BigInt(String(value ?? 0)));
  }
  getBlockRange(range) { return this.call('getBlockRange', [range]); }
  getTransaction(hash) { return this.call('getTransaction', [hash]); }
  getFeeState() { return this.call('getFeeState', []); }
  getProgramIds() { return this.call('getProgramIds', []); }
  sendTransaction(tx) { return this.call('sendTransaction', [tx]); }
}
