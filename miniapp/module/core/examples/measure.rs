//! How many requests the module sends the public RPC for what the app does on open: a refresh
//! (settings, accounts, now, list) and one claim summary. Read-only (the summary is a dry run).
//!
//! cargo run --release -p vesting-module-core --features lez --example measure -- \
//!     <wallet folder> <create tx hash> [--no-cache]
//!
//! The schedule is found from the transaction that created it, then imported into a fresh data
//! folder. Prints the relay's count per method.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (wallet, tx) = (&args[1], &args[2]);
    let cache = !args.iter().any(|a| a == "--no-cache");
    let dir = std::env::temp_dir().join(format!("vest-measure-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut module = vesting_module_core::Module::new(&dir);
    println!(
        "configure: {}",
        short(&module.configure(&format!(r#"{{"wallet_dir":"{wallet}"}}"#)))
    );
    module.set_caching(cache);
    let rpc = vesting_module_core::network::built_in().rpc;
    let candidates =
        vesting_sdk::lez::transaction_accounts(&rpc, tx).expect("the create transaction");
    let mut id = None;
    for account in candidates {
        let answer: serde_json::Value =
            serde_json::from_str(&module.import_schedule(&hex::encode(account))).unwrap();
        if answer["ok"] == true {
            id = answer["id"].as_u64();
            break;
        }
    }
    let id = id.expect("no schedule account in that transaction");
    let before = module.network_requests().unwrap().0.values().sum::<u64>();
    // What the app does on open: one refresh, then one claim summary.
    for call in ["settings", "accounts", "now", "list"] {
        let text = match call {
            "settings" => module.settings(),
            "accounts" => module.accounts(),
            "now" => module.now(),
            _ => module.list(),
        };
        println!("{call}: {}", short(&text));
    }
    println!("summary: {}", short(&module.pre_claim_summary(id, "")));
    let (sent, refused) = module.network_requests().unwrap();
    let total: u64 = sent.values().sum();
    println!("caching {}: {} requests for open + refresh + summary (plus {} to find and import the schedule), {} refused",
             if cache { "on" } else { "off" }, total - before, before, refused);
    for (method, n) in sent {
        println!("  {method}: {n}");
    }
}

fn short(text: &str) -> String {
    text.chars().take(160).collect()
}
