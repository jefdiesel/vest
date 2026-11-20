// Vesting: view and manage LEZ vesting schedules.
//
// UI-only mini-app with a MOCK backend (src/vesting_ui_backend.cpp). Each backend slot maps
// to one SDK Client call (see src/vesting_ui.rep and ../module/README.md); the real Logos core
// module can replace the C++ file without changing this view. No chain or wallet calls are
// made yet. All display logic is in logic.mjs, which the Node test runs: formatting, exact
// vesting maths, validation, next-unlock text, the pre-claim confirmation and result lines.
//
// Controls are drawn here rather than taken from Qt Quick Controls, as in the Names app.

import QtQuick
import QtQuick.Layouts
import Logos.Theme
import "logic.mjs" as Logic

Item {
    id: root

    // ── Backend data (one slot per SDK call) ─────────────────────────────────────────
    property var schedules: []
    property var names: ({})                   // LNS mock map: name -> account ID
    property real effectiveTime: 0             // backend.now(): what the module reports
    readonly property var backend: typeof logos !== "undefined" && logos.module ? logos.module("vesting_ui") : null
    property bool backendReady: false

    // ── Module / wallet-supplied placeholders (the real module supplies these) ──────
    readonly property string caller: "Public/Baaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    readonly property var publicAccounts: ["Public/Daaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
    readonly property var privateAccounts: ["Private/Paaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
    readonly property var gas: ({ "Public/Daaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa": "500",
                                  "Private/Paaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa": "0" })

    // ── View state ──────────────────────────────────────────────────────────────────
    property bool creatorTab: false
    property real tick: Date.now()             // only a fallback when now() has not answered
    property var recipientId: null
    property string destKind: "public"
    property string destAccount: ""
    property bool confirming: false
    property var confirm: null
    property string resultLine: ""
    property string problem: ""

    property bool preparing: false             // preClaimSummary in flight
    property bool claiming: false
    property bool creating: false
    property bool batchCreating: false
    property string busyCancel: ""
    property string busySignal: ""
    property string busyAction: ""             // irrevocable / transfer

    // ── Single create form ──────────────────────────────────────────────────────────
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

    // ── Derived ─────────────────────────────────────────────────────────────────────
    readonly property var recipientSchedules: schedules.filter(function(s) { return s.beneficiary === root.caller })
    readonly property var mySchedules: schedules.filter(function(s) { return s.creator === root.caller })
    readonly property var selected: findById(recipientSchedules, recipientId)
    readonly property var shownAccounts: destKind === "private" ? privateAccounts : publicAccounts

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

    // The module's effective time, falling back to the wall clock before now() answers.
    function viewTime() { return root.effectiveTime > 0 ? root.effectiveTime : root.tick }

    // ── Data and module calls ────────────────────────────────────────────────────────

    function findById(list, id) {
        for (var i = 0; i < list.length; i++) if (String(list[i].id) === String(id)) return list[i]
        return null
    }

    function callSlot(pending, done) {
        if (!backend || !backendReady) {
            done({ ok: false, error: "This app's module isn't running yet; try again in a moment." })
            return
        }
        logos.watch(pending(), function(text) {
            var r = null
            try { r = JSON.parse(text) } catch (error) {}
            done(r || { ok: false, error: "the module sent an unreadable answer" })
        }, function(error) { done({ ok: false, error: String(error) }) })
    }

    function refresh() {
        callSlot(function() { return backend.list() }, function(r) {
            if (!r || !r.ok) { fail(r && r.error ? r.error : "Couldn't load schedules."); return }
            schedules = r.schedules || []
            if (!findById(schedules, recipientId)) recipientId = schedules.length ? schedules[0].id : null
        })
        callSlot(function() { return backend.now() }, function(r) {
            if (r && r.ok) effectiveTime = r.time
        })
        callSlot(function() { return backend.names() }, function(r) {
            if (r && r.ok) names = r.names || ({})
        })
    }

    /// Resolve a typed name or account ID into the account the module expects.
    function resolveInput(text) { return Logic.accountInput(text, root.names) }

    function gasOf(account) { return root.gas[account] || "0" }

    function fail(message) {
        var t = String(message).trim()
        problem = t === "" ? "" : t.charAt(0).toUpperCase() + t.substr(1) + (/[.?!]$/.test(t) ? "" : ".")
    }

    Component.onCompleted: {
        backendReady = backend !== null && logos.isViewModuleReady && logos.isViewModuleReady("vesting_ui")
        refresh()
    }

    Connections {
        target: typeof logos !== "undefined" ? logos : null
        ignoreUnknownSignals: true
        function onViewModuleReadyChanged(moduleName, isReady) {
            if (moduleName === "vesting_ui") {
                root.backendReady = isReady && root.backend !== null
                if (root.backendReady) root.refresh()
            }
        }
    }

    Timer { interval: 1000; repeat: true; running: true; onTriggered: root.tick = Date.now() }

    // ── Recipient actions ───────────────────────────────────────────────────────────

    function chooseKind(kind) {
        destKind = kind
        var list = kind === "private" ? privateAccounts : publicAccounts
        destAccount = list.length ? list[0] : ""
    }

    /// The confirmation is the SDK's pre_claim_summary: claimable amount + estimated fee.
    function beginClaim() {
        problem = ""
        resultLine = ""
        var s = selected
        if (!s) { problem = "Choose one of your vesting positions."; return }
        preparing = true
        var id = Number(recipientId), isPrivate = destKind === "private"
        callSlot(function() { return backend.preClaimSummary(id, isPrivate) }, function(r) {
            preparing = false
            if (!r || !r.ok) {
                fail(r && r.error ? r.error : "Couldn't prepare the claim.")
                resultLine = "Claim couldn't be prepared: " + problem
                return
            }
            var sum = r.summary || {}
            var amount = sum.claimable || "0"
            var fee = sum.estimated_fee || Logic.FEE
            if (!Logic.above(amount, "0")) { resultLine = Logic.nothingClaimable(s, viewTime()); return }
            var dest = resolveInput(destAccount)
            if (!dest.ok) { problem = dest.error; return }
            destAccount = dest.account
            var destProblem = Logic.problemAccount(destAccount, isPrivate)
            var gasCheck = isPrivate ? Logic.canPayFee(gasOf(destAccount), fee) : null
            confirm = {
                id: recipientId, amount: amount, token: s.token || Logic.TOKEN,
                amountDisplay: sum.claimable_display || amount,
                fee: fee, feeDisplay: sum.estimated_fee_display || fee,
                isPrivate: isPrivate, destAccount: destAccount, destProblem: destProblem,
                gasCheck: gasCheck, privacy: isPrivate ? Logic.privacyDisclosure() : null
            }
            confirming = true
        })
    }

    function confirmClaim() {
        if (!confirm) return
        if (confirm.destProblem !== "" || (confirm.gasCheck && !confirm.gasCheck.ok)) return
        claiming = true
        resultLine = ""
        var c = confirm, id = Number(c.id), isPrivate = c.isPrivate
        var pending = isPrivate ? function() { return backend.claimPrivate(id, c.destAccount) }
                                : function() { return backend.claimPublic(id) }
        callSlot(pending, function(r) {
            claiming = false
            confirming = false
            if (!r || !r.ok) {
                fail(r && r.error ? r.error : "The claim failed.")
                resultLine = "Claim failed: " + problem
                return
            }
            resultLine = Logic.claimLineDisplay(r.amount_display || c.amountDisplay, c.destAccount, r.tx, isPrivate)
            confirm = null
            refresh()
        })
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
        var p = sharedParams()
        var beneficiary = resolveInput(cBeneficiary)
        if (!beneficiary.ok) { fail(beneficiary.error); resultLine = "Couldn't create: " + problem; return }
        p.beneficiary = beneficiary.account
        p.amount = cAmount.trim()
        p.tranches = cKind === "milestone" ? cTranches.map(function(x) { return String(x).trim() }) : []
        p.signalled = p.tranches.map(function() { return false })
        var v = Logic.validateCreate(p)
        if (!v.ok) { fail(v.errors.join(" ")); resultLine = "Couldn't create: " + v.errors[0]; return }
        creating = true
        callSlot(function() { return backend.create(JSON.stringify(p)) }, function(r) {
            creating = false
            if (!r || !r.ok) { fail(r && r.error ? r.error : "The create failed."); resultLine = "Create failed: " + problem; return }
            resultLine = Logic.createLine(r.id)
            refresh()
        })
    }

    function doBatch() {
        problem = ""
        resultLine = ""
        var rows = []
        for (var i = 0; i < bRows.length; i++) {
            var row = bRows[i]
            var dest = resolveInput(row.beneficiary)
            if (!dest.ok) {
                fail("Recipient " + (i + 1) + ": " + dest.error)
                resultLine = "Couldn't create batch: " + problem
                return
            }
            rows.push({ beneficiary: dest.account, amount: row.amount.trim(),
                        tranches: Logic.parseTrancheText(row.tranches) })
        }
        var shared = sharedParams()
        var v = Logic.validateBatch({ kind: shared.kind, token: shared.token, start: shared.start,
                                      cliff: shared.cliff, end: shared.end, rows: rows })
        if (!v.ok) { fail(v.errors.join(" ")); resultLine = "Couldn't create batch: " + v.errors[0]; return }
        var params = rows.map(function(r) {
            var p = Object.assign({}, shared)
            p.beneficiary = r.beneficiary
            p.amount = r.amount
            p.tranches = r.tranches
            p.signalled = r.tranches.map(function() { return false })
            return p
        })
        batchCreating = true
        callSlot(function() { return backend.createBatch(JSON.stringify(params)) }, function(r) {
            batchCreating = false
            if (!r || !r.ok) { fail(r && r.error ? r.error : "The batch failed."); resultLine = "Batch failed: " + problem; return }
            resultLine = Logic.batchLine((r.ids || []).length)
            refresh()
        })
    }

    function doCancel(id) {
        problem = ""; resultLine = ""
        busyCancel = String(id)
        callSlot(function() { return backend.cancel(id) }, function(r) {
            busyCancel = ""
            if (!r || !r.ok) { fail(r && r.error ? r.error : "The cancel failed."); resultLine = "Cancel failed: " + problem }
            else resultLine = Logic.cancelLine(id)
            refresh()
        })
    }

    function doIrrevocable(id) {
        problem = ""; resultLine = ""
        busyAction = "irrevocable:" + id
        callSlot(function() { return backend.makeIrrevocable(id) }, function(r) {
            busyAction = ""
            if (!r || !r.ok) { fail(r && r.error ? r.error : "The change failed."); resultLine = "Make irrevocable failed: " + problem }
            else resultLine = "Made schedule " + Logic.short(id) + " irrevocable."
            refresh()
        })
    }

    function doSignal(id, index) {
        problem = ""; resultLine = ""
        busySignal = String(id) + ":" + index
        callSlot(function() { return backend.signal(id, index) }, function(r) {
            busySignal = ""
            if (!r || !r.ok) { fail(r && r.error ? r.error : "The signal failed."); resultLine = "Signal failed: " + problem }
            else resultLine = Logic.signalLine(index, id)
            refresh()
        })
    }

    function doTransfer(id, target) {
        problem = ""; resultLine = ""
        var resolved = resolveInput(target)
        if (!resolved.ok) { fail(resolved.error); resultLine = "Transfer failed: " + problem; return }
        target = resolved.account
        busyAction = "transfer:" + id
        callSlot(function() { return backend.transfer(id, target) }, function(r) {
            busyAction = ""
            if (!r || !r.ok) { fail(r && r.error ? r.error : "The transfer failed."); resultLine = "Transfer failed: " + problem }
            else resultLine = "Transferred schedule " + Logic.short(id) + " to " + target + "."
            refresh()
        })
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

            RowLayout {
                Layout.fillWidth: true
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
                        text: "View a vesting position and claim to a public or private account, or create and manage schedules. Data is from a local mock."
                        color: root.dim
                        font.family: root.sans
                        font.pixelSize: root.px(16)
                        wrapMode: Text.Wrap
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 10
                Tab { label: "Recipient"; active: !root.creatorTab; onPressed: root.creatorTab = false }
                Tab { label: "Creator"; active: root.creatorTab; onPressed: root.creatorTab = true }
            }

            // ── Recipient view ──────────────────────────────────────────────────────
            Panel {
                visible: !root.creatorTab
                title: "1. Your position"
                Hint { visible: root.recipientSchedules.length === 0; text: "No vesting positions for this recipient yet." }
                Repeater {
                    model: root.recipientSchedules
                    delegate: Choice {
                        required property var modelData
                        Layout.fillWidth: true
                        label: modelData.id + " · " + modelData.kind
                        detail: modelData.total_display
                        chosen: String(root.recipientId) === String(modelData.id)
                        onPressed: root.recipientId = modelData.id
                    }
                }
                Fact { label: "Schedule kind"; value: root.selected ? root.selected.kind : "" }
                Fact { label: "Beneficiary"; value: root.selected ? Logic.labelled(root.selected.beneficiary, root.names) : "" }
                Fact { label: "Total"; value: root.selected ? root.selected.total_display : "" }
                Fact { label: "Locked"; value: root.selected ? root.selected.locked_display : "" }
                Fact { label: "Claimed"; value: root.selected ? root.selected.claimed_display : "" }
                Fact { label: "Effective time"; value: Logic.dateText(viewTime()) + " (from module)" }
            }

            Panel {
                visible: !root.creatorTab && root.selected !== null
                title: "2. Claim"
                Status {
                    text: "Claimable now: " + (root.selected ? root.selected.claimable_display : "")
                    tone: root.good
                }
                Fact { label: "Next unlock"; value: root.selected ? Logic.nextUnlock(root.selected, viewTime()).text : "" }
                Hint { text: "Send the claim to a public account (fully transparent) or a private account (post-claim movements hidden). Known LNS names are shown beside addresses." }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    ChoiceInline { label: "Public account"; chosen: root.destKind === "public"; onPressed: root.chooseKind("public") }
                    ChoiceInline { label: "Private account"; chosen: root.destKind === "private"; onPressed: root.chooseKind("private") }
                }
                Repeater {
                    model: root.shownAccounts
                    delegate: Choice {
                        required property string modelData
                        Layout.fillWidth: true
                        label: modelData
                        detail: Logic.labelled(modelData, root.names) + (root.destKind === "private" ? " · gas " + Logic.grouped(root.gasOf(modelData)) : "")
                        chosen: root.destAccount === modelData
                        onPressed: root.destAccount = modelData
                    }
                }
                RowLayout {
                    spacing: 12
                    Action {
                        label: root.preparing ? "Preparing…" : root.claiming ? "Claiming…" : "Claim"
                        strong: true
                        enabledNow: !root.preparing && !root.claiming && !root.confirming && root.selected !== null
                        onPressed: root.beginClaim()
                    }
                }
                Bad { text: root.problem }
            }

            // Pre-claim confirmation (the SDK's pre_claim_summary).
            Panel {
                visible: !root.creatorTab && root.confirming
                title: "Confirm claim"
                Fact { label: "Claimable amount"; value: root.confirm ? root.confirm.amountDisplay : "" }
                Fact { label: "Estimated fee"; value: root.confirm ? root.confirm.feeDisplay + " (module)" : "" }
                Fact { label: "Destination"; value: root.confirm ? Logic.labelled(root.confirm.destAccount, root.names) : "" }
                Status {
                    visible: root.confirm && root.confirm.isPrivate
                    text: root.confirm && root.confirm.privacy ? root.confirm.privacy.text : ""
                    tone: root.brass
                }
                Status {
                    visible: root.confirm && root.confirm.gasCheck && root.confirm.gasCheck.ok
                    text: root.confirm && root.confirm.gasCheck ? root.confirm.gasCheck.message : ""
                    tone: root.good
                }
                Bad {
                    visible: root.confirm && (root.confirm.destProblem !== "" || (root.confirm.gasCheck && !root.confirm.gasCheck.ok))
                    text: root.confirm ? (root.confirm.destProblem !== "" ? root.confirm.destProblem : root.confirm.gasCheck.message) : ""
                }
                RowLayout {
                    spacing: 12
                    Action {
                        label: root.claiming ? "Claiming…" : "Confirm claim"
                        strong: true
                        enabledNow: !root.claiming && root.confirm && root.confirm.destProblem === ""
                            && (!root.confirm.gasCheck || root.confirm.gasCheck.ok)
                        onPressed: root.confirmClaim()
                    }
                    Link { label: "Cancel"; onPressed: root.cancelConfirm() }
                }
            }

            // ── Creator view ────────────────────────────────────────────────────────
            Panel {
                visible: root.creatorTab
                title: "Create schedules"
                Hint { text: "Shared settings for all schedules. Dates are yyyy-mm-dd." }
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
                    Field { Layout.fillWidth: true; labelText: "Token"; text: root.cToken; onEdited: root.cToken = text.trim() }
                    Field { Layout.fillWidth: true; labelText: "Start date"; text: root.cStart; onEdited: root.cStart = text.trim() }
                    Field {
                        Layout.fillWidth: true
                        visible: root.cKind === "cliff-linear"
                        labelText: "Cliff date"
                        text: root.cCliff
                        onEdited: root.cCliff = text.trim()
                    }
                    Field { Layout.fillWidth: true; labelText: "End date"; text: root.cEnd; onEdited: root.cEnd = text.trim() }
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
                    ChoiceInline { label: "Batch (one token, one submit)"; chosen: root.batchMode; onPressed: root.batchMode = true }
                }

                // One recipient.
                ColumnLayout {
                    visible: !root.batchMode
                    Layout.fillWidth: true
                    spacing: 10
                    Field { Layout.fillWidth: true; labelText: "Beneficiary (LNS name or account ID)"; text: root.cBeneficiary; code: true; onEdited: root.cBeneficiary = text.trim() }
                    Field { Layout.fillWidth: true; labelText: "Total amount"; text: root.cAmount; code: true; onEdited: root.cAmount = text.trim() }
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
                        enabledNow: !root.creating
                        onPressed: root.doCreate()
                    }
                }

                // Batch: add/remove rows, shared token above, one submit.
                ColumnLayout {
                    visible: root.batchMode
                    Layout.fillWidth: true
                    spacing: 10
                    Hint { text: "One row per recipient. For milestone schedules, put tranche amounts in the tranches field, separated by commas." }
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
                                        labelText: "Recipient " + (index + 1) + " (LNS name or account ID)"
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
                            enabledNow: !root.batchCreating
                            onPressed: root.doBatch()
                        }
                    }
                }
            }

            Panel {
                visible: root.creatorTab
                title: "My schedules"
                Hint { visible: root.mySchedules.length === 0; text: "No schedules yet." }
                Repeater {
                    model: root.mySchedules
                    delegate: Rectangle {
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
                                text: "beneficiary " + Logic.labelled(modelData.beneficiary, root.names)
                                    + " · claimed " + modelData.claimed_display + (modelData.cancelled ? " · CANCELLED" : "")
                                color: root.dim
                                font.family: root.sans
                                font.pixelSize: root.px(14)
                            }
                            RowLayout {
                                spacing: 10
                                Action {
                                    visible: modelData.cancelable && !modelData.cancelled
                                    label: root.busyCancel === String(modelData.id) ? "Cancelling…" : "Cancel"
                                    enabledNow: root.busyCancel !== String(modelData.id)
                                    onPressed: root.doCancel(modelData.id)
                                }
                                Action {
                                    visible: modelData.cancelable && !modelData.cancelled
                                    label: root.busyAction === "irrevocable:" + modelData.id ? "Updating…" : "Make irrevocable"
                                    enabledNow: root.busyAction !== "irrevocable:" + modelData.id
                                    onPressed: root.doIrrevocable(modelData.id)
                                }
                                Repeater {
                                    model: modelData.kind === "milestone" ? modelData.tranches : []
                                    delegate: Action {
                                        property int trancheIndex: index
                                        visible: !modelData.signalled[index] && !modelData.cancelled
                                        label: root.busySignal === String(modelData.id) + ":" + index ? "Signalling…" : "Signal tranche " + (index + 1)
                                        enabledNow: root.busySignal !== String(modelData.id) + ":" + index
                                        onPressed: root.doSignal(modelData.id, trancheIndex)
                                    }
                                }
                            }
                            RowLayout {
                                visible: modelData.transferable && !modelData.cancelled
                                spacing: 10
                                Field {
                                    id: transferField
                                    Layout.fillWidth: true
                                    labelText: "New beneficiary for " + Logic.short(modelData.id) + " (LNS name or account ID)"
                                    code: true
                                }
                                Action {
                                    label: root.busyAction === "transfer:" + modelData.id ? "Transferring…" : "Transfer"
                                    enabledNow: root.busyAction !== "transfer:" + modelData.id
                                    onPressed: root.doTransfer(modelData.id, transferField.text)
                                }
                            }
                        }
                    }
                }
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
            onClicked: action.pressed()
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
                font.family: field.code ? root.mono : root.sans
                font.pixelSize: field.code ? root.px(14) : root.px(16)
                clip: true
                selectByMouse: true
                onEditingFinished: field.edited()
            }
        }
    }
}
