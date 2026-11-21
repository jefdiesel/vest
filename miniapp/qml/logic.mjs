// The Vesting mini-app's own logic, kept apart from the QML so the Node test
// (../logic.test.mjs) runs the same code the app does. The QML engine imports this
// as an ECMAScript module: `import "logic.mjs" as Logic`.
//
// The data comes from the `vesting` core module (../module): schedules, wallet accounts, the
// pre-claim summary with the simulated fee. Everything the view does with it — adapting the
// module's rows, formatting, the vesting maths for display, validation, next-unlock text, the
// fee check, the privacy disclosure and the plain result lines — lives here, tested in Node.

import * as SDK from "./NamesWalletSDK.mjs"

/// The three schedule kinds the creator can choose.
export const KINDS = ["cliff-linear", "linear", "milestone"]

/// Native LEZ, the token of a schedule whose token is the zero account.
export const TOKEN = "LEZ"
const ZERO = "0000000000000000000000000000000000000000000000000000000000000000"

const MINUTE = 60 * 1000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
const BASE58 = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/
const HEX = /^[0-9a-fA-F]{64}$/
// An LNS name as people type it (the module normalises and resolves it through the registry).
const NAME = /^[a-z0-9][a-z0-9._-]{0,62}$/i

function num(v) {
    var n = Number(v)
    return isFinite(n) ? n : 0
}

// ── Formatting ──────────────────────────────────────────────────────────────────────

/// 2000000 → "2,000,000".
export function grouped(digits) {
    var s = String(digits)
    return /^\d+$/.test(s) ? s.replace(/\B(?=(\d{3})+(?!\d))/g, ",") : s
}

/// "1,000,000 LEZ" — amounts are decimal text, never floats.
export function amountText(amount, token) {
    return grouped(amount) + " " + (token || TOKEN)
}

/// `abcdef…wxyz`.
export function short(text) {
    var s = String(text)
    return s.length <= 14 ? s : s.substr(0, 6) + "…" + s.substr(s.length - 4)
}

/// Epoch ms → "2026-11-23" (UTC).
export function dateText(ms) {
    if (ms === null || ms === undefined || ms === "" || !isFinite(Number(ms))) return ""
    var d = new Date(Number(ms))
    return isNaN(d.getTime()) ? "" : d.toISOString().slice(0, 10)
}

/// "yyyy-mm-dd" → epoch ms (UTC), or NaN.
export function parseDate(text) {
    return Date.parse(String(text || "").trim() + "T00:00:00Z")
}

// ── Local date and time picker ───────────────────────────────────────────────────────
// All of these use the local Date constructors. Adding 86400000 per day skips or repeats a day
// at a daylight-saving change, so the grid always builds dates from (year, month, day).

/// 42 cells (six weeks), Monday first: {day, inMonth, date} where date is local midnight in ms.
export function monthGrid(year, month) {
    var first = new Date(year, month, 1)
    var back = (first.getDay() + 6) % 7
    var start = new Date(year, month, 1 - back)
    var cells = []
    for (var i = 0; i < 42; i++)
        cells.push(new Date(start.getFullYear(), start.getMonth(), start.getDate() + i))
    return cells.map(function(d) {
        return { day: d.getDate(), inMonth: d.getMonth() === month, date: d.getTime() }
    })
}

/// Local midnight-plus-time as epoch ms.
export function atLocal(year, month, day, hour, minute) {
    return new Date(year, month, day, num(hour), num(minute), 0, 0).getTime()
}

/// The local parts of an epoch-ms moment.
export function partsOf(ms) {
    var d = new Date(ms)
    return { year: d.getFullYear(), month: d.getMonth(), day: d.getDate(),
        hour: d.getHours(), minute: d.getMinutes() }
}

/// The browser's short time-zone name, or "" when it cannot say.
export function timeZoneName() {
    // From the clock offset: Basecamp's engine ignores the timeZoneName option (it gave "PM").
    var off = -new Date().getTimezoneOffset()
    if (!isFinite(off)) return ""
    if (off === 0) return "UTC"
    var h = Math.floor(Math.abs(off) / 60), m = Math.abs(off) % 60
    return "UTC" + (off < 0 ? "−" : "+") + h + (m ? ":" + (m < 10 ? "0" : "") + m : "")
}

/// "Sat 21 Nov 2026 · 09:00 (your time, EST)" — "" when there is no moment yet.
export function dateTimeText(ms) {
    if (ms === null || ms === undefined || ms < 0 || !isFinite(Number(ms))) return ""
    var d = new Date(ms)
    var week = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
    var months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
    function two(n) { return (n < 10 ? "0" : "") + n }
    var zone = timeZoneName()
    return week[d.getDay()] + " " + d.getDate() + " " + months[d.getMonth()] + " " + d.getFullYear()
        + " · " + two(d.getHours()) + ":" + two(d.getMinutes())
        + " (your time" + (zone ? ", " + zone : "") + ")"
}

/// "in 3 days", "in 5 h", "in 2 min", "now". A positive duration from `now`.
export function inText(ms, now) {
    ms = num(ms)
    if (ms <= 0) return "now"
    // Round up once, to the unit shown, then carry: never "60 s", "60 min" or "24 h".
    if (ms < MINUTE - 999) return "in " + Math.ceil(ms / 1000) + " s"
    var mins = Math.ceil(ms / MINUTE)
    if (mins < 60) return "in " + mins + " min"
    if (mins < 24 * 60) {
        var h = Math.floor(mins / 60), m = mins % 60
        return "in " + h + " h" + (m > 0 ? " " + m + " min" : "")
    }
    var hours = Math.round(ms / HOUR), d = Math.floor(hours / 24), rh = hours % 24
    return "in " + d + " day" + (d !== 1 ? "s" : "") + (rh > 0 ? " " + rh + " h" : "")
}

// ── Exact integer arithmetic for amounts ────────────────────────────────────────────
// vesting-core uses u128 amounts and an exact floor(total*(t-start)/(end-start)). Amounts
// stay decimal strings here too. BigInt is used where the engine provides it (Node tests);
// a limb-based decimal fallback gives identical results in QML, whose JS engine has no
// BigInt. No float ever touches an amount.

function norm(a) { return String(a == null ? "" : a).replace(/^0+/, "") || "0" }

export const HAS_BIGINT = typeof BigInt === "function"

/// Compare decimal strings: -1, 0, 1. Works for the full u128 range.
export function cmpDec(a, b) {
    a = norm(a); b = norm(b)
    if (a.length !== b.length) return a.length < b.length ? -1 : 1
    return a < b ? -1 : a > b ? 1 : 0
}

/// Whether the decimal `a` is at least `b`.
export function atLeast(a, b) { return cmpDec(a, b) >= 0 }

/// Whether the decimal `a` is more than `b`.
export function above(a, b) { return cmpDec(a, b) > 0 }

const LIMB = 10000000
const LIMB_DIGITS = 7

function limbs(s) {
    s = norm(s)
    var out = []
    for (var i = s.length; i > 0; i -= LIMB_DIGITS)
        out.push(parseInt(s.slice(Math.max(0, i - LIMB_DIGITS), i), 10))
    while (out.length > 0 && out[out.length - 1] === 0) out.pop()
    return out
}
function fromLimbs(a) {
    if (a.length === 0) return "0"
    var s = String(a[a.length - 1])
    for (var i = a.length - 2; i >= 0; i--) s += ("0000000" + a[i]).slice(-LIMB_DIGITS)
    return s
}
function cmpLimbs(a, b) {
    if (a.length !== b.length) return a.length < b.length ? -1 : 1
    for (var i = a.length - 1; i >= 0; i--) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1
    return 0
}
function addLimbs(a, b) {
    var out = [], carry = 0, n = Math.max(a.length, b.length)
    for (var i = 0; i < n; i++) {
        var v = (a[i] || 0) + (b[i] || 0) + carry
        out.push(v % LIMB); carry = Math.floor(v / LIMB)
    }
    if (carry > 0) out.push(carry)
    return out
}
function subLimbs(a, b) { // a >= b
    var out = [], borrow = 0
    for (var i = 0; i < a.length; i++) {
        var v = a[i] - (b[i] || 0) - borrow
        if (v < 0) { v += LIMB; borrow = 1 } else borrow = 0
        out.push(v)
    }
    while (out.length > 0 && out[out.length - 1] === 0) out.pop()
    return out
}
function mulLimbs(a, b) {
    if (a.length === 0 || b.length === 0) return []
    var out = new Array(a.length + b.length).fill(0)
    for (var i = 0; i < a.length; i++) {
        var carry = 0
        for (var j = 0; j < b.length; j++) {
            var v = out[i + j] + a[i] * b[j] + carry
            out[i + j] = v % LIMB
            carry = Math.floor(v / LIMB)
        }
        var k = i + b.length
        while (carry > 0) {
            var v2 = out[k] + carry
            out[k] = v2 % LIMB
            carry = Math.floor(v2 / LIMB)
            k++
        }
    }
    while (out.length > 0 && out[out.length - 1] === 0) out.pop()
    return out
}
function divModLimbs(a, b) { // b > 0; returns [quotient, remainder]
    if (cmpLimbs(a, b) < 0) return [[], a]
    var q = new Array(a.length).fill(0), rem = []
    for (var i = a.length - 1; i >= 0; i--) {
        rem.unshift(a[i])
        while (rem.length > 0 && rem[rem.length - 1] === 0) rem.pop()
        var lo = 0, hi = LIMB - 1
        while (lo < hi) {
            var mid = Math.ceil((lo + hi) / 2)
            if (cmpLimbs(mulLimbs(b, [mid]), rem) <= 0) lo = mid; else hi = mid - 1
        }
        q[i] = lo
        if (lo > 0) rem = subLimbs(rem, mulLimbs(b, [lo]))
    }
    while (q.length > 0 && q[q.length - 1] === 0) q.pop()
    return [q, rem]
}

/// Exact `a - b` for decimal strings (a >= b).
export function decSub(a, b) { return fromLimbs(subLimbs(limbs(a), limbs(b))) }

/// Floor of `a * b / c` with exact integer arithmetic. `a` is an amount string; `b` and
/// `c` are integer time deltas (strings keep u64 times exact). Mirrors core::math::mul_div_floor.
export function mulDivFloor(a, b, c) {
    if (norm(c) === "0" || norm(a) === "0" || norm(b) === "0") return "0"
    if (HAS_BIGINT) return (BigInt(norm(a)) * BigInt(norm(b)) / BigInt(norm(c))).toString()
    return mulDivFloorString(a, b, c)
}

/// The same result without BigInt — for QML and for cross-checking in the tests.
export function mulDivFloorString(a, b, c) {
    if (norm(c) === "0" || norm(a) === "0" || norm(b) === "0") return "0"
    return fromLimbs(divModLimbs(mulLimbs(limbs(a), limbs(b)), limbs(c))[0])
}

/// Exact sum of decimal amount strings.
export function sumAmounts(values) {
    var i, acc
    if (HAS_BIGINT) {
        acc = BigInt(0)
        for (i = 0; i < values.length; i++) acc += BigInt(norm(values[i]))
        return acc.toString()
    }
    acc = []
    for (i = 0; i < values.length; i++) acc = addLimbs(acc, limbs(values[i]))
    return fromLimbs(acc)
}

// ── Vesting maths, for display ──────────────────────────────────────────────────────

/// Total vested amount at time `t`, as a decimal string, never below 0 or above the total.
///
/// Exactly vesting-core::vested_at. For a cancelled schedule the amount is frozen at the
/// cancellation record (when the backend supplies `vestedAtCancel`). cliff-linear: nothing
/// until the cliff, then the accrued lump and linear to the end. milestone: the exact sum
/// of the tranches the creator has signalled.
export function vested(s, t) {
    s = s || {}
    var total = norm(s.total)
    if (s.cancelled && typeof s.vestedAtCancel === "string") return norm(s.vestedAtCancel)
    var v = "0"
    if (s.kind === "milestone") {
        var tr = s.tranches || [], sg = s.signalled || [], picked = []
        for (var i = 0; i < tr.length; i++) if (sg[i]) picked.push(tr[i])
        v = sumAmounts(picked)
    } else {
        var start = norm(s.start), end = norm(s.end), cliff = norm(s.cliff || 0), now = norm(t)
        if (s.kind === "cliff-linear" && cmpDec(now, cliff) < 0) v = "0"
        else if (cmpDec(now, start) <= 0) v = "0"
        else if (cmpDec(now, end) >= 0) v = total
        else v = mulDivFloor(total, decSub(now, start), decSub(end, start))
    }
    return cmpDec(v, total) > 0 ? total : norm(v)
}

/// Amount claimable right now: vested minus what the beneficiary has already claimed.
export function claimable(s, t) {
    var v = vested(s, t), c = norm(s && s.claimed)
    return cmpDec(v, c) <= 0 ? "0" : decSub(v, c)
}

/// Still locked: the part of the total not yet vested.
export function locked(s, t) {
    var v = vested(s, t), total = norm(s && s.total)
    return cmpDec(total, v) <= 0 ? "0" : decSub(total, v)
}

/// Next unlock event: { at: epoch ms or 0, text: "… on 2026-11-23 — in 3 days." }.
export function nextUnlock(s, now) {
    s = s || {}
    if (s.kind === "milestone") {
        var sg = s.signalled || [], n = Math.max((s.tranches || []).length, sg.length), i = 0
        while (i < n && sg[i]) i++
        if (i >= n) return { at: 0, text: "All milestones unlocked." }
        return { at: 0, text: "Next unlock: milestone " + (i + 1) + " — when the creator says it's reached." }
    }
    if (s.kind === "cliff-linear" && cmpDec(now, s.cliff) < 0)
        return { at: num(s.cliff), text: "Cliff on " + dateText(s.cliff) + " — " + inText(num(s.cliff) - now, now) + "." }
    if (cmpDec(now, s.end) < 0)
        return { at: num(s.end), text: "Fully unlocked on " + dateText(s.end) + " — " + inText(num(s.end) - now, now) + "." }
    return { at: 0, text: "Fully unlocked — nothing left to unlock." }
}

// ── Module replies ───────────────────────────────────────────────────────────────────

/// A reply of `logos.callModule(Async)("vesting", ...)` as `{ok, error, ...}`. The host may hand
/// the module's JSON text encoded once or twice; no reply, or the host's own words for a module
/// that isn't there, become a plain error (`missing: true` for the latter).
export function reply(raw) {
    var parsed = raw
    for (var depth = 0; depth < 2 && typeof parsed === "string"; depth++) {
        try { parsed = JSON.parse(parsed) } catch (error) { break }
    }
    if (parsed && typeof parsed === "object" && typeof parsed.ok === "boolean") return parsed
    // The host's own refusal: {"error":"Call refused","message":"expected integer at arg0, …"}.
    if (parsed && typeof parsed === "object" && parsed.error) {
        var why = String(parsed.message || parsed.error)
        var what = parsed.method ? " (" + parsed.method + ")" : ""
        if (/module not connected|module not reachable|not loaded|is not installed|module not found/i.test(why + " " + parsed.error))
            return { ok: false, missing: true, error: "The Vesting module isn't installed or isn't running: " + why + "." }
        return { ok: false, error: "Basecamp refused the call" + what + ": " + why }
    }
    var said = typeof raw === "string" ? raw : JSON.stringify(raw)
    if (/module not connected|module not reachable|no such (module|method)|unknown (module|method)|method not found|module not found|not loaded|is not installed/i.test(said || ""))
        return { ok: false, missing: true, error: "The Vesting module isn't installed or isn't running (" + said + "). Install it and reopen the app." }
    return { ok: false, error: "No answer from the Vesting module (it may still be busy, e.g. proving a private claim); try again." }
}

// ── Module data ──────────────────────────────────────────────────────────────────────

/// A schedule row of the module's `list` as the view uses it: the kind as a string, its times
/// and tranches flat, and the module's ready display strings kept.
export function fromModule(row) {
    row = row || {}
    var k = row.kind || {}
    var out = {}
    var keys = Object.keys(row)
    for (var i = 0; i < keys.length; i++) out[keys[i]] = row[keys[i]]
    out.kind = k.type || ""
    out.start = k.start || 0
    out.cliff = k.cliff || 0
    out.end = k.end || 0
    out.tranches = k.tranches || []
    out.signalled = row.signalled || []
    out.mine = row.mine || {}
    out.tokenLabel = isNative(row.token) ? TOKEN : short(row.token)
    return out
}

/// Whether a token id is native LEZ (the zero account, or empty).
export function isNative(token) {
    var t = String(token == null ? "" : token).trim()
    return t === "" || t === ZERO || t.toUpperCase() === TOKEN
}

/// The module's `create` row for one schedule of the view's form `p` by `creator`.
export function toCreateRow(p, creator) {
    var kind = { type: p.kind }
    if (p.kind === "milestone") kind.tranches = (p.tranches || []).map(String)
    else {
        kind.start = num(p.start)
        kind.end = num(p.end)
        if (p.kind === "cliff-linear") kind.cliff = num(p.cliff)
    }
    var total = p.kind === "milestone" ? sumAmounts(kind.tranches) : String(p.amount)
    return {
        creator: creator, beneficiary: String(p.beneficiary).trim(),
        token: isNative(p.token) ? TOKEN : String(p.token).trim(),
        total: total, kind: kind,
        cancelable: !!p.cancelable, transferable: !!p.transferable,
        cancel_authority: p.cancelAuthority ? String(p.cancelAuthority).trim() : null,
        milestone_authority: p.milestoneAuthority ? String(p.milestoneAuthority).trim() : null
    }
}

// ── Accounts and creation validation ────────────────────────────────────────────────

/// What kind of account text this is: "hex", "base58", "name" or "" (not an account).
export function accountKind(text) {
    var s = String(text == null ? "" : text).trim()
    var m = /^(Public|Private)\/(.+)$/.exec(s)
    if (m) return BASE58.test(m[2]) ? "base58" : ""
    if (HEX.test(s)) return "hex"
    if (BASE58.test(s) && s.length >= 32) return "base58"
    if (NAME.test(s)) return "name"
    return ""
}

/// Why `text` can't be sent to the module as an account (an address or an LNS name), or "".
export function problemAccount(text) {
    var s = String(text == null ? "" : text).trim()
    if (s === "") return "Enter an address or an LNS name."
    if (accountKind(s) === "") return "That is not an address or an LNS name."
    return ""
}

/// The name the module's name book shows for an account ({hex: name}), or "".
export function nameOf(account, names) {
    var map = names || {}
    return map[String(account == null ? "" : account).trim().toLowerCase()] || ""
}

/// "alice · 0a1b2c…7f8e" for a known name, else just the short account.
export function labelled(account, names) {
    var name = nameOf(account, names)
    var shown = short(base58Of(account))
    return name ? name + " · " + shown : shown
}

var B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

/// A 64-hex account as base58 (how Names and the network show accounts); anything else as is.
export function base58Of(account) {
    var s = String(account == null ? "" : account)
    if (!/^[0-9a-fA-F]{64}$/.test(s)) return s
    // Plain-number long division over the bytes: QML's JavaScript engine has no BigInt.
    var bytes = []
    for (var i = 0; i < 64; i += 2) bytes.push(parseInt(s.substr(i, 2), 16))
    var digits = []
    for (var b = 0; b < bytes.length; b++) {
        var carry = bytes[b]
        for (var d = 0; d < digits.length; d++) { carry += digits[d] * 256; digits[d] = carry % 58; carry = Math.floor(carry / 58) }
        while (carry > 0) { digits.push(carry % 58); carry = Math.floor(carry / 58) }
    }
    var out = ""
    for (var k = digits.length - 1; k >= 0; k--) out += B58[digits[k]]
    for (var z = 0; z < bytes.length && bytes[z] === 0; z++) out = "1" + out
    return out
}

/// What the user typed, checked for shape only: {ok, account, error}. The module resolves
/// names through the LNS registry and says what each one resolved to.
export function accountInput(text) {
    var typed = String(text == null ? "" : text).trim()
    var problem = problemAccount(typed)
    return problem === "" ? { ok: true, account: typed, error: "" } : { ok: false, account: "", error: problem }
}

/// "alice resolves to 0a1b…" lines for the names a module answer resolved.
export function resolvedLines(resolved) {
    return (resolved || []).map(function(r) { return r.name + " resolves to " + r.base58 + "." })
}

/// Validate every parameter of a new schedule: { ok, errors: [...] }.
/// `p.start`, `p.cliff`, `p.end` are epoch ms; `p.tranches` is an array of amount strings.
export function validateCreate(p) {
    p = p || {}
    var e = []
    if (KINDS.indexOf(p.kind) < 0) e.push("Choose a schedule kind.")
    if (problemAccount(p.beneficiary) !== "") e.push("Who receives it: " + problemAccount(p.beneficiary))
    if (tokenProblem(p.token)) e.push(tokenProblem(p.token))
    if (p.kind !== "milestone") {
        if (!/^\d+$/.test(String(p.amount == null ? "" : p.amount)) || num(p.amount) <= 0) e.push("Amount must be a positive whole number.")
        e = e.concat(dateProblems(p))
    }
    if (p.kind === "milestone") {
        var tr = p.tranches || []
        if (!Array.isArray(tr) || tr.length === 0) e.push("Add at least one milestone amount.")
        else for (var i = 0; i < tr.length; i++)
            if (!/^\d+$/.test(String(tr[i] == null ? "" : tr[i])) || num(tr[i]) <= 0) {
                e.push("Milestone " + (i + 1) + " must be a positive whole number.")
                break
            }
    }
    if (p.cancelAuthority && problemAccount(p.cancelAuthority) !== "") e.push("Cancel authority: " + problemAccount(p.cancelAuthority))
    if (p.milestoneAuthority && problemAccount(p.milestoneAuthority) !== "") e.push("Milestone authority: " + problemAccount(p.milestoneAuthority))
    return { ok: e.length === 0, errors: e }
}

/// "LEZ" (native) or a token definition account; why not, or "".
function tokenProblem(token) {
    var t = String(token == null ? "" : token).trim()
    if (isNative(t)) return ""
    var k = accountKind(t)
    return k === "hex" || k === "base58" ? "" : "Enter LEZ or a token's address."
}

function dateProblems(p) {
    var e = [], start = num(p.start), end = num(p.end), cliff = num(p.cliff || 0)
    if (!isFinite(Number(p.start)) || start <= 0) e.push("Enter a valid start date.")
    if (!isFinite(Number(p.end)) || end <= start) e.push("End date must be after the start date.")
    if (p.kind === "cliff-linear" && (!isFinite(Number(p.cliff)) || cliff <= start || cliff >= end))
        e.push("Cliff date must be between the start and end dates.")
    return e
}

// ── Pre-claim confirmation: fee, privacy, result lines ──────────────────────────────

// ── Plain words for the screen ────────────────────────────────────────────────────────

/// Native LEZ has 9 decimals.
export const DECIMALS = 9

/// "0.5" LEZ → "500000000" (base units), or "" when it isn't a positive amount with at most
/// `decimals` decimals. Exact: works on the digits, never on floats.
export function toBase(text, decimals) {
    var d = decimals === undefined ? DECIMALS : decimals
    var s = String(text == null ? "" : text).trim().replace(/,/g, "")
    var m = /^(\d*)(?:\.(\d*))?$/.exec(s)
    if (!m || (m[1] === "" && (m[2] || "") === "")) return ""
    var frac = m[2] || ""
    if (frac.length > d) return ""
    while (frac.length < d) frac += "0"
    var out = ((m[1] || "") + frac).replace(/^0+/, "")
    return out === "" ? "" : out
}

/// Fully claimed: everything was paid to the beneficiary.
export function isFullyClaimed(s) {
    s = s || {}
    return String(s.total || "") !== "" && norm(s.claimed) === norm(s.total)
}

/// Nothing changes any more: cancelled or fully claimed. (A cancelled schedule may still hold
/// what had vested by the cancel: the program keeps that claimable by the beneficiary.)
export function isClosed(s) {
    s = s || {}
    return !!s.cancelled || isFullyClaimed(s)
}

/// Nothing left to do for anyone: closed, and nothing claimable.
export function isFinished(s) {
    s = s || {}
    return isClosed(s) && !above(s.claimable || "0", "0")
}

/// The words for a closed schedule: its status line, and the list row's right side.
export function statusLine(s) {
    s = s || {}
    if (s.cancelled)
        return "Cancelled" + (s.cancelled_at ? " on " + dateText(s.cancelled_at) : "") + ". "
            + (s.refund_display || "The amount still locked") + " went back to the creator."
    if (isFullyClaimed(s)) return "Fully claimed."
    return ""
}

export function rowState(s) {
    s = s || {}
    if (s.cancelled) return above(s.claimable || "0", "0") ? "Cancelled · " + s.claimable_display + " to claim" : "Cancelled"
    if (isFullyClaimed(s)) return "Fully claimed"
    return (s.claimable_display || "0 LEZ") + " to claim"
}

/// The confirm sheet's words for a step that can't be undone.
export function confirmText(kind, s) {
    s = s || {}
    if (kind === "cancel")
        return "Cancel schedule " + s.id + "? The locked amount " + (s.locked_display || "") + " goes back to you. This can't be undone."
    if (kind === "irrevocable")
        return "Make schedule " + s.id + " irrevocable? Nobody can cancel it after this. This can't be undone."
    return ""
}

/// "Schedule 3 · evenly · 1.5 LEZ".
export function scheduleTitle(s) {
    s = s || {}
    var kinds = { "linear": "evenly", "cliff-linear": "cliff, then evenly", "milestone": "milestones" }
    return "Schedule " + s.id + " · " + (kinds[s.kind] || s.kind) + " · " + (s.total_display || "")
}

/// Text from outside (a network's name, a reason) made safe to show: no control or bidi
/// characters, at most 200 characters.
export function clean(text) {
    return String(text == null ? "" : text).replace(/[\u0000-\u001f\u007f‎‏‪-‮⁦-⁩]/g, "").slice(0, 200)
}

/// "https://lez.nameswallet.app/" → "lez.nameswallet.app".
export function hostOf(url) {
    var m = /^[a-z]+:\/\/([^\/?#]+)/i.exec(String(url == null ? "" : url))
    return m ? m[1] : String(url == null ? "" : url)
}

// ── Who gets paid ───────────────────────────────────────────────────────────────────

/// What the payout box resolved to, as the line under it: `resolve_name`'s answer `{ok, name,
/// base58}` or its error. `{ok, text}`; ok false means Vest can't pay what was typed.
export function payeeLine(typed, r) {
    var t = String(typed == null ? "" : typed).trim()
    if (r && r.ok && r.base58) {
        var addr = short(r.base58)
        return { ok: true, base58: r.base58, name: r.name || "", text: r.name ? "Pays " + clean(r.name) + " · " + addr : "Pays the address " + addr + "." }
    }
    var why = String((r && r.error) || "")
    if (/unknown LNS name|not registered|no such name/i.test(why)) return { ok: false, text: "No one holds the LNS name " + clean(t) + ". Check the spelling, or paste their address." }
    if (/no account in its record|publishes no account|has no owner/i.test(why)) return { ok: false, text: "Can't pay " + clean(t) + " by name yet: its record is missing its account (a Names Wallet bug being fixed). Paste their address for now." }
    if (/rate|429|busy|timed out|network|connect/i.test(why)) return { ok: false, retry: true, text: "Couldn't check " + clean(t) + " just now: " + clean(why) }
    return { ok: false, text: "That isn't an address or an LNS name Vest can find" + (why ? ": " + clean(why) : ".") }
}

// ── Names Wallet ────────────────────────────────────────────────────────────────────
// The calls, retries, states and request words are the Names Wallet SDK's (NamesWalletSDK.mjs,
// copied unchanged from the Names repository); Vest keeps only its own words here.

export const requestLine = SDK.requestLine
export const requestOpen = SDK.requestOpen

/// What Vest says when Names Wallet is on another network than Vest's.
export const OTHER_ZONE = "Names Wallet is on another network. Switch Names Wallet to the bridge test network (in Names Wallet: Settings, Network), then come back."

/// The `get_config` zone check: "" when Names Wallet is on `chainId`, else what to do.
export function zoneProblem(config, chainId) {
    return SDK.zoneMatches(config, chainId) ? "" : OTHER_ZONE
}

/// What the payout box resolved to through Names Wallet (the SDK's `resolve`), as the line
/// under it: `{ok, text}`, and for a name found `{via: "names", typed, account, resolved}`, the
/// account the schedule is then made for.
export function payeeFromNames(typed, r) {
    var t = String(typed == null ? "" : typed).trim()
    var x = r || {}
    if (x.ok && x.account) {
        var asked = x.forwardedFrom || x.name
        return {
            ok: true, via: "names", typed: t, account: x.account,
            resolved: [{ name: asked, base58: x.account }],
            text: x.forwardedFrom ? "Pays " + clean(x.forwardedFrom) + " (it points to " + clean(x.name) + ") · " + short(x.account)
                                  : "Pays " + clean(x.name) + " · " + short(x.account)
        }
    }
    if (x.code === "not_registered") return { ok: false, text: "No one holds the LNS name " + clean(t) + ". Check the spelling, or paste their address." }
    if (x.code === "no_account") return { ok: false, text: "Can't pay " + clean(t) + " by name: its LNS record names no account to pay. Paste their address." }
    if (x.code === "unavailable" || x.code === "missing") return { ok: false, retry: true, text: "Couldn't check " + clean(t) + " just now: " + clean(x.error || "") }
    return { ok: false, text: "That isn't an address or an LNS name Vest can find" + (x.error ? ": " + clean(x.error) : ".") }
}

/// The account a schedule is made for when Names Wallet looked up what is in the payout box
/// (`payee` from payeeFromNames, still for `typed`): "Public/<id>", else "".
export function payeeAccount(payee, typed) {
    var p = payee || {}
    return p.ok && p.via === "names" && p.account && p.typed === String(typed == null ? "" : typed).trim() ? "Public/" + p.account : ""
}

/// Names `granted_accounts` + `account_balances` as the vesting module's `set_accounts` input.
export function namesAccounts(granted, balances) {
    var b = {}
    ;(balances || []).forEach(function(x) { b[x.account] = x.balance })
    return ((granted && granted.accounts) || []).map(function(a) {
        return { account: a.account, account_kind: a.account_kind, balance: b[a.account] || "0" }
    })
}

// ── Guides ──────────────────────────────────────────────────────────────────────────

/// The steps of each role, with the current one marked: `[{n, text, state}]`, state is
/// `done`, `now` or `later`. `at` is how far the person got (0-based index of the current step).
export function guideSteps(role, at) {
    var steps = role === "pay"
        ? ["Connect Names Wallet",
           "Choose who, how much and the schedule",
           "Approve it in Names Wallet",
           "It reaches them by itself",
           "Cancel it, or make it permanent, later if you need to"]
        : ["Connect Names Wallet",
           "Your schedules appear here by themselves",
           "See what you can claim now and when the rest unlocks",
           "Claim to your public account, or privately",
           "Approve it in Names Wallet"]
    var details = role === "pay"
        ? ["Names Wallet asks you which of your accounts Vest may see. You approve it in Names Wallet; Vest never sees a key.",
           "Below: the account that pays, the person (address or LNS name), the amount, and when it unlocks.",
           "Names Wallet shows the payment and its fee. Nothing leaves your account until you approve.",
           "It shows up in their Vest by itself, under Unvested or Claimable. If it doesn't, send them the schedule code (Copy schedule code): they add it under Unvested, Add a schedule by code.",
           "Cancel stops what hasn't unlocked and returns it to you. Permanent means nobody can cancel."]
        : ["Names Wallet asks you which of your accounts Vest may see. You approve it in Names Wallet; Vest never sees a key.",
           "Vest looks for schedules paying your accounts on the chain and lists them under Claimable or Unvested. If one is missing, add it with its schedule code under Unvested.",
           "Choose a schedule below: it shows what is claimable now, what is still locked, and the next unlock.",
           "Press Check claim. Vest shows the amount and where it goes before anything is sent.",
           "Names Wallet shows the claim and its fee. Approve it there; the tokens arrive in a few seconds."]
    return steps.map(function(text, i) {
        return { n: i + 1, text: text, detail: details[i], state: i < at ? "done" : i === at ? "now" : "later" }
    })
}

/// Where a receiver is in their guide: 0 connect, 1 schedules, 2 look, 3 claim, 4 approve.
export function receiveStep(s) {
    s = s || {}
    if (!s.connected) return 0
    if (!s.schedules) return 1
    if (s.approving) return 4
    if (s.claimable) return 3
    return 2
}

/// Where a payer is in their guide: 0 connect, 1 choose, 2 approve, 3 share, 4 later.
export function payStep(s) {
    s = s || {}
    if (!s.connected) return 0
    if (s.approving) return 2
    if (s.justCreated) return 3
    if (s.manages) return 4
    return 1
}

/// What each kind of schedule means, in one sentence.
export function kindExplain(kind) {
    if (kind === "cliff-linear") return "Nothing unlocks before the cliff date. At the cliff, everything earned so far unlocks at once; after that it keeps unlocking a little every moment until the end date."
    if (kind === "milestone") return "Each part unlocks when you (or the person you name) say a milestone was reached, not on a date."
    return "It unlocks a little every moment, evenly, from the start date to the end date."
}

/// The timeline drawing of a kind: segments from 0 to 1 across, each `{from, to, kind}` with
/// kind `locked`, `cliff` (a jump), `unlocking` or `steps` (milestones).
export function timeline(kind) {
    if (kind === "cliff-linear") return [{ from: 0, to: 0.3, kind: "locked" }, { from: 0.3, to: 0.3, kind: "cliff" }, { from: 0.3, to: 1, kind: "unlocking" }]
    if (kind === "milestone") return [{ from: 0, to: 0.25, kind: "locked" }, { from: 0.25, to: 0.6, kind: "steps" }, { from: 0.6, to: 1, kind: "steps" }]
    return [{ from: 0, to: 1, kind: "unlocking" }]
}

/// What happens next, after each action: one line for the Result line.
export function nextLine(action, info) {
    info = info || {}
    if (action === "create") return "Schedule " + info.id + " is live. It shows up in their Vest by itself. If it doesn't, send them its schedule code (Copy schedule code, below)."
    if (action === "batch") return "The schedules are live. Send each person their schedule code, or your LNS name."
    if (action === "claim") return "The tokens are in your account. Come back when more unlocks" + (info.next ? " (" + info.next + ")" : "") + "."
    if (action === "cancel") return "Nothing more unlocks. What had already unlocked stays theirs to claim."
    if (action === "irrevocable") return "Nobody can cancel it any more; it runs to the end."
    if (action === "signal") return "They can claim that part now."
    if (action === "transfer") return "The person receiving claims from now on; send them the schedule code."
    if (action === "import") return "It's in your list. Choose it to see what you can claim."
    if (action === "connect") return "Connected. Vest now looks for schedules paying your accounts; they show up under Claimable and Unvested."
    return ""
}

/// "" when the paying account holds enough for the schedule, else what to say. `balance` is the
/// account's LEZ in base units (text), `amount` the total in base units, `tranches` milestone parts.
/// Base units (9 decimals) as "1,111.5 LEZ".
export function lezText(base) {
    var d = norm(base)
    while (d.length < 10) d = "0" + d
    var whole = d.slice(0, d.length - 9), frac = d.slice(d.length - 9).replace(/0+$/, "")
    return grouped(whole) + (frac ? "." + frac : "") + " LEZ"
}

export function shortOfFunds(balance, amount, tranches) {
    if (balance === null || balance === undefined || balance === "") return ""
    var need = tranches && tranches.length ? sumAmounts(tranches.map(function(t) { return String(t || "0") })) : String(amount || "0")
    if (!above(need, String(balance))) return ""
    return "This schedule needs " + lezText(need) + ", and the paying account holds " + lezText(String(balance)) + ". Lower the amount, or add LEZ to the account first."
}

/// The words Names Wallet shows for a create: the LNS name the person typed, with the address.
export function createSummary(built, typed) {
    var name = String(typed == null ? "" : typed).trim()
    var base = String((built && built.summary) || "")
    if (name === "" || accountKind(name) !== "name") return base
    return "Create a Vest schedule: " + ((built && built.amount_display) || "") + " from this account, to " + clean(name) + (built && built.beneficiary_base58 ? " (" + short(built.beneficiary_base58) + ")" : "")
}

/// What to say when Add schedule was given something that isn't a schedule.
export function importProblem(typed, error) {
    var t = String(typed == null ? "" : typed).trim()
    var e = String(error || "")
    if (/is not a schedule of this Vest program/.test(e))
        return accountKind(t) === "name"
            ? clean(t) + " is a person's LNS name, not a schedule. Ask them for the schedule code (Vest shows it after they create it) and paste that here."
            : "That address isn't a Vest schedule. Ask the person paying you for the schedule code (Vest shows it after they create it) and paste that here."
    return e
}

/// The receipt shown after a create: what was made, between whom, on what plan, and the proof.
/// `r`: {id, amountDisplay, kind, start, cliff, end, cancelable, transferable, fromAddr, fromName,
/// toAddr, toName, tx, code}. Returns {title, rows: [{key, value, addr?}], share}.
export function receiptOf(r) {
    r = r || {}
    var kinds = { "linear": "Evenly", "cliff-linear": "Cliff, then evenly", "milestone": "Milestones" }
    var plan = (kinds[r.kind] || "Evenly") + (r.kind === "milestone" ? "" :
        ", from " + dateTimeText(r.start) + (r.kind === "cliff-linear" && r.cliff > 0 ? ", cliff " + dateTimeText(r.cliff) : "") + " to " + dateTimeText(r.end))
    var rows = [
        { key: "Amount", value: r.amountDisplay || "" },
        { key: "From", value: r.fromName ? clean(r.fromName) : short(r.fromAddr || ""), addr: r.fromAddr || "" },
        { key: "Receives", value: r.toName ? clean(r.toName) : short(r.toAddr || ""), addr: r.toAddr || "" },
        { key: "Plan", value: plan },
        { key: "Rules", value: (r.cancelable ? "You can cancel it" : "Permanent: nobody can cancel it") + " · " + (r.transferable ? "they can hand it on" : "they can't hand it on") },
        { key: "Transaction", value: r.tx ? short(r.tx) : "sent", addr: r.tx || "" },
        { key: "Schedule code", value: r.code ? short(r.code) : "", addr: r.code || "" }
    ]
    var share = "It shows up in their Vest by itself, under Unvested or Claimable. If it doesn't, send them the schedule code (Copy schedule code, below): they add it under Unvested."
    return { title: "Schedule " + (r.id !== undefined && r.id !== "" ? r.id + " " : "") + "created", rows: rows, share: share }
}

/// What a Names write is called in its status line ("the cancel"), from the view's step name.
export function writeWhat(what) {
    var kind = String(what || "").split(":")[0]
    var words = { create: "the new schedule", cancel: "the cancel", irrevocable: "making it permanent", signal: "the milestone", transfer: "the transfer" }
    return words[kind] || "the request"
}

/// The full guide (the repository's user guide).
export const GUIDE_URL = "https://github.com/jefdiesel/vest/blob/main/docs/USER-GUIDE.md"

/// Why paying out needs a wallet folder for now (Names Wallet's program calls can't carry it).
export const NAMES_CANT_PAY = "Names Wallet can approve claims, but a new schedule, a cancel or a milestone needs Names Wallet 0.18 or later: those also read the network's clock in the same step. Update Names Wallet, then come back. You can still read how it works below."

// ── Schedules paying you: the menu ──────────────────────────────────────────────────────

/// The left menu, in order.
export const MENU = [
    { key: "claimable", label: "Claimable" },
    { key: "unvested", label: "Unvested" },
    { key: "pay", label: "Paying out" },
    { key: "settings", label: "Settings" }
]

/// Where a schedule paying you belongs: "claimable" (something to claim now), "closed" (all
/// claimed, or cancelled with nothing left) or "unvested" (nothing now, more to come). The same
/// words as the module's incoming().
export function receiveCategory(s) {
    s = s || {}
    if (above(s.claimable || "0", "0")) return "claimable"
    if (s.cancelled || isFullyClaimed(s)) return "closed"
    return "unvested"
}

/// The schedules paying you that are in `category`.
export function paying(schedules, category) {
    return (schedules || []).filter(function(s) { return s && s.mine && s.mine.beneficiary && receiveCategory(s) === category })
}

/// The menu with a count badge on Claimable and Unvested: `[{key, label, count}]` (count -1: none).
export function menuItems(schedules) {
    return MENU.map(function(m) {
        var n = m.key === "claimable" || m.key === "unvested" ? paying(schedules, m.key).length : -1
        return { key: m.key, label: m.label, count: n }
    })
}

/// The first time more can be claimed (ms), or 0 when no date says so (milestones). The
/// module's `next_unlock` when it gave one; otherwise the start or the cliff still ahead.
export function nextClaimAt(s, now) {
    s = s || {}
    if (s.next_unlock !== undefined && s.next_unlock !== null) return num(s.next_unlock)
    if (s.kind === "milestone" || s.cancelled) return 0
    if (s.kind === "cliff-linear" && cmpDec(now, s.cliff) < 0) return num(s.cliff)
    if (cmpDec(now, s.start) < 0) return num(s.start)
    return 0
}

/// "Nothing to claim until Tue 1 Dec 2026 · 09:00 (your time)." for a schedule with nothing claimable yet.
export function untilLine(s, now) {
    s = s || {}
    if (s.kind === "milestone") return "Nothing to claim until the creator says the next milestone is reached."
    var at = nextClaimAt(s, now)
    return at > 0 ? "Nothing to claim until " + dateTimeText(at) + "." : "Nothing to claim yet."
}

/// Who pays: their LNS name when known, else the short address.
export function fromName(s, names) {
    s = s || {}
    return nameOf(s.creator, names) || short(base58Of(s.creator || ""))
}

/// An Unvested row: "Schedule 3 · 1,000 LEZ from alice" and "until 2026-12-01".
export function unvestedRow(s, names, now) {
    s = s || {}
    var at = nextClaimAt(s, now)
    return {
        label: "Schedule " + s.id + " · " + (s.total_display || "") + " from " + fromName(s, names),
        detail: s.kind === "milestone" ? "at the next milestone" : at > 0 ? "until " + dateText(at) : "soon"
    }
}

/// The quiet line while Vest reads the chain: "Looking for schedules on the chain… block X of Y".
/// `scan`: the last discover answer (`to_block`, `tip`, `done`) or null. "" once at the tip.
export function scanLine(scan) {
    if (!scan || scan.done) return ""
    var at = scan.to_block === null || scan.to_block === undefined ? scan.from_block : scan.to_block
    if (at === undefined || at === null || !scan.tip) return "Looking for schedules on the chain…"
    return "Looking for schedules on the chain… block " + grouped(String(at)) + " of " + grouped(String(scan.tip))
}

/// "New: 1 schedule from alice" for the schedules a scan just found (`[{creator}]`, hex).
export function newLine(found, names) {
    found = found || []
    if (!found.length) return ""
    var from = []
    found.forEach(function(f) {
        var who = nameOf(f.creator, names) || short(base58Of(f.creator || ""))
        if (from.indexOf(who) < 0) from.push(who)
    })
    var people = from.length === 1 ? from[0]
        : from.length <= 3 ? from.slice(0, -1).join(", ") + " and " + from[from.length - 1]
        : from.slice(0, 2).join(", ") + " and " + (from.length - 2) + " others"
    return "New: " + found.length + " schedule" + (found.length === 1 ? "" : "s") + " from " + clean(people)
}
