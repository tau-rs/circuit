# Circuit M2 Slice B — Forge + Checkpoint Adapters + Flow Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `circuit flow` show real review state — GitHub PR state via `gh`, or local `.circuit/checkpoints/` state when there is no remote — instead of the hardcoded `PR ?` slice A left behind.

**Architecture:** Hexagonal, extending slice A. Two new outbound adapters (`adapters/forge.rs` shelling out to `gh`, `adapters/checkpoints.rs` reading TOML) implement the existing `ForgePort` / `CheckpointStore` traits. A pure `adapters/delivery.rs` resolver picks the source. An additive `ReviewState::ChangesRequested` variant is the only foundation change. `run_flow` (CLI) is wired to feed the resolved `ReviewState` into the existing pure `derive_stage`/`render_rail`.

**Tech Stack:** Rust, `thiserror` (adapter errors), `toml` (checkpoint parse), `gh` CLI (`--jq` for a plain `STATE|DECISION` line — no `serde_json` needed), `tempfile` (tests). `#![forbid(unsafe_code)]`.

**Spec:** `docs/superpowers/specs/2026-06-16-circuit-m2-slice-b-forge-checkpoint-design.md`

---

## File structure

| File | Responsibility |
|---|---|
| `src/flow/facts.rs` | **Modify** — add `ReviewState::ChangesRequested` |
| `src/flow/stage.rs` | **Modify** — map `ChangesRequested → Review` |
| `src/flow/rail.rs` | **Modify** — label `"PR changes requested"` |
| `src/adapters/forge.rs` | **Create** — `impl ForgePort` for `gh`; pure `parse_review_state` + argv-builders |
| `src/adapters/checkpoints.rs` | **Create** — `impl CheckpointStore`; pure `parse_checkpoint_state` |
| `src/adapters/delivery.rs` | **Create** — `DeliveryMode` + pure `resolve` |
| `src/adapters/mod.rs` | **Modify** — declare the three new submodules |
| `src/main.rs` | **Modify** — detect mode + wire real review state into `run_flow` |

**Foundation contracts consumed unchanged:** `ForgePort`, `CheckpointStore`, `Worktree` (`src/ports.rs`); `DeliveryFacts`, `BranchFacts` (`src/flow/facts.rs`); `derive_stage`, `StageView`, `Stage` (`src/flow/stage.rs`); `render_rail` (`src/flow/rail.rs`). The only foundation *modification* is the additive `ReviewState` variant (Task 2).

---

## Task 1: Declare the new adapter submodules so the crate compiles

**Files:**
- Modify: `src/adapters/mod.rs`
- Create: `src/adapters/forge.rs`, `src/adapters/checkpoints.rs`, `src/adapters/delivery.rs`

- [ ] **Step 1: Add the module declarations**

Replace the contents of `src/adapters/mod.rs` with:

```rust
//! Outbound adapters: shell-out implementations of the port traits (§6).
pub mod checkpoints;
pub mod delivery;
pub mod forge;
pub mod git;
```

- [ ] **Step 2: Create empty stub files so the module decls resolve**

Create `src/adapters/forge.rs`:

```rust
//! `ForgePort` over the `gh` CLI (GitHub). Implemented in Task 3/4.
```

Create `src/adapters/checkpoints.rs`:

```rust
//! `CheckpointStore` over `.circuit/checkpoints/`. Implemented in Task 5.
```

Create `src/adapters/delivery.rs`:

```rust
//! Delivery-mode selection (forge vs local checkpoint). Implemented in Task 6.
```

- [ ] **Step 3: Verify the crate still compiles**

Run: `cargo build`
Expected: builds clean (empty modules are valid).

- [ ] **Step 4: Commit**

```bash
git add src/adapters/mod.rs src/adapters/forge.rs src/adapters/checkpoints.rs src/adapters/delivery.rs
git commit -m "chore(m2b): scaffold forge, checkpoint & delivery adapter modules"
```

---

## Task 2: Foundation extension — `ReviewState::ChangesRequested`

**Files:**
- Modify: `src/flow/facts.rs` (the `ReviewState` enum, ~line 18)
- Modify: `src/flow/stage.rs` (the `match facts.review` block, ~line 59; tests)
- Modify: `src/flow/rail.rs` (`review_label`, ~line 50; tests)

- [ ] **Step 1: Add the variant (breaks the exhaustive matches — the failing signal)**

In `src/flow/facts.rs`, add `ChangesRequested` to `ReviewState`, between `Open` and `Approved`:

```rust
pub enum ReviewState {
    /// No PR / checkpoint exists — a *known* fact (distinct from undeterminable).
    None,
    /// A PR (or `self-review` checkpoint) is open.
    Open,
    /// PR open, reviewer requested changes — ball is back with the developer.
    ChangesRequested,
    /// Approved / mergeable, not yet landed.
    Approved,
    /// Merged via the forge.
    Merged,
    /// Closed without merging.
    Closed,
}
```

- [ ] **Step 2: Verify it fails to compile**

Run: `cargo build`
Expected: FAIL — non-exhaustive `match` errors in `flow/stage.rs` and `flow/rail.rs` (the desired forcing function).

- [ ] **Step 3: Add the stage truth-table test**

In `src/flow/stage.rs`, inside `mod tests`, add after `open_pr_is_review` (~line 193):

```rust
    // ChangesRequested stays at Review — the stage marker does not jump back.
    #[test]
    fn changes_requested_is_review() {
        let v = derive_stage(
            &session(),
            &facts(true, true, false, Some(ReviewState::ChangesRequested)),
        );
        assert_eq!(
            v,
            StageView {
                stage: Stage::Review,
                forge_certain: true
            }
        );
    }
```

- [ ] **Step 4: Add the stage match arm**

In `src/flow/stage.rs`, in the `match facts.review` block, add after the `Some(ReviewState::Open)` arm (~line 66):

```rust
        Some(ReviewState::ChangesRequested) => StageView::certain(Stage::Review),
```

- [ ] **Step 5: Add the rail label test**

In `src/flow/rail.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn changes_requested_renders_its_own_label() {
        let view = StageView {
            stage: Stage::Review,
            forge_certain: true,
        };
        let out = render_rail(
            "a",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(2),
            Some(ReviewState::ChangesRequested),
            Health::Sound,
        );
        assert!(out.contains("PR changes requested"), "got: {out}");
    }
```

- [ ] **Step 6: Add the rail label arm**

In `src/flow/rail.rs`, in `review_label`, add after the `Some(ReviewState::Open)` arm (~line 54):

```rust
        Some(ReviewState::ChangesRequested) => "PR changes requested",
```

- [ ] **Step 7: Run the flow tests**

Run: `cargo test --lib flow::`
Expected: PASS, including the two new tests.

- [ ] **Step 8: Commit**

```bash
git add src/flow/facts.rs src/flow/stage.rs src/flow/rail.rs
git commit -m "feat(m2b): add ReviewState::ChangesRequested (Review stage, own label)"
```

---

## Task 3: Forge adapter — `review_state`

**Files:**
- Modify: `src/adapters/forge.rs`

- [ ] **Step 1: Write the error type, struct, run helper, and pure parser**

Replace the contents of `src/adapters/forge.rs` with:

```rust
//! `ForgePort` implemented by shelling out to the `gh` CLI (GitHub). Review
//! state comes from `gh pr view`; write actions wrap `gh pr create/merge/
//! update-branch` (Task 4). Forge-unreachable maps to the caller's `None`
//! (undeterminable) — never a fake verdict (§5).

use std::path::PathBuf;
use std::process::{Command, Output};

use thiserror::Error;

use crate::flow::facts::ReviewState;
use crate::ports::ForgePort;

/// Errors from shelling out to `gh`.
#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("failed to run gh (is it installed and on PATH?): {0}")]
    Spawn(#[source] std::io::Error),
    #[error("gh produced non-UTF8 output: {0}")]
    Utf8(#[source] std::string::FromUtf8Error),
    #[error("gh failed (exit {code}): {stderr}")]
    Command { code: String, stderr: String },
    #[error("could not parse gh output `{output}`: {reason}")]
    Parse { output: String, reason: String },
}

/// `ForgePort` over the `gh` CLI, rooted at a working tree. Commands run with
/// `current_dir(root)` so the adapter is independent of the process CWD.
pub struct Forge {
    root: PathBuf,
}

impl Forge {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Run a gh subcommand, capturing the full output for the caller to inspect.
    fn run(&self, args: &[&str]) -> Result<Output, ForgeError> {
        Command::new("gh")
            .current_dir(&self.root)
            .args(args)
            .output()
            .map_err(ForgeError::Spawn)
    }

    /// Run a gh subcommand that must succeed; discard stdout.
    fn run_checked(&self, args: &[&str]) -> Result<(), ForgeError> {
        let out = self.run(args)?;
        if !out.status.success() {
            return Err(ForgeError::Command {
                code: out
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(())
    }
}

/// Map a `gh pr view ... --jq '.state + "|" + (.reviewDecision // "")'` result
/// into a ReviewState. Exit-success with a parseable `STATE|DECISION` line =>
/// a concrete state. A non-zero exit whose stderr reports no PR => a *known*
/// `None`. Any other non-zero exit is undeterminable => Err (caller renders
/// `PR ?`). Pure — fully testable from canned (success, stdout, stderr).
fn parse_review_state(
    exit_ok: bool,
    stdout: &str,
    stderr: &str,
) -> Result<ReviewState, ForgeError> {
    if !exit_ok {
        let s = stderr.to_lowercase();
        if s.contains("no pull requests found") || s.contains("no pull request found") {
            return Ok(ReviewState::None);
        }
        return Err(ForgeError::Command {
            code: "nonzero".to_string(),
            stderr: stderr.trim().to_string(),
        });
    }
    let line = stdout.trim();
    let (state, decision) = line.split_once('|').ok_or_else(|| ForgeError::Parse {
        output: line.to_string(),
        reason: "expected `STATE|DECISION`".to_string(),
    })?;
    let review = match state {
        "MERGED" => ReviewState::Merged,
        "CLOSED" => ReviewState::Closed,
        "OPEN" => match decision {
            "APPROVED" => ReviewState::Approved,
            "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
            _ => ReviewState::Open,
        },
        other => {
            return Err(ForgeError::Parse {
                output: other.to_string(),
                reason: "unknown PR state".to_string(),
            })
        }
    };
    Ok(review)
}

impl ForgePort for Forge {
    type Error = ForgeError;

    fn review_state(&self, branch: &str) -> Result<ReviewState, ForgeError> {
        let out = self.run(&[
            "pr",
            "view",
            branch,
            "--json",
            "state,reviewDecision",
            "--jq",
            r#".state + "|" + (.reviewDecision // "")"#,
        ])?;
        let stdout = String::from_utf8(out.stdout).map_err(ForgeError::Utf8)?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        parse_review_state(out.status.success(), &stdout, &stderr)
    }

    fn create_pr(
        &self,
        _branch: &str,
        _base: &str,
        _title: &str,
        _body: &str,
    ) -> Result<(), ForgeError> {
        unimplemented!("Task 4")
    }

    fn merge(&self, _branch: &str) -> Result<(), ForgeError> {
        unimplemented!("Task 4")
    }

    fn update_from_base(&self, _branch: &str, _base: &str) -> Result<(), ForgeError> {
        unimplemented!("Task 4")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_no_decision_is_open() {
        assert_eq!(
            parse_review_state(true, "OPEN|", "").unwrap(),
            ReviewState::Open
        );
    }

    #[test]
    fn open_review_required_is_open() {
        assert_eq!(
            parse_review_state(true, "OPEN|REVIEW_REQUIRED", "").unwrap(),
            ReviewState::Open
        );
    }

    #[test]
    fn open_approved_is_approved() {
        assert_eq!(
            parse_review_state(true, "OPEN|APPROVED", "").unwrap(),
            ReviewState::Approved
        );
    }

    #[test]
    fn open_changes_requested_is_changes_requested() {
        assert_eq!(
            parse_review_state(true, "OPEN|CHANGES_REQUESTED", "").unwrap(),
            ReviewState::ChangesRequested
        );
    }

    #[test]
    fn merged_is_merged() {
        assert_eq!(
            parse_review_state(true, "MERGED|", "").unwrap(),
            ReviewState::Merged
        );
    }

    #[test]
    fn closed_is_closed() {
        assert_eq!(
            parse_review_state(true, "CLOSED|", "").unwrap(),
            ReviewState::Closed
        );
    }

    #[test]
    fn no_pr_stderr_is_known_none() {
        let r = parse_review_state(false, "", "no pull requests found for branch \"impl/x\"");
        assert_eq!(r.unwrap(), ReviewState::None);
    }

    #[test]
    fn other_nonzero_exit_is_error() {
        // Auth/network failure must be undeterminable (Err), NOT a known None.
        let r = parse_review_state(false, "", "gh: not authenticated");
        assert!(matches!(r, Err(ForgeError::Command { .. })));
    }

    #[test]
    fn unknown_state_is_parse_error() {
        let r = parse_review_state(true, "WAT|", "");
        assert!(matches!(r, Err(ForgeError::Parse { .. })));
    }

    #[test]
    fn missing_delimiter_is_parse_error() {
        let r = parse_review_state(true, "OPEN", "");
        assert!(matches!(r, Err(ForgeError::Parse { .. })));
    }
}
```

- [ ] **Step 2: Run the parser tests**

Run: `cargo test --lib adapters::forge`
Expected: PASS (all 10 tests).

- [ ] **Step 3: Commit**

```bash
git add src/adapters/forge.rs
git commit -m "feat(m2b): forge adapter review_state via gh pr view"
```

---

## Task 4: Forge adapter — write methods (`create_pr`, `merge`, `update_from_base`)

**Files:**
- Modify: `src/adapters/forge.rs`

- [ ] **Step 1: Write the argv-builder tests**

In `src/adapters/forge.rs`, inside `mod tests`, add:

```rust
    #[test]
    fn create_pr_args_are_well_formed() {
        let a = create_pr_args("impl/x", "main", "Add x", "body text");
        assert_eq!(
            a,
            vec![
                "pr", "create",
                "--head", "impl/x",
                "--base", "main",
                "--title", "Add x",
                "--body", "body text",
            ]
        );
    }

    #[test]
    fn merge_args_use_merge_strategy() {
        assert_eq!(merge_args("impl/x"), vec!["pr", "merge", "impl/x", "--merge"]);
    }

    #[test]
    fn update_from_base_args_target_the_branch() {
        assert_eq!(
            update_from_base_args("impl/x", "main"),
            vec!["pr", "update-branch", "impl/x"]
        );
    }
```

- [ ] **Step 2: Run to verify they fail to compile**

Run: `cargo test --lib adapters::forge`
Expected: FAIL — `create_pr_args` / `merge_args` / `update_from_base_args` not found.

- [ ] **Step 3: Add the pure argv-builders**

In `src/adapters/forge.rs`, add above the `#[cfg(test)]` module (after the `impl ForgePort` block):

```rust
/// Build the `gh` argv for opening a PR. Pure — asserted in tests.
fn create_pr_args<'a>(branch: &'a str, base: &'a str, title: &'a str, body: &'a str) -> Vec<&'a str> {
    vec![
        "pr", "create", "--head", branch, "--base", base, "--title", title, "--body", body,
    ]
}

/// Build the `gh` argv for merging a PR (merge-commit strategy).
fn merge_args(branch: &str) -> Vec<&str> {
    vec!["pr", "merge", branch, "--merge"]
}

/// Build the `gh` argv for updating a PR branch from its base.
fn update_from_base_args<'a>(branch: &'a str, _base: &'a str) -> Vec<&'a str> {
    vec!["pr", "update-branch", branch]
}
```

- [ ] **Step 4: Replace the `unimplemented!` write methods**

In `src/adapters/forge.rs`, replace the three `unimplemented!("Task 4")` method bodies in `impl ForgePort for Forge`:

```rust
    fn create_pr(
        &self,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        self.run_checked(&create_pr_args(branch, base, title, body))
    }

    fn merge(&self, branch: &str) -> Result<(), ForgeError> {
        self.run_checked(&merge_args(branch))
    }

    fn update_from_base(&self, branch: &str, base: &str) -> Result<(), ForgeError> {
        self.run_checked(&update_from_base_args(branch, base))
    }
```

- [ ] **Step 5: Add an `#[ignore]`d live smoke test**

In `src/adapters/forge.rs`, inside `mod tests`, add:

```rust
    // Live test against real `gh` + a real repo/PR. Never runs in CI; run
    // manually with `cargo test -- --ignored forge_live_review_state`.
    #[test]
    #[ignore]
    fn forge_live_review_state() {
        let forge = Forge::new(".");
        // Adjust the branch to one with a known PR before running manually.
        let _ = forge.review_state("main");
    }
```

- [ ] **Step 6: Run the forge tests**

Run: `cargo test --lib adapters::forge`
Expected: PASS (13 non-ignored tests; the live test is skipped).

- [ ] **Step 7: Commit**

```bash
git add src/adapters/forge.rs
git commit -m "feat(m2b): forge adapter write actions (create_pr/merge/update_from_base)"
```

---

## Task 5: Checkpoint store

**Files:**
- Modify: `src/adapters/checkpoints.rs`

- [ ] **Step 1: Write the store, pure parser, error type, and tests**

Replace the contents of `src/adapters/checkpoints.rs` with:

```rust
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
```

- [ ] **Step 2: Run the checkpoint tests**

Run: `cargo test --lib adapters::checkpoints`
Expected: PASS (6 tests).

- [ ] **Step 3: Commit**

```bash
git add src/adapters/checkpoints.rs
git commit -m "feat(m2b): checkpoint store review_state over .circuit/checkpoints"
```

---

## Task 6: Delivery-mode resolver

**Files:**
- Modify: `src/adapters/delivery.rs`

- [ ] **Step 1: Write the enum, pure resolver, and tests**

Replace the contents of `src/adapters/delivery.rs` with:

```rust
//! Delivery-mode selection: forge (GitHub via `gh`) vs local checkpoints. The
//! decision is a pure function of two detected facts so it is unit-testable
//! without shelling out; detection itself lives in the CLI (§7.1). Resolved
//! once per `circuit flow` run and applied repo-wide.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryMode {
    Forge,
    Local,
}

/// Forge iff `gh` is usable AND the repo has a GitHub remote; else Local.
pub fn resolve(gh_available: bool, has_github_remote: bool) -> DeliveryMode {
    if gh_available && has_github_remote {
        DeliveryMode::Forge
    } else {
        DeliveryMode::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_and_remote_selects_forge() {
        assert_eq!(resolve(true, true), DeliveryMode::Forge);
    }

    #[test]
    fn no_gh_selects_local() {
        assert_eq!(resolve(false, true), DeliveryMode::Local);
    }

    #[test]
    fn no_remote_selects_local() {
        assert_eq!(resolve(true, false), DeliveryMode::Local);
    }

    #[test]
    fn neither_selects_local() {
        assert_eq!(resolve(false, false), DeliveryMode::Local);
    }
}
```

- [ ] **Step 2: Run the resolver tests**

Run: `cargo test --lib adapters::delivery`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add src/adapters/delivery.rs
git commit -m "feat(m2b): delivery-mode resolver (forge vs local checkpoint)"
```

---

## Task 7: Wire real review state into `run_flow`

**Files:**
- Modify: `src/main.rs` (imports near top; `run_flow`)

- [ ] **Step 1: Add the imports**

In `src/main.rs`, add to the `use circuit::...` block near the top:

```rust
use circuit::adapters::checkpoints::Checkpoints;
use circuit::adapters::delivery::{self, DeliveryMode};
use circuit::adapters::forge::Forge;
```

And widen the existing ports import from `use circuit::ports::GitPort;` to:

```rust
use circuit::ports::{CheckpointStore, ForgePort, GitPort};
```

- [ ] **Step 2: Add the two detection helpers**

In `src/main.rs`, add these free functions (e.g. just above `fn run_flow`):

```rust
/// Is the `gh` CLI installed and runnable? (Auth is checked per-call via the
/// review_state error path; this only gates which source we consult.)
fn gh_available() -> bool {
    std::process::Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Does the repo at `root` have a remote pointing at github.com?
fn has_github_remote(root: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "-v"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("github.com"))
        .unwrap_or(false)
}
```

- [ ] **Step 3: Replace the hardcoded review block in `run_flow`**

In `src/main.rs`, in `run_flow`, after `let git = Git::new(ws.root());` add the mode + adapters:

```rust
    let mode = delivery::resolve(gh_available(), has_github_remote(ws.root()));
    let forge = Forge::new(ws.root());
    let checkpoints = Checkpoints::new(ws.root());
```

Then replace the per-session facts construction. The current loop body builds:

```rust
        let facts = DeliveryFacts {
            branch: branch_facts,
            review: None,
        };
```

Replace that `let facts = …;` statement with:

```rust
        // Resolve real review state from the selected source. Any adapter Err
        // (forge unreachable, unreadable checkpoint) degrades to `None` —
        // the honest "undeterminable" path that renders `PR ?` (§7.2).
        let review = match (&s.branch, mode) {
            (Some(b), DeliveryMode::Forge) => forge.review_state(b).ok(),
            (Some(_), DeliveryMode::Local) => {
                checkpoints.review_state(&s.id.to_string()).ok()
            }
            (None, _) => None,
        };
        let facts = DeliveryFacts {
            branch: branch_facts,
            review,
        };
```

- [ ] **Step 4: Build and run the full test suite**

Run: `cargo build && cargo test`
Expected: builds clean; all tests pass (no regressions; the new adapter/flow tests included).

- [ ] **Step 5: Manual smoke verification**

The `run_flow` glue is thin over unit-tested pieces; verify it end-to-end manually.

Local path (no GitHub remote needed):

```bash
# In a repo initialized with `circuit init` that has at least one impl session
# with a branch, create a checkpoint file keyed on that session's ULID:
mkdir -p .circuit/checkpoints
echo 'state = "self-review"' > .circuit/checkpoints/<SESSION_ULID>.toml
cargo run -- flow
```

Expected: that session's rail line shows `‹Review›` and `PR open` (not `PR ?`).
Remove the file and re-run → `‹Implement›` and `no PR`.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(m2b): wire forge/checkpoint review state into circuit flow"
```

---

## Self-review notes (for the implementer)

- **`gh --jq` requires no external `jq`** — `gh` ships its own query engine. The `--jq '.state + "|" + (.reviewDecision // "")'` expression yields e.g. `OPEN|APPROVED`, `OPEN|` (null decision), `MERGED|`.
- **Honesty invariant:** the only place `Err` is swallowed is `.ok()` in `run_flow` Step 3, which is exactly the documented forge-unreachable → `None` → `PR ?` path. `parse_review_state` keeps "known no PR" (`Ok(None)`) distinct from "couldn't ask" (`Err`).
- **No foundation type beyond `ReviewState::ChangesRequested`** is touched. Session archival, mode-aware wording, the `delivery` config override, and forge write CLI verbs are deferred (spec §9).
- **Run formatting/lints before the final commit:** `cargo fmt` and `cargo clippy` if used elsewhere in the repo.
