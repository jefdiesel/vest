//! Logos Name Service: name ↔ account, ported from the program registry SDK.
//!
//! Read-only. Derivation follows logos-names v5 (Argon2id + SHA-256), records
//! are version 1–7 (deed v7), and sealed profiles are AES-256-GCM with the
//! `logos-names/v5/record` AAD. The LNS program id is a required argument:
//! there is no default anywhere in our repos.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use borsh::BorshDeserialize;
use logos_accounts::AccountId;
use logos_rpc::{Client, RpcError};
use sha2::{Digest, Sha256};
use std::fmt;

const DERIVATION_SALT: &[u8] = b"logos-names/v5";
const DERIVATION_MEMORY_KIB: u32 = 65_536;
const DERIVATION_PASSES: u32 = 3;
const DERIVATION_LANES: u32 = 1;
const ID_DOMAIN: &[u8] = b"logos-names/v5/id";
const KEY_DOMAIN: &[u8] = b"logos-names/v5/key";
const NAME_SEED_DOMAIN: &[u8] = b"logos-names/v5/name/";
const RECORD_SEAL: &[u8] = b"logos-names/v5/record";
const PDA_PREFIX: &[u8; 32] = b"/LEE/v0.2/AccountId/PDA/\x00\x00\x00\x00\x00\x00\x00\x00";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const MAX_SEALED_RECORD_LEN: usize = 12 * 1024;
const MAX_NAME_LEN: usize = 32;
const GRACE_MS: u64 = 30 * 24 * 60 * 60 * 1000;
pub const NEVER: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Derived {
    pub id: [u8; 32],
    pub key: [u8; 32],
}

/// Derive a name's id and profile key. Deliberately slow (64 MiB, 3 passes);
/// cache the result.
pub fn derive(name: &str) -> Derived {
    let params = Params::new(
        DERIVATION_MEMORY_KIB,
        DERIVATION_PASSES,
        DERIVATION_LANES,
        Some(32),
    )
    .expect("the Argon2 parameters are valid");
    let mut master = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(name.as_bytes(), DERIVATION_SALT, &mut master)
        .expect("Argon2 accepts these inputs");
    let mut id = Sha256::new();
    id.update(ID_DOMAIN);
    id.update(master);
    let mut key = Sha256::new();
    key.update(KEY_DOMAIN);
    key.update(master);
    Derived {
        id: id.finalize().into(),
        key: key.finalize().into(),
    }
}

pub fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_NAME_LEN
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
}

fn is_shared_space(byte: char) -> bool {
    matches!(byte, ' ' | '\t' | '\n' | '\u{000b}' | '\u{000c}' | '\r')
}

/// Normalise a user-typed name: ASCII trim, lower-case, one `lns:` prefix,
/// one `@` prefix and one `.lez` suffix.
pub fn normalize_name(input: &str) -> String {
    let trimmed = input.trim_matches(is_shared_space);
    let mut text = trimmed.to_ascii_lowercase();
    if let Some(rest) = text.strip_prefix("lns:") {
        text = rest.to_string();
    }
    if let Some(rest) = text.strip_prefix('@') {
        text = rest.to_string();
    }
    if let Some(rest) = text.strip_suffix(".lez") {
        text = rest.to_string();
    }
    text
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Invalid,
    AccountId,
    Ambiguous,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameError::Invalid => write!(f, "invalid name"),
            NameError::AccountId => write!(f, "a name cannot look like a 32-byte account id"),
            NameError::Ambiguous => write!(f, "ambiguous whitespace in the name"),
        }
    }
}

impl std::error::Error for NameError {}

pub fn parse_name(input: &str) -> Result<String, NameError> {
    let trimmed = input.trim_matches(is_shared_space);
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| is_shared_space(c) || c == '\u{feff}')
        || trimmed
            .chars()
            .last()
            .is_some_and(|c| is_shared_space(c) || c == '\u{feff}')
    {
        return Err(NameError::Ambiguous);
    }
    let name = normalize_name(input);
    if !is_valid_name(&name) {
        return Err(NameError::Invalid);
    }
    if name.parse::<AccountId>().is_ok() {
        return Err(NameError::AccountId);
    }
    Ok(name)
}

fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// The name record is a public PDA under the LNS program.
pub fn record_address(lns: &AccountId, id: &[u8; 32]) -> AccountId {
    let seed = sha256(&[NAME_SEED_DOMAIN, id].concat());
    let mut hash = Sha256::new();
    hash.update(PDA_PREFIX);
    hash.update(lns.value());
    hash.update(seed);
    AccountId::new(hash.finalize().into())
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub texts: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry<'a> {
    Absent,
    One(&'a str),
    Duplicate,
}

impl Profile {
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.entry(key) {
            Entry::One(value) => Some(value),
            _ => None,
        }
    }

    /// Duplicate keys fail closed: clients disagree on which one wins.
    pub fn entry(&self, key: &str) -> Entry<'_> {
        let mut found = None;
        for (candidate, value) in &self.texts {
            if candidate == key {
                if found.is_some() {
                    return Entry::Duplicate;
                }
                found = Some(value.as_str());
            }
        }
        found.map(Entry::One).unwrap_or(Entry::Absent)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameRecord {
    pub owner_tag: [u8; 32],
    pub expires: u64,
    pub profile: Option<Profile>,
    pub deed: Option<([u8; 32], [u8; 32])>,
}

impl NameRecord {
    pub fn gone(&self, now_ms: u64) -> bool {
        self.expires != NEVER && now_ms >= self.expires.saturating_add(GRACE_MS)
    }
}

#[derive(Debug)]
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|bytes| bytes[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8)
            .map(|bytes| u64::from_le_bytes(bytes.try_into().unwrap()))
    }
    fn array32(&mut self) -> Option<[u8; 32]> {
        self.take(32).map(|bytes| bytes.try_into().unwrap())
    }
    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

#[derive(BorshDeserialize, borsh::BorshSerialize)]
struct ProfileWire {
    name: String,
    account: Option<[u8; 32]>,
    npk: Option<[u8; 32]>,
    vpk: Vec<u8>,
    texts: Vec<(String, String)>,
}

/// Open a sealed profile. A wrong key or a wrong name reads as `None` rather
/// than as a different person's data.
pub fn open_profile(key: &[u8; 32], sealed: &[u8], name: &str) -> Option<Profile> {
    if sealed.len() < NONCE_LEN + TAG_LEN || sealed.len() > MAX_SEALED_RECORD_LEN {
        return None;
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: body,
                aad: RECORD_SEAL,
            },
        )
        .ok()?;
    let unpadded = {
        let mut end = plaintext.len();
        while end > 0 && plaintext[end - 1] == 0 {
            end -= 1;
        }
        &plaintext[..end]
    };
    let wire = ProfileWire::try_from_slice(unpadded).ok()?;
    (wire.name == name).then_some(Profile {
        name: wire.name,
        texts: wire.texts,
    })
}

/// Decode a version 1–7 name record. Version 6 adds `claimed`; version 7 adds
/// a deed; trailing bytes are always rejected.
pub fn decode_record(bytes: &[u8], key: &[u8; 32], name: &str) -> Result<NameRecord, RpcError> {
    let mut reader = Reader { bytes, at: 0 };
    let version = reader
        .u8()
        .ok_or_else(|| RpcError::Parse("empty name record".into()))?;
    if !(1..=7).contains(&version) {
        return Err(RpcError::Parse(format!(
            "unknown name record version {}",
            version
        )));
    }
    let owner_tag = reader
        .array32()
        .ok_or_else(|| RpcError::Parse("name record has no owner tag".into()))?;
    let expires = reader
        .u64()
        .ok_or_else(|| RpcError::Parse("name record has no expiry".into()))?;
    let sealed_len = reader
        .u32()
        .ok_or_else(|| RpcError::Parse("name record has no profile length".into()))?
        as usize;
    let sealed = reader
        .take(sealed_len)
        .ok_or_else(|| RpcError::Parse("name record profile is truncated".into()))?;
    if version >= 6 {
        let claimed = reader
            .u8()
            .ok_or_else(|| RpcError::Parse("name record has no claimed flag".into()))?;
        if claimed > 1 {
            return Err(RpcError::Parse("invalid claimed flag".into()));
        }
    }
    let deed = if version >= 7 {
        match reader
            .u8()
            .ok_or_else(|| RpcError::Parse("name record has no deed tag".into()))?
        {
            0 => None,
            1 => Some((
                reader
                    .array32()
                    .ok_or_else(|| RpcError::Parse("bad deed definition".into()))?,
                reader
                    .array32()
                    .ok_or_else(|| RpcError::Parse("bad deed nonce".into()))?,
            )),
            other => return Err(RpcError::Parse(format!("bad deed tag {}", other))),
        }
    } else {
        None
    };
    if !reader.done() {
        return Err(RpcError::Parse("trailing bytes in name record".into()));
    }
    let profile = open_profile(key, sealed, name);
    Ok(NameRecord {
        owner_tag,
        expires,
        profile,
        deed,
    })
}

/// Resolve a name with one `getAccountView` call. Returns `None` for an
/// absent, empty or lapsed record.
pub fn resolve(
    client: &Client,
    lns: &AccountId,
    name: &str,
    derived: &Derived,
    now_ms: u64,
) -> Result<Option<NameRecord>, RpcError> {
    let record = record_address(lns, &derived.id);
    let bytes = client.get_actor_state(
        &logos_accounts::format_account(&record),
        &logos_accounts::format_account(lns),
    )?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let record = decode_record(&bytes, &derived.key, name)?;
    Ok((!record.gone(now_ms)).then_some(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use logos_rpc::{json, MockRpc};
    use std::sync::Arc;

    #[test]
    fn derivation_matches_spec_vectors() {
        // SPEC section 10 vectors shared with the registry SDK's lns.rs tests.
        let cases = [
            (
                "jef",
                "7041d9e447f06bdfe327d870402b1524cfe38bd03445b8fc3d0e87bc978eb763",
                "aad91a1a3dd0fb56a213212fbea58fe25d9139db42fc59ce2116c3cfa3440088",
            ),
            (
                "logos",
                "4a8bf61b00dbc63907a935aa15ed534bb32d866978aa1f2128f9b6f37e0c4025",
                "d67dd628fa51fa62bd32ea82c3e9ff1b67f93b01103be956eaebc52f1699df44",
            ),
        ];
        for (name, id, key) in cases {
            let derived = derive(name);
            assert_eq!(hex::encode(derived.id), id, "id for {}", name);
            assert_eq!(hex::encode(derived.key), key, "key for {}", name);
        }
    }

    #[test]
    fn record_address_matches_the_pda_rule() {
        let program = AccountId::new([0x07; 32]);
        let derived = derive("jef");
        assert_eq!(
            logos_accounts::format_account(&record_address(&program, &derived.id)),
            "5b5xVGob2GYvrq21MSuxm1MfieucDEy9yxhDwdXndbpD"
        );
    }

    #[test]
    fn name_normalisation_and_validation() {
        for input in [
            "Jef",
            "@jef",
            "lns:jef",
            "Jef.LEZ",
            " jef \n",
            "LNS:@JEF.lez",
        ] {
            assert_eq!(normalize_name(input), "jef", "{}", input);
        }
        assert!(parse_name("jef").is_ok());
        assert!(parse_name("a-b-c").is_ok());
        assert_eq!(parse_name("jef.eth").unwrap_err(), NameError::Invalid);
        assert_eq!(parse_name("-jef").unwrap_err(), NameError::Invalid);
        assert_eq!(parse_name("\u{feff}jef").unwrap_err(), NameError::Ambiguous);
    }

    fn seal(name: &str, key: &[u8; 32], texts: &[(&str, &str)]) -> Vec<u8> {
        let wire = ProfileWire {
            name: name.to_string(),
            account: None,
            npk: None,
            vpk: Vec::new(),
            texts: texts
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        let mut plain = borsh::to_vec(&wire).unwrap();
        while plain.len() % 256 != 0 {
            plain.push(0);
        }
        let nonce = [0u8; NONCE_LEN];
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: RECORD_SEAL,
                },
            )
            .unwrap();
        [nonce.as_slice(), sealed.as_slice()].concat()
    }

    fn record_bytes(version: u8, sealed: &[u8], claimed: Option<u8>, deed: Option<u8>) -> Vec<u8> {
        let mut bytes = vec![version];
        bytes.extend_from_slice(&[1u8; 32]);
        bytes.extend_from_slice(&10_000u64.to_le_bytes());
        bytes.extend_from_slice(&(sealed.len() as u32).to_le_bytes());
        bytes.extend_from_slice(sealed);
        if version >= 6 {
            bytes.push(claimed.unwrap_or(0));
        }
        if version >= 7 {
            bytes.push(deed.unwrap_or(0));
        }
        bytes
    }

    #[test]
    fn sealed_profile_opens_only_with_the_right_key_and_name() {
        let derived = derive("jef");
        let sealed = seal("jef", &derived.key, &[("chat", "hi")]);
        let profile = open_profile(&derived.key, &sealed, "jef").unwrap();
        assert_eq!(profile.text("chat"), Some("hi"));
        assert!(open_profile(&derived.key, &sealed, "ada").is_none());
        assert!(open_profile(&[0u8; 32], &sealed, "jef").is_none());
    }

    #[test]
    fn record_decoding_accepts_v7_and_fails_closed() {
        let derived = derive("logos");
        let sealed = seal("logos", &derived.key, &[]);
        let record = decode_record(
            &record_bytes(7, &sealed, Some(0), Some(0)),
            &derived.key,
            "logos",
        )
        .unwrap();
        assert_eq!(record.owner_tag, [1u8; 32]);
        assert!(record.deed.is_none());
        assert!(decode_record(
            &record_bytes(7, &sealed, Some(0), Some(2)),
            &derived.key,
            "logos"
        )
        .is_err());
        let mut trailing = record_bytes(6, &sealed, Some(0), None);
        trailing.push(9);
        assert!(decode_record(&trailing, &derived.key, "logos").is_err());
        assert!(decode_record(
            &record_bytes(99, &sealed, Some(0), None),
            &derived.key,
            "logos"
        )
        .is_err());
    }

    #[test]
    fn resolve_uses_one_get_account_view_call() {
        let rpc = Arc::new(MockRpc::new());
        let derived = derive("jef");
        let lns = AccountId::new([0x07; 32]);
        let sealed = seal("jef", &derived.key, &[("chat", "hi")]);
        let record = record_bytes(7, &sealed, Some(0), Some(0));
        let program = logos_accounts::format_account(&lns);
        rpc.push_ok(
            "getAccountView",
            json!({ "data": { "actor_states": { program: record } } }),
        );
        let client = Client::new(rpc.clone());
        let found = resolve(&client, &lns, "jef", &derived, 1_000)
            .unwrap()
            .unwrap();
        assert_eq!(found.profile.unwrap().text("chat"), Some("hi"));
        assert_eq!(rpc.call_count("getAccountView"), 1);
    }
}
