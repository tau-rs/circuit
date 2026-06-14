//! Health roll-up from a worktree (design §9). The pure roll-up math lives in
//! `cockpit::health`; this module discovers a node's worktree via `GitPort` and
//! runs M1's indicators against it. No worktree => `Unknown` (no bare-git
//! reconstruction). Also computes traceability `m/n` (design §8.3).

use std::path::Path;

use crate::builder::build_graph;
use crate::cockpit::health::{Health, SessionHealth};
use crate::indicators::cycles::find_cycles;
use crate::indicators::dependency_rule::violations;
use crate::model::node::DagNode;
use crate::ports::GitPort;

/// Discover the worktree for `branch` via the port and measure it. No matching
/// worktree, or a `list_worktrees` error, yields `Unknown` (design §9).
pub fn node_health<G: GitPort>(git: &G, branch: &str) -> Health {
    let worktrees = match git.list_worktrees() {
        Ok(w) => w,
        Err(_) => return Health::Unknown,
    };
    match worktrees.into_iter().find(|w| w.branch.as_deref() == Some(branch)) {
        Some(w) => health_at_worktree(&w.path),
        None => Health::Unknown,
    }
}

/// Run M1's indicators against a worktree and roll the counts up. A build error
/// (e.g. the path vanished) yields `Unknown` — we measured nothing, claim nothing.
pub fn health_at_worktree(path: &Path) -> Health {
    match build_graph(path) {
        Ok(graph) => SessionHealth {
            cycles: find_cycles(&graph).len(),
            dep_violations: violations(&graph).len(),
        }
        .rollup(),
        Err(_) => Health::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::facts::BranchFacts;
    use crate::ports::{GitPort, Worktree};
    use std::collections::HashMap;
    use std::path::{Path as StdPath, PathBuf};

    fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[derive(Debug)]
    struct FakeError;
    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "fake error")
        }
    }
    impl std::error::Error for FakeError {}

    /// Configurable fake: a list of worktrees, a branch->merged map for facts,
    /// and a switch to force `branch_facts` to error.
    struct FakeGit {
        worktrees: Vec<Worktree>,
        merged: HashMap<String, bool>,
        facts_err: bool,
    }
    impl FakeGit {
        fn new() -> Self {
            Self { worktrees: vec![], merged: HashMap::new(), facts_err: false }
        }
    }
    impl GitPort for FakeGit {
        type Error = FakeError;
        fn branch_facts(&self, branch: &str, _base: &str) -> Result<BranchFacts, Self::Error> {
            if self.facts_err {
                return Err(FakeError);
            }
            Ok(BranchFacts {
                exists: true,
                merged_into_base: *self.merged.get(branch).unwrap_or(&false),
                ..Default::default()
            })
        }
        fn create_branch(&self, _branch: &str, _base: &str) -> Result<(), Self::Error> {
            Err(FakeError)
        }
        fn add_worktree(&self, _branch: &str, _path: &StdPath) -> Result<(), Self::Error> {
            Err(FakeError)
        }
        fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error> {
            Ok(self.worktrees.clone())
        }
    }

    #[test]
    fn node_health_measures_the_matching_worktree() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/a.rs", "use crate::b::X;");
        write(dir.path(), "src/b.rs", "use crate::a::Y;");
        let mut git = FakeGit::new();
        git.worktrees = vec![Worktree {
            path: dir.path().to_path_buf(),
            branch: Some("impl/cycle".to_string()),
        }];
        assert_eq!(node_health(&git, "impl/cycle"), Health::Critical);
    }

    #[test]
    fn node_health_is_unknown_when_no_worktree_matches() {
        let mut git = FakeGit::new();
        git.worktrees = vec![Worktree {
            path: PathBuf::from("/tmp/other"),
            branch: Some("impl/other".to_string()),
        }];
        assert_eq!(node_health(&git, "impl/missing"), Health::Unknown);
    }

    #[test]
    fn node_health_is_unknown_when_list_worktrees_errors() {
        // A port that errors on list_worktrees.
        struct ErrGit;
        impl GitPort for ErrGit {
            type Error = FakeError;
            fn branch_facts(&self, _b: &str, _base: &str) -> Result<BranchFacts, Self::Error> {
                Err(FakeError)
            }
            fn create_branch(&self, _b: &str, _base: &str) -> Result<(), Self::Error> {
                Err(FakeError)
            }
            fn add_worktree(&self, _b: &str, _p: &StdPath) -> Result<(), Self::Error> {
                Err(FakeError)
            }
            fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error> {
                Err(FakeError)
            }
        }
        assert_eq!(node_health(&ErrGit, "impl/x"), Health::Unknown);
    }

    #[test]
    fn clean_layered_worktree_is_sound() {
        let dir = tempfile::tempdir().unwrap();
        // adapters -> domain : outer depends on inner, no cycle, no violation.
        write(dir.path(), "src/domain.rs", "pub struct Order;");
        write(dir.path(), "src/adapters.rs", "use crate::domain::Order;");
        assert_eq!(health_at_worktree(dir.path()), Health::Sound);
    }

    #[test]
    fn worktree_with_a_cycle_is_critical() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/a.rs", "use crate::b::X;");
        write(dir.path(), "src/b.rs", "use crate::a::Y;");
        assert_eq!(health_at_worktree(dir.path()), Health::Critical);
    }

    #[test]
    fn worktree_with_a_dependency_violation_is_critical() {
        let dir = tempfile::tempdir().unwrap();
        // domain -> adapters : inner depends on outer = violation.
        write(dir.path(), "src/domain.rs", "use crate::adapters::Db;");
        write(dir.path(), "src/adapters.rs", "pub struct Db;");
        assert_eq!(health_at_worktree(dir.path()), Health::Critical);
    }

    #[test]
    fn nonexistent_path_is_unknown() {
        assert_eq!(
            health_at_worktree(Path::new("/no/such/worktree/xyz")),
            Health::Unknown
        );
    }
}
