//! Seeds, public/private accounts and PDA derivation.
//!
//! The wallet owns secrets. This crate derives account ids and defines a
//! [`Signer`] seam; it never stores a private key. When the real LEZ wallet is
//! wired in, `AccountKind` will come from the wallet's private-account state,
//! never from a caller-supplied flag.

pub use lee_core::account::AccountId;
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountKind {
    Public,
    Private,
}

impl AccountKind {
    pub fn is_private(self) -> bool {
        self == AccountKind::Private
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccountRef {
    pub id: AccountId,
    pub kind: AccountKind,
}

impl AccountRef {
    pub fn public(id: AccountId) -> Self {
        Self {
            id,
            kind: AccountKind::Public,
        }
    }
    pub fn private(id: AccountId) -> Self {
        Self {
            id,
            kind: AccountKind::Private,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountsError {
    BadAccount(String),
    SeedTooShort,
    SeedTooLong,
}

impl fmt::Display for AccountsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountsError::BadAccount(text) => write!(f, "bad account id '{}'", text),
            AccountsError::SeedTooShort => write!(f, "a seed needs at least one byte"),
            AccountsError::SeedTooLong => write!(f, "a seed cannot exceed 32 bytes"),
        }
    }
}

impl std::error::Error for AccountsError {}

/// A seed is an opaque 32-byte value. BIP39 phrase handling lives in the
/// wallet's encrypted store; this type only derives account ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seed([u8; 32]);

impl Seed {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AccountsError> {
        if bytes.is_empty() {
            return Err(AccountsError::SeedTooShort);
        }
        if bytes.len() > 32 {
            return Err(AccountsError::SeedTooLong);
        }
        let mut value = [0u8; 32];
        value[..bytes.len()].copy_from_slice(bytes);
        Ok(Self(value))
    }

    pub const fn from_array(value: [u8; 32]) -> Self {
        Self(value)
    }

    /// Derive one account id from the seed and an index. This is a local
    /// derivation for tests and offline tools; the wallet's key tree is the
    /// authority for production accounts.
    pub fn derive(&self, index: u32, kind: AccountKind) -> Account {
        let mut bytes = Vec::with_capacity(36);
        bytes.extend_from_slice(&self.0);
        bytes.extend_from_slice(&index.to_le_bytes());
        let hash = Sha256::digest(&bytes);
        Account {
            id: AccountId::new(hash.into()),
            kind,
        }
    }

    pub fn value(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    pub kind: AccountKind,
}

impl Account {
    pub fn is_private(&self) -> bool {
        self.kind.is_private()
    }
    pub fn as_ref_account(&self) -> AccountRef {
        AccountRef {
            id: self.id,
            kind: self.kind,
        }
    }
}

/// Signing stays with the wallet. A module supplies a signer handle; this
/// crate never sees the secret key material.
pub trait Signer {
    fn public_key(&self) -> [u8; 32];
    fn sign(&self, message: &[u8]) -> [u8; 64];
}

/// SPEL `compute_pda` string component: UTF-8, zero-padded to 32 bytes.
pub fn seed_str(text: &str) -> Result<[u8; 32], AccountsError> {
    let bytes = text.as_bytes();
    if bytes.len() > 32 {
        return Err(AccountsError::BadAccount(format!(
            "seed string '{}' is longer than 32 bytes",
            text
        )));
    }
    let mut out = [0u8; 32];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

/// Combine several 32-byte seed components as `sha256(c1 ‖ c2 ‖ …)`.
pub fn combine(parts: &[[u8; 32]]) -> [u8; 32] {
    assert!(!parts.is_empty(), "a PDA needs at least one component");
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}

/// `lee_core::account::AccountId::for_public_pda`, the same rule the program
/// registry uses: `sha256(prefix ‖ program ‖ seed)`.
pub fn derive_pda(program: &AccountId, seed: &[u8; 32]) -> AccountId {
    const PREFIX: &[u8; 32] = b"/LEE/v0.2/AccountId/PDA/\x00\x00\x00\x00\x00\x00\x00\x00";
    let mut hash = Sha256::new();
    hash.update(PREFIX);
    hash.update(program.value());
    hash.update(seed);
    AccountId::new(hash.finalize().into())
}

/// Accounts are written as base58 in JSON-RPC and in the CLI.
pub fn format_account(id: &AccountId) -> String {
    id.to_string()
}

pub fn parse_account(text: &str) -> Result<AccountId, AccountsError> {
    AccountId::from_str(text).map_err(|error| AccountsError::BadAccount(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_derives_stable_accounts() {
        let seed = Seed::from_bytes(b"test seed").unwrap();
        let public = seed.derive(0, AccountKind::Public);
        let private = seed.derive(0, AccountKind::Private);
        assert_eq!(public.id, seed.derive(0, AccountKind::Public).id);
        assert_eq!(public.kind, AccountKind::Public);
        assert!(private.is_private());
        assert_ne!(public.id, seed.derive(1, AccountKind::Public).id);
        assert_eq!(
            public.id, private.id,
            "kind is metadata, not a derivation input"
        );
    }

    #[test]
    fn seed_length_is_checked() {
        assert!(Seed::from_bytes(&[]).is_err());
        assert!(Seed::from_bytes(&[7u8; 33]).is_err());
        assert_eq!(Seed::from_bytes(&[7u8; 32]).unwrap().value(), &[7u8; 32]);
    }

    #[test]
    fn pda_matches_the_spel_rule() {
        let program = AccountId::new([0xABu8; 32]);
        let seed = seed_str("release").unwrap();
        let derived = derive_pda(&program, &seed);

        // Independent calculation of sha256(prefix || program || seed).
        let mut expected = Sha256::new();
        expected.update(b"/LEE/v0.2/AccountId/PDA/\x00\x00\x00\x00\x00\x00\x00\x00");
        expected.update([0xABu8; 32]);
        expected.update(seed);
        let expected = AccountId::new(expected.finalize().into());
        assert_eq!(derived, expected);
        assert_ne!(derived, derive_pda(&program, &seed_str("other").unwrap()));
    }

    #[test]
    fn account_roundtrips_through_base58() {
        let id = AccountId::new([1u8; 32]);
        let text = format_account(&id);
        assert_eq!(parse_account(&text).unwrap(), id);
        assert!(parse_account("not-base58!").is_err());
    }

    #[test]
    fn accounts_come_with_an_explicit_kind() {
        let id = AccountId::new([2u8; 32]);
        let public = AccountRef::public(id);
        let private = AccountRef::private(id);
        assert!(!public.kind.is_private());
        assert!(private.kind.is_private());
    }
}
