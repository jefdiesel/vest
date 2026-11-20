// Vesting: view and manage LEZ vesting schedules (Logos RFP-017).
//
// The view of the `vesting` core module (../module): every action is one module call through
// `logos.callModuleAsync("vesting", ...)`, which is one SDK call on the LEZ chain through the
// LEZ wallet set in Settings. Schedules, wallet accounts, balances, the simulated claim fee and
// LNS names all come from the module; nothing here is sample data. All display logic is in
// logic.mjs, which the Node test runs.
//
// Controls are drawn here rather than taken from Qt Quick Controls, as in the Names app.

import QtQuick
import QtQuick.Layouts
import Logos.Theme
import "logic.mjs" as Logic

Item {
    id: root

    // ── Module data ──────────────────────────────────────────────────────────────────
    property var schedules: []                 // Logic.fromModule rows of `list`
    property var names: ({})                   // the module's name book: hex account -> name
    property var accounts: []                  // the wallet's accounts (`accounts`)
    property var info: null                    // `settings`: network, connection, status
    property real effectiveTime: 0             // `now`: the chain's clock
    readonly property bool bridgeReady: typeof logos !== "undefined" && !!logos.callModule
    readonly property bool connected: info !== null && info.connected === true

    // ── View state ──────────────────────────────────────────────────────────────────
    property string tab: "recipient"           // recipient | creator | settings
    property real tick: Date.now()             // only a fallback when now() has not answered
    property var recipientId: null
    property string destKind: "public"
    property string destAccount: ""
    property bool confirming: false
    property var confirm: null
    property string resultLine: ""
    property string problem: ""
    property bool loading: false

    property bool preparing: false             // pre_claim_summary in flight
    property bool claiming: false
    property bool creating: false
    property bool batchCreating: false
    property bool importing: false
    property bool saving: false
    property string busyCancel: ""
    property string busySignal: ""
    property string busyAction: ""             // irrevocable / transfer

    // ── Single create form ──────────────────────────────────────────────────────────
    property string cCreator: ""
    property string cKind: "cliff-linear"
    property string cBeneficiary: ""
    property string cToken: "LEZ"
    property string cAmount: ""
    property string cStart: ""
    property string cCliff: ""
    property string cEnd: ""
    property bool cCancelable: true
    property bool cTransferable: false
    property string cCancelAuthority: ""
    property string cMilestoneAuthority: ""
    property var cTranches: [""]

    // ── Batch create form ───────────────────────────────────────────────────────────
    property bool batchMode: false
    property var bRows: [ { beneficiary: "", amount: "", tranches: "" } ]

    // ── Settings form (filled from `settings`) ──────────────────────────────────────
    property string sWallet: ""
    property string sPassword: ""
    property string sNetworkFile: ""
    property string sRpc: ""
    property string sChainId: ""
    property string sProgram: ""
    property string sTokenProgram: ""
    property string sLns: ""
    property string sClockElf: ""
    property string sImport: ""

    // ── Derived ─────────────────────────────────────────────────────────────────────
    readonly property var recipientSchedules: schedules.filter(function(s) { return s.mine.beneficiary })
    readonly property var mySchedules: schedules.filter(function(s) {
        return s.mine.creator || s.mine.cancel_authority || s.mine.milestone_authority })
    readonly property var selected: findById(recipientSchedules, recipientId)
    readonly property var publicAccounts: accounts.filter(function(a) { return !a["private"] })
    readonly property var privateAccounts: accounts.filter(function(a) { return a["private"] })

    // ── Palette (the shared Logos theme, as the Names app uses it) ──────────────────
    readonly property color slate: Theme.palette.background
    readonly property color card: Theme.isDark ? Theme.palette.backgroundSecondary : Theme.palette.surfaceRaised
    readonly property color raised: Theme.isDark ? Theme.palette.surface : Theme.palette.backgroundButton
    readonly property color line: Theme.palette.border
    readonly property color ink: Theme.palette.text
    readonly property color dim: Theme.palette.textSecondary
    readonly property color faint: Theme.palette.textPlaceholder
    readonly property color brass: Theme.isDark ? Theme.palette.warning : Theme.palette.primary
    readonly property color alarm: Theme.palette.error
    readonly property color good: Theme.palette.success
    readonly property string sans: "Avenir Next"
    readonly property string mono: "Menlo"
    function px(size) { return Math.round(size * (Theme.typography.scale || 1)) }

    // The chain's time, falling back to the wall clock before now() answers.
    function viewTime() { return root.effectiveTime > 0 ? root.effectiveTime : root.tick }

    // ── Module calls ─────────────────────────────────────────────────────────────────

    function findById(list, id) {
        for (var i = 0; i < list.length; i++) if (String(list[i].id) === String(id)) return list[i]
        return null
    }

    /// One call of the vesting module. Reads answer in seconds; writes wait for the chain and a
    /// private claim proves on this machine, so they get a long `patience` (ms).
    function call(method, args, done, patience) {
        if (!bridgeReady) {
            done({ ok: false, error: "This app runs inside Basecamp, with the Vesting module installed." })
            return
        }
        var settle = function(raw) { done(Logic.reply(raw)) }
        if (logos.callModuleAsync)
            logos.callModuleAsync("vesting", method, args, settle, patience || 60000)
        else
            Qt.callLater(function() {
                try { settle(logos.callModule("vesting", method, args)) }
                catch (error) { done({ ok: false, error: String(error.message || error) }) }
            })
    }

    function refresh() {
        loading = true
        call("settings", [], function(r) {
            if (r.ok) {
                info = r
                fillSettings(r)
            } else if (r.missing) {
                info = null
                fail(r.error)
            }
            if (!r.ok || !r.connected) { loading = false; if (r.ok && r.status) problem = "Not connected: " + r.status + "."; return }
            call("accounts", [], function(a) {
                if (a.ok) {
                    accounts = a.accounts || []
                    if (cCreator === "" && publicAccounts.length) cCreator = publicAccounts[0].id
                } else fail(a.error)
            })
            call("now", [], function(n) { if (n.ok) effectiveTime = n.time })
            call("list", [], function(l) {
                loading = false
                if (!l.ok) { fail(l.error || "Couldn't load schedules."); return }
                schedules = (l.schedules || []).map(Logic.fromModule)
                names = l.names || ({})
                if (l.time) effectiveTime = l.time
                if (!findById(recipientSchedules, recipientId))
                    recipientId = recipientSchedules.length ? recipientSchedules[0].id : null
            }, 120000)
        })
    }

    function fillSettings(r) {
        var s = r.settings || {}
        sWallet = s.wallet_dir || ""
        sNetworkFile = s.network_file || ""
        sRpc = s.rpc || ""
        sChainId = s.chain_id || ""
        sProgram = s.program || ""
        sTokenProgram = s.token_program || ""
        sLns = s.lns_registry || ""
        sClockElf = s.clock_elf || ""
    }

    function fail(message) {
        var t = String(message).trim()
        problem = t === "" ? "" : t.charAt(0).toUpperCase() + t.substr(1) + (/[.?!]$/.test(t) ? "" : ".")
    }

    Component.onCompleted: refresh()

    Timer { interval: 1000; repeat: true; running: true; onTriggered: root.tick = Date.now() }

    // ── Settings ────────────────────────────────────────────────────────────────────

    function saveSettings() {
        problem = ""; resultLine = ""
        saving = true
        var patch = {
            wallet_dir: sWallet.trim(), network_file: sNetworkFile.trim(), rpc: sRpc.trim(),
            chain_id: sChainId.trim(), program: sProgram.trim(), token_program: sTokenProgram.trim(),
            lns_registry: sLns.trim(), clock_elf: sClockElf.trim()
        }
        if (sPassword !== "") patch.password = sPassword
        call("configure", [JSON.stringify(patch)], function(r) {
            saving = false
            sPassword = ""
            if (r.network) info = r
            if (!r.ok) { fail(r.error); resultLine = "Settings saved, not connected."; return }
            resultLine = "Connected to " + (r.network.name || r.network.rpc) + "."
            refresh()
        }, 60000)
    }

    function doImport() {
        problem = ""; resultLine = ""
        var a = Logic.accountInput(sImport)
        if (!a.ok) { fail(a.error); return }
        importing = true
        call("import_schedule", [a.account], function(r) {
            importing = false
            if (!r.ok) { fail(r.error); resultLine = "Import failed: " + problem; return }
            resultLine = "Imported the schedule as " + r.id + "."
            sImport = ""
            refresh()
        }, 60000)
    }

    // ── Recipient actions ───────────────────────────────────────────────────────────

    function chooseKind(kind) {
        destKind = kind
        destAccount = kind === "private" && privateAccounts.length ? privateAccounts[0].id : ""
    }

    /// The confirmation is the SDK's pre_claim_summary: claimable amount and the simulated fee.
    function beginClaim() {
        problem = ""
        resultLine = ""
        var s = selected
        if (!s) { problem = "Choose one of your vesting positions."; return }
        var isPrivate = destKind === "private"
        if (isPrivate && destAccount === "") { problem = "Choose a private account of your wallet."; return }
        preparing = true
        var id = Number(recipientId)
        call("pre_claim_summary", [id, isPrivate ? destAccount : ""], function(r) {
            preparing = false
            if (!r.ok) {
                fail(r.error || "Couldn't prepare the claim.")
                resultLine = "Claim couldn't be prepared: " + problem
                return
            }
            var sum = r.summary || {}
            if (!Logic.above(sum.claimable || "0", "0")) { resultLine = Logic.nothingClaimable(s, viewTime()); return }
            confirm = {
                id: recipientId, isPrivate: isPrivate,
                amountDisplay: sum.claimable_display, feeDisplay: sum.estimated_fee_display,
                destAccount: isPrivate ? destAccount : s.beneficiary,
                fee: Logic.feeCheck(sum), privacy: isPrivate ? Logic.privacyDisclosure() : null
            }
            confirming = true
        }, 120000)
    }

    function confirmClaim() {
        if (!confirm || !confirm.fee.ok) return
        claiming = true
        resultLine = confirm.isPrivate ? "Proving the private claim on this machine; this takes minutes." : ""
        var c = confirm, id = Number(c.id)
        var method = c.isPrivate ? "claim_private" : "claim_public"
        var args = c.isPrivate ? [id, c.destAccount] : [id]
        call(method, args, function(r) {
            claiming = false
            confirming = false
            if (!r.ok) {
                fail(r.error || "The claim failed.")
                resultLine = "Claim failed: " + problem
                return
            }
            resultLine = Logic.claimLineDisplay(r.amount_display, Logic.labelled(c.destAccount, root.names), r.tx, c.isPrivate)
            confirm = null
            refresh()
        }, c.isPrivate ? 3600000 : 300000)
    }

    function cancelConfirm() {
        confirming = false
        confirm = null
        resultLine = "Claim not sent."
    }

    // ── Creator actions ─────────────────────────────────────────────────────────────

    function sharedParams() {
        return {
            kind: cKind, token: cToken.trim(),
            start: Logic.parseDate(cStart), cliff: Logic.parseDate(cCliff), end: Logic.parseDate(cEnd),
            cancelable: cCancelable, transferable: cTransferable,
            cancelAuthority: cCancelAuthority.trim(), milestoneAuthority: cMilestoneAuthority.trim()
        }
    }

    function doCreate() {
        problem = ""
        resultLine = ""
        if (cCreator === "") { fail("Choose the creator: a public account of your wallet that pays"); return }
        var p = sharedParams()
        p.beneficiary = cBeneficiary.trim()
        p.amount = cAmount.trim()
        p.tranches = cKind === "milestone" ? cTranches.map(function(x) { return String(x).trim() }) : []
        var v = Logic.validateCreate(p)
        if (!v.ok) { fail(v.errors.join(" ")); resultLine = "Couldn't create: " + v.errors[0]; return }
        creating = true
        call("create", [JSON.stringify(Logic.toCreateRow(p, cCreator))], function(r) {
            creating = false
            if (!r.ok) { fail(r.error || "The create failed."); resultLine = "Create failed: " + problem; return }
            resultLine = Logic.resolvedLines(r.resolved).concat([Logic.createLine(r.id) + " " + Logic.txText(r.tx)]).join(" ")
            refresh()
        }, 300000)
    }

    function doBatch() {
        problem = ""
        resultLine = ""
        if (cCreator === "") { fail("Choose the creator: a public account of your wallet that pays"); return }
        var shared = sharedParams()
        var rows = bRows.map(function(row) {
            return { beneficiary: row.beneficiary.trim(), amount: row.amount.trim(), tranches: row.tranches }
        })
        var check = Object.assign({ rows: rows }, shared)
        var v = Logic.validateBatch(check)
        if (!v.ok) { fail(v.errors.join(" ")); resultLine = "Couldn't create batch: " + v.errors[0]; return }
        var params = rows.map(function(r) {
            var p = Object.assign({}, shared)
            p.beneficiary = r.beneficiary
            p.amount = r.amount
            p.tranches = shared.kind === "milestone" ? Logic.parseTrancheText(r.tranches) : []
            return Logic.toCreateRow(p, cCreator)
        })
        batchCreating = true
        call("create_batch", [JSON.stringify(params)], function(r) {
            batchCreating = false
            if (!r.ok) { fail(r.error || "The batch failed."); resultLine = "Batch failed: " + problem; return }
            resultLine = Logic.resolvedLines(r.resolved).concat([Logic.batchLine((r.ids || []).length) + " " + Logic.txText(r.tx)]).join(" ")
            refresh()
        }, 300000)
    }

    function doCancel(id) {
        problem = ""; resultLine = ""
        busyCancel = String(id)
        call("cancel", [Number(id)], function(r) {
            busyCancel = ""
            if (!r.ok) { fail(r.error || "The cancel failed."); resultLine = "Cancel failed: " + problem }
            else resultLine = Logic.cancelLine(id) + " Refunded " + r.refund_display + " to the creator."
            refresh()
        }, 300000)
    }

    function doIrrevocable(id) {
        problem = ""; resultLine = ""
        busyAction = "irrevocable:" + id
        call("make_irrevocable", [Number(id)], function(r) {
            busyAction = ""
            if (!r.ok) { fail(r.error || "The change failed."); resultLine = "Make irrevocable failed: " + problem }
            else resultLine = "Made schedule " + Logic.short(id) + " irrevocable."
            refresh()
        }, 300000)
    }

    function doSignal(id, index) {
        problem = ""; resultLine = ""
        busySignal = String(id) + ":" + index
        call("signal_milestone", [Number(id), Number(index)], function(r) {
            busySignal = ""
            if (!r.ok) { fail(r.error || "The signal failed."); resultLine = "Signal failed: " + problem }
            else resultLine = Logic.signalLine(index, id) + " Unlocked " + r.amount_display + "."
            refresh()
        }, 300000)
    }

    function doTransfer(id, target) {
        problem = ""; resultLine = ""
        var typed = Logic.accountInput(target)
        if (!typed.ok) { fail(typed.error); resultLine = "Transfer failed: " + problem; return }
        busyAction = "transfer:" + id
        call("transfer_beneficiary", [Number(id), typed.account], function(r) {
            busyAction = ""
            if (!r.ok) { fail(r.error || "The transfer failed."); resultLine = "Transfer failed: " + problem }
            else resultLine = "Transferred schedule " + Logic.short(id) + " to "
                + (r.name ? r.name + " (" + Logic.short(r.beneficiary) + ")" : Logic.short(r.beneficiary)) + "."
            refresh()
        }, 300000)
    }

    // ── View ────────────────────────────────────────────────────────────────────────

    Rectangle { anchors.fill: parent; color: root.slate }

    Flickable {
        id: page
        anchors.fill: parent
        contentHeight: column.implicitHeight + 64
        clip: true

        ColumnLayout {
            id: column
            width: Math.min(page.width - 48, 760)
            x: (page.width - width) / 2
            y: 32
            spacing: 18

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                Text {
                    text: "Vesting"
                    color: root.ink
                    font.family: root.sans
                    font.pixelSize: root.px(28)
                    font.weight: Font.Bold
                }
                Text {
                    Layout.fillWidth: true
                    text: "Claim what has vested to a public or private account, or create and manage schedules. "
                        + (root.connected ? "On " + (root.info.network.name || root.info.network.rpc) + "."
                                          : "Not connected: open Settings.")
                    color: root.dim
                    font.family: root.sans
                    font.pixelSize: root.px(16)
                    wrapMode: Text.Wrap
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                Tab { label: "Recipient"; active: root.tab === "recipient"; onPressed: root.tab = "recipient" }
                Tab { label: "Creator"; active: root.tab === "creator"; onPressed: root.tab = "creator" }
                Tab { label: "Settings"; active: root.tab === "settings"; onPressed: root.tab = "settings" }
                Item { Layout.fillWidth: true }
                Link { label: root.loading ? "Loading…" : "Refresh"; onPressed: if (!root.loading) root.refresh() }
            }

            Bad { text: root.problem }

            // ── Recipient view ──────────────────────────────────────────────────────
            Panel {
                visible: root.tab === "recipient"
                title: "1. Your position"
                Hint {
                    visible: root.recipientSchedules.length === 0
                    text: "No vesting positions for your wallet's accounts yet. A schedule made on another machine appears once you import its account (below)."
                }
                Repeater {
                    model: root.recipientSchedules
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: modelData.id + " · " + modelData.kind + " · " + modelData.tokenLabel
                        detail: modelData.total_display
                        chosen: String(root.recipientId) === String(modelData.id)
                        onPressed: root.recipientId = modelData.id
                    }
                }
                Fact { label: "Schedule kind"; value: root.selected ? root.selected.kind : "" }
                Fact { label: "Schedule account"; value: root.selected && root.selected.address ? root.selected.address : "" }
                Fact { label: "Beneficiary"; value: root.selected ? Logic.labelled(root.selected.beneficiary, root.names) : "" }
                Fact { label: "Total"; value: root.selected ? root.selected.total_display : "" }
                Fact { label: "Locked"; value: root.selected ? root.selected.locked_display : "" }
                Fact { label: "Claimed"; value: root.selected ? root.selected.claimed_display : "" }
                Fact { label: "Chain time"; value: Logic.dateText(viewTime()) }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Field { Layout.fillWidth: true; labelText: "Import a schedule account (hex or base58)"; text: root.sImport; code: true; onEdited: root.sImport = text.trim() }
                    Action { label: root.importing ? "Importing…" : "Import"; enabledNow: !root.importing && root.connected; onPressed: root.doImport() }
                }
            }

            Panel {
                visible: root.tab === "recipient" && root.selected !== null
                title: "2. Claim"
                Status {
                    text: "Claimable now: " + (root.selected ? root.selected.claimable_display : "")
                    tone: root.good
                }
                Fact { label: "Next unlock"; value: root.selected ? Logic.nextUnlock(root.selected, viewTime()).text : "" }
                Hint { text: "A public claim pays the beneficiary account, in the open. A private claim pays a private account of your wallet; later movements are hidden." }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    ChoiceInline { label: "Public account"; chosen: root.destKind === "public"; onPressed: root.chooseKind("public") }
                    ChoiceInline { label: "Private account"; chosen: root.destKind === "private"; onPressed: root.chooseKind("private") }
                }
                Fact {
                    visible: root.destKind === "public"
                    label: "Pays"
                    value: root.selected ? Logic.labelled(root.selected.beneficiary, root.names) : ""
                }
                Hint { visible: root.destKind === "private" && root.privateAccounts.length === 0; text: "Your wallet has no private account. Create one with the LEZ wallet first." }
                Repeater {
                    model: root.destKind === "private" ? root.privateAccounts : []
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: modelData.base58
                        detail: modelData.balance_display
                        chosen: root.destAccount === modelData.id
                        onPressed: root.destAccount = modelData.id
                    }
                }
                RowLayout {
                    spacing: 12
                    Action {
                        label: root.preparing ? "Checking the fee…" : root.claiming ? "Claiming…" : "Claim"
                        strong: true
                        enabledNow: !root.preparing && !root.claiming && !root.confirming && root.selected !== null
                        onPressed: root.beginClaim()
                    }
                }
            }

            // Pre-claim confirmation (the SDK's pre_claim_summary, with the simulated fee).
            Panel {
                visible: root.tab === "recipient" && root.confirming
                title: "Confirm claim"
                Fact { label: "Claimable amount"; value: root.confirm ? root.confirm.amountDisplay : "" }
                Fact { label: "Estimated fee"; value: root.confirm ? root.confirm.feeDisplay : "" }
                Fact { label: "Destination"; value: root.confirm ? Logic.labelled(root.confirm.destAccount, root.names) + (root.confirm.isPrivate ? " (private)" : " (public)") : "" }
                Status {
                    visible: root.confirm !== null && root.confirm.isPrivate
                    text: root.confirm && root.confirm.privacy ? root.confirm.privacy.text : ""
                    tone: root.brass
                }
                Status {
                    visible: root.confirm !== null && root.confirm.fee.ok
                    text: root.confirm ? root.confirm.fee.message : ""
                    tone: root.good
                }
                Bad { text: root.confirm && !root.confirm.fee.ok ? root.confirm.fee.message : "" }
                RowLayout {
                    spacing: 12
                    Action {
                        label: root.claiming ? "Claiming…" : "Confirm claim"
                        strong: true
                        enabledNow: !root.claiming && root.confirm !== null && root.confirm.fee.ok
                        onPressed: root.confirmClaim()
                    }
                    Link { label: "Cancel"; visible: !root.claiming; onPressed: root.cancelConfirm() }
                }
            }

            // ── Creator view ────────────────────────────────────────────────────────
            Panel {
                visible: root.tab === "creator"
                title: "Create schedules"
                Hint { text: "The creator signs and pays: choose one of your wallet's public accounts. Dates are yyyy-mm-dd. Accounts may be hex, base58 or an LNS name." }
                Repeater {
                    model: root.publicAccounts
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: modelData.base58
                        detail: modelData.balance_display
                        chosen: root.cCreator === modelData.id
                        onPressed: root.cCreator = modelData.id
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8
                    ChoiceInline { label: "Cliff + linear"; chosen: root.cKind === "cliff-linear"; onPressed: root.cKind = "cliff-linear" }
                    ChoiceInline { label: "Linear"; chosen: root.cKind === "linear"; onPressed: root.cKind = "linear" }
                    ChoiceInline { label: "Milestone"; chosen: root.cKind === "milestone"; onPressed: root.cKind = "milestone" }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Field { Layout.fillWidth: true; labelText: "Token (LEZ or a token definition)"; text: root.cToken; code: true; onEdited: root.cToken = text.trim() }
                    Field { Layout.fillWidth: true; visible: root.cKind !== "milestone"; labelText: "Start date"; text: root.cStart; onEdited: root.cStart = text.trim() }
                    Field {
                        Layout.fillWidth: true
                        visible: root.cKind === "cliff-linear"
                        labelText: "Cliff date"
                        text: root.cCliff
                        onEdited: root.cCliff = text.trim()
                    }
                    Field { Layout.fillWidth: true; visible: root.cKind !== "milestone"; labelText: "End date"; text: root.cEnd; onEdited: root.cEnd = text.trim() }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 18
                    Toggle { label: "Cancelable"; checked: root.cCancelable; onPressed: root.cCancelable = !root.cCancelable }
                    Toggle { label: "Transferable"; checked: root.cTransferable; onPressed: root.cTransferable = !root.cTransferable }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Field { Layout.fillWidth: true; labelText: "Cancel authority (optional)"; text: root.cCancelAuthority; code: true; onEdited: root.cCancelAuthority = text.trim() }
                    Field { Layout.fillWidth: true; labelText: "Milestone authority (optional)"; text: root.cMilestoneAuthority; code: true; onEdited: root.cMilestoneAuthority = text.trim() }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    ChoiceInline { label: "One recipient"; chosen: !root.batchMode; onPressed: root.batchMode = false }
                    ChoiceInline { label: "Batch (one token, one transaction)"; chosen: root.batchMode; onPressed: root.batchMode = true }
                }

                // One recipient.
                ColumnLayout {
                    visible: !root.batchMode
                    Layout.fillWidth: true
                    spacing: 10
                    Field { Layout.fillWidth: true; labelText: "Beneficiary (LNS name or account)"; text: root.cBeneficiary; code: true; onEdited: root.cBeneficiary = text.trim() }
                    Field { Layout.fillWidth: true; visible: root.cKind !== "milestone"; labelText: "Total amount (smallest unit)"; text: root.cAmount; code: true; onEdited: root.cAmount = text.trim() }
                    Repeater {
                        model: root.cKind === "milestone" ? root.cTranches : []
                        delegate: RowLayout {
                            Layout.fillWidth: true
                            spacing: 10
                            Field {
                                Layout.fillWidth: true
                                labelText: "Tranche " + (index + 1) + " amount"
                                text: root.cTranches[index]
                                code: true
                                onEdited: {
                                    var next = root.cTranches.slice()
                                    next[index] = text.trim()
                                    root.cTranches = next
                                }
                            }
                            Link {
                                visible: root.cTranches.length > 1
                                label: "Remove"
                                onPressed: { var next = root.cTranches.slice(); next.splice(index, 1); root.cTranches = next }
                            }
                        }
                    }
                    Link {
                        visible: root.cKind === "milestone"
                        label: "+ Add tranche"
                        onPressed: { var next = root.cTranches.slice(); next.push(""); root.cTranches = next }
                    }
                    Action {
                        label: root.creating ? "Creating…" : "Create schedule"
                        strong: true
                        enabledNow: !root.creating && root.connected
                        onPressed: root.doCreate()
                    }
                }

                // Batch: add/remove rows, shared settings above, one transaction.
                ColumnLayout {
                    visible: root.batchMode
                    Layout.fillWidth: true
                    spacing: 10
                    Hint { text: "One row per recipient. For milestone schedules, put tranche amounts in the tranches field, separated by commas. Up to 140 native or 72 token schedules fit in one transaction." }
                    Repeater {
                        model: root.bRows
                        delegate: Rectangle {
                            required property var modelData
                            required property int index
                            Layout.fillWidth: true
                            implicitHeight: batchRow.implicitHeight + 20
                            radius: 6
                            color: "transparent"
                            border.width: 1
                            border.color: root.line
                            ColumnLayout {
                                id: batchRow
                                anchors.fill: parent
                                anchors.margins: 10
                                spacing: 6
                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 10
                                    Field {
                                        Layout.fillWidth: true
                                        labelText: "Recipient " + (index + 1) + " (LNS name or account)"
                                        text: modelData.beneficiary
                                        code: true
                                        onEdited: {
                                            var next = root.bRows.slice()
                                            next[index] = Object.assign({}, next[index], { beneficiary: text.trim() })
                                            root.bRows = next
                                        }
                                    }
                                    Field {
                                        Layout.fillWidth: true
                                        visible: root.cKind !== "milestone"
                                        labelText: "Amount"
                                        text: modelData.amount
                                        code: true
                                        onEdited: {
                                            var next = root.bRows.slice()
                                            next[index] = Object.assign({}, next[index], { amount: text.trim() })
                                            root.bRows = next
                                        }
                                    }
                                }
                                Field {
                                    visible: root.cKind === "milestone"
                                    Layout.fillWidth: true
                                    labelText: "Tranches (comma separated)"
                                    text: modelData.tranches
                                    code: true
                                    onEdited: {
                                        var next = root.bRows.slice()
                                        next[index] = Object.assign({}, next[index], { tranches: text.trim() })
                                        root.bRows = next
                                    }
                                }
                                Link {
                                    label: "Remove recipient"
                                    visible: root.bRows.length > 1
                                    onPressed: { var next = root.bRows.slice(); next.splice(index, 1); root.bRows = next }
                                }
                            }
                        }
                    }
                    RowLayout {
                        spacing: 12
                        Link {
                            label: "+ Add recipient"
                            onPressed: { var next = root.bRows.slice(); next.push({ beneficiary: "", amount: "", tranches: "" }); root.bRows = next }
                        }
                        Action {
                            label: root.batchCreating ? "Creating batch…" : "Create " + root.bRows.length + " schedules"
                            strong: true
                            enabledNow: !root.batchCreating && root.connected
                            onPressed: root.doBatch()
                        }
                    }
                }
            }

            Panel {
                visible: root.tab === "creator"
                title: "Schedules you manage"
                Hint { visible: root.mySchedules.length === 0; text: "No schedules where your wallet is the creator or an authority." }
                Repeater {
                    model: root.mySchedules
                    delegate: Rectangle {
                        id: schedCard
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: sched.implicitHeight + 20
                        radius: 6
                        color: "transparent"
                        border.width: 1
                        border.color: root.line
                        ColumnLayout {
                            id: sched
                            anchors.fill: parent
                            anchors.margins: 10
                            spacing: 6
                            Text {
                                text: modelData.id + " · " + modelData.kind + " · " + modelData.total_display
                                color: root.ink
                                font.family: root.sans
                                font.pixelSize: root.px(16)
                                font.weight: Font.DemiBold
                            }
                            Text {
                                Layout.fillWidth: true
                                text: "beneficiary " + Logic.labelled(modelData.beneficiary, root.names)
                                    + " · claimed " + modelData.claimed_display
                                    + (modelData.cancelled ? " · CANCELLED" : modelData.cancelable ? "" : " · irrevocable")
                                color: root.dim
                                font.family: root.sans
                                font.pixelSize: root.px(14)
                                wrapMode: Text.Wrap
                            }
                            RowLayout {
                                spacing: 10
                                Action {
                                    visible: modelData.cancelable && !modelData.cancelled && modelData.mine.cancel_authority
                                    label: root.busyCancel === String(modelData.id) ? "Cancelling…" : "Cancel"
                                    enabledNow: root.busyCancel !== String(modelData.id)
                                    onPressed: root.doCancel(modelData.id)
                                }
                                Action {
                                    visible: modelData.cancelable && !modelData.cancelled && modelData.mine.cancel_authority
                                    label: root.busyAction === "irrevocable:" + modelData.id ? "Updating…" : "Make irrevocable"
                                    enabledNow: root.busyAction !== "irrevocable:" + modelData.id
                                    onPressed: root.doIrrevocable(modelData.id)
                                }
                                Repeater {
                                    model: schedCard.modelData.kind === "milestone" && schedCard.modelData.mine.milestone_authority ? schedCard.modelData.tranches : []
                                    delegate: Action {
                                        required property int index
                                        readonly property var sched: schedCard.modelData
                                        visible: !sched.signalled[index] && !sched.cancelled
                                        label: root.busySignal === String(sched.id) + ":" + index ? "Signalling…" : "Signal tranche " + (index + 1)
                                        enabledNow: root.busySignal !== String(sched.id) + ":" + index
                                        onPressed: root.doSignal(sched.id, index)
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Panel {
                visible: root.tab === "recipient" && root.selected !== null && root.selected.transferable && !root.selected.cancelled
                title: "Transfer this position"
                Hint { text: "This schedule was created transferable: you can hand it to another beneficiary." }
                RowLayout {
                    spacing: 10
                    Layout.fillWidth: true
                    Field {
                        id: transferField
                        Layout.fillWidth: true
                        labelText: "New beneficiary (LNS name or account)"
                        code: true
                    }
                    Action {
                        label: root.busyAction === "transfer:" + (root.selected ? root.selected.id : "") ? "Transferring…" : "Transfer"
                        enabledNow: root.selected !== null && root.busyAction === ""
                        onPressed: root.doTransfer(root.selected.id, transferField.text)
                    }
                }
            }

            // ── Settings ────────────────────────────────────────────────────────────
            Panel {
                visible: root.tab === "settings"
                title: "Wallet and network"
                Status {
                    text: root.connected ? "Connected: " + (root.info.network.name || "") + " · " + root.info.network.rpc
                                         : "Not connected" + (root.info && root.info.status ? ": " + root.info.status : "")
                    tone: root.connected ? root.good : root.alarm
                }
                Hint { text: "The wallet is a LEZ rc3 wallet folder (wallet_config.json and storage.json). Its password is kept in memory only. Leave the network fields empty to use the built-in network (the public LEZ rc3 demo chain and its Vesting deployment), or give a network file." }
                Field { Layout.fillWidth: true; labelText: "Wallet folder"; text: root.sWallet; code: true; onEdited: root.sWallet = text.trim() }
                Field { Layout.fillWidth: true; labelText: "Wallet password (if the wallet is encrypted)"; text: root.sPassword; secret: true; onEdited: root.sPassword = text }
                Field { Layout.fillWidth: true; labelText: "Network file (optional, names-network format)"; text: root.sNetworkFile; code: true; onEdited: root.sNetworkFile = text.trim() }
                Fact { label: "Network"; value: root.info && root.info.network ? (root.info.network.name || "") + " (" + (root.info.network.source || "") + ")" : "" }
                Fact { label: "RPC"; value: root.info && root.info.network ? root.info.network.rpc || "" : "" }
                Fact { label: "Chain id"; value: root.info && root.info.network ? root.info.network.chain_id || "" : "" }
                Fact { label: "Vesting program"; value: root.info && root.info.network ? root.info.network.program || "" : "" }
                Fact { label: "Token program"; value: root.info && root.info.network ? root.info.network.token_program || "" : "" }
                Fact { label: "LNS registry"; value: root.info && root.info.network ? root.info.network.lns_registry || "" : "" }
                Hint { text: "Overrides (optional): each replaces one value of the network. Another RPC needs its chain id, unless it is local (127.0.0.1)." }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Field { Layout.fillWidth: true; labelText: "RPC"; text: root.sRpc; code: true; onEdited: root.sRpc = text.trim() }
                    Field { Layout.fillWidth: true; labelText: "Chain id"; text: root.sChainId; code: true; onEdited: root.sChainId = text.trim() }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Field { Layout.fillWidth: true; labelText: "Vesting program"; text: root.sProgram; code: true; onEdited: root.sProgram = text.trim() }
                    Field { Layout.fillWidth: true; labelText: "Token program"; text: root.sTokenProgram; code: true; onEdited: root.sTokenProgram = text.trim() }
                }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Field { Layout.fillWidth: true; labelText: "LNS registry"; text: root.sLns; code: true; onEdited: root.sLns = text.trim() }
                    Field { Layout.fillWidth: true; labelText: "Clock ELF (only if the chain doesn't serve it)"; text: root.sClockElf; code: true; onEdited: root.sClockElf = text.trim() }
                }
                Action {
                    label: root.saving ? "Connecting…" : "Save and connect"
                    strong: true
                    enabledNow: !root.saving
                    onPressed: root.saveSettings()
                }
                Fact { label: "Data folder"; value: root.info ? root.info.data_dir || "" : "" }
            }

            Panel {
                title: "Result"
                Status { visible: root.resultLine !== ""; text: root.resultLine; tone: root.good }
                Hint { visible: root.resultLine === "" && !root.problem; text: "Actions show their result here." }
            }
        }
    }

    // ── Controls ────────────────────────────────────────────────────────────────────

    component Panel: Rectangle {
        default property alias content: panelColumn.data
        property string title: ""
        Layout.fillWidth: true
        implicitHeight: panelColumn.implicitHeight + 36
        radius: 8
        color: root.card
        border.width: 1
        border.color: root.line
        ColumnLayout {
            id: panelColumn
            anchors.fill: parent
            anchors.margins: 18
            spacing: 10
            Text {
                text: parent.parent.title
                color: root.ink
                font.family: root.sans
                font.pixelSize: root.px(19)
                font.weight: Font.DemiBold
            }
        }
    }

    component Hint: Text {
        Layout.fillWidth: true
        color: root.dim
        font.family: root.sans
        font.pixelSize: root.px(15)
        wrapMode: Text.Wrap
        textFormat: Text.PlainText
    }

    component Bad: Text {
        Layout.fillWidth: true
        visible: text !== ""
        color: root.alarm
        font.family: root.sans
        font.pixelSize: root.px(15)
        wrapMode: Text.Wrap
        textFormat: Text.PlainText
    }

    component Status: Text {
        property color tone: root.ink
        Layout.fillWidth: true
        color: tone
        font.family: root.sans
        font.pixelSize: root.px(17)
        font.weight: Font.Medium
        wrapMode: Text.Wrap
        textFormat: Text.PlainText
    }

    component Fact: RowLayout {
        property string label: ""
        property string value: ""
        Layout.fillWidth: true
        spacing: 10
        Text { text: parent.label; color: root.dim; font.family: root.sans; font.pixelSize: root.px(15) }
        TextEdit {
            Layout.fillWidth: true
            text: parent.value
            readOnly: true
            selectByMouse: true
            color: root.ink
            font.family: root.mono
            font.pixelSize: root.px(14)
            wrapMode: TextEdit.WrapAnywhere
        }
    }

    component Action: Rectangle {
        id: action
        property string label: ""
        property bool strong: false
        property bool enabledNow: true
        signal pressed()
        implicitWidth: actionText.implicitWidth + 28
        implicitHeight: 38
        radius: 6
        color: !enabledNow ? "transparent" : strong ? (actionArea.containsMouse ? Qt.lighter(root.brass, 1.08) : root.brass) : (actionArea.containsMouse ? root.raised : "transparent")
        border.width: strong && enabledNow ? 0 : 1
        border.color: activeFocus ? root.brass : root.line
        opacity: enabledNow ? 1 : 0.55
        activeFocusOnTab: enabledNow
        Keys.onReturnPressed: if (enabledNow) action.pressed()
        Keys.onSpacePressed: if (enabledNow) action.pressed()
        Text {
            id: actionText
            anchors.centerIn: parent
            text: action.label
            color: action.strong && action.enabledNow ? root.slate : root.ink
            font.family: root.sans
            font.pixelSize: root.px(17)
            font.weight: Font.DemiBold
        }
        MouseArea {
            id: actionArea
            anchors.fill: parent
            hoverEnabled: true
            enabled: action.enabledNow
            cursorShape: Qt.PointingHandCursor
            // Take focus first: a Field commits its text on editingFinished, which a click
            // elsewhere does not send, so without this a value typed just before the click is lost.
            onClicked: { action.forceActiveFocus(); action.pressed() }
        }
    }

    component Choice: Rectangle {
        id: choice
        property string label: ""
        property string detail: ""
        property bool chosen: false
        signal pressed()
        implicitHeight: 40
        radius: 6
        color: choiceArea.containsMouse ? root.raised : "transparent"
        border.width: 1
        border.color: chosen ? root.brass : root.line
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            spacing: 10
            Rectangle {
                implicitWidth: 14
                implicitHeight: 14
                radius: 7
                color: choice.chosen ? root.brass : "transparent"
                border.width: 1
                border.color: choice.chosen ? root.brass : root.dim
            }
            Text {
                Layout.fillWidth: true
                text: choice.label
                color: root.ink
                font.family: root.mono
                font.pixelSize: root.px(14)
                elide: Text.ElideMiddle
            }
            Text { text: choice.detail; color: root.dim; font.family: root.sans; font.pixelSize: root.px(14) }
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
        color: ci.chosen ? root.brass : (ciArea.containsMouse ? root.raised : "transparent")
        border.width: 1
        border.color: ci.chosen ? root.brass : root.line
        opacity: ci.chosen ? 1 : 0.9
        Text {
            id: ciText
            anchors.centerIn: parent
            text: ci.label
            color: ci.chosen ? root.slate : root.ink
            font.family: root.sans
            font.pixelSize: root.px(15)
            font.weight: Font.Medium
        }
        MouseArea { id: ciArea; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: ci.pressed() }
    }

    component Tab: Rectangle {
        id: tab
        property string label: ""
        property bool active: false
        signal pressed()
        implicitWidth: tabText.implicitWidth + 28
        implicitHeight: 36
        radius: 6
        color: tab.active ? root.card : "transparent"
        border.width: tab.active ? 2 : 1
        border.color: tab.active ? root.brass : root.line
        Text {
            id: tabText
            anchors.centerIn: parent
            text: tab.label
            color: tab.active ? root.brass : root.dim
            font.family: root.sans
            font.pixelSize: root.px(17)
            font.weight: Font.DemiBold
        }
        MouseArea { anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: tab.pressed() }
    }

    component Toggle: Rectangle {
        id: tog
        property string label: ""
        property bool checked: false
        signal pressed()
        implicitWidth: togText.implicitWidth + 46
        implicitHeight: 32
        radius: 5
        color: "transparent"
        RowLayout {
            anchors.fill: parent
            spacing: 8
            Rectangle {
                implicitWidth: 16
                implicitHeight: 16
                radius: 3
                color: tog.checked ? root.brass : "transparent"
                border.width: 1
                border.color: tog.checked ? root.brass : root.dim
            }
            Text {
                id: togText
                text: tog.label
                color: root.ink
                font.family: root.sans
                font.pixelSize: root.px(15)
            }
        }
        MouseArea { anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: tog.pressed() }
    }

    component Link: Text {
        id: link
        property string label: ""
        signal pressed()
        text: label
        color: linkArea.containsMouse ? root.brass : root.ink
        font.family: root.sans
        font.pixelSize: root.px(16)
        font.underline: true
        MouseArea { id: linkArea; anchors.fill: parent; anchors.margins: -6; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: link.pressed() }
    }

    component Field: Rectangle {
        id: field
        property alias text: input.text
        property string labelText: ""
        property bool code: false
        property bool secret: false
        signal edited()
        implicitHeight: 58
        radius: 6
        color: root.slate
        border.width: 1
        border.color: input.activeFocus ? root.brass : root.line
        ColumnLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            spacing: 0
            Text {
                visible: field.labelText !== ""
                text: field.labelText
                color: root.faint
                font.family: root.sans
                font.pixelSize: root.px(12)
            }
            TextInput {
                id: input
                Layout.fillWidth: true
                Layout.fillHeight: true
                verticalAlignment: TextInput.AlignVCenter
                color: root.ink
                selectionColor: root.brass
                selectedTextColor: root.slate
                echoMode: field.secret ? TextInput.Password : TextInput.Normal
                font.family: field.code ? root.mono : root.sans
                font.pixelSize: field.code ? root.px(14) : root.px(16)
                clip: true
                selectByMouse: true
                onEditingFinished: field.edited()
            }
        }
    }
}
