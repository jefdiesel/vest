//! The Vesting core module for Basecamp (Logos RFP-017).
//!
//! Rust-first authoring: the trait below is the module's contract; the builder derives the
//! `.lidl` from it, generates the C-ABI scaffold around it and compiles this crate to a
//! staticlib. Every method is one call of `vesting_module_core::Module` (miniapp/module/core,
//! where it is tested) and answers JSON text: `{"ok":true,...}` or `{"ok":false,"error":...}`.
//!
//! Apps call it as `logos.callModuleAsync("vesting", "<method>", [args], callback, timeout)`.
//! Writes wait for the chain (a private claim proves on this machine: minutes), so give them a
//! long timeout. Settings, the schedule id map and the name book live in the module's
//! persistence folder.

/// The vesting module's calls. Every parameter is text: ids and tranche indexes are the module's
/// local numbers written in decimal (Basecamp's call checker refuses a QML number for an integer
/// parameter); accounts are hex, base58 or LNS names; amounts are decimal strings.
pub trait VestingModule: Send + 'static {
    /// Settings (no password), the network in use, and whether the chain is connected.
    fn settings(&mut self) -> String;
    /// Change settings (JSON object; `password` is kept in memory only) and reconnect.
    fn configure(&mut self, settings: String) -> String;
    /// The wallet's accounts and native balances.
    fn accounts(&mut self) -> String;
    /// Every known schedule, with the roles the wallet holds in each.
    fn list(&mut self) -> String;
    /// The chain's time (ms), which all display maths use.
    fn now(&mut self) -> String;
    /// Claimable now, the simulated fee and the fee check. Empty target: a public claim.
    fn pre_claim_summary(&mut self, id: String, private_target: String) -> String;
    /// One schedule (JSON object).
    fn create(&mut self, params: String) -> String;
    /// Several schedules of one creator in one transaction (JSON array).
    fn create_batch(&mut self, params: String) -> String;
    /// Claim everything vested to the beneficiary's public account.
    fn claim_public(&mut self, id: String) -> String;
    /// Claim everything vested to a private account the wallet holds.
    fn claim_private(&mut self, id: String, target: String) -> String;
    /// Cancel (the cancel authority signs); unvested goes back to the creator.
    fn cancel(&mut self, id: String) -> String;
    /// Make a cancelable schedule irrevocable (one way).
    fn make_irrevocable(&mut self, id: String) -> String;
    /// Signal milestone tranche `index` (the milestone authority signs).
    fn signal_milestone(&mut self, id: String, index: String) -> String;
    /// Move a transferable position to another beneficiary.
    fn transfer_beneficiary(&mut self, id: String, new_beneficiary: String) -> String;
    /// Give a schedule account made elsewhere a local id.
    fn import_schedule(&mut self, account: String) -> String;
    /// Resolve an LNS name (or check an address).
    fn resolve_name(&mut self, name: String) -> String;

    /// The host's context: the persistence folder for settings and the id map.
    fn on_context_ready(&mut self, _ctx: &RustModuleContext) {}
}

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/generated/provider_gen.rs"
));

use vesting_module_core::{bad_input, parse_index, Module};

#[derive(Default)]
struct Vesting {
    module: Option<Module>,
}

impl Vesting {
    fn m(&mut self) -> &mut Module {
        self.module.get_or_insert_with(|| {
            // Before the host stamps a context (never in Basecamp): a folder of our own.
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            let dir = std::path::Path::new(&home).join(".config/logos/vesting");
            let _ = std::fs::create_dir_all(&dir);
            Module::new(&dir)
        })
    }
}

impl VestingModule for Vesting {
    fn settings(&mut self) -> String {
        self.m().settings()
    }
    fn configure(&mut self, settings: String) -> String {
        self.m().configure(&settings)
    }
    fn accounts(&mut self) -> String {
        self.m().accounts()
    }
    fn list(&mut self) -> String {
        self.m().list()
    }
    fn now(&mut self) -> String {
        self.m().now()
    }
    fn pre_claim_summary(&mut self, id: String, private_target: String) -> String {
        match parse_index("id", &id) {
            Ok(id) => self.m().pre_claim_summary(id, &private_target),
            Err(error) => bad_input(error),
        }
    }
    fn create(&mut self, params: String) -> String {
        self.m().create(&params)
    }
    fn create_batch(&mut self, params: String) -> String {
        self.m().create_batch(&params)
    }
    fn claim_public(&mut self, id: String) -> String {
        match parse_index("id", &id) {
            Ok(id) => self.m().claim_public(id),
            Err(error) => bad_input(error),
        }
    }
    fn claim_private(&mut self, id: String, target: String) -> String {
        match parse_index("id", &id) {
            Ok(id) => self.m().claim_private(id, &target),
            Err(error) => bad_input(error),
        }
    }
    fn cancel(&mut self, id: String) -> String {
        match parse_index("id", &id) {
            Ok(id) => self.m().cancel(id),
            Err(error) => bad_input(error),
        }
    }
    fn make_irrevocable(&mut self, id: String) -> String {
        match parse_index("id", &id) {
            Ok(id) => self.m().make_irrevocable(id),
            Err(error) => bad_input(error),
        }
    }
    fn signal_milestone(&mut self, id: String, index: String) -> String {
        match (parse_index("id", &id), parse_index("index", &index)) {
            (Ok(id), Ok(index)) => self.m().signal_milestone(id, index),
            (Err(error), _) | (_, Err(error)) => bad_input(error),
        }
    }
    fn transfer_beneficiary(&mut self, id: String, new_beneficiary: String) -> String {
        match parse_index("id", &id) {
            Ok(id) => self.m().transfer_beneficiary(id, &new_beneficiary),
            Err(error) => bad_input(error),
        }
    }
    fn import_schedule(&mut self, account: String) -> String {
        self.m().import_schedule(&account)
    }
    fn resolve_name(&mut self, name: String) -> String {
        self.m().resolve_name(&name)
    }
    fn on_context_ready(&mut self, ctx: &RustModuleContext) {
        let dir = std::path::PathBuf::from(&ctx.instance_persistence_path);
        if !ctx.instance_persistence_path.is_empty() {
            let _ = std::fs::create_dir_all(&dir);
            self.module = Some(Module::new(&dir));
        }
    }
}

#[no_mangle]
pub extern "Rust" fn logos_module_install() {
    install::<Vesting>();
}
