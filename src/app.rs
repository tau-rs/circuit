//! Application-layer orchestration for the forge actions and checkpoint writing
//! (M2b §5). Generic over `ForgePort` so the logic is exercised in tests with a
//! fake forge. `anyhow` internally; the adapters carry the typed errors.

use anyhow::{Context, Result};

use crate::adapters::checkpoints::{CheckpointRecord, CheckpointState, FsCheckpointStore};
use crate::model::store::Workspace;
use crate::ports::ForgePort;
use crate::session::SessionRecord;

/// Resolve a session id to `(record, branch)`, erroring clearly when the session
/// is missing or has no branch yet.
fn session_branch(ws: &Workspace, session: &str) -> Result<(SessionRecord, String)> {
    let record = ws
        .load_session(session)
        .with_context(|| format!("no such session: {session}"))?;
    let branch = record
        .branch
        .clone()
        .with_context(|| format!("session {session} has no branch yet"))?;
    Ok((record, branch))
}

/// Open a PR for the session's branch. Title/body default to the authored DAG
/// node (title + intent); explicit args override.
pub fn pr_create<F: ForgePort>(
    ws: &Workspace,
    forge: &F,
    session: &str,
    title: Option<String>,
    body: Option<String>,
) -> Result<()> {
    let (record, branch) = session_branch(ws, session)?;
    let base = ws.load_config().context("loading config")?.base_branch;

    let (default_title, default_body) = match &record.dag_node {
        Some(node_id) => {
            let node = ws
                .load_dag_node(node_id)
                .with_context(|| format!("loading dag node {node_id}"))?;
            (node.title, node.intent)
        }
        None => (branch.clone(), String::new()),
    };
    let title = title.unwrap_or(default_title);
    let body = body.unwrap_or(default_body);

    forge
        .create_pr(&branch, &base, &title, &body)
        .with_context(|| format!("creating PR for {branch}"))?;
    println!("Opened PR for session {session} ({branch})");
    Ok(())
}

/// Merge the session's PR.
pub fn pr_merge<F: ForgePort>(ws: &Workspace, forge: &F, session: &str) -> Result<()> {
    let (_record, branch) = session_branch(ws, session)?;
    forge
        .merge(&branch)
        .with_context(|| format!("merging {branch}"))?;
    println!("Merged session {session} ({branch})");
    Ok(())
}

/// Update the session's PR branch from base.
pub fn pr_update_from_base<F: ForgePort>(ws: &Workspace, forge: &F, session: &str) -> Result<()> {
    let (_record, branch) = session_branch(ws, session)?;
    let base = ws.load_config().context("loading config")?.base_branch;
    forge
        .update_from_base(&branch, &base)
        .with_context(|| format!("updating {branch} from {base}"))?;
    println!("Updated session {session} ({branch}) from {base}");
    Ok(())
}

/// Write a local checkpoint for a session (the no-remote review-state substitute).
/// The session must exist (its id is the checkpoint key); a branch is not required.
pub fn write_checkpoint(
    ws: &Workspace,
    session: &str,
    state: CheckpointState,
    commit: String,
    note: Option<String>,
) -> Result<()> {
    ws.load_session(session)
        .with_context(|| format!("no such session: {session}"))?;
    let record = CheckpointRecord {
        schema_version: 1,
        session: session.to_string(),
        commit,
        state,
        note,
    };
    FsCheckpointStore::new(ws)
        .save(&record)
        .with_context(|| format!("writing checkpoint for {session}"))?;
    println!("Checkpoint recorded for session {session}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use crate::flow::facts::ReviewState;
    use crate::model::config::Config;
    use crate::model::node::DagNode;
    use crate::session::{SessionId, SessionRecord};

    /// A fake forge that records the action it was asked to perform.
    #[derive(Default)]
    struct FakeForge {
        calls: RefCell<Vec<String>>,
    }

    impl ForgePort for FakeForge {
        type Error = std::convert::Infallible;

        fn review_state(&self, _branch: &str) -> Result<ReviewState, Self::Error> {
            Ok(ReviewState::None)
        }
        fn create_pr(
            &self,
            branch: &str,
            base: &str,
            title: &str,
            body: &str,
        ) -> Result<(), Self::Error> {
            self.calls
                .borrow_mut()
                .push(format!("create_pr|{branch}|{base}|{title}|{body}"));
            Ok(())
        }
        fn merge(&self, branch: &str) -> Result<(), Self::Error> {
            self.calls.borrow_mut().push(format!("merge|{branch}"));
            Ok(())
        }
        fn update_from_base(&self, branch: &str, base: &str) -> Result<(), Self::Error> {
            self.calls
                .borrow_mut()
                .push(format!("update|{branch}|{base}"));
            Ok(())
        }
    }

    /// An initialized workspace with one impl session + its DAG node.
    fn workspace_with_impl_session() -> (tempfile::TempDir, Workspace, String) {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        ws.save_config(&Config::default()).unwrap();

        let mut node = DagNode::new("auth-slice", "checkout", "Auth slice", "impl/checkout-auth");
        node.intent = "Log in and gate checkout.".to_string();
        ws.save_dag_node(&node).unwrap();

        let session = SessionRecord::impl_(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "impl/checkout-auth",
        );
        let id = session.id.to_string();
        ws.save_session(&session).unwrap();
        (dir, ws, id)
    }

    #[test]
    fn pr_create_resolves_branch_and_derives_title_body_from_dag_node() {
        let (_dir, ws, id) = workspace_with_impl_session();
        let forge = FakeForge::default();
        pr_create(&ws, &forge, &id, None, None).unwrap();
        assert_eq!(
            forge.calls.borrow().as_slice(),
            ["create_pr|impl/checkout-auth|main|Auth slice|Log in and gate checkout."]
        );
    }

    #[test]
    fn pr_create_honors_explicit_title_and_body() {
        let (_dir, ws, id) = workspace_with_impl_session();
        let forge = FakeForge::default();
        pr_create(&ws, &forge, &id, Some("T".into()), Some("B".into())).unwrap();
        assert_eq!(
            forge.calls.borrow().as_slice(),
            ["create_pr|impl/checkout-auth|main|T|B"]
        );
    }

    #[test]
    fn pr_create_fails_for_missing_session() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        ws.save_config(&Config::default()).unwrap();
        let forge = FakeForge::default();
        let err = pr_create(&ws, &forge, "01J-missing", None, None).unwrap_err();
        assert!(err.to_string().contains("no such session"));
        assert!(forge.calls.borrow().is_empty());
    }

    #[test]
    fn pr_create_fails_for_branchless_session() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        ws.save_config(&Config::default()).unwrap();
        let spec = SessionRecord::spec(SessionId::generate());
        let id = spec.id.to_string();
        ws.save_session(&spec).unwrap();
        let forge = FakeForge::default();
        let err = pr_create(&ws, &forge, &id, None, None).unwrap_err();
        assert!(err.to_string().contains("no branch"));
        assert!(forge.calls.borrow().is_empty());
    }

    #[test]
    fn pr_merge_calls_forge_merge() {
        let (_dir, ws, id) = workspace_with_impl_session();
        let forge = FakeForge::default();
        pr_merge(&ws, &forge, &id).unwrap();
        assert_eq!(
            forge.calls.borrow().as_slice(),
            ["merge|impl/checkout-auth"]
        );
    }

    #[test]
    fn pr_update_from_base_passes_config_base() {
        let (_dir, ws, id) = workspace_with_impl_session();
        let forge = FakeForge::default();
        pr_update_from_base(&ws, &forge, &id).unwrap();
        assert_eq!(
            forge.calls.borrow().as_slice(),
            ["update|impl/checkout-auth|main"]
        );
    }

    #[test]
    fn write_checkpoint_persists_state_for_the_session() {
        let (dir, ws, id) = workspace_with_impl_session();
        write_checkpoint(
            &ws,
            &id,
            CheckpointState::SelfReview,
            "deadbeef".into(),
            None,
        )
        .unwrap();
        let path = dir.path().join(format!(".circuit/checkpoints/{id}.toml"));
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("state = \"self-review\""));
        assert!(text.contains("commit = \"deadbeef\""));
    }

    #[test]
    fn write_checkpoint_fails_for_missing_session() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        ws.save_config(&Config::default()).unwrap();
        let err = write_checkpoint(
            &ws,
            "01J-missing",
            CheckpointState::Accepted,
            "x".into(),
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no such session"));
    }
}
