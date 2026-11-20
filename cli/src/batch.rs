//! JSON shape for `create-batch`.

use serde::Deserialize;
use vesting_core::{AccountId, CreateParams, ScheduleKind};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum KindDto {
    Linear { start: u64, end: u64 },
    CliffLinear { start: u64, cliff: u64, end: u64 },
    Milestone { tranches: Vec<String> },
}

#[derive(Debug, Deserialize)]
pub struct BatchParams {
    pub creator: String,
    pub beneficiary: String,
    pub token: String,
    /// Amounts are strings because serde_json cannot decode a u128 number.
    pub total: String,
    pub kind: KindDto,
    pub cancelable: Option<bool>,
    pub transferable: Option<bool>,
    pub cancel_authority: Option<String>,
    pub milestone_authority: Option<String>,
}

fn amount(text: &str, field: &str) -> Result<u128, String> {
    text.parse::<u128>()
        .map_err(|error| format!("{}: invalid amount '{}': {}", field, text, error))
}

impl BatchParams {
    /// Resolve each account field with the same resolver used by the CLI, so
    /// batch rows accept names as well as addresses.
    pub fn into_params_with<F>(self, resolve: &mut F) -> Result<CreateParams, String>
    where
        F: FnMut(&str) -> Result<AccountId, String>,
    {
        let mut account = |text: &str, field: &str| {
            resolve(text).map_err(|error| format!("{}: {}", field, error))
        };
        let kind = match self.kind {
            KindDto::Linear { start, end } => ScheduleKind::Linear { start, end },
            KindDto::CliffLinear { start, cliff, end } => {
                ScheduleKind::CliffLinear { start, cliff, end }
            }
            KindDto::Milestone { tranches } => ScheduleKind::Milestone {
                tranches: tranches
                    .iter()
                    .map(|text| amount(text, "tranche"))
                    .collect::<Result<Vec<_>, _>>()?,
            },
        };
        let cancel_authority = match &self.cancel_authority {
            Some(text) => Some(account(text, "cancel_authority")?),
            None => None,
        };
        let milestone_authority = match &self.milestone_authority {
            Some(text) => Some(account(text, "milestone_authority")?),
            None => None,
        };
        Ok(CreateParams {
            creator: account(&self.creator, "creator")?,
            beneficiary: account(&self.beneficiary, "beneficiary")?,
            token: account(&self.token, "token")?,
            total: amount(&self.total, "total")?,
            kind,
            cancelable: self.cancelable,
            transferable: self.transferable,
            cancel_authority,
            milestone_authority,
        })
    }
}
