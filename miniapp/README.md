# Vest mini-app for Basecamp

Two Basecamp packages:

| Package | Folder | What it is |
|---|---|---|
| `vesting` (core module) | `module/` | The module: a Rust crate (`module/rust-lib`, trait `VestingModule`) over `module/core` (`vesting-module-core`), which calls the SDK's `Client` with `LezBackend`: a LEZ rc3 wallet and RPC. Built with `logos-module-builder`'s `mkLogosModule`. |
| `vesting_ui` (app) | this folder | The view: `qml/Main.qml` and `qml/logic.mjs`, no C++. It calls the module with `logos.callModuleAsync("vesting", method, args, …)`. Built with `mkLogosQmlModule`; depends on `vesting`. |

The module's calls (`module/rust-lib/src/lib.rs`; each answers JSON `{"ok":true,…}` or
`{"ok":false,"error":…,"code":…}`):

| Call | SDK / what it does |
|---|---|
| `settings()`, `configure(json)` | The settings (wallet folder, network file, overrides), the network in use and whether it is connected. `configure` saves them in the module's data folder and reconnects; the wallet `password` is kept in memory only. |
| `accounts()` | The wallet's public (signing) and private accounts, with native balances. |
| `list()`, `now()` | `Client::list()` with the roles the wallet holds in each schedule; the chain's clock. |
| `pre_claim_summary(id, private_target)` | `Client::pre_claim_summary`: claimable now, the **simulated fee** of the very claim (the wallet's dry run), who pays it and whether they can (`fee_covered`). |
| `create(json)`, `create_batch(json)` | `Client::create`, `Client::create_batch` (one transaction). |
| `claim_public(id)`, `claim_private(id, target)` | `Client::claim_public` / `claim_private`. A private target must be a private account the wallet holds (else code 1001), and a claim whose private destination could not pay the simulated fee is not sent. |
| `cancel(id)`, `make_irrevocable(id)`, `signal_milestone(id, index)`, `transfer_beneficiary(id, to)` | The matching `Client` calls. |
| `import_schedule(account)` | Gives a schedule account made elsewhere a local id, so its beneficiary sees it. |
| `resolve_name(name)` | An LNS name through the network's LNS registry (as the CLI resolves it). |

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
node --test miniapp/logic.test.mjs                 # the view's logic (75 tests)
cargo test -p vesting-module-core                  # every module call on the JSON test ledger (12 tests)
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

`tools/render.py` renders `qml/Main.qml` offscreen with a stand-in module (8 scenarios: positions,
public and private confirmations, a fee the private account can't pay, creator, settings, not
connected, module missing) and fails on any QML error.

## Known limits

- The module answers one call at a time. A private claim proves on the machine (minutes, a real
  proof on the public chain); other calls wait for it.
- Schedule ids are local to the module's data folder; a beneficiary on another machine imports
  the schedule account (Recipient tab, Import).
- Display maths use the module's chain time and match `vesting-core` exactly (fixed cases from
  `core/tests/unit.rs` in the Node tests; a decimal-limb fallback where QML has no BigInt).
- Only the beneficiary address is public at creation (as the RFP specifies); commitment-based
  anonymous beneficiaries are not offered.
