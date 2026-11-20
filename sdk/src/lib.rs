//! SDK for building Logos modules on the RFP-017 vesting program.
//!
//! The full lifecycle lives in `Client`; storage and chain access are behind
//! the small `Backend` trait, so the same API works against a local ledger
//! (the CLI's JSON file, tests) and the LEZ program (`LezBackend`, feature `lez`).

pub mod backend;
pub mod client;
pub mod error;
/// LEZ rc3 backend. The `wire` mirror and its offline tests build with no chain dependencies;
/// `LezBackend` itself needs the `lez` feature.
pub mod lez;
/// LNS names for account arguments (feature `names`).
#[cfg(feature = "names")]
pub mod names;

pub use backend::{Backend, BackendError, FeeEstimate, FeePayer, ScheduleId};
pub use client::{pre_claim_summary, AccountKind, AccountRef, Client, PreClaimSummary};
pub use error::SdkError;
#[cfg(feature = "lez")]
pub use lez::LezBackend;
