# Vest mini-app for Basecamp

Two Basecamp packages:

| Package | Folder | What it is |
|---|---|---|
| `vesting` (core module) | `module/` | The module: a Rust crate (`module/rust-lib`, trait `VestingModule`) over `module/core` (`vesting-module-core`), which calls the SDK's `Client` with `LezBackend`: a LEZ rc3 wallet and RPC. Built with `logos-module-builder`'s `mkLogosModule`. |
| `vesting_ui` (app) | this folder | The view: `qml/Main.qml` and `qml/logic.mjs`, no C++, plus the Names Wallet SDK (`qml/NamesWallet.qml`, `qml/NamesWalletSDK.mjs`). It calls the module with `logos.callModuleAsync("vesting", method, args, …)`. Built with `mkLogosQmlModule`; depends on `vesting`. |

The module's calls (`module/rust-lib/src/lib.rs`; each answers JSON `{"ok":true,…}` or
`{"ok":false,"error":…,"code":…}`):

| Call | SDK / what it does |
|---|---|
| `settings()`, `configure(json)` | The settings (network file, overrides; a wallet folder for the CLI and tests only), the network in use and whether it is connected. `configure` saves them in the module's data folder and reconnects. The app sends only network settings (and `signer: "names"` to switch off a wallet folder an older Vest saved). |
| `accounts()` | The wallet's public (signing) and private accounts, with native balances. |
| `list()`, `now()` | `Client::list()` with the roles the wallet holds in each schedule; the chain's clock. |
| `pre_claim_summary(id, private_target)` | `Client::pre_claim_summary`: claimable now, the **simulated fee** of the very claim (the wallet's dry run), who pays it and whether they can (`fee_covered`). |
| `create(json)`, `create_batch(json)` | `Client::create`, `Client::create_batch` (one transaction). |
| `claim_public(id)`, `claim_private(id, target)` | `Client::claim_public` / `claim_private`. A private target must be a private account the wallet holds (else code 1001), and a claim whose private destination could not pay the simulated fee is not sent. |
| `cancel(id)`, `make_irrevocable(id)`, `signal_milestone(id, index)`, `transfer_beneficiary(id, to)` | The matching `Client` calls. |
| `import_schedule(account)` | Gives a schedule account made elsewhere a local id, so its beneficiary sees it. |
| `resolve_name(name)` | An LNS name through the network's LNS registry (as the CLI resolves it). The app calls it for an address typed in the payout box, and for a name while the wallet isn't connected; with the wallet connected a name is looked up through the wallet instead (below). |
| `set_accounts(json)`, `claim_call(id)` | Names Wallet mode: the accounts Names granted, and a public LEZ claim built as a Names `propose_call` (program, accounts, instruction, summary). |
| `create_call(params)`, `cancel_call(id)`, `irrevocable_call(id)`, `signal_call(id, index)`, `transfer_call(id, new_beneficiary)` | Names Wallet 0.18+ mode: the same change built as a Names `propose_call` (`{program, accounts_json, instruction_hex, summary, caller}`); the person approves it in Names, choosing `caller`. |

The app talks to the person's wallet only through the **Names Wallet SDK** 1.0.0:
`qml/NamesWallet.qml` and `qml/NamesWalletSDK.mjs`, copied unchanged from the Names repository's
`sdk/qml/` (as the kit is copied; `logic.test.mjs` checks their hashes, so a new copy is a
deliberate change). Vest never names a wallet module: the SDK finds the wallet among Basecamp's
installed packages (`getWallet`: Names, or another wallet that declares the SDK; with several,
the person chooses once and Vest remembers it; Settings shows the wallet in use and can change
it). The SDK makes every wallet call (`get_config`, `request_access`, `granted_accounts`,
`account_balances`, `propose_call`, `request_status`, `cancel_request`, `drop_access`,
`resolve`), retries busy ones on a timer each, never sends a proposal twice, says each state in
plain words, and takes the person to the wallet with Basecamp's navigation intent
(`basecamp.apps.launch`, declared in `metadata.json` `uses`); the wallet brings them back once the
request is answered. With the wallet connected, an LNS name typed in the payout box is looked up
through the wallet's own `resolve` (`wallet.resolve`), and the schedule is made for the account
it found. Vest has no signer of its own in Basecamp. Vest keeps only its own words in
`logic.mjs`. The SDK's reference: Names `docs/wallet/PROVIDER-SDK.md`, "Drop-in SDK for apps",
where Vest is the worked example.

The acting account is never chosen by the view: it is the schedule's beneficiary (claim,
transfer), cancel authority (cancel, make irrevocable) or milestone authority (signal), and the
wallet must hold its key. Accounts may be typed as hex, base58 (`Public/…` too) or an LNS name;
the module resolves names and its answer says what each resolved to.

## Network

With no network file, the module uses its built-in profile (`module/core/network/rc3-demo.json`,
format `names-network/1` + v2 keys): the public LEZ rc3 demo chain (`https://lez.nameswallet.app/`,
chain `2da7f7df…5374`), this program's deployment there (`2kgxMAue…eM2` =
`1a0cd691…327d`), the 0.4 token program deployed for it (`FCvRhHFs…zqz`) and the LNS registry
(`6Yxj5k8A…h2gq`). A network file (`names-network/1` or `/2`, LEZ 0.4, wire `rc3`) replaces it;
single fields can be overridden in Settings. Before anything is read or sent the module checks
the RPC's `getChannelId` against the chain id; an RPC other than the network's needs its chain id
unless it is local (`127.0.0.1`).

## Build and test

```sh
node --test miniapp/logic.test.mjs                 # the view's logic (121 tests)
cargo test -p vesting-module-core                  # every module call on the JSON test ledger (13 tests)
cargo build -p vesting-module-core --features lez  # the chain build of the module's core
```

The Basecamp packages build with nix (the Mac mini; not on this laptop): stage the two folders,
then build each with the pinned builder:

```sh
miniapp/module/stage.sh /tmp/vest-stage
(cd /tmp/vest-stage/module && nix build .#lgx-portable)
(cd /tmp/vest-stage/app && nix build .#lgx-portable --override-input vesting path:/tmp/vest-stage/module)
```

`stage.sh` copies the repository crates the module needs into `module/rust-lib/vest/` (the
builder compiles only `rust-lib/`); `module/rust-lib/Cargo.lock` is the workspace's lock plus the
module's own two crates. Loading and clicking through: [`docs/BASECAMP-TEST.md`](../docs/BASECAMP-TEST.md).

`tools/loadcheck.py <plugin.dylib> <Basecamp Frameworks dir>` checks a built module plugin
loads: it lists every undefined symbol nothing provides (the plugin links with
`-undefined dynamic_lookup`, so a missing library only shows at load) and dlopens it.

`tools/render.py` renders `qml/Main.qml` offscreen at 1250x700 with a stand-in vesting module,
wallet and package list (27 scenarios: first run, connecting, Names missing or on another
network, no wallet installed, a wallet that speaks another SDK version, two wallets to choose
  from and the choice in Settings, the guides,
no schedules, a schedule, a claim waiting in Names and its result, create, the payout box's
lookup of an address and of a name through Names, the cancel confirmation, settings, an old
wallet-folder setting) and fails on any QML error.

The screen uses the shared kit in `qml/kit/` (Look, Action, PageHeader, Field, FactRow,
TechDetails, Tick, Link, RunsOn, BackButton), copied unchanged from the Program Registry browser
(jefdiesel/lez-program-registry `guide-ui`, `registry_browser/qml/kit/`): one left-aligned column
at most 680 px wide, 56 px from the window's edge, as in the Names Wallet.

## Known limits

- The app signs only through the wallet (the Names Wallet SDK). What the wallet can't approve
  yet is not offered: batch creates, token schedules and private claims (the module's
  `create_batch`, `claim_private` and wallet-folder calls stay for the CLI and its tests; the app
  never calls them).
- The module answers one call at a time.
- Schedule ids are local to the module's data folder; a beneficiary on another machine imports
  the schedule account (Recipient tab, Import).
- Display maths use the module's chain time and match `vesting-core` exactly (fixed cases from
  `core/tests/unit.rs` in the Node tests; a decimal-limb fallback where QML has no BigInt).
- Only the beneficiary address is public at creation (as the RFP specifies); commitment-based
  anonymous beneficiaries are not offered.
