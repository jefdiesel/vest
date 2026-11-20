//! LNS names for account arguments.
//!
//! An account argument may be a 64-hex address or a logos-names v5 name.
//! Names are resolved at the moment of the action against `logos-sdk`'s `lns`
//! module; only the resolved address is stored on chain. A small local book
//! remembers name → address so `show`, `list` and events can display names
//! when a reverse lookup exists.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use borsh::BorshDeserialize;
use logos_lns::{derive, normalize_name, record_address};
use logos_rpc::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;
use vesting_core::AccountId;

const GRACE_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const RECORD_SEAL: &[u8] = b"logos-names/v5/record";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    InvalidName { name: String },
    LnsNotConfigured { name: String },
    UnknownName { name: String },
    ExpiredName { name: String, expires: u64 },
    NoOwner { name: String },
    Rpc(String),
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::InvalidName { name } => write!(f, "invalid LNS name '{}'", name),
            NameError::LnsNotConfigured { name } => write!(
                f,
                "'{}' is a name: pass --lns <program> and --lns-rpc <url> to resolve it",
                name
            ),
            NameError::UnknownName { name } => write!(f, "unknown LNS name '{}'", name),
            NameError::ExpiredName { name, expires } => {
                write!(f, "LNS name '{}' expired at {} ms", name, expires)
            }
            NameError::NoOwner { name } => write!(f, "LNS name '{}' has no owner", name),
            NameError::Rpc(message) => write!(f, "LNS lookup failed: {}", message),
        }
    }
}

impl std::error::Error for NameError {}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct NameBook {
    /// base58 address -> name, so display can show `alice (5T7Az…)`.
    names: HashMap<String, String>,
}

impl NameBook {
    pub fn open(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|error| format!("names file: {}", error))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("names file: {}", error)),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        fs::write(path, bytes).map_err(|error| format!("names file: {}", error))
    }

    pub fn remember(&mut self, account: &AccountId, name: &str) {
        self.names.insert(base58(account), name.to_string());
    }

    /// Reverse lookup, if we have seen this name before.
    pub fn label(&self, account: &AccountId) -> Option<&str> {
        self.names.get(&base58(account)).map(String::as_str)
    }

    /// A short display form: `alice (5T7Az…)` when known, else the address.
    pub fn display(&self, account: &AccountId) -> String {
        match self.label(account) {
            Some(name) => {
                let address = base58(account);
                let short = &address[..address.len().min(6)];
                format!("{} ({}…)", name, short)
            }
            None => base58(account),
        }
    }
}

pub fn base58(account: &AccountId) -> String {
    bs58::encode(account).into_string()
}

fn base58_lee(account: &logos_accounts::AccountId) -> String {
    bs58::encode(account.value()).into_string()
}

fn parse_address(text: &str) -> Option<AccountId> {
    let bytes = hex::decode(text).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut account = [0u8; 32];
    account.copy_from_slice(&bytes);
    Some(account)
}

#[derive(BorshDeserialize, borsh::BorshSerialize)]
struct ProfileWire {
    #[allow(dead_code)]
    name: String,
    account: Option<[u8; 32]>,
    #[allow(dead_code)]
    npk: Option<[u8; 32]>,
    #[allow(dead_code)]
    vpk: Vec<u8>,
    #[allow(dead_code)]
    texts: Vec<(String, String)>,
}

/// Resolve one argument. A name is looked up now; an address is returned as-is.
pub fn resolve(
    client: Option<&Client>,
    lns: Option<AccountId>,
    book: &mut NameBook,
    raw: &str,
    now: u64,
) -> Result<(AccountId, Option<String>), NameError> {
    if let Some(account) = parse_address(raw) {
        return Ok((account, book.label(&account).map(str::to_string)));
    }

    let name = normalize_name(raw);
    if name.is_empty() || !logos_lns::is_valid_name(&name) {
        return Err(NameError::InvalidName {
            name: raw.to_string(),
        });
    }
    let (Some(client), Some(lns)) = (client, lns) else {
        return Err(NameError::LnsNotConfigured { name });
    };

    let derived = derive(&name);
    let record = record_address(&lee_account(&lns), &derived.id);
    let bytes = client
        .get_actor_state(&base58_lee(&record), &base58(&lns))
        .map_err(|error| NameError::Rpc(error.to_string()))?;
    if bytes.is_empty() {
        return Err(NameError::UnknownName { name });
    }
    if bytes.len() < 45 {
        return Err(NameError::Rpc("truncated name record".into()));
    }
    let expires = u64::from_le_bytes(bytes[33..41].try_into().unwrap());
    if expires != u64::MAX && now >= expires.saturating_add(GRACE_MS) {
        return Err(NameError::ExpiredName { name, expires });
    }
    let sealed_len = u32::from_le_bytes(bytes[41..45].try_into().unwrap()) as usize;
    let sealed = bytes
        .get(45..45 + sealed_len)
        .ok_or_else(|| NameError::Rpc("truncated name record profile".into()))?;
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return Err(NameError::Rpc("bad sealed profile length".into()));
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&derived.key));
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&sealed[..NONCE_LEN]),
            Payload {
                msg: &sealed[NONCE_LEN..],
                aad: RECORD_SEAL,
            },
        )
        .map_err(|_| NameError::NoOwner { name: name.clone() })?;
    // The plaintext is padded to a multiple of 256 with zero bytes. Do not
    // simply trim trailing zeroes: a profile ending in a zero u32 (for example
    // an empty text list) would lose real bytes. Find the longest prefix that
    // is a complete borsh profile instead.
    let mut decoded = None;
    for end in (1..=plain.len()).rev() {
        if let Ok(wire) = ProfileWire::try_from_slice(&plain[..end]) {
            decoded = Some(wire);
            break;
        }
    }
    let profile = decoded.ok_or_else(|| NameError::NoOwner { name: name.clone() })?;
    if profile.name != name {
        return Err(NameError::NoOwner { name });
    }
    let account_bytes = profile
        .account
        .ok_or_else(|| NameError::NoOwner { name: name.clone() })?;
    let account: AccountId = account_bytes;
    book.remember(&account, &name);
    Ok((account, Some(name)))
}

fn lee_account(account: &AccountId) -> logos_accounts::AccountId {
    logos_accounts::AccountId::new(*account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos_rpc::{json, MockRpc};
    use std::sync::Arc;

    fn record_with_account(name: &str, account: &AccountId, expires: u64) -> Vec<u8> {
        let derived = derive(name);
        let wire = ProfileWire {
            name: name.to_string(),
            account: Some(*account),
            npk: None,
            vpk: Vec::new(),
            texts: Vec::new(),
        };
        let mut plain = borsh::to_vec(&wire).unwrap();
        while plain.len() % 256 != 0 {
            plain.push(0);
        }
        let nonce = [0u8; NONCE_LEN];
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&derived.key));
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: RECORD_SEAL,
                },
            )
            .unwrap();
        let mut bytes = vec![7u8];
        bytes.extend_from_slice(&[1u8; 32]);
        bytes.extend_from_slice(&expires.to_le_bytes());
        bytes.extend_from_slice(&((nonce.len() + sealed.len()) as u32).to_le_bytes());
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&sealed);
        bytes.push(0); // claimed
        bytes.push(0); // no deed
        bytes
    }

    fn client_with(_name: &str, lns: &AccountId, record: Vec<u8>) -> Client {
        let rpc = Arc::new(MockRpc::new());
        let program = base58(lns);
        rpc.push_ok(
            "getAccountView",
            json!({ "data": { "actor_states": { program: record } } }),
        );
        // Keep the Arc alive only through the Client.
        Client::new(rpc)
    }

    #[test]
    fn address_arguments_do_not_need_lns() {
        let address = [7u8; 32];
        let text = hex::encode(address);
        let mut book = NameBook::default();
        let (found, name) = resolve(None, None, &mut book, &text, 0).unwrap();
        assert_eq!(found, address);
        assert_eq!(name, None);
    }

    #[test]
    fn a_name_resolves_and_is_remembered() {
        let lns = [0x07u8; 32];
        let target = [2u8; 32];
        let record = record_with_account("alice", &target, u64::MAX);
        let client = client_with("alice", &lns, record);
        let mut book = NameBook::default();
        let (found, name) = resolve(Some(&client), Some(lns), &mut book, "alice", 1_000).unwrap();
        assert_eq!(found, target);
        assert_eq!(name.as_deref(), Some("alice"));
        assert_eq!(book.label(&target), Some("alice"));
        assert!(book.display(&target).starts_with("alice ("));
    }

    #[test]
    fn unknown_expired_and_ownerless_names_fail_clearly() {
        let lns = [0x07u8; 32];
        let rpc = Arc::new(MockRpc::new());
        let program = base58(&lns);
        rpc.push_ok(
            "getAccountView",
            json!({ "data": { "actor_states": { program: Vec::<u8>::new() } } }),
        );
        let client = Client::new(rpc);
        let mut book = NameBook::default();
        assert_eq!(
            resolve(Some(&client), Some(lns), &mut book, "nobody", 0).unwrap_err(),
            NameError::UnknownName {
                name: "nobody".into()
            }
        );

        let expired = record_with_account("old", &[3u8; 32], 1_000);
        let client = client_with("old", &lns, expired);
        assert!(matches!(
            resolve(Some(&client), Some(lns), &mut book, "old", 1_001 + GRACE_MS).unwrap_err(),
            NameError::ExpiredName { .. }
        ));
    }

    #[test]
    fn a_name_without_lns_config_is_a_clear_error() {
        let mut book = NameBook::default();
        let error = resolve(None, None, &mut book, "alice", 0).unwrap_err();
        assert!(error.to_string().contains("--lns"));
    }
}
