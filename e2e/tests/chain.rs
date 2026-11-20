//! End to end (RFP-017 Supportability 2, Functionality 6): a LEZ v0.4.0-rc3 standalone sequencer
//! runs in this process (its home made by the `sequencer_service setup` binary), `scripts/e2e.sh`
//! drives every operation through the vesting CLI over its RPC, and the program's events are read
//! back from the sequencer's own event store (the storage behind `getEvents`), which the JSON-RPC
//! of a standalone sequencer does not serve.
//!
//! Ignored by default; run with `cargo test -p vesting-e2e -- --ignored --nocapture` and:
//!   SEQ_BIN      sequencer_service binary (v0.4.0-rc3, `--features standalone`), for `setup`
//!   LOCAL_JSON   rc3 lez/sequencer/service/networks/local.json
//!   VESTING_BIN  the vesting CLI (built with --features lez)
//!   VESTING_ELF, TOKEN_ELF   guest ELFs (target/riscv-guest/.../vesting_guest.bin, token_guest.bin)
//!   E2E_PORT     optional RPC port (default 3071)
//! Proofs are dev-mode (RISC0_DEV_MODE=1 is set for the sequencer and the CLI).

use std::{collections::BTreeMap, path::PathBuf, process::Command};

use sequencer_storage_actor::protocol::GetBlockEvents;
use vesting_program::VestingEvent;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a sequencer binary, the CLI and guest ELFs (see the module docs)"]
async fn every_operation_on_a_standalone_sequencer() {
    // SAFETY: set before any thread of ours reads the environment.
    unsafe { std::env::set_var("RISC0_DEV_MODE", "1") };
    let work = PathBuf::from(std::env::var("E2E_DIR").unwrap_or_else(|_| {
        std::env::temp_dir()
            .join(format!("vesting-e2e-{}", std::process::id()))
            .display()
            .to_string()
    }));
    std::fs::create_dir_all(&work).unwrap();
    let home = work.join("seq");
    let port: u16 = std::env::var("E2E_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3071);

    // 1. A fresh chain home from LEZ's own setup (keys, channel, genesis of local.json).
    let status = Command::new(env("SEQ_BIN"))
        .args([
            "setup",
            home.to_str().unwrap(),
            "--config",
            &env("LOCAL_JSON"),
        ])
        .args([
            "--funding-public-key",
            "0101010101010101010101010101010101010101010101010101010101010100",
            "--create-channel",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "sequencer setup failed");
    let config_path = home.join("sequencer_config.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
    config["home"] = serde_json::json!(home);
    config["block_create_timeout"] = serde_json::json!("3s");
    config["gossip"] = serde_json::Value::Null;
    config["cross_zone"] = serde_json::Value::Null;
    std::fs::write(&config_path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();

    // 2. The sequencer, in process, serving JSON-RPC on loopback.
    let sequencer_config = sequencer_service::SequencerConfig::from_path(&config_path).unwrap();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let handle = sequencer_service::run(sequencer_config, addr)
        .await
        .expect("sequencer starts");
    let rpc = format!("http://127.0.0.1:{port}");

    // 3. Every operation through the CLI.
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/e2e.sh");
    let (rpc2, work2) = (rpc.clone(), work.join("run"));
    let ok = tokio::task::spawn_blocking(move || {
        Command::new("bash")
            .arg(script)
            .env("V", env("VESTING_BIN"))
            .env("RPC", rpc2)
            .env("W", &work2)
            .env("VESTING_ELF", env("VESTING_ELF"))
            .env("TOKEN_ELF", env("TOKEN_ELF"))
            .status()
            .unwrap()
            .success()
    })
    .await
    .unwrap();
    println!(
        "{}",
        std::fs::read_to_string(work.join("run/RESULTS.txt")).unwrap_or_default()
    );

    // 4. The vesting program's events, from the sequencer's event store, block by block.
    let program_hex = std::fs::read_to_string(work.join("run/program.hex"))
        .expect("the script deployed the program");
    let program = lee_core::account::AccountId::new(
        hex::decode(program_hex.trim()).unwrap().try_into().unwrap(),
    );
    let tip: u64 = {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}"#;
        let out = Command::new("curl")
            .args([
                "-s",
                "-X",
                "POST",
                &rpc,
                "-H",
                "Content-Type: application/json",
                "-d",
                body,
            ])
            .output()
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap()["result"]
            .as_u64()
            .unwrap()
    };
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for block_id in 1..=tip {
        let stored = handle
            .storage_ref()
            .ask(GetBlockEvents { block_id })
            .send()
            .await
            .unwrap();
        let groups = stored.unwrap_or_default();
        if !groups.is_empty() {
            let programs: Vec<String> = groups
                .iter()
                .flat_map(|g| g.events.iter().map(|e| e.account_id.to_string()))
                .collect();
            println!(
                "block {block_id}: {} transaction(s) with events from {programs:?}",
                groups.len()
            );
        }
        for group in groups {
            for e in group.events {
                if e.account_id != program {
                    continue;
                }
                let decoded: VestingEvent =
                    borsh::from_slice(&e.event.data).expect("a vesting event decodes");
                assert_eq!(
                    e.event.selector,
                    vesting_program::selector(&decoded.event),
                    "selector matches the event"
                );
                let name = format!("{:?}", decoded.event)
                    .split([' ', '{'])
                    .next()
                    .unwrap()
                    .to_string();
                println!(
                    "block {block_id} tx {} {name} schedule {}",
                    hex::encode(group.tx_hash.0),
                    decoded.schedule
                );
                *seen.entry(name).or_default() += 1;
            }
        }
    }
    println!("events: {seen:?}");
    handle.shutdown().await;
    assert!(ok, "scripts/e2e.sh reported a failure (see RESULTS above)");
    // An rc3 sequencer drops the events of blocks it produces (lez/sequencer/core/src/lib.rs at
    // bafd07be: `Ok((_events, tx_reverted))`, `events: Vec::new()`) and stores them when a block
    // comes back through Bedrock; a standalone sequencer's Bedrock is a mock, so only genesis events
    // are stored here.
    // Each transition's event is checked natively in program/tests/events.rs. On a chain with
    // Bedrock (the testnet) the same check reads them through the indexer's getEvents.
    if seen.is_empty() {
        println!(
            "NOTE: no vesting events stored: this standalone sequencer has not finalized any block \
             past genesis (events are stored on Bedrock finalization)"
        );
    } else {
        for name in [
            "Created",
            "Claimed",
            "Cancelled",
            "MilestoneSignalled",
            "MadeIrrevocable",
            "BeneficiaryTransferred",
        ] {
            assert!(seen.contains_key(name), "no {name} event was stored");
        }
    }
}
