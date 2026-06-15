# Circuit M2 Slice A — Git Adapter + Spawn + Flow Rail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `GitPort` by shelling out to the `git` CLI (branch facts + worktree ops), add a colorless per-session flow rail renderer, and wire two CLI commands — `circuit session spawn <dag-node>` (write session record + create branch + worktree) and `circuit flow [<id>]` (render the rail).

**Architecture:** Hexagonal, extending the merged M2 foundation. The git adapter (`src/adapters/git.rs`) is the only new IO boundary; it implements the foundation's `GitPort` trait (in `src/ports.rs`) exactly — signatures are fixed, do not change them. The rail renderer (`src/flow/rail.rs`) and worktree-path resolver (`src/model/local.rs`) are **pure** functions, unit-tested with zero IO. The CLI (`src/main.rs`) wires `Workspace` (record persistence) + `Git` (git ops) + the pure helpers; it is touched only additively (one `mod` line in `lib.rs`, new `clap` variants + their `run_*` functions).

**Tech Stack:** Rust 2021, `std::process::Command` for git shell-out, `thiserror` at the adapter boundary, `anyhow` in the CLI, `serde`/`toml` for `local.toml`; `#![forbid(unsafe_code)]` (already enforced crate-wide). Dev: `assert_cmd`, `predicates`, `tempfile` (all already present).

**Foundation contracts this slice consumes (do NOT modify):**
- `ports::GitPort` — `type Error`; `branch_facts(&self,&str,&str) -> Result<BranchFacts,E>`, `create_branch(&self,&str,&str) -> Result<(),E>`, `add_worktree(&self,&str,&Path) -> Result<(),E>`, `list_worktrees(&self) -> Result<Vec<Worktree>,E>`.
- `ports::Worktree { path: PathBuf, branch: Option<String> }`.
- `flow::facts::{BranchFacts { exists, commits_ahead_of_base, has_substantive_changes, merged_into_base }, ReviewState}`.
- `flow::stage::{Stage, StageView { stage, forge_certain }, derive_stage(&SessionRecord, &DeliveryFacts)}` and `flow::facts::DeliveryFacts { branch, review: Option<ReviewState> }`.
- `session::{SessionId, SessionKind, SessionRecord::impl_(id, parent, dag_node, branch)}`.
- `model::store::Workspace::{new, root, load_config, load_dag_node, save_session, load_session, list_sessions, circuit_dir}`; `model::config::Config { base_branch, .. }`; `model::node::DagNode { id, spec, branch, .. }`.
- `cockpit::health::Health { Sound, Warn, Critical, Unknown }`.

> **NOTE — intentional deviation from spec §6:** the design text lists `remove_worktree` on `GitPort`, but the *merged* trait does not declare it. Implement the merged trait exactly: four methods, no `remove_worktree`.

---

## File structure

| File | Responsibility |
|---|---|
| `src/lib.rs` | Add `pub mod adapters;` (one line, alphabetical) |
| `src/adapters/mod.rs` | **Create.** `pub mod git;` |
| `src/adapters/git.rs` | **Create.** `Git` struct + `GitError` (thiserror) + `impl GitPort`; shell-out helpers; temp-repo tests |
| `src/model/mod.rs` | Add `pub mod local;` to the submodule block |
| `src/model/local.rs` | **Create.** `LocalConfig { worktrees_dir: Option<PathBuf> }`; `resolve_worktree_dir(..)` (pure); tests |
| `src/model/store.rs` | Add `local_path()` + `load_local()` to `Workspace` |
| `src/flow/mod.rs` | Add `pub mod rail;` |
| `src/flow/rail.rs` | **Create.** `render_rail(..)` pure renderer + tests |
| `src/main.rs` | Add `Session { Spawn }` + `Flow` clap variants and `run_session_spawn` / `run_flow` |
| `tests/session_flow.rs` | **Create.** `assert_cmd` end-to-end: init → spec → dag add-node → spawn → flow |

**Parallelization:** Task 1 first (scaffolding). Then Tasks 2–4 (git adapter), Task 5 (local.rs), and Task 6 (rail.rs) are mutually independent and parallel-eligible. Tasks 7–8 (CLI) depend on 2–6 and are sequential (shared `main.rs`). Task 9 (e2e) depends on 7–8.

---

## Task 1: Module scaffolding so the crate compiles

**Files:**
- Modify: `src/lib.rs`
- Create: `src/adapters/mod.rs`, `src/adapters/git.rs`, `src/model/local.rs`
- Modify: `src/model/mod.rs`, `src/flow/mod.rs`
- Create: `src/flow/rail.rs`

- [ ] **Step 1: Declare the adapters module in the library root**

In `src/lib.rs`, add `pub mod adapters;` in alphabetical position so the block reads:

```rust
#![forbid(unsafe_code)]

pub mod adapters;
pub mod builder;
pub mod cockpit;
pub mod dag;
pub mod flow;
pub mod graph;
pub mod indicators;
pub mod lang;
pub mod layer;
pub mod model;
pub mod ports;
pub mod render;
pub mod session;
```

- [ ] **Step 2: Create the adapters module file**

Create `src/adapters/mod.rs`:

```rust
//! Outbound adapters: shell-out implementations of the port traits (§6).
pub mod git;
```

- [ ] **Step 3: Create empty stub files so module decls resolve**

Create `src/adapters/git.rs` with just a doc comment (filled in Task 2):

```rust
//! `GitPort` implemented by shelling out to the `git` CLI. Offline-capable;
//! branch facts come from `rev-list`/`merge-base`/`diff` against the shared
//! object store, worktree ops from `git worktree` (§6, §7).
```

Create `src/model/local.rs` with a doc comment (filled in Task 5):

```rust
//! Machine-local settings (`.circuit/local.toml`, gitignored). Holds the
//! worktree root override and the pure resolver for a session's worktree path (§7.2).
```

Create `src/flow/rail.rs` with a doc comment (filled in Task 6):

```rust
//! The per-session flow rail: a colorless text render of the six-stage spine
//! with the current stage marked, plus a branch-facts line (§8.1).
```

- [ ] **Step 4: Wire the new submodules**

In `src/model/mod.rs`, add `pub mod local;` to the submodule block so it reads:

```rust
pub mod config;
pub mod glossary;
pub mod local;
pub mod node;
pub mod spec;
pub mod store;
```

In `src/flow/mod.rs`, add `pub mod rail;` so it reads:

```rust
pub mod facts;
pub mod rail;
pub mod stage;
```

- [ ] **Step 5: Verify the crate still compiles**

Run: `cargo build`
Expected: builds successfully (warnings about unused modules are fine).

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/adapters src/model/mod.rs src/model/local.rs src/flow/mod.rs src/flow/rail.rs
git commit -m "chore: scaffold adapters/git, model/local, flow/rail modules"
```

---

## Task 2: `GitError` + `Git` struct + shell-out helpers

**Files:**
- Modify: `src/adapters/git.rs`

- [ ] **Step 1: Write the failing test**

Append to `src/adapters/git.rs`. This test helper (`init_repo`) is reused by Tasks 3–4, so define it now. It creates a temp git repo with one base commit on `main`.

```rust
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::flow::facts::BranchFacts;
use crate::ports::{GitPort, Worktree};

/// Errors from shelling out to `git`.
#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to run git (is it installed and on PATH?): {0}")]
    Spawn(#[source] std::io::Error),
    #[error("git {args} failed (exit {code}): {stderr}")]
    Command {
        args: String,
        code: String,
        stderr: String,
    },
    #[error("git produced non-UTF8 output: {0}")]
    Utf8(#[source] std::string::FromUtf8Error),
    #[error("could not parse git output `{output}`: {reason}")]
    Parse { output: String, reason: String },
}

/// `GitPort` over the `git` CLI, rooted at a working tree. Every command runs
/// with `-C <root>` so the adapter is independent of the process CWD.
pub struct Git {
    root: PathBuf,
}

impl Git {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Run a git subcommand that must succeed; return captured stdout (trimmed).
    fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(GitError::Spawn)?;
        if !out.status.success() {
            return Err(GitError::Command {
                args: args.join(" "),
                code: out
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        String::from_utf8(out.stdout)
            .map(|s| s.trim().to_string())
            .map_err(GitError::Utf8)
    }

    /// Run a yes/no git query. Exit 0 => true, exit 1 => false (a valid
    /// negative answer for `--is-ancestor` / `diff --quiet` / `rev-parse --verify`).
    /// Any other exit code is a real error.
    fn run_bool(&self, args: &[&str]) -> Result<bool, GitError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(GitError::Spawn)?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            other => Err(GitError::Command {
                args: args.join(" "),
                code: other
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp repo on `main` with one base commit. Returns the tempdir
    /// (keep it alive for the test) and a `Git` rooted at it.
    fn init_repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let run = |args: &[&str]| {
            let ok = Command::new("git")
                .arg("-C")
                .arg(p)
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?} failed");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@e.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(p.join("base.txt"), "base\n").unwrap();
        run(&["add", "base.txt"]);
        run(&["commit", "-qm", "base"]);
        let git = Git::new(p);
        (dir, git)
    }

    #[test]
    fn run_returns_stdout_on_success() {
        let (_d, git) = init_repo();
        let head = git.run(&["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head.len(), 40, "expected a 40-char sha, got {head:?}");
    }

    #[test]
    fn run_errors_on_nonzero_exit() {
        let (_d, git) = init_repo();
        let err = git.run(&["rev-parse", "does-not-exist"]).unwrap_err();
        assert!(matches!(err, GitError::Command { .. }));
    }

    #[test]
    fn run_bool_maps_exit_codes() {
        let (_d, git) = init_repo();
        // HEAD is an ancestor of itself => true (exit 0).
        assert!(git
            .run_bool(&["merge-base", "--is-ancestor", "HEAD", "HEAD"])
            .unwrap());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail to compile, then pass once the code above is in**

Run: `cargo test --lib adapters::git`
Expected: the module-level `use` of `GitPort`/`Worktree` is currently unused (no `impl` yet) — that is fine. The three tests should PASS (the code under test in Step 1 is the helper itself).

> If you get an "unused import" error treated as a hard failure, leave the `GitPort`/`Worktree`/`BranchFacts` imports — Tasks 3–4 use them. A warning is acceptable; do not delete them.

- [ ] **Step 3: Commit**

```bash
git add src/adapters/git.rs
git commit -m "feat(git): add Git shell-out struct, GitError, and run helpers"
```

---

## Task 3: `branch_facts` via rev-list / merge-base / diff

**Files:**
- Modify: `src/adapters/git.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests inside the existing `mod tests` block in `src/adapters/git.rs`. They build branch states with raw git and assert the derived `BranchFacts`. (Reuses `init_repo` from Task 2.)

```rust
    /// Helper: run a raw git command in the repo (test setup only).
    fn git_raw(p: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(p)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?} failed");
    }

    #[test]
    fn branch_facts_for_missing_branch_is_default() {
        let (_d, git) = init_repo();
        let f = git.branch_facts("nope", "main").unwrap();
        assert_eq!(f, BranchFacts::default());
        assert!(!f.exists);
    }

    #[test]
    fn branch_facts_for_branch_without_changes_is_project_shaped() {
        let (d, git) = init_repo();
        git_raw(d.path(), &["branch", "feat", "main"]);
        let f = git.branch_facts("feat", "main").unwrap();
        assert!(f.exists);
        assert_eq!(f.commits_ahead_of_base, 0);
        assert!(!f.has_substantive_changes);
        assert!(!f.merged_into_base);
    }

    #[test]
    fn branch_facts_for_branch_with_commits_has_changes() {
        let (d, git) = init_repo();
        let p = d.path();
        git_raw(p, &["branch", "feat", "main"]);
        git_raw(p, &["worktree", "add", "-q", "wt", "feat"]);
        std::fs::write(p.join("wt/new.txt"), "x\n").unwrap();
        git_raw(&p.join("wt"), &["add", "new.txt"]);
        git_raw(&p.join("wt"), &["commit", "-qm", "work"]);

        let f = git.branch_facts("feat", "main").unwrap();
        assert!(f.exists);
        assert_eq!(f.commits_ahead_of_base, 1);
        assert!(f.has_substantive_changes);
        assert!(!f.merged_into_base);
    }

    #[test]
    fn branch_facts_detects_merged_into_base() {
        let (d, git) = init_repo();
        let p = d.path();
        git_raw(p, &["branch", "feat", "main"]);
        git_raw(p, &["worktree", "add", "-q", "wt", "feat"]);
        std::fs::write(p.join("wt/new.txt"), "x\n").unwrap();
        git_raw(&p.join("wt"), &["add", "new.txt"]);
        git_raw(&p.join("wt"), &["commit", "-qm", "work"]);
        // Fast-forward main to feat so feat is an ancestor of main.
        git_raw(p, &["merge", "-q", "feat"]);

        let f = git.branch_facts("feat", "main").unwrap();
        assert!(f.exists);
        assert!(f.merged_into_base);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --lib adapters::git::tests::branch_facts`
Expected: FAIL to compile — `branch_facts` is not yet a method of `Git` (the trait isn't implemented).

- [ ] **Step 3: Implement `branch_facts` (partial `GitPort` impl)**

Add to `src/adapters/git.rs`, after the `impl Git` block. Start the `impl GitPort for Git` block with `branch_facts`; the worktree methods are added in Task 4 (a trait impl must be complete to compile, so include `create_branch`/`add_worktree`/`list_worktrees` as `todo!()` placeholders *only within this task's working state* — but to keep each commit green, implement all four now is preferable). **To keep the build green, implement `branch_facts` here and stub the other three with a temporary `unimplemented!()`; Task 4 replaces the stubs.**

```rust
impl GitPort for Git {
    type Error = GitError;

    fn branch_facts(&self, branch: &str, base: &str) -> Result<BranchFacts, GitError> {
        // A missing branch is Draft-shaped: report all-false defaults, not an error.
        let exists = self.run_bool(&["rev-parse", "--verify", "--quiet", &format!("{branch}^{{commit}}")])?;
        if !exists {
            return Ok(BranchFacts::default());
        }

        let ahead_raw = self.run(&["rev-list", "--count", &format!("{base}..{branch}")])?;
        let commits_ahead_of_base = ahead_raw.parse::<usize>().map_err(|e| GitError::Parse {
            output: ahead_raw.clone(),
            reason: e.to_string(),
        })?;

        // `diff --quiet base...branch` exits 1 when the merge-base..branch diff
        // is non-empty. run_bool: true => no diff, so substantive = !no_diff.
        let no_diff = self.run_bool(&["diff", "--quiet", &format!("{base}...{branch}")])?;
        let has_substantive_changes = !no_diff;

        // branch is an ancestor of base => already merged.
        let merged_into_base =
            self.run_bool(&["merge-base", "--is-ancestor", branch, base])?;

        Ok(BranchFacts {
            exists: true,
            commits_ahead_of_base,
            has_substantive_changes,
            merged_into_base,
        })
    }

    fn create_branch(&self, _branch: &str, _base: &str) -> Result<(), GitError> {
        unimplemented!("Task 4")
    }

    fn add_worktree(&self, _branch: &str, _path: &Path) -> Result<(), GitError> {
        unimplemented!("Task 4")
    }

    fn list_worktrees(&self) -> Result<Vec<Worktree>, GitError> {
        unimplemented!("Task 4")
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --lib adapters::git::tests::branch_facts`
Expected: all four `branch_facts_*` tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/adapters/git.rs
git commit -m "feat(git): derive BranchFacts from rev-list/merge-base/diff"
```

---

## Task 4: Worktree ops — `create_branch`, `add_worktree`, `list_worktrees`

**Files:**
- Modify: `src/adapters/git.rs`

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests`:

```rust
    #[test]
    fn create_branch_makes_a_new_ref() {
        let (d, git) = init_repo();
        git.create_branch("impl/x", "main").unwrap();
        // The ref now exists.
        let f = git.branch_facts("impl/x", "main").unwrap();
        assert!(f.exists);
    }

    #[test]
    fn add_worktree_checks_out_the_branch_to_a_path() {
        let (d, git) = init_repo();
        git.create_branch("impl/x", "main").unwrap();
        let wt = d.path().join("wt-x");
        git.add_worktree("impl/x", &wt).unwrap();
        assert!(wt.join("base.txt").exists(), "worktree should contain base commit");
    }

    #[test]
    fn list_worktrees_includes_added_worktree_with_branch() {
        let (d, git) = init_repo();
        git.create_branch("impl/x", "main").unwrap();
        let wt = d.path().join("wt-x");
        git.add_worktree("impl/x", &wt).unwrap();

        let list = git.list_worktrees().unwrap();
        // The main worktree plus the one we added.
        assert!(list.iter().any(|w| w.branch.as_deref() == Some("main")));
        let added = list
            .iter()
            .find(|w| w.branch.as_deref() == Some("impl/x"))
            .expect("added worktree should be listed");
        // git may canonicalize the path (e.g. /var -> /private/var on macOS);
        // compare on the final path component to stay portable.
        assert_eq!(added.path.file_name().unwrap(), wt.file_name().unwrap());
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --lib adapters::git::tests`
Expected: the three new tests FAIL (the methods are `unimplemented!()` and panic).

- [ ] **Step 3: Replace the stubs with real implementations**

In `src/adapters/git.rs`, replace the three `unimplemented!("Task 4")` method bodies with:

```rust
    fn create_branch(&self, branch: &str, base: &str) -> Result<(), GitError> {
        // Create the ref without checking it out; add_worktree checks it out.
        self.run(&["branch", branch, base]).map(|_| ())
    }

    fn add_worktree(&self, branch: &str, path: &Path) -> Result<(), GitError> {
        let path_str = path.to_str().ok_or_else(|| GitError::Parse {
            output: path.display().to_string(),
            reason: "worktree path is not valid UTF-8".to_string(),
        })?;
        self.run(&["worktree", "add", path_str, branch]).map(|_| ())
    }

    fn list_worktrees(&self) -> Result<Vec<Worktree>, GitError> {
        let out = self.run(&["worktree", "list", "--porcelain"])?;
        Ok(parse_worktree_porcelain(&out))
    }
```

Add this free function below the `impl GitPort` block (pure parser, separately testable):

```rust
/// Parse `git worktree list --porcelain` into `Worktree` entries. Blocks are
/// separated by blank lines; each has a `worktree <path>` line and either a
/// `branch refs/heads/<name>` line or a `detached` line.
fn parse_worktree_porcelain(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;

    let mut flush = |path: &mut Option<PathBuf>, branch: &mut Option<String>| {
        if let Some(p) = path.take() {
            out.push(Worktree {
                path: p,
                branch: branch.take(),
            });
        } else {
            *branch = None;
        }
    };

    for line in text.lines() {
        if line.is_empty() {
            flush(&mut path, &mut branch);
        } else if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        }
        // `HEAD <sha>` and `detached` lines need no handling (branch stays None).
    }
    flush(&mut path, &mut branch);
    out
}
```

- [ ] **Step 4: Add a pure parser unit test**

Add inside `mod tests`:

```rust
    #[test]
    fn parse_worktree_porcelain_handles_branch_and_detached() {
        let text = "\
worktree /repo
HEAD aaaa
branch refs/heads/main

worktree /repo/wt
HEAD bbbb
branch refs/heads/impl/x

worktree /repo/detached
HEAD cccc
detached
";
        let ws = parse_worktree_porcelain(text);
        assert_eq!(ws.len(), 3);
        assert_eq!(ws[0].branch.as_deref(), Some("main"));
        assert_eq!(ws[1].branch.as_deref(), Some("impl/x"));
        assert_eq!(ws[1].path, PathBuf::from("/repo/wt"));
        assert_eq!(ws[2].branch, None);
    }
```

- [ ] **Step 5: Run all git-adapter tests**

Run: `cargo test --lib adapters::git`
Expected: all tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/adapters/git.rs
git commit -m "feat(git): implement create_branch, add_worktree, list_worktrees"
```

---

## Task 5: `LocalConfig` + worktree-path resolver

**Files:**
- Modify: `src/model/local.rs`, `src/model/store.rs`

- [ ] **Step 1: Write the failing tests for the pure resolver**

Replace the contents of `src/model/local.rs` (keep the doc comment at top) with the type, resolver, and tests. Implementation comes in Step 3; write the tests first by including the full file but expect compile/test failure until Step 3 — to follow TDD strictly, first add only the `tests` module and a minimal type, run, watch it fail, then flesh out. For a single commit, add the complete file below and verify tests pass.

```rust
//! Machine-local settings (`.circuit/local.toml`, gitignored). Holds the
//! worktree root override and the pure resolver for a session's worktree path (§7.2).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `.circuit/local.toml` — never committed (machine-specific paths). Absent file
/// deserializes to the all-`None` default via `Workspace::load_local`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalConfig {
    /// Root directory under which per-session worktrees are created. When unset,
    /// the default sibling `../<repo>-worktrees/` is used.
    #[serde(default)]
    pub worktrees_dir: Option<PathBuf>,
}

/// Resolve the worktree directory for a session. Precedence (§7.2):
/// 1. `env` (the `CIRCUIT_WORKTREES_DIR` value), 2. `local.worktrees_dir`,
/// 3. default sibling `<repo_root>/../<repo_name>-worktrees`.
/// In all cases the session id is appended as the final path component, so the
/// returned path is `<base>/<session_id>`.
pub fn resolve_worktree_dir(
    env: Option<&str>,
    local: &LocalConfig,
    repo_root: &Path,
    session_id: &str,
) -> PathBuf {
    let base: PathBuf = if let Some(e) = env.filter(|e| !e.is_empty()) {
        PathBuf::from(e)
    } else if let Some(d) = &local.worktrees_dir {
        d.clone()
    } else {
        let name = repo_root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".to_string());
        let parent = repo_root
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| repo_root.to_path_buf());
        parent.join(format!("{name}-worktrees"))
    };
    base.join(session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_takes_precedence_over_everything() {
        let local = LocalConfig {
            worktrees_dir: Some(PathBuf::from("/from/local")),
        };
        let got = resolve_worktree_dir(Some("/from/env"), &local, Path::new("/repos/circuit"), "SID");
        assert_eq!(got, PathBuf::from("/from/env/SID"));
    }

    #[test]
    fn empty_env_is_ignored() {
        let local = LocalConfig {
            worktrees_dir: Some(PathBuf::from("/from/local")),
        };
        let got = resolve_worktree_dir(Some(""), &local, Path::new("/repos/circuit"), "SID");
        assert_eq!(got, PathBuf::from("/from/local/SID"));
    }

    #[test]
    fn local_config_used_when_no_env() {
        let local = LocalConfig {
            worktrees_dir: Some(PathBuf::from("/from/local")),
        };
        let got = resolve_worktree_dir(None, &local, Path::new("/repos/circuit"), "SID");
        assert_eq!(got, PathBuf::from("/from/local/SID"));
    }

    #[test]
    fn default_is_sibling_worktrees_dir() {
        let got = resolve_worktree_dir(None, &LocalConfig::default(), Path::new("/repos/circuit"), "SID");
        assert_eq!(got, PathBuf::from("/repos/circuit-worktrees/SID"));
    }

    #[test]
    fn local_config_round_trips_through_toml() {
        let c = LocalConfig {
            worktrees_dir: Some(PathBuf::from("/tmp/wt")),
        };
        let parsed: LocalConfig = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn empty_toml_is_default() {
        let c: LocalConfig = toml::from_str("").unwrap();
        assert_eq!(c, LocalConfig::default());
    }
}
```

- [ ] **Step 2: Run to verify the tests fail, then pass**

Run: `cargo test --lib model::local`
Expected: with the full file from Step 1 in place, all six tests PASS. (If you staged the type stub first, you'd see FAIL → implement → PASS.)

- [ ] **Step 3: Add `load_local` to `Workspace`**

In `src/model/store.rs`, add `local::LocalConfig` to the `use super::{...}` import list and add these methods inside `impl Workspace` (place them near `config_path`/`load_config`):

```rust
    pub fn local_path(&self) -> PathBuf {
        self.circuit_dir().join("local.toml")
    }

    /// Load `.circuit/local.toml`, or the all-`None` default when it is absent
    /// (the file is gitignored and may simply not exist on this machine).
    pub fn load_local(&self) -> Result<LocalConfig, ModelError> {
        if self.local_path().exists() {
            load_toml(&self.local_path())
        } else {
            Ok(LocalConfig::default())
        }
    }
```

Update the import block at the top of `src/model/store.rs` so it reads:

```rust
use super::{
    config::Config, glossary::Glossary, load_toml, local::LocalConfig, node::DagNode, save_toml,
    spec::SpecRecord, ModelError,
};
```

- [ ] **Step 4: Add a `load_local` round-trip test**

Add inside the `mod tests` block in `src/model/store.rs`:

```rust
    #[test]
    fn load_local_defaults_when_absent_and_round_trips_when_present() {
        use crate::model::local::LocalConfig;
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        // Absent => default.
        assert_eq!(ws.load_local().unwrap(), LocalConfig::default());
        // Present => round-trips.
        let c = LocalConfig {
            worktrees_dir: Some(std::path::PathBuf::from("/tmp/wt")),
        };
        save_toml(&ws.local_path(), &c).unwrap();
        assert_eq!(ws.load_local().unwrap(), c);
    }
```

- [ ] **Step 5: Run the model tests**

Run: `cargo test --lib model::local model::store`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/model/local.rs src/model/store.rs
git commit -m "feat(model): add LocalConfig + worktree-dir resolver and Workspace::load_local"
```

---

## Task 6: `render_rail` — colorless per-session text rail

**Files:**
- Modify: `src/flow/rail.rs`

- [ ] **Step 1: Write the failing tests**

Replace the contents of `src/flow/rail.rs` (keep the doc comment) with the renderer + tests below.

```rust
//! The per-session flow rail: a colorless text render of the six-stage spine
//! with the current stage marked, plus a branch-facts line (§8.1).

use crate::cockpit::health::Health;
use crate::flow::facts::{BranchFacts, ReviewState};
use crate::flow::stage::{Stage, StageView};
use crate::session::SessionKind;

/// The six-stage spine, in order.
const SPINE: [Stage; 6] = [
    Stage::Draft,
    Stage::Project,
    Stage::Implement,
    Stage::Review,
    Stage::Merge,
    Stage::Done,
];

fn stage_label(s: Stage) -> &'static str {
    match s {
        Stage::Draft => "Draft",
        Stage::Project => "Project",
        Stage::Implement => "Implement",
        Stage::Review => "Review",
        Stage::Merge => "Merge",
        Stage::Done => "Done",
    }
}

fn kind_label(k: SessionKind) -> &'static str {
    match k {
        SessionKind::Spec => "spec",
        SessionKind::Impl => "impl",
        SessionKind::Fix => "fix",
    }
}

fn health_glyph(h: Health) -> &'static str {
    match h {
        Health::Sound => "●",
        Health::Warn => "◐",
        Health::Critical => "◍",
        Health::Unknown => "?",
    }
}

fn review_label(r: Option<ReviewState>) -> &'static str {
    match r {
        None => "PR ?",
        Some(ReviewState::None) => "no PR",
        Some(ReviewState::Open) => "PR open",
        Some(ReviewState::Approved) => "PR approved",
        Some(ReviewState::Merged) => "PR merged",
        Some(ReviewState::Closed) => "PR closed",
    }
}

/// Render one session's rail. Pure; colorless (§8). `review = None` means the
/// forge state is undeterminable (printed `PR ?`), distinct from a known `no PR`.
/// `health` is always rendered from its glyph; this slice passes `Unknown`.
pub fn render_rail(
    node_id: &str,
    kind: SessionKind,
    view: StageView,
    branch: Option<&str>,
    facts: &BranchFacts,
    review: Option<ReviewState>,
    health: Health,
) -> String {
    // Spine: current stage wrapped in guillemets, joined by " › ".
    let spine = SPINE
        .iter()
        .map(|&s| {
            if s == view.stage {
                format!("‹{}›", stage_label(s))
            } else {
                stage_label(s).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" › ");

    let uncertain = if view.forge_certain {
        ""
    } else {
        "  (forge state unknown)"
    };

    let line1 = format!("{node_id}  [{}]  {spine}{uncertain}", kind_label(kind));

    let line2 = match branch {
        Some(name) => format!(
            "            branch {name} · {} commits · {} · health {}",
            facts.commits_ahead_of_base,
            review_label(review),
            health_glyph(health),
        ),
        None => format!("            no branch · health {}", health_glyph(health)),
    };

    format!("{line1}\n{line2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(commits: usize) -> BranchFacts {
        BranchFacts {
            exists: true,
            commits_ahead_of_base: commits,
            has_substantive_changes: commits > 0,
            merged_into_base: false,
        }
    }

    #[test]
    fn marks_the_current_stage_with_guillemets() {
        let view = StageView { stage: Stage::Project, forge_certain: true };
        let out = render_rail("auth-slice", SessionKind::Impl, view, Some("impl/x"), &facts(0), Some(ReviewState::None), Health::Sound);
        assert!(out.contains("‹Project›"), "got: {out}");
        // Other stages are unmarked.
        assert!(out.contains(" Draft "));
        assert!(out.contains("Done"));
        assert!(!out.contains("‹Draft›"));
    }

    #[test]
    fn line_one_has_node_id_and_kind() {
        let view = StageView { stage: Stage::Implement, forge_certain: true };
        let out = render_rail("auth-slice", SessionKind::Impl, view, Some("impl/x"), &facts(3), Some(ReviewState::None), Health::Critical);
        let line1 = out.lines().next().unwrap();
        assert!(line1.starts_with("auth-slice  [impl]"));
    }

    #[test]
    fn undeterminable_review_prints_pr_question_mark() {
        let view = StageView { stage: Stage::Implement, forge_certain: false };
        let out = render_rail("auth-slice", SessionKind::Impl, view, Some("impl/x"), &facts(3), None, Health::Unknown);
        assert!(out.contains("PR ?"), "got: {out}");
        assert!(out.contains("(forge state unknown)"), "got: {out}");
        assert!(out.contains("health ?"), "got: {out}");
    }

    #[test]
    fn known_no_pr_differs_from_undeterminable() {
        let view = StageView { stage: Stage::Implement, forge_certain: true };
        let out = render_rail("a", SessionKind::Impl, view, Some("impl/x"), &facts(1), Some(ReviewState::None), Health::Unknown);
        assert!(out.contains("no PR"));
        assert!(!out.contains("PR ?"));
    }

    #[test]
    fn commit_count_and_branch_name_appear() {
        let view = StageView { stage: Stage::Implement, forge_certain: true };
        let out = render_rail("a", SessionKind::Impl, view, Some("impl/checkout-auth"), &facts(3), Some(ReviewState::None), Health::Sound);
        assert!(out.contains("branch impl/checkout-auth"));
        assert!(out.contains("3 commits"));
        assert!(out.contains("health ●"));
    }

    #[test]
    fn spec_session_without_branch_renders_no_branch() {
        let view = StageView { stage: Stage::Draft, forge_certain: true };
        let out = render_rail("checkout", SessionKind::Spec, view, None, &BranchFacts::default(), None, Health::Unknown);
        assert!(out.contains("no branch"));
        assert!(!out.contains("commits"));
    }

    #[test]
    fn rail_contains_no_ansi_color_codes() {
        let view = StageView { stage: Stage::Review, forge_certain: true };
        let out = render_rail("a", SessionKind::Impl, view, Some("impl/x"), &facts(2), Some(ReviewState::Open), Health::Sound);
        // The colorless invariant (§8): no ESC byte anywhere.
        assert!(!out.contains('\u{1b}'), "rail must be colorless");
    }
}
```

- [ ] **Step 2: Run to verify they fail, then pass**

Run: `cargo test --lib flow::rail`
Expected: with the file from Step 1, all seven tests PASS.

- [ ] **Step 3: Commit**

```bash
git add src/flow/rail.rs
git commit -m "feat(flow): add colorless per-session rail renderer"
```

---

## Task 7: CLI — `circuit session spawn <dag-node>`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add the `Session` subcommand to clap**

In `src/main.rs`, add a new variant to `enum Command` (after `Dag`):

```rust
    /// Session lifecycle commands
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Render the per-session flow rail
    Flow {
        /// Session id (ULID) or unique DAG-node name; omit to show all sessions
        session: Option<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

Add the subcommand enum after `enum DagCommand { .. }`:

```rust
#[derive(Subcommand)]
enum SessionCommand {
    /// Spawn an impl session for a DAG node: write the record, create the
    /// branch, and add a worktree (the session derives to Project).
    Spawn {
        /// The DAG node id to execute
        dag_node: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}
```

Add the match arms in `fn main`:

```rust
        Command::Session { command } => run_session(command),
        Command::Flow { session, path } => run_flow(session.as_deref(), &path),
```

- [ ] **Step 2: Add imports**

At the top of `src/main.rs`, add to the `use circuit::...` block:

```rust
use circuit::adapters::git::Git;
use circuit::cockpit::health::Health;
use circuit::flow::facts::DeliveryFacts;
use circuit::flow::rail::render_rail;
use circuit::flow::stage::derive_stage;
use circuit::model::local::resolve_worktree_dir;
use circuit::ports::GitPort;
use circuit::session::{SessionId, SessionRecord};
```

- [ ] **Step 3: Write `run_session` / `run_session_spawn`**

Add these functions to `src/main.rs`:

```rust
fn run_session(command: SessionCommand) -> Result<()> {
    match command {
        SessionCommand::Spawn { dag_node, path } => run_session_spawn(&dag_node, &path),
    }
}

fn run_session_spawn(dag_node: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;

    let node = ws
        .load_dag_node(dag_node)
        .with_context(|| format!("loading dag node {dag_node}"))?;
    let config = ws.load_config().context("loading config.toml")?;
    let base = &config.base_branch;

    let git = Git::new(ws.root());

    // Refuse to clobber an existing branch (a session may already own it).
    if git
        .branch_facts(&node.branch, base)
        .with_context(|| format!("checking branch {}", node.branch))?
        .exists
    {
        anyhow::bail!(
            "branch {} already exists — refusing to spawn over it",
            node.branch
        );
    }

    // 1. Allocate identity and write the authored record (parent = node.spec).
    let id = SessionId::generate();
    let record = SessionRecord::impl_(id, node.spec.clone(), node.id.clone(), node.branch.clone());
    ws.save_session(&record)
        .with_context(|| format!("writing session {id}"))?;

    // 2. Resolve the (machine-local, never-stored) worktree path.
    let local = ws.load_local().context("loading local.toml")?;
    let env = std::env::var("CIRCUIT_WORKTREES_DIR").ok();
    let worktree = resolve_worktree_dir(env.as_deref(), &local, ws.root(), &id.to_string());

    // 3. Create the branch + worktree.
    git.create_branch(&node.branch, base)
        .with_context(|| format!("creating branch {}", node.branch))?;
    git.add_worktree(&node.branch, &worktree)
        .with_context(|| format!("adding worktree at {}", worktree.display()))?;

    println!("Spawned session {id} for node {} (stage: Project)", node.id);
    println!("  branch:   {}", node.branch);
    println!("  worktree: {}", worktree.display());
    Ok(())
}
```

- [ ] **Step 4: Build (compile check before the e2e test in Task 9)**

Run: `cargo build`
Expected: builds. `run_flow` is referenced but not yet defined — **add `run_flow` in Task 8 before building**; to keep this task self-contained, temporarily stub it:

```rust
fn run_flow(_session: Option<&str>, _path: &Path) -> Result<()> {
    unimplemented!("Task 8")
}
```

Then `cargo build` should succeed.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): add `session spawn` — write record, create branch + worktree"
```

---

## Task 8: CLI — `circuit flow [<id>]`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace the `run_flow` stub with the real implementation**

In `src/main.rs`, replace the `run_flow` stub with:

```rust
/// Render the rail for one session (by ULID, else by unique DAG-node name) or,
/// when `selector` is `None`, every session in the workspace.
fn run_flow(selector: Option<&str>, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;

    let sessions = match selector {
        Some(sel) => vec![resolve_session(&ws, sel)?],
        None => ws.list_sessions().context("listing sessions")?,
    };

    if sessions.is_empty() {
        println!("No sessions yet.");
        return Ok(());
    }

    let config = ws.load_config().context("loading config.toml")?;
    let git = Git::new(ws.root());

    let mut blocks = Vec::new();
    for s in &sessions {
        // git-only facts; forge/health are out of this slice (review = None,
        // health = Unknown) and render honestly as `PR ?` / `health ?`.
        let branch_facts = match &s.branch {
            Some(b) => git
                .branch_facts(b, &config.base_branch)
                .with_context(|| format!("deriving facts for {b}"))?,
            None => Default::default(),
        };
        let view = derive_stage(
            s,
            &DeliveryFacts {
                branch: branch_facts.clone(),
                review: None,
            },
        );
        // Label by DAG node when present (impl/fix), else by session id (spec).
        let label = s.dag_node.clone().unwrap_or_else(|| s.id.to_string());
        blocks.push(render_rail(
            &label,
            s.kind,
            view,
            s.branch.as_deref(),
            &branch_facts,
            None,
            Health::Unknown,
        ));
    }
    println!("{}", blocks.join("\n\n"));
    Ok(())
}

/// Resolve a session selector: first as a ULID, then as a unique DAG-node name.
fn resolve_session(
    ws: &Workspace,
    selector: &str,
) -> Result<SessionRecord> {
    // Exact ULID match.
    if selector.parse::<SessionId>().is_ok() {
        if let Ok(s) = ws.load_session(selector) {
            return Ok(s);
        }
    }
    // Fall back to a unique DAG-node-name match.
    let all = ws.list_sessions().context("listing sessions")?;
    let mut matches: Vec<SessionRecord> = all
        .into_iter()
        .filter(|s| s.dag_node.as_deref() == Some(selector))
        .collect();
    match matches.len() {
        1 => Ok(matches.pop().unwrap()),
        0 => anyhow::bail!("no session matches `{selector}` (not a known session id or DAG-node name)"),
        n => anyhow::bail!(
            "`{selector}` matches {n} sessions — pass the session id (ULID) to disambiguate"
        ),
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: builds with no errors.

- [ ] **Step 3: Run the full library + existing CLI suite**

Run: `cargo test`
Expected: all existing tests + the new unit tests PASS.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): add `flow` rail command with id/dag-node resolution"
```

---

## Task 9: End-to-end CLI test

**Files:**
- Create: `tests/session_flow.rs`

- [ ] **Step 1: Write the e2e test**

Create `tests/session_flow.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use std::process::Command as Stdcmd;

/// Run `circuit` with args in a given working directory.
fn circuit(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("circuit").unwrap();
    cmd.current_dir(dir);
    cmd
}

/// Initialize a git repo with one base commit on `main`.
fn init_git_repo(dir: &Path) {
    let run = |args: &[&str]| {
        assert!(
            Stdcmd::new("git").arg("-C").arg(dir).args(args).output().unwrap().status.success(),
            "git {args:?} failed"
        );
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["config", "user.email", "t@e.com"]);
    run(&["config", "user.name", "t"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    run(&["add", "base.txt"]);
    run(&["commit", "-qm", "base"]);
}

#[test]
fn spawn_creates_worktree_and_flow_shows_project() {
    let dir = tempfile::tempdir().unwrap();
    let wt_root = tempfile::tempdir().unwrap(); // controlled worktree location
    init_git_repo(dir.path());

    // init + author a spec and a DAG node.
    circuit(dir.path()).arg("init").assert().success();
    circuit(dir.path())
        .args(["spec", "new", "checkout", "--title", "Checkout", "--intent", "Pay."])
        .assert()
        .success();
    circuit(dir.path())
        .args([
            "dag", "add-node", "auth-slice", "--spec", "checkout", "--title", "Auth",
            "--branch", "impl/checkout-auth",
        ])
        .assert()
        .success();

    // spawn — pin the worktree location via the env override.
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stage: Project"))
        .stdout(predicate::str::contains("impl/checkout-auth"));

    // The branch and a worktree now exist.
    let branch_listed = Stdcmd::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&branch_listed.stdout);
    assert!(listing.contains("refs/heads/impl/checkout-auth"), "got: {listing}");

    // flow by DAG-node name shows the Project stage for a fresh branch.
    circuit(dir.path())
        .args(["flow", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auth-slice  [impl]"))
        .stdout(predicate::str::contains("‹Project›"))
        .stdout(predicate::str::contains("PR ?"));

    // flow with no arg lists all sessions (the spec + the impl session).
    circuit(dir.path())
        .arg("flow")
        .assert()
        .success()
        .stdout(predicate::str::contains("auth-slice"));
}

#[test]
fn spawn_refuses_to_clobber_an_existing_branch() {
    let dir = tempfile::tempdir().unwrap();
    let wt_root = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    circuit(dir.path()).arg("init").assert().success();
    circuit(dir.path())
        .args(["spec", "new", "checkout", "--title", "C", "--intent", "x"])
        .assert()
        .success();
    circuit(dir.path())
        .args(["dag", "add-node", "auth-slice", "--spec", "checkout", "--title", "A", "--branch", "impl/x"])
        .assert()
        .success();

    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .success();
    // Second spawn hits the existing branch and fails clearly.
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}
```

- [ ] **Step 2: Run the e2e test**

Run: `cargo test --test session_flow`
Expected: both tests PASS.

- [ ] **Step 3: Run the entire suite + lints**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all green. Fix any clippy/fmt issues (e.g. an unused import left from a stub) and re-run.

- [ ] **Step 4: Commit**

```bash
git add tests/session_flow.rs
git commit -m "test(e2e): spawn creates worktree and flow renders the rail"
```

---

## Self-review notes (verified against the M2 spec + foundation contracts)

- **Spec coverage:** §5 stage derivation reused via `derive_stage` (not reimplemented); §6 `GitPort` shell-out (branch_facts + 3 worktree ops) = Tasks 2–4; §7.1 spawn sequence (record → branch → worktree, derives to Project) = Task 7; §7.2 worktree location precedence (env > local.toml > sibling), paths never stored (discovered/derived, only the session record is written) = Task 5 + Task 7; §8 colorless rail with current stage + branch facts = Task 6 + Task 8. Out-of-scope (forge, checkpoints, health computation, DAG board, PR commands) is **not** implemented; the rail accepts `review`/`health` inputs so those slices wire in later without reopening `rail.rs`.
- **Determinism honesty:** `flow` passes `review = None` → `derive_stage` returns `forge_certain = false` for substantive branches, rendered as `(forge state unknown)` + `PR ?`; known `no PR` (a checkpoint slice concern) stays distinct. `merged_into_base` is git-only, so `Done` is offline-confident.
- **Contracts unchanged:** `GitPort` implemented exactly as declared (4 methods, associated `Error = GitError`); `remove_worktree` from spec §6 is intentionally omitted (not in the merged trait).
- **Type consistency:** `resolve_worktree_dir(env: Option<&str>, &LocalConfig, repo_root, session_id)` is called identically in Task 7; `render_rail(node_id, kind, view, branch, facts, review, health)` signature matches between Task 6 and Task 8; `Git::new(ws.root())` (`&Path` via `impl Into<PathBuf>`) used consistently.
- **Boundaries:** `thiserror` (`GitError`) at the adapter boundary; `anyhow` in `main.rs`; pure logic (`resolve_worktree_dir`, `render_rail`, `parse_worktree_porcelain`) has zero IO and is unit-tested; `#![forbid(unsafe_code)]` unaffected.
```
