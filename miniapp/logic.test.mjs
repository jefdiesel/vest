// Checks the Vesting mini-app's own logic (qml/logic.mjs, which qml/Main.qml imports), since
// the QML engine cannot run in CI. Run with: node --test miniapp/logic.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import * as L from "./qml/logic.mjs";
import { readFileSync } from "node:fs";

const check = (label, got, want) => {
  test(label, () => assert.deepStrictEqual(got, want));
};
const NOW = Date.UTC(2026, 10, 20, 12, 0, 0);
const DAY = 86400000;
const ID = "Baaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; // 34 base58 chars
const CLIFF = { id: "A", kind: "cliff-linear", total: "2000000", claimed: "0", start: NOW - 180 * DAY, cliff: NOW + 3 * DAY, end: NOW + 730 * DAY };
const LINEAR = { id: "B", kind: "linear", total: "1000000", claimed: "200000", start: NOW - 100 * DAY, cliff: 0, end: NOW + 264 * DAY };
const MILE = { id: "C", kind: "milestone", total: "400000", claimed: "100000", tranches: ["100000", "100000", "100000", "100000"], signalled: [true, true, false, false] };

// ── Formatting ────────────────────────────────────────────────────────────────────────
check("grouped", [L.grouped("2000000"), L.grouped("12"), L.grouped("x")], ["2,000,000", "12", "x"]);
check("amountText", L.amountText("1000000", "LEZ"), "1,000,000 LEZ");
check("short", [L.short("ABCDEF1234567890WXYZ"), L.short("short")], ["ABCDEF…WXYZ", "short"]);
check("dateText", L.dateText(NOW), "2026-11-20");
check("parseDate", L.parseDate("2026-11-23"), Date.UTC(2026, 10, 23));
check("inText", [L.inText(0, NOW), L.inText(3 * DAY, NOW), L.inText(5 * 3600000, NOW), L.inText(90000, NOW)], ["now", "in 3 days", "in 5 h", "in 2 min"]);

// ── Decimal comparison ────────────────────────────────────────────────────────────────
check("atLeast", [L.atLeast("10", "2"), L.atLeast("9", "10"), L.atLeast("0002", "2")], [true, false, true]);
check("above", [L.above("10", "2"), L.above("2", "2")], [true, false]);

// ── Vesting maths (amounts are exact decimal strings) ────────────────────────────────
check("cliff-linear: nothing before the cliff", [L.vested(CLIFF, NOW), L.claimable(CLIFF, NOW), L.locked(CLIFF, NOW)], ["0", "0", "2000000"]);
const atCliff = L.claimable(CLIFF, CLIFF.cliff);
check("cliff-linear: a lump at the cliff", [L.above(atCliff, "0"), L.above("2000000", atCliff)], [true, true]);
check("cliff-linear: fully vested at the end", [L.claimable(CLIFF, CLIFF.end), L.locked(CLIFF, CLIFF.end)], ["2000000", "0"]);
check("linear: halfway", [L.vested(LINEAR, LINEAR.start + 182 * DAY), L.claimable(LINEAR, LINEAR.start + 182 * DAY), L.locked(LINEAR, LINEAR.start + 182 * DAY)], ["500000", "300000", "500000"]);
check("linear: clamps before start and after end", [L.claimable(LINEAR, LINEAR.start - DAY), L.claimable(LINEAR, LINEAR.end + DAY)], ["0", "800000"]);
check("milestone: signalled tranches only", [L.vested(MILE, NOW), L.claimable(MILE, NOW), L.locked(MILE, NOW)], ["200000", "100000", "200000"]);
check("milestone: all signalled", L.claimable({ ...MILE, signalled: [true, true, true, true] }, NOW), "300000");
check("cancelled freezes vested", L.vested({ ...LINEAR, cancelled: true, vestedAtCancel: "500000" }, LINEAR.start + 300 * DAY), "500000");

// ── Fixed cases from core/tests/unit.rs (so the two cannot drift) ───────────────────
const lin = (total, start, end) => ({ kind: "linear", total: String(total), claimed: "0", start: String(start), cliff: "0", end: String(end) });
const cliffS = (total, start, cliff, end) => ({ kind: "cliff-linear", total: String(total), claimed: "0", start: String(start), cliff: String(cliff), end: String(end) });
const L1000 = lin(1000, 1000, 2000);
check("core: linear_vesting_is_exact_and_monotone", [999, 1000, 1001, 1500, 1999, 2000].map(t => L.vested(L1000, t)), ["0", "0", "1", "500", "999", "1000"]);
check("core: linear at u64::MAX", L.vested(L1000, "18446744073709551615"), "1000");
const C1200 = cliffS(1200, 100, 400, 1000);
check("core: cliff_linear_waits_then_unlocks_at_cliff", [399, 400, 700, 1000].map(t => L.vested(C1200, t)), ["0", "400", "800", "1200"]);
const Cplain = cliffS(1000, 100, 100, 1100);
check("core: cliff equal to start", [100, 600].map(t => L.vested(Cplain, t)), ["0", "500"]);
const U128 = "340282366920938463463374607431768211455", U64 = "18446744073709551615";
const HALF = ((BigInt(1) << BigInt(127)) - (BigInt(1) << BigInt(63)) - BigInt(1)).toString();
const HUGE = lin(U128, 0, U64);
check("core: u128::MAX total, no overflow", L.vested(HUGE, U64), U128);
check("core: (2^128-1)/(2^64-1) exact at half", L.vested(HUGE, "9223372036854775807"), HALF);
// BigInt path (Node) and the decimal-limb fallback (QML) must give identical results.
const triples = [["1000", "1", "1000"], ["1000", "500", "1000"], ["1200", "300", "900"], [U128, "9223372036854775807", U64], [U128, "18446744073709551614", U64], ["7", "3", "2"], ["100", "1", "3"]];
check("BigInt path available", L.HAS_BIGINT, true);
check("mulDivFloor matches core cases", triples.map(x => L.mulDivFloor(...x)), ["1", "500", "400", HALF, ((BigInt(U128) * (BigInt(U64) - BigInt(1))) / BigInt(U64)).toString(), "10", "33"]);
check("BigInt path == QML limb fallback", triples.map(x => L.mulDivFloorString(...x)), triples.map(x => L.mulDivFloor(...x)));

// ── Next unlock ───────────────────────────────────────────────────────────────────────
const nu = L.nextUnlock(CLIFF, NOW);
check("next unlock: date + in 3 days", [nu.at, L.dateText(nu.at), /in 3 days/.test(nu.text), /Cliff on/.test(nu.text)], [CLIFF.cliff, "2026-11-23", true, true]);
check("next unlock: milestone has no date", [/tranche 3/.test(L.nextUnlock(MILE, NOW).text), L.nextUnlock(MILE, NOW).at], [true, 0]);
check("next unlock: all signalled", L.nextUnlock({ ...MILE, signalled: [true, true, true, true] }, NOW).text, "All milestones signalled.");

// ── Accounts (addresses or LNS names; the module resolves names) ───────────────────
const HEXID = "0a".repeat(32);
check("account kinds", [L.accountKind("Public/" + ID), L.accountKind("Private/" + ID), L.accountKind(HEXID), L.accountKind(ID), L.accountKind("alice"), L.accountKind("not an account!")], ["base58", "base58", "hex", "base58", "name", ""]);
check("account: names and addresses pass", [L.problemAccount("alice"), L.problemAccount(HEXID), L.problemAccount("Private/" + ID)], ["", "", ""]);
check("account: junk and empty refused", [/not an account/.test(L.problemAccount("b@d!")), /Enter/.test(L.problemAccount("  "))], [true, true]);
check("account: bad base58 after a prefix refused", L.problemAccount("Public/bad!0OIl") !== "", true);

// ── Creation validation ───────────────────────────────────────────────────────────────
const valid = (kind, extra) => ({ kind, beneficiary: "Public/" + ID, token: "LEZ", amount: "1000000", start: NOW, cliff: NOW + 30 * DAY, end: NOW + 365 * DAY, tranches: [], cancelable: true, transferable: false, ...extra });
check("validateCreate: valid kinds", [L.validateCreate(valid("cliff-linear")).ok, L.validateCreate(valid("linear")).ok, L.validateCreate(valid("milestone", { tranches: ["250000", "750000"] })).ok], [true, true, true]);
check("validateCreate: bad kind", L.validateCreate(valid("other")).ok, false);
check("validateCreate: bad beneficiary", L.validateCreate(valid("linear", { beneficiary: "no pe!" })).ok, false);
check("validateCreate: a name or hex beneficiary", [L.validateCreate(valid("linear", { beneficiary: "alice" })).ok, L.validateCreate(valid("linear", { beneficiary: HEXID })).ok], [true, true]);
check("validateCreate: token is LEZ or a definition", [L.validateCreate(valid("linear", { token: HEXID })).ok, L.validateCreate(valid("linear", { token: "DOGE" })).ok], [true, false]);
check("validateCreate: bad amount", L.validateCreate(valid("linear", { amount: "-1" })).ok, false);
check("validateCreate: bad dates", [L.validateCreate(valid("linear", { end: NOW - DAY })).ok, L.validateCreate(valid("cliff-linear", { cliff: NOW - DAY })).ok], [false, false]);
check("validateCreate: milestone needs tranches", [L.validateCreate(valid("milestone")).ok, L.validateCreate(valid("milestone", { tranches: ["0"] })).ok], [false, false]);
check("validateCreate: bad authority", L.validateCreate(valid("linear", { cancelAuthority: "b@d" })).ok, false);

// ── Pre-claim confirmation ────────────────────────────────────────────────────────────
const SIM = { fee_simulated: true, estimated_fee_display: "0.000012 LEZ", fee_available_display: "0.00001 LEZ" };
check("feeCheck: covered", [L.feeCheck({ ...SIM, fee_covered: true, fee_payer: "caller" }).ok, /can pay the estimated fee of 0.000012 LEZ/.test(L.feeCheck({ ...SIM, fee_covered: true }).message)], [true, true]);
const shortFee = L.feeCheck({ ...SIM, fee_covered: false, fee_payer: "claim_target" });
check("feeCheck: not covered is clear", [shortFee.ok, /private account/.test(shortFee.message), /can't pay/.test(shortFee.message), /0.00001 LEZ/.test(shortFee.message)], [false, true, true, true]);
check("feeCheck: a reverting claim is refused", L.feeCheck({ ...SIM, fee_covered: true, revert_reason: "x" }).ok, false);
check("feeCheck: no fee on a local ledger", [L.feeCheck({ fee_simulated: false }).ok, /No network fee/.test(L.feeCheck({}).message)], [true, true]);
const pd = L.privacyDisclosure();
check("privacy disclosure: visible", [pd.visible.length, /claim amount/.test(pd.text), /beneficiary address/.test(pd.text), /vesting schedule address/.test(pd.text)], [3, true, true, true]);
check("privacy disclosure: not traceable", [/which private account/.test(pd.text), /any later movement/.test(pd.text)], [true, true]);

// ── Plain result lines ───────────────────────────────────────────────────────────────
check("claimLine: private", L.claimLine("300000", "LEZ", "Private/" + ID, "tx1", true), "Claimed 300,000 LEZ to Private/" + ID + " (private) · tx1.");
check("claimLine: public", /\(public\)/.test(L.claimLine("1", "LEZ", "Public/" + ID, "", false)), true);
check("claimLineDisplay uses the module's string", L.claimLineDisplay("0.094138 LEZ", "Private/" + ID, "tx9", true), "Claimed 0.094138 LEZ to Private/" + ID + " (private) · tx9.");
check("claimLineDisplay with the module's tx", L.claimLineDisplay("1 LEZ", "x", { hash: "c5d782913d8819b009c8d15815697b20", block: 39066 }, false), "Claimed 1 LEZ to x (public) · tx c5d782…7b20 in block 39066.");
check("result lines", [L.createLine("SCHED-1234567890"), L.cancelLine("SCHED-1234567890"), L.signalLine(2, "SCHED-1234567890")], ["Created schedule SCHED-…7890.", "Cancelled schedule SCHED-…7890.", "Signalled milestone 3 on SCHED-…7890."]);
check("nothingClaimable", /in 3 days/.test(L.nothingClaimable(CLIFF, NOW)), true);

// ── LNS names: the module's name book is {hex account: name} ─────────────────────
const NAMES = { [HEXID]: "alice" };
check("nameOf", [L.nameOf(HEXID, NAMES), L.nameOf(HEXID.toUpperCase(), NAMES), L.nameOf("0b".repeat(32), NAMES)], ["alice", "alice", ""]);
check("labelled", [L.labelled(HEXID, NAMES), L.labelled("0b".repeat(32), {})], ["alice · " + L.short(HEXID), L.short("0b".repeat(32))]);
check("accountInput passes names and accounts through", [L.accountInput("  alice ").account, L.accountInput("Private/" + ID).ok], ["alice", true]);
check("accountInput refuses junk", L.accountInput("not a name!").ok, false);
check("resolvedLines", L.resolvedLines([{ name: "alice", base58: ID }]), ["alice resolves to " + ID + "."]);

// ── Module replies ───────────────────────────────────────────────────────────────
check("reply: plain, once and twice encoded", [L.reply('{"ok":true,"id":1}').id, L.reply(JSON.stringify('{"ok":true,"id":2}')).id, L.reply({ ok: false, error: "x" }).error], [1, 2, "x"]);
check("reply: a missing module", [L.reply("Module not connected: vesting").ok, L.reply("Module not connected: vesting").missing], [false, true]);
const refused = L.reply(JSON.stringify({ error: "Call refused", message: "expected integer at arg0, got number", method: "pre_claim_summary", module: "vesting" }));
check("reply: the host's refusal is shown, not 'no answer'", [refused.ok, refused.error], [false, "Basecamp refused the call (pre_claim_summary): expected integer at arg0, got number"]);
check("reply: an error object without ok or message", [L.reply({ error: "boom" }).ok, /boom/.test(L.reply({ error: "boom" }).error), /No answer/.test(L.reply({ error: "boom" }).error)], [false, true, false]);
check("reply: a missing module as an error object", L.reply({ error: "Module not connected", module: "vesting" }).missing, true);
check("reply: the module's detail stays with the answer (shown only under Technical details)", L.reply(JSON.stringify({ ok: false, error: "Still busy. Try again in a minute.", detail: "HTTP 429: Rate limited" })).detail, "HTTP 429: Rate limited");
check("reply: an HTML body from the edge is never the message", (() => { const r = L.reply("<!DOCTYPE html><html><body>Error 1015 rate limited</body></html>"); return [r.ok, /<|html/i.test(r.error)]; })(), [false, false]);
check("reply: no answer", [L.reply("").ok, /No answer/.test(L.reply(undefined).error)], [false, true]);

// ── The module's data ─────────────────────────────────────────────────────────────
const ROW = { id: 3, kind: { type: "cliff-linear", start: 1, cliff: 5, end: 9 }, token: "0".repeat(64), total: "9", claimed: "0", signalled: [], mine: { beneficiary: true } };
const flat = L.fromModule(ROW);
check("fromModule flattens the kind", [flat.kind, flat.start, flat.cliff, flat.end, flat.tokenLabel, flat.mine.beneficiary, flat.id], ["cliff-linear", 1, 5, 9, "LEZ", true, 3]);
check("fromModule: milestone tranches", L.fromModule({ kind: { type: "milestone", tranches: ["4", "6"] }, signalled: [true, false] }).tranches, ["4", "6"]);
check("fromModule: maths work on the adapted row", L.claimable(flat, 9), "9");
const made = L.toCreateRow({ kind: "cliff-linear", beneficiary: " alice ", token: "LEZ", amount: "100", start: 1, cliff: 2, end: 3, cancelable: true, transferable: false, cancelAuthority: "", milestoneAuthority: "bob" }, HEXID);
check("toCreateRow", made, { creator: HEXID, beneficiary: "alice", token: "LEZ", total: "100", kind: { type: "cliff-linear", start: 1, end: 3, cliff: 2 }, cancelable: true, transferable: false, cancel_authority: null, milestone_authority: "bob" });
check("toCreateRow: milestone total is the tranche sum", L.toCreateRow({ kind: "milestone", beneficiary: "a", token: "LEZ", tranches: ["40", "60"] }, HEXID).total, "100");

// ── Batch creation ──────────────────────────────────────────────────────────────────
check("parseTrancheText", [L.parseTrancheText("100000, 200000;300000"), L.parseTrancheText(""), L.parseTrancheText("1,,2")], [["100000", "200000", "300000"], [], ["1", "2"]]);
const batchShared = { kind: "linear", token: "LEZ", start: NOW, cliff: 0, end: NOW + 365 * DAY, cancelable: true, transferable: false };
const batchValid = { ...batchShared, rows: [{ beneficiary: "Public/" + ID, amount: "100000" }, { beneficiary: "Private/" + ID, amount: "200000" }] };
check("validateBatch: valid", L.validateBatch(batchValid).ok, true);
check("validateBatch: bad row is named", (() => { const v = L.validateBatch({ ...batchValid, rows: [{ beneficiary: "Public/" + ID, amount: "100000" }, { beneficiary: "bad", amount: "0" }] }); return [v.ok, /Recipient 2/.test(v.errors.join(" "))]; })(), [false, true]);
check("validateBatch: no rows", L.validateBatch({ ...batchValid, rows: [] }).ok, false);
check("validateBatch: milestone tranches", L.validateBatch({ ...batchShared, kind: "milestone", rows: [{ beneficiary: "Public/" + ID, amount: "100000", tranches: "40000,60000" }] }).ok, true);
check("validateBatch: milestone needs tranches", L.validateBatch({ ...batchShared, kind: "milestone", rows: [{ beneficiary: "Public/" + ID, amount: "100000", tranches: "" }] }).ok, false);
check("batchLine", [L.batchLine(1), L.batchLine(3)], ["Created 1 schedule.", "Created 3 schedules."]);

// ── Every module call passes text: Basecamp refuses a QML number for an integer parameter ──
{
  const qml = readFileSync(new URL("./qml/Main.qml", import.meta.url), "utf8");
  const calls = [...qml.matchAll(/\b(?:call|write)\((?:"[^"]*"(?:\s*\+\s*[^,]+)?,\s*)?"(\w+)",\s*(\[[^\]]*\]|\w+)/g)];
  check("Main.qml calls the module", calls.length >= 12, true);
  check("no module call passes a Number(…)", calls.filter(([, , args]) => /Number\(/.test(args)).map(([, m]) => m), []);
  check("ids handed to calls are made with String(…)", [...qml.matchAll(/\bid = (\w+)\(/g)].map(m => m[1]).filter(f => f !== "String"), []);
}

// ── Plain words on the screen ────────────────────────────────────────────────────
check("toBase: LEZ to base units, exact", [L.toBase("0.5"), L.toBase("1,000"), L.toBase(".000000001"), L.toBase("2"), L.toBase("1.0000000001"), L.toBase("x"), L.toBase(""), L.toBase("0")], ["500000000", "1000000000000", "1", "2000000000", "", "", "", ""]);
check("toBase: token decimals", L.toBase("1.5", 2), "150");
const NOWALLET = { ok: true, connected: false, status: "not opened yet", settings: { wallet_dir: "" } };
check("first run: no wallet chosen is not an error", [L.firstRun(NOWALLET), L.connectProblem(NOWALLET)], [true, ""]);
check("a saved wallet not opened yet is not an error either", L.connectProblem({ connected: false, status: "not opened yet", settings: { wallet_dir: "/w" } }), "");
check("a wrong folder says so plainly", L.connectProblem({ connected: false, status: "no wallet at /w", settings: { wallet_dir: "/w" } }), "No wallet found in that folder. Choose the folder that holds wallet_config.json and storage.json.");
check("an encrypted wallet, another chain", [/password/.test(L.connectProblem({ connected: false, status: "the wallet is encrypted: give its password", settings: { wallet_dir: "/w" } })), /different network/.test(L.connectProblem({ connected: false, status: "https://x serves chain ab, not cd", settings: { wallet_dir: "/w" } }))], [true, true]);
check("connected: no problem, no first run", [L.connectProblem({ connected: true }), L.firstRun({ connected: true, settings: {} })], ["", false]);
check("isFinished", [L.isFinished({ total: "10", claimed: "10", claimable: "0" }), L.isFinished({ total: "10", claimed: "3", claimable: "1" }), L.isFinished({ total: "10", claimed: "3", claimable: "0", cancelled: true })], [true, false, true]);
// A cancelled schedule: the program keeps what had vested by the cancel claimable (core
// CancelRecord.vested_at_cancel); nothing is locked and nothing more vests.
const CANCELLED = { id: 0, total: "1000000", claimed: "652652", claimable: "231", claimable_display: "0.000000231 LEZ", cancelled: true, cancelled_at: Date.UTC(2026, 10, 20, 21, 5), refund: "347117", refund_display: "0.000347117 LEZ" };
check("cancelled: status line, row, still claimable", [L.statusLine(CANCELLED), L.rowState(CANCELLED), L.isClosed(CANCELLED), L.isFinished(CANCELLED)],
  ["Cancelled on 2026-11-20. 0.000347117 LEZ went back to the creator.", "Cancelled · 0.000000231 LEZ to claim", true, false]);
check("cancelled and claimed out", [L.rowState({ ...CANCELLED, claimable: "0" }), L.isFinished({ ...CANCELLED, claimable: "0" })], ["Cancelled", true]);
const DONE = { id: 1, total: "500", claimed: "500", claimable: "0", claimable_display: "0 LEZ" };
check("fully claimed: status line and row", [L.statusLine(DONE), L.rowState(DONE), L.isClosed(DONE), L.isFinished(DONE)], ["Fully claimed.", "Fully claimed", true, true]);
check("a live schedule has no status line", [L.statusLine({ total: "10", claimed: "1", claimable: "2", claimable_display: "2 LEZ" }), L.rowState({ total: "10", claimed: "1", claimable: "2", claimable_display: "2 LEZ" })], ["", "2 LEZ to claim"]);
check("confirm sheet: cancel", L.confirmText("cancel", { id: 0, locked_display: "0.000347 LEZ" }), "Cancel schedule 0? The unvested 0.000347 LEZ goes back to you. This can't be undone.");
check("confirm sheet: irrevocable", /can't be undone/.test(L.confirmText("irrevocable", { id: 2 })), true);
check("scheduleTitle", L.scheduleTitle({ id: 3, kind: "cliff-linear", total_display: "1.5 LEZ" }), "Schedule 3 · cliff, then linear · 1.5 LEZ");
check("clean and hostOf", [L.clean("a\u202eb\u0007c"), L.hostOf("https://lez.nameswallet.app/"), L.hostOf("http://127.0.0.1:3071")], ["abc", "lez.nameswallet.app", "127.0.0.1:3071"]);
{
  const qml = readFileSync(new URL("./qml/Main.qml", import.meta.url), "utf8");
  check("no 'hex or base58' wording on the screen", /hex or base58/i.test(qml), false);
  check("no green for state: state is said in words, in ink", /look\.good|\.success\b|"green"/.test(qml), false);
  check("a closed schedule hides Still locked and Next", /key: "Still locked"; visible: !!root\.selected && !Logic\.isClosed/.test(qml) && /key: "Next"; visible: !!root\.selected && !Logic\.isClosed/.test(qml), true);
  check("the kit is used, Vest's own copies are gone", [/import "kit"/.test(qml), /component (Action|Field|Link|Toggle|Fact|Panel):/.test(qml)], [true, false]);
}

// ── Names Wallet ───────────────────────────────────────────────────────────────────
check("namesReply: value, busy, refusal, missing", [
  L.namesReply(JSON.stringify({ success: true, value: { connected: true }, error: null })).value.connected,
  L.namesReply(JSON.stringify({ success: false, busy: true, value: null, error: "The wallet is busy; still working." })).busy,
  L.namesReply({ success: false, value: null, error: "ask for access first" }).error,
  L.namesReply("Module not connected: names").missing,
  L.namesReply("").busy], [true, true, "ask for access first", true, true]);
check("zone check", [L.zoneProblem({ zone: { channel: "2DA7" } }, "2da7"), /Switch Names Wallet to the bridge test network/.test(L.zoneProblem({ zone: { channel: "01" } }, "2da7")), L.zoneProblem({}, "2da7")], ["", true, ""]);
check("request lines", [L.requestLine({ state: "pending" }, "the claim"), L.requestLine({ state: "rejected" }, "the claim"), L.requestLine({ state: "done" }), L.requestOpen({ state: "running" }), L.requestOpen({ state: "done" })],
  ["Waiting for you to approve the claim in Names Wallet.", "You declined the claim in Names Wallet. Nothing was sent.", "", true, false]);
check("namesAccounts joins balances", L.namesAccounts({ accounts: [{ account: "Public/A", account_kind: "public" }, { account: "Private/B", account_kind: "private" }] }, [{ account: "Public/A", balance: "12" }]),
  [{ account: "Public/A", account_kind: "public", balance: "12" }, { account: "Private/B", account_kind: "private", balance: "0" }]);

// ── Guides ────────────────────────────────────────────────────────────────────────
check("guide: receiving, five one-line steps, current marked", L.guideSteps("receive", 2).map(s => s.state), ["done", "done", "now", "later", "later"]);
check("guide: paying out, the steps", L.guideSteps("pay", 0).map(s => s.text), ["Connect Names Wallet", "Choose who, how much and the schedule", "Approve it in Names Wallet", "Share the schedule's address or your LNS name with them", "Cancel it, or make it permanent, later if you need to"]);
check("guide: every step says what to do", ["receive", "pay"].every(r => L.guideSteps(r, 0).every(x => typeof x.detail === "string" && x.detail.length > 20)), true);
check("guide: where a receiver is", [L.receiveStep({}), L.receiveStep({ connected: true }), L.receiveStep({ connected: true, schedules: true }), L.receiveStep({ connected: true, schedules: true, claimable: true }), L.receiveStep({ connected: true, schedules: true, approving: true })], [0, 1, 2, 3, 4]);
check("guide: where a payer is", [L.payStep({}), L.payStep({ connected: true }), L.payStep({ connected: true, justCreated: true }), L.payStep({ connected: true, manages: true })], [0, 1, 3, 4]);
check("each kind explained in one sentence", ["linear", "cliff-linear", "milestone"].map(k => L.kindExplain(k).split(". ").length <= 2 && L.kindExplain(k).length > 20), [true, true, true]);
check("timelines", [L.timeline("linear").length, L.timeline("cliff-linear")[1].kind, L.timeline("milestone").every(s => s.kind !== "unlocking")], [1, "cliff", true]);
check("what happens next", [L.nextLine("create", { id: 3 }), /address/.test(L.nextLine("import")) || /list/.test(L.nextLine("import")), L.nextLine("nothing")], ["Schedule 3 is live. Send them its address or your LNS name so they can find it in Vest.", true, ""]);
{
  const qml = readFileSync(new URL("./qml/Main.qml", import.meta.url), "utf8");
  check("the words: LEZ is the coin, Logos the network", [/on LEZ\b/.test(qml), /a LEZ wallet/i.test(qml), /Pay tokens out over time, or claim what has vested to you\./.test(qml)], [false, false, true]);
  check("names calls pass text only", [...qml.matchAll(/namesCall\("(\w+)",\s*(\[[^\]]*\])/g)].filter(m => /Number\(/.test(m[2])).map(m => m[1]), []);
  check("first run asks to connect Names Wallet, not for a wallet folder", [/What do you want to do\?/.test(qml) && /label: "Connect"/.test(qml), /Advanced: a wallet folder/.test(qml)], [true, true]);
}

// ── QML hygiene (as basecamp/logic.test.mjs) ───────────────────────────────────────
const reserved = /\.(public|private|protected|package|interface|implements|static|let|yield|enum|class|const|export|import|extends|super)\b/;
for (const file of ["./qml/Main.qml", "./qml/logic.mjs", ...["Action", "BackButton", "FactRow", "Field", "Link", "Look", "PageHeader", "RunsOn", "TechDetails", "Tick"].map(k => `./qml/kit/${k}.qml`)]) {
  const source = readFileSync(new URL(file, import.meta.url), "utf8").split("\n");
  const dotted = source
    .map((line, i) => [i + 1, line.replace(/"(\\.|[^"\\])*"|'(\\.|[^'\\])*'|\/\/.*$/g, '""')])
    .filter(([, line]) => reserved.test(line))
    .map(([n]) => n);
  check(`no reserved word after a dot in ${file}`, dotted, []);
}

