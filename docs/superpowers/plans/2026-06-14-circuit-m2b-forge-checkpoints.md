# M2 Slice B — Forge + Checkpoints + PR Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `ForgePort` (GitHub `gh` shell-out) and `CheckpointStore` (`.circuit/checkpoints/`) review-state backends plus the `circuit pr …` / `circuit checkpoint` CLI actions, against the foundation's frozen port traits.

**Architecture:** Hexagonal. Two adapters behind the foundation's port traits: `GhForge<R: CommandRunner>` (injected command runner → offline-testable parse/arg logic) and `FsCheckpointStore` (one TOML file per session, current-state). A thin port-generic `app.rs` resolves `<session>` → branch and drives the ports — the seam unit-tested with a fake `ForgePort`. `main.rs` wires real adapters into new clap subcommands.

**Tech Stack:** Rust 2021, `#![forbid(unsafe_code)]`, `serde`/`toml`/`serde_json`, `thiserror` at adapter boundaries, `anyhow` inside app/main, `clap` derive, `assert_cmd`/`tempfile`/`predicates` for tests.

**Design ref:** `docs/superpowers/specs/2026-06-14-circuit-m2b-forge-checkpoints-design.md`

**Conventions to honor:** inline `#[cfg(test)] mod tests` per source file (as in `ports.rs`, `session/mod.rs`, `model/store.rs`); `anyhow::Context` for CLI errors; `--path` defaults to `.`; conventional commits.

---

## Task 1: Checkpoint store (`adapters/checkpoints.rs`)

**Files:**
- Create: `src/adapters/mod.rs`
- Create: `src/adapters/checkpoints.rs`
- Modify: `src/lib.rs` (add `pub mod adapters;`)
- Modify: `src/model/store.rs` (add `checkpoints_dir` / `checkpoint_path`)

- [ ] **Step 1: Wire the module tree**

Create `src/adapters/mod.rs`:

```rust
//! IO adapters behind the foundation's port traits (M2 §6). Each adapter brings
//! its own `thiserror` error so the foundation never guesses failure modes.

pub mod checkpoints;
```

Add to `src/lib.rs` after `pub mod model;` (keep the list alphabetic-ish, matching existing order — insert before `pub mod ports;`):

```rust
pub mod adapters;
```

Add to `src/model/store.rs`, inside `impl Workspace` (next to `sessions_dir` / `session_path`):

```rust
    pub fn checkpoints_dir(&self) -> PathBuf {
        self.circuit_dir().join("checkpoints")
    }

    pub fn checkpoint_path(&self, session: &str) -> PathBuf {
        self.checkpoints_dir().join(format!("{session}.toml"))
    }
```

- [ ] **Step 2: Write the failing tests**

Create `src/adapters/checkpoints.rs`:

```rust
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
        let record: CheckpointRecord = toml::from_str(&text).map_err(|source| {
            CheckpointError::Parse {
                path: path.display().to_string(),
                source,
            }
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
        assert_eq!(store.review_state("01J-missing").unwrap(), ReviewState::None);
    }

    #[test]
    fn self_review_maps_to_open() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        store.save(&record("s1", CheckpointState::SelfReview)).unwrap();
        assert_eq!(store.review_state("s1").unwrap(), ReviewState::Open);
    }

    #[test]
    fn accepted_maps_to_merged() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        store.save(&record("s2", CheckpointState::Accepted)).unwrap();
        assert_eq!(store.review_state("s2").unwrap(), ReviewState::Merged);
    }

    #[test]
    fn archived_maps_to_closed() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        store.save(&record("s3", CheckpointState::Archived)).unwrap();
        assert_eq!(store.review_state("s3").unwrap(), ReviewState::Closed);
    }

    #[test]
    fn save_overwrites_prior_state() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        let store = FsCheckpointStore::new(&ws);
        store.save(&record("s4", CheckpointState::SelfReview)).unwrap();
        store.save(&record("s4", CheckpointState::Accepted)).unwrap();
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
```

- [ ] **Step 3: Run tests to verify they fail to compile/pass**

Run: `cargo test --lib adapters::checkpoints`
Expected: FAIL — first a compile error if `checkpoint_path` was mistyped, then green once Step 1+2 are in. (If it does not compile, fix the module wiring in Step 1.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib adapters::checkpoints`
Expected: PASS — 7 tests.

- [ ] **Step 5: Verify the whole crate still builds and is clippy-clean**

Run: `cargo build && cargo clippy --all-targets -- -D warnings`
Expected: no errors, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/adapters/mod.rs src/adapters/checkpoints.rs src/lib.rs src/model/store.rs
git commit -m "feat: checkpoint store — .circuit/checkpoints as no-remote ReviewState"
```

---

## Task 2: Forge adapter (`adapters/forge.rs`)

**Files:**
- Create: `src/adapters/forge.rs`
- Modify: `src/adapters/mod.rs` (add `pub mod forge;`)
- Modify: `Cargo.toml` (add `serde_json`)

- [ ] **Step 1: Add the dependency and module**

Add to `Cargo.toml` under `[dependencies]` (after the `toml` line):

```toml
serde_json = "1"
```

Add to `src/adapters/mod.rs`:

```rust
pub mod forge;
```

- [ ] **Step 2: Write the failing tests + implementation skeleton**

Create `src/adapters/forge.rs`:

```rust
//! GitHub forge adapter — `ForgePort` by shelling out to the `gh` CLI (M2b §3).
//! The command runner is injected so JSON-parse and argument-construction logic
//! is unit-testable offline; only `SystemRunner` spawns a real process.

use serde::Deserialize;
use thiserror::Error;

use crate::flow::facts::ReviewState;
use crate::ports::ForgePort;

/// Output of a finished command, owned so fakes need not build a `process::Output`.
pub struct CommandOutput {
    pub success: bool,
    pub status: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// The injectable process boundary. Production is `SystemRunner`; tests supply a
/// fake returning canned `CommandOutput`.
pub trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<CommandOutput>;
}

/// Spawns real processes via `std::process::Command`.
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<CommandOutput> {
        let out = std::process::Command::new(program).args(args).output()?;
        Ok(CommandOutput {
            success: out.status.success(),
            status: out.status.to_string(),
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }
}

/// Errors at the forge boundary.
#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("failed to launch {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("`gh {args}` failed ({status}): {stderr}")]
    Command {
        args: String,
        status: String,
        stderr: String,
    },
    #[error("failed to parse gh output: {source}")]
    Parse {
        #[source]
        source: serde_json::Error,
    },
}

/// One PR row from `gh pr list --json state,reviewDecision`.
#[derive(Debug, Deserialize)]
struct GhPr {
    state: String,
    #[serde(rename = "reviewDecision", default)]
    review_decision: Option<String>,
}

/// GitHub forge over the `gh` CLI, generic over the command runner.
pub struct GhForge<R = SystemRunner> {
    runner: R,
}

impl GhForge<SystemRunner> {
    pub fn new() -> Self {
        Self { runner: SystemRunner }
    }
}

impl Default for GhForge<SystemRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: CommandRunner> GhForge<R> {
    pub fn with_runner(runner: R) -> Self {
        Self { runner }
    }

    /// Run `gh <args>`, returning stdout on success or a typed error.
    fn gh(&self, args: &[&str]) -> Result<Vec<u8>, ForgeError> {
        let out = self.runner.run("gh", args).map_err(|source| ForgeError::Spawn {
            program: "gh".to_string(),
            source,
        })?;
        if !out.success {
            return Err(ForgeError::Command {
                args: args.join(" "),
                status: out.status,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(out.stdout)
    }
}

impl<R: CommandRunner> ForgePort for GhForge<R> {
    type Error = ForgeError;

    fn review_state(&self, branch: &str) -> Result<ReviewState, Self::Error> {
        let stdout = self.gh(&[
            "pr", "list", "--head", branch, "--state", "all", "--json",
            "state,reviewDecision",
        ])?;
        let prs: Vec<GhPr> =
            serde_json::from_slice(&stdout).map_err(|source| ForgeError::Parse { source })?;
        let Some(pr) = prs.into_iter().next() else {
            return Ok(ReviewState::None);
        };
        Ok(match pr.state.as_str() {
            "MERGED" => ReviewState::Merged,
            "CLOSED" => ReviewState::Closed,
            "OPEN" if pr.review_decision.as_deref() == Some("APPROVED") => ReviewState::Approved,
            // OPEN (no approval) and any unknown future state: conservative Open.
            _ => ReviewState::Open,
        })
    }

    fn create_pr(
        &self,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<(), Self::Error> {
        self.gh(&[
            "pr", "create", "--head", branch, "--base", base, "--title", title, "--body", body,
        ])?;
        Ok(())
    }

    fn merge(&self, branch: &str) -> Result<(), Self::Error> {
        self.gh(&["pr", "merge", branch, "--merge"])?;
        Ok(())
    }

    fn update_from_base(&self, branch: &str, _base: &str) -> Result<(), Self::Error> {
        // `gh pr update-branch` updates the PR head from its base branch.
        self.gh(&["pr", "update-branch", branch])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records the args of each call and returns a preset outcome.
    struct FakeRunner {
        out: Option<CommandOutput>,
        spawn_err: bool,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn ok(stdout: &str) -> Self {
            Self {
                out: Some(CommandOutput {
                    success: true,
                    status: "exit status: 0".to_string(),
                    stdout: stdout.as_bytes().to_vec(),
                    stderr: Vec::new(),
                }),
                spawn_err: false,
                calls: RefCell::new(Vec::new()),
            }
        }
        fn fail(stderr: &str) -> Self {
            Self {
                out: Some(CommandOutput {
                    success: false,
                    status: "exit status: 1".to_string(),
                    stdout: Vec::new(),
                    stderr: stderr.as_bytes().to_vec(),
                }),
                spawn_err: false,
                calls: RefCell::new(Vec::new()),
            }
        }
        fn missing() -> Self {
            Self {
                out: None,
                spawn_err: true,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, _program: &str, args: &[&str]) -> std::io::Result<CommandOutput> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            if self.spawn_err {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no gh"));
            }
            let o = self.out.as_ref().unwrap();
            Ok(CommandOutput {
                success: o.success,
                status: o.status.clone(),
                stdout: o.stdout.clone(),
                stderr: o.stderr.clone(),
            })
        }
    }

    fn last_call(f: &GhForge<FakeRunner>) -> Vec<String> {
        f.runner.calls.borrow().last().cloned().unwrap()
    }

    #[test]
    fn empty_pr_list_is_known_none() {
        let f = GhForge::with_runner(FakeRunner::ok("[]"));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::None);
    }

    #[test]
    fn open_pr_without_approval_is_open() {
        let f = GhForge::with_runner(FakeRunner::ok(r#"[{"state":"OPEN","reviewDecision":""}]"#));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Open);
    }

    #[test]
    fn open_pr_with_null_review_decision_is_open() {
        let f = GhForge::with_runner(FakeRunner::ok(r#"[{"state":"OPEN","reviewDecision":null}]"#));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Open);
    }

    #[test]
    fn approved_open_pr_is_approved() {
        let f =
            GhForge::with_runner(FakeRunner::ok(r#"[{"state":"OPEN","reviewDecision":"APPROVED"}]"#));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Approved);
    }

    #[test]
    fn merged_pr_is_merged() {
        let f = GhForge::with_runner(FakeRunner::ok(r#"[{"state":"MERGED","reviewDecision":"APPROVED"}]"#));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Merged);
    }

    #[test]
    fn closed_pr_is_closed() {
        let f = GhForge::with_runner(FakeRunner::ok(r#"[{"state":"CLOSED","reviewDecision":""}]"#));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Closed);
    }

    #[test]
    fn review_state_query_uses_head_and_all_states() {
        let f = GhForge::with_runner(FakeRunner::ok("[]"));
        f.review_state("impl/x").unwrap();
        let call = last_call(&f);
        assert_eq!(
            call,
            vec![
                "pr", "list", "--head", "impl/x", "--state", "all", "--json",
                "state,reviewDecision"
            ]
        );
    }

    #[test]
    fn nonzero_exit_is_command_error() {
        let f = GhForge::with_runner(FakeRunner::fail("could not authenticate"));
        let err = f.review_state("b").unwrap_err();
        assert!(matches!(err, ForgeError::Command { .. }));
        assert!(err.to_string().contains("could not authenticate"));
    }

    #[test]
    fn missing_gh_is_spawn_error() {
        let f = GhForge::with_runner(FakeRunner::missing());
        let err = f.review_state("b").unwrap_err();
        assert!(matches!(err, ForgeError::Spawn { .. }));
    }

    #[test]
    fn create_pr_builds_expected_args() {
        let f = GhForge::with_runner(FakeRunner::ok(""));
        f.create_pr("impl/x", "main", "My title", "My body").unwrap();
        assert_eq!(
            last_call(&f),
            vec![
                "pr", "create", "--head", "impl/x", "--base", "main", "--title", "My title",
                "--body", "My body"
            ]
        );
    }

    #[test]
    fn merge_builds_expected_args() {
        let f = GhForge::with_runner(FakeRunner::ok(""));
        f.merge("impl/x").unwrap();
        assert_eq!(last_call(&f), vec!["pr", "merge", "impl/x", "--merge"]);
    }

    #[test]
    fn update_from_base_builds_expected_args() {
        let f = GhForge::with_runner(FakeRunner::ok(""));
        f.update_from_base("impl/x", "main").unwrap();
        assert_eq!(last_call(&f), vec!["pr", "update-branch", "impl/x"]);
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib adapters::forge`
Expected: PASS — 12 tests.

- [ ] **Step 4: Build + clippy clean**

Run: `cargo build && cargo clippy --all-targets -- -D warnings`
Expected: no errors, no warnings.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/adapters/mod.rs src/adapters/forge.rs
git commit -m "feat: gh forge adapter — review_state + create/merge/update actions"
```

---

## Task 3: Application orchestration (`app.rs`)

**Files:**
- Create: `src/app.rs`
- Modify: `src/lib.rs` (add `pub mod app;`)

- [ ] **Step 1: Add the module to the library**

Add to `src/lib.rs` after `pub mod adapters;`:

```rust
pub mod app;
```

- [ ] **Step 2: Write the failing tests + implementation**

Create `src/app.rs`:

```rust
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
        assert_eq!(forge.calls.borrow().as_slice(), ["merge|impl/checkout-auth"]);
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
        write_checkpoint(&ws, &id, CheckpointState::SelfReview, "deadbeef".into(), None).unwrap();
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
        let err = write_checkpoint(&ws, "01J-missing", CheckpointState::Accepted, "x".into(), None)
            .unwrap_err();
        assert!(err.to_string().contains("no such session"));
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib app::`
Expected: PASS — 8 tests.

- [ ] **Step 4: Build + clippy clean**

Run: `cargo build && cargo clippy --all-targets -- -D warnings`
Expected: no errors, no warnings.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs src/lib.rs
git commit -m "feat: app orchestration for pr actions + checkpoint (fake-forge tested)"
```

---

## Task 4: CLI subcommands (`main.rs`) + e2e tests

**Files:**
- Modify: `src/main.rs`
- Create: `tests/forge_checkpoints.rs`

- [ ] **Step 1: Add the imports + clap subcommands**

Add to the imports block at the top of `src/main.rs` (after the existing `use circuit::model::store::Workspace;`):

```rust
use circuit::adapters::forge::GhForge;
```

Add two arms to the `Command` enum (after the `Dag { … }` arm):

```rust
    /// Pull-request automation via the `gh` CLI
    Pr {
        #[command(subcommand)]
        command: PrCommand,
    },
    /// Record a local review checkpoint (no-remote synthetic PR)
    Checkpoint {
        /// Session id
        session: String,
        #[arg(long, value_enum)]
        state: CheckpointStateArg,
        /// Commit sha this checkpoint snapshots
        #[arg(long)]
        commit: String,
        #[arg(long)]
        note: Option<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

Add the `PrCommand` enum and the `CheckpointStateArg` value-enum after the `DagCommand` enum:

```rust
#[derive(Subcommand)]
enum PrCommand {
    /// Open a PR for a session's branch
    Create {
        session: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Merge a session's PR
    Merge {
        session: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Update a session's PR branch from base
    UpdateFromBase {
        session: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

/// CLI spelling of the checkpoint states (kebab-case: self-review|accepted|archived).
#[derive(Clone, Copy, clap::ValueEnum)]
enum CheckpointStateArg {
    SelfReview,
    Accepted,
    Archived,
}

impl From<CheckpointStateArg> for circuit::adapters::checkpoints::CheckpointState {
    fn from(a: CheckpointStateArg) -> Self {
        use circuit::adapters::checkpoints::CheckpointState as S;
        match a {
            CheckpointStateArg::SelfReview => S::SelfReview,
            CheckpointStateArg::Accepted => S::Accepted,
            CheckpointStateArg::Archived => S::Archived,
        }
    }
}
```

- [ ] **Step 2: Dispatch the new commands**

Add two arms to the `match cli.command` block in `main()` (after `Command::Dag { command } => run_dag(command),`):

```rust
        Command::Pr { command } => run_pr(command),
        Command::Checkpoint { session, state, commit, note, path } => {
            run_checkpoint(session, state, commit, note, path)
        }
```

Add the two handler functions at the end of `src/main.rs` (before `ensure_gitignored`):

```rust
fn run_pr(command: PrCommand) -> Result<()> {
    match command {
        PrCommand::Create { session, title, body, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::pr_create(&ws, &GhForge::new(), &session, title, body)
        }
        PrCommand::Merge { session, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::pr_merge(&ws, &GhForge::new(), &session)
        }
        PrCommand::UpdateFromBase { session, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::pr_update_from_base(&ws, &GhForge::new(), &session)
        }
    }
}

fn run_checkpoint(
    session: String,
    state: CheckpointStateArg,
    commit: String,
    note: Option<String>,
    path: PathBuf,
) -> Result<()> {
    let ws = Workspace::new(&path);
    require_initialized(&ws)?;
    circuit::app::write_checkpoint(&ws, &session, state.into(), commit, note)
}
```

- [ ] **Step 3: Verify it builds**

Run: `cargo build`
Expected: success. (If clap complains about `value_enum`, confirm the `clap` derive feature is on — it is, in `Cargo.toml`.)

- [ ] **Step 4: Write the e2e tests**

Create `tests/forge_checkpoints.rs`:

```rust
//! End-to-end CLI tests for the checkpoint command and the offline failure paths
//! of `circuit pr …` (paths that never reach `gh`), plus a real-`gh` smoke test
//! that is skipped-with-log when `gh` is unavailable.

use assert_cmd::Command;
use predicates::prelude::*;

fn circuit(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("circuit").unwrap();
    cmd.current_dir(dir);
    cmd
}

/// Hand-write a minimal impl-session record so checkpoint/pr commands have a
/// session to resolve (no `session spawn` command exists in this slice).
fn write_impl_session(dir: &std::path::Path, id: &str) {
    let sessions = dir.join(".circuit/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let toml = format!(
        "schema_version = 1\nid = \"{id}\"\nkind = \"impl\"\nparent = \"checkout\"\ndag_node = \"auth-slice\"\nbranch = \"impl/checkout-auth\"\n"
    );
    std::fs::write(sessions.join(format!("{id}.toml")), toml).unwrap();
}

const SAMPLE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[test]
fn checkpoint_writes_a_record_file() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();
    write_impl_session(dir.path(), SAMPLE_ID);

    circuit(dir.path())
        .args(["checkpoint", SAMPLE_ID])
        .args(["--state", "self-review"])
        .args(["--commit", "a1b2c3d"])
        .args(["--note", "first pass"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Checkpoint recorded"));

    let path = dir.path().join(format!(".circuit/checkpoints/{SAMPLE_ID}.toml"));
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("state = \"self-review\""));
    assert!(text.contains("commit = \"a1b2c3d\""));
    assert!(text.contains("first pass"));
}

#[test]
fn checkpoint_requires_init() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path())
        .args(["checkpoint", SAMPLE_ID, "--state", "accepted", "--commit", "x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("circuit init"));
}

#[test]
fn pr_create_fails_for_missing_session() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();
    // Never reaches gh: session resolution fails first.
    circuit(dir.path())
        .args(["pr", "create", "01J-does-not-exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no such session"));
}

#[test]
fn pr_create_fails_for_branchless_session() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();
    // A spec session has no branch -> resolution fails before gh.
    let sessions = dir.path().join(".circuit/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join(format!("{SAMPLE_ID}.toml")),
        format!("schema_version = 1\nid = \"{SAMPLE_ID}\"\nkind = \"spec\"\n"),
    )
    .unwrap();

    circuit(dir.path())
        .args(["pr", "create", SAMPLE_ID])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no branch"));
}

#[test]
fn real_gh_review_state_smoke() {
    use circuit::adapters::forge::GhForge;
    use circuit::ports::ForgePort;

    // Skip-with-log when gh is unavailable — never silently passing.
    if std::process::Command::new("gh").arg("--version").output().is_err() {
        eprintln!("SKIP real_gh_review_state_smoke: `gh` not found on PATH");
        return;
    }

    // gh present: exercise the real SystemRunner shell-out + parse path. The
    // ambient git/gh context decides Ok(None)/Err; the point is no panic.
    let forge = GhForge::new();
    let result = forge.review_state("circuit-smoke-nonexistent-branch-xyz");
    // Either outcome is acceptable; assert the call completed without panicking.
    let _ = result.is_ok();
}
```

- [ ] **Step 5: Run the e2e tests**

Run: `cargo test --test forge_checkpoints`
Expected: PASS — 5 tests (the smoke test passes whether `gh` is present or skipped; when skipped it prints `SKIP …` to stderr, visible with `--nocapture`).

- [ ] **Step 6: Full suite + clippy + fmt**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all green, no warnings, formatted.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs tests/forge_checkpoints.rs
git commit -m "feat: circuit pr create|merge|update-from-base + circuit checkpoint CLI"
```

---

## Task 5: Final verification

- [ ] **Step 1: Confirm the exit-criteria walk + honesty invariants**

Run: `cargo test`
Expected: all tests pass — checkpoints (7) + forge (12) + app (8) + e2e (5) + the pre-existing foundation/M1 suite, with no regressions.

- [ ] **Step 2: Confirm `gh` skip behavior is logged**

Run: `PATH="/usr/bin:/bin" cargo test --test forge_checkpoints real_gh_review_state_smoke -- --nocapture`
Expected: prints `SKIP real_gh_review_state_smoke: gh not found on PATH` and the test passes (proving the skip is logged, not silently green). *(If `gh` lives under `/usr/bin`, point `PATH` at a directory without it.)*

- [ ] **Step 3: Confirm `forbid(unsafe_code)` is intact and no `unwrap` leaked into non-test code**

Run: `grep -rn "unsafe" src/ ; grep -rn "unwrap()" src/adapters src/app.rs`
Expected: no `unsafe`; no `unwrap()` outside `#[cfg(test)]` blocks.

---

## Self-Review (author checklist — completed)

- **Spec coverage:** forge `review_state` + 3 actions (Task 2 ✓); checkpoint store + mapping (Task 1 ✓); `app` fake-forge orchestration (Task 3 ✓); CLI `pr`/`checkpoint` (Task 4 ✓); fake-forge tests + skipped real-`gh` smoke (Tasks 3–4 ✓); determinism honesty `[]`→`None` vs `Err` (Task 2 ✓).
- **Placeholders:** none — every code step is complete.
- **Type consistency:** `CommandOutput`, `CommandRunner`, `GhForge::with_runner`/`new`, `CheckpointState::review_state`, `FsCheckpointStore::new`/`save`, `app::{pr_create,pr_merge,pr_update_from_base,write_checkpoint}`, `Config.base_branch`, `DagNode.{title,intent}`, `SessionRecord.{branch,dag_node}` — all consistent across tasks and verified against the merged foundation.
