//! Vesting schedule maths, state and rules for RFP-017.
//!
//! This crate is pure Rust: it has no LEZ dependencies. The on-chain program
//! will be a thin shell that reads the clock, calls these functions, and moves
//! tokens. Every rule in the RFP therefore lives and is tested here.

pub mod error;
pub mod event;
pub mod math;
pub mod ops;
pub mod types;

pub use error::{VestingError, WaitingFor};
pub use event::Event;
pub use math::mul_div_floor;
pub use ops::*;
pub use types::*;
