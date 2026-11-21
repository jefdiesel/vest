// Vest: pay tokens out over time, or claim what has vested to you (Logos RFP-017).
//
// Two modules do the work. The `vesting` core module (../module) reads the network: schedules,
// the clock, what is claimable, and builds the claim call. The person's Names Wallet (the `names`
// module, its Wallet Provider SDK) shows Vest the accounts the person chooses and signs every
// change once the person approves it in the Names app: Vest never sees a key. People without
// Names Wallet can give a wallet folder instead (Settings, Advanced).
//
// Guides are built in: a "How it works" path for paying out and for receiving, shown on first run
// and one click from the header; the choices explained where they are made; what happens next
// after every action. All the words and the display logic are in logic.mjs, which the Node test
// runs. The look is the shared kit (./kit): one left-aligned column, at most 680 px.

import QtQuick
import QtQuick.Layouts
import Logos.Theme
import "logic.mjs" as Logic
import "kit"

Item {
    id: root
    property Look look: Look {}

    // ── The vesting module ─────────────────────────────────────────────────────────
    property var schedules: []                 // Logic.fromModule rows of `list`
    property var names: ({})                   // the module's name book: hex account -> name
    property var accounts: []                  // the accounts Vest may use (`accounts`)
    property var info: null                    // `settings`: network, connection, signer
    property real effectiveTime: 0             // `now`: the network's clock
    readonly property bool bridgeReady: typeof logos !== "undefined" && !!logos.callModule
    readonly property bool chainOpen: info !== null && info.connected === true
    readonly property bool walletMode: info !== null && info.signer === "wallet"

    // ── Names Wallet ───────────────────────────────────────────────────────────────
    // "checking" | "missing" | "otherZone" | "disconnected" | "asking" | "connected"
    property string namesState: "checking"
    property string namesProblem: ""
    property var accessRequest: null           // the connect request while it waits
    property var claimRequest: null            // the claim request while it waits
    property var writeRequest: null            // a create, cancel, permanent, milestone or transfer while it waits
    property bool namesCanCall: false          // Names Wallet 0.18+: its calls may read the network clock
    property var granted: null
    property var balances: []
    readonly property bool connected: walletMode ? chainOpen : (namesState === "connected" && chainOpen)
    // Who signs creates, cancels, milestones and transfers: the wallet folder, or Names Wallet 0.18+.
    readonly property bool canWrite: walletMode || namesCanCall

    // ── View state ──────────────────────────────────────────────────────────────────
    property string tab: "receive"             // receive | pay | settings
    property bool guideOpen: false
    property string guideRole: "receive"
    property real tick: Date.now()
    property var recipientId: null
    property string destKind: "public"
    property string destAccount: ""
    property var confirm: null                 // the claim, before Names Wallet is asked
    property var sheet: null                   // a step that can't be undone: {kind, schedule}
    property string resultLine: ""
    property string nextLine: ""               // what happens next, after a result
    property bool resultBad: false
    property string resultDetail: ""
    property string lastDetail: ""
    property bool loading: false
    property bool busy: false
    property string workingOn: ""
    property bool justCreated: false

    // ── Forms ───────────────────────────────────────────────────────────────────────
    property string cCreator: ""
    property string cKind: "linear"
    property bool batchMode: false
    property bool cCancelable: true
    property bool cTransferable: false
    property var cTranches: [""]
    property var bRows: [ { beneficiary: "", amount: "", tranches: "" } ]
    // The rows above are changed in place while typing (a new array would rebuild the fields under
    // the cursor, one character at a time); these counts rebuild them only on add and remove.
    property int trancheCount: 1
    property int rowCount: 1
    property var payee: null                   // the payout box, checked: {ok, text} or null

    // ── Derived ─────────────────────────────────────────────────────────────────────
    readonly property var recipientSchedules: schedules.filter(function(s) { return s.mine.beneficiary })
    readonly property var mySchedules: schedules.filter(function(s) {
        return s.mine.creator || s.mine.cancel_authority || s.mine.milestone_authority })
    readonly property var selected: findById(recipientSchedules, recipientId)
    readonly property var publicAccounts: accounts.filter(function(a) { return !a["private"] })
    readonly property var privateAccounts: accounts.filter(function(a) { return a["private"] })
    readonly property string networkName: info && info.network && info.network.name ? info.network.name : ""
    readonly property string firstAccount: accounts.length ? accounts[0].base58 : ""
    readonly property bool guideDone: info !== null && firstAccount !== ""
        && ((info.settings || {}).guide_done || []).indexOf(firstAccount) >= 0
    readonly property bool showGuide: guideOpen || (!guideDone && tab !== "settings")
    readonly property int receiveAt: Logic.receiveStep({ connected: connected, schedules: recipientSchedules.length > 0,
        claimable: !!(selected && Logic.above(selected.claimable || "0", "0")), approving: claimRequest !== null })
    readonly property int payAt: Logic.payStep({ connected: connected, justCreated: justCreated, manages: mySchedules.length > 0 })

    function viewTime() { return root.effectiveTime > 0 ? root.effectiveTime : root.tick }

    function findById(list, id) {
        for (var i = 0; i < list.length; i++) if (String(list[i].id) === String(id)) return list[i]
        return null
    }

    // ── The vesting module ─────────────────────────────────────────────────────────

    /// One call of the vesting module; every argument is a string (Basecamp's call checker
    /// refuses a QML number for an integer parameter).
    function call(method, args, done, patience) {
        if (!bridgeReady) {
            done({ ok: false, error: "This app runs inside Basecamp, with the Vest module installed." })
            return
        }
        var settle = function(raw) {
            var r = Logic.reply(raw)
            if (!r.ok && r.detail) root.lastDetail = String(r.detail)
            done(r)
        }
        if (logos.callModuleAsync)
            logos.callModuleAsync("vesting", method, args, settle, patience || 60000)
        else
            Qt.callLater(function() {
                try { settle(logos.callModule("vesting", method, args)) }
                catch (error) { done({ ok: false, error: String(error.message || error) }) }
            })
    }

    // ── Names Wallet (its Wallet Provider SDK) ─────────────────────────────────────

    /// One call of the names module. Busy means nothing happened: ask again, a little later
    /// each time. Missing means Names isn't installed or open.
    function namesCall(method, args, done, failed, tries) {
        var n = tries || 0
        var no = failed || function(r) { root.say(r.error, true) }
        if (!bridgeReady) { no({ ok: false, missing: true, error: Logic.NAMES_MISSING }); return }
        logos.callModuleAsync("names", method, args, function(raw) {
            var r = Logic.namesReply(raw)
            if (r.ok) { done(r.value); return }
            if (r.busy && n < 5) { root.later(function() { root.namesCall(method, args, done, failed, n + 1) }, 1500 * (n + 1)); return }
            if (r.missing) { root.namesState = "missing"; root.namesProblem = r.error }
            no(r)
        }, 30000)
    }

    /// Run `f` once after `ms`. Each call gets its own timer: two busy Names calls retrying at the
    /// same time each come back (one shared timer kept only the last).
    function later(f, ms) {
        var t = oneShot.createObject(root, { interval: ms })
        t.then = f
        t.start()
    }

    Component {
        id: oneShot
        Timer {
            property var then: null
            onTriggered: { var f = then; then = null; destroy(); if (f) f() }
        }
    }

    /// Where Names Wallet stands: installed and open, on our network, and which accounts it gave.
    function readNames() {
        namesCall("get_config", [], function(config) {
            root.namesCanCall = ((config && config.features) || []).indexOf("call_reads_clock") >= 0
            var zone = Logic.zoneProblem(config, info && info.network ? info.network.chain_id : "")
            if (zone !== "") { namesState = "otherZone"; namesProblem = zone; loading = false; return }
            namesCall("granted_accounts", [], function(g) {
                granted = g
                if (!g || !g.connected || !(g.accounts || []).length) {
                    if (namesState !== "asking") namesState = "disconnected"
                    loading = false
                    return
                }
                namesState = "connected"
                namesCall("account_balances", [], function(b) {
                    balances = b || []
                    call("set_accounts", [JSON.stringify(Logic.namesAccounts(granted, balances))], function() { readChain() })
                }, function() {
                    call("set_accounts", [JSON.stringify(Logic.namesAccounts(granted, []))], function() { readChain() })
                })
            }, function(r) { loading = false })
        }, function(r) { loading = false })
    }

    /// Take the person to the Names app, where the request waits; Names brings them back here
    /// once they answer (Basecamp's own navigation intent, declared in metadata.json `uses`).
    function openNames() {
        if (typeof logos !== "undefined" && logos.request)
            logos.request("basecamp.apps.launch", { app: "names_ui" }, function() {})
    }

    /// 1. Connect: the person picks, in the Names app, which accounts Vest may see.
    function connectNames() {
        say("")
        namesCall("request_access", ["To show your vesting schedules and claim what has vested to you"], function(v) {
            if (v.state === "approved") { readNames(); say("Connected.", false, Logic.nextLine("connect")); return }
            accessRequest = v
            namesState = "asking"
            pollAccess.start()
            openNames()
        })
    }

    Timer {
        id: pollAccess
        interval: 2000
        repeat: true
        onTriggered: root.namesCall("request_status", [root.accessRequest.request], function(v) {
            root.accessRequest = v
            if (v.state === "pending") return
            pollAccess.stop()
            root.accessRequest = null
            if (v.state === "approved") { root.readNames(); root.say("Connected.", false, Logic.nextLine("connect")) }
            else { root.namesState = "disconnected"; root.say(Logic.requestLine(v, "the connection"), true) }
        }, function(r) { pollAccess.stop(); root.namesState = "disconnected"; root.say(r.error, true) })
    }

    // ── Reading ─────────────────────────────────────────────────────────────────────

    /// The vesting module's settings, then the signer: Names Wallet (default) or a wallet folder.
    function refresh() {
        loading = true
        call("settings", [], function(r) {
            if (!r.ok) { loading = false; if (r.missing) say(r.error, true); return }
            info = r
            fillSettings(r)
            if (r.signer === "wallet") {
                if (!r.connected && !String((r.settings || {}).wallet_dir || "").trim()) { loading = false; return }
                readChain()
                return
            }
            readNames()
        })
    }

    function readChain() {
        call("accounts", [], function(a) {
            if (!a.ok) return
            accounts = a.accounts || []
            if (cCreator === "" && publicAccounts.length) cCreator = publicAccounts[0].id
        })
        call("now", [], function(n) { if (n.ok) effectiveTime = n.time })
        call("list", [], function(l) {
            loading = false
            call("settings", [], function(again) { if (again.ok) info = again })
            if (!l.ok) { say(l.error || "Couldn't read your schedules.", true); return }
            schedules = (l.schedules || []).map(Logic.fromModule)
            names = l.names || ({})
            if (l.time) effectiveTime = l.time
            if (!findById(recipientSchedules, recipientId))
                recipientId = recipientSchedules.length ? recipientSchedules[0].id : null
        }, 120000)
    }

    function fillSettings(r) {
        var s = r.settings || {}
        walletField.text = s.wallet_dir || ""
        networkFileField.text = s.network_file || ""
        rpcField.text = s.rpc || ""
        chainField.text = s.chain_id || ""
        programField.text = s.program || ""
        tokenProgramField.text = s.token_program || ""
        lnsField.text = s.lns_registry || ""
        clockField.text = s.clock_elf || ""
    }

    /// The result line under the page, and what happens next; red only for a problem.
    function say(message, bad, next) {
        var t = String(message || "").trim()
        resultLine = t === "" ? "" : t.charAt(0).toUpperCase() + t.substr(1) + (/[.?!…]$/.test(t) ? "" : ".")
        resultBad = !!bad
        nextLine = bad ? "" : (next || "")
        resultDetail = bad ? lastDetail : ""
        lastDetail = ""
    }

    /// A write through the vesting module (wallet-folder mode).
    function write(what, method, args, done, patience) {
        say("")
        busy = true
        workingOn = what
        call(method, args, function(r) {
            busy = false
            workingOn = ""
            done(r)
            refresh()
        }, patience || 300000)
    }

    function copy(text, what) {
        clipboard.text = text
        clipboard.selectAll()
        clipboard.copy()
        say(what + " copied.")
    }

    /// The person has seen the guide: fold it away for this wallet from now on.
    function finishGuide() {
        guideOpen = false
        if (firstAccount === "" || guideDone) return
        var done = ((info && info.settings && info.settings.guide_done) || []).slice()
        done.push(firstAccount)
        call("configure", [JSON.stringify({ guide_done: done })], function(r) { if (r.network) info = r })
    }

    Component.onCompleted: refresh()
    Timer { interval: 1000; repeat: true; running: true; onTriggered: root.tick = Date.now() }
    TextEdit { id: clipboard; visible: false }

    // ── Settings ────────────────────────────────────────────────────────────────────

    function saveSettings(signer) {
        say("")
        busy = true
        workingOn = "settings"
        var patch = {
            signer: signer, wallet_dir: walletField.text.trim(), network_file: networkFileField.text.trim(),
            rpc: rpcField.text.trim(), chain_id: chainField.text.trim(), program: programField.text.trim(),
            token_program: tokenProgramField.text.trim(), lns_registry: lnsField.text.trim(),
            clock_elf: clockField.text.trim()
        }
        if (passwordField.text !== "") patch.password = passwordField.text
        call("configure", [JSON.stringify(patch)], function(r) {
            busy = false
            workingOn = ""
            passwordField.text = ""
            if (r.network) info = r
            if (r.ok) { say(signer === "wallet" ? "Vest now signs with the wallet folder." : "Vest now uses Names Wallet."); tab = "receive" }
            else say(r.error, true)
            refresh()
        }, 60000)
    }

    function doImport() {
        var a = Logic.accountInput(importField.text)
        if (!a.ok) { say("Paste the schedule's address or the sender's LNS name.", true); return }
        busy = true
        workingOn = "import"
        call("import_schedule", [a.account], function(r) {
            busy = false
            workingOn = ""
            if (!r.ok) { say(r.error, true); return }
            importField.text = ""
            recipientId = r.id
            say("Added schedule " + r.id + ".", false, Logic.nextLine("import"))
            refresh()
        }, 60000)
    }

    // ── Claiming ────────────────────────────────────────────────────────────────────

    /// The choice on the first screen: what the person came to do. Once connected it also opens that tab.
    function pickRole(role) {
        guideRole = role
        if (connected) tab = role === "pay" ? "pay" : "receive"
    }
    onTabChanged: if (tab !== "settings") guideRole = tab === "pay" ? "pay" : "receive"

    function chooseKind(kind) {
        destKind = kind
        destAccount = kind === "private" && privateAccounts.length ? privateAccounts[0].id : ""
    }

    /// Names Wallet: the module builds the claim call; the person sees it here, then approves it
    /// in Names. A wallet folder: the module simulates the claim for its fee.
    function beginClaim() {
        var s = selected
        if (!s) return
        say("")
        busy = true
        workingOn = "summary"
        if (!walletMode) {
            call("claim_call", [String(s.id)], function(r) {
                busy = false
                workingOn = ""
                if (!r.ok) { say(r.error, true); return }
                confirm = { id: String(s.id), names: r, amountDisplay: r.amount_display, isPrivate: false,
                            destAccount: s.beneficiary, fee: { ok: true, message: "Names Wallet shows the network fee before you approve; it is paid from this account." } }
            }, 120000)
            return
        }
        var isPrivate = destKind === "private"
        if (isPrivate && destAccount === "") { busy = false; workingOn = ""; say("Choose one of your private accounts.", true); return }
        call("pre_claim_summary", [String(s.id), isPrivate ? destAccount : ""], function(r) {
            busy = false
            workingOn = ""
            if (!r.ok) { say("Couldn't prepare the claim: " + r.error, true); return }
            var sum = r.summary || {}
            if (!Logic.above(sum.claimable || "0", "0")) { say(Logic.nothingClaimable(s, viewTime())); return }
            confirm = {
                id: String(s.id), isPrivate: isPrivate,
                amountDisplay: sum.claimable_display, feeDisplay: sum.estimated_fee_display,
                destAccount: isPrivate ? destAccount : s.beneficiary,
                fee: Logic.feeCheck(sum), privacy: isPrivate ? Logic.privacyDisclosure() : null
            }
        }, 120000)
    }

    function confirmClaim() {
        if (!confirm || !confirm.fee.ok) return
        var c = confirm
        if (c.names) {
            busy = true
            workingOn = "claim"
            namesCall("propose_call", [c.names.program, c.names.accounts_json, c.names.instruction_hex, c.names.summary], function(v) {
                claimRequest = v
                claimRequest.amountDisplay = c.amountDisplay
                pollClaim.start()
                openNames()
            }, function(r) { busy = false; workingOn = ""; say(r.error, true) })
            return
        }
        if (c.isPrivate) say("Proving the private claim on this computer; this takes a few minutes.")
        write("claim", c.isPrivate ? "claim_private" : "claim_public", c.isPrivate ? [c.id, c.destAccount] : [c.id], function(r) {
            confirm = null
            if (!r.ok) { say("The claim didn't go through: " + r.error, true); return }
            say(Logic.claimLineDisplay(r.amount_display, Logic.labelled(c.destAccount, root.names), r.tx, c.isPrivate), false, Logic.nextLine("claim"))
        }, c.isPrivate ? 3600000 : 300000)
    }

    Timer {
        id: pollClaim
        interval: 2000
        repeat: true
        onTriggered: root.namesCall("request_status", [root.claimRequest.request], function(v) {
            v.amountDisplay = root.claimRequest.amountDisplay
            root.claimRequest = v
            if (Logic.requestOpen(v)) return
            pollClaim.stop()
            root.busy = false
            root.workingOn = ""
            root.claimRequest = null
            root.confirm = null
            if (v.state === "done") root.say("Claimed " + (v.amountDisplay || "") + (v.tx ? " · tx " + Logic.short(v.tx) : "") + ".", false, Logic.nextLine("claim"))
            else root.say(Logic.requestLine(v, "the claim"), v.state === "failed")
            root.refresh()
        }, function(r) { pollClaim.stop(); root.busy = false; root.workingOn = ""; root.say(r.error, true) })
    }

    function withdrawClaim() {
        if (!claimRequest) return
        namesCall("cancel_request", [claimRequest.request], function() {})
    }

    function doTransfer(id, target) {
        var typed = Logic.accountInput(target)
        if (!typed.ok) { say("Enter the new beneficiary's address or LNS name.", true); return }
        if (!walletMode) {
            namesWrite("transfer", "transfer_call", [String(id), typed.account], function() {
                transferField.text = ""
                say("Schedule " + id + " now goes to " + typed.account + ".", false, Logic.nextLine("transfer"))
            })
            return
        }
        write("transfer", "transfer_beneficiary", [String(id), typed.account], function(r) {
            if (!r.ok) { say(r.error, true); return }
            transferField.text = ""
            say("Schedule " + id + " now belongs to " + (r.name ? r.name : Logic.short(r.beneficiary)) + ".", false, Logic.nextLine("transfer"))
        })
    }

    // ── Paying out ──────────────────────────────────────────────────────────────────

    function amountOf(text) {
        return Logic.isNative(tokenField.text) ? Logic.toBase(text) : String(text).trim()
    }

    function sharedParams() {
        return {
            kind: cKind, token: tokenField.text.trim() || "LEZ",
            start: Logic.parseDate(startField.text), cliff: Logic.parseDate(cliffField.text), end: Logic.parseDate(endField.text),
            cancelable: cCancelable, transferable: cTransferable,
            cancelAuthority: cancelAuthField.text.trim(), milestoneAuthority: milestoneAuthField.text.trim()
        }
    }

    /// A change signed through Names Wallet: the module builds the call (`<method>` → {program,
    /// accounts_json, instruction_hex, summary, caller}), Names shows it and the person approves,
    /// choosing `caller`. `done(built, status)` runs once Names says it was sent.
    function namesWrite(what, method, args, done) {
        say("")
        busy = true
        workingOn = what
        call(method, args, function(r) {
            if (!r.ok) { busy = false; workingOn = ""; say(r.error, true); return }
            namesCall("propose_call", [r.program, r.accounts_json, r.instruction_hex, r.summary], function(v) {
                writeRequest = { request: v.request, built: r, done: done, what: Logic.writeWhat(what) }
                say("In Names Wallet, choose " + Logic.labelled(r.caller, root.names) + " and approve. You come back here after.")
                pollWrite.start()
                openNames()
            }, function(e) { busy = false; workingOn = ""; say(e.error, true) })
        }, 120000)
    }

    Timer {
        id: pollWrite
        interval: 2000
        repeat: true
        onTriggered: root.namesCall("request_status", [root.writeRequest.request], function(v) {
            if (Logic.requestOpen(v)) return
            pollWrite.stop()
            var w = root.writeRequest
            root.writeRequest = null
            root.busy = false
            root.workingOn = ""
            if (v.state === "done") w.done(w.built, v)
            else root.say(Logic.requestLine(v, w.what), v.state === "failed")
            root.refresh()
        }, function(e) { pollWrite.stop(); root.writeRequest = null; root.busy = false; root.workingOn = ""; root.say(e.error, true) })
    }

    /// Look up what was typed in the payout box (an LNS name or an address) and say who it is.
    function checkPayee() {
        var t = beneficiaryField.text.trim()
        if (t === "") { payee = null; return }
        payee = { ok: false, checking: true, text: "Checking " + t + "…" }
        call("resolve_name", [t], function(r) {
            if (beneficiaryField.text.trim() !== t) return
            payee = Logic.payeeLine(t, r)
        }, 30000)
    }

    Timer { id: payeeTimer; interval: 700; onTriggered: root.checkPayee() }

    function doCreate() {
        if (cCreator === "") { say("Choose which of your accounts pays.", true); return }
        if (!root.batchMode && root.payee && !root.payee.ok && !root.payee.checking) { say(root.payee.text, true); return }
        var p = sharedParams()
        p.beneficiary = beneficiaryField.text.trim()
        p.amount = amountOf(amountField.text)
        p.tranches = cKind === "milestone" ? cTranches.map(amountOf) : []
        var v = Logic.validateCreate(p)
        if (!v.ok) { say(v.errors[0], true); return }
        if (!walletMode) {
            namesWrite("create", "create_call", [JSON.stringify(Logic.toCreateRow(p, cCreator))], function(built, v) {
                justCreated = true
                call("import_schedule", [built.schedule_address], function(imp) {
                    var id = imp.ok ? imp.id : ""
                    say(Logic.resolvedLines(built.resolved).concat(["Created " + (id !== "" ? "schedule " + id : "the schedule") + " · " + built.amount_display + (v.tx ? " · tx " + Logic.short(v.tx) : "") + "."]).join(" "),
                        false, Logic.nextLine("create", { id: id }))
                    refresh()
                }, 60000)
            })
            return
        }
        write("create", "create", [JSON.stringify(Logic.toCreateRow(p, cCreator))], function(r) {
            if (!r.ok) { say(r.error, true); return }
            justCreated = true
            say(Logic.resolvedLines(r.resolved).concat([Logic.createLine(r.id)]).join(" "), false, Logic.nextLine("create", { id: r.id }))
        })
    }

    function doBatch() {
        if (cCreator === "") { say("Choose which of your accounts pays.", true); return }
        var shared = sharedParams()
        var rows = bRows.map(function(row) {
            return { beneficiary: row.beneficiary.trim(), amount: amountOf(row.amount),
                     tranches: Logic.parseTrancheText(row.tranches).map(amountOf).join(",") }
        })
        var v = Logic.validateBatch(Object.assign({ rows: rows }, shared))
        if (!v.ok) { say(v.errors[0], true); return }
        var params = rows.map(function(r) {
            var p = Object.assign({}, shared)
            p.beneficiary = r.beneficiary
            p.amount = r.amount
            p.tranches = shared.kind === "milestone" ? Logic.parseTrancheText(r.tranches) : []
            return Logic.toCreateRow(p, cCreator)
        })
        write("batch", "create_batch", [JSON.stringify(params)], function(r) {
            if (!r.ok) { say(r.error, true); return }
            justCreated = true
            say(Logic.resolvedLines(r.resolved).concat([Logic.batchLine((r.ids || []).length)]).join(" "), false, Logic.nextLine("batch"))
        })
    }

    function askGrave(kind, s) { sheet = { kind: kind, schedule: s } }

    function doGrave() {
        var k = sheet.kind, s = sheet.schedule
        sheet = null
        if (!walletMode) {
            if (k === "cancel")
                namesWrite("cancel:" + s.id, "cancel_call", [String(s.id)], function(built) {
                    say("Cancelled schedule " + s.id + ". " + built.amount_display + " went back to the creator.", false, Logic.nextLine("cancel"))
                })
            else
                namesWrite("irrevocable:" + s.id, "irrevocable_call", [String(s.id)], function() {
                    say("Schedule " + s.id + " is now permanent.", false, Logic.nextLine("irrevocable"))
                })
            return
        }
        if (k === "cancel")
            write("cancel:" + s.id, "cancel", [String(s.id)], function(r) {
                if (!r.ok) { say(r.error, true); return }
                say("Cancelled schedule " + s.id + ". " + r.refund_display + " went back to the creator.", false, Logic.nextLine("cancel"))
            })
        else
            write("irrevocable:" + s.id, "make_irrevocable", [String(s.id)], function(r) {
                if (!r.ok) { say(r.error, true); return }
                say("Schedule " + s.id + " is now permanent.", false, Logic.nextLine("irrevocable"))
            })
    }

    function doSignal(s, index) {
        if (!walletMode) {
            namesWrite("signal:" + s.id + ":" + index, "signal_call", [String(s.id), String(index)], function(built) {
                say("Unlocked milestone " + (index + 1) + " of schedule " + s.id + ": " + built.amount_display + ".", false, Logic.nextLine("signal"))
            })
            return
        }
        write("signal:" + s.id + ":" + index, "signal_milestone", [String(s.id), String(index)], function(r) {
            if (!r.ok) { say(r.error, true); return }
            say("Unlocked milestone " + (index + 1) + " of schedule " + s.id + ": " + r.amount_display + ".", false, Logic.nextLine("signal"))
        })
    }

    // ── Screen ──────────────────────────────────────────────────────────────────────

    Rectangle { anchors.fill: parent; color: look.slate }

    Flickable {
        id: page
        anchors.fill: parent
        contentHeight: column.implicitHeight + 80
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ColumnLayout {
            id: column
            x: root.width >= 600 ? look.edge : 24
            y: 40
            width: Math.min(root.width - 2 * x, look.pageWidth)
            spacing: 16

            RowLayout {
                Layout.fillWidth: true
                spacing: 16
                PageHeader {
                    look: root.look
                    title: "Vest"
                    sentence: "Pay tokens out over time, or claim what has vested to you."
                    runsOnPrefix: "Runs on"
                    network: root.networkName !== "" ? Logic.clean(root.networkName) : ""
                    networkDetail: root.info && root.info.network && root.info.network.rpc ? Logic.hostOf(root.info.network.rpc) : ""
                }
                Link { look: root.look; Layout.alignment: Qt.AlignTop; visible: root.connected; text: root.showGuide ? "Hide steps" : "Steps"; onPressed: root.showGuide ? root.finishGuide() : root.guideOpen = true }
            }

            // ── Names Wallet: missing, another network, not connected yet, asking ─────
            Card {
                visible: !root.walletMode && root.namesState === "missing" && root.tab !== "settings"
                Heading { text: "Vest needs Names Wallet" }
                Body { text: Logic.NAMES_MISSING }
                Body { text: "No Names Wallet? A wallet folder works too: Settings, Advanced." }
                Action { look: root.look; label: "Check again"; onPressed: root.refresh() }
            }
            Card {
                visible: !root.walletMode && root.namesState === "otherZone" && root.tab !== "settings"
                Heading { text: "Switch Names Wallet to the bridge test network" }
                Body { text: root.namesProblem }
                Action { look: root.look; label: "Check again"; onPressed: root.refresh() }
            }
            Card {
                visible: root.walletMode && !root.chainOpen && root.tab !== "settings" && Logic.connectProblem(root.info) !== ""
                Heading { text: "Not connected" }
                Body { text: Logic.connectProblem(root.info); color: look.alarm }
                Action { look: root.look; label: "Open settings"; onPressed: root.tab = "settings" }
            }

            // ── Steps: what to do, one at a time ─────────────────────────────────────
            // The first screen: pick what you came to do, then numbered steps. The step you are
            // on says what to do and holds its button (step 1: connect Names Wallet).
            Card {
                visible: root.showGuide && root.namesState !== "missing" && root.namesState !== "otherZone"
                Heading { text: "What do you want to do?" }
                RowLayout {
                    spacing: 8
                    ChoiceInline { label: "Claim tokens sent to me"; chosen: root.guideRole === "receive"; onPressed: root.pickRole("receive") }
                    ChoiceInline { label: "Pay someone over time"; chosen: root.guideRole === "pay"; onPressed: root.pickRole("pay") }
                }
                Repeater {
                    model: Logic.guideSteps(root.guideRole, root.guideRole === "pay" ? root.payAt : root.receiveAt)
                    delegate: RowLayout {
                        id: step
                        required property var modelData
                        readonly property bool now: modelData.state === "now"
                        Layout.fillWidth: true
                        Layout.topMargin: 6
                        spacing: 12
                        Rectangle {
                            Layout.alignment: Qt.AlignTop
                            Layout.preferredWidth: look.px(24); Layout.preferredHeight: look.px(24); radius: width / 2
                            color: step.now ? look.brass : "transparent"
                            border.width: 1
                            border.color: step.modelData.state === "later" ? look.line : look.brass
                            Text { anchors.centerIn: parent; textFormat: Text.PlainText; text: step.modelData.state === "done" ? "✓" : String(step.modelData.n); color: step.now ? look.slate : look.ink; font.family: look.sans; font.pixelSize: look.px(13); font.weight: Font.DemiBold }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 6
                            Text {
                                textFormat: Text.PlainText
                                Layout.fillWidth: true
                                text: step.modelData.text
                                color: step.modelData.state === "later" ? look.dim : look.ink
                                font.family: look.sans
                                font.pixelSize: look.px(16)
                                font.weight: step.now ? Font.DemiBold : Font.Normal
                                wrapMode: Text.Wrap
                            }
                            Body {
                                visible: step.now
                                text: step.modelData.n === 1 && root.namesState === "asking"
                                    ? "Names Wallet is open with the request: tick the accounts you use for Vest and approve. You come back here after."
                                    : step.modelData.detail
                            }
                            Body {
                                visible: step.now && step.modelData.n === 2 && root.guideRole === "pay" && !root.canWrite && root.connected
                                text: "Not with Names Wallet yet: it can approve claims, but not a new schedule. To pay out today, use a wallet folder (Settings, Advanced)."
                            }
                            RowLayout {
                                visible: step.now && step.modelData.n === 1 && !root.walletMode
                                spacing: 16
                                Action {
                                    look: root.look; strong: true
                                    label: "Connect"; busyLabel: "Waiting for you in Names…"; working: root.namesState === "asking"
                                    onPressed: root.connectNames()
                                }
                                Link { look: root.look; visible: root.namesState === "asking"; text: "Check again"; onPressed: root.readNames() }
                            }
                        }
                    }
                }
                RowLayout {
                    Layout.topMargin: 4
                    spacing: 20
                    Action { look: root.look; label: "Got it"; visible: root.connected; onPressed: root.finishGuide() }
                    Link { look: root.look; text: "Read the full guide"; onPressed: Qt.openUrlExternally(Logic.GUIDE_URL) }
                }
            }

            RowLayout {
                visible: root.connected || root.tab === "settings" || root.walletMode
                Layout.fillWidth: true
                spacing: 8
                Tab { label: "Receive"; active: root.tab === "receive"; onPressed: root.tab = "receive" }
                Tab { label: "Pay out"; active: root.tab === "pay"; onPressed: root.tab = "pay" }
                Tab { label: "Settings"; active: root.tab === "settings"; onPressed: root.tab = "settings" }
                Item { Layout.fillWidth: true }
                Link { look: root.look; visible: root.connected; text: root.loading ? "Reading…" : "Refresh"; onPressed: if (!root.loading) root.refresh() }
            }

            // ── Receive ─────────────────────────────────────────────────────────────
            Card {
                visible: root.tab === "receive" && root.connected && root.recipientSchedules.length === 0
                Heading { text: "No schedules for your accounts yet" }
                Body { text: "A schedule shows up here once someone pays you through Vest and you add it. Ask the person paying you for the schedule's address, or for their LNS name, and paste it below." }
                ImportRow {}
            }

            Card {
                visible: root.tab === "receive" && root.recipientSchedules.length > 0
                Heading { text: "Your schedules" }
                Repeater {
                    model: root.recipientSchedules
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: Logic.scheduleTitle(modelData)
                        detail: Logic.rowState(modelData)
                        chosen: String(root.recipientId) === String(modelData.id)
                        onPressed: { root.recipientId = modelData.id; root.confirm = null }
                    }
                }
            }

            Card {
                visible: root.tab === "receive" && root.selected !== null
                Heading { text: root.selected ? "Schedule " + root.selected.id : "" }
                Body { visible: !!root.selected && Logic.isClosed(root.selected); text: root.selected ? Logic.statusLine(root.selected) : ""; color: look.ink }
                FactRow {
                    look: root.look; key: "Claimable now"
                    visible: !!root.selected && (!Logic.isClosed(root.selected) || Logic.above(root.selected.claimable || "0", "0"))
                    value: root.selected ? root.selected.claimable_display + (root.selected.cancelled ? " (vested before the cancel)" : "") : ""
                }
                FactRow { look: root.look; key: "Claimed"; value: root.selected ? root.selected.claimed_display + (root.selected.cancelled ? "" : " of " + root.selected.total_display) : "" }
                FactRow { look: root.look; key: "Still locked"; visible: !!root.selected && !Logic.isClosed(root.selected); value: root.selected ? root.selected.locked_display : "" }
                FactRow { look: root.look; key: "Next"; visible: !!root.selected && !Logic.isClosed(root.selected); value: root.selected ? Logic.nextUnlock(root.selected, root.viewTime()).text : "" }
                FactRow { look: root.look; key: "From"; value: root.selected ? Logic.labelled(root.selected.creator, root.names) : "" }
                RowLayout {
                    visible: !!(root.selected && root.selected.address)
                    spacing: 16
                    Text { textFormat: Text.PlainText; text: "Address"; color: look.dim; font.family: look.sans; font.pixelSize: look.px(14); Layout.preferredWidth: look.px(132) }
                    Text { textFormat: Text.PlainText; text: root.selected && root.selected.address ? Logic.short(root.selected.address) : ""; color: look.ink; font.family: look.mono; font.pixelSize: look.px(13) }
                    Link { look: root.look; text: "Copy"; onPressed: root.copy(root.selected.address, "Schedule address") }
                }

                // Claim: where to, then the confirmation.
                ColumnLayout {
                    visible: !!root.selected && !Logic.isFinished(root.selected) && root.confirm === null
                    Layout.fillWidth: true
                    Layout.topMargin: 8
                    spacing: 10
                    Body { text: "A public claim pays your account in the open: anyone can see the amount and where it went." }
                    Body { visible: root.walletMode; text: "A private claim pays a private account of yours: the amount is public, but where the tokens go after that stays hidden. It is proven on this computer and takes a few minutes." }
                    Body { visible: !root.walletMode; text: "Private claims need Names Wallet to sign a public claim into a private account, which it can't yet: claims go to your public account for now." }
                    RowLayout {
                        visible: root.walletMode
                        spacing: 8
                        ChoiceInline { label: "Public account"; chosen: root.destKind === "public"; onPressed: root.chooseKind("public") }
                        ChoiceInline { label: "Private account"; chosen: root.destKind === "private"; onPressed: root.chooseKind("private") }
                    }
                    Repeater {
                        model: root.walletMode && root.destKind === "private" ? root.privateAccounts : []
                        delegate: Choice {
                            required property var modelData
                            Layout.fillWidth: true
                            label: Logic.short(modelData.base58)
                            detail: modelData.balance_display
                            chosen: root.destAccount === modelData.id
                            onPressed: root.destAccount = modelData.id
                        }
                    }
                    Action {
                        look: root.look; strong: true; changes: true; busy: root.busy
                        label: "Check claim"; busyLabel: "Checking…"; working: root.workingOn === "summary"
                        onPressed: root.beginClaim()
                    }
                }

                Sheet {
                    visible: root.confirm !== null
                    Heading { text: root.confirm ? "Claim " + root.confirm.amountDisplay + "?" : "" }
                    FactRow { look: root.look; key: "To"; value: root.confirm ? Logic.labelled(root.confirm.destAccount, root.names) + (root.confirm.isPrivate ? " (private)" : " (public)") : "" }
                    FactRow { look: root.look; key: "Network fee"; value: root.confirm ? (root.confirm.feeDisplay || "shown by Names Wallet before you approve") : "" }
                    Body { visible: !!(root.confirm && root.confirm.privacy); text: root.confirm && root.confirm.privacy ? root.confirm.privacy.text : "" }
                    Body { text: root.confirm ? root.confirm.fee.message : ""; color: root.confirm && !root.confirm.fee.ok ? look.alarm : look.dim }
                    Body { visible: root.claimRequest !== null; text: Logic.requestLine(root.claimRequest, "the claim"); color: look.ink }
                    RowLayout {
                        spacing: 8
                        Action {
                            look: root.look; strong: true; changes: true; busy: root.busy
                            enabledNow: !!(root.confirm && root.confirm.fee.ok)
                            label: root.confirm && root.confirm.names ? "Claim in Names Wallet" : "Claim " + (root.confirm ? root.confirm.amountDisplay : "")
                            busyLabel: root.claimRequest !== null ? "Waiting for Names…" : "Claiming…"; working: root.workingOn === "claim"
                            onPressed: root.confirmClaim()
                        }
                        Action { look: root.look; label: "Don't claim"; visible: root.workingOn !== "claim"; onPressed: root.confirm = null }
                        Link { look: root.look; visible: root.claimRequest !== null && root.claimRequest.state === "pending"; text: "Withdraw the request"; onPressed: root.withdrawClaim() }
                    }
                }
            }

            Card {
                visible: root.tab === "receive" && root.canWrite && root.selected !== null && root.selected.transferable && !Logic.isClosed(root.selected)
                Heading { text: "Hand this schedule to someone else" }
                Body { text: "This schedule was made transferable: what is still to vest can go to another account." }
                Field { id: transferField; look: root.look; Layout.fillWidth: true; hint: "New beneficiary: address or LNS name" }
                Action {
                    look: root.look; changes: true; busy: root.busy
                    label: "Transfer schedule"; busyLabel: "Transferring…"; working: root.workingOn === "transfer"
                    onPressed: root.doTransfer(root.selected.id, transferField.text)
                }
            }

            Card {
                visible: root.tab === "receive" && root.recipientSchedules.length > 0
                Heading { text: "Add a schedule" }
                Body { text: "Someone set up another schedule for you? Paste its address, or the sender's LNS name." }
                ImportRow {}
            }

            TechDetails {
                visible: root.tab === "receive" && root.selected !== null
                look: root.look
                summary: root.selected && root.selected.address ? "Schedule " + Logic.short(root.selected.address) : ""
                FactRow { look: root.look; key: "Schedule address"; value: root.selected && root.selected.address ? root.selected.address : ""; monospace: true }
                FactRow { look: root.look; key: "Beneficiary"; value: root.selected ? root.selected.beneficiary : ""; monospace: true }
                FactRow { look: root.look; key: "Creator"; value: root.selected ? root.selected.creator : ""; monospace: true }
                FactRow { look: root.look; key: "Token"; value: root.selected ? root.selected.token : ""; monospace: true }
                FactRow { look: root.look; key: "Amounts (base units)"; value: root.selected ? "total " + root.selected.total + " · claimed " + root.selected.claimed + " · claimable " + root.selected.claimable : ""; monospace: true }
                FactRow { look: root.look; key: "Network time"; value: Logic.dateText(root.viewTime()) + " (" + root.viewTime() + " ms)" }
            }

            // ── Pay out ─────────────────────────────────────────────────────────────
            Sheet {
                visible: root.tab === "pay" && root.sheet !== null
                Heading { text: root.sheet ? Logic.confirmText(root.sheet.kind, root.sheet.schedule) : "" }
                RowLayout {
                    spacing: 8
                    Action {
                        look: root.look; strong: true; grave: true; changes: true; busy: root.busy
                        label: root.sheet && root.sheet.kind === "cancel" ? "Cancel schedule" : "Make it permanent"
                        onPressed: root.doGrave()
                    }
                    Action { look: root.look; label: "Keep it as it is"; onPressed: root.sheet = null }
                }
            }

            Card {
                visible: root.tab === "pay" && !root.canWrite && root.connected
                Heading { text: "Paying out with Names Wallet: not yet" }
                Body { text: Logic.NAMES_CANT_PAY }
                Link { look: root.look; text: "Use a wallet folder instead"; onPressed: root.tab = "settings" }
            }

            Card {
                visible: root.tab === "pay" && (root.connected || root.walletMode)
                Heading { text: "New schedule" }
                Body { text: "Lock tokens for someone and let them unlock over time. Example: 1,000 LEZ, nothing for a year, then unlocking evenly over the next three years." }
                Label { text: "Paid from" }
                Body { visible: root.publicAccounts.length === 0; text: "Your wallet has no public account to pay from." }
                Repeater {
                    model: root.publicAccounts
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: modelData.name ? modelData.name + " · " + Logic.short(modelData.base58) : Logic.short(modelData.base58)
                        detail: modelData.balance_display
                        chosen: root.cCreator === modelData.id
                        onPressed: root.cCreator = modelData.id
                    }
                }
                Label { text: "How it unlocks" }
                RowLayout {
                    spacing: 8
                    ChoiceInline { label: "Evenly over time"; chosen: root.cKind === "linear"; onPressed: root.cKind = "linear" }
                    ChoiceInline { label: "Cliff, then evenly"; chosen: root.cKind === "cliff-linear"; onPressed: root.cKind = "cliff-linear" }
                    ChoiceInline { label: "Milestones"; chosen: root.cKind === "milestone"; onPressed: root.cKind = "milestone" }
                }
                Timeline { kind: root.cKind }
                Body { text: Logic.kindExplain(root.cKind) }
                GridLayout {
                    visible: root.cKind !== "milestone"
                    Layout.fillWidth: true
                    columns: root.cKind === "cliff-linear" ? 3 : 2
                    columnSpacing: 10
                    rowSpacing: 4
                    Label { text: "Starts" }
                    Label { visible: root.cKind === "cliff-linear"; text: "Cliff" }
                    Label { text: "Fully unlocked" }
                    Field { id: startField; look: root.look; Layout.preferredWidth: 1; Layout.fillWidth: true; hint: "2026-12-01" }
                    Field { id: cliffField; look: root.look; Layout.preferredWidth: 1; visible: root.cKind === "cliff-linear"; Layout.fillWidth: true; hint: "2027-12-01" }
                    Field { id: endField; look: root.look; Layout.preferredWidth: 1; Layout.fillWidth: true; hint: "2030-12-01" }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    ChoiceInline { label: "One person"; chosen: !root.batchMode; onPressed: root.batchMode = false }
                    ChoiceInline { label: "Several people, one transaction"; chosen: root.batchMode; onPressed: root.batchMode = true }
                }
                ColumnLayout {
                    visible: !root.batchMode
                    Layout.fillWidth: true
                    spacing: 6
                    Label { text: "Who receives it" }
                    Field { id: beneficiaryField; look: root.look; Layout.fillWidth: true; hint: "Their address or LNS name"; onTextChanged: { root.payee = null; payeeTimer.restart() } }
                    Text {
                        textFormat: Text.PlainText
                        visible: root.payee !== null
                        Layout.fillWidth: true
                        text: root.payee ? root.payee.text : ""
                        color: !root.payee || root.payee.checking ? look.dim : root.payee.ok ? look.ink : look.alarm
                        font.family: look.sans
                        font.pixelSize: look.px(14)
                        font.weight: root.payee && root.payee.ok ? Font.DemiBold : Font.Normal
                        wrapMode: Text.Wrap
                    }
                    Label { visible: root.cKind !== "milestone"; text: "How much" }
                    Field { id: amountField; look: root.look; visible: root.cKind !== "milestone"; Layout.fillWidth: true; hint: Logic.isNative(tokenField.text) ? "LEZ, e.g. 1000" : "In the token's smallest unit" }
                    Repeater {
                        model: root.cKind === "milestone" ? root.trancheCount : 0
                        delegate: RowLayout {
                            required property int index
                            Layout.fillWidth: true
                            spacing: 10
                            Field {
                                look: root.look
                                Layout.fillWidth: true
                                hint: "Milestone " + (index + 1) + (Logic.isNative(tokenField.text) ? ": LEZ" : "")
                                text: root.cTranches[index]
                                onTextChanged: root.cTranches[index] = text
                            }
                            Link { look: root.look; visible: root.trancheCount > 1; text: "Remove"; onPressed: { root.cTranches.splice(index, 1); root.trancheCount = root.cTranches.length } }
                        }
                    }
                    Link { look: root.look; visible: root.cKind === "milestone"; text: "Add a milestone"; onPressed: { root.cTranches.push(""); root.trancheCount = root.cTranches.length } }
                }
                ColumnLayout {
                    visible: root.batchMode
                    Layout.fillWidth: true
                    spacing: 8
                    Body { text: root.cKind === "milestone" ? "One row per person: their address or LNS name, and their milestone amounts separated by commas." : "One row per person: their address or LNS name, and how much. Up to 140 people in one transaction." }
                    Repeater {
                        model: root.rowCount
                        delegate: RowLayout {
                            required property int index
                            Layout.fillWidth: true
                            spacing: 8
                            Field {
                                look: root.look
                                Layout.fillWidth: true
                                Layout.preferredWidth: 3
                                hint: "Person " + (index + 1) + ": address or LNS name"
                                text: root.bRows[index].beneficiary
                                onTextChanged: root.bRows[index].beneficiary = text
                            }
                            Field {
                                look: root.look
                                Layout.fillWidth: true
                                Layout.preferredWidth: 2
                                hint: root.cKind === "milestone" ? "e.g. 400, 600" : "LEZ"
                                text: root.cKind === "milestone" ? root.bRows[index].tranches : root.bRows[index].amount
                                onTextChanged: root.bRows[index][root.cKind === "milestone" ? "tranches" : "amount"] = text
                            }
                            Link { look: root.look; visible: root.rowCount > 1; text: "Remove"; onPressed: { root.bRows.splice(index, 1); root.rowCount = root.bRows.length } }
                        }
                    }
                    Link { look: root.look; text: "Add a person"; onPressed: { root.bRows.push({ beneficiary: "", amount: "", tranches: "" }); root.rowCount = root.bRows.length } }
                }

                Tick { look: root.look; checked: root.cCancelable; text: "You can cancel it later"; onToggled: root.cCancelable = !root.cCancelable }
                Body { text: "Cancelable: until you make it permanent, you can stop it. What hasn't unlocked comes back to you; what has stays theirs." }
                Tick { look: root.look; checked: root.cTransferable; text: "They can hand it to someone else"; onToggled: root.cTransferable = !root.cTransferable }
                Body { text: "Transferable: the person receiving can pass the rest of the schedule to another account. This can't be changed later." }

                TechDetails {
                    look: root.look
                    title: "More options"
                    summary: "Another token; someone else who may cancel or unlock milestones"
                    Label { text: "Token" }
                    Field { id: tokenField; look: root.look; Layout.fillWidth: true; text: "LEZ"; hint: "LEZ, or a token's definition address" }
                    Body { text: "By default you decide: you can cancel, and you say when a milestone is reached. Name someone else here to give them that job instead, for example a board or a lawyer." }
                    Label { text: "Who may cancel (empty: you)" }
                    Field { id: cancelAuthField; look: root.look; Layout.fillWidth: true; hint: "Address or LNS name" }
                    Label { text: "Who says a milestone is reached (empty: you)" }
                    Field { id: milestoneAuthField; look: root.look; Layout.fillWidth: true; hint: "Address or LNS name" }
                }

                Body {
                    visible: !root.walletMode
                    color: look.ink
                    text: !root.namesCanCall
                        ? "Create needs Names Wallet 0.18 or later, which can sign a new schedule. Update Names, or use a wallet folder: Settings, Advanced."
                        : root.batchMode
                            ? "Several people in one transaction needs a wallet folder for now (Settings, Advanced). One person at a time works with Names Wallet."
                            : "Names Wallet opens with the schedule: choose the account that pays and approve. You come back here after."
                }
                Action {
                    look: root.look; strong: true; changes: true; busy: root.busy
                    enabledNow: root.walletMode || (root.namesCanCall && !root.batchMode)
                    label: root.batchMode ? "Create " + root.rowCount + " schedules" : "Create schedule"
                    busyLabel: "Creating…"; working: root.workingOn === "create" || root.workingOn === "batch"
                    onPressed: root.batchMode ? root.doBatch() : root.doCreate()
                }
            }

            Card {
                visible: root.tab === "pay" && root.canWrite && root.mySchedules.length === 0 && root.chainOpen
                Heading { text: "Nothing paid out yet" }
                Body { text: "Schedules you create appear here, with buttons to cancel them, make them permanent, or unlock a milestone." }
            }

            Card {
                visible: root.tab === "pay" && root.mySchedules.length > 0
                Heading { text: "Schedules you manage" }
                Repeater {
                    model: root.mySchedules
                    delegate: ColumnLayout {
                        id: managed
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: 6
                        Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: look.line }
                        Text { textFormat: Text.PlainText; Layout.fillWidth: true; text: Logic.scheduleTitle(managed.modelData); color: look.ink; font.family: look.sans; font.pixelSize: look.px(16); font.weight: Font.DemiBold }
                        Body {
                            text: "To " + Logic.labelled(managed.modelData.beneficiary, root.names) + " · claimed " + managed.modelData.claimed_display
                                + (managed.modelData.cancelled ? " · cancelled" : Logic.isFullyClaimed(managed.modelData) ? " · fully claimed" : managed.modelData.cancelable ? "" : " · permanent")
                        }
                        Flow {
                            visible: root.canWrite && !Logic.isClosed(managed.modelData)
                            Layout.fillWidth: true
                            spacing: 8
                            Action {
                                look: root.look; grave: true; changes: true; busy: root.busy
                                visible: managed.modelData.cancelable && managed.modelData.mine.cancel_authority
                                label: "Cancel schedule"; busyLabel: "Cancelling…"; working: root.workingOn === "cancel:" + managed.modelData.id
                                onPressed: root.askGrave("cancel", managed.modelData)
                            }
                            Action {
                                look: root.look; grave: true; changes: true; busy: root.busy
                                visible: managed.modelData.cancelable && managed.modelData.mine.cancel_authority
                                label: "Make it permanent"; busyLabel: "Updating…"; working: root.workingOn === "irrevocable:" + managed.modelData.id
                                onPressed: root.askGrave("irrevocable", managed.modelData)
                            }
                            Repeater {
                                model: managed.modelData.kind === "milestone" && managed.modelData.mine.milestone_authority ? managed.modelData.tranches : []
                                delegate: Action {
                                    required property int index
                                    look: root.look; changes: true; busy: root.busy
                                    visible: !managed.modelData.signalled[index]
                                    label: "Unlock milestone " + (index + 1); busyLabel: "Unlocking…"
                                    working: root.workingOn === "signal:" + managed.modelData.id + ":" + index
                                    onPressed: root.doSignal(managed.modelData, index)
                                }
                            }
                        }
                    }
                }
            }

            // ── Settings ────────────────────────────────────────────────────────────
            Card {
                visible: root.tab === "settings"
                Heading { text: "Signing" }
                Body { text: root.walletMode ? "Vest signs with the wallet folder below." : "Vest signs through Names Wallet: you approve every payment and claim in the Names app." }
                Action { look: root.look; visible: root.walletMode; strong: true; label: "Use Names Wallet instead"; changes: true; busy: root.busy; onPressed: root.saveSettings("names") }
                Link { look: root.look; visible: !root.walletMode && root.namesState === "connected"; text: "Disconnect Names Wallet"; onPressed: root.namesCall("drop_access", [], function() { root.namesState = "disconnected"; root.accounts = []; root.schedules = [] }) }
            }

            TechDetails {
                visible: root.tab === "settings"
                look: root.look
                title: "Advanced: a wallet folder instead of Names Wallet"
                summary: root.walletMode ? "In use" : "For people without Names Wallet"
                open: root.walletMode
                Body { text: "A wallet folder holds wallet_config.json and storage.json, made with the LEZ wallet's own command line. Vest then signs itself, and can create and manage schedules, which Names Wallet can't approve yet." }
                Label { text: "Wallet folder" }
                Field { id: walletField; look: root.look; Layout.fillWidth: true; code: true; hint: "e.g. ~/lez-wallet" }
                Label { text: "Its password (only if it has one; kept until Basecamp closes)" }
                Field { id: passwordField; look: root.look; Layout.fillWidth: true; Component.onCompleted: children[0].echoMode = TextInput.Password }
                Body { visible: root.walletMode && Logic.connectProblem(root.info) !== ""; text: Logic.connectProblem(root.info); color: look.alarm }
                Action {
                    look: root.look; changes: true; busy: root.busy
                    label: "Sign with this wallet folder"; busyLabel: "Connecting…"; working: root.workingOn === "settings"
                    onPressed: root.saveSettings("wallet")
                }
            }

            TechDetails {
                visible: root.tab === "settings"
                look: root.look
                title: "Network"
                summary: root.info && root.info.network && root.info.network.name ? Logic.clean(root.info.network.name) : ""
                FactRow { look: root.look; key: "RPC"; value: root.info && root.info.network ? root.info.network.rpc || "" : ""; monospace: true }
                FactRow { look: root.look; key: "Chain"; value: root.info && root.info.network ? root.info.network.chain_id || "" : ""; monospace: true }
                FactRow { look: root.look; key: "Vest program"; value: root.info && root.info.network ? root.info.network.program || "" : ""; monospace: true }
                FactRow { look: root.look; key: "Token program"; value: root.info && root.info.network ? root.info.network.token_program || "" : ""; monospace: true }
                FactRow { look: root.look; key: "LNS registry"; value: root.info && root.info.network ? root.info.network.lns_registry || "" : ""; monospace: true }
                FactRow { look: root.look; key: "Data folder"; value: root.info ? root.info.data_dir || "" : ""; monospace: true }
                Body { text: "To use another network, give its network file, or change single values below." }
                Label { text: "Network file" }
                Field { id: networkFileField; look: root.look; Layout.fillWidth: true; code: true; hint: "Empty: the built-in network" }
                Label { text: "RPC" }
                Field { id: rpcField; look: root.look; Layout.fillWidth: true; code: true }
                Label { text: "Chain id" }
                Field { id: chainField; look: root.look; Layout.fillWidth: true; code: true }
                Label { text: "Vest program" }
                Field { id: programField; look: root.look; Layout.fillWidth: true; code: true }
                Label { text: "Token program" }
                Field { id: tokenProgramField; look: root.look; Layout.fillWidth: true; code: true }
                Label { text: "LNS registry" }
                Field { id: lnsField; look: root.look; Layout.fillWidth: true; code: true }
                Label { text: "Clock program file (only if the network doesn't serve it)" }
                Field { id: clockField; look: root.look; Layout.fillWidth: true; code: true }
                Action { look: root.look; label: "Save network settings"; changes: true; busy: root.busy; onPressed: root.saveSettings(root.walletMode ? "wallet" : "names") }
            }

            // The result of the last step, and what happens next.
            Text {
                textFormat: Text.PlainText
                visible: root.resultLine !== ""
                Layout.fillWidth: true
                text: root.resultLine
                color: root.resultBad ? look.alarm : look.ink
                font.family: look.sans
                font.pixelSize: look.px(16)
                font.weight: Font.Medium
                wrapMode: Text.Wrap
            }
            Text {
                textFormat: Text.PlainText
                visible: root.nextLine !== ""
                Layout.fillWidth: true
                text: "What happens next: " + root.nextLine
                color: look.dim
                font.family: look.sans
                font.pixelSize: look.px(15)
                wrapMode: Text.Wrap
            }
            TechDetails {
                visible: root.resultBad && root.resultDetail !== ""
                look: root.look
                FactRow { look: root.look; key: "Detail"; value: root.resultDetail; monospace: true }
            }
        }
    }

    // ── Parts of this screen (the shared ones are in ./kit) ────────────────────────────

    component Card: Rectangle {
        default property alias content: cardColumn.data
        Layout.fillWidth: true
        implicitHeight: cardColumn.implicitHeight + 36
        radius: 8
        color: root.look.card
        border.width: 1
        border.color: root.look.line
        ColumnLayout {
            id: cardColumn
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: 18 }
            spacing: 10
        }
    }

    component Sheet: Card { border.color: root.look.brass }

    component Heading: Text {
        textFormat: Text.PlainText
        Layout.fillWidth: true
        color: root.look.ink
        font.family: root.look.sans
        font.pixelSize: root.look.px(19)
        font.weight: Font.DemiBold
        wrapMode: Text.Wrap
    }

    component Body: Text {
        textFormat: Text.PlainText
        Layout.fillWidth: true
        color: root.look.dim
        font.family: root.look.sans
        font.pixelSize: root.look.px(15)
        lineHeight: 1.2
        wrapMode: Text.Wrap
    }

    component Label: Text {
        textFormat: Text.PlainText
        Layout.fillWidth: true
        Layout.topMargin: 4
        color: root.look.dim
        font.family: root.look.sans
        font.pixelSize: root.look.px(14)
        font.weight: Font.Medium
        wrapMode: Text.Wrap
    }

    // A small drawing of how a kind of schedule unlocks: locked, a cliff, unlocking, fully unlocked.
    component Timeline: Item {
        id: line
        property string kind: "linear"
        Layout.fillWidth: true
        implicitHeight: 74
        onKindChanged: art.requestPaint()
        onWidthChanged: art.requestPaint()
        Canvas {
            id: art
            anchors { left: parent.left; right: parent.right; top: parent.top }
            height: 52
            onPaint: {
                var ctx = getContext("2d")
                ctx.reset()
                var w = width, h = height, base = h - 6, top = 6
                ctx.strokeStyle = root.look.line
                ctx.lineWidth = 1
                ctx.beginPath(); ctx.moveTo(0, base + 0.5); ctx.lineTo(w, base + 0.5); ctx.stroke()
                ctx.strokeStyle = root.look.brass
                ctx.lineWidth = 2
                ctx.beginPath()
                ctx.moveTo(0, base)
                if (line.kind === "cliff-linear") {
                    var c = w * 0.3, atCliff = base - (base - top) * 0.3
                    ctx.lineTo(c, base); ctx.lineTo(c, atCliff); ctx.lineTo(w, top)
                } else if (line.kind === "milestone") {
                    var a = w * 0.25, b = w * 0.6, s1 = base - (base - top) * 0.4
                    ctx.lineTo(a, base); ctx.lineTo(a, s1); ctx.lineTo(b, s1); ctx.lineTo(b, top); ctx.lineTo(w, top)
                } else {
                    ctx.lineTo(w, top)
                }
                ctx.stroke()
            }
        }
        RowLayout {
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            Text { textFormat: Text.PlainText; text: line.kind === "milestone" ? "locked" : line.kind === "cliff-linear" ? "locked until the cliff" : "start"; color: root.look.dim; font.family: root.look.sans; font.pixelSize: root.look.px(12) }
            Item { Layout.fillWidth: true }
            Text { textFormat: Text.PlainText; text: line.kind === "milestone" ? "each milestone unlocks a part" : "unlocking"; color: root.look.dim; font.family: root.look.sans; font.pixelSize: root.look.px(12) }
            Item { Layout.fillWidth: true }
            Text { textFormat: Text.PlainText; text: "fully unlocked"; color: root.look.dim; font.family: root.look.sans; font.pixelSize: root.look.px(12) }
        }
    }

    component ImportRow: RowLayout {
        Layout.fillWidth: true
        spacing: 8
        Field { id: importInput; look: root.look; Layout.fillWidth: true; hint: "Schedule address or LNS name"; onTextChanged: importField.text = text }
        Action {
            look: root.look; changes: true; busy: root.busy
            label: "Add schedule"; busyLabel: "Adding…"; working: root.workingOn === "import"
            onPressed: { importField.text = importInput.text; root.doImport() }
        }
        Connections { target: importField; function onTextChanged() { if (importField.text === "") importInput.text = "" } }
    }
    TextInput { id: importField; visible: false }

    component Choice: Rectangle {
        id: choice
        property string label: ""
        property string detail: ""
        property bool chosen: false
        signal pressed()
        implicitHeight: 42
        radius: 6
        color: choiceArea.containsMouse ? root.look.raised : "transparent"
        border.width: 1
        border.color: chosen ? root.look.brass : root.look.line
        activeFocusOnTab: true
        Keys.onSpacePressed: choice.pressed()
        RowLayout {
            anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
            spacing: 10
            Rectangle {
                implicitWidth: 14; implicitHeight: 14; radius: 7
                color: choice.chosen ? root.look.brass : "transparent"
                border.width: 1
                border.color: choice.chosen ? root.look.brass : root.look.dim
            }
            Text { textFormat: Text.PlainText; Layout.fillWidth: true; text: choice.label; color: root.look.ink; font.family: root.look.sans; font.pixelSize: root.look.px(15); elide: Text.ElideRight }
            Text { textFormat: Text.PlainText; text: choice.detail; color: root.look.dim; font.family: root.look.sans; font.pixelSize: root.look.px(14) }
        }
        MouseArea { id: choiceArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: choice.pressed() }
    }

    component ChoiceInline: Rectangle {
        id: ci
        property string label: ""
        property bool chosen: false
        signal pressed()
        implicitWidth: ciText.implicitWidth + 24
        implicitHeight: 34
        radius: 6
        color: ci.chosen ? root.look.raised : (ciArea.containsMouse ? root.look.raised : "transparent")
        border.width: 1
        border.color: ci.chosen ? root.look.brass : root.look.line
        activeFocusOnTab: true
        Keys.onSpacePressed: ci.pressed()
        Text { id: ciText; textFormat: Text.PlainText; anchors.centerIn: parent; text: ci.label; color: root.look.ink; font.family: root.look.sans; font.pixelSize: root.look.px(15); font.weight: Font.Medium }
        MouseArea { id: ciArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: ci.pressed() }
    }

    component Tab: Rectangle {
        id: tabButton
        property string label: ""
        property bool active: false
        signal pressed()
        implicitWidth: tabText.implicitWidth + 28
        implicitHeight: 36
        radius: 6
        color: tabButton.active ? root.look.card : (tabArea.containsMouse ? root.look.raised : "transparent")
        border.width: 1
        border.color: tabButton.active ? root.look.brass : root.look.line
        activeFocusOnTab: true
        Keys.onSpacePressed: tabButton.pressed()
        Text { id: tabText; textFormat: Text.PlainText; anchors.centerIn: parent; text: tabButton.label; color: tabButton.active ? root.look.ink : root.look.dim; font.family: root.look.sans; font.pixelSize: root.look.px(16); font.weight: Font.DemiBold }
        MouseArea { id: tabArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: tabButton.pressed() }
    }
}
