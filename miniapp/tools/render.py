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
            "settings": {"wallet_dir": "~/lez-wallet", "network_file": "", "rpc": "", "chain_id": "", "program": "", "token_program": "", "lns_registry": "", "clock_elf": ""},
            "network": {"name": "Bridge test network (LEZ rc3 demo chain)", "source": "built-in", "rpc": "https://lez.nameswallet.app/",
                        "chain_id": "2da7f7dfb6e51a28c942ea80391aa518253ec515e98d28cb4d83cc9e16ff5374",
                        "program": "2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2", "token_program": "FCvRhHFsraY3pBKajWtvC1QCsvXw7MCeYBbNgMnjNzqz",
                        "lns_registry": "6Yxj5k8AiuBSuMAYVQFGrYKjJGUSSaWWgoB9D9XXN3gA"}}
ACCOUNTS = {"ok": True, "accounts": [
    {"id": ME, "base58": "2BhKj8RUHy3bhKXGrxpYy1sZgVNXfcG3kGSbZ1EZfGrD", "private": False, "balance": "28431410", "balance_display": "0.02843141 LEZ", "name": "alice"},
    {"id": PRIV, "base58": "3DyBfLzS7f9yJeKHsQ2FoLxNbyAvKMq4QEo1WdLm8vZR", "private": True, "balance": "0", "balance_display": "0 LEZ", "name": None}]}


def summary(covered, payer, private):
    return {"ok": True, "summary": {"schedule": 0, "claimable": "486", "claimable_display": "486 LEZ", "estimated_fee": "11904",
            "estimated_fee_display": "0.000011904 LEZ", "fee_simulated": True, "fee_payer": payer, "fee_reserve": "23808",
            "fee_reserve_display": "0.000023808 LEZ", "fee_available": "486" if not covered else "28431410",
            "fee_available_display": "0.000000486 LEZ" if not covered else "0.02843141 LEZ", "fee_covered": covered,
            "revert_reason": None, "next_unlock": NOW + 330 * DAY, "destination_kind": "private" if private else "public"}}


FIRST = dict(SETTINGS, connected=False, status="not opened yet", settings=dict(SETTINGS["settings"], wallet_dir=""))
WRONG = dict(SETTINGS, connected=False, status="no wallet at ~/lez-wallet")

SCENARIOS = {
    "first-run": ({"settings": FIRST}, ""),
    "recipient-empty": ({"list": {"ok": True, "time": NOW, "names": {}, "schedules": []}}, ""),
    "recipient": ({}, ""),
    "confirm-public": ({"pre_claim_summary": summary(True, "caller", False)}, "root.beginClaim()"),
    "confirm-private": ({"pre_claim_summary": summary(True, "claim_target", True)}, "root.chooseKind('private'); root.beginClaim()"),
    "confirm-private-short": ({"pre_claim_summary": summary(False, "claim_target", True)}, "root.chooseKind('private'); root.beginClaim()"),
    "creator": ({}, "root.tab = 'creator'"),
    "confirm-cancel": ({}, "root.tab = 'creator'; root.askGrave('cancel', root.mySchedules[0])"),
    "settings": ({}, "root.tab = 'settings'"),
    "not-connected": ({"settings": WRONG}, ""),
    "no-module": ({"__missing": True}, ""),
}

BRIDGE = r"""
    width: 1250; height: %(height)s
    property var logos: QtObject {
        function callModuleAsync(module, method, args, callback, timeout) {
            var reply = callModule(module, method, args)
            Qt.callLater(function() { callback(reply) })
        }
        function callModule(module, method, args) {
            var answers = %(answers)s
            if (answers.__missing) return "Module not connected: " + module
            if (module !== "vesting") return ""
            var a = answers[method]
            // Basecamp may hand the module's JSON text encoded once more.
            return a === undefined ? JSON.stringify(JSON.stringify({ ok: false, error: "no answer in this scenario" })) : JSON.stringify(JSON.stringify(a))
        }
    }
    Timer { interval: 300; running: true; onTriggered: { %(scenario)s; shot.start() } }
    Timer { id: shot; interval: 600; onTriggered: root.grabToImage(function(result) { result.saveToFile("%(out)s"); Qt.quit() }) }
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
        extra, script = SCENARIOS[name]
        answers = {"settings": SETTINGS, "accounts": ACCOUNTS, "list": LIST, "now": {"ok": True, "time": NOW}}
        answers.update(extra)
        out = os.path.join(out_dir, name + ".png")
        bridge = BRIDGE % {"answers": json.dumps(answers), "scenario": script or "0", "out": out, "height": int(os.environ.get("RENDER_HEIGHT", "700"))}
        with tempfile.NamedTemporaryFile("w", suffix=".qml", delete=False, dir=os.path.join(HERE, "..", "qml")) as handle:
            handle.write(source.replace(marker, marker + bridge, 1))
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
