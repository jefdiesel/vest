// Vest: LEZ token vesting in Basecamp (Logos RFP-017).
//
// The view of the `vesting` core module (../module): every action is one module call through
// `logos.callModuleAsync("vesting", ...)`, which is one SDK call on the LEZ chain through the
// LEZ wallet set in Settings. Schedules, wallet accounts, balances, the simulated claim fee and
// LNS names all come from the module. All display logic is in logic.mjs, which the Node test runs.
//
// The look is the shared kit (./kit, as the Names Wallet and the Program Registry browser use
// it): one left-aligned column at most 680 px wide, 56 px from the window's edge.

import QtQuick
import QtQuick.Layouts
import Logos.Theme
import "logic.mjs" as Logic
import "kit"

Item {
    id: root
    property Look look: Look {}

    // ── Module data ──────────────────────────────────────────────────────────────────
    property var schedules: []                 // Logic.fromModule rows of `list`
    property var names: ({})                   // the module's name book: hex account -> name
    property var accounts: []                  // the wallet's accounts (`accounts`)
    property var info: null                    // `settings`: network, connection, status
    property real effectiveTime: 0             // `now`: the chain's clock
    readonly property bool bridgeReady: typeof logos !== "undefined" && !!logos.callModule
    readonly property bool connected: info !== null && info.connected === true
    readonly property bool firstRun: Logic.firstRun(info)
    readonly property string connectProblem: Logic.connectProblem(info)

    // ── View state ──────────────────────────────────────────────────────────────────
    property string tab: "recipient"           // recipient | creator | settings
    property real tick: Date.now()             // only a fallback when now() has not answered
    property var recipientId: null
    property string destKind: "public"
    property string destAccount: ""
    property var confirm: null                 // the pre-claim confirmation
    property var sheet: null                   // a step that can't be undone: {kind, schedule}
    property string resultLine: ""
    property bool resultBad: false
    property string resultDetail: ""           // a failure's raw detail, for Technical details
    property bool loading: false
    property bool busy: false                  // a write is running: others rest
    property string workingOn: ""              // which one
    property string lastDetail: ""             // the detail of the last failed call

    // ── Forms ───────────────────────────────────────────────────────────────────────
    property string cCreator: ""
    property string cKind: "linear"
    property bool batchMode: false
    property bool cCancelable: true
    property bool cTransferable: false
    property var cTranches: [""]
    property var bRows: [ { beneficiary: "", amount: "", tranches: "" } ]

    // ── Derived ─────────────────────────────────────────────────────────────────────
    readonly property var recipientSchedules: schedules.filter(function(s) { return s.mine.beneficiary })
    readonly property var mySchedules: schedules.filter(function(s) {
        return s.mine.creator || s.mine.cancel_authority || s.mine.milestone_authority })
    readonly property var selected: findById(recipientSchedules, recipientId)
    readonly property var publicAccounts: accounts.filter(function(a) { return !a["private"] })
    readonly property var privateAccounts: accounts.filter(function(a) { return a["private"] })
    readonly property string networkName: info && info.network && info.network.name ? info.network.name : ""

    function viewTime() { return root.effectiveTime > 0 ? root.effectiveTime : root.tick }

    function findById(list, id) {
        for (var i = 0; i < list.length; i++) if (String(list[i].id) === String(id)) return list[i]
        return null
    }

    // ── Module calls ─────────────────────────────────────────────────────────────────

    /// One call of the vesting module; every argument is a string (Basecamp's call checker
    /// refuses a QML number for an integer parameter, so ids and indexes go as text). Reads
    /// answer in seconds; writes wait for the chain and a private claim proves on this machine,
    /// so they get a long `patience` (ms).
    function call(method, args, done, patience) {
        if (!bridgeReady) {
            done({ ok: false, error: "This app runs inside Basecamp, with the Vesting module installed." })
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

    /// Settings first; with a wallet saved but not connected yet (a fresh start, a reinstall),
    /// the first read connects, and the settings are read again for the outcome.
    function refresh() {
        loading = true
        call("settings", [], function(r) {
            if (!r.ok) { loading = false; if (r.missing) say(r.error, true); return }
            info = r
            fillSettings(r)
            if (!r.connected) {
                if (Logic.firstRun(r)) { loading = false; return }
                call("list", [], function() {
                    call("settings", [], function(again) {
                        if (again.ok) info = again
                        if (again.ok && again.connected) readChain()
                        else loading = false
                    })
                }, 120000)
                return
            }
            readChain()
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

    /// The result line under the page: plain words, red only for a problem.
    function say(message, bad) {
        var t = String(message || "").trim()
        resultLine = t === "" ? "" : t.charAt(0).toUpperCase() + t.substr(1) + (/[.?!]$/.test(t) ? "" : ".")
        resultBad = !!bad
        resultDetail = bad ? lastDetail : ""
        lastDetail = ""
    }

    /// A write: one at a time, the button shows its busy label, then the list is read again so
    /// finished schedules lose their buttons.
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

    Component.onCompleted: refresh()
    Timer { interval: 1000; repeat: true; running: true; onTriggered: root.tick = Date.now() }
    TextEdit { id: clipboard; visible: false }

    // ── Settings ────────────────────────────────────────────────────────────────────

    function saveSettings() {
        say("")
        busy = true
        workingOn = "settings"
        var patch = {
            wallet_dir: walletField.text.trim(), network_file: networkFileField.text.trim(),
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
            if (r.ok) {
                say("Connected to " + (r.network.name || r.network.rpc) + ".")
                tab = "recipient"
            }
            refresh()
        }, 60000)
    }

    function doImport() {
        var a = Logic.accountInput(importField.text)
        if (!a.ok) { say("Enter the schedule's address or LNS name.", true); return }
        write("import", "import_schedule", [a.account], function(r) {
            if (!r.ok) { say(r.error, true); return }
            importField.text = ""
            recipientId = r.id
            say("Added schedule " + r.id + ".")
        }, 60000)
    }

    // ── Recipient actions ───────────────────────────────────────────────────────────

    function chooseKind(kind) {
        destKind = kind
        destAccount = kind === "private" && privateAccounts.length ? privateAccounts[0].id : ""
    }

    /// The confirmation is the SDK's pre_claim_summary: claimable amount and the simulated fee.
    function beginClaim() {
        var s = selected
        if (!s) return
        var isPrivate = destKind === "private"
        if (isPrivate && destAccount === "") { say("Choose one of your private accounts.", true); return }
        say("")
        busy = true
        workingOn = "summary"
        call("pre_claim_summary", [String(recipientId), isPrivate ? destAccount : ""], function(r) {
            busy = false
            workingOn = ""
            if (!r.ok) { say("Couldn't prepare the claim: " + r.error, true); return }
            var sum = r.summary || {}
            if (!Logic.above(sum.claimable || "0", "0")) { say(Logic.nothingClaimable(s, viewTime())); return }
            confirm = {
                id: String(recipientId), isPrivate: isPrivate,
                amountDisplay: sum.claimable_display, feeDisplay: sum.estimated_fee_display,
                destAccount: isPrivate ? destAccount : s.beneficiary,
                fee: Logic.feeCheck(sum), privacy: isPrivate ? Logic.privacyDisclosure() : null
            }
        }, 120000)
    }

    function confirmClaim() {
        if (!confirm || !confirm.fee.ok) return
        var c = confirm
        var method = c.isPrivate ? "claim_private" : "claim_public"
        var args = c.isPrivate ? [c.id, c.destAccount] : [c.id]
        if (c.isPrivate) say("Proving the private claim on this computer; this takes a few minutes.")
        write("claim", method, args, function(r) {
            confirm = null
            if (!r.ok) { say("The claim didn't go through: " + r.error, true); return }
            say(Logic.claimLineDisplay(r.amount_display, Logic.labelled(c.destAccount, root.names), r.tx, c.isPrivate))
        }, c.isPrivate ? 3600000 : 300000)
    }

    function doTransfer(id, target) {
        var typed = Logic.accountInput(target)
        if (!typed.ok) { say("Enter the new beneficiary's address or LNS name.", true); return }
        write("transfer", "transfer_beneficiary", [String(id), typed.account], function(r) {
            if (!r.ok) { say("The transfer didn't go through: " + r.error, true); return }
            transferField.text = ""
            say("Schedule " + id + " now belongs to "
                + (r.name ? r.name + " (" + Logic.short(r.beneficiary) + ")" : Logic.short(r.beneficiary)) + ".")
        })
    }

    // ── Creator actions ─────────────────────────────────────────────────────────────

    /// The form in base units: amounts are typed in LEZ (9 decimals), or in the token's
    /// smallest unit for another token.
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

    function doCreate() {
        if (cCreator === "") { say("Choose which of your accounts creates and pays.", true); return }
        var p = sharedParams()
        p.beneficiary = beneficiaryField.text.trim()
        p.amount = amountOf(amountField.text)
        p.tranches = cKind === "milestone" ? cTranches.map(amountOf) : []
        var v = Logic.validateCreate(p)
        if (!v.ok) { say(v.errors[0], true); return }
        write("create", "create", [JSON.stringify(Logic.toCreateRow(p, cCreator))], function(r) {
            if (!r.ok) { say("The schedule wasn't created: " + r.error, true); return }
            say(Logic.resolvedLines(r.resolved).concat([Logic.createLine(r.id)]).join(" "))
        })
    }

    function doBatch() {
        if (cCreator === "") { say("Choose which of your accounts creates and pays.", true); return }
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
            if (!r.ok) { say("The schedules weren't created: " + r.error, true); return }
            say(Logic.resolvedLines(r.resolved).concat([Logic.batchLine((r.ids || []).length)]).join(" "))
        })
    }

    /// Cancel and make irrevocable can't be undone: they open the confirm sheet first.
    function askGrave(kind, s) { sheet = { kind: kind, schedule: s } }

    function doGrave() {
        var k = sheet.kind, s = sheet.schedule
        sheet = null
        if (k === "cancel")
            write("cancel:" + s.id, "cancel", [String(s.id)], function(r) {
                if (!r.ok) { say("Schedule " + s.id + " wasn't cancelled: " + r.error, true); return }
                say("Cancelled schedule " + s.id + ". " + r.refund_display + " went back to the creator.")
            })
        else
            write("irrevocable:" + s.id, "make_irrevocable", [String(s.id)], function(r) {
                if (!r.ok) { say("Schedule " + s.id + " didn't change: " + r.error, true); return }
                say("Schedule " + s.id + " is now irrevocable.")
            })
    }

    function doSignal(s, index) {
        write("signal:" + s.id + ":" + index, "signal_milestone", [String(s.id), String(index)], function(r) {
            if (!r.ok) { say("The milestone wasn't signalled: " + r.error, true); return }
            say("Signalled milestone " + (index + 1) + " of schedule " + s.id + ": " + r.amount_display + " unlocked.")
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

            PageHeader {
                look: root.look
                title: "Vest"
                sentence: "Token vesting on LEZ: claim what has vested to you, or set up schedules for others."
                runsOnPrefix: "Runs on"
                network: root.connected ? Logic.clean(root.networkName) : ""
                networkDetail: root.connected && root.info.network.rpc ? Logic.hostOf(root.info.network.rpc) : ""
            }

            // First run: nothing is wrong yet, so nothing is red.
            Card {
                visible: root.firstRun && root.tab !== "settings"
                Heading { text: "Connect a wallet to start" }
                Body { text: "Vest uses a LEZ wallet on the bridge test network to see and claim your schedules." }
                Action { look: root.look; strong: true; label: "Open settings"; onPressed: root.tab = "settings" }
            }

            // A real failure after the person tried: plain words, and the way to fix it.
            Card {
                visible: !root.firstRun && root.connectProblem !== "" && root.tab !== "settings"
                Heading { text: "Not connected" }
                Body { text: root.connectProblem; color: look.alarm }
                Action { look: root.look; label: "Open settings"; onPressed: root.tab = "settings" }
            }

            RowLayout {
                visible: !root.firstRun
                Layout.fillWidth: true
                spacing: 8
                Tab { label: "Your schedules"; active: root.tab === "recipient"; onPressed: root.tab = "recipient" }
                Tab { label: "Create"; active: root.tab === "creator"; onPressed: root.tab = "creator" }
                Tab { label: "Settings"; active: root.tab === "settings"; onPressed: root.tab = "settings" }
                Item { Layout.fillWidth: true }
                Link { look: root.look; visible: root.connected; text: root.loading ? "Reading…" : "Refresh"; onPressed: if (!root.loading) root.refresh() }
            }

            // ── Your schedules ──────────────────────────────────────────────────────
            Card {
                visible: root.tab === "recipient" && root.connected && root.recipientSchedules.length === 0
                Heading { text: "No schedules for this wallet yet." }
                Body { text: "Got a schedule from someone? Paste its address or LNS name below." }
                ImportRow {}
            }

            Card {
                visible: root.tab === "recipient" && root.recipientSchedules.length > 0
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
                visible: root.tab === "recipient" && root.selected !== null
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

                // Claim: where to, then the confirmation with the simulated fee.
                ColumnLayout {
                    visible: !!root.selected && !Logic.isFinished(root.selected) && root.confirm === null
                    Layout.fillWidth: true
                    Layout.topMargin: 8
                    spacing: 10
                    Body { text: "A public claim pays your account in the open. A private claim pays a private account of your wallet; where the tokens go after that stays hidden." }
                    RowLayout {
                        spacing: 8
                        ChoiceInline { label: "Public account"; chosen: root.destKind === "public"; onPressed: root.chooseKind("public") }
                        ChoiceInline { label: "Private account"; chosen: root.destKind === "private"; onPressed: root.chooseKind("private") }
                    }
                    Body { visible: root.destKind === "private" && root.privateAccounts.length === 0; text: "Your wallet has no private account yet. Make one with the LEZ wallet first." }
                    Repeater {
                        model: root.destKind === "private" ? root.privateAccounts : []
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
                        label: "Check claim"; busyLabel: "Checking the fee…"; working: root.workingOn === "summary"
                        onPressed: root.beginClaim()
                    }
                }

                // The pre-claim confirmation.
                Sheet {
                    visible: root.confirm !== null
                    Heading { text: root.confirm ? "Claim " + root.confirm.amountDisplay + "?" : "" }
                    FactRow { look: root.look; key: "To"; value: root.confirm ? Logic.labelled(root.confirm.destAccount, root.names) + (root.confirm.isPrivate ? " (private)" : " (public)") : "" }
                    FactRow { look: root.look; key: "Network fee"; value: root.confirm ? root.confirm.feeDisplay : "" }
                    Body { visible: !!(root.confirm && root.confirm.privacy); text: root.confirm && root.confirm.privacy ? root.confirm.privacy.text : "" }
                    Body { text: root.confirm ? root.confirm.fee.message : ""; color: root.confirm && !root.confirm.fee.ok ? look.alarm : look.dim }
                    RowLayout {
                        spacing: 8
                        Action {
                            look: root.look; strong: true; changes: true; busy: root.busy
                            enabledNow: !!(root.confirm && root.confirm.fee.ok)
                            label: "Claim " + (root.confirm ? root.confirm.amountDisplay : ""); busyLabel: "Claiming…"; working: root.workingOn === "claim"
                            onPressed: root.confirmClaim()
                        }
                        Action { look: root.look; label: "Don't claim"; visible: root.workingOn !== "claim"; onPressed: root.confirm = null }
                    }
                }
            }

            Card {
                visible: root.tab === "recipient" && root.selected !== null && root.selected.transferable && !Logic.isClosed(root.selected)
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
                visible: root.tab === "recipient" && root.recipientSchedules.length > 0
                Heading { text: "Add a schedule" }
                Body { text: "Got a schedule from someone? Paste its address or LNS name." }
                ImportRow {}
            }

            TechDetails {
                visible: root.tab === "recipient" && root.selected !== null
                look: root.look
                summary: root.selected && root.selected.address ? "Schedule " + Logic.short(root.selected.address) : ""
                FactRow { look: root.look; key: "Schedule address"; value: root.selected && root.selected.address ? root.selected.address : ""; monospace: true }
                FactRow { look: root.look; key: "Beneficiary"; value: root.selected ? root.selected.beneficiary : ""; monospace: true }
                FactRow { look: root.look; key: "Creator"; value: root.selected ? root.selected.creator : ""; monospace: true }
                FactRow { look: root.look; key: "Token"; value: root.selected ? root.selected.token : ""; monospace: true }
                FactRow { look: root.look; key: "Amounts (base units)"; value: root.selected ? "total " + root.selected.total + " · claimed " + root.selected.claimed + " · claimable " + root.selected.claimable : ""; monospace: true }
                FactRow { look: root.look; key: "Kind"; value: root.selected ? root.selected.kind + (root.selected.cancelable ? ", cancelable" : ", irrevocable") + (root.selected.transferable ? ", transferable" : "") : "" }
                FactRow { look: root.look; key: "Chain time"; value: Logic.dateText(root.viewTime()) + " (" + root.viewTime() + " ms)" }
            }

            // ── Create ──────────────────────────────────────────────────────────────
            // A step that can't be undone (cancel, make irrevocable): confirm first.
            Sheet {
                visible: root.tab === "creator" && root.sheet !== null
                Heading { text: root.sheet ? Logic.confirmText(root.sheet.kind, root.sheet.schedule) : "" }
                RowLayout {
                    spacing: 8
                    Action {
                        look: root.look; strong: true; grave: true; changes: true; busy: root.busy
                        label: root.sheet && root.sheet.kind === "cancel" ? "Cancel schedule" : "Make irrevocable"
                        onPressed: root.doGrave()
                    }
                    Action { look: root.look; label: "Keep it as it is"; onPressed: root.sheet = null }
                }
            }

            Card {
                visible: root.tab === "creator" && root.connected
                Heading { text: "New schedule" }
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
                Label { text: "How it vests" }
                RowLayout {
                    spacing: 8
                    ChoiceInline { label: "Evenly over time"; chosen: root.cKind === "linear"; onPressed: root.cKind = "linear" }
                    ChoiceInline { label: "Cliff, then evenly"; chosen: root.cKind === "cliff-linear"; onPressed: root.cKind = "cliff-linear" }
                    ChoiceInline { label: "Milestones"; chosen: root.cKind === "milestone"; onPressed: root.cKind = "milestone" }
                }
                GridLayout {
                    visible: root.cKind !== "milestone"
                    Layout.fillWidth: true
                    columns: root.cKind === "cliff-linear" ? 3 : 2
                    columnSpacing: 10
                    rowSpacing: 4
                    Label { text: "Starts" }
                    Label { visible: root.cKind === "cliff-linear"; text: "Cliff" }
                    Label { text: "Fully vested" }
                    Field { id: startField; look: root.look; Layout.fillWidth: true; hint: "2026-12-01" }
                    Field { id: cliffField; look: root.look; visible: root.cKind === "cliff-linear"; Layout.fillWidth: true; hint: "2027-03-01" }
                    Field { id: endField; look: root.look; Layout.fillWidth: true; hint: "2027-12-01" }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    ChoiceInline { label: "One person"; chosen: !root.batchMode; onPressed: root.batchMode = false }
                    ChoiceInline { label: "Several people, one transaction"; chosen: root.batchMode; onPressed: root.batchMode = true }
                }

                // One person.
                ColumnLayout {
                    visible: !root.batchMode
                    Layout.fillWidth: true
                    spacing: 6
                    Label { text: "Beneficiary" }
                    Field { id: beneficiaryField; look: root.look; Layout.fillWidth: true; hint: "Address or LNS name" }
                    Label { visible: root.cKind !== "milestone"; text: "Amount" }
                    Field { id: amountField; look: root.look; visible: root.cKind !== "milestone"; Layout.fillWidth: true; hint: Logic.isNative(tokenField.text) ? "LEZ, e.g. 1.5" : "Amount in the token's smallest unit" }
                    Repeater {
                        model: root.cKind === "milestone" ? root.cTranches : []
                        delegate: RowLayout {
                            required property int index
                            Layout.fillWidth: true
                            spacing: 10
                            Field {
                                look: root.look
                                Layout.fillWidth: true
                                hint: "Milestone " + (index + 1) + (Logic.isNative(tokenField.text) ? ", LEZ" : "")
                                text: root.cTranches[index]
                                onTextChanged: if (text !== root.cTranches[index]) { var next = root.cTranches.slice(); next[index] = text; root.cTranches = next }
                            }
                            Link { look: root.look; visible: root.cTranches.length > 1; text: "Remove"; onPressed: { var next = root.cTranches.slice(); next.splice(index, 1); root.cTranches = next } }
                        }
                    }
                    Link { look: root.look; visible: root.cKind === "milestone"; text: "Add a milestone"; onPressed: { var next = root.cTranches.slice(); next.push(""); root.cTranches = next } }
                }

                // Several people.
                ColumnLayout {
                    visible: root.batchMode
                    Layout.fillWidth: true
                    spacing: 8
                    Body { text: root.cKind === "milestone" ? "One row per person: their address or LNS name and their milestone amounts, separated by commas." : "One row per person: their address or LNS name and their amount. Up to 140 people in one transaction." }
                    Repeater {
                        model: root.bRows
                        delegate: RowLayout {
                            required property var modelData
                            required property int index
                            Layout.fillWidth: true
                            spacing: 8
                            Field {
                                look: root.look
                                Layout.fillWidth: true
                                Layout.preferredWidth: 3
                                hint: "Person " + (index + 1) + ": address or LNS name"
                                text: modelData.beneficiary
                                onTextChanged: if (text !== root.bRows[index].beneficiary) { var next = root.bRows.slice(); next[index] = Object.assign({}, next[index], { beneficiary: text }); root.bRows = next }
                            }
                            Field {
                                look: root.look
                                Layout.fillWidth: true
                                Layout.preferredWidth: 2
                                hint: root.cKind === "milestone" ? "e.g. 0.4, 0.6" : "LEZ"
                                text: root.cKind === "milestone" ? modelData.tranches : modelData.amount
                                onTextChanged: {
                                    var key = root.cKind === "milestone" ? "tranches" : "amount"
                                    if (text === root.bRows[index][key]) return
                                    var next = root.bRows.slice(), patch = {}
                                    patch[key] = text
                                    next[index] = Object.assign({}, next[index], patch)
                                    root.bRows = next
                                }
                            }
                            Link { look: root.look; visible: root.bRows.length > 1; text: "Remove"; onPressed: { var next = root.bRows.slice(); next.splice(index, 1); root.bRows = next } }
                        }
                    }
                    Link { look: root.look; text: "Add a person"; onPressed: { var next = root.bRows.slice(); next.push({ beneficiary: "", amount: "", tranches: "" }); root.bRows = next } }
                }

                Tick { look: root.look; checked: root.cCancelable; text: "You can cancel it later (what hasn't vested comes back to you)"; onToggled: root.cCancelable = !root.cCancelable }
                Tick { look: root.look; checked: root.cTransferable; text: "The beneficiary can hand it to someone else"; onToggled: root.cTransferable = !root.cTransferable }

                TechDetails {
                    look: root.look
                    title: "More options"
                    summary: "Another token, separate cancel or milestone authority"
                    Label { text: "Token" }
                    Field { id: tokenField; look: root.look; Layout.fillWidth: true; text: "LEZ"; hint: "LEZ, or a token definition's address" }
                    Label { text: "Who can cancel (empty: you)" }
                    Field { id: cancelAuthField; look: root.look; Layout.fillWidth: true; hint: "Address or LNS name" }
                    Label { text: "Who signals milestones (empty: you)" }
                    Field { id: milestoneAuthField; look: root.look; Layout.fillWidth: true; hint: "Address or LNS name" }
                }

                Action {
                    look: root.look; strong: true; changes: true; busy: root.busy
                    label: root.batchMode ? "Create " + root.bRows.length + " schedules" : "Create schedule"
                    busyLabel: "Creating…"; working: root.workingOn === "create" || root.workingOn === "batch"
                    onPressed: root.batchMode ? root.doBatch() : root.doCreate()
                }
            }

            Card {
                visible: root.tab === "creator" && root.connected && root.mySchedules.length > 0
                Heading { text: "Schedules you manage" }
                Repeater {
                    model: root.mySchedules
                    delegate: ColumnLayout {
                        id: managed
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: 6
                        Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: look.line }
                        Text {
                            textFormat: Text.PlainText
                            Layout.fillWidth: true
                            text: Logic.scheduleTitle(managed.modelData)
                            color: look.ink; font.family: look.sans; font.pixelSize: look.px(16); font.weight: Font.DemiBold
                        }
                        Body {
                            text: "To " + Logic.labelled(managed.modelData.beneficiary, root.names) + " · claimed " + managed.modelData.claimed_display
                                + (managed.modelData.cancelled ? " · cancelled" : Logic.isFullyClaimed(managed.modelData) ? " · fully claimed" : managed.modelData.cancelable ? "" : " · irrevocable")
                        }
                        Flow {
                            visible: !Logic.isClosed(managed.modelData)
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
                                label: "Make irrevocable"; busyLabel: "Updating…"; working: root.workingOn === "irrevocable:" + managed.modelData.id
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
                Heading { text: "Wallet" }
                Body { text: "Vest signs with a LEZ wallet on this computer: the folder that holds its wallet_config.json and storage.json." }
                Label { text: "Wallet folder" }
                Field { id: walletField; look: root.look; Layout.fillWidth: true; code: true; hint: "e.g. ~/lez-wallet" }
                Label { text: "Wallet password (only if it has one; kept until Basecamp closes)" }
                Field { id: passwordField; look: root.look; Layout.fillWidth: true; Component.onCompleted: children[0].echoMode = TextInput.Password }
                Body {
                    visible: !root.firstRun && root.connectProblem !== ""
                    text: root.connectProblem
                    color: look.alarm
                }
                Body { visible: root.connected; text: "Connected."; color: look.ink }
                Action {
                    look: root.look; strong: true; changes: true; busy: root.busy
                    label: "Save and connect"; busyLabel: "Connecting…"; working: root.workingOn === "settings"
                    onPressed: root.saveSettings()
                }
            }

            TechDetails {
                visible: root.tab === "settings"
                look: root.look
                title: "Network"
                summary: root.info && root.info.network && root.info.network.name ? Logic.clean(root.info.network.name) + " (" + Logic.clean(root.info.network.source || "") + ")" : ""
                FactRow { look: root.look; key: "RPC"; value: root.info && root.info.network ? root.info.network.rpc || "" : ""; monospace: true }
                FactRow { look: root.look; key: "Chain"; value: root.info && root.info.network ? root.info.network.chain_id || "" : ""; monospace: true }
                FactRow { look: root.look; key: "Vest program"; value: root.info && root.info.network ? root.info.network.program || "" : ""; monospace: true }
                FactRow { look: root.look; key: "Token program"; value: root.info && root.info.network ? root.info.network.token_program || "" : ""; monospace: true }
                FactRow { look: root.look; key: "LNS registry"; value: root.info && root.info.network ? root.info.network.lns_registry || "" : ""; monospace: true }
                FactRow { look: root.look; key: "Data folder"; value: root.info ? root.info.data_dir || "" : ""; monospace: true }
                Body { text: "To use another network, give its network file, or change single values below. Another RPC needs its chain id unless it runs on this computer." }
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
            }

            // A failure's raw detail (status, a cleaned excerpt), folded under the result line.
            TechDetails {
                visible: root.resultBad && root.resultDetail !== ""
                look: root.look
                summary: ""
                FactRow { look: root.look; key: "Detail"; value: root.resultDetail; monospace: true }
            }

            // The result of the last step, on the same left edge.
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

    // A confirmation: a card with the accent border.
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
    // The text of whichever import row was used last (there are two: empty list and below it).
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
