//! Read-only: run `discover` to the tip of a real chain, as the app does (one call at a time,
//! the module's own pacing), then `incoming` for some beneficiaries. Nothing is signed or sent.
//!
//!   cargo run -p vesting-module-core --example discover_live -- <rpc> <chain id> <floor> <account>...
//!
//! e.g. through a tunnel to the Mini's local RPC:
//!   ssh -N -L 13290:127.0.0.1:3290 minim4 &
//!   cargo run -p vesting-module-core --example discover_live -- http://127.0.0.1:13290 \
//!     2da7f7df…5374 38700 5g78oRK2ngo1yBVZ2LvrSHLmw6qcieH52FL7wa3qXKtd

use serde_json::{json, Value};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [rpc, chain, floor, accounts @ ..] = args.as_slice() else {
        eprintln!("usage: discover_live <rpc> <chain id> <floor block> <account>...");
        std::process::exit(2);
    };
    let dir = std::env::temp_dir().join(format!("vest-discover-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("net.json");
    let network = json!({
        "format": "names-network/2", "lez": "0.4", "name": "live", "rpc": rpc, "chain_id": chain,
        "programs": { "vesting": { "id": "2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2",
                                   "floor_block": floor.parse::<u64>().unwrap() } }
    });
    std::fs::write(&file, network.to_string()).unwrap();
    let mut module = vesting_module_core::Module::new(&dir);
    let cfg: Value = serde_json::from_str(&module.configure(
        &json!({ "signer": "names", "network_file": file.display().to_string() }).to_string(),
    ))
    .unwrap();
    assert_eq!(cfg["ok"], true, "{cfg}");
    let rows: Vec<Value> = accounts.iter().map(|a| json!({ "account": format!("Public/{a}") })).collect();
    println!("set_accounts {}", module.set_accounts(&Value::Array(rows).to_string()));
    let all = Instant::now();
    let mut calls = 0;
    loop {
        let t = Instant::now();
        let answer: Value = serde_json::from_str(&module.discover("2000")).unwrap();
        calls += 1;
        println!(
            "call {calls}: {:.1} s  blocks {}-{} of {}  found {}  incoming {}  known {}  pending {}  done {}  {}",
            t.elapsed().as_secs_f64(), answer["from_block"], answer["to_block"], answer["tip"],
            answer["found"].as_array().map_or(0, |f| f.len()), answer["incoming"], answer["known"],
            answer["pending"], answer["done"], answer["error"].as_str().unwrap_or("")
        );
        if answer["done"] == true || calls > 200 {
            break;
        }
    }
    println!("whole scan: {:.0} s, {calls} calls", all.elapsed().as_secs_f64());
    let state: Value = serde_json::from_slice(&std::fs::read(dir.join("vesting-discover.json")).unwrap()).unwrap();
    let schedules = state["schedules"].as_object().unwrap();
    println!("schedule accounts found on the chain: {}", schedules.len());
    for a in schedules.keys() {
        println!("  {a}");
    }
    println!("other accounts Vest touched: {}", state["touched"].as_object().unwrap().len() - schedules.len());
    let answer: Value = serde_json::from_str(&module.incoming(&json!(accounts).to_string())).unwrap();
    for row in answer["schedules"].as_array().unwrap() {
        println!("incoming {} from {} total {} claimable {} status {}", row["address_base58"], row["creator_base58"], row["total_display"], row["claimable_display"], row["status"]);
    }
}
