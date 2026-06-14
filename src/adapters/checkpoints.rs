//! Local synthetic-PR review state from `.circuit/checkpoints/`, the no-remote
//! fallback (M2b design §4). Maps to the SAME `ReviewState` as the forge so
//! `derive_stage` is backend-agnostic. One file per session, current-state
//! (Model B): writing overwrites, no history, no clock read.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::flow::facts::ReviewState;
use crate::model::store::Workspace;
use crate::ports::CheckpointStore;

/// The three checkpoint states (§3.2). Serializes kebab-case
/// (`"self-review" | "accepted" | "archived"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckpointState {
    SelfReview,
    Accepted,
    Archived,
}

impl CheckpointState {
    /// Map to the shared `ReviewState` (§4.2): self-review→Open, accepted→Merged,
    /// archived→Closed.
    pub fn review_state(self) -> ReviewState {
        match self {
            CheckpointState::SelfReview => ReviewState::Open,
            CheckpointState::Accepted => ReviewState::Merged,
            CheckpointState::Archived => ReviewState::Closed,
        }
    }
}

/// `.circuit/checkpoints/<session>.toml` — a local review snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub schema_version: u32,
    pub session: String,
    pub commit: String,
    pub state: CheckpointState,
    #[serde(default)]
    pub note: Option<String>,
}

/// Errors at the checkpoint persistence boundary. A missing file is NOT an error.
#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to serialize checkpoint: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Filesystem-backed `CheckpointStore` rooted at a `Workspace`.
pub struct FsCheckpointStore<'a> {
    ws: &'a Workspace,
}

impl<'a> FsCheckpointStore<'a> {
    pub fn new(ws: &'a Workspace) -> Self {
        Self { ws }
    }

    /// Persist a checkpoint, overwriting any prior state for this session.
    pub fn save(&self, record: &CheckpointRecord) -> Result<(), CheckpointError> {
        let path = self.ws.checkpoint_path(&record.session);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| CheckpointError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }
        let text = toml::to_string_pretty(record)?;
        std::fs::write(&path, text).map_err(|source| CheckpointError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}

impl CheckpointStore for FsCheckpointStore<'_> {
    type Error = CheckpointError;

    fn review_state(&self, session: &str) -> Result<ReviewState, Self::Error> {
        let path = self.ws.checkpoint_path(session);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReviewState::None);
            }
            Err(source) => {
                return Err(CheckpointError::Io {
                    path: path.display().to_string(),
                    source,
                });
            }
        };
        let record: CheckpointRecord =
            toml::from_str(&text).map_err(|source| CheckpointError::Parse {
                path: path.display().to_string(),
                source,
            })?;
        Ok(record.state.review_state())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session: &str, state: CheckpointState) -> CheckpointRecord {
        CheckpointRecord {
            schema_version: 1,
            session: session.to_string(),
            commit: "a1b2c3d".to_string(),
            state,
            note: Some("first pass".to_string()),
        }
    }

    #[test]
    fn absent_checkpoint_is_known_none() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        assert_eq!(
            store.review_state("01J-missing").unwrap(),
            ReviewState::None
        );
    }

    #[test]
    fn self_review_maps_to_open() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        store
            .save(&record("s1", CheckpointState::SelfReview))
            .unwrap();
        assert_eq!(store.review_state("s1").unwrap(), ReviewState::Open);
    }

    #[test]
    fn accepted_maps_to_merged() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        store
            .save(&record("s2", CheckpointState::Accepted))
            .unwrap();
        assert_eq!(store.review_state("s2").unwrap(), ReviewState::Merged);
    }

    #[test]
    fn archived_maps_to_closed() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        store
            .save(&record("s3", CheckpointState::Archived))
            .unwrap();
        assert_eq!(store.review_state("s3").unwrap(), ReviewState::Closed);
    }

    #[test]
    fn save_overwrites_prior_state() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        store
            .save(&record("s4", CheckpointState::SelfReview))
            .unwrap();
        store
            .save(&record("s4", CheckpointState::Accepted))
            .unwrap();
        assert_eq!(store.review_state("s4").unwrap(), ReviewState::Merged);
    }

    #[test]
    fn record_round_trips_through_toml() {
        let r = record("s5", CheckpointState::SelfReview);
        let text = toml::to_string_pretty(&r).unwrap();
        assert!(text.contains("state = \"self-review\""));
        let parsed: CheckpointRecord = toml::from_str(&text).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn note_is_optional() {
        let text = "schema_version = 1\nsession = \"s6\"\ncommit = \"abc\"\nstate = \"accepted\"\n";
        let r: CheckpointRecord = toml::from_str(text).unwrap();
        assert!(r.note.is_none());
        assert_eq!(r.state, CheckpointState::Accepted);
    }
}
