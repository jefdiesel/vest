// Checks the Vesting mini-app's own logic (qml/logic.mjs, which qml/Main.qml imports), since
// the QML engine cannot run in CI. Run with: node --test miniapp/logic.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import * as L from "./qml/logic.mjs";
import * as SDK from "./qml/NamesWalletSDK.mjs";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";

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
check("next unlock: milestone has no date", [/milestone 3/.test(L.nextUnlock(MILE, NOW).text), L.nextUnlock(MILE, NOW).at], [true, 0]);
check("next unlock: all unlocked", L.nextUnlock({ ...MILE, signalled: [true, true, true, true] }, NOW).text, "All milestones unlocked.");

// ── Accounts (addresses or LNS names; the module resolves names) ───────────────────
const HEXID = "0a".repeat(32);
check("account kinds", [L.accountKind("Public/" + ID), L.accountKind("Private/" + ID), L.accountKind(HEXID), L.accountKind(ID), L.accountKind("alice"), L.accountKind("not an account!")], ["base58", "base58", "hex", "base58", "name", ""]);
check("account: names and addresses pass", [L.problemAccount("alice"), L.problemAccount(HEXID), L.problemAccount("Private/" + ID)], ["", "", ""]);
check("account: junk and empty refused", [/not an address/.test(L.problemAccount("b@d!")), /Enter/.test(L.problemAccount("  "))], [true, true]);
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

// ── LNS names: the module's name book is {hex account: name} ─────────────────────
const NAMES = { [HEXID]: "alice" };
check("nameOf", [L.nameOf(HEXID, NAMES), L.nameOf(HEXID.toUpperCase(), NAMES), L.nameOf("0b".repeat(32), NAMES)], ["alice", "alice", ""]);
check("labelled", [L.labelled(HEXID, NAMES), L.labelled("0b".repeat(32), {})], ["alice · " + L.short(L.base58Of(HEXID)), L.short(L.base58Of("0b".repeat(32)))]);
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
check("confirm sheet: cancel", L.confirmText("cancel", { id: 0, locked_display: "0.000347 LEZ" }), "Cancel schedule 0? The locked amount 0.000347 LEZ goes back to you. This can't be undone.");
check("confirm sheet: irrevocable", /can't be undone/.test(L.confirmText("irrevocable", { id: 2 })), true);
check("scheduleTitle", L.scheduleTitle({ id: 3, kind: "cliff-linear", total_display: "1.5 LEZ" }), "Schedule 3 · cliff, then evenly · 1.5 LEZ");
check("clean and hostOf", [L.clean("a\u202eb\u0007c"), L.hostOf("https://lez.nameswallet.app/"), L.hostOf("http://127.0.0.1:3071")], ["abc", "lez.nameswallet.app", "127.0.0.1:3071"]);
{
  const qml = readFileSync(new URL("./qml/Main.qml", import.meta.url), "utf8");
  check("no 'hex or base58' wording on the screen", /hex or base58/i.test(qml), false);
  check("no green for state: state is said in words, in ink", /look\.good|\.success\b|"green"/.test(qml), false);
  check("a closed schedule hides Still locked and Next", /key: "Still locked"; visible: !!root\.selected && !Logic\.isClosed/.test(qml) && /key: "Next"; visible: !!root\.selected && !Logic\.isClosed/.test(qml), true);
  check("the kit is used, Vest's own copies are gone", [/import "kit"/.test(qml), /component (Action|Field|Link|Toggle|Fact|Panel):/.test(qml)], [true, false]);
}

// ── Names Wallet ───────────────────────────────────────────────────────────────────
// The Names Wallet SDK, copied unchanged from the Names repository (sdk/qml/): its own tests are
// there (sdk/NamesWalletSDK.test.mjs). A new copy changes these hashes on purpose.
const SDK_FILES = { "NamesWalletSDK.mjs": "69be803a6c8db2ee988183564a20ecf9950920f2b235b6f52ec8201dc59a40ef", "NamesWallet.qml": "6161ab99dca96e6b50290ff84e9fa327c5a26b6d5c03c665c419d8382ccafc65" };
check("the Names Wallet SDK is version 1.0.0, unchanged", [SDK.VERSION, ...Object.keys(SDK_FILES).map(f => createHash("sha256").update(readFileSync(new URL("./qml/" + f, import.meta.url))).digest("hex"))],
  ["1.0.0", ...Object.values(SDK_FILES)]);
check("names replies through the SDK: value, busy, refusal, missing, unclear", [
  SDK.reply(JSON.stringify({ success: true, value: { connected: true }, error: null })).value.connected,
  SDK.reply(JSON.stringify({ success: false, busy: true, value: null, error: "The wallet is busy; still working." })).busy,
  SDK.reply({ success: false, value: null, error: "ask for access first" }).error,
  SDK.reply("Module not connected: names").missing,
  SDK.reply("").unclear], [true, true, "ask for access first", true, true]);
check("Vest's Names words are the SDK's shared ones", L.requestLine === SDK.requestLine, true);
check("zone check", [L.zoneProblem({ zone: { channel: "2DA7" } }, "2da7"), /Switch Names Wallet to the bridge test network/.test(L.zoneProblem({ zone: { channel: "01" } }, "2da7")), L.zoneProblem({}, "2da7")], ["", true, ""]);
check("request lines", [L.requestLine({ state: "pending" }, "the claim"), L.requestLine({ state: "rejected" }, "the claim"), L.requestLine({ state: "done" }), L.requestOpen({ state: "running" }), L.requestOpen({ state: "done" })],
  ["Waiting for you to approve the claim in Names Wallet.", "You declined the claim in Names Wallet. Nothing was sent.", "", true, false]);
check("namesAccounts joins balances", L.namesAccounts({ accounts: [{ account: "Public/A", account_kind: "public" }, { account: "Private/B", account_kind: "private" }] }, [{ account: "Public/A", balance: "12" }]),
  [{ account: "Public/A", account_kind: "public", balance: "12" }, { account: "Private/B", account_kind: "private", balance: "0" }]);

// ── Guides ────────────────────────────────────────────────────────────────────────
check("guide: receiving, five one-line steps, current marked", L.guideSteps("receive", 2).map(s => s.state), ["done", "done", "now", "later", "later"]);
check("guide: paying out, the steps", L.guideSteps("pay", 0).map(s => s.text), ["Connect Names Wallet", "Choose who, how much and the schedule", "Approve it in Names Wallet", "Send them the schedule code", "Cancel it, or make it permanent, later if you need to"]);
check("guide: every step says what to do", ["receive", "pay"].every(r => L.guideSteps(r, 0).every(x => typeof x.detail === "string" && x.detail.length > 20)), true);
check("payee: a name shows who and the address", L.payeeLine("alice", { ok: true, name: "alice", base58: "FLBC4VrQ12UDpbG2ArnRgcwrZTz5cabKbT9oY1M7BE2Z" }).ok, true);
check("payee: an unknown name says so", L.payeeLine("nobdy", { ok: false, error: "unknown LNS name: nobdy" }).text.startsWith("No one holds the LNS name nobdy"), true);
check("payee: a name with no account says so", L.payeeLine("jefdiesel", { ok: false, error: "LNS name 'jefdiesel' publishes no account to pay" }).text.startsWith("Can't pay jefdiesel by name yet"), true);
check("payee: an address is shown back", L.payeeLine("x", { ok: true, base58: "FLBC4VrQ12UDpbG2ArnRgcwrZTz5cabKbT9oY1M7BE2Z" }).text.startsWith("Pays the address"), true);
check("names write words", [L.writeWhat("create"), L.writeWhat("cancel:3"), L.writeWhat("signal:3:1"), L.writeWhat("x")], ["the new schedule", "the cancel", "the milestone", "the request"]);
check("hex accounts show as base58", [L.base58Of("11".repeat(32)), L.base58Of("00".repeat(31) + "01"), L.base58Of("Public/x")], ["29d2S7vB453rNYFdR5Ycwt7y9haRT5fwVwL9zTmBhfV2", "1".repeat(31) + "2", "Public/x"]);
check("names: an unreadable answer is unclear (a read is asked again; a proposal never)", SDK.reply("<html>").unclear, true);
const FOUND = { ok: true, name: "alice", display: "alice", account: "FLBC4VrQ12UDpbG2ArnRgcwrZTz5cabKbT9oY1M7BE2Z", forwardedFrom: null, lapsed: false };
check("payee through Names: a name shows who and the address", [L.payeeFromNames("alice", FOUND).ok, L.payeeFromNames("alice", FOUND).text], [true, "Pays alice · FLBC4V…BE2Z"]);
check("payee through Names: a forward says where it points", L.payeeFromNames("old", { ...FOUND, forwardedFrom: "old" }).text, "Pays old (it points to alice) · FLBC4V…BE2Z");
check("payee through Names: unknown, no account, can't check", [
  L.payeeFromNames("nobdy", { ok: false, code: "not_registered", name: "nobdy" }).text.startsWith("No one holds the LNS name nobdy"),
  L.payeeFromNames("empty", { ok: false, code: "no_account", name: "empty" }).text.startsWith("Can't pay empty by name"),
  L.payeeFromNames("alice", { ok: false, code: "unavailable", error: "Names Wallet is busy." }).retry], [true, true, true]);
check("the schedule is made for the account Names found, only for what is still typed", [
  L.payeeAccount(L.payeeFromNames("alice", FOUND), "alice"), L.payeeAccount(L.payeeFromNames("alice", FOUND), "alicia"),
  L.payeeAccount({ ok: true, base58: FOUND.account, text: "x" }, "alice"), L.payeeAccount(null, "alice")],
  ["Public/" + FOUND.account, "", "", ""]);
check("lezText", [L.lezText("1111000000000"), L.lezText("107285968"), L.lezText("0")], ["1,111 LEZ", "0.107285968 LEZ", "0 LEZ"]);
check("shortOfFunds: enough, too little, milestones, unknown balance", [
  L.shortOfFunds("2000000000000", "1111000000000"), L.shortOfFunds("107285968000", "1111000000000").startsWith("This schedule needs 1,111 LEZ, and the paying account holds 107.285968 LEZ"),
  L.shortOfFunds("1000", "0", ["600", "600"]).startsWith("This schedule needs"), L.shortOfFunds(undefined, "5")], ["", true, true, ""]);
check("createSummary says the LNS name typed", [L.createSummary({ summary: "S", amount_display: "1 LEZ" }, "jefdiesel"), L.createSummary({ summary: "S" }, "76DoKtAcVM98avrHdPV7PiUgzCz5mAUJWJKdv5Tbj8ub")], ["Create a Vest schedule: 1 LEZ from this account, to jefdiesel", "S"]);
check("importing a name says to ask for the schedule code", L.importProblem("jefdiesel", "BawxwquWtPEVcrwSFftD6jS3XFxZNv4qYrV8pkyo9mu7 is not a schedule of this Vest program").startsWith("jefdiesel is a person's LNS name, not a schedule."), true);
{
  const rc = L.receiptOf({ id: 3, amountDisplay: "1 LEZ", kind: "linear", start: L.atLocal(2026, 10, 21, 9, 0), end: L.atLocal(2026, 10, 22, 9, 0), cancelable: true, transferable: false,
    fromAddr: "BawxwquWtPEVcrwSFftD6jS3XFxZNv4qYrV8pkyo9mu7", fromName: "jefdiesel", toAddr: "CCVd76Tk3WvFb63Wbuu4exMeLRTbSuMWHqxuvysapKnk", toName: "jef04", tx: "ab".repeat(32), code: "cd".repeat(16) });
  check("receipt: title, names first, addresses kept, what to share", [rc.title, rc.rows[1].value, rc.rows[1].addr.length, rc.rows[2].value, rc.rows[3].value.startsWith("Evenly, from Sat 21 Nov 2026 · 09:00"), rc.share.startsWith("Send them the schedule code")],
    ["Schedule 3 created", "jefdiesel", 44, "jef04", true, true]);
}
check("guide: where a receiver is", [L.receiveStep({}), L.receiveStep({ connected: true }), L.receiveStep({ connected: true, schedules: true }), L.receiveStep({ connected: true, schedules: true, claimable: true }), L.receiveStep({ connected: true, schedules: true, approving: true })], [0, 1, 2, 3, 4]);
check("guide: where a payer is", [L.payStep({}), L.payStep({ connected: true }), L.payStep({ connected: true, justCreated: true }), L.payStep({ connected: true, manages: true })], [0, 1, 3, 4]);
check("each kind explained in one sentence", ["linear", "cliff-linear", "milestone"].map(k => L.kindExplain(k).split(". ").length <= 2 && L.kindExplain(k).length > 20), [true, true, true]);
check("timelines", [L.timeline("linear").length, L.timeline("cliff-linear")[1].kind, L.timeline("milestone").every(s => s.kind !== "unlocking")], [1, "cliff", true]);
check("what happens next", [L.nextLine("create", { id: 3 }), /address/.test(L.nextLine("import")) || /list/.test(L.nextLine("import")), L.nextLine("nothing")], ["Schedule 3 is live. Send them its schedule code (Copy schedule code, below): in their Vest they open Receive, paste it and press Add schedule.", true, ""]);
{
  const qml = readFileSync(new URL("./qml/Main.qml", import.meta.url), "utf8");
  check("the words: LEZ is the coin, Logos the network", [/on LEZ\b/.test(qml), /a LEZ wallet/i.test(qml), /Pay tokens out over time, or claim what has unlocked for you\./.test(qml)], [false, false, true]);
  check("the wallet only through the SDK, never named: no module calls to it, no \"names\" in Vest", [/callModuleAsync\("names"/.test(qml), /"names"/.test(qml.replace('signer: "names"', "")), /NamesWallet \{ id: wallet; app: "vesting_ui"/.test(qml), /wallet\.(connect|proposeCall|withdraw|resolve|check|disconnect|chooseWallet)\(/.test(qml)], [false, false, true, true]);
  check("names calls pass text only", [...qml.matchAll(/wallet\.\w+\(([^)]*)\)/g)].filter(m => /Number\(/.test(m[1])).map(m => m[0]), []);
  check("first run asks to connect Names Wallet; no wallet folder anywhere", [/What do you want to do\?/.test(qml) && /label: "Connect"/.test(qml), /wallet folder|wallet_dir|password|passwordField/i.test(qml)], [true, false]);
  check("Vest signs only through the wallet: no module call that signs itself", ["claim_public", "claim_private", "\"create\"", "create_batch", "\"cancel\"", "make_irrevocable", "signal_milestone", "transfer_beneficiary", "pre_claim_summary"].filter(m => qml.includes('call(' + (m.startsWith('"') ? m : '"' + m + '"'))), []);
  check("several wallets: Vest offers the choice", [/root\.namesState === "choose"/.test(qml), /wallet\.wallets/.test(qml)], [true, true]);
}

// ── QML hygiene (as basecamp/logic.test.mjs) ───────────────────────────────────────
const reserved = /\.(public|private|protected|package|interface|implements|static|let|yield|enum|class|const|export|import|extends|super)\b/;
for (const file of ["./qml/Main.qml", "./qml/logic.mjs", "./qml/NamesWalletSDK.mjs", "./qml/NamesWallet.qml", ...["Action", "BackButton", "FactRow", "Field", "Link", "Look", "PageHeader", "RunsOn", "TechDetails", "Tick"].map(k => `./qml/kit/${k}.qml`)]) {
  const source = readFileSync(new URL(file, import.meta.url), "utf8").split("\n");
  const dotted = source
    .map((line, i) => [i + 1, line.replace(/"(\\.|[^"\\])*"|'(\\.|[^'\\])*'|\/\/.*$/g, '""')])
    .filter(([, line]) => reserved.test(line))
    .map(([n]) => n);
  check(`no reserved word after a dot in ${file}`, dotted, []);
}

// ── Extra coverage: exports with no test, or only a one-line assertion ───────────────
// Amounts stay decimal strings; times are epoch ms. The BUG-marked checks below are meant to
// fail: they show a wrong result for an ordinary input. logic.mjs is not changed in this job.

// cmpDec, decSub and sumAmounts were only exercised indirectly through atLeast/above/vested.
check("cmpDec: order, leading zeros, null, u128 boundary", [
  L.cmpDec("0", "0"), L.cmpDec("2", "10"), L.cmpDec("10", "2"), L.cmpDec("0002", "2"),
  L.cmpDec(null, "0"), L.cmpDec(U128, String(BigInt(U128) - 1n))
], [0, -1, 1, 0, 0, 1]);
check("decSub: zeros, borrows, u128", [
  L.decSub("0", "0"), L.decSub("1000", "1"), L.decSub("000100", "99"), L.decSub(U128, "1")
], ["0", "999", "1", (BigInt(U128) - 1n).toString()]);
check("sumAmounts: empty, null, several, leading zeros, u128", [
  L.sumAmounts([]), L.sumAmounts([null]), L.sumAmounts(["1", "2", "3"]),
  L.sumAmounts(["0002", "7"]), L.sumAmounts([U128, "0"])
], ["0", "0", "6", "9", U128]);

// isNative and txText had only indirect coverage.
check("isNative: LEZ, zero account, a token id", [
  L.isNative(""), L.isNative(null), L.isNative(" lez "), L.isNative("0".repeat(64)),
  L.isNative(ID), L.isNative("LEZ ")
], [true, true, true, true, false, true]);

// isFullyClaimed and isClosed: empty, zero, partial, cancelled, fully claimed, u128.
check("isFullyClaimed: empty, zero, partial, exact, u128", [
  L.isFullyClaimed({}), L.isFullyClaimed({ total: "0", claimed: "0" }),
  L.isFullyClaimed({ total: "100", claimed: "99" }), L.isFullyClaimed({ total: "100", claimed: "000100" }),
  L.isFullyClaimed({ total: U128, claimed: U128 })
], [false, true, false, true, true]);
check("isClosed: cancelled, claimed out, still live", [
  L.isClosed({}), L.isClosed({ cancelled: true, total: "10", claimed: "0" }),
  L.isClosed({ total: "10", claimed: "10" }), L.isClosed({ total: "10", claimed: "9" })
], [false, true, true, false]);

// Vesting maths around a full claim and a cancellation that froze the vested amount.
check("a fully claimed schedule has nothing claimable or locked", [
  L.vested(DONE, NOW), L.claimable(DONE, NOW), L.locked(DONE, NOW)
], ["500", "0", "0"]);
check("a cancelled schedule keeps only its frozen amount claimable", [
  L.claimable({ ...LINEAR, cancelled: true, vestedAtCancel: "500000", claimed: "200000" }, LINEAR.end),
  L.locked({ ...LINEAR, cancelled: true, vestedAtCancel: "500000" }, LINEAR.end)
], ["300000", "500000"]);

// inText and dateText at the boundaries their headers promise.
check("inText: zero, seconds, minutes, a day and a half", [
  L.inText(-5, NOW), L.inText(1, NOW), L.inText(59999, NOW), L.inText(60000, NOW),
  L.inText(3600000 - 1, NOW), L.inText(DAY, NOW), L.inText(DAY + 12 * 3600000, NOW)
], ["now", "in 1 s", "in 1 min", "in 1 min", "in 1 h", "in 1 day", "in 1 day 12 h"]);
check("dateText: epoch and a normal date", [L.dateText(0), L.dateText(NOW)], ["1970-01-01", "2026-11-20"]);
// Fixed (found by these tests):
check("dateText: an invalid date should be empty", [L.dateText("not a date"), L.dateText(undefined), L.dateText(null)], ["", "", ""]);
// Fixed (found by these tests):
check("inText: one millisecond under a day", L.inText(DAY - 1, NOW), "in 1 day");

// nextUnlock: the end boundary and a row with no dates.
check("nextUnlock: at the end there is nothing left", [
  L.nextUnlock(LINEAR, LINEAR.end).at, L.nextUnlock(LINEAR, LINEAR.end).text
], [0, "Fully unlocked — nothing left to unlock."]);
check("nextUnlock: a row with no dates", L.nextUnlock({}, NOW).text, "Fully unlocked — nothing left to unlock.");
// Fixed (found by these tests):
check("nextUnlock: fewer signals than tranches", [
  L.nextUnlock({ ...MILE, signalled: [] }, NOW).text,
  L.nextUnlock({ ...MILE, signalled: [true] }, NOW).text
], ["Next unlock: milestone 1 — when the creator says it's reached.", "Next unlock: milestone 2 — when the creator says it's reached."]);

// Fee coverage and payer names not reached by the existing feeCheck assertions.

// connectProblem: an unreachable RPC, an unknown reason, and a wallet not opened yet.

// clean and the payee/reply fallbacks that earlier checks did not reach.
check("clean: length, null, control and bidi characters", [
  L.clean("x".repeat(250)).length, L.clean(null), L.clean("a\u0000b\u202ec")
], [200, "", "abc"]);
check("payeeLine: a rate limit is retryable", (() => {
  const p = L.payeeLine("alice", { ok: false, error: "429 too many requests" });
  return [p.ok, p.retry, /Couldn't check alice/.test(p.text), /429/.test(p.text)];
})(), [false, true, true, true]);
check("payeeLine: an unrecognised refusal, and no reason at all", [
  L.payeeLine("bob", { ok: false, error: "nope" }).text,
  L.payeeLine("bob", { ok: false }).text
], ["That isn't an address or an LNS name Vest can find: nope", "That isn't an address or an LNS name Vest can find."]);

// requestLine: the states not in the earlier check.
check("requestLine: running, expired, cancelled, failed, nothing", [
  L.requestLine({ state: "running" }, "the claim"),
  L.requestLine({ state: "expired" }, "the claim"),
  L.requestLine({ state: "cancelled" }, "the claim"),
  L.requestLine({ state: "failed", error: "no fee" }, "the claim"),
  L.requestLine(null, "the claim"),
  L.requestOpen({ state: "pending" }), L.requestOpen(null)
], [
  "Names Wallet is sending the claim…",
  "Nobody approved the claim within 10 minutes, so Names Wallet dropped it. Nothing was sent.",
  "You withdrew the claim. Nothing was sent.",
  "Names Wallet couldn't send the claim: no fee.",
  "", true, false
]);

// Guides and result lines: boundaries and the remaining actions.
check("guideSteps: before the first and past the last step", [
  L.guideSteps("receive", -1).map(s => s.state), L.guideSteps("receive", 9).map(s => s.state)
], [Array(5).fill("later"), Array(5).fill("done")]);
check("guide steps: approving wins over a claim or a new schedule", [
  L.receiveStep({ connected: true, schedules: true, claimable: true, approving: true }),
  L.payStep({ connected: true, approving: true, justCreated: true, manages: true })
], [4, 2]);
check("nextLine: every remaining action", [
  L.nextLine("batch"), L.nextLine("claim", { next: "in 3 days" }), L.nextLine("cancel"),
  L.nextLine("irrevocable"), L.nextLine("signal"), L.nextLine("transfer"), L.nextLine("connect")
], [
  "The schedules are live. Send each person their schedule code, or your LNS name.",
  "The tokens are in your account. Come back when more unlocks (in 3 days).",
  "Nothing more unlocks. What had already unlocked stays theirs to claim.",
  "Nobody can cancel it any more; it runs to the end.",
  "They can claim that part now.",
  "The person receiving claims from now on; send them the schedule code.",
  "Connected. Your schedules are below; if someone sent you one, paste its schedule code."
]);
check("writeWhat: the other write names", [L.writeWhat("irrevocable:3"), L.writeWhat("transfer:3")], ["making it permanent", "the transfer"]);
check("an unknown kind reads as linear in words and on the timeline", [
  L.kindExplain("x"), L.timeline("x")
], [L.kindExplain("linear"), L.timeline("linear")]);

// Accounts and amounts at the edges not covered above.
check("accountKind: empty, upper-case hex, a dotted name, a prefixed hex id", [
  L.accountKind(""), L.accountKind(null), L.accountKind(HEXID.toUpperCase()),
  L.accountKind("alice.name"), L.accountKind("Public/" + HEXID)
], ["", "", "hex", "name", ""]);
check("base58Of: empty and all-zero accounts", [L.base58Of(""), L.base58Of("0".repeat(64))], ["", "1".repeat(32)]);
check("fromModule: a null row", [L.fromModule(null).kind, L.fromModule(null).tranches], ["", []]);
const BIGAMT = "123456789012345678901234567890";
check("toBase: zero decimals, a large amount, spaces and commas", [
  L.toBase("0.5", 0), L.toBase(BIGAMT), L.toBase(" 1,000.5 ")
], ["", BIGAMT + "000000000", "1000500000000"]);

// The last one-line formatting helpers: empty input and the default token.
check("grouped, amountText and short: empty and boundary lengths", [
  L.grouped(null), L.grouped(""), L.grouped("1234567"),
  L.amountText("1000", ""), L.amountText("1000", "USDC"),
  L.short(null), L.short("12345678901234"), L.short("123456789012345")
], ["null", "", "1,234,567", "1,000 LEZ", "1,000 USDC", "null", "12345678901234", "123456…2345"]);
check("parseDate: empty and null are not a date", [
  Number.isNaN(L.parseDate("")), Number.isNaN(L.parseDate(null)), L.parseDate(" 2026-11-23 ")
], [true, true, Date.UTC(2026, 10, 23)]);
check("hostOf: no scheme, a port, nothing", [
  L.hostOf("lez.nameswallet.app"), L.hostOf("https://x:8080/a"), L.hostOf(null)
], ["lez.nameswallet.app", "x:8080", ""]);
check("resolvedLines: none and several", [
  L.resolvedLines([]), L.resolvedLines([{ name: "a", base58: "B" }, { name: "c", base58: "D" }])
], [[], ["a resolves to B.", "c resolves to D."]]);
check("zoneProblem: no zone or no chain to compare", [
  L.zoneProblem(null, "2da7"), L.zoneProblem({ zone: { channel: "2da7" } }, ""),
  L.zoneProblem({ zone: { channel: "2DA7" } }, "2da7")
], ["", "", ""]);


// QML's JavaScript engine (V4) has no BigInt: a BigInt literal makes the whole file fail to load in Basecamp.
{
  const src = readFileSync(new URL("./qml/logic.mjs", import.meta.url), "utf8").replace(/\/\/.*$/gm, "");
  check("logic.mjs has no BigInt literals (QML can't parse them)", /\b\d+n\b/.test(src), false);
}

// QML's engine reserves the old Java-style words: a variable named like these stops the whole file loading in Basecamp.
{
  const reservedNames = ["short", "int", "long", "byte", "char", "float", "double", "boolean", "native", "transient", "volatile", "synchronized", "throws", "goto", "abstract", "final"];
  for (const f of ["./qml/Main.qml", "./qml/logic.mjs", "./qml/NamesWalletSDK.mjs"]) {
    const src = readFileSync(new URL(f, import.meta.url), "utf8").replace(/\/\/.*$/gm, "").replace(/"(\\.|[^"\\])*"/g, '""');
    const bad = reservedNames.filter(w => new RegExp("\\b(var|let|const)\\s+" + w + "\\b").test(src));
    check("no reserved word used as a name in " + f, bad, []);
  }
}
// ── Local date-and-time picker (monthGrid, atLocal, partsOf, dateTimeText) ────────────
check("monthGrid: February 2027 starts on Monday with 28 real days", (() => {
  const g = L.monthGrid(2027, 1);
  return [g.length, g[0].day, g[0].inMonth, new Date(g[0].date).getDay(),
    g.filter((c) => c.inMonth).length, g[41].day, g[41].inMonth];
})(), [42, 1, true, 1, 28, 14, false]);
check("monthGrid: a 31-day month starting on Sunday (March 2026)", (() => {
  const g = L.monthGrid(2026, 2);
  return [g[0].day, g[0].inMonth, g[6].day, g[6].inMonth,
    g.filter((c) => c.inMonth).length, g[41].day, g[41].inMonth];
})(), [23, false, 1, true, 31, 5, false]);
check("monthGrid: days stay local midnight across a daylight-saving change", (() => {
  const g = L.monthGrid(2026, 2);
  const at = (day) => g.filter((c) => c.inMonth && c.day === day)[0];
  return [7, 8, 9].map((day) => {
    const d = new Date(at(day).date);
    return [L.partsOf(at(day).date).day, d.getHours(), d.getMinutes()];
  });
})(), [[7, 0, 0], [8, 0, 0], [9, 0, 0]]);
check("atLocal and partsOf round-trip a local moment", L.partsOf(L.atLocal(2026, 10, 21, 9, 5)),
  { year: 2026, month: 10, day: 21, hour: 9, minute: 5 });
{
  const text = L.dateTimeText(L.atLocal(2026, 10, 21, 9, 0));
  check("dateTimeText: a full moment and an empty field",
    [/^Sat 21 Nov 2026 · 09:00 \(your time(, .+)?\)$/.test(text), L.dateTimeText(-1), L.dateTimeText(null)],
    [true, "", ""]);
}

// QML's int is 32-bit: a time in milliseconds held in one wraps to 1970. Times must be real.
{
  const src = readFileSync(new URL("./qml/kit/DateTimeField.qml", import.meta.url), "utf8") + readFileSync(new URL("./qml/Main.qml", import.meta.url), "utf8");
  const bad = (src.match(/property int (ms|picked|today|[a-zA-Z]*(Ms|Time))\b/g) || []);
  check("no time in milliseconds is held in a 32-bit int property", bad, []);
}
