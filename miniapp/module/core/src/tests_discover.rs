//! `discover` and `incoming` against REAL chain bytes: `getBlockRange` and `getAccountView`
//! answers captured from the bridge test network (fixtures/, read through the Mini's local RPC,
//! 2026-11-21). The expected accounts, transaction hashes and schedule fields below come from an
//! independent decoder, the site's own vest/decode.js + vest/logic.js and its build-time index
//! (nameswallet-site site/vest/index.js), not from this crate's output.

use super::*;
use std::sync::Arc;
use std::time::Duration;
use vesting_sdk::names::MockRpc;

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const PROGRAM: &str = "2kgxMAueD56gK2tArRA4ziFFw1Hz4W6njVZWGNJXHeM2";
const CHAIN: &str = "2da7f7dfb6e51a28c942ea80391aa518253ec515e98d28cb4d83cc9e16ff5374";
// Schedule 5sSE6 (create-linear, block 38738): EuXKk pays 5g78o.
const S1: &str = "5sSE6c4M8Jpfs9JgozowgT3Gm4urm3EbqZ6PAPA5u7sF";
const TX1: &str = "1e54efd29e3cf817ad3c96ca01808150612b86893055f0d847d2a76589988d12";
// Schedule 6Zwf9 (milestone, created in block 38748, moved from 5g78o to BDetF in block 38752).
const S2: &str = "6Zwf9EvGkJAwfjVqioa5CgYHbyATaMmUwCxS3FfzMkcf";
const TX_CREATE2: &str = "1c6eddce1ca1877fe92a2337df6952207836d44bc2102d2d1bebd5881aa237b0";
const TX_TRANSFER: &str = "f42caa5d53b647533f95340011e7da4e63fe7d1485cf9fad99f22386fb1144ce";
const CREATOR: &str = "EuXKk9jwn58PXyQjY9Cd5EZorrPb8ay7Thid9TzFeVhW";
const PAYEE: &str = "5g78oRK2ngo1yBVZ2LvrSHLmw6qcieH52FL7wa3qXKtd";
const NEW_PAYEE: &str = "BDetFzTXvxVgfaEgAZSPQdp3YfUW9qKyGTNwq2y8mU2N";

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn result(text: &str) -> Value {
    let answer: Value = serde_json::from_str(text).unwrap();
    answer["result"].clone()
}

fn block_38738() -> Value {
    result(include_str!("../fixtures/getBlockRange-38738.json"))
}
fn block_38748() -> Value {
    result(include_str!("../fixtures/getBlockRange-38748.json"))
}
fn block_38752() -> Value {
    result(include_str!("../fixtures/getBlockRange-38752.json"))
}
fn view_s1() -> Value {
    result(include_str!("../fixtures/getAccountView-5sSE6.json"))
}
fn view_s2() -> Value {
    result(include_str!("../fixtures/getAccountView-6Zwf9.json"))
}
fn view_creator() -> Value {
    result(include_str!("../fixtures/getAccountView-EuXKk.json"))
}
fn view_payee() -> Value {
    result(include_str!("../fixtures/getAccountView-5g78o.json"))
}
fn chain_id() -> Value {
    result(include_str!("../fixtures/getChannelId.json"))
}

fn id(b58: &str) -> AccountId {
    names::parse_address(b58).unwrap()
}

fn parse(text: String) -> Value {
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// 6Zwf9's real state as it is now, with its beneficiary put back to 5g78o: the state before
/// the transfer of block 38752.
fn view_s2_before_transfer() -> Value {
    let raw = view_s2()["data"]["actor_states"][PROGRAM].clone();
    let bytes: Vec<u8> = serde_json::from_value(raw).unwrap();
    let mut state: vesting_program::ScheduleState = borsh::from_slice(&bytes).unwrap();
    state.schedule.as_mut().unwrap().beneficiary = id(PAYEE);
    json!({ "data": { "actor_states": { PROGRAM: borsh::to_vec(&state).unwrap() } } })
}

fn view_s1_paying(beneficiary: &str) -> Value {
    let raw = view_s1()["data"]["actor_states"][PROGRAM].clone();
    let bytes: Vec<u8> = serde_json::from_value(raw).unwrap();
    let mut state: vesting_program::ScheduleState = borsh::from_slice(&bytes).unwrap();
    state.schedule.as_mut().unwrap().beneficiary = id(beneficiary);
    json!({ "data": { "actor_states": { PROGRAM: borsh::to_vec(&state).unwrap() } } })
}

/// An empty block with block_id `id`, encoded as `getBlockRange` returns it.
fn empty_block(id: u64) -> Value {
    let mut block = discover::decode_blocks(&block_38738()).unwrap().remove(0);
    block.header.block_id = id;
    block.body.transactions.clear();
    serde_json::to_value(&block).unwrap()
}

fn empty_range(from: u64, to: u64) -> Value {
    Value::Array((from..=to).map(empty_block).collect())
}

fn range_with(from: u64, to: u64, at: u64, fixture: Value) -> Value {
    let real = fixture.as_array().unwrap()[0].clone();
    Value::Array(
        (from..=to)
            .map(|id| {
                if id == at {
                    real.clone()
                } else {
                    empty_block(id)
                }
            })
            .collect(),
    )
}

fn block_with_id(fixture: &Value, id: u64) -> Value {
    let mut block = discover::decode_blocks(fixture).unwrap().remove(0);
    block.header.block_id = id;
    json!([serde_json::to_value(&block).unwrap()])
}

/// The network clock's account, as the RPC answers it.
fn clock(timestamp: u64) -> Value {
    let program = vesting_program::clock::clock_account_id().into_value();
    json!({ "data": { "actor_states": { names::base58(&program): borsh::to_vec(&(7u64, timestamp)).unwrap() } } })
}

/// A module signing with Names Wallet on a scripted chain that is the bridge test network's
/// Vest deployment, with the program's first block as the floor (as the built-in profile has).
fn scan_module(rpc: &Arc<MockRpc>, floor: u64) -> Module {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("vesting-discover-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("net.json");
    std::fs::write(
        &file,
        json!({
            "format": "names-network/2", "lez": "0.4", "name": "Scripted",
            "rpc": "http://127.0.0.1:9", "chain_id": "",
            "programs": { "vesting": { "id": PROGRAM, "floor_block": floor } }
        })
        .to_string(),
    )
    .unwrap();
    let mut module = Module::new(&dir);
    module.set_chain_client(RpcClient::new(rpc.clone()));
    module.set_scan_client(RpcClient::new(rpc.clone()));
    module.set_scan_pacing(Duration::ZERO, Duration::from_secs(60));
    let answer = parse(module.configure(
        &json!({ "signer": "names", "network_file": file.display().to_string() }).to_string(),
    ));
    assert_eq!(answer["ok"], true, "{answer}");
    module
}

fn give(module: &mut Module, accounts: &[&str]) {
    let rows: Vec<Value> = accounts
        .iter()
        .map(|a| json!({ "account": format!("Public/{a}"), "account_kind": "public" }))
        .collect();
    assert_eq!(
        parse(module.set_accounts(&Value::Array(rows).to_string()))["ok"],
        true
    );
}

/// The answers of the start of a call: the chain id and the tip.
fn start(rpc: &MockRpc, tip: u64) {
    rpc.push_ok("getChannelId", chain_id());
    rpc.push_ok("getLastBlockId", json!(tip));
}

fn calls(rpc: &MockRpc, method: &str) -> Vec<Value> {
    rpc.calls()
        .into_iter()
        .filter(|c| c.method == method)
        .map(|c| c.params)
        .collect()
}

fn viewed(rpc: &MockRpc) -> Vec<String> {
    calls(rpc, "getAccountView")
        .iter()
        .map(|p| p[0]["account_id"].as_str().unwrap_or("").to_string())
        .collect()
}

// ── Decoding real blocks ─────────────────────────────────────────────────────────────

#[test]
fn real_blocks_decode_to_the_accounts_vest_touched() {
    let program = id(PROGRAM);
    let shown = |answer: Value| -> Vec<(u64, String, String)> {
        let blocks = discover::decode_blocks(&answer).unwrap();
        blocks
            .iter()
            .flat_map(|b| {
                discover::vest_accounts(b, &program)
                    .into_iter()
                    .map(|(a, tx)| (b.header.block_id, names::base58(&a), tx))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    // Each block has three transactions; only the Vest one names Vest's accounts.
    assert_eq!(
        discover::decode_blocks(&block_38738()).unwrap()[0]
            .body
            .transactions
            .len(),
        3
    );
    assert_eq!(
        shown(block_38738()),
        vec![
            (38738, S1.into(), TX1.into()),
            (38738, CREATOR.into(), TX1.into())
        ]
    );
    assert_eq!(
        shown(block_38748()),
        vec![
            (38748, S2.into(), TX_CREATE2.into()),
            (38748, CREATOR.into(), TX_CREATE2.into())
        ]
    );
    assert_eq!(
        shown(block_38752()),
        vec![
            (38752, PAYEE.into(), TX_TRANSFER.into()),
            (38752, S2.into(), TX_TRANSFER.into())
        ]
    );
    // Another program id finds nothing in the same bytes.
    let other = discover::decode_blocks(&block_38738()).unwrap();
    assert!(discover::vest_accounts(&other[0], &[7u8; 32]).is_empty());
    // Not a block: refused in plain words.
    assert!(discover::decode_blocks(&json!(["AAAA"])).is_err());
    assert!(discover::decode_blocks(&json!({})).is_err());
}

#[test]
fn a_real_view_decodes_to_its_schedule_and_beneficiary() {
    let rpc = Arc::new(MockRpc::new());
    let client = RpcClient::new(rpc.clone());
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    let state = discover::read_state(&client, &id(S1), &id(PROGRAM))
        .unwrap()
        .unwrap();
    let s = state.schedule.unwrap();
    assert_eq!(names::base58(&s.creator), CREATOR);
    assert_eq!(names::base58(&s.beneficiary), PAYEE);
    assert_eq!((s.total, s.claimed), (600_000, 81_385));
    assert!(matches!(
        s.kind,
        vesting_core::ScheduleKind::Linear {
            start: 1795194308000,
            end: 1795194638000
        }
    ));
    // The creator's own account holds no Vest state (an empty list under the program).
    assert_eq!(
        discover::read_state(&client, &id(CREATOR), &id(PROGRAM)).unwrap(),
        None
    );
}

// ── discover ─────────────────────────────────────────────────────────────────────────

#[test]
fn discover_imports_a_real_schedule_for_its_beneficiary() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    start(&rpc, 38738);
    rpc.push_ok("getBlockRange", block_38738());
    rpc.push_ok("getAccountView", view_s1()); // the schedule
    rpc.push_ok("getAccountView", view_creator()); // the creator's account: no schedule
    rpc.push_ok("getAccountView", view_s1()); // the import's own check
    let found = parse(module.discover(""));
    assert_eq!(found["ok"], true, "{found}");
    assert_eq!(found["chain_id"], CHAIN);
    assert_eq!(found["from_block"], 38738);
    assert_eq!(found["to_block"], 38738);
    assert_eq!(found["done"], true);
    assert_eq!(found["found"], json!([S1]));
    assert_eq!(found["new"][0]["creator"], hex::encode(id(CREATOR)));
    assert_eq!(found["incoming"], 1);
    assert_eq!(found["known"], 1);
    assert_eq!(calls(&rpc, "getBlockRange"), vec![json!([38738, 38738])]);
    assert_eq!(viewed(&rpc), vec![S1, CREATOR, S1]);

    // list() shows it, with the role.
    rpc.push_ok("getAccountView", clock(1795194400000));
    rpc.push_ok("getAccountView", view_s1());
    let listed = parse(module.list());
    assert_eq!(listed["ok"], true, "{listed}");
    assert_eq!(listed["schedules"][0]["address"], hex::encode(id(S1)));
    assert_eq!(listed["schedules"][0]["mine"]["beneficiary"], true);
    assert_eq!(listed["schedules"][0]["mine"]["creator"], false);

    // At the tip already: nothing read, nothing announced twice.
    start(&rpc, 38738);
    let again = parse(module.discover("2000"));
    assert_eq!(again["done"], true, "{again}");
    assert_eq!(again["found"], json!([]));
    assert_eq!(again["incoming"], 1);
    assert_eq!(calls(&rpc, "getBlockRange").len(), 1);

    // incoming(): from the saved scan, no network read; the creator is paid nothing.
    let before = rpc.calls().len();
    let mine = parse(module.incoming(&json!([PAYEE]).to_string()));
    assert_eq!(mine["ok"], true, "{mine}");
    let row = &mine["schedules"][0];
    assert_eq!(row["address"], hex::encode(id(S1)));
    assert_eq!(row["beneficiary_base58"], PAYEE);
    assert_eq!(row["creator_base58"], CREATOR);
    assert_eq!(row["total"], "600000");
    // Ended long ago with 81385 of 600000 claimed: the rest is claimable.
    assert_eq!(row["status"], "claimable");
    assert_eq!(row["claimable"], "518615");
    let hex_too = parse(module.incoming(&json!([hex::encode(id(PAYEE))]).to_string()));
    assert_eq!(hex_too["schedules"].as_array().unwrap().len(), 1);
    let creator = parse(module.incoming(&json!([CREATOR]).to_string()));
    assert_eq!(creator["schedules"], json!([]));
    assert_eq!(rpc.calls().len(), before);
}

#[test]
fn an_account_shared_later_gets_its_schedules_without_new_blocks() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[NEW_PAYEE]);
    start(&rpc, 38738);
    rpc.push_ok("getBlockRange", block_38738());
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    let first = parse(module.discover(""));
    assert_eq!(
        (first["found"].clone(), first["incoming"].clone()),
        (json!([]), json!(0)),
        "{first}"
    );
    // Names Wallet shares the payee's account: the next call imports it from the saved scan.
    give(&mut module, &[PAYEE]);
    start(&rpc, 38738);
    rpc.push_ok("getAccountView", view_s1()); // the import's own check
    let second = parse(module.discover(""));
    assert_eq!(second["found"], json!([S1]), "{second}");
    assert_eq!(calls(&rpc, "getBlockRange").len(), 1);
}

#[test]
fn discover_resumes_and_reads_at_most_max_blocks() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    // 150 blocks of a far tip: two ranges, 100 then 50.
    start(&rpc, 50_000);
    rpc.push_ok(
        "getBlockRange",
        range_with(38738, 38837, 38738, block_38738()),
    );
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getBlockRange", empty_range(38838, 38887));
    rpc.push_ok("getAccountView", view_s1()); // import
    let first = parse(module.discover("150"));
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["to_block"], 38887);
    assert_eq!(first["done"], false);
    assert_eq!(first["found"], json!([S1]));
    assert_eq!(
        calls(&rpc, "getBlockRange"),
        vec![json!([38738, 38837]), json!([38838, 38887])]
    );
    // A new module on the same folder (Basecamp restarted) carries on where it stopped.
    let dir = module.dir.clone();
    let mut again = Module::new(&dir);
    again.set_chain_client(RpcClient::new(rpc.clone()));
    again.set_scan_client(RpcClient::new(rpc.clone()));
    again.set_scan_pacing(Duration::ZERO, Duration::from_secs(60));
    give(&mut again, &[PAYEE]);
    start(&rpc, 38900);
    rpc.push_ok("getBlockRange", empty_range(38888, 38900));
    let second = parse(again.discover("2000"));
    assert_eq!(second["from_block"], 38888, "{second}");
    assert_eq!(second["to_block"], 38900);
    assert_eq!(second["done"], true);
    assert_eq!(second["found"], json!([]));
    assert_eq!(second["incoming"], 1);
    assert_eq!(
        calls(&rpc, "getBlockRange").last().unwrap(),
        &json!([38888, 38900])
    );
    // The default is 2000 blocks; more than the cap is cut to it; 0 and words are refused.
    assert_eq!(discover::parse_max_blocks("").unwrap(), 2000);
    assert_eq!(
        discover::parse_max_blocks("99999999").unwrap(),
        discover::MAX_BLOCKS_CAP
    );
    assert_eq!(parse(again.discover("0"))["ok"], false);
    assert_eq!(parse(again.discover("lots"))["ok"], false);
}

#[test]
fn a_view_that_fails_is_read_first_on_the_next_call() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    start(&rpc, 38738);
    rpc.push_ok("getBlockRange", block_38738());
    // The view's answer is broken: the block is read, its accounts wait for the next call.
    rpc.push_ok("getAccountView", json!({}));
    let failed = parse(module.discover(""));
    assert_eq!(failed["ok"], false, "{failed}");
    assert_eq!(failed["to_block"], 38738);
    start(&rpc, 38738);
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getAccountView", view_s1());
    let next = parse(module.discover(""));
    assert_eq!(next["found"], json!([S1]), "{next}");
    assert_eq!(next["done"], true);
    assert_eq!(calls(&rpc, "getBlockRange").len(), 1);
}

#[test]
fn another_chain_id_starts_over_from_the_floor() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    start(&rpc, 38738);
    rpc.push_ok("getBlockRange", block_38738());
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getAccountView", view_s1());
    assert_eq!(parse(module.discover(""))["known"], 1);
    // The network was reset: a new chain id. The old scan is dropped, reading starts again.
    rpc.push_ok("getChannelId", json!("ab".repeat(32)));
    rpc.push_ok("getLastBlockId", json!(38800));
    rpc.push_ok("getBlockRange", empty_range(38738, 38800));
    let reset = parse(module.discover(""));
    assert_eq!(reset["ok"], true, "{reset}");
    assert_eq!(reset["chain_id"], "ab".repeat(32));
    assert_eq!(reset["from_block"], 38738);
    assert_eq!(reset["to_block"], 38800);
    assert_eq!(reset["known"], 0);
    assert_eq!(reset["incoming"], 0);
    let saved = discover::ScanState::load(&module.dir).unwrap();
    assert_eq!(saved.chain_id, "ab".repeat(32));
    assert!(saved.touched.is_empty());
    assert_eq!(
        calls(&rpc, "getBlockRange").last().unwrap(),
        &json!([38738, 38800])
    );
}

#[test]
fn a_transfer_away_is_read_again() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38748);
    give(&mut module, &[PAYEE]);
    start(&rpc, 38748);
    rpc.push_ok("getBlockRange", block_38748());
    rpc.push_ok("getAccountView", view_s2_before_transfer());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getAccountView", view_s2_before_transfer()); // import
    let made = parse(module.discover(""));
    assert_eq!(
        (made["found"].clone(), made["incoming"].clone()),
        (json!([S2]), json!(1)),
        "{made}"
    );
    // Block 38752 moves it to BDetF: the scan reads it again, and it no longer pays 5g78o.
    start(&rpc, 38752);
    rpc.push_ok(
        "getBlockRange",
        range_with(38749, 38752, 38752, block_38752()),
    );
    rpc.push_ok("getAccountView", view_payee());
    rpc.push_ok("getAccountView", view_s2());
    let moved = parse(module.discover(""));
    assert_eq!(moved["ok"], true, "{moved}");
    assert_eq!(moved["changed"], true, "{moved}");
    assert_eq!(moved["incoming"], 0);
    assert_eq!(viewed(&rpc)[3..], [PAYEE, S2]);
    assert_eq!(
        parse(module.incoming(&json!([PAYEE]).to_string()))["schedules"],
        json!([])
    );
    let theirs = parse(module.incoming(&json!([NEW_PAYEE]).to_string()));
    assert_eq!(theirs["schedules"][0]["address_base58"], S2);
    // Both milestones signalled and all claimed: closed.
    assert_eq!(theirs["schedules"][0]["status"], "closed");
    let saved = discover::ScanState::load(&module.dir).unwrap();
    assert_eq!(
        saved.touched[S2],
        vec![
            (38748, TX_CREATE2.to_string()),
            (38752, TX_TRANSFER.to_string())
        ]
    );
}

#[test]
fn a_transfer_to_you_is_found() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38748);
    give(&mut module, &[NEW_PAYEE]);
    start(&rpc, 38748);
    rpc.push_ok("getBlockRange", block_38748());
    rpc.push_ok("getAccountView", view_s2_before_transfer());
    rpc.push_ok("getAccountView", view_creator());
    let before = parse(module.discover(""));
    assert_eq!(
        (before["found"].clone(), before["incoming"].clone()),
        (json!([]), json!(0)),
        "{before}"
    );
    start(&rpc, 38752);
    rpc.push_ok(
        "getBlockRange",
        range_with(38749, 38752, 38752, block_38752()),
    );
    rpc.push_ok("getAccountView", view_payee());
    rpc.push_ok("getAccountView", view_s2());
    rpc.push_ok("getAccountView", view_s2()); // import
    let after = parse(module.discover(""));
    assert_eq!(after["found"], json!([S2]), "{after}");
    assert_eq!(after["changed"], true);
    assert_eq!(after["incoming"], 1);
}

// ── incoming(): the status buckets ───────────────────────────────────────────────────

#[test]
fn incoming_sorts_schedules_into_unvested_claimable_and_closed() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let (me, other) = ([2u8; 32], [3u8; 32]);
    let day = 86_400_000;
    let linear = |start: u64, end: u64, beneficiary: [u8; 32]| {
        let p =
            vesting_core::CreateParams::linear([1; 32], beneficiary, [0; 32], 1_000, start, end);
        vesting_core::create(p, 0).unwrap().0
    };
    let unvested = linear(now + day, now + 2 * day, me);
    let claimable = linear(now - day, now + day, me);
    let mut claimed = linear(now - 2 * day, now - day, me);
    claimed.claimed = 1_000;
    let mut cancelled = linear(now - 2 * day, now + day, me);
    cancelled.cancelled = Some(vesting_core::CancelRecord {
        at: now - day,
        vested_at_cancel: 300,
    });
    cancelled.claimed = 300;
    let not_mine = linear(now - day, now + day, other);
    let mut state = discover::ScanState::fresh(CHAIN, &id(PROGRAM), 0);
    for (i, s) in [&unvested, &claimable, &claimed, &cancelled, &not_mine]
        .into_iter()
        .enumerate()
    {
        state.schedules.insert(
            names::base58(&[10 + i as u8; 32]),
            discover::Known {
                schedule: hex::encode(borsh::to_vec(s).unwrap()),
                native: true,
                block: 1,
            },
        );
    }
    state.save(&module.dir).unwrap();
    let answer = parse(module.incoming(&json!([names::base58(&me)]).to_string()));
    assert_eq!(answer["ok"], true, "{answer}");
    let by: std::collections::BTreeMap<String, (String, Value)> = answer["schedules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["address_base58"].as_str().unwrap().to_string(),
                (
                    r["status"].as_str().unwrap().to_string(),
                    r["next_unlock_ms"].clone(),
                ),
            )
        })
        .collect();
    assert_eq!(by.len(), 4);
    let at = |i: u8| by[&names::base58(&[10 + i; 32])].clone();
    assert_eq!(at(0).0, "unvested");
    assert!(at(0).1.as_u64().unwrap() > now + day - 1000);
    assert_eq!(at(1).0, "claimable");
    assert_eq!(at(2), ("closed".to_string(), Value::Null));
    assert_eq!(at(3), ("closed".to_string(), Value::Null));
    // Another network's scan is not answered from.
    let other_file = module.dir.join("other.json");
    std::fs::write(
        &other_file,
        json!({ "format": "names-network/2", "lez": "0.4", "rpc": "http://127.0.0.1:9",
        "programs": { "vesting": { "id": names::base58(&[7; 32]) } } })
        .to_string(),
    )
    .unwrap();
    parse(
        module.configure(&json!({ "network_file": other_file.display().to_string() }).to_string()),
    );
    assert_eq!(
        parse(module.incoming(&json!([names::base58(&me)]).to_string()))["schedules"],
        json!([])
    );
    // Not a list of accounts: refused.
    assert_eq!(parse(module.incoming("nope"))["ok"], false);
    assert_eq!(parse(module.incoming(r#"["junk!"]"#))["ok"], false);
}

#[test]
fn status_words() {
    let p = vesting_core::CreateParams::linear([1; 32], [2; 32], [0; 32], 100, 1000, 2000);
    let mut s = vesting_core::create(p, 0).unwrap().0;
    assert_eq!(discover::status(&s, 500), "unvested");
    assert_eq!(discover::status(&s, 1500), "claimable");
    s.claimed = 100;
    assert_eq!(discover::status(&s, 3000), "closed");
}

#[test]
fn imports_are_a_few_per_call() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    let program = id(PROGRAM);
    // Seven known schedules paying 5g78o, already scanned up to the tip.
    let mut state = discover::ScanState::fresh(CHAIN, &program, 38738);
    state.last_block = Some(38738);
    let p = vesting_core::CreateParams::linear(id(CREATOR), id(PAYEE), [0; 32], 1_000, 0, 1_000);
    let schedule = vesting_core::create(p, 0).unwrap().0;
    let answer = {
        let full = vesting_program::ScheduleState {
            version: 1,
            nonce: [9; 32],
            asset: vesting_program::Asset::Native,
            schedule: Some(schedule.clone()),
            float: 0,
            per_claim: 0,
            pending: None,
        };
        json!({ "data": { "actor_states": { PROGRAM: borsh::to_vec(&full).unwrap() } } })
    };
    for i in 0..7u8 {
        state.schedules.insert(
            names::base58(&[40 + i; 32]),
            discover::Known {
                schedule: hex::encode(borsh::to_vec(&schedule).unwrap()),
                native: true,
                block: 38738,
            },
        );
    }
    state.save(&module.dir).unwrap();
    start(&rpc, 38738);
    for _ in 0..5 {
        rpc.push_ok("getAccountView", answer.clone());
    }
    let first = parse(module.discover(""));
    assert_eq!(first["found"].as_array().unwrap().len(), 5, "{first}");
    assert_eq!(first["incoming"], 7);
    assert_eq!(first["done"], false);
    start(&rpc, 38738);
    for _ in 0..2 {
        rpc.push_ok("getAccountView", answer.clone());
    }
    let second = parse(module.discover(""));
    assert_eq!(second["found"].as_array().unwrap().len(), 2, "{second}");
    assert_eq!(second["done"], true);
    assert_eq!(calls(&rpc, "getBlockRange").len(), 0);
}

#[test]
fn get_block_range_halves_on_error_down_to_one() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    // Four blocks [38738, 38741]: errors halve 4 -> 2 -> 1 before succeeding on [38738, 38738].
    start(&rpc, 38741);
    rpc.push_ok("getBlockRange", json!({})); // [38738, 38741] fails
    rpc.push_ok("getBlockRange", json!({})); // [38738, 38739] fails
    rpc.push_ok("getBlockRange", block_38738()); // [38738, 38738] succeeds
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getBlockRange", empty_range(38739, 38741));
    rpc.push_ok("getAccountView", view_s1()); // import
    let res = parse(module.discover(""));
    assert_eq!(res["ok"], true, "{res}");
    assert_eq!(res["done"], true);
    assert_eq!(res["to_block"], 38741);
    assert_eq!(res["found"], json!([S1]));
    assert_eq!(
        calls(&rpc, "getBlockRange"),
        vec![
            json!([38738, 38741]),
            json!([38738, 38739]),
            json!([38738, 38738]),
            json!([38739, 38741]),
        ]
    );
    // At a 1-block range, an error gives up without looping and keeps pacing between calls.
    module.set_scan_pacing(Duration::from_millis(15), Duration::from_secs(60));
    start(&rpc, 38743);
    rpc.push_ok("getBlockRange", json!({})); // [38742, 38743] fails
    rpc.push_ok("getBlockRange", json!({})); // [38742, 38742] fails -> gives up
    let t = std::time::Instant::now();
    let failed = parse(module.discover(""));
    assert_eq!(failed["ok"], false, "{failed}");
    assert_eq!(failed["to_block"], 38741);
    assert!(
        t.elapsed() >= Duration::from_millis(35),
        "expected paced calls across halving"
    );
    assert_eq!(
        calls(&rpc, "getBlockRange")[4..],
        [json!([38742, 38743]), json!([38742, 38742])]
    );
}

#[test]
fn short_or_gapped_block_range_never_skips_blocks() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    // RPC returns only 38738..=38739 for [38738, 38741]: next range must start at 38740.
    start(&rpc, 38741);
    rpc.push_ok(
        "getBlockRange",
        range_with(38738, 38739, 38738, block_38738()),
    );
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getBlockRange", empty_range(38740, 38741));
    rpc.push_ok("getAccountView", view_s1()); // import
    let res = parse(module.discover(""));
    assert_eq!(res["ok"], true, "{res}");
    assert_eq!(res["to_block"], 38741);
    assert_eq!(res["done"], true);
    assert_eq!(
        calls(&rpc, "getBlockRange"),
        vec![json!([38738, 38741]), json!([38740, 38741])]
    );
    // A gap (38742 and 38744 without 38743) advances only to 38742, then asks [38743, 38744].
    start(&rpc, 38744);
    rpc.push_ok(
        "getBlockRange",
        Value::Array(vec![empty_block(38742), empty_block(38744)]),
    );
    rpc.push_ok("getBlockRange", empty_range(38743, 38744));
    let gapped = parse(module.discover(""));
    assert_eq!(gapped["ok"], true, "{gapped}");
    assert_eq!(gapped["to_block"], 38744);
    assert_eq!(
        calls(&rpc, "getBlockRange")[2..],
        [json!([38742, 38744]), json!([38743, 38744])]
    );
    // Missing `next` (empty list or starts at next+1): errors and does not advance last_block.
    start(&rpc, 38746);
    rpc.push_ok("getBlockRange", json!([]));
    let empty = parse(module.discover(""));
    assert_eq!(empty["ok"], false, "{empty}");
    assert_eq!(empty["to_block"], 38744);
    start(&rpc, 38746);
    rpc.push_ok("getBlockRange", Value::Array(vec![empty_block(38746)]));
    let missing_next = parse(module.discover(""));
    assert_eq!(missing_next["ok"], false, "{missing_next}");
    assert_eq!(missing_next["to_block"], 38744);
}

#[test]
fn imports_skip_after_scan_error_respect_budget_and_stop_on_first_error() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    let program = id(PROGRAM);
    let mut state = discover::ScanState::fresh(CHAIN, &program, 38738);
    state.last_block = Some(38738);
    let p = vesting_core::CreateParams::linear(id(CREATOR), id(PAYEE), [0; 32], 1_000, 0, 1_000);
    let schedule = vesting_core::create(p, 0).unwrap().0;
    for i in 0..3u8 {
        state.schedules.insert(
            names::base58(&[50 + i; 32]),
            discover::Known {
                schedule: hex::encode(borsh::to_vec(&schedule).unwrap()),
                native: true,
                block: 38738,
            },
        );
    }
    state.save(&module.dir).unwrap();

    // 1. Scan error: import loop is skipped completely (zero getAccountView calls).
    start(&rpc, 38739);
    rpc.push_ok("getBlockRange", json!({}));
    let scan_err = parse(module.discover(""));
    assert_eq!(scan_err["ok"], false, "{scan_err}");
    assert!(calls(&rpc, "getAccountView").is_empty());

    // 2. Import error: stops at the first failing import instead of attempting all 3.
    start(&rpc, 38738);
    rpc.push_ok("getAccountView", json!({}));
    let imp_err = parse(module.discover(""));
    assert_eq!(imp_err["ok"], false, "{imp_err}");
    assert_eq!(calls(&rpc, "getAccountView").len(), 1);

    // 3. Exhausted time budget before imports: makes no import calls and reports done: false.
    module.set_scan_pacing(Duration::ZERO, Duration::ZERO);
    start(&rpc, 38738);
    let budgeted = parse(module.discover(""));
    assert_eq!(budgeted["ok"], true, "{budgeted}");
    assert_eq!(budgeted["done"], false);
    assert_eq!(budgeted["found"], json!([]));
    assert_eq!(budgeted["incoming"], 3);
    assert_eq!(calls(&rpc, "getAccountView").len(), 1);
}

#[test]
fn tip_below_last_block_resets_scan_state() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    let program = id(PROGRAM);
    let mut old = discover::ScanState::fresh(CHAIN, &program, 38738);
    old.last_block = Some(40_000);
    old.tip = 40_000;
    old.schedules.insert(
        names::base58(&[99u8; 32]),
        discover::Known {
            schedule: String::new(),
            native: true,
            block: 39_000,
        },
    );
    old.save(&module.dir).unwrap();
    // Same chain_id, but tip (38738) is below last_block (40000): state resets from floor.
    start(&rpc, 38738);
    rpc.push_ok("getBlockRange", block_38738());
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getAccountView", view_s1()); // import
    let res = parse(module.discover(""));
    assert_eq!(res["ok"], true, "{res}");
    assert_eq!(res["from_block"], 38738);
    assert_eq!(res["to_block"], 38738);
    assert_eq!(res["found"], json!([S1]));
    assert_eq!(res["known"], 1);
    assert_eq!(calls(&rpc, "getBlockRange"), vec![json!([38738, 38738])]);
}

#[test]
fn creator_schedule_announced_when_transferred_to_creator_and_changed_flag() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[CREATOR]);
    // Block 38738: S1 is imported for CREATOR (not beneficiary), so changed=true, found=[].
    start(&rpc, 38738);
    rpc.push_ok("getBlockRange", block_38738());
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getAccountView", view_s1()); // import for creator role
    let first = parse(module.discover(""));
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["changed"], true);
    assert_eq!(first["found"], json!([]));
    assert_eq!(first["new"], json!([]));
    assert_eq!(first["incoming"], 0);

    // Idle poll: nothing changed.
    start(&rpc, 38738);
    let idle = parse(module.discover(""));
    assert_eq!(idle["changed"], false, "{idle}");
    assert_eq!(idle["found"], json!([]));

    // Block 38739 touches S1 again, now paying CREATOR: already imported in backend, but
    // newly paying CREATOR, so it is announced in found/new and sets changed=true.
    start(&rpc, 38739);
    rpc.push_ok("getBlockRange", block_with_id(&block_38738(), 38739));
    rpc.push_ok("getAccountView", view_s1_paying(CREATOR));
    rpc.push_ok("getAccountView", view_creator());
    let transferred = parse(module.discover(""));
    assert_eq!(transferred["ok"], true, "{transferred}");
    assert_eq!(transferred["changed"], true);
    assert_eq!(transferred["found"], json!([S1]));
    assert_eq!(transferred["new"][0]["id"], 0);
    assert_eq!(transferred["new"][0]["address_base58"], S1);
    assert_eq!(transferred["incoming"], 1);

    // Next poll: already announced, nothing changed.
    start(&rpc, 38739);
    let after = parse(module.discover(""));
    assert_eq!(after["changed"], false, "{after}");
    assert_eq!(after["found"], json!([]));
    assert_eq!(after["new"], json!([]));
}

#[test]
fn deleted_id_map_reimports_schedule_even_when_in_state_imported() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    start(&rpc, 38738);
    rpc.push_ok("getBlockRange", block_38738());
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getAccountView", view_s1()); // import
    assert_eq!(parse(module.discover(""))["found"], json!([S1]));
    assert!(discover::ScanState::load(&module.dir)
        .unwrap()
        .imported
        .contains(S1));

    // Delete the backend's id map while state.imported still records S1.
    std::fs::remove_file(module.dir.join("vesting-names-ids.json")).unwrap();
    let mut reopened = Module::new(&module.dir);
    reopened.set_chain_client(RpcClient::new(rpc.clone()));
    reopened.set_scan_client(RpcClient::new(rpc.clone()));
    reopened.set_scan_pacing(Duration::ZERO, Duration::from_secs(60));
    give(&mut reopened, &[PAYEE]);

    start(&rpc, 38738);
    rpc.push_ok("getAccountView", view_s1()); // re-import into fresh id map
    let res = parse(reopened.discover(""));
    assert_eq!(res["ok"], true, "{res}");
    assert_eq!(res["changed"], true);

    rpc.push_ok("getAccountView", clock(1795194400000));
    rpc.push_ok("getAccountView", view_s1());
    let listed = parse(reopened.list());
    assert_eq!(listed["schedules"].as_array().unwrap().len(), 1, "{listed}");
    assert_eq!(listed["schedules"][0]["address"], hex::encode(id(S1)));
}

#[test]
fn idle_discover_does_not_rewrite_state_file() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, 38738);
    give(&mut module, &[PAYEE]);
    start(&rpc, 38738);
    rpc.push_ok("getBlockRange", block_38738());
    rpc.push_ok("getAccountView", view_s1());
    rpc.push_ok("getAccountView", view_creator());
    rpc.push_ok("getAccountView", view_s1());
    assert_eq!(parse(module.discover(""))["ok"], true);

    // Rewrite with pretty JSON (contains newlines); save() writes compact JSON (no newlines).
    let path = discover::ScanState::path(&module.dir);
    let saved = discover::ScanState::load(&module.dir).unwrap();
    assert!(!saved.touched.is_empty(), "touched is preserved");
    let pretty = serde_json::to_vec_pretty(&saved).unwrap();
    assert!(pretty.contains(&b'\n'));
    std::fs::write(&path, &pretty).unwrap();

    start(&rpc, 38738);
    let idle = parse(module.discover(""));
    assert_eq!(idle["ok"], true, "{idle}");
    assert_eq!(idle["done"], true);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        pretty,
        "idle discover must not rewrite vesting-discover.json"
    );
}

#[test]
fn near_u64_max_blocks_and_overflowing_milestone_do_not_panic() {
    let _serial = lock();
    let rpc = Arc::new(MockRpc::new());
    let mut module = scan_module(&rpc, u64::MAX - 1);
    give(&mut module, &[PAYEE]);
    // Scan blocks [u64::MAX - 1, u64::MAX] without overflow.
    start(&rpc, u64::MAX);
    rpc.push_ok("getBlockRange", empty_range(u64::MAX - 1, u64::MAX));
    let res = parse(module.discover(""));
    assert_eq!(res["ok"], true, "{res}");
    assert_eq!(res["to_block"], u64::MAX);
    assert_eq!(res["done"], true);
    let saved = discover::ScanState::load(&module.dir).unwrap();
    assert_eq!(saved.next_block(), None);

    // Calling again at u64::MAX does not wrap or rescan.
    start(&rpc, u64::MAX);
    let again = parse(module.discover(""));
    assert_eq!(again["ok"], true, "{again}");
    assert_eq!(again["done"], true);
    assert_eq!(again["from_block"], Value::Null);
    assert_eq!(calls(&rpc, "getBlockRange").len(), 1);

    // Crafted milestone schedule whose signalled tranches sum past u128::MAX saturates safely.
    let mut bad = vesting_core::Schedule {
        creator: id(CREATOR),
        beneficiary: id(PAYEE),
        token: [0u8; 32],
        total: u128::MAX,
        claimed: 0,
        kind: vesting_core::ScheduleKind::Milestone {
            tranches: vec![u128::MAX, 1],
        },
        cancelable: false,
        transferable: false,
        cancel_authority: id(CREATOR),
        milestone_authority: id(CREATOR),
        signalled: vec![3],
        cancelled: None,
        last_time: 0,
        version: vesting_core::STATE_VERSION,
    };
    assert_eq!(bad.signalled_total(), u128::MAX);
    assert_eq!(discover::status(&bad, 0), "claimable");
    let mut state = discover::ScanState::fresh(CHAIN, &id(PROGRAM), 0);
    state.schedules.insert(
        S1.into(),
        discover::Known {
            schedule: hex::encode(borsh::to_vec(&bad).unwrap()),
            native: true,
            block: 1,
        },
    );
    state.save(&module.dir).unwrap();
    let inc = parse(module.incoming(&json!([PAYEE]).to_string()));
    assert_eq!(inc["ok"], true, "{inc}");
    assert_eq!(inc["schedules"][0]["status"], "claimable");
    assert_eq!(inc["schedules"][0]["claimable"], u128::MAX.to_string());
    bad.claimed = u128::MAX;
    assert_eq!(discover::status(&bad, 0), "closed");
}

#[test]
fn save_writes_atomically_and_leaves_no_tmp_file() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "vesting-discover-save-{}-{nanos}",
        std::process::id()
    ));
    let mut state = discover::ScanState::fresh(CHAIN, &id(PROGRAM), 38738);
    state.last_block = Some(38738);
    state.tip = 38738;
    state.save(&dir).unwrap();
    assert!(discover::ScanState::path(&dir).exists());
    assert!(!dir.join(format!("{}.tmp", discover::STATE_FILE)).exists());
    assert_eq!(discover::ScanState::load(&dir), Some(state));
    let _ = std::fs::remove_dir_all(&dir);
}
