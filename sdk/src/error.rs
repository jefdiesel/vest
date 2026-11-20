//! SDK errors: core errors plus SDK-specific validation failures.

use std::fmt;
use vesting_core::{AccountId, VestingError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SdkError {
    /// A rule from `vesting-core` rejected the operation.
    Core(VestingError),
    /// The storage/chain layer failed.
    Backend(String),
    /// `claim_private` was given an account that is not private (RFP Privacy 3).
    TargetNotPrivate { account: AccountId },
}

impl SdkError {
    /// Stable numeric code. Core codes are unchanged; SDK codes start at 1000.
    pub fn code(&self) -> u32 {
        match self {
            SdkError::Core(error) => error.code(),
            SdkError::Backend(_) => 1000,
            SdkError::TargetNotPrivate { .. } => 1001,
        }
    }

    pub fn message(&self) -> String {
        match self {
            SdkError::Core(error) => error.message(),
            SdkError::Backend(message) => format!("backend error: {}", message),
            SdkError::TargetNotPrivate { account } => format!(
                "The claim target {} is a public account. Only a private (shielded) account can receive a private claim.",
                hex::encode(account)
            ),
        }
    }
}

impl fmt::Display for SdkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for SdkError {}

impl From<VestingError> for SdkError {
    fn from(error: VestingError) -> Self {
        SdkError::Core(error)
    }
}
