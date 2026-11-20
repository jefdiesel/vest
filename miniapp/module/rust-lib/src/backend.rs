//! The SDK `Backend` implementations used by the module skeleton.
//!
//! `JsonFileBackend` (default feature `json-file-backend`) stores schedules and events as
//! hex-encoded borsh bytes in a JSON file, exactly like the CLI's `cli/src/backend.rs`. The
//! LEZ backend will be written separately later. `MemoryBackend` is the fallback when the
//! feature is off, so the crate also builds without file I/O.

#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
use borsh::BorshDeserialize;
#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
use serde::{Deserialize, Serialize};
#[cfg(not(feature = "lez"))]
use std::collections::BTreeMap;
#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
use std::fs;
#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
use std::path::{Path, PathBuf};
#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(not(feature = "lez"))]
use vesting_core::{Event, Schedule, Timestamp};

#[cfg(not(feature = "lez"))]
pub use vesting_sdk::{Backend, BackendError, ScheduleId};

#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
#[derive(Serialize, Deserialize, Default)]
struct StateFile {
    next_id: ScheduleId,
    /// id -> hex(borsh(Schedule))
    schedules: BTreeMap<ScheduleId, String>,
    /// hex(borsh(Event)), in submission order
    events: Vec<String>,
}

/// A JSON-file implementation of the SDK's `Backend` trait.
#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
pub struct JsonFileBackend {
    path: PathBuf,
    now: Timestamp,
    state: StateFile,
}

#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
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

#[cfg(all(not(feature = "lez"), feature = "json-file-backend"))]
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
        let bytes = hex::decode(encoded).map_err(|e| BackendError::Decode(e.to_string()))?;
        Schedule::try_from_slice(&bytes).map_err(|e| BackendError::Decode(e.to_string()))
    }

    fn store(&mut self, id: ScheduleId, schedule: &Schedule) -> Result<(), BackendError> {
        let bytes = borsh::to_vec(schedule).map_err(|e| BackendError::Decode(e.to_string()))?;
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
        let bytes = borsh::to_vec(event).map_err(|e| BackendError::Decode(e.to_string()))?;
        self.state.events.push(hex::encode(bytes));
        self.flush()
    }
}

/// In-memory backend for builds without the JSON-file feature.
#[cfg(all(not(feature = "lez"), not(feature = "json-file-backend")))]
#[derive(Default)]
pub struct MemoryBackend {
    now: Timestamp,
    next_id: ScheduleId,
    schedules: BTreeMap<ScheduleId, Schedule>,
    events: Vec<Event>,
}

#[cfg(all(not(feature = "lez"), not(feature = "json-file-backend")))]
impl MemoryBackend {
    pub fn open(now: Option<Timestamp>) -> Self {
        Self {
            now: now.unwrap_or_default(),
            ..Self::default()
        }
    }
}

#[cfg(all(not(feature = "lez"), not(feature = "json-file-backend")))]
impl Backend for MemoryBackend {
    fn now(&self) -> Timestamp {
        self.now
    }
    fn load(&self, id: ScheduleId) -> Result<Schedule, BackendError> {
        self.schedules
            .get(&id)
            .cloned()
            .ok_or(BackendError::NotFound(id))
    }
    fn store(&mut self, id: ScheduleId, schedule: &Schedule) -> Result<(), BackendError> {
        self.schedules.insert(id, schedule.clone());
        Ok(())
    }
    fn next_id(&mut self) -> ScheduleId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    fn list(&self) -> Vec<(ScheduleId, Schedule)> {
        self.schedules
            .iter()
            .map(|(id, s)| (*id, s.clone()))
            .collect()
    }
    fn submit(&mut self, event: &Event) -> Result<(), BackendError> {
        self.events.push(event.clone());
        Ok(())
    }
}
