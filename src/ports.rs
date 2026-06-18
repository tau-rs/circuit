//! Outbound port traits — the IO boundary the domain depends on. Signatures
//! only; implementations live in the adapter slices (git/`gh` shell-out,
//! checkpoint store). Each port carries an associated `Error` so adapters bring
//! their own `thiserror` type without the foundation guessing failure modes (§6).

use std::path::{Path, PathBuf};

use crate::flow::facts::{BranchFacts, ReviewState};
use crate::model::config::Config;
use crate::model::glossary::Glossary;
use crate::model::local::LocalConfig;
use crate::model::node::DagNode;
use crate::model::spec::SpecRecord;
use crate::session::SessionRecord;

/// One entry from `git worktree list`. The path is derived at runtime and never
/// stored; `branch` is `None` for a detached-HEAD worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// Git operations: branch facts plus worktree orchestration. Offline-capable.
pub trait GitPort {
    type Error: std::error::Error + Send + Sync + 'static;

    fn branch_facts(&self, branch: &str, base: &str) -> Result<BranchFacts, Self::Error>;
    fn create_branch(&self, branch: &str, base: &str) -> Result<(), Self::Error>;
    fn add_worktree(&self, branch: &str, path: &Path) -> Result<(), Self::Error>;
    fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error>;
    /// Remove a worktree dir. `force` removes a dirty/locked worktree
    /// (`git worktree remove --force`). The branch is never touched.
    fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), Self::Error>;
    /// Delete a branch. `force` (`git branch -D`) deletes an un-merged branch;
    /// without it (`-d`) git refuses an un-merged branch.
    fn delete_branch(&self, branch: &str, force: bool) -> Result<(), Self::Error>;
}

/// Forge operations (GitHub via `gh` in M2). `review_state` returning `Err`
/// (forge unreachable) is mapped by the caller into `DeliveryFacts.review = None`.
pub trait ForgePort {
    type Error: std::error::Error + Send + Sync + 'static;

    fn review_state(&self, branch: &str) -> Result<ReviewState, Self::Error>;
    fn create_pr(
        &self,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<(), Self::Error>;
    fn merge(&self, branch: &str) -> Result<(), Self::Error>;
    fn update_from_base(&self, branch: &str, base: &str) -> Result<(), Self::Error>;
}

/// Local synthetic-PR review state from `.circuit/checkpoints/`, the no-remote
/// fallback. Returns `Ok(ReviewState::None)` when no checkpoint exists.
pub trait CheckpointStore {
    type Error: std::error::Error + Send + Sync + 'static;

    fn review_state(&self, session: &str) -> Result<ReviewState, Self::Error>;
}

/// Persistence for project-level settings: `config.toml`, `glossary.toml`,
/// and the machine-local `local.toml`. `is_initialized` is a fast existence
/// check used by every command to gate "not yet init" errors.
pub trait SettingsRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn is_initialized(&self) -> bool;
    fn load_config(&self) -> Result<Config, Self::Error>;
    fn save_config(&self, c: &Config) -> Result<(), Self::Error>;
    fn load_glossary(&self) -> Result<Glossary, Self::Error>;
    fn save_glossary(&self, g: &Glossary) -> Result<(), Self::Error>;
    fn load_local(&self) -> Result<LocalConfig, Self::Error>;
}

/// Persistence for spec sessions' authored intent (`specs/<id>.toml`).
pub trait SpecRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_spec(&self, id: &str) -> Result<SpecRecord, Self::Error>;
    fn save_spec(&self, s: &SpecRecord) -> Result<(), Self::Error>;
}

/// Persistence for DAG nodes (`dag/<id>.toml`). `list_dag_nodes` is used by
/// board rendering and cycle checks.
pub trait DagRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_dag_node(&self, id: &str) -> Result<DagNode, Self::Error>;
    fn save_dag_node(&self, n: &DagNode) -> Result<(), Self::Error>;
    fn list_dag_nodes(&self) -> Result<Vec<DagNode>, Self::Error>;
}

/// Persistence for session records (`sessions/<id>.toml`). `list_sessions` is
/// used by board rendering and the `resolve_session` helper.
pub trait SessionRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_session(&self, id: &str) -> Result<SessionRecord, Self::Error>;
    fn save_session(&self, s: &SessionRecord) -> Result<(), Self::Error>;
    fn list_sessions(&self) -> Result<Vec<SessionRecord>, Self::Error>;
}

/// Read-only probes for delivery-mode detection. Separated from `ForgePort` so
/// tests can inject a simple bool pair without a full forge stub.
pub trait DeliveryProbe {
    fn gh_available(&self) -> bool;
    fn has_github_remote(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trivial error type satisfying the associated-error bound.
    #[derive(Debug)]
    struct FakeError;

    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "fake error")
        }
    }
    impl std::error::Error for FakeError {}

    struct FakeGit;
    impl GitPort for FakeGit {
        type Error = FakeError;
        fn branch_facts(&self, _branch: &str, _base: &str) -> Result<BranchFacts, Self::Error> {
            Ok(BranchFacts::default())
        }
        fn create_branch(&self, _branch: &str, _base: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn add_worktree(&self, _branch: &str, _path: &Path) -> Result<(), Self::Error> {
            Ok(())
        }
        fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error> {
            Ok(vec![Worktree {
                path: PathBuf::from("/tmp/wt"),
                branch: Some("impl/x".to_string()),
            }])
        }
        fn remove_worktree(&self, _path: &Path, _force: bool) -> Result<(), Self::Error> {
            Ok(())
        }
        fn delete_branch(&self, _branch: &str, _force: bool) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct FakeForge;
    impl ForgePort for FakeForge {
        type Error = FakeError;
        fn review_state(&self, _branch: &str) -> Result<ReviewState, Self::Error> {
            Ok(ReviewState::Open)
        }
        fn create_pr(
            &self,
            _branch: &str,
            _base: &str,
            _title: &str,
            _body: &str,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
        fn merge(&self, _branch: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn update_from_base(&self, _branch: &str, _base: &str) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct FakeCheckpoints;
    impl CheckpointStore for FakeCheckpoints {
        type Error = FakeError;
        fn review_state(&self, _session: &str) -> Result<ReviewState, Self::Error> {
            Ok(ReviewState::None)
        }
    }

    #[test]
    fn ports_are_implementable_and_callable() {
        let g = FakeGit;
        assert_eq!(g.branch_facts("b", "main").unwrap(), BranchFacts::default());
        assert!(g.create_branch("b", "main").is_ok());
        assert!(g.add_worktree("b", Path::new("/tmp/wt")).is_ok());
        assert_eq!(g.list_worktrees().unwrap().len(), 1);

        let f = FakeForge;
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Open);
        assert!(f.create_pr("b", "main", "t", "body").is_ok());
        assert!(f.merge("b").is_ok());
        assert!(f.update_from_base("b", "main").is_ok());

        let c = FakeCheckpoints;
        assert_eq!(c.review_state("session-id").unwrap(), ReviewState::None);
    }
}
