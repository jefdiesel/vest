//! Logos vesting core module skeleton: the eleven mini-app calls over a C ABI.
//!
//! Each `vesting_*` export corresponds to exactly one `vesting_sdk::Client` method (see
//! ../README.md). Input and output are JSON text: `const char*` in, a heap `char*` out, freed
//! with `vesting_free`. A successful answer has `{"ok":true, ...}`; a failure has
//! `{"ok":false,"error":"...","code":<n>}` with the SDK error's own message and code, so the
//! Qt wrapper can show it directly. No key, wallet or network is involved yet.
//!
//! `vesting_init` is module setup (not an SDK call): it opens the JSON-file backend at
//! `state_path` and fixes the clock with `now`. The LEZ backend will replace that later.

mod backend;
mod display;
mod wire;

use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;
use vesting_core::AccountId;
use vesting_sdk::{AccountKind, AccountRef, Client, SdkError};

#[cfg(feature = "lez")]
type AppBackend = vesting_sdk::LezBackend;
#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
type AppBackend = backend::JsonFileBackend;
#[cfg(all(not(feature = "lez"), not(feature = "json-file-backend")))]
type AppBackend = backend::MemoryBackend;

static CLIENT: Mutex<Option<Client<AppBackend>>> = Mutex::new(None);
/// Mock LNS map until the SDK's names module lands (module helper, not an SDK call).
static NAMES: Mutex<Option<std::collections::BTreeMap<String, String>>> = Mutex::new(None);

// ── ABI plumbing ────────────────────────────────────────────────────────────────────

fn to_c(value: Value) -> *mut c_char {
    let text = value.to_string();
    CString::new(text)
        .unwrap_or_else(|_| CString::new("{\"ok\":false,\"error\":\"NUL in answer\"}").unwrap())
        .into_raw()
}

fn parse(text: *const c_char) -> Result<Value, String> {
    if text.is_null() {
        return Ok(json!({}));
    }
    let text = unsafe { CStr::from_ptr(text) }
        .to_str()
        .map_err(|e| e.to_string())?;
    serde_json::from_str(text).map_err(|e| format!("invalid JSON: {e}"))
}

/// Params for a call; a parse error becomes an input-error object the call can pass through.
fn params(text: *const c_char) -> Value {
    parse(text).unwrap_or_else(|e| json!({ "error": e }))
}

fn fail(message: impl Into<String>) -> Value {
    json!({ "ok": false, "error": message.into() })
}

fn sdk_fail(error: SdkError) -> Value {
    json!({ "ok": false, "error": error.message(), "code": error.code() })
}

fn answer(run: impl FnOnce(&mut Client<AppBackend>) -> Value) -> *mut c_char {
    let value = match CLIENT.lock() {
        Ok(mut guard) => match guard.as_mut() {
            Some(client) => run(client),
            None => fail("module is not initialised; call vesting_init first"),
        },
        Err(_) => fail("module state lock is poisoned"),
    };
    to_c(value)
}

fn id_of(p: &Value) -> Result<u64, String> {
    p.get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "id is required".to_string())
}

fn account_of(p: &Value, field: &str) -> Result<AccountId, String> {
    wire::account(p.get(field), field)
}

// ── Module setup, not an SDK call ───────────────────────────────────────────────────

/// Open the JSON-file backend and set the clock. Config:
/// `{"state_path": "/path/state.json", "now": <ms>}`. `now` may be omitted (wall clock).
#[no_mangle]
pub extern "C" fn vesting_init(config: *const c_char) -> *mut c_char {
    let value = match parse(config) {
        Ok(value) => value,
        Err(error) => return to_c(fail(error)),
    };
    let path = value
        .get("state_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    let now = value.get("now").and_then(Value::as_u64);
    if let Some(decimals) = value.get("decimals").and_then(Value::as_u64) {
        display::set_decimals(decimals as u8);
    }

    let opened: Result<Client<AppBackend>, String> = (|| {
        #[cfg(feature = "lez")]
        {
            use std::path::{Path, PathBuf};
            let rpc = value
                .get("rpc_url")
                .and_then(Value::as_str)
                .ok_or("rpc_url is required for the lez backend")?;
            let wallet_dir = value
                .get("wallet_dir")
                .and_then(Value::as_str)
                .ok_or("wallet_dir is required for the lez backend")?;
            let program_hex = value
                .get("program")
                .and_then(Value::as_str)
                .ok_or("program (32-byte hex account) is required for the lez backend")?;
            let bytes = hex::decode(program_hex.trim()).map_err(|e| format!("program: {e}"))?;
            if bytes.len() != 32 {
                return Err("program must be 32 bytes of hex".into());
            }
            let mut program = [0u8; 32];
            program.copy_from_slice(&bytes);
            let clock_elf = value
                .get("clock_elf")
                .and_then(Value::as_str)
                .map(PathBuf::from);
            let map_path = value
                .get("lez_map")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("vesting-lez-ids.json"));
            let mut backend = vesting_sdk::LezBackend::open(
                rpc,
                Path::new(wallet_dir),
                program,
                clock_elf,
                &map_path,
            )
            .map_err(|error| error.to_string())?;
            // Token schedules: the 0.4 token program, and the fee float for private claims.
            if let Some(token_program) = value.get("token_program").and_then(Value::as_str) {
                let bytes =
                    hex::decode(token_program.trim()).map_err(|e| format!("token_program: {e}"))?;
                let token_program: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| "token_program must be 32 bytes of hex".to_string())?;
                backend.set_token_program(token_program);
            }
            let sponsor = |key: &str| {
                value
                    .get(key)
                    .and_then(Value::as_str)
                    .and_then(|t| t.parse::<u128>().ok())
            };
            if let (Some(deposit), Some(per_claim)) =
                (sponsor("sponsor_deposit"), sponsor("sponsor_per_claim"))
            {
                backend.set_sponsor(deposit, per_claim);
            }
            let _ = (path, now); // the chain supplies time; the JSON path is unused
            Ok(Client::new(backend))
        }
        #[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
        {
            let backend = backend::JsonFileBackend::open(std::path::Path::new(path), now)
                .map_err(|error| error.to_string())?;
            Ok(Client::new(backend))
        }
        #[cfg(all(not(feature = "lez"), not(feature = "json-file-backend")))]
        {
            let _ = path;
            Ok(Client::new(backend::MemoryBackend::open(now)))
        }
    })();

    match opened {
        Ok(client) => match CLIENT.lock() {
            Ok(mut guard) => {
                *guard = Some(client);
                to_c(json!({ "ok": true }))
            }
            Err(_) => to_c(fail("module state lock is poisoned")),
        },
        Err(error) => to_c(fail(error)),
    }
}

/// Free a JSON answer returned by any `vesting_*` call.
#[no_mangle]
pub unsafe extern "C" fn vesting_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

// ── The eleven SDK calls ────────────────────────────────────────────────────────────

/// `Client::list()`.
#[no_mangle]
pub extern "C" fn vesting_list(_params: *const c_char) -> *mut c_char {
    answer(|client| {
        let now = client.now();
        let schedules = client
            .list()
            .into_iter()
            .map(|(id, state)| wire::schedule_json(id, &state, now))
            .collect::<Vec<_>>();
        json!({ "ok": true, "schedules": schedules })
    })
}

/// `Client::now()`.
#[no_mangle]
pub extern "C" fn vesting_now(_params: *const c_char) -> *mut c_char {
    answer(|client| json!({ "ok": true, "time": client.now() }))
}

/// `Client::pre_claim_summary(id, AccountKind)`.
#[no_mangle]
pub extern "C" fn vesting_pre_claim_summary(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let id = match id_of(&p) {
        Ok(id) => id,
        Err(error) => return to_c(fail(error)),
    };
    let kind = match p.get("destination_kind").and_then(Value::as_str) {
        Some("private") => AccountKind::Private,
        Some("public") | None => AccountKind::Public,
        Some(other) => return to_c(fail(format!("unknown destination_kind: {other}"))),
    };
    answer(|client| match client.pre_claim_summary(id, kind) {
        Ok(summary) => json!({
            "ok": true,
            "summary": {
                "schedule": summary.schedule,
                "claimable": summary.claimable.to_string(),
                "claimable_display": display::amount(summary.claimable),
                "estimated_fee": summary.estimated_fee.to_string(),
                "estimated_fee_display": display::amount(summary.estimated_fee),
                "next_unlock": summary.next_unlock,
                "destination_kind": if summary.destination_kind == AccountKind::Private { "private" } else { "public" },
            }
        }),
        Err(error) => sdk_fail(error),
    })
}

/// `Client::create(CreateParams)`.
#[no_mangle]
pub extern "C" fn vesting_create(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let create = match wire::create_params(&p) {
        Ok(create) => create,
        Err(error) => return to_c(fail(error)),
    };
    answer(|client| match client.create(create) {
        Ok((id, state, _event)) => {
            let now = client.now();
            json!({ "ok": true, "id": id, "schedule": wire::schedule_json(id, &state, now) })
        }
        Err(error) => sdk_fail(error),
    })
}

/// `Client::create_batch(Vec<CreateParams>)`. Input is a JSON array of create params.
#[no_mangle]
pub extern "C" fn vesting_create_batch(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let rows = match p
        .as_array()
        .or_else(|| p.get("params").and_then(Value::as_array))
    {
        Some(rows) => rows,
        None => return to_c(fail("batch must be a JSON array of schedules")),
    };
    let mut parsed = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match wire::create_params(row) {
            Ok(create) => parsed.push(create),
            Err(error) => return to_c(fail(format!("params[{index}]: {error}"))),
        }
    }
    answer(|client| match client.create_batch(parsed) {
        Ok(created) => {
            let ids = created.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
            json!({ "ok": true, "ids": ids })
        }
        Err(error) => sdk_fail(error),
    })
}

/// `Client::claim_public(id, caller)`.
#[no_mangle]
pub extern "C" fn vesting_claim_public(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let (id, caller) = match (id_of(&p), account_of(&p, "caller")) {
        (Ok(id), Ok(caller)) => (id, caller),
        (Err(error), _) | (_, Err(error)) => return to_c(fail(error)),
    };
    answer(|client| match client.claim_public(id, caller) {
        Ok((next, amount, _event)) => json!({
            "ok": true, "amount": amount.to_string(),
            "amount_display": display::amount(amount),
            "claimed_total": next.claimed.to_string(),
            "claimed_total_display": display::amount(next.claimed),
            "tx": ""
        }),
        Err(error) => sdk_fail(error),
    })
}

/// `Client::claim_private(id, caller, AccountRef)` — the SDK rejects a non-private target.
#[no_mangle]
pub extern "C" fn vesting_claim_private(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let (id, caller, target) = match (
        id_of(&p),
        account_of(&p, "caller"),
        account_of(&p, "target"),
    ) {
        (Ok(id), Ok(caller), Ok(target)) => (id, caller, target),
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => return to_c(fail(error)),
    };
    // The SDK itself enforces Privacy 3: if the caller says this is not a private account,
    // hand it a Public AccountRef so claim_private returns SdkError::TargetNotPrivate.
    let kind = if p
        .get("is_private")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        AccountKind::Private
    } else {
        AccountKind::Public
    };
    answer(
        |client| match client.claim_private(id, caller, AccountRef { id: target, kind }) {
            Ok((next, amount, _event)) => json!({
                "ok": true, "amount": amount.to_string(),
                "amount_display": display::amount(amount),
                "claimed_total": next.claimed.to_string(),
                "claimed_total_display": display::amount(next.claimed),
                "tx": ""
            }),
            Err(error) => sdk_fail(error),
        },
    )
}

/// `Client::cancel(id, caller)`.
#[no_mangle]
pub extern "C" fn vesting_cancel(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let (id, caller) = match (id_of(&p), account_of(&p, "caller")) {
        (Ok(id), Ok(caller)) => (id, caller),
        (Err(error), _) | (_, Err(error)) => return to_c(fail(error)),
    };
    answer(|client| match client.cancel(id, caller) {
        Ok((_next, refund, _event)) => json!({
            "ok": true, "refund": refund.to_string(), "refund_display": display::amount(refund)
        }),
        Err(error) => sdk_fail(error),
    })
}

/// `Client::make_irrevocable(id, caller)`.
#[no_mangle]
pub extern "C" fn vesting_make_irrevocable(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let (id, caller) = match (id_of(&p), account_of(&p, "caller")) {
        (Ok(id), Ok(caller)) => (id, caller),
        (Err(error), _) | (_, Err(error)) => return to_c(fail(error)),
    };
    answer(|client| match client.make_irrevocable(id, caller) {
        Ok((_next, _event)) => json!({ "ok": true }),
        Err(error) => sdk_fail(error),
    })
}

/// `Client::signal_milestone(id, caller, index)`.
#[no_mangle]
pub extern "C" fn vesting_signal(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let index = match p.get("index").and_then(Value::as_u64) {
        Some(index) => index as usize,
        None => return to_c(fail("index is required")),
    };
    let (id, caller) = match (id_of(&p), account_of(&p, "caller")) {
        (Ok(id), Ok(caller)) => (id, caller),
        (Err(error), _) | (_, Err(error)) => return to_c(fail(error)),
    };
    answer(|client| match client.signal_milestone(id, caller, index) {
        Ok((_next, amount, _event)) => json!({
            "ok": true, "amount": amount.to_string(), "amount_display": display::amount(amount)
        }),
        Err(error) => sdk_fail(error),
    })
}

/// `Client::transfer_beneficiary(id, caller, new_beneficiary)`.
#[no_mangle]
pub extern "C" fn vesting_transfer(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let (id, caller, new_beneficiary) = match (
        id_of(&p),
        account_of(&p, "caller"),
        account_of(&p, "new_beneficiary"),
    ) {
        (Ok(id), Ok(caller), Ok(new_beneficiary)) => (id, caller, new_beneficiary),
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => return to_c(fail(error)),
    };
    answer(
        |client| match client.transfer_beneficiary(id, caller, new_beneficiary) {
            Ok((_next, _event)) => json!({ "ok": true }),
            Err(error) => sdk_fail(error),
        },
    )
}

// ── Name seam (module helpers, not SDK calls) ───────────────────────────────────────

fn name_map() -> std::collections::BTreeMap<String, String> {
    let mut guard = NAMES.lock().expect("name map lock");
    guard
        .get_or_insert_with(|| {
            let mut map = std::collections::BTreeMap::new();
            map.insert("alice".to_string(), hex::encode([1u8; 32]));
            map.insert("bob".to_string(), hex::encode([2u8; 32]));
            map.insert("carol".to_string(), hex::encode([3u8; 32]));
            map
        })
        .clone()
}

/// `vesting_names("{}")` -> `{"ok":true,"names":{"alice":"<hex account>",...}}`.
#[no_mangle]
pub extern "C" fn vesting_names(_params: *const c_char) -> *mut c_char {
    to_c(json!({ "ok": true, "names": name_map() }))
}

/// `vesting_resolve_name("{\"name\":\"alice\"}")` -> the account, or a clear error.
#[no_mangle]
pub extern "C" fn vesting_resolve_name(raw: *const c_char) -> *mut c_char {
    let p = params(raw);
    let name = p
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    match name_map().get(&name) {
        Some(account) => to_c(json!({ "ok": true, "name": name, "account": account })),
        None => to_c(fail(format!("no LNS name '{name}' is known to this mock"))),
    }
}

// ── Tests: call the C ABI with JSON, check the JSON out ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    // The global Client is shared, so tests take a lock rather than run in parallel on it.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    type Abi = extern "C" fn(*const c_char) -> *mut c_char;

    fn call(function: Abi, input: &str) -> Value {
        let input = CString::new(input).unwrap();
        let out = function(input.as_ptr());
        assert!(!out.is_null(), "ABI returned NULL");
        let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_owned();
        unsafe { vesting_free(out) };
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
    }

    fn fresh(now: u64, decimals: u8) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vesting-module-{}-{nanos}.json",
            std::process::id()
        ));
        json!({ "state_path": path.to_string_lossy(), "now": now, "decimals": decimals })
            .to_string()
    }

    fn hex_id(byte: u8) -> String {
        hex::encode([byte; 32])
    }

    fn linear(start: u64, end: u64) -> Value {
        json!({ "type": "linear", "start": start, "end": end })
    }

    fn create(total: &str, kind: Value) -> String {
        json!({
            "creator": hex_id(1), "beneficiary": hex_id(2), "token": hex_id(9),
            "total": total, "kind": kind, "cancelable": true, "transferable": true
        })
        .to_string()
    }

    fn caller() -> String {
        hex_id(2)
    }

    #[test]
    fn init_now_and_empty_list() {
        let _guard = TEST_LOCK.lock().unwrap();
        assert_eq!(call(vesting_init, &fresh(500, 9))["ok"], true);
        assert_eq!(call(vesting_now, "{}")["time"], 500);
        assert_eq!(
            call(vesting_list, "{}")["schedules"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn create_then_list_roundtrip() {
        let _guard = TEST_LOCK.lock().unwrap();
        call(vesting_init, &fresh(500, 9));
        let made = call(vesting_create, &create("1000", linear(0, 1000)));
        assert_eq!(made["ok"], true);
        assert_eq!(made["id"], 0);
        let listed = call(vesting_list, "{}");
        let schedules = listed["schedules"].as_array().unwrap();
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0]["total"], "1000");
        assert_eq!(schedules[0]["kind"]["type"], "linear");
        assert_eq!(schedules[0]["claimed"], "0");
    }

    #[test]
    fn summary_and_public_claim_through_sdk() {
        let _guard = TEST_LOCK.lock().unwrap();
        call(vesting_init, &fresh(500, 9));
        call(vesting_create, &create("1000", linear(0, 1000)));
        let summary = call(
            vesting_pre_claim_summary,
            r#"{"id":0,"destination_kind":"public"}"#,
        );
        assert_eq!(summary["ok"], true);
        assert_eq!(summary["summary"]["claimable"], "500");
        assert_eq!(summary["summary"]["estimated_fee"], "0");
        let claim = call(
            vesting_claim_public,
            &json!({ "id": 0, "caller": caller() }).to_string(),
        );
        assert_eq!(claim["ok"], true);
        assert_eq!(claim["amount"], "500");
        assert_eq!(call(vesting_list, "{}")["schedules"][0]["claimed"], "500");
        // Nothing left: the SDK's actionable error comes back with its code.
        let again = call(
            vesting_claim_public,
            &json!({ "id": 0, "caller": caller() }).to_string(),
        );
        assert_eq!(again["ok"], false);
        assert!(again["error"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("claimable"));
        assert!(again["code"].is_number());
    }

    #[test]
    fn private_claim_rejects_a_public_target() {
        let _guard = TEST_LOCK.lock().unwrap();
        call(vesting_init, &fresh(500, 9));
        call(vesting_create, &create("1000", linear(0, 1000)));
        let wrong = call(
            vesting_claim_private,
            &json!({ "id": 0, "caller": caller(), "target": hex_id(5), "is_private": false })
                .to_string(),
        );
        assert_eq!(wrong["ok"], false);
        assert_eq!(wrong["code"], 1001); // SdkError::TargetNotPrivate
        assert!(wrong["error"].as_str().unwrap().contains("private"));
        let right = call(
            vesting_claim_private,
            &json!({ "id": 0, "caller": caller(), "target": hex_id(5), "is_private": true })
                .to_string(),
        );
        assert_eq!(right["ok"], true);
        assert_eq!(right["amount"], "500");
    }

    #[test]
    fn batch_creates_every_row_in_one_call() {
        let _guard = TEST_LOCK.lock().unwrap();
        call(vesting_init, &fresh(500, 9));
        let rows = json!([
            { "creator": hex_id(1), "beneficiary": hex_id(2), "token": hex_id(9), "total": "100", "kind": linear(0, 100) },
            { "creator": hex_id(1), "beneficiary": hex_id(3), "token": hex_id(9), "total": "200", "kind": linear(0, 200) }
        ]).to_string();
        let made = call(vesting_create_batch, &rows);
        assert_eq!(made["ok"], true);
        assert_eq!(made["ids"], json!([0, 1]));
        assert_eq!(
            call(vesting_list, "{}")["schedules"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn signal_cancel_irrevocable_and_transfer() {
        let _guard = TEST_LOCK.lock().unwrap();
        call(vesting_init, &fresh(500, 9));
        let milestone = json!({ "type": "milestone", "tranches": ["40", "60"] });
        call(vesting_create, &create("100", milestone));
        let signalled = call(
            vesting_signal,
            &json!({ "id": 0, "caller": hex_id(1), "index": 0 }).to_string(),
        );
        assert_eq!(signalled["ok"], true);
        assert_eq!(signalled["amount"], "40");
        let again = call(
            vesting_signal,
            &json!({ "id": 0, "caller": hex_id(1), "index": 0 }).to_string(),
        );
        assert_eq!(again["ok"], false);
        assert!(again["error"].as_str().unwrap().contains("twice"));
        let cancel = call(
            vesting_cancel,
            &json!({ "id": 0, "caller": hex_id(1) }).to_string(),
        );
        assert_eq!(cancel["ok"], true);
        assert_eq!(cancel["refund"], "60");
        let irrevocable = call(
            vesting_make_irrevocable,
            &json!({ "id": 0, "caller": hex_id(1) }).to_string(),
        );
        // Already cancelled: the SDK error passes through, not a panic.
        assert_eq!(irrevocable["ok"], false);
        let transfer = call(
            vesting_transfer,
            &json!({ "id": 0, "caller": hex_id(2), "new_beneficiary": hex_id(4) }).to_string(),
        );
        assert_eq!(transfer["ok"], true);
    }

    #[test]
    fn bad_input_is_a_plain_error() {
        let _guard = TEST_LOCK.lock().unwrap();
        call(vesting_init, &fresh(500, 9));
        let bad = call(vesting_create, &create("not-a-number", linear(0, 1000)));
        assert_eq!(bad["ok"], false);
        assert!(bad["error"].as_str().unwrap().contains("total"));
        let no_id = call(vesting_pre_claim_summary, "{}");
        assert_eq!(no_id["ok"], false);
        assert!(no_id["error"].as_str().unwrap().contains("id"));
    }

    #[test]
    fn amounts_arrive_ready_formatted_from_the_module() {
        let _guard = TEST_LOCK.lock().unwrap();
        call(vesting_init, &fresh(500, 2)); // 2 decimals: 1000 -> 10 LEZ
        call(vesting_create, &create("1000", linear(0, 1000)));
        let listed = call(vesting_list, "{}");
        let schedule = &listed["schedules"][0];
        assert_eq!(schedule["total"], "1000"); // raw decimal kept
        assert_eq!(schedule["total_display"], "10 LEZ");
        assert_eq!(schedule["claimable"], "500");
        assert_eq!(schedule["claimable_display"], "5 LEZ");
        let summary = call(
            vesting_pre_claim_summary,
            r#"{"id":0,"destination_kind":"public"}"#,
        );
        assert_eq!(summary["summary"]["claimable_display"], "5 LEZ");
        assert_eq!(summary["summary"]["estimated_fee_display"], "0 LEZ");
        let claim = call(
            vesting_claim_public,
            &json!({ "id": 0, "caller": caller() }).to_string(),
        );
        assert_eq!(claim["amount_display"], "5 LEZ");
        assert_eq!(claim["claimed_total_display"], "5 LEZ");
    }

    #[test]
    fn name_seam_resolves_and_lists() {
        let names = call(vesting_names, "{}");
        assert_eq!(names["ok"], true);
        assert!(names["names"]["alice"].as_str().unwrap().len() == 64);
        let found = call(vesting_resolve_name, r#"{"name":"ALICE"}"#);
        assert_eq!(found["ok"], true);
        assert_eq!(found["name"], "alice");
        let missing = call(vesting_resolve_name, r#"{"name":"nobody"}"#);
        assert_eq!(missing["ok"], false);
    }

    #[test]
    fn all_eleven_exports_answer_with_json() {
        let _guard = TEST_LOCK.lock().unwrap();
        call(vesting_init, &fresh(500, 9));
        call(vesting_create, &create("100", linear(0, 100)));
        let with_id = json!({ "id": 0, "caller": hex_id(1) }).to_string();
        let list: [Abi; 11] = [
            vesting_list,
            vesting_now,
            vesting_pre_claim_summary,
            vesting_create,
            vesting_create_batch,
            vesting_claim_public,
            vesting_claim_private,
            vesting_cancel,
            vesting_make_irrevocable,
            vesting_signal,
            vesting_transfer,
        ];
        for function in list {
            let answer = call(function, &with_id);
            assert!(answer.get("ok").is_some(), "an export answered without ok");
        }
    }
}
