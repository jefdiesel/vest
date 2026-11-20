// The Vesting mini-app's own logic, kept apart from the QML so the Node test
// (../../logic.test.mjs) runs the same code the app does. The QML engine imports this
// as an ECMAScript module: `import "logic.mjs" as Logic`.
//
// The backend is a mock that returns raw data only: sample schedules, destination accounts
// and a fixed fee. Every piece of display logic — formatting, the vesting maths for display,
// validation, next-unlock text, the pre-claim confirmation, fee and privacy checks, and the
// plain result lines — lives here so it can be tested without the QML engine.

/// The three schedule kinds the creator can choose.
export const KINDS = ["cliff-linear", "linear", "milestone"]

/// Mock estimated claim fee, in whole LEZ. The real module will supply this.
export const FEE = "2"
/// Mock token of the sample schedules.
export const TOKEN = "LEZ"

const MINUTE = 60 * 1000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
const BASE58 = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/

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
    var d = new Date(num(ms))
    return isNaN(d.getTime()) ? "" : d.toISOString().slice(0, 10)
}

/// "yyyy-mm-dd" → epoch ms (UTC), or NaN.
export function parseDate(text) {
    return Date.parse(String(text || "").trim() + "T00:00:00Z")
}

/// "in 3 days", "in 5 h", "in 2 min", "now". A positive duration from `now`.
export function inText(ms, now) {
    ms = num(ms)
    if (ms <= 0) return "now"
    if (ms < MINUTE) return "in " + Math.ceil(ms / 1000) + " s"
    if (ms < HOUR) return "in " + Math.ceil(ms / MINUTE) + " min"
    if (ms < DAY) {
        var h = Math.floor(ms / HOUR), m = Math.ceil((ms % HOUR) / MINUTE)
        return "in " + h + " h" + (m > 0 ? " " + m + " min" : "")
    }
    var d = Math.floor(ms / DAY), rh = Math.round((ms % DAY) / HOUR)
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
        var sg = s.signalled || [], i = 0
        while (i < sg.length && sg[i]) i++
        if (i >= sg.length) return { at: 0, text: "All milestones signalled." }
        return { at: 0, text: "Next unlock: tranche " + (i + 1) + " — signalled by the creator." }
    }
    if (s.kind === "cliff-linear" && cmpDec(now, s.cliff) < 0)
        return { at: num(s.cliff), text: "Cliff on " + dateText(s.cliff) + " — " + inText(num(s.cliff) - now, now) + "." }
    if (cmpDec(now, s.end) < 0)
        return { at: num(s.end), text: "Fully vested on " + dateText(s.end) + " — " + inText(num(s.end) - now, now) + "." }
    return { at: 0, text: "Fully vested — nothing left to unlock." }
}

// ── Accounts and creation validation ────────────────────────────────────────────────

/// Why `account` is not the requested account kind, or "". `wantPrivate`: true (private
/// only), false (public only), null (either).
export function problemAccount(account, wantPrivate) {
    var s = String(account == null ? "" : account).trim()
    var m = /^(Public|Private)\/(.+)$/.exec(s)
    if (!m) return "Enter an account ID as Public/<id> or Private/<id>."
    if (!BASE58.test(m[2])) return "That is not a valid account ID (base58, 32–44 characters)."
    if (wantPrivate === true && m[1] !== "Private") return "Choose a private account for a private claim."
    if (wantPrivate === false && m[1] !== "Public") return "Choose a public account for a public claim."
    return ""
}

/// SDK-style check that a private claim's target really is a private (shielded) account.
export function privateTargetProblem(account) { return problemAccount(account, true) }

// ── LNS names (mock map from the module until the SDK name module lands) ──────────────

/// The name registered for `account` in a {name: account} map, or "".
export function nameOf(account, names) {
    var map = names || {}, keys = Object.keys(map), want = String(account).trim().toLowerCase()
    for (var i = 0; i < keys.length; i++)
        if (String(map[keys[i]]).trim().toLowerCase() === want) return keys[i]
    return ""
}

/// "alice · Baaa…aaaa" for a known name, else just the short account.
export function labelled(account, names) {
    var name = nameOf(account, names)
    return name ? name + " · " + short(account) : short(account)
}

/// Resolve what the user typed: an LNS name from the mock map, or a literal account ID.
/// Returns {ok, account, error}; callers use `account` for module calls and `labelled` for display.
export function accountInput(text, names) {
    var typed = String(text == null ? "" : text).trim()
    var map = names || {}
    if (map[typed.toLowerCase()]) return { ok: true, account: map[typed.toLowerCase()], error: "" }
    if (problemAccount(typed, null) === "") return { ok: true, account: typed, error: "" }
    return { ok: false, account: "", error: "Enter a known LNS name or an account ID (Public/<id> or Private/<id>)." }
}

/// "100000, 200000;300000" → ["100000", "200000", "300000"].
export function parseTrancheText(text) {
    return String(text == null ? "" : text).split(/[,;]/)
        .map(function(x) { return String(x).trim() })
        .filter(function(x) { return x !== "" })
}

/// Validate a batch: one shared kind/token/dates/flags, plus one row per recipient.
/// Each row is {beneficiary, amount, tranches} (tranches is text for milestone kinds).
export function validateBatch(p) {
    p = p || {}
    var e = [], rows = p.rows || []
    if (KINDS.indexOf(p.kind) < 0) e.push("Choose a schedule kind.")
    if (typeof p.token !== "string" || !/^[A-Za-z0-9]{2,10}$/.test(p.token.trim())) e.push("Token symbol must be 2–10 letters or digits.")
    var start = num(p.start), end = num(p.end), cliff = num(p.cliff || 0)
    if (!isFinite(Number(p.start)) || start <= 0) e.push("Enter a valid start date.")
    if (!isFinite(Number(p.end)) || end <= start) e.push("End date must be after the start date.")
    if (p.kind === "cliff-linear" && (!isFinite(Number(p.cliff)) || cliff <= start || cliff >= end))
        e.push("Cliff date must be between the start and end dates.")
    if (rows.length === 0) e.push("Add at least one recipient.")
    for (var i = 0; i < rows.length; i++) {
        var r = rows[i] || {}
        var one = validateCreate({
            kind: p.kind, beneficiary: r.beneficiary, token: p.token, amount: r.amount,
            start: p.start, cliff: p.cliff, end: p.end,
            tranches: p.kind === "milestone" ? parseTrancheText(r.tranches) : [],
            cancelable: p.cancelable, transferable: p.transferable,
            cancelAuthority: p.cancelAuthority, milestoneAuthority: p.milestoneAuthority
        })
        for (var j = 0; j < one.errors.length; j++) e.push("Recipient " + (i + 1) + ": " + one.errors[j])
    }
    return { ok: e.length === 0, errors: e }
}

/// Validate every parameter of a new schedule: { ok, errors: [...] }.
/// `p.start`, `p.cliff`, `p.end` are epoch ms; `p.tranches` is an array of amount strings.
export function validateCreate(p) {
    p = p || {}
    var e = []
    if (KINDS.indexOf(p.kind) < 0) e.push("Choose a schedule kind.")
    if (problemAccount(p.beneficiary, null) !== "") e.push("Beneficiary must be an account ID (Public/<id> or Private/<id>).")
    if (typeof p.token !== "string" || !/^[A-Za-z0-9]{2,10}$/.test(p.token.trim())) e.push("Token symbol must be 2–10 letters or digits.")
    if (!/^\d+$/.test(String(p.amount == null ? "" : p.amount)) || num(p.amount) <= 0) e.push("Amount must be a positive whole number.")
    var start = num(p.start), end = num(p.end), cliff = num(p.cliff || 0)
    if (!isFinite(Number(p.start)) || start <= 0) e.push("Enter a valid start date.")
    if (!isFinite(Number(p.end)) || end <= start) e.push("End date must be after the start date.")
    if (p.kind === "cliff-linear" && (!isFinite(Number(p.cliff)) || cliff <= start || cliff >= end))
        e.push("Cliff date must be between the start and end dates.")
    if (p.kind === "milestone") {
        var tr = p.tranches || []
        if (!Array.isArray(tr) || tr.length === 0) e.push("Add at least one milestone tranche.")
        else for (var i = 0; i < tr.length; i++)
            if (!/^\d+$/.test(String(tr[i] == null ? "" : tr[i])) || num(tr[i]) <= 0) {
                e.push("Tranche " + (i + 1) + " must be a positive whole number.")
                break
            }
    }
    if (p.cancelAuthority && problemAccount(p.cancelAuthority, null) !== "") e.push("Cancel authority must be an account ID.")
    if (p.milestoneAuthority && problemAccount(p.milestoneAuthority, null) !== "") e.push("Milestone authority must be an account ID.")
    return { ok: e.length === 0, errors: e }
}

// ── Pre-claim confirmation: fee, privacy, result lines ──────────────────────────────

/// Can the private account pay the estimated fee? { ok, message } with a clear error if not.
export function canPayFee(gas, fee) {
    var g = gas == null ? "" : String(gas), f = String(fee == null ? FEE : fee)
    if (!/^\d+$/.test(g) || !/^\d+$/.test(f))
        return { ok: false, message: "Couldn't read the private account's gas balance; try again." }
    var feeText = amountText(f)
    if (atLeast(g, f)) return { ok: true, message: "The private account can pay the estimated fee of " + feeText + "." }
    return { ok: false, message: "The private account can't pay the estimated fee of " + feeText
        + ". It holds " + amountText(g) + " for gas. Fund that account, or choose a public destination." }
}

/// The privacy disclosure shown before every private claim.
export function privacyDisclosure() {
    var visible = ["the claim amount", "the beneficiary address", "the vesting schedule address"]
    var hidden = ["which private account receives the tokens", "any later movement of those tokens"]
    return {
        visible: visible,
        hidden: hidden,
        text: "Visible on-chain: " + visible.join(", ") + ". Not traceable: " + hidden.join(", ") + "."
    }
}

/// A plain result line after a claim.
export function claimLine(amount, token, dest, tx, isPrivate) {
    return "Claimed " + amountText(amount, token) + " to " + dest + (isPrivate ? " (private)" : " (public)")
        + (tx ? " · " + tx : "") + "."
}

/// Plain result line when the module already formatted the amount for display.
export function claimLineDisplay(display, dest, tx, isPrivate) {
    return "Claimed " + display + " to " + dest + (isPrivate ? " (private)" : " (public)")
        + (tx ? " · " + tx : "") + "."
}
export function createLine(id) { return "Created schedule " + short(id) + "." }
export function batchLine(n) { return "Created " + num(n) + " schedule" + (num(n) === 1 ? "" : "s") + "." }
export function cancelLine(id) { return "Cancelled schedule " + short(id) + "." }
export function signalLine(index, id) { return "Signalled milestone " + (num(index) + 1) + " on " + short(id) + "." }

/// The actionable error when a claim is attempted before anything is vested.
export function nothingClaimable(s, now) { return "Nothing claimable yet. " + nextUnlock(s, now).text }
