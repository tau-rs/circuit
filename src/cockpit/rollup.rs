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

    fn write(dir: &Path, rel: &str, body: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
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
