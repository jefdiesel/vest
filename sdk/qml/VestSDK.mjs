// The Vest SDK for Basecamp apps: a drop-in for QML + JS apps that want to use the vesting
// core module and sign through the person's Names Wallet (the Names Wallet SDK). Vest itself
// uses this flow in its Main.qml (`namesWrite` and `doCreate`); an app copies this file unchanged.
//
//     var vest = createVest(host, wallet)
//     vest.schedules(function(r) { ... })
//     vest.create(row, function(r) { ... })
//
// `host` supplies only what QML has: call(module, method, args, done, timeoutMs).
// `wallet` is a Names Wallet SDK wallet (`proposeCall`, `watch` through it, `resolve`).
// Every callback gets {ok: true, ...} or {ok: false, code, error}; the SDK never throws.
// Every module argument is text: Basecamp's call checker refuses a QML number for an integer.
// Plain ES module for Node tests and Qt's V4 engine: var and plain functions only.

export const VERSION = "1.0.0"
export const MODULE = "vesting"

function isObject(x) { return x !== null && typeof x === "object" }
function text(x) { return x === null || x === undefined ? "" : String(x) }
function answer(ok, extra) {
    var out = { ok: ok }
    if (extra) {
        var keys = Object.keys(extra)
        for (var i = 0; i < keys.length; i++) out[keys[i]] = extra[keys[i]]
    }
    return out
}
function fail(code, message) { return answer(false, { code: code, error: text(message) }) }
function done(fn, value) { if (fn) fn(value) }

var MISSING = /not connected|not reachable|not loaded|not installed|module not found|no such module/i

/// The module's JSON, once or twice encoded, as {ok, value} or {ok:false, code, error}. The
/// vesting module answers `{"ok": true, ...}` (the value is that whole object) or
/// `{"ok": false, "error": ...}`; Basecamp's own refusal is `{"error", "message"}` or plain text.
export function reply(raw) {
    var parsed = raw
    for (var depth = 0; depth < 2 && typeof parsed === "string"; depth++) {
        try { parsed = JSON.parse(parsed) } catch (error) { break }
    }
    if (isObject(parsed) && typeof parsed.ok === "boolean") {
        if (parsed.ok) return answer(true, { value: parsed })
        var why = text(parsed.error || "the Vesting module refused the request")
        return fail(MISSING.test(why) ? "module_missing" : "module_error", why)
    }
    if (isObject(parsed) && parsed.error) {
        var said = text(parsed.message || parsed.error)
        return fail(MISSING.test(said + " " + text(parsed.error)) ? "module_missing" : "module_error", said)
    }
    if (typeof parsed === "string" && MISSING.test(parsed))
        return fail("module_missing", parsed)
    return fail("no_answer", "No answer from the Vesting module. It may still be busy; try again.")
}

/// A private destination cannot be signed by Names Wallet yet.
export function isPrivate(to) {
    if (isObject(to)) return text(to.account_kind) === "private" || text(to.account).indexOf("Private/") === 0
    return text(to).indexOf("Private/") === 0
}

function stateCode(state) {
    if (state === "rejected") return "rejected"
    if (state === "expired") return "expired"
    if (state === "cancelled") return "cancelled"
    if (state === "failed") return "wallet_failed"
    return "wallet_error"
}

/// One factory. `host` and `wallet` are supplied by the app (NamesWalletSDK makes the wallet).
export function createVest(host, wallet) {
    var h = host || {}
    var w = wallet || {}
    var sdk = { version: VERSION, module: MODULE }

    function call(method, args, callback, timeoutMs) {
        var asked
        try {
            asked = h.call(MODULE, method, args, function(raw) {
                var r = reply(raw)
                if (!r.ok) { done(callback, r); return }
                done(callback, answer(true, { value: r.value }))
            }, timeoutMs || 30000)
        } catch (error) {
            done(callback, fail("module_error", text(error && error.message ? error.message : error)))
            return
        }
        if (asked === false)
            done(callback, fail("no_bridge", "This app runs inside Basecamp, with the Vest module installed."))
    }

    /// The module builds a call, the wallet proposes it, and the result comes back once it is sent.
    function write(method, args, callback) {
        call(method, args, function(built) {
            if (!built.ok) { done(callback, built); return }
            var b = built.value
            if (!isObject(b) || !b.program || !b.accounts_json || !b.instruction_hex) {
                done(callback, fail("bad_call", "The Vesting module didn't build a complete call."))
                return
            }
            if (!w.proposeCall) {
                done(callback, fail("no_wallet", "A Names Wallet SDK wallet is needed to sign this."))
                return
            }
            try {
                w.proposeCall(b.program, b.accounts_json, b.instruction_hex, b.summary || "", {
                    sent: function() {},
                    update: function() {},
                    done: function(status) {
                        if (status && status.state === "done") {
                            done(callback, answer(true, {
                                built: b, status: status, request: status,
                                state: status.state, tx: status.tx ? text(status.tx) : ""
                            }))
                        } else {
                            var reason = status && status.error ? text(status.error)
                                : "Names Wallet did not send the request."
                            done(callback, fail(stateCode(status && status.state), reason))
                        }
                    },
                    failed: function(r) {
                        done(callback, fail("wallet_failed", text(r && r.error ? r.error : "Names Wallet refused the request.")))
                    }
                })
            } catch (error) {
                done(callback, fail("wallet_error", text(error && error.message ? error.message : error)))
            }
        }, 120000)
    }

    /// Every schedule Vest knows, with the names book and the clock it was read at.
    sdk.schedules = function(callback) {
        call("list", [], function(r) {
            if (!r.ok) { done(callback, r); return }
            var v = isObject(r.value) ? r.value : {}
            done(callback, answer(true, { value: v, schedules: v.schedules || [], time: v.time, names: v.names || {} }))
        })
    }

    /// The claimable amount and the simulated fee, without asking the wallet to sign.
    sdk.claimable = function(id, callback) {
        call("pre_claim_summary", [text(id), ""], function(r) {
            if (!r.ok) { done(callback, r); return }
            var v = isObject(r.value) && isObject(r.value.summary) ? r.value.summary : {}
            done(callback, answer(true, { value: r.value, summary: v, claimable: text(v.claimable) }))
        }, 120000)
    }

    /// Create one schedule (the row shape is `toCreateRow`'s in Vest's own logic), then import it.
    sdk.create = function(row, callback) {
        write("create_call", [JSON.stringify(row || {})], callback)
    }

    /// Claim to the schedule's public beneficiary account. A private destination is refused here:
    /// it needs a proof Names Wallet cannot sign into a program call yet.
    sdk.claim = function(id, to, callback) {
        if (isPrivate(to)) {
            done(callback, fail("private_not_in_wallet",
                "Names Wallet can't sign a private claim yet. Claim to a public account instead."))
            return
        }
        write("claim_call", [text(id)], callback)
    }

    sdk.cancel = function(id, callback) { write("cancel_call", [text(id)], callback) }
    sdk.makePermanent = function(id, callback) { write("irrevocable_call", [text(id)], callback) }
    sdk.unlockMilestone = function(id, index, callback) {
        write("signal_call", [text(id), text(index)], callback)
    }
    /// A private account can't be a schedule's beneficiary: it could never sign the claim.
    sdk.transfer = function(id, to, callback) {
        if (isPrivate(to)) {
            done(callback, fail("private_beneficiary",
                "A private account can't receive a schedule. Transfer it to a public account."))
            return
        }
        write("transfer_call", [text(id), text(isObject(to) ? to.account : to)], callback)
    }

    /// Add a schedule the wallet signed to this module's local list; gives its local id.
    sdk.addSchedule = function(code, callback) {
        call("import_schedule", [text(code)], function(r) {
            if (!r.ok) { done(callback, r); return }
            var v = isObject(r.value) ? r.value : {}
            done(callback, answer(true, { value: v, id: v.id }))
        })
    }

    /// An LNS name (or account) through Names Wallet's own resolve, passed through unchanged.
    sdk.resolve = function(nameOrAccount, callback) {
        if (!w.resolve) {
            done(callback, fail("no_wallet", "A Names Wallet SDK wallet is needed to resolve a name."))
            return
        }
        try {
            w.resolve(text(nameOrAccount), function(r) {
                if (r && r.ok) { done(callback, answer(true, r)); return }
                done(callback, fail(r && r.code ? text(r.code) : "resolve_failed",
                    r && r.error ? text(r.error) : "Names Wallet couldn't resolve that name."))
            })
        } catch (error) {
            done(callback, fail("wallet_error", text(error && error.message ? error.message : error)))
        }
    }

    return sdk
}
