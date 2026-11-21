// Tests for the Vest SDK a Basecamp app copies in (VestSDK.mjs). Run with:
//   node --test sdk/qml/VestSDK.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createVest, reply, isPrivate, VERSION, MODULE } from './VestSDK.mjs';

const BUILT = {
    program: 'Prog111111111111111111111111111111111',
    accounts_json: '[{"seed":"ab"}]', instruction_hex: '00', summary: 'Create a Vest schedule'
};
const ask = (fn) => new Promise((resolve) => fn(resolve));

function hostFor(answers, options) {
    const o = options || {};
    const calls = [];
    return {
        calls: calls,
        call: function (module, method, args, done, timeout) {
            calls.push({ module: module, method: method, args: args, timeout: timeout });
            if (o.throws) throw new Error('host is gone');
            if (o.noBridge) return false;
            const value = Object.prototype.hasOwnProperty.call(answers, method) ? answers[method] : {};
            if (value && value.__error) done(JSON.stringify({ success: false, error: value.__error }));
            else done(JSON.stringify(JSON.stringify({ success: true, value: value })));
            return true;
        }
    };
}
function walletFor(options) {
    const o = options || {};
    const proposed = [];
    return {
        proposed: proposed,
        proposeCall: function (program, accounts, hex, summary, handlers) {
            proposed.push({ program: program, accounts: accounts, hex: hex, summary: summary });
            if (o.throws) throw new Error('wallet is gone');
            if (o.proposeFailed) { handlers.failed({ error: 'Names Wallet refused the request.' }); return; }
            handlers.sent({ request: 'r1' });
            handlers.done({ state: o.state || 'done', tx: 'abcdef', error: o.error });
        },
        resolve: function (name, done) {
            done(o.resolved || { ok: true, name: name, account: 'Pub1111111111111111111111111111111', display: 'alice' });
        }
    };
}

test('exports and the module name', () => {
    assert.equal(VERSION, '1.0.0');
    assert.equal(MODULE, 'vesting');
});

test('reply: encoded once or twice, a refusal and no answer', () => {
    assert.deepEqual(reply('{"success":true,"value":3}'), { ok: true, value: 3 });
    assert.deepEqual(reply(JSON.stringify('{"success":true,"value":4}')), { ok: true, value: 4 });
    assert.equal(reply('{"success":false,"error":"bad id"}').code, 'module_error');
    assert.equal(reply('module not connected: vesting').code, 'no_answer');
    assert.equal(reply(undefined).code, 'no_answer');
});

test('a private destination is recognised', () => {
    assert.equal(isPrivate('Private/abc'), true);
    assert.equal(isPrivate({ account_kind: 'private', account: 'x' }), true);
    assert.equal(isPrivate('Public/abc'), false);
    assert.equal(isPrivate(null), false);
});

test('schedules reads list with no arguments', async () => {
    const h = hostFor({ list: { schedules: [{ id: 1 }], time: 9, names: { a: 'alice' } } });
    const r = await ask((done) => createVest(h, walletFor()).schedules(done));
    assert.equal(r.ok, true);
    assert.deepEqual(r.schedules, [{ id: 1 }]);
    assert.equal(r.time, 9);
    assert.deepEqual(r.names, { a: 'alice' });
    assert.deepEqual(h.calls[0], { module: 'vesting', method: 'list', args: [], timeout: 30000 });
});

test('claimable sends the id and a public destination', async () => {
    const h = hostFor({ pre_claim_summary: { summary: { claimable: '42', claimable_display: '0.000000042 LEZ' } } });
    const r = await ask((done) => createVest(h, walletFor()).claimable(7, done));
    assert.equal(r.ok, true);
    assert.equal(r.claimable, '42');
    assert.equal(r.summary.claimable_display, '0.000000042 LEZ');
    assert.deepEqual(h.calls[0].args, ['7', '']);
});

test('create builds through the module, proposes through the wallet, then reports the status', async () => {
    const h = hostFor({ create_call: BUILT });
    const w = walletFor();
    const row = { kind: 'linear', total: '100' };
    const r = await ask((done) => createVest(h, w).create(row, done));
    assert.equal(r.ok, true);
    assert.equal(r.built.program, BUILT.program);
    assert.equal(r.tx, 'abcdef');
    assert.equal(r.state, 'done');
    assert.deepEqual(h.calls[0].args, [JSON.stringify(row)]);
    assert.equal(h.calls[0].timeout, 120000);
    assert.deepEqual(w.proposed[0], {
        program: BUILT.program, accounts: BUILT.accounts_json, hex: BUILT.instruction_hex, summary: BUILT.summary
    });
});

test('claim uses claim_call and a public destination', async () => {
    const h = hostFor({ claim_call: BUILT });
    const w = walletFor();
    const r = await ask((done) => createVest(h, w).claim(3, 'Public/abc', done));
    assert.equal(r.ok, true);
    assert.equal(h.calls[0].method, 'claim_call');
    assert.deepEqual(h.calls[0].args, ['3']);
    assert.equal(w.proposed.length, 1);
});

test('a private claim is refused before any call', async () => {
    const h = hostFor({ claim_call: BUILT });
    const w = walletFor();
    const r = await ask((done) => createVest(h, w).claim(3, 'Private/abc', done));
    assert.equal(r.ok, false);
    assert.equal(r.code, 'private_not_in_wallet');
    assert.match(r.error, /public account/);
    assert.equal(h.calls.length, 0);
    assert.equal(w.proposed.length, 0);
});

test('cancel, makePermanent, unlockMilestone and transfer use their module methods', async () => {
    const h = hostFor({
        cancel_call: BUILT, irrevocable_call: BUILT, signal_call: BUILT, transfer_call: BUILT
    });
    const v = createVest(h, walletFor());
    const cancel = await ask((done) => v.cancel(4, done));
    const permanent = await ask((done) => v.makePermanent(4, done));
    const unlock = await ask((done) => v.unlockMilestone(4, 2, done));
    const transfer = await ask((done) => v.transfer(4, 'alice', done));
    assert.equal(cancel.ok, true);
    assert.equal(permanent.ok, true);
    assert.equal(unlock.ok, true);
    assert.equal(transfer.ok, true);
    assert.deepEqual(h.calls.map((c) => [c.method, c.args]), [
        ['cancel_call', ['4']], ['irrevocable_call', ['4']],
        ['signal_call', ['4', '2']], ['transfer_call', ['4', 'alice']]
    ]);
});

test('addSchedule gives the local id', async () => {
    const h = hostFor({ import_schedule: { id: 12 } });
    const r = await ask((done) => createVest(h, walletFor()).addSchedule('Sched111', done));
    assert.equal(r.ok, true);
    assert.equal(r.id, 12);
    assert.deepEqual(h.calls[0].args, ['Sched111']);
});

test('resolve is the wallet SDK own answer', async () => {
    const w = walletFor({ resolved: { ok: true, name: 'alice', account: 'Pub1111111111111111111111111111111' } });
    const r = await ask((done) => createVest(hostFor({}), w).resolve('alice', done));
    assert.equal(r.ok, true);
    assert.equal(r.name, 'alice');
});

test('a wallet rejection is a plain error', async () => {
    const w = walletFor({ proposeFailed: true });
    const r = await ask((done) => createVest(hostFor({ cancel_call: BUILT }), w).cancel(1, done));
    assert.equal(r.ok, false);
    assert.equal(r.code, 'wallet_failed');
    assert.match(r.error, /refused/);
});

test('a rejected request is reported by its state', async () => {
    const w = walletFor({ state: 'rejected' });
    const r = await ask((done) => createVest(hostFor({ cancel_call: BUILT }), w).cancel(1, done));
    assert.equal(r.ok, false);
    assert.equal(r.code, 'rejected');
});

test('a module error and a missing module are separate codes', async () => {
    const bad = await ask((done) => createVest(hostFor({ list: { __error: 'bad id' } }), walletFor()).schedules(done));
    assert.equal(bad.ok, false);
    assert.equal(bad.code, 'module_error');
    assert.match(bad.error, /bad id/);
    const missing = await ask((done) => createVest(hostFor({ list: { __error: 'module not connected: vesting' } }), walletFor()).schedules(done));
    assert.equal(missing.code, 'module_missing');
});

test('no bridge, a throwing host and a throwing wallet never throw out of the SDK', async () => {
    const no = await ask((done) => createVest(hostFor({}, { noBridge: true }), walletFor()).schedules(done));
    assert.equal(no.code, 'no_bridge');
    const broken = await ask((done) => createVest(hostFor({}, { throws: true }), walletFor()).schedules(done));
    assert.equal(broken.code, 'module_error');
    const wallet = await ask((done) => createVest(hostFor({ cancel_call: BUILT }), walletFor({ throws: true })).cancel(1, done));
    assert.equal(wallet.code, 'wallet_error');
});

test('an incomplete built call is refused before the wallet is asked', async () => {
    const w = walletFor();
    const r = await ask((done) => createVest(hostFor({ cancel_call: { summary: 'x' } }), w).cancel(1, done));
    assert.equal(r.ok, false);
    assert.equal(r.code, 'bad_call');
    assert.equal(w.proposed.length, 0);
});

test('the file is QML-engine safe: no BigInt literals or reserved names', () => {
    const src = readFileSync(new URL('./VestSDK.mjs', import.meta.url), 'utf8')
        .replace(/\/\/.*$/gm, '');
    assert.equal(/\b\d+n\b/.test(src), false);
    assert.equal(/\?\./.test(src), false);
    assert.equal(/\?\?/.test(src), false);
    assert.equal(/\bclass\s+\w+\s*\{/.test(src), false);
    assert.equal(/\b(short|int|long|char|byte|float|double|final)\b/.test(src), false);
});
