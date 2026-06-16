//! `CheckpointStore` reading `.circuit/checkpoints/<session-ULID>.toml`, the
//! no-remote review fallback. An absent file is a *known* `ReviewState::None`,
//! not an error (§6). Local checkpoints carry only `self-review` / `accepted`;
//! cancellation is session archival (Axis 2), out of this slice.

use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use crate::flow::facts::ReviewState;
use crate::ports::CheckpointStore;

/// Errors from reading or parsing a checkpoint file.
#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("failed to read checkpoint file {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse checkpoint file: {0}")]
    Parse(#[source] toml::de::Error),
    #[error("unknown checkpoint state `{0}` (expected self-review|accepted)")]
    UnknownState(String),
}

/// On-disk shape of `<session>.toml`. Only `state` is read in this slice; a
/// future slice adds a `snapshots` log, which serde ignores here.
#[derive(Debug, Deserialize)]
struct CheckpointFile {
    state: String,
}

/// `CheckpointStore` rooted at a working tree.
pub struct Checkpoints {
    root: PathBuf,
}

impl Checkpoints {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, session: &str) -> PathBuf {
        self.root
            .join(".circuit")
            .join("checkpoints")
            .join(format!("{session}.toml"))
    }
}

/// Map a checkpoint file's `state` to a ReviewState. Pure.
fn parse_checkpoint_state(contents: &str) -> Result<ReviewState, CheckpointError> {
    let cp: CheckpointFile = toml::from_str(contents).map_err(CheckpointError::Parse)?;
    match cp.state.as_str() {
        "self-review" => Ok(ReviewState::Open),
        "accepted" => Ok(ReviewState::Approved),
        other => Err(CheckpointError::UnknownState(other.to_string())),
    }
}

impl CheckpointStore for Checkpoints {
    type Error = CheckpointError;

    fn review_state(&self, session: &str) -> Result<ReviewState, CheckpointError> {
        let path = self.path_for(session);
        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReviewState::None)
            }
            Err(e) => {
                return Err(CheckpointError::Read {
                    path: path.display().to_string(),
                    source: e,
                })
            }
        };
        parse_checkpoint_state(&contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn self_review_maps_to_open() {
        assert_eq!(
            parse_checkpoint_state("state = \"self-review\"").unwrap(),
            ReviewState::Open
        );
    }

    #[test]
    fn accepted_maps_to_approved() {
        assert_eq!(
            parse_checkpoint_state("state = \"accepted\"").unwrap(),
            ReviewState::Approved
        );
    }

    #[test]
    fn unknown_state_is_error() {
        assert!(matches!(
            parse_checkpoint_state("state = \"archived\""),
            Err(CheckpointError::UnknownState(_))
        ));
    }

    #[test]
    fn malformed_toml_is_parse_error() {
        assert!(matches!(
            parse_checkpoint_state("not = toml = ="),
            Err(CheckpointError::Parse(_))
        ));
    }

    #[test]
    fn absent_file_is_known_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = Checkpoints::new(dir.path());
        assert_eq!(
            store.review_state("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            ReviewState::None
        );
    }

    // Integration: a real fixture file at the resolved path round-trips through
    // the full review_state read path.
    #[test]
    fn present_file_is_read_from_resolved_path() {
        let dir = tempfile::tempdir().unwrap();
        let cp_dir = dir.path().join(".circuit").join("checkpoints");
        fs::create_dir_all(&cp_dir).unwrap();
        fs::write(cp_dir.join("SID.toml"), "state = \"self-review\"\n").unwrap();

        let store = Checkpoints::new(dir.path());
        assert_eq!(store.review_state("SID").unwrap(), ReviewState::Open);
    }
}
