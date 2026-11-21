//! A local relay in front of a public LEZ RPC (feature `guard`): pacing, retries on HTTP 429,
//! and plain words instead of whatever the network's edge answers.
//!
//! The public RPC sits behind a rate limit (more than 50 requests in 10 s from one address are
//! refused for 10 s with an HTML page and HTTP 429), and every app at home shares one address.
//! Everything an app sends — the LEZ wallet's own calls, LNS lookups, the chain check — goes to
//! this relay on 127.0.0.1 instead, which:
//!
//! - sends at most `per_second` requests upstream (one queue for the whole process);
//! - retries a 429 or 503 up to `tries` times, waiting `Retry-After` when the edge gives one,
//!   else 2, 4, 8 s;
//! - never resends a transaction blindly: before resending a `sendTransaction` it asks
//!   `getTransaction` for the transaction's hash (from `tx_hash`), and answers with that hash if
//!   the transaction is already there;
//! - never passes a response body on: an answer that isn't JSON-RPC becomes a JSON-RPC error
//!   whose message is plain words and whose `data.detail` is the status and at most 200
//!   characters of the body, HTML stripped.
//!
//! It counts what it sends, per method (`stats`), so costs can be measured.

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The words an app shows (the messages of the relay's JSON-RPC errors).
pub const BUSY_RETRYING: &str = "The network is busy. Trying again in a few seconds.";
pub const STILL_BUSY: &str = "Still busy. Try again in a minute.";
pub const UNREACHABLE: &str = "Can't reach the network right now.";
/// The detail of the last problem any relay in this process met, for an error whose wrapper
/// dropped the JSON-RPC error's data (see `plain_error`).
static LAST_PROBLEM: Mutex<Option<String>> = Mutex::new(None);

fn note_problem(inner: &Inner, text: String) {
    inner.state.lock().expect("guard state").last_problem = Some(text.clone());
    if let Ok(mut last) = LAST_PROBLEM.lock() {
        *last = Some(text);
    }
}

/// The JSON-RPC error code of the relay's own errors.
pub const GUARD_ERROR_CODE: i64 = -32099;

/// Computes the JSON form of a `sendTransaction`'s transaction hash from its params, as
/// `getTransaction` takes it. `None`: unknown, so the relay never resends that write.
pub type TxHash = dyn Fn(&Value) -> Option<Value> + Send + Sync;

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub per_second: u32,
    pub tries: u32,
    /// Waits when the edge gives no `Retry-After` (doubling).
    pub first_wait: Duration,
    /// Never wait longer than this for one retry.
    pub max_wait: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            per_second: 3,
            tries: 4,
            first_wait: Duration::from_secs(2),
            max_wait: Duration::from_secs(15),
        }
    }
}

#[derive(Default)]
struct State {
    next_slot: Option<Instant>,
    sent: BTreeMap<String, u64>,
    refused: u64,
    last_problem: Option<String>,
}

struct Inner {
    upstream: String,
    policy: Policy,
    tx_hash: Option<Box<TxHash>>,
    state: Mutex<State>,
    agent: ureq::Agent,
}

/// A running relay. Dropping it leaves the thread serving until the process ends.
#[derive(Clone)]
pub struct RpcGuard {
    url: String,
    inner: Arc<Inner>,
}

/// Status and at most 200 characters of a body, tags removed, for Technical details.
pub fn detail(status: u16, body: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in body.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(if ch.is_control() { ' ' } else { ch }),
            _ => {}
        }
    }
    let squeezed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = squeezed.chars().take(200).collect();
    if status == 0 {
        cut
    } else {
        format!("HTTP {status}: {cut}").trim_end().to_string()
    }
}

impl RpcGuard {
    /// Start a relay to `upstream` (http or https) on 127.0.0.1, on a free port.
    pub fn start(
        upstream: &str,
        policy: Policy,
        tx_hash: Option<Box<TxHash>>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}/", listener.local_addr()?);
        let inner = Arc::new(Inner {
            upstream: upstream.to_string(),
            policy,
            tx_hash,
            state: Mutex::new(State::default()),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .build(),
        });
        let serving = inner.clone();
        std::thread::Builder::new()
            .name("vest-rpc-guard".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let inner = serving.clone();
                    let _ = std::thread::Builder::new()
                        .name("vest-rpc-guard-conn".into())
                        .spawn(move || serve(&inner, stream));
                }
            })?;
        Ok(Self { url, inner })
    }

    /// Where to send requests instead of the upstream RPC.
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn upstream(&self) -> &str {
        &self.inner.upstream
    }

    /// Requests sent upstream so far, per JSON-RPC method (retries and checks included), and
    /// how many 429/503 answers came back.
    pub fn stats(&self) -> (BTreeMap<String, u64>, u64) {
        let state = self.inner.state.lock().expect("guard state");
        (state.sent.clone(), state.refused)
    }

    pub fn reset_stats(&self) {
        let mut state = self.inner.state.lock().expect("guard state");
        state.sent.clear();
        state.refused = 0;
    }

    /// The last problem the relay met (plain detail), cleared by `take`.
    pub fn take_problem(&self) -> Option<String> {
        self.inner
            .state
            .lock()
            .expect("guard state")
            .last_problem
            .take()
    }
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Option<(Vec<u8>, bool)> {
    let mut length = 0usize;
    let mut close = false;
    let mut line = String::new();
    // Request line.
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    loop {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "content-length" {
                length = value.parse().ok()?;
            } else if name == "connection" && value.eq_ignore_ascii_case("close") {
                close = true;
            }
        }
    }
    let mut body = vec![0u8; length.min(64 * 1024 * 1024)];
    reader.read_exact(&mut body).ok()?;
    Some((body, close))
}

fn serve(inner: &Inner, stream: TcpStream) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    while let Some((body, close)) = read_request(&mut reader) {
        let answer = relay(inner, &body);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}\r\n",
            answer.len(),
            if close { "Connection: close\r\n" } else { "" }
        );
        if out
            .write_all(head.as_bytes())
            .and_then(|_| out.write_all(&answer))
            .is_err()
            || close
        {
            return;
        }
    }
}

/// The error a client gets in place of an unusable answer.
fn guard_error(id: &Value, message: &str, detail: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id,
            "error": { "code": GUARD_ERROR_CODE, "message": message, "data": { "detail": detail } } })
}

fn methods(request: &Value) -> Vec<String> {
    let one = |r: &Value| {
        r.get("method")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string()
    };
    match request {
        Value::Array(items) => items.iter().map(one).collect(),
        other => vec![one(other)],
    }
}

fn ids(request: &Value) -> Vec<Value> {
    match request {
        Value::Array(items) => items
            .iter()
            .map(|r| r.get("id").cloned().unwrap_or(Value::Null))
            .collect(),
        other => vec![other.get("id").cloned().unwrap_or(Value::Null)],
    }
}

/// Wait for this process's next upstream slot.
fn pace(inner: &Inner) {
    let gap = Duration::from_millis(1000 / u64::from(inner.policy.per_second.max(1)));
    let wait = {
        let mut state = inner.state.lock().expect("guard state");
        let now = Instant::now();
        let slot = state.next_slot.map_or(now, |next| next.max(now));
        state.next_slot = Some(slot + gap);
        slot.saturating_duration_since(now)
    };
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
}

enum Upstream {
    Answer(Vec<u8>),
    Busy {
        wait: Option<Duration>,
        detail: String,
    },
    Failed {
        status: u16,
        detail: String,
    },
    Unreachable(String),
}

fn send_upstream(inner: &Inner, body: &[u8], request: &Value) -> Upstream {
    pace(inner);
    {
        let mut state = inner.state.lock().expect("guard state");
        for method in methods(request) {
            *state.sent.entry(method).or_default() += 1;
        }
    }
    let answer = inner
        .agent
        .post(&inner.upstream)
        .set("Content-Type", "application/json")
        .send_bytes(body);
    let read = |response: ureq::Response| -> Vec<u8> {
        let mut bytes = Vec::new();
        let _ = response
            .into_reader()
            .take(4 * 1024 * 1024)
            .read_to_end(&mut bytes);
        bytes
    };
    match answer {
        Ok(response) => {
            let bytes = read(response);
            if serde_json::from_slice::<Value>(&bytes).is_ok() {
                Upstream::Answer(bytes)
            } else {
                Upstream::Failed {
                    status: 200,
                    detail: detail(200, &String::from_utf8_lossy(&bytes)),
                }
            }
        }
        Err(ureq::Error::Status(code, response)) if code == 429 || code == 503 => {
            let wait = response
                .header("Retry-After")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            let text = String::from_utf8_lossy(&read(response)).to_string();
            inner.state.lock().expect("guard state").refused += 1;
            Upstream::Busy {
                wait,
                detail: detail(code, &text),
            }
        }
        Err(ureq::Error::Status(code, response)) => {
            let bytes = read(response);
            // A JSON-RPC error with a non-200 status is still the server's own answer.
            if serde_json::from_slice::<Value>(&bytes).is_ok() {
                Upstream::Answer(bytes)
            } else {
                Upstream::Failed {
                    status: code,
                    detail: detail(code, &String::from_utf8_lossy(&bytes)),
                }
            }
        }
        Err(error) => Upstream::Unreachable(detail(0, &error.to_string())),
    }
}

/// Whether a write already reached the chain: `getTransaction` for its hash.
fn already_sent(inner: &Inner, hash: &Value) -> bool {
    let check = json!({ "jsonrpc": "2.0", "id": "vest-guard-check", "method": "getTransaction", "params": [hash] });
    let body = serde_json::to_vec(&check).expect("json");
    match send_upstream(inner, &body, &check) {
        Upstream::Answer(bytes) => serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v.get("result").cloned())
            .is_some_and(|result| !result.is_null()),
        _ => false,
    }
}

fn relay(inner: &Inner, body: &[u8]) -> Vec<u8> {
    let request: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => {
            return serde_json::to_vec(&guard_error(
                &Value::Null,
                "Bad request to the network relay.",
                "not JSON",
            ))
            .expect("json")
        }
    };
    // The hash of a single sendTransaction, when known: the only write that may be resent, and
    // only after the chain says it doesn't have it.
    let write_hash = match &request {
        Value::Object(_)
            if request.get("method").and_then(Value::as_str) == Some("sendTransaction") =>
        {
            Some(
                inner
                    .tx_hash
                    .as_ref()
                    .and_then(|hash| hash(request.get("params").unwrap_or(&Value::Null))),
            )
        }
        _ => None,
    };
    let mut wait = inner.policy.first_wait;
    let mut last_detail = String::new();
    for attempt in 1..=inner.policy.tries.max(1) {
        if attempt > 1 {
            if let Some(hash) = &write_hash {
                let Some(hash) = hash else {
                    // No hash: never resend a write we can't check.
                    break;
                };
                if already_sent(inner, hash) {
                    let id = request.get("id").cloned().unwrap_or(Value::Null);
                    return serde_json::to_vec(
                        &json!({ "jsonrpc": "2.0", "id": id, "result": hash }),
                    )
                    .expect("json");
                }
            }
        }
        match send_upstream(inner, body, &request) {
            Upstream::Answer(bytes) => return bytes,
            Upstream::Busy { wait: told, detail } => {
                last_detail = detail.clone();
                note_problem(inner, detail.clone());
                if attempt < inner.policy.tries {
                    std::thread::sleep(told.unwrap_or(wait).min(inner.policy.max_wait));
                    wait = (wait * 2).min(inner.policy.max_wait);
                }
            }
            Upstream::Failed { status, detail } => {
                note_problem(inner, detail.clone());
                return error_body(
                    &request,
                    &format!("The network answered with an error (HTTP {status})."),
                    &detail,
                );
            }
            Upstream::Unreachable(detail) => {
                note_problem(inner, detail.clone());
                return error_body(&request, UNREACHABLE, &detail);
            }
        }
    }
    note_problem(inner, last_detail.clone());
    error_body(&request, STILL_BUSY, &last_detail)
}

fn error_body(request: &Value, message: &str, detail: &str) -> Vec<u8> {
    let errors: Vec<Value> = ids(request)
        .iter()
        .map(|id| guard_error(id, message, detail))
        .collect();
    let value = if request.is_array() {
        Value::Array(errors)
    } else {
        errors.into_iter().next().unwrap_or(Value::Null)
    };
    serde_json::to_vec(&value).expect("json")
}

/// The plain words of an error that passed through the relay, and its detail: the relay's
/// messages are recognised wherever they are quoted; anything else keeps its own text, with
/// tags removed and cut to 300 characters.
pub fn plain_error(text: &str) -> (String, Option<String>) {
    for words in [STILL_BUSY, UNREACHABLE, BUSY_RETRYING] {
        if text.contains(words) {
            let detail = text
                .split("detail")
                .nth(1)
                .map(|rest| detail(0, rest.trim_start_matches(|c: char| !c.is_alphanumeric())))
                .filter(|d| !d.is_empty())
                .or_else(|| LAST_PROBLEM.lock().ok().and_then(|mut last| last.take()));
            return (words.to_string(), detail);
        }
    }
    if text.contains("The network answered with an error") {
        return (
            "The network answered with an error. Try again in a minute.".into(),
            Some(detail(0, text)),
        );
    }
    if text.contains('<') && text.contains('>') {
        return (
            "The network gave an answer Vest can't read. Try again in a minute.".into(),
            Some(detail(0, text)),
        );
    }
    (text.chars().take(300).collect(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fake upstream: answers each request with the next scripted reply (status, headers,
    /// body); records the methods it saw.
    fn fake(script: Vec<(u16, &'static str, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let step = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut out = stream;
                while let Some((body, _)) = read_request(&mut reader) {
                    let request: Value = serde_json::from_slice(&body).unwrap();
                    log.lock().unwrap().push(methods(&request).join(","));
                    let i = step.fetch_add(1, Ordering::SeqCst).min(script.len() - 1);
                    let (status, headers, body) = &script[i];
                    let reason = if *status == 200 {
                        "OK"
                    } else {
                        "Too Many Requests"
                    };
                    let head = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n{headers}\r\n",
                        body.len()
                    );
                    if out
                        .write_all(head.as_bytes())
                        .and_then(|_| out.write_all(body.as_bytes()))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
        (url, seen)
    }

    const HTML_429: &str = "<!DOCTYPE html><html><head><title>Rate limited</title></head><body><h1>Error 1015</h1><p>You are being rate limited.</p></body></html>";

    fn quick() -> Policy {
        Policy {
            per_second: 50,
            tries: 4,
            first_wait: Duration::from_millis(20),
            max_wait: Duration::from_millis(50),
        }
    }

    fn post(url: &str, request: &Value) -> Value {
        let text = ureq::post(url)
            .set("Content-Type", "application/json")
            .send_string(&request.to_string())
            .unwrap()
            .into_string()
            .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn a_429_is_retried_and_the_answer_passes_through() {
        let ok = r#"{"jsonrpc":"2.0","id":1,"result":"2da7"}"#.to_string();
        let (up, seen) = fake(vec![
            (429, "Retry-After: 0\r\n", HTML_429.into()),
            (429, "", HTML_429.into()),
            (200, "", ok),
        ]);
        let guard = RpcGuard::start(&up, quick(), None).unwrap();
        let answer = post(
            guard.url(),
            &json!({"jsonrpc":"2.0","id":1,"method":"getChannelId","params":[]}),
        );
        assert_eq!(answer["result"], "2da7");
        assert_eq!(seen.lock().unwrap().len(), 3);
        let (sent, refused) = guard.stats();
        assert_eq!(sent["getChannelId"], 3);
        assert_eq!(refused, 2);
    }

    #[test]
    fn still_busy_is_plain_words_never_the_html() {
        let (up, _) = fake(vec![(429, "", HTML_429.into())]);
        let guard = RpcGuard::start(&up, quick(), None).unwrap();
        let answer = post(
            guard.url(),
            &json!({"jsonrpc":"2.0","id":7,"method":"getAccount","params":["x"]}),
        );
        assert_eq!(answer["id"], 7);
        assert_eq!(answer["error"]["message"], STILL_BUSY);
        let text = answer.to_string();
        assert!(!text.contains("<html") && !text.contains("<h1>"), "{text}");
        let d = answer["error"]["data"]["detail"].as_str().unwrap();
        assert!(d.starts_with("HTTP 429: Rate limited Error 1015"), "{d}");
        assert!(d.chars().count() <= 210);
        assert_eq!(guard.stats().0["getAccount"], 4); // tries
    }

    #[test]
    fn a_write_is_checked_by_hash_before_it_is_resent() {
        // The first send is refused; the chain already has the transaction (an earlier attempt
        // went through): the relay answers with its hash and does not send it again.
        let found =
            r#"{"jsonrpc":"2.0","id":"vest-guard-check","result":{"tx":"..."}}"#.to_string();
        let (up, seen) = fake(vec![
            (429, "Retry-After: 0\r\n", HTML_429.into()),
            (200, "", found),
        ]);
        let hook: Box<TxHash> = Box::new(|_params| Some(json!("abcd")));
        let guard = RpcGuard::start(&up, quick(), Some(hook)).unwrap();
        let answer = post(
            guard.url(),
            &json!({"jsonrpc":"2.0","id":3,"method":"sendTransaction","params":[{"Public":{}}]}),
        );
        assert_eq!(answer["result"], "abcd");
        assert_eq!(
            *seen.lock().unwrap(),
            vec!["sendTransaction".to_string(), "getTransaction".to_string()]
        );
    }

    #[test]
    fn a_write_not_on_chain_is_resent_once_checked() {
        let none = r#"{"jsonrpc":"2.0","id":"vest-guard-check","result":null}"#.to_string();
        let sent = r#"{"jsonrpc":"2.0","id":3,"result":"abcd"}"#.to_string();
        let (up, seen) = fake(vec![
            (429, "Retry-After: 0\r\n", HTML_429.into()),
            (200, "", none),
            (200, "", sent),
        ]);
        let hook: Box<TxHash> = Box::new(|_params| Some(json!("abcd")));
        let guard = RpcGuard::start(&up, quick(), Some(hook)).unwrap();
        let answer = post(
            guard.url(),
            &json!({"jsonrpc":"2.0","id":3,"method":"sendTransaction","params":[{}]}),
        );
        assert_eq!(answer["result"], "abcd");
        assert_eq!(
            seen.lock().unwrap().join(" "),
            "sendTransaction getTransaction sendTransaction"
        );
    }

    #[test]
    fn a_write_without_a_hash_is_never_resent() {
        let (up, seen) = fake(vec![(429, "", HTML_429.into())]);
        let guard = RpcGuard::start(&up, quick(), None).unwrap();
        let answer = post(
            guard.url(),
            &json!({"jsonrpc":"2.0","id":3,"method":"sendTransaction","params":[{}]}),
        );
        assert_eq!(answer["error"]["message"], STILL_BUSY);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn unreachable_and_html_errors_are_plain() {
        let guard = RpcGuard::start("http://127.0.0.1:9/", quick(), None).unwrap();
        let answer = post(
            guard.url(),
            &json!({"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}),
        );
        assert_eq!(answer["error"]["message"], UNREACHABLE);
        let (up, _) = fake(vec![(
            200,
            "",
            "<html><body>Bad gateway</body></html>".into(),
        )]);
        let guard = RpcGuard::start(&up, quick(), None).unwrap();
        let answer = post(
            guard.url(),
            &json!({"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}),
        );
        assert!(answer["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("The network answered with an error"));
        assert_eq!(answer["error"]["data"]["detail"], "HTTP 200: Bad gateway");
    }

    #[test]
    fn requests_are_paced() {
        let ok = r#"{"jsonrpc":"2.0","id":1,"result":1}"#.to_string();
        let (up, _) = fake(vec![(200, "", ok)]);
        let policy = Policy {
            per_second: 10,
            ..quick()
        };
        let guard = RpcGuard::start(&up, policy, None).unwrap();
        let started = Instant::now();
        for _ in 0..6 {
            post(
                guard.url(),
                &json!({"jsonrpc":"2.0","id":1,"method":"getLastBlockId","params":[]}),
            );
        }
        // Six requests at 10 a second take at least 0.5 s.
        assert!(
            started.elapsed() >= Duration::from_millis(480),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn plain_error_finds_the_words_in_wrapped_errors() {
        let wrapped = format!("RPC call failed: ErrorObject {{ code: ServerError(-32099), message: \"{STILL_BUSY}\", data: Some(RawValue({{\"detail\":\"HTTP 429: Rate limited\"}})) }}");
        let (words, detail) = plain_error(&wrapped);
        assert_eq!(words, STILL_BUSY);
        assert!(detail.unwrap().contains("HTTP 429: Rate limited"));
        let (words, detail) = plain_error("bad <b>html</b> answer");
        assert!(words.contains("can't read"));
        assert_eq!(detail.unwrap(), "bad html answer");
        assert_eq!(
            plain_error("no wallet at /x"),
            ("no wallet at /x".to_string(), None)
        );
    }
}
