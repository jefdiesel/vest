# Vest SDK for Basecamp apps

`sdk/qml/VestSDK.mjs` is a drop-in for a Basecamp `ui_qml` app that wants to use the vesting
program: list schedules, read what is claimable, and create, claim, cancel, make permanent, unlock
a milestone or transfer a schedule. The app never holds a key: the `vesting` core module builds the
call and a Names Wallet SDK wallet signs it after the person approves in the wallet app.

## Copy these files into your app's qml/ folder, unchanged

- `VestSDK.mjs` (this folder)
- `NamesWalletSDK.mjs` and `NamesWallet.qml` (the Names Wallet SDK, copied from the Names
  repository; Vest keeps its own copies in `miniapp/qml/`)

They are plain ES modules / QML: no build step, no package install. Vest's `Main.qml` uses exactly
this flow in `namesWrite` and `doCreate`.

## Example (about 20 lines)

```qml
import QtQuick
import "NamesWalletSDK.mjs" as Names
import "VestSDK.mjs" as VestSdk

Item {
    NamesWallet { id: wallet; app: "my_app_ui"; bridge: typeof logos !== "undefined" ? logos : null }
    property var vest: VestSdk.createVest({
        call: function(module, method, args, done, timeout) {
            logos.callModuleAsync(module, method, args, done, timeout)
        }
    }, wallet)

    Component.onCompleted: vest.schedules(function(r) {
        if (!r.ok) { console.log(r.error); return }
        for (var i = 0; i < r.schedules.length; i++) console.log(r.schedules[i].id)
    })

    function claim(id) {
        vest.claim(id, "Public/...", function(r) {
            console.log(r.ok ? "Sent " + r.tx : r.code + ": " + r.error)
        })
    }
}
```

Call the wallet's `check()` (and connect when needed) before a write. `create(row, ...)` takes the
same row Vest's `Logic.toCreateRow` builds: creator, beneficiary, token, total, kind, flags and
authorities. Every module argument is sent as text, as Basecamp's call checker requires.

## Calls

All callbacks receive `{ok: true, ...}` or `{ok: false, code, error}`; the SDK never throws.

| Call | Module method | On success |
|---|---|---|
| `schedules(done)` | `list` | `schedules`, `time`, `names` |
| `claimable(id, done)` | `pre_claim_summary` | `summary`, `claimable` (base units as text) |
| `create(row, done)` | `create_call` + `propose_call` | `built`, `status`, `tx` |
| `claim(id, to, done)` | `claim_call` + `propose_call` | `built`, `status`, `tx`; a private `to` is refused with `private_not_in_wallet` |
| `cancel(id, done)` | `cancel_call` + `propose_call` | as above |
| `makePermanent(id, done)` | `irrevocable_call` + `propose_call` | as above |
| `unlockMilestone(id, index, done)` | `signal_call` + `propose_call` | as above |
| `transfer(id, to, done)` | `transfer_call` + `propose_call` | as above |
| `addSchedule(code, done)` | `import_schedule` | `id` (the module's local schedule id) |
| `resolve(nameOrAccount, done)` | the wallet's `resolve` | `name`, `account`, `display`, `forwardedFrom` |

Error codes: `module_missing`, `module_error`, `no_answer`, `no_bridge`, `bad_call`, `no_wallet`,
`private_not_in_wallet`, `wallet_failed`, `wallet_error`, `rejected`, `expired`, `cancelled`,
`wallet_error`, `resolve_failed`.

For a write, the module's built call carries `program`, `accounts_json`, `instruction_hex` and
`summary`; the SDK passes them to `wallet.proposeCall` and reports the request's final state.
`create` does not import the new schedule: call `addSchedule(built.schedule_address, ...)` once the
request is done, as Vest does.

## Tests

`node --test sdk/qml/VestSDK.test.mjs` (17 tests): every call, the private-claim refusal, a wallet
rejection, module errors, and a guard that the file uses no BigInt literal or QML-reserved name.
