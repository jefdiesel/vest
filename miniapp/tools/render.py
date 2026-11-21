#!/usr/bin/env python3
"""Renders qml/Main.qml to PNG files without Basecamp, with a stand-in for the vesting module.

Usage: render.py <qml binary> <output dir> [scenario ...]
Needs QML_IMPORT_PATH (Qt 6 QML modules plus Logos.Theme) and QT_PLUGIN_PATH set, and
QT_QPA_PLATFORM=offscreen to run without a display. The stand-in answers the module's calls with
the shapes vesting-module-core gives (see miniapp/module/core/src/lib.rs); a scenario then
presses through the view. Any QML error is printed under the scenario's name.
"""
import json, os, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ME = "11" * 32
PRIV = "22" * 32
OTHER = "33" * 32
NOW = 1795200000000
DAY = 86400000


def lez(units):
    whole, frac = divmod(int(units), 10**9)
    text = f"{whole:,}" + ("." + f"{frac:09d}".rstrip("0") if frac else "")
    return text + " LEZ"


def schedule(id, kind, total, claimed, claimable, mine, **extra):
    locked = str(int(total) - int(claimed) - int(claimable))
    row = {"id": id, "kind": kind, "creator": ME, "beneficiary": ME, "token": "00" * 32, "total": total,
           "total_display": lez(total), "claimed": claimed, "claimed_display": lez(claimed),
           "claimable": claimable, "claimable_display": lez(claimable), "locked": locked, "locked_display": lez(locked),
           "signalled": [], "cancelable": True, "transferable": True, "cancel_authority": ME, "milestone_authority": ME,
           "cancelled": False, "address": "44" * 32, "mine": mine}
    row.update(extra)
    return row


LIST = {"ok": True, "time": NOW, "names": {ME: "alice"}, "schedules": [
    schedule(0, {"type": "linear", "start": NOW - 30 * DAY, "end": NOW + 330 * DAY}, "1000000", "52000", "31333",
             {"beneficiary": True, "creator": True, "cancel_authority": True, "milestone_authority": True}),
    schedule(1, {"type": "milestone", "tranches": ["400000", "600000"]}, "1000000", "0", "400000",
             {"beneficiary": False, "creator": True, "cancel_authority": True, "milestone_authority": True},
             beneficiary=OTHER, signalled=[True, False]),
]}
SETTINGS = {"ok": True, "connected": True, "status": "ready", "chain": True, "has_password": False, "data_dir": "~/Library/Application Support/Logos/vesting",
            "settings": {"network_file": "", "rpc": "", "chain_id": "", "program": "", "token_program": "", "lns_registry": "", "clock_elf": ""},
            "network": {"name": "Bridge test network", "source": "built-in", "rpc": "https://lez.nameswallet.app/",
                        "chain_id": "2da7f7dfb6e51a28c942ea80391aa518253ec515e98d28cb4d83cc9e16ff5374",
                        "program": "2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2", "token_program": "FCvRhHFsraY3pBKajWtvC1QCsvXw7MCeYBbNgMnjNzqz",
                        "lns_registry": "6Yxj5k8AiuBSuMAYVQFGrYKjJGUSSaWWgoB9D9XXN3gA"}}
ACCOUNTS = {"ok": True, "accounts": [
    {"id": ME, "base58": "2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD", "private": False, "balance": "28431410", "balance_display": "0.02843141 LEZ", "name": "alice"},
    {"id": PRIV, "base58": "3DyBfLzS7f9yJeKHsQ2FoLxNbyAvKMq4QEo1WdLm8vZR", "private": True, "balance": "0", "balance_display": "0 LEZ", "name": None}]}


CANCELLED = {"ok": True, "time": NOW, "names": {ME: "alice"}, "schedules": [
    schedule(0, {"type": "linear", "start": NOW - 2 * DAY, "end": NOW + DAY}, "1000000", "652652", "231",
             {"beneficiary": True, "creator": True, "cancel_authority": True, "milestone_authority": True},
             cancelled=True, cancelled_at=NOW - 3600000, vested_at_cancel="652883", refund="347117", refund_display=lez("347117"),
             locked="347117", locked_display=lez("347117"))]}
CLAIMED = {"ok": True, "time": NOW, "names": {ME: "alice"}, "schedules": [
    schedule(0, {"type": "linear", "start": NOW - 9 * DAY, "end": NOW - DAY}, "1000000", "1000000", "0",
             {"beneficiary": True, "creator": True, "cancel_authority": True, "milestone_authority": True})]}
FIRST = dict(SETTINGS, connected=False, status="not opened yet", signer="names")
NAMES_SETTINGS = dict(SETTINGS, signer="names")
# A wallet-folder signer saved by an older Vest: switched off at once (configure signer names).
OLD_WALLET = dict(SETTINGS, signer="wallet")
CHAIN = SETTINGS["network"]["chain_id"]
CONFIG = {"zone": {"id": "home", "channel": CHAIN, "home": True}, "features": ["request_access_with"]}
GRANTED = {"connected": True, "accounts": [{"account": "Public/2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD", "account_kind": "public"}]}
NOT_GRANTED = {"connected": False, "accounts": []}
WALLET_ON = {"get_config": dict(CONFIG, features=["call_reads_clock"]), "granted_accounts": GRANTED, "account_balances": []}
DONE_GUIDE = dict(NAMES_SETTINGS, settings=dict(SETTINGS["settings"], guide_done=["2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD"]))
NAMES_ROW = {"name": "names", "moduleName": "names", "type": "core", "category": "wallet", "displayName": "Names", "version": "0.19.0"}
CLAIM_CALL = {"ok": True, "program": "2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2", "accounts_json": "[]", "instruction_hex": "05",
              "amount": "31333", "amount_display": lez("31333"), "summary": "Claim", "caller": "Public/2BhK"}

# Schedules arriving by themselves: two to claim (one from bob), one still unvested, one finished.
BOB = "55" * 32
INCOMING = {"ok": True, "time": NOW, "names": {ME: "alice", BOB: "bob"}, "schedules": [
    schedule(0, {"type": "linear", "start": NOW - 30 * DAY, "end": NOW + 330 * DAY}, "1000000", "52000", "31333",
             {"beneficiary": True, "creator": True, "cancel_authority": True, "milestone_authority": True}),
    schedule(2, {"type": "cliff-linear", "start": NOW - 60 * DAY, "cliff": NOW - DAY, "end": NOW + 300 * DAY}, "5000000000000", "0", "833333333333",
             {"beneficiary": True, "creator": False, "cancel_authority": False, "milestone_authority": False}, creator=BOB, cancel_authority=BOB, milestone_authority=BOB),
    schedule(3, {"type": "linear", "start": NOW + 9 * DAY, "end": NOW + 374 * DAY}, "12000000000000", "0", "0",
             {"beneficiary": True, "creator": False, "cancel_authority": False, "milestone_authority": False}, creator=BOB, cancel_authority=BOB, milestone_authority=BOB,
             next_unlock=NOW + 9 * DAY + 1, address="66" * 32),
    schedule(4, {"type": "linear", "start": NOW - 90 * DAY, "end": NOW - 10 * DAY}, "250000000000", "250000000000", "0",
             {"beneficiary": True, "creator": False, "cancel_authority": False, "milestone_authority": False}, creator=BOB, cancel_authority=BOB, milestone_authority=BOB),
    schedule(1, {"type": "milestone", "tranches": ["400000", "600000"]}, "1000000", "0", "400000",
             {"beneficiary": False, "creator": True, "cancel_authority": True, "milestone_authority": True},
             beneficiary=OTHER, signalled=[True, False]),
]}
SCAN_DONE = {"ok": True, "chain_id": SETTINGS["network"]["chain_id"], "from_block": 45884, "to_block": 45883, "tip": 45883, "done": True, "found": [], "new": [], "incoming": 4, "known": 20, "pending": 0}
SCAN_NEW = dict(SCAN_DONE, found=["7Ue2Wx…"], new=[{"id": 3, "address": "66" * 32, "creator": BOB}])
# The app calls discover again until it is done: three answers not done, then the one that finds
# a schedule. Only the fourth answer names bob, so the line shows only if the loop went on.
SCAN_LOOP = [dict(SCAN_DONE, done=False, to_block=b, from_block=b - 1999) for b in (40699, 42699, 44699)] + [SCAN_NEW]

# Each scenario: (vesting answers, names answers, script[, late script]). The late script runs
# after the app's first reads and its first scan have answered. Names answers are values (wrapped
# in the Provider SDK's envelope by the bridge).
SCENARIOS = {
    "first-run": ({"settings": FIRST}, {"get_config": CONFIG, "granted_accounts": NOT_GRANTED}, ""),
    "connect-pending": ({"settings": FIRST}, {"get_config": CONFIG, "granted_accounts": NOT_GRANTED,
                        "request_access": {"request": "ab" * 16, "kind": "connect", "state": "pending"}, "request_status": {"request": "ab" * 16, "kind": "connect", "state": "pending"}}, "root.connectNames()"),
    "names-missing": ({"settings": FIRST}, {"__missing": True}, ""),
    "other-zone": ({"settings": FIRST}, {"get_config": {"zone": {"id": "home", "channel": "01" * 32, "home": True}}}, ""),
    "guide-receive": ({"settings": NAMES_SETTINGS}, {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": []}, ""),
    "guide-pay": ({"settings": NAMES_SETTINGS}, {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": []}, "root.guideRole = 'pay'"),
    "recipient-empty": ({"settings": NAMES_SETTINGS, "list": {"ok": True, "time": NOW, "names": {}, "schedules": []}},
                        {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": []}, "root.guideOpen = false; root.finishGuide()"),
    "recipient": ({"settings": dict(NAMES_SETTINGS, settings=dict(SETTINGS["settings"], guide_done=["2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD"]))},
                  {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": []}, ""),
    "claim-confirm": ({"settings": dict(NAMES_SETTINGS, settings=dict(SETTINGS["settings"], guide_done=["2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD"])), "claim_call": CLAIM_CALL},
                      {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": []}, "root.beginClaim(); scroll.start()"),
    "claim-approval-pending": ({"settings": dict(NAMES_SETTINGS, settings=dict(SETTINGS["settings"], guide_done=["2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD"])), "claim_call": CLAIM_CALL},
                               {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": [],
                                "propose_call": {"request": "cd" * 16, "kind": "call", "state": "pending"},
                                "request_status": {"request": "cd" * 16, "kind": "call", "state": "pending"}},
                               "root.beginClaim(); later.start(); scroll.start()"),
    "claim-result": ({"settings": dict(NAMES_SETTINGS, settings=dict(SETTINGS["settings"], guide_done=["2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD"])), "claim_call": CLAIM_CALL},
                     {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": [],
                      "propose_call": {"request": "cd" * 16, "kind": "call", "state": "pending"},
                      "request_status": {"request": "cd" * 16, "kind": "call", "state": "done", "tx": "c5d782913d8819b009c8d15815697b20a59b71f2568d2cd976165af267b7b8fd"}},
                     "root.beginClaim(); later.start(); scroll.start()"),
    "recipient-cancelled": ({"settings": DONE_GUIDE, "list": CANCELLED}, WALLET_ON, "root.guideOpen = false"),
    "create-linear": ({"settings": DONE_GUIDE}, WALLET_ON, "root.tab = 'pay'; root.cKind = 'linear'; page.contentY = 360"),
    "create-cliff": ({"settings": DONE_GUIDE}, WALLET_ON, "root.tab = 'pay'; root.cKind = 'cliff-linear'; page.contentY = 360"),
    "create-milestone": ({"settings": DONE_GUIDE}, WALLET_ON, "root.tab = 'pay'; root.cKind = 'milestone'; page.contentY = 360"),
    "payee-address": ({"settings": DONE_GUIDE, "resolve_name": {"ok": True, "account": "11" * 32, "base58": "2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD"}}, WALLET_ON,
                 "root.tab = 'pay'; beneficiaryField.text = '2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD'; root.checkPayee(); page.contentY = 560"),
    "payee-names": ({"settings": NAMES_SETTINGS}, {"get_config": dict(CONFIG, features=["call_reads_clock"]), "granted_accounts": GRANTED, "account_balances": [],
                     "resolve": {"name": "alice", "display": "alice.lez", "account": "2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD", "forward": None, "lapsed": False}},
                    "root.guideOpen = false; root.tab = 'pay'; beneficiaryField.text = 'alice'; root.checkPayee(); page.contentY = 560"),
    "payee-names-unknown": ({"settings": NAMES_SETTINGS}, {"get_config": dict(CONFIG, features=["call_reads_clock"]), "granted_accounts": GRANTED, "account_balances": [], "resolve": None},
                    "root.guideOpen = false; root.tab = 'pay'; beneficiaryField.text = 'alcie'; root.checkPayee(); page.contentY = 560"),
    "create-names-018": ({"settings": NAMES_SETTINGS}, {"get_config": dict(CONFIG, features=["call_reads_clock"]), "granted_accounts": GRANTED, "account_balances": []},
                         "root.guideOpen = false; root.tab = 'pay'; page.contentY = page.contentHeight - page.height"),
    "receipt": ({"settings": DONE_GUIDE}, WALLET_ON,
                "root.tab = 'pay'; root.receipt = { id: 3, amountDisplay: '1 LEZ', kind: 'linear', start: 1795251600000, cliff: -1, end: 1795338000000, cancelable: true, transferable: false, fromAddr: 'BawxwquWtPEVcrwSFftD6jS3XFxZNv4qYrV8pkyo9mu7', fromName: 'jefdiesel', toAddr: 'CCVd76Tk3WvFb63Wbuu4exMeLRTbSuMWHqxuvysapKnk', toName: 'jef04', tx: '1678b0' + 'ab'.repeat(29) + '131d', code: 'c798db' + 'cd'.repeat(13) + '09bc' }"),
    "picker-open": ({"settings": DONE_GUIDE}, WALLET_ON,
                "root.tab = 'pay'; root.cKind = 'linear'; startField.show(); page.contentY = 560"),
    "picker-cliff": ({"settings": DONE_GUIDE}, WALLET_ON,
                "root.tab = 'pay'; root.cKind = 'cliff-linear'; endField.show(); page.contentY = 560"),
    "picker-done": ({"settings": DONE_GUIDE}, WALLET_ON,
                "root.tab = 'pay'; root.cKind = 'linear'; startField.show(); startField.done(); endField.show(); endField.shiftMonth(1); endField.done(); page.contentY = 560"),
    "create-names": ({"settings": NAMES_SETTINGS}, {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": []}, "root.tab = 'pay'"),
    "confirm-cancel": ({"settings": DONE_GUIDE}, WALLET_ON, "root.tab = 'pay'; root.askGrave('cancel', root.mySchedules[0])"),
    "settings": ({"settings": NAMES_SETTINGS}, {"get_config": CONFIG, "granted_accounts": GRANTED, "account_balances": []}, "root.tab = 'settings'"),
    "old-wallet-folder-setting": ({"settings": OLD_WALLET}, WALLET_ON, ""),
    "no-wallet": ({"settings": FIRST}, {"__packages": [{"name": "vesting", "type": "core", "category": "finance"}]}, ""),
    "choose-wallet": ({"settings": FIRST}, dict(WALLET_ON, __packages=[NAMES_ROW, {"name": "xwallet", "type": "core", "category": "wallet", "displayName": "X Wallet"}]), ""),
    "unsupported-wallet": ({"settings": FIRST}, {"get_config": {"zone": CONFIG["zone"], "features": []}, "__packages": [{"name": "oldwallet", "type": "core", "category": "wallet", "displayName": "Old Wallet"}]}, ""),
    "menu-badges": ({"settings": DONE_GUIDE, "list": INCOMING}, WALLET_ON, ""),
    "claimable-detail": ({"settings": DONE_GUIDE, "list": INCOMING}, WALLET_ON, "", "root.recipientId = 2"),
    "unvested-list": ({"settings": DONE_GUIDE, "list": INCOMING}, WALLET_ON, "", "root.tab = 'unvested'"),
    "unvested-detail": ({"settings": DONE_GUIDE, "list": INCOMING}, WALLET_ON, "", "root.tab = 'unvested'; root.recipientId = 3; page.contentY = 300"),
    "unvested-add-by-code": ({"settings": DONE_GUIDE, "list": INCOMING}, WALLET_ON, "", "root.tab = 'unvested'; page.contentY = page.contentHeight - page.height"),
    "unvested-empty": ({"settings": DONE_GUIDE, "list": {"ok": True, "time": NOW, "names": {}, "schedules": []}}, WALLET_ON, "", "root.tab = 'unvested'"),
    "scanning": ({"settings": DONE_GUIDE, "list": INCOMING}, WALLET_ON, "", "root.scan = { ok: true, from_block: 38700, to_block: 40699, tip: 45883, done: false }"),
    "new-schedule": ({"settings": DONE_GUIDE, "list": INCOMING, "discover": SCAN_NEW}, WALLET_ON, ""),
    "scan-loop": ({"settings": DONE_GUIDE, "list": INCOMING, "__discover_seq": SCAN_LOOP}, WALLET_ON, ""),
    "scan-problem": ({"settings": DONE_GUIDE, "list": INCOMING, "discover": {"ok": False, "error": "The network is busy (rate limited); try again in a minute"}}, WALLET_ON, ""),
    "narrow": ({"settings": DONE_GUIDE, "list": INCOMING, "discover": SCAN_NEW}, WALLET_ON, "root.width = 560"),
    "narrow-unvested": ({"settings": DONE_GUIDE, "list": INCOMING}, WALLET_ON, "root.width = 560", "root.tab = 'unvested'; root.recipientId = 3"),
    "chose-other-wallet": ({"settings": DONE_GUIDE}, dict(WALLET_ON, __packages=[NAMES_ROW, {"name": "xwallet", "type": "core", "category": "wallet", "displayName": "X Wallet"}]),
                           "wallet.chooseWallet('xwallet'); root.tab = 'settings'"),
}

BRIDGE = r"""
    width: 1250; height: %(height)s
    property int renderDiscoverCalls: 0
    property var logos: QtObject {
        function callModuleAsync(module, method, args, callback, timeout) {
            var reply = callModule(module, method, args)
            Qt.callLater(function() { callback(reply) })
        }
        function callModule(module, method, args) {
            var answers = %(answers)s
            var na = %(names)s
            if (module === "package_manager")
                return na.__packages ? JSON.stringify(na.__packages) : ""
            if (module === "names" || module === "xwallet" || module === "oldwallet") {
                if (na.__missing) return "Module not connected: " + module
                var v = na[method]
                if (module === "xwallet" && method === "get_config" && v) v = Object.assign({}, v, { wallet_sdk: { name: "names-wallet", version: "1.0", app: "xwallet_ui" } })
                return JSON.stringify(JSON.stringify(v === undefined ? { success: false, value: null, error: "not in this scenario" } : { success: true, value: v, error: null }))
            }
            if (module !== "vesting") return ""
            if (method === "discover" && answers.__discover_seq) {
                var seq = answers.__discover_seq
                return JSON.stringify(JSON.stringify(seq[Math.min(root.renderDiscoverCalls++, seq.length - 1)]))
            }
            var a = answers[method]
            // Basecamp may hand the module's JSON text encoded once more.
            return a === undefined ? JSON.stringify(JSON.stringify({ ok: false, error: "no answer in this scenario" })) : JSON.stringify(JSON.stringify(a))
        }
    }
    Timer { id: later; interval: 400; onTriggered: root.confirmClaim() }
    Timer { id: scroll; interval: 2600; onTriggered: page.contentY = Math.max(0, page.contentHeight - page.height) }
    Timer { interval: 300; running: true; onTriggered: { %(scenario)s; shot.start(); lateShot.start() } }
    Timer { id: lateShot; interval: 1400; onTriggered: { %(late)s } }
    Timer { id: shot; interval: 2900; onTriggered: root.grabToImage(function(result) { result.saveToFile("%(out)s"); Qt.quit() }) }
"""


def main():
    qml, out_dir = sys.argv[1], sys.argv[2]
    wanted = sys.argv[3:] or list(SCENARIOS)
    source = open(os.path.join(HERE, "..", "qml", "Main.qml")).read()
    marker = "Item {\n    id: root\n"
    assert marker in source, "Main.qml no longer starts the way the harness expects"
    os.makedirs(out_dir, exist_ok=True)
    failed = 0
    for name in wanted:
        extra, names, script = SCENARIOS[name][:3]
        late = SCENARIOS[name][3] if len(SCENARIOS[name]) > 3 else ""
        answers = {"settings": SETTINGS, "accounts": ACCOUNTS, "list": LIST, "now": {"ok": True, "time": NOW}, "set_accounts": {"ok": True},
                   "configure": {"ok": True}, "discover": SCAN_DONE}
        answers.update(extra)
        out = os.path.join(out_dir, name + ".png")
        bridge = BRIDGE % {"answers": json.dumps(answers), "names": json.dumps(names), "scenario": script or "0", "late": late or "0", "out": out, "height": int(os.environ.get("RENDER_HEIGHT", "700"))}
        with tempfile.NamedTemporaryFile("w", suffix=".qml", delete=False, dir=os.path.join(HERE, "..", "qml")) as handle:
            # The wallet a scenario chose is kept (QtQuick.LocalStorage) under the app's name: a
            # name of its own per run, so it never leaks into the next scenario or run.
            own = source.replace('app: "vesting_ui"', 'app: "render-%s-%d"' % (name, os.getpid()), 1)
            assert own != source, "Main.qml no longer names its app for the wallet"
            handle.write(own.replace(marker, marker + bridge, 1))
        try:
            result = subprocess.run([qml, handle.name], capture_output=True, text=True, timeout=60)
        except subprocess.TimeoutExpired:
            print(name, "FAILED: did not quit within 60 s")
            failed += 1
            continue
        finally:
            os.unlink(handle.name)
        problems = [l for l in result.stderr.splitlines() if l.strip() and "qt.qpa.fonts" not in l and "propagateSizeHints" not in l]
        ok = os.path.exists(out) and not problems
        failed += 0 if ok else 1
        print(name, "ok" if ok else "FAILED", *problems[:8], sep="\n  " if problems else " ")
    sys.exit(1 if failed else 0)


main()
