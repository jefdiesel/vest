//! The module's calls with the JSON backend: JSON in, JSON out, as Basecamp calls them.

use super::*;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use vesting_sdk::names::MockRpc;

// The display decimals are module-wide: tests that set them run one at a time.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn hex_id(byte: u8) -> String {
    hex::encode([byte; 32])
}

fn parse(text: String) -> Value {
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// A module in a fresh folder, with creator 1, beneficiary 2 and a private account 5 in the
/// "wallet", the clock at `now`, and `decimals`.
fn fresh(now: u64, decimals: u8) -> Module {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("vesting-module-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut module = Module::new(&dir);
    let configured = parse(
        module.configure(
            &json!({
                "now": now, "decimals": decimals,
                "accounts": [
                    { "id": hex_id(1) }, { "id": hex_id(2) },
                    { "id": hex_id(5), "private": true },
                ]
            })
            .to_string(),
        ),
    );
    assert_eq!(configured["ok"], true, "{configured}");
    module
}

fn linear(start: u64, end: u64) -> Value {
    json!({ "type": "linear", "start": start, "end": end })
}

fn create_row(total: &str, kind: Value) -> String {
    json!({
        "creator": hex_id(1), "beneficiary": hex_id(2), "token": "LEZ",
        "total": total, "kind": kind, "cancelable": true, "transferable": true
    })
    .to_string()
}

#[test]
fn settings_persist_and_name_the_built_in_network() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    let settings = parse(module.settings());
    assert_eq!(settings["ok"], true);
    assert_eq!(settings["connected"], true);
    assert_eq!(settings["network"]["source"], "built-in");
    assert_eq!(
        settings["network"]["program"],
        "2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2"
    );
    // The password is held in memory, never written.
    parse(module.configure(r#"{"password":"secret","clock_elf":"/x/clock.bin"}"#));
    let saved = std::fs::read_to_string(module.dir.join("settings.json")).unwrap();
    assert!(saved.contains("/x/clock.bin"));
    assert!(!saved.contains("secret"));
    // A second module on the same folder reads the saved settings.
    let mut again = Module::new(&module.dir);
    assert_eq!(
        parse(again.settings())["settings"]["clock_elf"],
        "/x/clock.bin"
    );
}

#[test]
fn a_network_file_and_overrides() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    let file = module.dir.join("net.json");
    std::fs::write(
        &file,
        json!({
            "format": "names-network/1", "version": 2, "lez": "0.4", "name": "Test chain",
            "chain_id": "AB".repeat(32), "rpc": "https://rpc.example/",
            "programs": { "vesting": { "id": hex_id(7) }, "token": null }
        })
        .to_string(),
    )
    .unwrap();
    let settings = parse(
        module.configure(
            &json!({ "network_file": file.display().to_string(), "token_program": hex_id(8) })
                .to_string(),
        ),
    );
    assert_eq!(settings["network"]["name"], "Test chain");
    assert_eq!(settings["network"]["chain_id"], "ab".repeat(32));
    assert_eq!(settings["network"]["program"], names::base58(&[7u8; 32]));
    assert_eq!(
        settings["network"]["token_program"],
        names::base58(&[8u8; 32])
    );
    // Another RPC is another chain: the file's chain id no longer applies to it.
    let settings = parse(module.configure(r#"{"rpc":"http://127.0.0.1:3071"}"#));
    assert_eq!(settings["network"]["chain_id"], "");
    // A file for another LEZ line is refused.
    std::fs::write(&file, r#"{"format":"names-network/2","lez":"0.5"}"#).unwrap();
    let refused = parse(module.configure(r#"{"rpc":""}"#));
    assert_eq!(refused["ok"], false);
    assert!(refused["error"].as_str().unwrap().contains("0.4"));
}

#[test]
fn create_list_and_roles_from_the_wallet() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    let made = parse(module.create(&create_row("1000", linear(0, 1000))));
    assert_eq!(made["ok"], true, "{made}");
    assert_eq!(made["id"], 0);
    // "LEZ" is native LEZ: the zero token.
    assert_eq!(made["schedule"]["token"], hex_id(0));
    let listed = parse(module.list());
    let row = &listed["schedules"][0];
    assert_eq!(listed["time"], 500);
    assert_eq!(row["total"], "1000");
    assert_eq!(row["claimable"], "500");
    assert_eq!(row["mine"]["beneficiary"], true);
    assert_eq!(row["mine"]["creator"], true);
    let accounts = parse(module.accounts());
    assert_eq!(accounts["accounts"].as_array().unwrap().len(), 3);
    assert_eq!(accounts["accounts"][2]["private"], true);
}

#[test]
fn accounts_may_be_base58() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    let row = json!({
        "creator": format!("Public/{}", names::base58(&[1u8; 32])),
        "beneficiary": names::base58(&[2u8; 32]),
        "token": "LEZ", "total": "10", "kind": linear(0, 10)
    });
    let made = parse(module.create(&row.to_string()));
    assert_eq!(made["ok"], true, "{made}");
    assert_eq!(made["schedule"]["beneficiary"], hex_id(2));
}

#[test]
fn names_resolve_through_the_lns_registry() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    let lns = module.effective_network().unwrap().lns_registry.unwrap();
    let rpc = Arc::new(MockRpc::new());
    rpc.push_ok(
        "getAccountView",
        names::mock_record_answer(&lns, "alice", &[2u8; 32], u64::MAX),
    );
    module.set_lns_client(RpcClient::new(rpc.clone()));
    let row = json!({
        "creator": hex_id(1), "beneficiary": "alice", "token": "LEZ",
        "total": "10", "kind": linear(0, 10)
    });
    let made = parse(module.create(&row.to_string()));
    assert_eq!(made["ok"], true, "{made}");
    assert_eq!(made["schedule"]["beneficiary"], hex_id(2));
    assert_eq!(made["resolved"][0]["name"], "alice");
    assert_eq!(rpc.call_count("getAccountView"), 1);
    // The name book shows the name beside the address afterwards.
    assert_eq!(parse(module.list())["names"][hex_id(2)], "alice");
    // An unknown name is a plain error and nothing is created.
    rpc.push_ok(
        "getAccountView",
        json!({ "data": { "actor_states": { names::base58(&lns): Vec::<u8>::new() } } }),
    );
    let unknown = parse(module.resolve_name("nobody"));
    assert_eq!(unknown["ok"], false);
    assert!(unknown["error"]
        .as_str()
        .unwrap()
        .contains("unknown LNS name"));
    assert_eq!(
        parse(module.list())["schedules"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn summary_and_public_claim_by_the_beneficiary() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    module.create(&create_row("1000", linear(0, 1000)));
    let summary = parse(module.pre_claim_summary(0, ""));
    assert_eq!(summary["ok"], true, "{summary}");
    assert_eq!(summary["summary"]["claimable"], "500");
    // The JSON ledger charges no fee: nothing simulated, nothing to check.
    assert_eq!(summary["summary"]["estimated_fee"], "0");
    assert_eq!(summary["summary"]["fee_simulated"], false);
    assert_eq!(summary["summary"]["fee_covered"], Value::Null);
    let claim = parse(module.claim_public(0));
    assert_eq!(claim["ok"], true);
    assert_eq!(claim["amount"], "500");
    let again = parse(module.claim_public(0));
    assert_eq!(again["ok"], false);
    assert!(again["error"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("claimable"));
    assert!(again["code"].is_number());
}

#[test]
fn a_private_claim_needs_a_private_account_of_the_wallet() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    module.create(&create_row("1000", linear(0, 1000)));
    // Account 4 is not a private account the wallet holds: refused (RFP Privacy 3, code 1001).
    let wrong = parse(module.claim_private(0, &hex_id(4)));
    assert_eq!(wrong["ok"], false);
    assert_eq!(wrong["code"], 1001);
    // Nor is the wallet's public account 2.
    assert_eq!(parse(module.claim_private(0, &hex_id(2)))["code"], 1001);
    let right = parse(module.claim_private(0, &hex_id(5)));
    assert_eq!(right["ok"], true, "{right}");
    assert_eq!(right["amount"], "500");
    let summary = parse(module.pre_claim_summary(0, &hex_id(5)));
    assert_eq!(summary["summary"]["destination_kind"], "private");
}

#[test]
fn batch_signal_cancel_irrevocable_transfer() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    let rows = json!([
        { "creator": hex_id(1), "beneficiary": hex_id(2), "token": "LEZ", "total": "100", "kind": linear(0, 100) },
        { "creator": hex_id(1), "beneficiary": hex_id(3), "token": "LEZ", "total": "200", "kind": linear(0, 200) }
    ]);
    let made = parse(module.create_batch(&rows.to_string()));
    assert_eq!(made["ids"], json!([0, 1]));
    let milestone = json!({ "type": "milestone", "tranches": ["40", "60"] });
    module.create(&create_row("100", milestone));
    let signalled = parse(module.signal_milestone(2, 0));
    assert_eq!(signalled["amount"], "40");
    let again = parse(module.signal_milestone(2, 0));
    assert_eq!(again["code"], 18);
    let cancel = parse(module.cancel(2));
    assert_eq!(cancel["refund"], "60");
    // The list says when it was cancelled, what went back and what had vested (still claimable).
    let cancelled = &parse(module.list())["schedules"][2];
    assert_eq!(cancelled["cancelled"], true);
    assert_eq!(cancelled["cancelled_at"], 500);
    assert_eq!(cancelled["vested_at_cancel"], "40");
    assert_eq!(cancelled["refund"], "60");
    assert_eq!(cancelled["claimable"], "40");
    let live = &parse(module.list())["schedules"][0];
    assert_eq!(live["cancelled_at"], Value::Null);
    assert_eq!(live["refund"], Value::Null);
    assert_eq!(parse(module.make_irrevocable(2))["ok"], false);
    let transfer = parse(module.transfer_beneficiary(2, &hex_id(4)));
    assert_eq!(transfer["ok"], true);
    assert_eq!(transfer["beneficiary"], hex_id(4));
    assert_eq!(parse(module.make_irrevocable(0))["ok"], true);
}

#[test]
fn bad_input_is_a_plain_error() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    let bad = parse(module.create(&create_row("not-a-number", linear(0, 1000))));
    assert_eq!(bad["ok"], false);
    assert!(bad["error"].as_str().unwrap().contains("total"));
    let missing = parse(module.pre_claim_summary(9, ""));
    assert_eq!(missing["ok"], false);
    assert!(missing["error"].as_str().unwrap().contains("9"));
    assert_eq!(parse(module.import_schedule(&hex_id(9)))["ok"], false);
    assert_eq!(parse(module.configure("{nope"))["ok"], false);
}

#[test]
fn amounts_arrive_ready_formatted() {
    let _serial = lock();
    let mut module = fresh(500, 2); // 2 decimals: 1000 -> 10 LEZ
    module.create(&create_row("1000", linear(0, 1000)));
    let row = &parse(module.list())["schedules"][0];
    assert_eq!(row["total_display"], "10 LEZ");
    assert_eq!(row["claimable_display"], "5 LEZ");
    let summary = parse(module.pre_claim_summary(0, ""));
    assert_eq!(summary["summary"]["estimated_fee_display"], "0 LEZ");
    let claim = parse(module.claim_public(0));
    assert_eq!(claim["claimed_total_display"], "5 LEZ");
    display::set_decimals(9);
}

#[test]
fn ids_arrive_as_text() {
    assert_eq!(parse_index("id", "0"), Ok(0));
    assert_eq!(parse_index("id", " 42 "), Ok(42));
    assert!(parse_index("id", "")
        .unwrap_err()
        .contains("id must be a whole number"));
    assert!(parse_index("index", "-1").is_err());
    assert!(parse_index("id", "1.5").is_err());
    let answer = parse(bad_input("id must be a whole number, got 'x'"));
    assert_eq!(answer["ok"], false);
}

/// A fake public RPC that always answers like the edge's rate limit: HTTP 429 and an HTML page.
fn rate_limited_rpc() -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut out = stream;
            loop {
                let mut length = 0usize;
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let page = "<!DOCTYPE html><html><body><h1>Error 1015</h1><p>You are being rate limited</p></body></html>";
                let head = format!("HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: {}\r\n\r\n", page.len());
                if out
                    .write_all(head.as_bytes())
                    .and_then(|_| out.write_all(page.as_bytes()))
                    .is_err()
                {
                    break;
                }
            }
        }
    });
    url
}

#[test]
fn a_rate_limited_network_is_plain_words_never_html() {
    let _serial = lock();
    let mut module = fresh(500, 9);
    module.guard_local_rpc(vesting_sdk::rpc_guard::Policy {
        per_second: 50,
        tries: 3,
        first_wait: std::time::Duration::from_millis(10),
        max_wait: std::time::Duration::from_millis(20),
    });
    let rpc = rate_limited_rpc();
    parse(
        module.configure(
            &json!({ "rpc": rpc, "chain_id": "", "lns_registry": hex_id(7) }).to_string(),
        ),
    );
    let answer = parse(module.resolve_name("alice"));
    assert_eq!(answer["ok"], false);
    assert_eq!(answer["error"], vesting_sdk::rpc_guard::STILL_BUSY);
    let text = answer.to_string();
    assert!(!text.contains("<html") && !text.contains("<h1>"), "{text}");
    assert!(
        answer["detail"].as_str().unwrap_or("").contains("HTTP 429"),
        "{text}"
    );
    // The relay counted every try, and the settings say so.
    let settings = parse(module.settings());
    assert_eq!(settings["network_requests"]["refused"], 3);
}

#[test]
fn an_error_with_html_in_it_never_reaches_the_app() {
    let answer = parse(fail(
        "RPC failed: <!DOCTYPE html><html><body>blocked</body></html>",
    ));
    assert_eq!(answer["ok"], false);
    assert!(!answer.to_string().contains("<html"));
    assert!(answer["error"].as_str().unwrap().contains("can't read"));
}

// ── Names Wallet as the signer ──────────────────────────────────────────────────────

/// A schedule account's state as the Vest program keeps it, in the RPC's answer.
fn schedule_answer(
    program: &[u8; 32],
    schedule: &vesting_core::Schedule,
    asset: vesting_program::Asset,
) -> Value {
    let state = vesting_program::ScheduleState {
        version: 1,
        nonce: [9; 32],
        asset,
        schedule: Some(schedule.clone()),
        float: 0,
        per_claim: 0,
        pending: None,
    };
    let bytes = borsh::to_vec(&state).unwrap();
    json!({ "data": { "actor_states": { names::base58(program): bytes } } })
}

fn clock_answer(timestamp: u64) -> Value {
    let program = vesting_program::clock::clock_account_id().into_value();
    json!({ "data": { "actor_states": { names::base58(&program): borsh::to_vec(&(7u64, timestamp)).unwrap() } } })
}

/// A module that signs with Names Wallet, reading a scripted chain; schedule account S pays B.
fn names_module(rpc: Arc<MockRpc>) -> Module {
    let mut module = fresh(0, 9);
    module.set_chain_client(RpcClient::new(rpc));
    let answer =
        parse(module.configure(r#"{"signer":"names","rpc":"http://127.0.0.1:9","chain_id":""}"#));
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["signer"], "names");
    module
}

fn linear_schedule() -> vesting_core::Schedule {
    let params = vesting_core::CreateParams::linear([1; 32], [2; 32], [0; 32], 1_000, 0, 1_000);
    vesting_core::create(params, 0).unwrap().0
}

#[test]
fn names_wallet_claims_with_a_program_call_it_can_carry() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = names_module(rpc.clone());
    let program = module.effective_network().unwrap().program.unwrap();
    let schedule = linear_schedule();
    let s = [0x44u8; 32];
    // Import (one read), a refused claim (one), then the claim: the schedule, the state again
    // and the clock.
    for _ in 0..4 {
        rpc.push_ok(
            "getAccountView",
            schedule_answer(&program, &schedule, vesting_program::Asset::Native),
        );
    }
    rpc.push_ok("getAccountView", clock_answer(400));
    let imported = parse(module.import_schedule(&hex::encode(s)));
    assert_eq!(imported["ok"], true, "{imported}");
    // Not yet connected in Names: the beneficiary's account isn't one Vest was given.
    let refused = parse(module.claim_call(0));
    assert!(
        refused["error"].as_str().unwrap().contains("hasn't shared"),
        "{refused}"
    );
    parse(module.set_accounts(&json!([{ "account": format!("Public/{}", names::base58(&[2; 32])), "account_kind": "public", "balance": "5" }]).to_string()));
    let call = parse(module.claim_call(0));
    assert_eq!(call["ok"], true, "{call}");
    assert_eq!(call["program"], names::base58(&program));
    assert_eq!(call["amount"], "400");
    assert_eq!(
        call["caller"],
        format!("Public/{}", names::base58(&[2; 32]))
    );
    // The message is the program's ClaimAsOf at the network's time, to the beneficiary itself.
    let expected = vesting_program::Instruction::ClaimAsOf {
        schedule: lee_core::account::AccountId::new(s),
        to: lee_core::account::AccountId::new([2; 32]),
        to_private: false,
        as_of: 400,
    };
    assert_eq!(
        call["instruction_hex"],
        hex::encode(borsh::to_vec(&expected).unwrap())
    );
    // The accounts Names Wallet's propose_call takes: the schedule's data, its LEZ, the signer's LEZ.
    let accounts: Value = serde_json::from_str(call["accounts_json"].as_str().unwrap()).unwrap();
    let sb = names::base58(&s);
    assert_eq!(
        accounts,
        json!([sb, { "account": sb, "balance": true }, { "account": "signer", "balance": true }])
    );
    // The list says the schedule is the wallet's to claim.
    rpc.push_ok("getAccountView", clock_answer(400));
    rpc.push_ok(
        "getAccountView",
        schedule_answer(&program, &schedule, vesting_program::Asset::Native),
    );
    let listed = parse(module.list());
    assert_eq!(
        listed["schedules"][0]["mine"]["beneficiary"], true,
        "{listed}"
    );
}

#[test]
fn names_wallet_refuses_what_it_cant_sign_in_plain_words() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = names_module(rpc.clone());
    for write in [
        module.create(&create_row("10", linear(0, 10))),
        module.cancel(0),
        module.make_irrevocable(0),
        module.signal_milestone(0, 0),
        module.transfer_beneficiary(0, &hex_id(4)),
    ] {
        let answer = parse(write);
        assert_eq!(answer["ok"], false);
        assert!(
            answer["error"]
                .as_str()
                .unwrap()
                .contains("Names Wallet can't sign this for Vest yet"),
            "{answer}"
        );
    }
    // A token schedule can't be claimed through Names yet either.
    let program = module.effective_network().unwrap().program.unwrap();
    let token = vesting_program::Asset::Token {
        program: lee_core::account::AccountId::new([5; 32]),
        definition: lee_core::account::AccountId::new([6; 32]),
    };
    for _ in 0..3 {
        rpc.push_ok(
            "getAccountView",
            schedule_answer(&program, &linear_schedule(), token),
        );
    }
    parse(module.import_schedule(&hex_id(0x45)));
    parse(module.set_accounts(
        &json!([{ "account": format!("Public/{}", names::base58(&[2; 32])) }]).to_string(),
    ));
    let answer = parse(module.claim_call(0));
    assert!(
        answer["error"].as_str().unwrap().contains("token schedule"),
        "{answer}"
    );
}
