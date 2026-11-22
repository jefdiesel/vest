// Vest: pay tokens out over time, or claim what has vested to you (Logos RFP-017).
//
// Two modules do the work. The `vesting` core module (../module) reads the network: schedules,
// the clock, what is claimable, and builds the claim call. The person's Names Wallet (the `names`
// module, its Wallet Provider SDK) shows Vest the accounts the person chooses and signs every
// change once the person approves it in the Names app: Vest never sees a key, and has no signer
// of its own. Vest talks to the wallet only through the Names Wallet SDK (NamesWallet.qml +
// NamesWalletSDK.mjs, copied unchanged from the Names repository's sdk/qml/), which finds the
// wallet installed (Names, or another that implements the SDK) and never makes Vest name one.
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

    // ── Names Wallet (the Names Wallet SDK) ────────────────────────────────────────
    NamesWallet { id: wallet; app: "vesting_ui"; bridge: typeof logos !== "undefined" ? logos : null }
    // "checking" | "missing" | "otherZone" | "disconnected" | "asking" | "connected"
    readonly property string namesState: wallet.status
    readonly property string namesProblem: namesState === "otherZone" ? Logic.OTHER_ZONE : wallet.problem
    property var accessRequest: null           // the connect request while it waits
    property var claimRequest: null            // the claim request while it waits
    property var claimWatch: null              // its watch (withdraw)
    property var writeRequest: null            // a create, cancel, permanent, milestone or transfer while it waits
    // Names Wallet 0.18+: its calls may read the network clock
    readonly property bool namesCanCall: wallet.features.indexOf("call_reads_clock") >= 0
    readonly property bool connected: namesState === "connected" && chainOpen
    onConnectedChanged: {
        if (connected) {
            startScan()
        } else {
            resetScan()
            if (namesState !== "connected" && accountsSent) {
                accountsSent = false
                call("set_accounts", ["[]"], function() {})
            }
        }
    }
    // Creates, cancels, milestones and transfers need Names Wallet 0.18+ (calls that read the clock).
    readonly property bool canWrite: namesCanCall

    // ── View state ──────────────────────────────────────────────────────────────────
    property string tab: "claimable"           // claimable | unvested | pay | settings (the left menu)
    property bool guideOpen: false
    property string guideRole: "receive"
    property real tick: Date.now()
    property var recipientId: null
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
    property bool fromFull: false             // the From row shows the whole address
    property var receipt: null                 // the last create's receipt (Logic.receiptOf input)
    property var shownAddr: ({})               // which receipt rows show their full address
    property bool autoTabDone: false           // the first read picked Unvested when nothing is claimable

    // ── Balance retry (when Names Wallet isn't ready with balances yet) ────────────
    property var balanceAccounts: []           // the granted accounts we are reading balances for
    property int balanceTries: 0               // tries made so far (1 initial + 3 retries = 4)
    property bool balanceFailed: false         // all tries failed: show "Balance not read · Refresh"
    property int balanceGen: 0                 // bumped on each fresh read or disconnect

    // ── Finding schedules on the chain (the module's `discover`) ───────────────────
    property var scan: null                    // the last discover answer
    property bool scanBusy: false              // a discover call is out (one at a time)
    property bool scanStarted: false
    property string scanProblem: ""
    property var newFound: []                  // schedules a scan added: [{id, address, creator}]
    property bool scanFoundUnread: false       // a scan added schedules the list hasn't read yet
    property real scanReadAt: 0                // when a scan last made the list read again (ms)
    property int scanGen: 0                    // bumped on disconnect or network change to drop stale replies
    property bool accountsSent: false          // set_accounts finished for the current connection

    // ── Forms ───────────────────────────────────────────────────────────────────────
    property string cCreator: ""
    property string cKind: "linear"
    property bool cCancelable: true
    property bool cTransferable: false
    property var cTranches: [""]
    // The tranches above are changed in place while typing (a new array would rebuild the fields
    // under the cursor, one character at a time); this count rebuilds them only on add and remove.
    property int trancheCount: 1
    property var payee: null                   // the payout box, checked: {ok, text} or null

    // ── Derived ─────────────────────────────────────────────────────────────────────
    readonly property var recipientSchedules: schedules.filter(function(s) { return s.mine.beneficiary })
    readonly property var claimableSchedules: Logic.paying(schedules, "claimable")
    readonly property var unvestedSchedules: Logic.paying(schedules, "unvested")
    readonly property var closedSchedules: Logic.paying(schedules, "closed")
    readonly property bool receiving: tab === "claimable" || tab === "unvested"
    // The schedules of the menu entry on screen; the detail shows the chosen one (or the first).
    readonly property var shownSchedules: tab === "claimable" ? claimableSchedules
        : tab === "unvested" ? unvestedSchedules.concat(closedSchedules) : []
    readonly property var menu: Logic.menuItems(schedules)
    // A wide window has the menu on the left; a narrow one has it as a row at the top.
    readonly property bool wide: width >= 860
    readonly property bool menuShown: connected || tab === "settings"
    readonly property string scanText: scanBusy && scan === null ? "Looking for schedules on the chain…" : Logic.scanLine(scan)
    readonly property string newText: Logic.newLine(newFound, names)
    readonly property var mySchedules: schedules.filter(function(s) {
        return s.mine.creator || s.mine.cancel_authority || s.mine.milestone_authority })
    readonly property var selected: findById(shownSchedules, recipientId) || (shownSchedules.length ? shownSchedules[0] : null)
    readonly property var publicAccounts: accounts.filter(function(a) { return !a["private"] })
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

    // ── Names Wallet (the Names Wallet SDK: busy retry, one timer per wait, states) ──

    /// Where Names Wallet stands (installed and open, on our network, which accounts it gave),
    /// then the vesting module is told the accounts and the chain is read.
    function readNames() {
        stopBalanceRetry()
        wallet.check({ chain: info && info.network ? info.network.chain_id : "" }, function(w) {
            if (w.state !== "connected") { loading = false; return }
            namesAccountsRead(w.accounts)
        })
    }

    function stopBalanceRetry() {
        balanceRetryTimer.stop()
        balanceGen += 1
        balanceTries = 0
        balanceFailed = false
        balanceAccounts = []
    }

    function readAccounts() {
        call("accounts", [], function(a) {
            if (!a.ok) return
            accounts = a.accounts || []
            // "Paid from" must be an account Names Wallet gives now (not one of an earlier connection).
            if (!findById(publicAccounts, cCreator)) cCreator = publicAccounts.length ? publicAccounts[0].id : ""
        })
    }

    function scheduleBalanceRetry(gen) {
        if (gen !== balanceGen) return
        var wait = Logic.balanceRetryDelay(balanceTries)
        if (wait > 0) {
            balanceRetryTimer.interval = wait
            balanceRetryTimer.restart()
        } else {
            balanceFailed = true
        }
    }

    function namesAccountsRead(grantedAccounts) {
        stopBalanceRetry()
        balanceAccounts = grantedAccounts || []
        balanceTries = 1
        var gen = balanceGen
        wallet.balances(function(b) {
            if (gen !== balanceGen) return
            var rows = Logic.namesAccounts({ accounts: balanceAccounts }, b)
            call("set_accounts", [JSON.stringify(rows)], function() {
                if (gen !== balanceGen) return
                accountsSent = true
                startScan()
                readChain()
            })
            if (!Logic.balancesKnown(rows)) scheduleBalanceRetry(gen)
        }, function() {
            if (gen !== balanceGen) return
            var rows = Logic.namesAccounts({ accounts: balanceAccounts }, [])
            call("set_accounts", [JSON.stringify(rows)], function() {
                if (gen !== balanceGen) return
                accountsSent = true
                startScan()
                readChain()
            })
            if (!Logic.balancesKnown(rows)) scheduleBalanceRetry(gen)
        })
    }

    function retryBalances() {
        if (namesState !== "connected" || !balanceAccounts.length) { stopBalanceRetry(); return }
        balanceTries += 1
        var gen = balanceGen
        wallet.balances(function(b) {
            if (gen !== balanceGen) return
            var rows = Logic.namesAccounts({ accounts: balanceAccounts }, b)
            call("set_accounts", [JSON.stringify(rows)], function() {
                if (gen === balanceGen) readAccounts()
            })
            if (Logic.balancesKnown(rows)) balanceFailed = false
            else scheduleBalanceRetry(gen)
        }, function() {
            if (gen !== balanceGen) return
            scheduleBalanceRetry(gen)
        })
    }
    Timer { id: balanceRetryTimer; repeat: false; onTriggered: root.retryBalances() }

    /// 1. Connect: the person picks, in the Names app, which accounts Vest may see. The SDK takes
    /// them to Names (Basecamp's navigation intent, declared in metadata.json `uses`) and Names
    /// brings them back once they answer.
    function connectNames() {
        say("")
        wallet.connect("To show your schedules and claim what has unlocked for you", {}, {
            waiting: function(v) { root.accessRequest = v },
            done: function(v) {
                root.accessRequest = null
                if (v.state === "approved") {
                    if (wallet.status === "connected") namesAccountsRead(wallet.accounts)
                    else loading = false
                    say("Connected.", false, Logic.nextLine("connect"))
                } else say(Logic.requestLine(v, "the connection"), true)
            },
            failed: function(r) { root.accessRequest = null; say(r.error, true) }
        })
    }

    // ── Reading ─────────────────────────────────────────────────────────────────────

    /// The vesting module's settings, then the wallet. Vest signs only through the wallet: a
    /// wallet-folder signer saved by an older Vest is switched off once.
    function refresh() {
        loading = true
        call("settings", [], function(r) {
            if (!r.ok) { loading = false; if (r.missing) say(r.error, true); return }
            info = r
            fillSettings(r)
            if (r.signer === "wallet") {
                call("configure", [JSON.stringify({ signer: "names" })], function(again) { if (again.network) info = again; readNames() }, 60000)
                return
            }
            readNames()
        })
    }

    function readChain() {
        readAccounts()
        call("now", [], function(n) { if (n.ok) effectiveTime = n.time })
        call("list", [], function(l) {
            loading = false
            call("settings", [], function(again) { if (again.ok) info = again })
            if (!l.ok) { say(l.error || "Couldn't read your schedules.", true); return }
            schedules = (l.schedules || []).map(Logic.fromModule)
            names = l.names || ({})
            learnNames()
            if (l.time) effectiveTime = l.time
            if (!findById(recipientSchedules, recipientId))
                recipientId = recipientSchedules.length ? recipientSchedules[0].id : null
            if (!autoTabDone && recipientSchedules.length) {
                autoTabDone = true
                if (tab === "claimable" && claimableSchedules.length === 0 && unvestedSchedules.length > 0) tab = "unvested"
            }
        }, 120000)
    }

    function resetScan() {
        scanTimer.stop()
        scanGen += 1
        scanStarted = false
        scan = null
        newFound = []
        scanProblem = ""
        scanFoundUnread = false
    }

    function startScan() {
        if (!connected || !accountsSent || scanStarted) return
        scanStarted = true
        discoverOnce()
    }

    function onDisconnected() {
        stopBalanceRetry()
        resetScan()
        accountsSent = false
        accounts = []
        cCreator = ""
        schedules = []
        call("set_accounts", ["[]"], function() {})
    }

    /// Look for schedules on the chain: one `discover` call at a time, the next as soon as the
    /// last one answers, until the scan reaches the chain's tip; then again every minute. A
    /// schedule it adds or changes is read with the rest (`list`), and new ones paying you are
    /// said in one line.
    function discoverOnce() {
        if (scanBusy || !connected) return
        scanBusy = true
        var gen = scanGen
        call("discover", ["2000"], function(r) {
            scanBusy = false
            if (!connected || gen !== scanGen) {
                if (connected && accountsSent) {
                    scanStarted = true
                    Qt.callLater(discoverOnce)
                }
                return
            }
            // New or changed schedules are read with the rest (`list`): when the scan is done, or at most
            // every 30 s while it runs (each read asks the chain for every schedule).
            if (Logic.scanHasChanges(r)) scanFoundUnread = true
            if (scanFoundUnread && (r.done || !r.ok || Date.now() - scanReadAt > 30000)) {
                scanFoundUnread = false
                scanReadAt = Date.now()
                readChain()
            }
            if (!r.ok) {
                scanProblem = r.error || "no answer"
                scanTimer.restart()
                return
            }
            scanProblem = ""
            scan = r
            if (r["new"] && r["new"].length) newFound = newFound.concat(r["new"])
            if (r.done) scanTimer.restart()
            else Qt.callLater(discoverOnce)
        }, 120000)
    }
    Timer { id: scanTimer; interval: 60000; onTriggered: root.discoverOnce() }

    /// Show LNS names, not addresses: ask the wallet for the confirmed name of every account on
    /// screen (creators, people receiving, our own accounts) and keep it in the name book.
    function learnNames() {
        if (namesState !== "connected") return
        var seen = {}, list = []
        var add = function(hex) { var h = String(hex || "").toLowerCase(); if (/^[0-9a-f]{64}$/.test(h) && !seen[h] && !names[h]) { seen[h] = true; list.push(h) } }
        schedules.forEach(function(sc) { add(sc.creator); add(sc.beneficiary) })
        accounts.forEach(function(a) { add(a.id) })
        list.slice(0, 24).forEach(function(h) {
            wallet.reverse(Logic.base58Of(h), function(rv) {
                if (!rv || !rv.ok || !rv.name) return
                var m = Object.assign({}, root.names); m[h] = rv.name; root.names = m
            })
        })
    }

    function fillSettings(r) {
        var s = r.settings || {}
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

    function saveNetwork() {
        say("")
        busy = true
        workingOn = "settings"
        resetScan()
        var patch = {
            network_file: networkFileField.text.trim(), rpc: rpcField.text.trim(), chain_id: chainField.text.trim(),
            program: programField.text.trim(), token_program: tokenProgramField.text.trim(),
            lns_registry: lnsField.text.trim(), clock_elf: clockField.text.trim()
        }
        call("configure", [JSON.stringify(patch)], function(r) {
            busy = false
            workingOn = ""
            if (r.network) info = r
            if (r.ok) say("Network settings saved.")
            else say(r.error, true)
            refresh()
        }, 60000)
    }

    function doImport() {
        var a = Logic.accountInput(importField.text)
        if (!a.ok) { say("Paste the schedule code the person paying you sent.", true); return }
        busy = true
        workingOn = "import"
        call("import_schedule", [a.account], function(r) {
            busy = false
            workingOn = ""
            if (!r.ok) { say(Logic.importProblem(importField.text, r.error), true); return }
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
        if (connected) tab = role === "pay" ? "pay" : (claimableSchedules.length || !unvestedSchedules.length ? "claimable" : "unvested")
    }
    onTabChanged: { if (tab !== "settings") guideRole = tab === "pay" ? "pay" : "receive"; confirm = null }

    /// The module builds the claim call; the person sees it here, then approves it in their
    /// wallet. Claims go to the schedule's (public) beneficiary account.
    function beginClaim() {
        var s = selected
        if (!s) return
        say("")
        busy = true
        workingOn = "summary"
        call("claim_call", [String(s.id)], function(r) {
            busy = false
            workingOn = ""
            if (!r.ok) { say(r.error, true); return }
            confirm = { id: String(s.id), names: r, amountDisplay: r.amount_display,
                        destAccount: s.beneficiary, fee: { ok: true, message: "Names Wallet shows the network fee before you approve; it is paid from this account." } }
        }, 120000)
    }

    function confirmClaim() {
        if (!confirm || !confirm.fee.ok) return
        var c = confirm
        busy = true
        workingOn = "claim"
        var shown = function(v) { v.amountDisplay = c.amountDisplay; root.claimRequest = v }
        wallet.proposeCall(c.names.program, c.names.accounts_json, c.names.instruction_hex, c.names.summary, {
            sent: function(v, watch) { root.claimWatch = watch; shown(v) },
            update: shown,
            done: function(v) {
                v.amountDisplay = c.amountDisplay
                root.busy = false
                root.workingOn = ""
                root.claimRequest = null
                root.claimWatch = null
                root.confirm = null
                if (v.state === "done") root.say("Claimed " + (v.amountDisplay || "") + (v.tx ? " · transaction " + Logic.short(v.tx) : "") + ".", false, Logic.nextLine("claim"))
                else root.say(Logic.requestLine(v, "the claim"), v.state === "failed")
                root.refresh()
            },
            failed: function(r) { root.claimRequest = null; root.claimWatch = null; busy = false; workingOn = ""; say(r.error, true) }
        })
    }

    function withdrawClaim() {
        if (claimRequest) wallet.withdraw(claimRequest.request, function() {}, function(r) { say(r.error, true) })
    }

    function doTransfer(id, target) {
        var typed = Logic.accountInput(target)
        if (!typed.ok) { say("Enter the address or LNS name of the person receiving.", true); return }
        if (Logic.problemRole(typed.account) !== "") { say(Logic.problemRole(typed.account), true); return }
        namesWrite("transfer", "transfer_call", [String(id), typed.account], function() {
            transferField.text = ""
            say("Schedule " + id + " now goes to " + typed.account + ".", false, Logic.nextLine("transfer"))
        })
    }

    // ── Paying out ──────────────────────────────────────────────────────────────────

    /// Vest pays out LEZ (the wallet can't approve a token schedule yet).
    function amountOf(text) {
        return Logic.toBase(text)
    }

    function sharedParams() {
        return {
            kind: cKind, token: "LEZ",
            start: startField.ms, cliff: cliffField.ms, end: endField.ms,
            cancelable: cCancelable, transferable: cTransferable,
            cancelAuthority: cancelAuthField.text.trim(), milestoneAuthority: milestoneAuthField.text.trim()
        }
    }

    /// A change signed through Names Wallet: the module builds the call (`<method>` → {program,
    /// accounts_json, instruction_hex, summary, caller}), Names shows it and the person approves,
    /// choosing `caller`. `done(built, status)` runs once Names says it was sent.
    function namesWrite(what, method, args, done, summaryFor) {
        say("")
        busy = true
        workingOn = what
        call(method, args, function(r) {
            if (!r.ok) { busy = false; workingOn = ""; say(r.error, true); return }
            var thing = Logic.writeWhat(what)
            // The summary Names shows is Vest's own words: say the LNS name the person typed, not only an address.
            var summary = summaryFor ? summaryFor(r) : r.summary
            wallet.proposeCall(r.program, r.accounts_json, r.instruction_hex, summary, {
                sent: function(v) {
                    root.writeRequest = { request: v.request, built: r, done: done, what: thing }
                    root.say("In Names Wallet, choose " + Logic.labelled(r.caller, root.names) + " and approve. You come back here after.")
                },
                done: function(v) {
                    root.writeRequest = null
                    root.busy = false
                    root.workingOn = ""
                    if (v.state === "done") done(r, v)
                    else root.say(Logic.requestLine(v, thing), v.state === "failed")
                    root.refresh()
                },
                failed: function(e) { root.writeRequest = null; root.busy = false; root.workingOn = ""; root.say(e.error, true) }
            })
        }, 120000)
    }

    /// Look up what was typed in the payout box (an LNS name or an address) and say who it is.
    /// A name goes to Names Wallet's own lookup when it is connected (the schedule is then made
    /// for the account Names found); otherwise, and for an address, to the vesting module.
    function checkPayee() {
        var t = beneficiaryField.text.trim()
        if (t === "") { payee = null; return }
        payee = { ok: false, checking: true, text: "Checking " + t + "…" }
        // Ask Names Wallet and Vest's own lookup (it reads the chain directly) at once; the first
        // account found wins, so a busy Names Wallet (catching up with the chain) never leaves this
        // spinning, and a failure of one waits for the other (Logic.payeePick).
        var answered = false
        var askNames = namesState === "connected" && Logic.accountKind(t) === "name"
        var out = askNames ? 2 : 1
        var earlier = null
        var settle = function(v) {
            out -= 1
            if (answered || beneficiaryField.text.trim() !== t) return
            var pick = Logic.payeePick(v, out > 0, earlier)
            if (pick === null) { earlier = v; return }
            answered = true
            payee = pick
        }
        if (askNames)
            wallet.resolve(t, function(r) { settle(Logic.payeeFromNames(t, r)) })
        call("resolve_name", [t], function(r) { settle(Logic.payeeLine(t, r)) }, 30000)
    }

    Timer { id: payeeTimer; interval: 700; onTriggered: root.checkPayee() }

    function doCreate() {
        if (cCreator === "") { say("Choose which of your accounts pays.", true); return }
        if (root.payee && !root.payee.ok && !root.payee.checking) { say(root.payee.text, true); return }
        var p = sharedParams()
        p.beneficiary = beneficiaryField.text.trim()
        // A name Names Wallet looked up: the schedule is for the account it found.
        var found = Logic.payeeAccount(root.payee, p.beneficiary)
        if (found !== "") p.beneficiary = found
        p.amount = amountOf(amountField.text)
        p.tranches = cKind === "milestone" ? cTranches.map(amountOf) : []
        var v = Logic.validateCreate(p)
        if (!v.ok) { say(v.errors[0], true); return }
        // Enough LEZ in the paying account? Say so here, before Names Wallet is asked.
        var payer = findById(publicAccounts, cCreator)
        var notEnough = Logic.shortOfFunds(payer ? payer.balance : null, p.amount, p.tranches)
        if (notEnough !== "") { say(notEnough, true); return }
        var looked = found !== "" ? root.payee.resolved : []
        var typed = beneficiaryField.text.trim()
        // What was sent, kept for the receipt: the form may change while Names Wallet asks.
        var creatorId = cCreator
        var sentPayee = root.payee
        namesWrite("create", "create_call", [JSON.stringify(Logic.toCreateRow(p, creatorId))], function(built, v) {
            justCreated = true
            call("import_schedule", [built.schedule_address], function(imp) {
                var id = imp.ok ? imp.id : ""
                var payerAcct = payer
                    var shown = function(nm) {
                        var toAddr = Logic.receiptTo(sentPayee, p.beneficiary, typed, built.resolved)
                        root.shownAddr = ({})
                        root.receipt = {
                            id: id, amountDisplay: built.amount_display, kind: p.kind, start: p.start, cliff: p.cliff, end: p.end,
                            cancelable: p.cancelable, transferable: p.transferable,
                            fromAddr: payerAcct ? payerAcct.base58 : "", fromName: nm,
                            toAddr: toAddr, toName: Logic.accountKind(typed) === "name" ? typed : (sentPayee && sentPayee.name ? sentPayee.name : ""),
                            tx: v.tx || "", code: built.schedule_address || ""
                        }
                        say(Logic.resolvedLines(looked.concat(built.resolved || [])).concat(["Created " + (id !== "" ? "schedule " + id : "the schedule") + " · " + built.amount_display + (v.tx ? " · transaction " + Logic.short(v.tx) : "") + "."]).join(" "),

                            false, Logic.nextLine("create", { id: id, name: nm }))
                    }
                    if (payerAcct && payerAcct.base58) wallet.reverse(payerAcct.base58, function(rv) { shown(rv && rv.ok && rv.name ? rv.name : "") })
                    else shown("")
                refresh()
            }, 60000)
        }, function(built) { return Logic.createSummary(built, typed) })
    }

    function askGrave(kind, s) { sheet = { kind: kind, schedule: s } }

    function doGrave() {
        var k = sheet.kind, s = sheet.schedule
        sheet = null
        if (k === "cancel")
            namesWrite("cancel:" + s.id, "cancel_call", [String(s.id)], function(built) {
                say("Cancelled schedule " + s.id + ". " + built.amount_display + " went back to the creator.", false, Logic.nextLine("cancel"))
            })
        else
            namesWrite("irrevocable:" + s.id, "irrevocable_call", [String(s.id)], function() {
                say("Schedule " + s.id + " is now permanent.", false, Logic.nextLine("irrevocable"))
            })
    }

    function doSignal(s, index) {
        namesWrite("signal:" + s.id + ":" + index, "signal_call", [String(s.id), String(index)], function(built) {
            say("Unlocked milestone " + (index + 1) + " of schedule " + s.id + ": " + built.amount_display + ".", false, Logic.nextLine("signal"))
        })
    }

    // ── Screen ──────────────────────────────────────────────────────────────────────

    Rectangle { anchors.fill: parent; color: look.slate }

    // ── The left menu (a wide window) ─────────────────────────────────────────────────
    Rectangle {
        id: sideMenu
        visible: root.wide && root.menuShown
        anchors { left: parent.left; top: parent.top; bottom: parent.bottom }
        width: visible ? 216 : 0
        color: look.card
        Rectangle { anchors { right: parent.right; top: parent.top; bottom: parent.bottom } width: 1; color: look.line }
        ColumnLayout {
            anchors { left: parent.left; right: parent.right; top: parent.top; leftMargin: 14; rightMargin: 14; topMargin: 40 }
            spacing: 6
            Text { textFormat: Text.PlainText; text: "Vest"; color: look.dim; font.family: look.sans; font.pixelSize: look.px(14); font.weight: Font.DemiBold; Layout.leftMargin: 10; Layout.bottomMargin: 6 }
            Repeater {
                model: root.menu
                delegate: MenuItem {
                    required property var modelData
                    Layout.fillWidth: true
                    label: modelData.label
                    count: modelData.count
                    active: root.tab === modelData.key
                    onPressed: root.tab = modelData.key
                }
            }
            Link { look: root.look; Layout.leftMargin: 10; Layout.topMargin: 10; visible: root.connected; text: root.loading ? "Reading…" : "Refresh"; onPressed: if (!root.loading) root.refresh() }
        }
    }

    Flickable {
        id: page
        anchors { left: sideMenu.visible ? sideMenu.right : parent.left; right: parent.right; top: parent.top; bottom: parent.bottom }
        contentHeight: column.implicitHeight + 80
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ColumnLayout {
            id: column
            x: sideMenu.visible ? 40 : page.width >= 600 ? look.edge : 24
            y: 40
            width: Math.min(page.width - 2 * x, look.pageWidth)
            spacing: 16

            RowLayout {
                Layout.fillWidth: true
                spacing: 16
                PageHeader {
                    look: root.look
                    title: "Vest"
                    sentence: "Pay tokens out over time, or claim what has unlocked for you."
                    runsOnPrefix: "Runs on"
                    network: root.networkName !== "" ? Logic.clean(root.networkName) : ""
                    networkDetail: root.info && root.info.network && root.info.network.rpc ? Logic.hostOf(root.info.network.rpc) : ""
                }
                Link { look: root.look; Layout.alignment: Qt.AlignTop; visible: root.connected; text: root.showGuide ? "Hide steps" : "Steps"; onPressed: root.showGuide ? root.finishGuide() : root.guideOpen = true }
            }

            // ── The wallet: none, several, not running, another network, asking ──────
            Card {
                visible: ["missing", "none", "unsupported"].indexOf(root.namesState) >= 0 && root.tab !== "settings"
                Heading { text: "Vest needs Names Wallet" }
                Body { text: root.namesProblem }
                Action { look: root.look; label: "Check again"; onPressed: root.refresh() }
            }
            Card {
                visible: root.namesState === "choose" && root.tab !== "settings"
                Heading { text: "Which wallet should Vest use?" }
                Body { text: "More than one wallet is installed. Vest remembers your choice; you can change it in Settings." }
                Repeater {
                    model: root.namesState === "choose" ? wallet.wallets : []
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: Logic.clean(modelData.display_name)
                        detail: Logic.clean(modelData.module)
                        chosen: false
                        onPressed: wallet.chooseWallet(modelData.module)
                    }
                }
            }
            Card {
                visible: root.namesState === "otherZone" && root.tab !== "settings"
                Heading { text: "Switch Names Wallet to the bridge test network" }
                Body { text: root.namesProblem }
                Action { look: root.look; label: "Check again"; onPressed: root.refresh() }
            }
            // ── Steps: what to do, one at a time ─────────────────────────────────────
            // The first screen: pick what you came to do, then numbered steps. The step you are
            // on says what to do and holds its button (step 1: connect Names Wallet).
            Card {
                visible: root.showGuide && ["missing", "none", "choose", "unsupported", "otherZone"].indexOf(root.namesState) < 0
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
                                text: Logic.NAMES_CANT_PAY
                            }
                            RowLayout {
                                visible: step.now && step.modelData.n === 1
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
                    Action { look: root.look; label: "Hide the steps"; visible: root.connected; onPressed: root.finishGuide() }
                    Link { look: root.look; text: "Read the full guide"; onPressed: Qt.openUrlExternally(Logic.GUIDE_URL) }
                }
            }

            // A narrow window: the menu as a row at the top.
            Flow {
                visible: !root.wide && root.menuShown
                Layout.fillWidth: true
                spacing: 8
                Repeater {
                    model: root.menu
                    delegate: MenuItem {
                        required property var modelData
                        label: modelData.label
                        count: modelData.count
                        active: root.tab === modelData.key
                        inline: true
                        onPressed: root.tab = modelData.key
                    }
                }
                Link { look: root.look; visible: root.connected; text: root.loading ? "Reading…" : "Refresh"; onPressed: if (!root.loading) root.refresh() }
            }

            // Looking for schedules on the chain: a quiet line, and one when new ones arrive.
            Text {
                textFormat: Text.PlainText
                visible: root.connected && root.scanText !== "" && root.scanProblem === ""
                Layout.fillWidth: true
                text: root.scanText
                color: look.dim
                font.family: look.sans
                font.pixelSize: look.px(14)
                wrapMode: Text.Wrap
            }
            Text {
                textFormat: Text.PlainText
                visible: root.connected && root.scanProblem !== ""
                Layout.fillWidth: true
                text: "Couldn't look for schedules on the chain just now (" + Logic.clean(root.scanProblem) + "). Vest tries again in a minute."
                color: look.dim
                font.family: look.sans
                font.pixelSize: look.px(14)
                wrapMode: Text.Wrap
            }
            RowLayout {
                visible: root.connected && root.newText !== ""
                Layout.fillWidth: true
                spacing: 12
                Text { textFormat: Text.PlainText; Layout.fillWidth: true; text: root.newText; color: look.ink; font.family: look.sans; font.pixelSize: look.px(15); font.weight: Font.DemiBold; wrapMode: Text.Wrap }
                Link { look: root.look; text: "Hide"; onPressed: root.newFound = [] }
            }

            // ── Claimable and Unvested: schedules paying you ────────────────────────────
            Card {
                visible: root.tab === "claimable" && root.connected && root.claimableSchedules.length === 0
                Heading { text: "Nothing to claim right now" }
                Body {
                    text: root.unvestedSchedules.length > 0
                        ? Logic.grouped(String(root.unvestedSchedules.length)) + (root.unvestedSchedules.length === 1 ? " schedule pays you later" : " schedules pay you later") + ": see Unvested for when."
                        : "When a schedule paying you has unlocked tokens, it shows up here. Vest finds schedules paying your accounts on the chain by itself."
                }
            }
            Card {
                visible: root.tab === "unvested" && root.connected && root.unvestedSchedules.length === 0 && root.closedSchedules.length === 0
                Heading { text: "No schedules on their way to you" }
                Body { text: "Vest looks on the chain for schedules paying your accounts and lists them here until they unlock. If someone set one up for you and it isn't here, add it with its schedule code below." }
            }

            Card {
                visible: root.tab === "claimable" && root.claimableSchedules.length > 0
                Heading { text: "Ready to claim" }
                Repeater {
                    model: root.tab === "claimable" ? root.claimableSchedules : []
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: Logic.scheduleTitle(modelData) + " from " + Logic.fromName(modelData, root.names)
                        detail: Logic.rowState(modelData)
                        chosen: !!root.selected && String(root.selected.id) === String(modelData.id)
                        onPressed: { root.recipientId = modelData.id; root.confirm = null }
                    }
                }
            }

            Card {
                visible: root.tab === "unvested" && root.unvestedSchedules.length > 0
                Heading { text: "Still unvested" }
                Body { text: "These pay you later: nothing to claim yet." }
                Repeater {
                    model: root.tab === "unvested" ? root.unvestedSchedules : []
                    delegate: Choice {
                        required property var modelData
                        readonly property var row: Logic.unvestedRow(modelData, root.names, root.viewTime())
                        Layout.fillWidth: true
                        label: row.label
                        detail: row.detail
                        chosen: !!root.selected && String(root.selected.id) === String(modelData.id)
                        onPressed: { root.recipientId = modelData.id; root.confirm = null }
                    }
                }
            }

            Card {
                visible: root.tab === "unvested" && root.closedSchedules.length > 0
                Heading { text: "Finished" }
                Body { text: "Fully claimed, or cancelled with nothing left to claim." }
                Repeater {
                    model: root.tab === "unvested" ? root.closedSchedules : []
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: Logic.scheduleTitle(modelData) + " from " + Logic.fromName(modelData, root.names)
                        detail: Logic.rowState(modelData)
                        chosen: !!root.selected && String(root.selected.id) === String(modelData.id)
                        onPressed: { root.recipientId = modelData.id; root.confirm = null }
                    }
                }
            }

            Card {
                visible: root.receiving && root.selected !== null
                Heading { text: root.selected ? "Schedule " + root.selected.id : "" }
                Body { visible: !!root.selected && Logic.isClosed(root.selected); text: root.selected ? Logic.statusLine(root.selected) : ""; color: look.ink }
                Body {
                    visible: !!root.selected && Logic.receiveCategory(root.selected) === "unvested"
                    text: root.selected ? Logic.untilLine(root.selected, root.viewTime()) : ""
                    color: look.ink
                    font.weight: Font.DemiBold
                }
                FactRow { look: root.look; key: "Total"; visible: !!root.selected && Logic.receiveCategory(root.selected) === "unvested"; value: root.selected ? root.selected.total_display : "" }
                FactRow {
                    look: root.look; key: "Claimable now"
                    visible: !!root.selected && Logic.receiveCategory(root.selected) === "claimable"
                    value: root.selected ? root.selected.claimable_display + (root.selected.cancelled ? " (unlocked before the cancel)" : "") : ""
                }
                FactRow { look: root.look; key: "Claimed"; value: root.selected ? root.selected.claimed_display + (root.selected.cancelled ? "" : " of " + root.selected.total_display) : "" }
                FactRow { look: root.look; key: "Still locked"; visible: !!root.selected && !Logic.isClosed(root.selected); value: root.selected ? root.selected.locked_display : "" }
                FactRow { look: root.look; key: "Next"; visible: !!root.selected && !Logic.isClosed(root.selected); value: root.selected ? Logic.nextUnlock(root.selected, root.viewTime()).text : "" }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 12
                    FactRow { look: root.look; Layout.fillWidth: true; key: "From"; value: root.selected ? (root.fromFull ? Logic.base58Of(root.selected.creator) : (Logic.nameOf(root.selected.creator, root.names) || Logic.short(Logic.base58Of(root.selected.creator)))) : ""; monospace: root.fromFull }
                    Link { look: root.look; visible: !!root.selected; text: root.fromFull ? "Hide" : "Show"; onPressed: root.fromFull = !root.fromFull }
                    Link { look: root.look; visible: !!root.selected; text: "Copy"; onPressed: root.copy(Logic.base58Of(root.selected.creator), "Address") }
                    Link { look: root.look; visible: !!root.selected && root.connected; text: "Open in Names"; onPressed: wallet.show(Logic.nameOf(root.selected.creator, root.names) || Logic.base58Of(root.selected.creator), function(r) { if (!r || !r.ok) root.say("Names Wallet couldn't open it" + (r && r.error ? ": " + r.error : "") + ".", true) }) }
                }
                RowLayout {
                    visible: !!(root.selected && root.selected.address)
                    spacing: 16
                    Text { textFormat: Text.PlainText; text: "Schedule code"; color: look.dim; font.family: look.sans; font.pixelSize: look.px(14); Layout.preferredWidth: look.px(132) }
                    Text { textFormat: Text.PlainText; text: root.selected && root.selected.address ? Logic.short(root.selected.address) : ""; color: look.ink; font.family: look.mono; font.pixelSize: look.px(13) }
                    Link { look: root.look; text: "Copy schedule code"; onPressed: root.copy(root.selected.address, "Schedule code") }
                }

                // Claim: where to, then the confirmation.
                ColumnLayout {
                    visible: !!root.selected && Logic.receiveCategory(root.selected) === "claimable" && root.confirm === null
                    Layout.fillWidth: true
                    Layout.topMargin: 8
                    spacing: 10
                    Body { text: "A public claim pays your account in the open: anyone can see the amount and where it went." }
                    Body { text: "Private claims need Names Wallet to sign a public claim into a private account, which it can't yet: claims go to your public account for now." }
                    Action {
                        look: root.look; strong: true; changes: true; busy: root.busy
                        label: "Check claim"; busyLabel: "Checking…"; working: root.workingOn === "summary"
                        onPressed: root.beginClaim()
                    }
                }

                Sheet {
                    visible: root.confirm !== null
                    Heading { text: root.confirm ? "Claim " + root.confirm.amountDisplay + "?" : "" }
                    FactRow { look: root.look; key: "Receives"; value: root.confirm ? Logic.labelled(root.confirm.destAccount, root.names) + " (public)" : "" }
                    FactRow { look: root.look; key: "Network fee"; value: "shown by Names Wallet before you approve" }
                    Body { text: root.confirm ? root.confirm.fee.message : ""; color: root.confirm && !root.confirm.fee.ok ? look.alarm : look.dim }
                    Body { visible: root.claimRequest !== null; text: Logic.requestLine(root.claimRequest, "the claim"); color: look.ink }
                    RowLayout {
                        spacing: 8
                        Action {
                            look: root.look; strong: true; changes: true; busy: root.busy
                            enabledNow: !!(root.confirm && root.confirm.fee.ok)
                            label: "Claim in Names Wallet"
                            busyLabel: "Waiting for Names…"; working: root.workingOn === "claim"
                            onPressed: root.confirmClaim()
                        }
                        Action { look: root.look; label: "Don't claim"; visible: root.workingOn !== "claim"; onPressed: root.confirm = null }
                        Link { look: root.look; visible: root.claimRequest !== null && root.claimRequest.state === "pending"; text: "Withdraw the request"; onPressed: root.withdrawClaim() }
                    }
                }
            }

            Card {
                visible: root.receiving && root.canWrite && root.selected !== null && root.selected.transferable && !Logic.isClosed(root.selected)
                Heading { text: "Hand this schedule to someone else" }
                Body { text: "This schedule was made transferable: what is still locked can go to another account." }
                Field { id: transferField; look: root.look; Layout.fillWidth: true; hint: "Who receives it: address or LNS name" }
                Action {
                    look: root.look; changes: true; busy: root.busy
                    label: "Transfer schedule"; busyLabel: "Transferring…"; working: root.workingOn === "transfer"
                    onPressed: root.doTransfer(root.selected.id, transferField.text)
                }
            }

            Card {
                visible: root.tab === "unvested" && root.connected
                Heading { text: "Add a schedule by code" }
                Body { text: "Schedules paying your accounts show up by themselves. One missing? Paste the schedule code the person paying you has (Vest shows it to them after they create it)." }
                ImportRow {}
            }

            TechDetails {
                visible: root.receiving && root.selected !== null
                look: root.look
                summary: root.selected && root.selected.address ? "Schedule " + Logic.short(root.selected.address) : ""
                FactRow { look: root.look; key: "Schedule code"; value: root.selected && root.selected.address ? root.selected.address : ""; monospace: true }
                FactRow { look: root.look; key: "Receives"; value: root.selected ? root.selected.beneficiary : ""; monospace: true }
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
            }

            // The receipt of the last create: what was made, between whom, and the proof.
            Sheet {
                id: receiptCard
                visible: root.tab === "pay" && root.receipt !== null
                readonly property var rc: root.receipt ? Logic.receiptOf(root.receipt) : null
                Heading { text: receiptCard.rc ? receiptCard.rc.title : "" }
                Repeater {
                    model: receiptCard.rc ? receiptCard.rc.rows : []
                    delegate: RowLayout {
                        required property var modelData
                        required property int index
                        Layout.fillWidth: true
                        spacing: 12
                        Text { textFormat: Text.PlainText; text: modelData.key; color: look.dim; font.family: look.sans; font.pixelSize: look.px(14); Layout.preferredWidth: look.px(120) }
                        Text {
                            textFormat: Text.PlainText
                            Layout.fillWidth: true
                            text: root.shownAddr[index] && modelData.addr ? modelData.addr : modelData.value
                            color: look.ink
                            font.family: root.shownAddr[index] ? look.mono : look.sans
                            font.pixelSize: look.px(root.shownAddr[index] ? 13 : 15)
                            wrapMode: root.shownAddr[index] ? Text.WrapAnywhere : Text.WordWrap
                        }
                        Link { look: root.look; visible: !!modelData.addr; text: root.shownAddr[index] ? "Hide" : "Show"; onPressed: { var m = Object.assign({}, root.shownAddr); m[index] = !m[index]; root.shownAddr = m } }
                        Link { look: root.look; visible: !!modelData.addr; text: "Copy"; onPressed: root.copy(modelData.addr, modelData.key) }
                    }
                }
                Body { color: look.ink; text: receiptCard.rc ? receiptCard.rc.share : "" }
                RowLayout {
                    spacing: 20
                    Action { look: root.look; label: "Done"; onPressed: root.receipt = null }
                    Link { look: root.look; visible: !!(root.receipt && root.receipt.code); text: "Copy schedule code"; onPressed: root.copy(root.receipt.code, "Schedule code") }
                }
            }

            Card {
                visible: root.tab === "pay" && root.connected
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
                        detail: Logic.balanceText(modelData, root.balanceFailed)
                        chosen: root.cCreator === modelData.id
                        onPressed: root.cCreator = modelData.id
                    }
                }
                Label { text: "How it unlocks" }
                RowLayout {
                    spacing: 8
                    ChoiceInline { label: "Evenly"; chosen: root.cKind === "linear"; onPressed: root.cKind = "linear" }
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
                    DateTimeField { id: startField; look: root.look; Layout.preferredWidth: 1; Layout.alignment: Qt.AlignTop; Layout.fillWidth: true; hint: "Pick a date" }
                    DateTimeField { id: cliffField; look: root.look; Layout.preferredWidth: 1; Layout.alignment: Qt.AlignTop; visible: root.cKind === "cliff-linear"; Layout.fillWidth: true; hint: "Pick a date" }
                    DateTimeField { id: endField; look: root.look; Layout.preferredWidth: 1; Layout.alignment: Qt.AlignTop; Layout.fillWidth: true; hint: "Pick a date" }
                }
                ColumnLayout {
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
                    Field { id: amountField; look: root.look; visible: root.cKind !== "milestone"; Layout.fillWidth: true; hint: "LEZ, e.g. 1000" }
                    Repeater {
                        model: root.cKind === "milestone" ? root.trancheCount : 0
                        delegate: RowLayout {
                            required property int index
                            Layout.fillWidth: true
                            spacing: 10
                            Field {
                                look: root.look
                                Layout.fillWidth: true
                                hint: "Milestone " + (index + 1) + ": LEZ"
                                text: root.cTranches[index]
                                onTextChanged: root.cTranches[index] = text
                            }
                            Link { look: root.look; visible: root.trancheCount > 1; text: "Remove milestone"; onPressed: { root.cTranches.splice(index, 1); root.trancheCount = root.cTranches.length } }
                        }
                    }
                    Link { look: root.look; visible: root.cKind === "milestone"; text: "Add a milestone"; onPressed: { root.cTranches.push(""); root.trancheCount = root.cTranches.length } }
                }
                Body { text: "One person per schedule, in LEZ. Several people in one transaction, and schedules of other tokens, aren't offered: Names Wallet can't approve them yet." }

                Tick { look: root.look; checked: root.cCancelable; text: "You can cancel it later"; onToggled: root.cCancelable = !root.cCancelable }
                Body { text: "Cancelable: until you make it permanent, you can stop it. What hasn't unlocked comes back to you; what has stays theirs." }
                Tick { look: root.look; checked: root.cTransferable; text: "They can hand it to someone else"; onToggled: root.cTransferable = !root.cTransferable }
                Body { text: "Transferable: the person receiving can pass the rest of the schedule to another account. This can't be changed later." }

                TechDetails {
                    look: root.look
                    title: "More options"
                    summary: "Someone else who may cancel or unlock milestones"
                    Body { text: "By default you decide: you can cancel, and you say when a milestone is reached. Name someone else here to give them that job instead, for example a board or a lawyer." }
                    Label { text: "Who may cancel (empty: you)" }
                    Field { id: cancelAuthField; look: root.look; Layout.fillWidth: true; hint: "Address or LNS name" }
                    Label { text: "Who says a milestone is reached (empty: you)" }
                    Field { id: milestoneAuthField; look: root.look; Layout.fillWidth: true; hint: "Address or LNS name" }
                }

                Body {
                    color: look.ink
                    text: !root.namesCanCall
                        ? "Create needs Names Wallet 0.18 or later, which can sign a new schedule. Update Names Wallet, then come back."
                        : "Names Wallet opens with the schedule: choose the account that pays and approve. You come back here after."
                }
                Action {
                    look: root.look; strong: true; changes: true; busy: root.busy
                    enabledNow: root.namesCanCall
                    label: "Create schedule"
                    busyLabel: "Creating…"; working: root.workingOn === "create"
                    onPressed: root.doCreate()
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
                            text: "Receives " + Logic.labelled(managed.modelData.beneficiary, root.names) + " · claimed " + managed.modelData.claimed_display
                                + (managed.modelData.cancelled ? " · cancelled" : Logic.isFullyClaimed(managed.modelData) ? " · fully claimed" : managed.modelData.cancelable ? "" : " · permanent")
                        }
                        Link { look: root.look; visible: !!managed.modelData.address; text: "Copy schedule code"; onPressed: root.copy(managed.modelData.address, "Schedule code") }

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
                                visible: managed.modelData.cancelable && (managed.modelData.mine.cancel_authority || managed.modelData.mine.creator)
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
                Body { text: "Vest signs through Names Wallet: you approve every payment and claim in Names Wallet. Vest has no signer of its own." }
                FactRow { look: root.look; visible: wallet.current !== null; key: "Wallet"; value: wallet.current ? Logic.clean(wallet.current.display_name) + " (" + Logic.clean(wallet.current.module) + ")" : "" }
                Link { look: root.look; visible: root.namesState === "connected"; text: "Disconnect Names Wallet"; onPressed: wallet.disconnect(function() { root.onDisconnected() }, function(r) { root.say(r.error, true) }) }
                Link { look: root.look; visible: wallet.wallets.length > 1; text: "Use another wallet"; onPressed: { wallet.forgetWallet(); root.refresh() } }
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
                Action { look: root.look; label: "Save network settings"; changes: true; busy: root.busy; onPressed: root.saveNetwork() }
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
        Field { id: importInput; look: root.look; Layout.fillWidth: true; hint: "Schedule code (the person paying you has it)"; onTextChanged: importField.text = text }
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

    // One entry of the menu: a label and, for Claimable and Unvested, a count. `inline`: in the
    // narrow window's row at the top.
    component MenuItem: Rectangle {
        id: item
        property string label: ""
        property int count: -1
        property bool active: false
        property bool inline: false
        signal pressed()
        implicitWidth: itemRow.implicitWidth + 24
        implicitHeight: inline ? 36 : 40
        radius: 6
        color: item.active ? root.look.raised : (itemArea.containsMouse ? root.look.raised : "transparent")
        border.width: item.inline || item.activeFocus ? 1 : 0
        border.color: item.active || item.activeFocus ? root.look.brass : root.look.line
        activeFocusOnTab: true
        Accessible.role: Accessible.Button
        Accessible.name: item.label + (item.count > 0 ? ", " + item.count : "")
        Keys.onSpacePressed: item.pressed()
        Keys.onReturnPressed: item.pressed()
        Rectangle { visible: item.active && !item.inline; anchors { left: parent.left; top: parent.top; bottom: parent.bottom; topMargin: 8; bottomMargin: 8 } width: 3; radius: 1; color: root.look.brass }
        RowLayout {
            id: itemRow
            anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
            spacing: 8
            Text { textFormat: Text.PlainText; Layout.fillWidth: !item.inline; text: item.label; color: item.active ? root.look.ink : root.look.dim; font.family: root.look.sans; font.pixelSize: root.look.px(16); font.weight: Font.DemiBold; elide: Text.ElideRight }
            Rectangle {
                visible: item.count > 0
                implicitWidth: Math.max(implicitHeight, badge.implicitWidth + 12)
                implicitHeight: root.look.px(20)
                radius: height / 2
                color: root.look.brass
                Text { id: badge; anchors.centerIn: parent; textFormat: Text.PlainText; text: String(item.count); color: root.look.slate; font.family: root.look.sans; font.pixelSize: root.look.px(13); font.weight: Font.DemiBold }
            }
        }
        MouseArea { id: itemArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: item.pressed() }
    }
}
