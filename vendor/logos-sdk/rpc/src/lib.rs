//! The one RPC door for Logos: retries, 429 pacing, batch fallback and a mock.
//!
//! The transport here is a small synchronous JSON-RPC client (ureq). The
//! upstream `sequencer_service_rpc::SequencerClient` is the async transport to
//! adopt when an async facade is needed; the retry and pacing policy in this
//! module is independent of the transport and comes from Names Wallet and the
//! program-registry SDK (see `docs/INVENTORY.md`).

pub use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Errors from an RPC call. Variants are stable so callers can branch on them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RpcError {
    Transport(String),
    Parse(String),
    Server { code: i64, message: String },
    RateLimited { retry_after_ms: Option<u64> },
    MempoolFull,
    PrivateFeeBelowHead,
    BatchUnsupported,
    BatchRejected,
    RetriesExhausted { attempts: u32, message: String },
}

impl RpcError {
    /// Is this error worth retrying? Rate limits and transport failures are;
    /// a protocol refusal is not.
    pub fn is_retryable(&self) -> bool {
        matches!(self, RpcError::Transport(_) | RpcError::RateLimited { .. })
    }

    /// Classify a JSON-RPC error object. Codes -31900 and -31901 come from
    /// upstream `sequencer_service_rpc`.
    pub fn from_server(code: i64, message: String) -> Self {
        match code {
            -31900 => RpcError::MempoolFull,
            -31901 => RpcError::PrivateFeeBelowHead,
            -32005 => RpcError::RateLimited {
                retry_after_ms: None,
            },
            _ => RpcError::Server { code, message },
        }
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcError::Transport(message) => write!(f, "transport error: {}", message),
            RpcError::Parse(message) => write!(f, "bad RPC response: {}", message),
            RpcError::Server { code, message } => write!(f, "RPC error {}: {}", code, message),
            RpcError::RateLimited { .. } => write!(f, "rate limited by the sequencer"),
            RpcError::MempoolFull => write!(f, "the sequencer mempool is full"),
            RpcError::PrivateFeeBelowHead => {
                write!(f, "the private fee is below the current head")
            }
            RpcError::BatchUnsupported => write!(f, "the endpoint does not support batches"),
            RpcError::BatchRejected => write!(f, "the endpoint refused the batch"),
            RpcError::RetriesExhausted { attempts, message } => {
                write!(f, "{} attempts failed: {}", attempts, message)
            }
        }
    }
}

impl std::error::Error for RpcError {}

/// Exponential backoff: `first_ms * 2^(attempt-1)`, capped at `cap_ms`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub first_ms: u64,
    pub cap_ms: u64,
}

impl Backoff {
    pub const DEFAULT: Self = Self {
        first_ms: 2_000,
        cap_ms: 10_000,
    };

    pub fn delay_ms(&self, attempt: u32) -> u64 {
        let shift = attempt.saturating_sub(1).min(20);
        self.first_ms.saturating_mul(1u64 << shift).min(self.cap_ms)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub attempts: u32,
    pub backoff: Backoff,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 4,
            backoff: Backoff::DEFAULT,
        }
    }
}

/// Token bucket. Names Wallet uses 4 requests/s with a burst of 8 and skips
/// loopback endpoints; `Pace::LOCAL` disables waiting for local tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pace {
    pub rate_per_sec: u32,
    pub burst: u32,
}

impl Pace {
    pub const LOCAL: Self = Self {
        rate_per_sec: u32::MAX,
        burst: u32::MAX,
    };
    pub const REMOTE: Self = Self {
        rate_per_sec: 4,
        burst: 8,
    };
}

/// Injectable clock so pacing can be tested without sleeping.
pub trait Now: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub struct SystemNow;
impl Now for SystemNow {
    fn now_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

/// Sleep is injected too: tests use a recorder that does not actually wait.
pub trait Sleep: Send + Sync {
    fn sleep_ms(&self, ms: u64);
    fn waits(&self) -> usize;
}

pub struct ThreadSleep;
impl Sleep for ThreadSleep {
    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    fn waits(&self) -> usize {
        0
    }
}

/// A recorded JSON-RPC call, used by `MockRpc::calls`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub method: String,
    pub params: Value,
}

/// A transport is the only thing the client needs from the network.
pub trait Transport: Send + Sync {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError>;
    fn call_batch(&self, method: &str, params: &[Value]) -> Result<Vec<Value>, RpcError> {
        let _ = (method, params);
        Err(RpcError::BatchUnsupported)
    }
}

struct MockState {
    responses: HashMap<String, VecDeque<Result<Value, RpcError>>>,
    batch: VecDeque<Result<Vec<Value>, RpcError>>,
    calls: Vec<Call>,
    batch_calls: usize,
}

/// Scriptable transport for tests. Responses are queued per method.
pub struct MockRpc {
    state: Mutex<MockState>,
}

impl MockRpc {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MockState {
                responses: HashMap::new(),
                batch: VecDeque::new(),
                calls: Vec::new(),
                batch_calls: 0,
            }),
        }
    }

    pub fn push(&self, method: &str, response: Result<Value, RpcError>) {
        self.state
            .lock()
            .unwrap()
            .responses
            .entry(method.to_string())
            .or_default()
            .push_back(response);
    }

    pub fn push_ok(&self, method: &str, value: Value) {
        self.push(method, Ok(value));
    }

    pub fn push_batch(&self, response: Result<Vec<Value>, RpcError>) {
        self.state.lock().unwrap().batch.push_back(response);
    }

    pub fn calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().calls.clone()
    }

    pub fn call_count(&self, method: &str) -> usize {
        self.calls()
            .iter()
            .filter(|call| call.method == method)
            .count()
    }

    pub fn batch_calls(&self) -> usize {
        self.state.lock().unwrap().batch_calls
    }
}

impl Default for MockRpc {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for MockRpc {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let mut state = self.state.lock().unwrap();
        state.calls.push(Call {
            method: method.to_string(),
            params,
        });
        match state.responses.get_mut(method) {
            Some(queue) => queue.pop_front().unwrap_or_else(|| {
                Err(RpcError::Transport(format!(
                    "no queued response for {}",
                    method
                )))
            }),
            None => Err(RpcError::Transport(format!(
                "no queued response for {}",
                method
            ))),
        }
    }

    fn call_batch(&self, method: &str, params: &[Value]) -> Result<Vec<Value>, RpcError> {
        let mut state = self.state.lock().unwrap();
        state.batch_calls += 1;
        state.calls.push(Call {
            method: format!("batch:{}", method),
            params: Value::Array(params.to_vec()),
        });
        state
            .batch
            .pop_front()
            .unwrap_or(Err(RpcError::BatchUnsupported))
    }
}

pub struct RecordingSleep {
    count: Mutex<usize>,
}
impl RecordingSleep {
    pub fn new() -> Self {
        Self {
            count: Mutex::new(0),
        }
    }
}
impl Default for RecordingSleep {
    fn default() -> Self {
        Self::new()
    }
}
impl Sleep for RecordingSleep {
    fn sleep_ms(&self, _ms: u64) {
        *self.count.lock().unwrap() += 1;
    }
    fn waits(&self) -> usize {
        *self.count.lock().unwrap()
    }
}

/// Client with retry, pacing and a single place for observability.
pub struct Client {
    transport: Arc<dyn Transport>,
    retry: RetryPolicy,
    pace: Pace,
    now: Arc<dyn Now>,
    sleep: Arc<dyn Sleep>,
    bucket: Mutex<(u64, f64)>, // (last refill time, tokens)
}

impl Client {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self::with_policy(transport, RetryPolicy::default(), Pace::LOCAL)
    }

    pub fn with_policy(transport: Arc<dyn Transport>, retry: RetryPolicy, pace: Pace) -> Self {
        let burst = pace.burst as f64;
        Self {
            transport,
            retry,
            pace,
            now: Arc::new(SystemNow),
            sleep: Arc::new(ThreadSleep),
            bucket: Mutex::new((0, burst)),
        }
    }

    pub fn with_clock_and_sleep(mut self, now: Arc<dyn Now>, sleep: Arc<dyn Sleep>) -> Self {
        self.now = now;
        self.sleep = sleep;
        self
    }

    /// Wait for a token before making a request. With `Pace::LOCAL` this
    /// returns immediately.
    fn take_token(&self) {
        let (last, tokens) = *self.bucket.lock().unwrap();
        let now = self.now.now_ms();
        let elapsed = now.saturating_sub(last) as f64 / 1_000.0;
        let refill = elapsed * self.pace.rate_per_sec as f64;
        let available = (tokens + refill).min(self.pace.burst as f64);
        let mut bucket = self.bucket.lock().unwrap();
        if available >= 1.0 {
            *bucket = (now, available - 1.0);
            return;
        }
        // Not enough tokens: record a wait and advance the clock past the
        // next refill. Tests use an injectable clock, so no real sleep occurs.
        self.sleep.sleep_ms(1_000);
        let waited = self.now.now_ms();
        *bucket = (waited, 1.0);
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let mut last_error = None;
        for attempt in 1..=self.retry.attempts.max(1) {
            self.take_token();
            match self.transport.call(method, params.clone()) {
                Ok(value) => return Ok(value),
                Err(error) if error.is_retryable() => {
                    last_error = Some(error);
                    if attempt < self.retry.attempts {
                        self.sleep.sleep_ms(self.retry.backoff.delay_ms(attempt));
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Err(RpcError::RetriesExhausted {
            attempts: self.retry.attempts,
            message: last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "unknown error".to_string()),
        })
    }

    /// Send a batch, falling back to one call per item when the endpoint
    /// refuses batches (registry SDK behaviour).
    pub fn batch(&self, method: &str, params: &[Value]) -> Result<Vec<Value>, RpcError> {
        self.take_token();
        match self.transport.call_batch(method, params) {
            Ok(values) => Ok(values),
            Err(RpcError::BatchUnsupported | RpcError::BatchRejected) => params
                .iter()
                .map(|item| self.call(method, item.clone()))
                .collect(),
            Err(error) => Err(error),
        }
    }

    pub fn get_account(&self, account: &str) -> Result<Value, RpcError> {
        self.call("getAccount", json!([account]))
    }

    pub fn get_account_view(&self, account: &str) -> Result<Vec<u8>, RpcError> {
        let value = self.call("getAccountView", json!([account]))?;
        decode_bytes(&value, "getAccountView")
    }

    pub fn get_balance(&self, account: &str) -> Result<u128, RpcError> {
        let value = self.call("getAccountBalance", json!([account]))?;
        match &value {
            Value::String(text) => text
                .parse::<u128>()
                .map_err(|error| RpcError::Parse(error.to_string())),
            Value::Number(number) => number
                .as_u64()
                .map(u128::from)
                .ok_or_else(|| RpcError::Parse("balance is not an unsigned integer".into())),
            _ => Err(RpcError::Parse("unexpected balance shape".into())),
        }
    }

    /// Read one program's actor state from an account. This is the shape LNS
    /// uses: `getAccountView` with both the account and the program id.
    pub fn get_actor_state(&self, account: &str, program: &str) -> Result<Vec<u8>, RpcError> {
        let value = self.call(
            "getAccountView",
            json!([{ "account_id": account, "program_account_id": program }]),
        )?;
        // An account nothing ever wrote to (e.g. an LNS name nobody registered) comes back as
        // `null`: it has no state for any program.
        if value.is_null() {
            return Ok(Vec::new());
        }
        let states = value
            .get("data")
            .and_then(|data| data.get("actor_states"))
            .and_then(Value::as_object)
            .ok_or_else(|| RpcError::Parse("getAccountView has no actor_states".into()))?;
        let bytes = states
            .get(program)
            .ok_or_else(|| RpcError::Parse("getAccountView has no state for the program".into()))?;
        decode_bytes(bytes, "actor state")
    }

    pub fn get_fee_state(&self) -> Result<Value, RpcError> {
        self.call("getFeeState", json!([]))
    }

    pub fn get_program_ids(&self) -> Result<Vec<String>, RpcError> {
        let value = self.call("getProgramIds", json!([]))?;
        serde_json::from_value(value).map_err(|error| RpcError::Parse(error.to_string()))
    }

    pub fn send_transaction(&self, transaction: Value) -> Result<String, RpcError> {
        let value = self.call("sendTransaction", json!([transaction]))?;
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| RpcError::Parse("sendTransaction did not return a hash".into()))
    }
}

fn decode_bytes(value: &Value, field: &str) -> Result<Vec<u8>, RpcError> {
    if let Some(text) = value.as_str() {
        return hex::decode(text).map_err(|error| RpcError::Parse(format!("{}: {}", field, error)));
    }
    let array = value
        .as_array()
        .ok_or_else(|| RpcError::Parse(format!("{} is not bytes", field)))?;
    array
        .iter()
        .map(|item| {
            item.as_u64()
                .and_then(|byte| u8::try_from(byte).ok())
                .ok_or_else(|| RpcError::Parse(format!("{} has a bad byte", field)))
        })
        .collect()
}

/// A simple HTTP JSON-RPC transport. No chain access happens unless a caller
/// constructs this type.
pub struct HttpTransport {
    url: String,
}

impl HttpTransport {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

impl Transport for HttpTransport {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        })
        .to_string();
        let response = ureq::post(&self.url)
            .set("Content-Type", "application/json")
            .send_string(&body)
            .map_err(|error| RpcError::Transport(error.to_string()))?;
        let value: Value = response
            .into_string()
            .map_err(|error| RpcError::Parse(error.to_string()))
            .and_then(|text| {
                serde_json::from_str(&text).map_err(|e| RpcError::Parse(e.to_string()))
            })?;
        if let Some(error) = value.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string();
            return Err(RpcError::from_server(code, message));
        }
        value
            .get("result")
            .cloned()
            .ok_or_else(|| RpcError::Parse("response has no result".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        let backoff = Backoff {
            first_ms: 100,
            cap_ms: 250,
        };
        assert_eq!(backoff.delay_ms(1), 100);
        assert_eq!(backoff.delay_ms(2), 200);
        assert_eq!(backoff.delay_ms(3), 250);
    }

    #[test]
    fn retries_a_rate_limit_then_succeeds() {
        let rpc = Arc::new(MockRpc::new());
        rpc.push(
            "ping",
            Err(RpcError::RateLimited {
                retry_after_ms: Some(10),
            }),
        );
        rpc.push_ok("ping", json!(42));
        let client = Client::new(rpc.clone())
            .with_clock_and_sleep(Arc::new(SystemNow), Arc::new(RecordingSleep::new()));
        assert_eq!(client.call("ping", json!([])).unwrap(), json!(42));
        assert_eq!(rpc.call_count("ping"), 2);
    }

    #[test]
    fn does_not_retry_a_protocol_refusal() {
        let rpc = Arc::new(MockRpc::new());
        rpc.push(
            "ping",
            Err(RpcError::Server {
                code: -32602,
                message: "bad arg".into(),
            }),
        );
        let client = Client::new(rpc.clone());
        assert_eq!(
            client.call("ping", json!([])).unwrap_err(),
            RpcError::Server {
                code: -32602,
                message: "bad arg".into()
            }
        );
        assert_eq!(rpc.call_count("ping"), 1);
    }

    #[test]
    fn retries_exhausted_has_a_clear_error() {
        let rpc = Arc::new(MockRpc::new());
        for _ in 0..4 {
            rpc.push("ping", Err(RpcError::Transport("offline".into())));
        }
        let client = Client::new(rpc.clone())
            .with_clock_and_sleep(Arc::new(SystemNow), Arc::new(RecordingSleep::new()));
        let error = client.call("ping", json!([])).unwrap_err();
        assert!(matches!(
            error,
            RpcError::RetriesExhausted { attempts: 4, .. }
        ));
        assert!(error.to_string().contains("offline"));
    }

    #[test]
    fn batch_falls_back_to_single_calls() {
        let rpc = Arc::new(MockRpc::new());
        rpc.push_batch(Err(RpcError::BatchRejected));
        for i in 0..3 {
            rpc.push_ok("getAccount", json!(i));
        }
        let client = Client::new(rpc.clone());
        let values = client
            .batch("getAccount", &[json!(0), json!(1), json!(2)])
            .unwrap();
        assert_eq!(values, vec![json!(0), json!(1), json!(2)]);
        assert_eq!(rpc.batch_calls(), 1);
        assert_eq!(rpc.call_count("getAccount"), 3);
    }

    #[test]
    fn pacing_waits_when_the_burst_is_spent() {
        struct FixedClock(Mutex<u64>);
        impl Now for FixedClock {
            fn now_ms(&self) -> u64 {
                *self.0.lock().unwrap()
            }
        }
        struct AdvancingSleep {
            clock: Arc<FixedClock>,
            count: Mutex<usize>,
        }
        impl Sleep for AdvancingSleep {
            fn sleep_ms(&self, ms: u64) {
                *self.clock.0.lock().unwrap() += ms;
                *self.count.lock().unwrap() += 1;
            }
            fn waits(&self) -> usize {
                *self.count.lock().unwrap()
            }
        }
        let clock = Arc::new(FixedClock(Mutex::new(0)));
        let sleep = Arc::new(AdvancingSleep {
            clock: clock.clone(),
            count: Mutex::new(0),
        });
        let rpc = Arc::new(MockRpc::new());
        for _ in 0..2 {
            rpc.push_ok("ping", json!(true));
        }
        let pace = Pace {
            rate_per_sec: 1,
            burst: 1,
        };
        let client = Client::with_policy(rpc.clone(), RetryPolicy::default(), pace.clone())
            .with_clock_and_sleep(clock.clone(), sleep.clone());
        assert_eq!(
            pace,
            Pace {
                rate_per_sec: 1,
                burst: 1
            }
        );
        client.call("ping", json!([])).unwrap();
        client.call("ping", json!([])).unwrap();
        assert!(sleep.waits() >= 1, "the second call should be paced");
        assert!(clock.now_ms() >= 1_000, "the clock should be advanced");
    }
}
