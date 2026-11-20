//! SDK for building Logos modules on the RFP-017 vesting program.
//!
//! The full lifecycle lives in `Client`; storage and chain access are behind
//! the small `Backend` trait so the same API works against the in-memory
//! ledger today and the LEZ program later.

pub mod backend;
pub mod client;
pub mod error;
/// LEZ rc3 backend. The `wire` mirror and its offline tests build with no chain dependencies;
/// `LezBackend` itself needs the `lez` feature.
pub mod lez;

pub use backend::{Backend, BackendError, ScheduleId};
pub use client::{AccountKind, AccountRef, Client, PreClaimSummary, ESTIMATED_FEE_PLACEHOLDER};
pub use error::SdkError;
#[cfg(feature = "lez")]
pub use lez::LezBackend;
