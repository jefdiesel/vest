//! The CLI's JSON-file implementation of the SDK's `Backend` trait.
//!
//! Schedules and events are stored as hex-encoded borsh bytes, which keeps
//! `vesting-core` free of serde dependencies.

use borsh::BorshDeserialize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use vesting_core::{Event, Schedule, Timestamp};

pub use vesting_sdk::{Backend, BackendError, ScheduleId};

#[derive(Serialize, Deserialize, Default)]
struct StateFile {
    next_id: ScheduleId,
    /// id -> hex(borsh(Schedule))
    schedules: BTreeMap<ScheduleId, String>,
    /// hex(borsh(Event)), in submission order
    events: Vec<String>,
}

pub struct JsonFileBackend {
    path: PathBuf,
    now: Timestamp,
    state: StateFile,
}

impl JsonFileBackend {
    pub fn open(path: &Path, now_override: Option<Timestamp>) -> Result<Self, BackendError> {
        let state = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| BackendError::Decode(error.to_string()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StateFile::default(),
            Err(error) => return Err(BackendError::Io(error.to_string())),
        };
        let now = now_override.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as Timestamp
        });
        Ok(Self {
            path: path.to_path_buf(),
            now,
            state,
        })
    }

    fn flush(&self) -> Result<(), BackendError> {
        let bytes = serde_json::to_vec_pretty(&self.state)
            .map_err(|error| BackendError::Decode(error.to_string()))?;
        fs::write(&self.path, bytes).map_err(|error| BackendError::Io(error.to_string()))
    }
}

impl Backend for JsonFileBackend {
    fn now(&self) -> Timestamp {
        self.now
    }

    fn load(&self, id: ScheduleId) -> Result<Schedule, BackendError> {
        let encoded = self
            .state
            .schedules
            .get(&id)
            .ok_or(BackendError::NotFound(id))?;
        let bytes =
            hex::decode(encoded).map_err(|error| BackendError::Decode(error.to_string()))?;
        Schedule::try_from_slice(&bytes).map_err(|error| BackendError::Decode(error.to_string()))
    }

    fn store(&mut self, id: ScheduleId, schedule: &Schedule) -> Result<(), BackendError> {
        let bytes =
            borsh::to_vec(schedule).map_err(|error| BackendError::Decode(error.to_string()))?;
        self.state.schedules.insert(id, hex::encode(bytes));
        self.flush()
    }

    fn next_id(&mut self) -> ScheduleId {
        let id = self.state.next_id;
        self.state.next_id = id + 1;
        id
    }

    fn list(&self) -> Vec<(ScheduleId, Schedule)> {
        self.state
            .schedules
            .iter()
            .filter_map(|(id, encoded)| {
                let bytes = hex::decode(encoded).ok()?;
                let schedule = Schedule::try_from_slice(&bytes).ok()?;
                Some((*id, schedule))
            })
            .collect()
    }

    fn submit(&mut self, event: &Event) -> Result<(), BackendError> {
        let bytes =
            borsh::to_vec(event).map_err(|error| BackendError::Decode(error.to_string()))?;
        self.state.events.push(hex::encode(bytes));
        self.flush()
    }
}
