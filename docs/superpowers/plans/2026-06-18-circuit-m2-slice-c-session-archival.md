# M2 Slice C — Session Archival Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an Axis-2 `active | archived` lifecycle to sessions — `circuit session archive`/`unarchive` commands that tear down / rehydrate the git worktree and flip a durable status field, plus `circuit flow --all` filtering.

**Architecture:** A new `status` field on `SessionRecord` (the durable, machine-readable "session retired" signal). `archive` locates the worktree via `git worktree list` (path is never stored), removes it, optionally deletes the branch, then flips status; `unarchive` flips back and re-adds the worktree from the kept branch. `derive_stage` (Axis 1) is untouched. Mirrors slice B's adapter/test patterns; hexagonal — new IO lives behind `GitPort`.

**Tech Stack:** Rust, `serde`/`toml`, `thiserror` at adapter boundaries, `anyhow` in `main.rs`, `clap` derive CLI, `assert_cmd`/`predicates` for integration tests.

## Global Constraints

- `#![forbid(unsafe_code)]` — no `unsafe` anywhere (copied from crate root).
- New adapter errors reuse existing `thiserror` enums (`GitError`); `main.rs` uses `anyhow` context.
- `SessionStatus` serializes lowercase: `"active" | "archived"`.
- `SCHEMA_VERSION = 2` (bumped from 1); back-compat via `#[serde(default)]`, never version-gated parsing.
- Archive is **idempotent** (re-archiving an archived session is a no-op success).
- Branch is **kept by default**; deletion only via `--delete-branch`. `--force` governs both dirty-worktree removal and un-merged-branch deletion.
- Spec at `docs/superpowers/specs/2026-06-16-circuit-m2-slice-c-session-archival-design.md`.

---

### Task 1: `SessionStatus` enum + `status` field + schema bump

**Files:**
- Modify: `src/session/mod.rs`
- Test: `src/session/mod.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub enum SessionStatus { Active, Archived }` (derives `Default` → `Active`); `pub const SCHEMA_VERSION: u32 = 2;`; `SessionRecord.status: SessionStatus`; methods `SessionRecord::is_archived(&self) -> bool`, `archive(&mut self)`, `unarchive(&mut self)`.

- [ ] **Step 1: Write the failing tests**

Add these tests inside the existing `mod tests` block in `src/session/mod.rs`:

```rust
    #[test]
    fn new_session_is_active_at_schema_v2() {
        let s = SessionRecord::spec(SessionId::generate());
        assert_eq!(s.status, SessionStatus::Active);
        assert!(!s.is_archived());
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert_eq!(SCHEMA_VERSION, 2);
    }

    #[test]
    fn archive_and_unarchive_flip_status_and_normalize_version() {
        let mut s = SessionRecord::impl_(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "impl/checkout-auth",
        );
        s.archive();
        assert!(s.is_archived());
        assert_eq!(s.status, SessionStatus::Archived);
        assert_eq!(s.schema_version, 2);
        s.unarchive();
        assert!(!s.is_archived());
        assert_eq!(s.status, SessionStatus::Active);
    }

    #[test]
    fn status_serializes_lowercase_and_round_trips() {
        let mut s = SessionRecord::spec(SessionId::generate());
        s.archive();
        let text = toml::to_string_pretty(&s).unwrap();
        assert!(
            text.contains("status = \"archived\""),
            "expected lowercase status, got: {text}"
        );
        let parsed: SessionRecord = toml::from_str(&text).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn v1_record_without_status_parses_as_active() {
        // A slice-0/A/B record predates the `status` field. It must load as
        // Active (back-compat via #[serde(default)]).
        let text = format!(
            "schema_version = 1\nid = \"{SAMPLE_ULID}\"\nkind = \"spec\"\n"
        );
        let s: SessionRecord = toml::from_str(&text).unwrap();
        assert_eq!(s.status, SessionStatus::Active);
        assert!(!s.is_archived());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib session::tests 2>&1 | head -30`
Expected: FAIL — compile errors (`SessionStatus` / `SCHEMA_VERSION` / `is_archived` / `archive` not found).

- [ ] **Step 3: Add the enum, constant, field, and methods**

In `src/session/mod.rs`, add the enum and constant just above `SessionRecord` (after the `SessionKind` enum):

```rust
/// Axis-2 lifecycle status (the M2 "session model" §3.3 — orthogonal to the
/// derived flow stage). Serializes lowercase (`"active" | "archived"`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    #[default]
    Active,
    Archived,
}

/// On-disk schema version for `SessionRecord`. Bumped to 2 when the `status`
/// field was added; stored (not validated) in M2, so the bump is documentary.
pub const SCHEMA_VERSION: u32 = 2;
```

Add the field to `SessionRecord` (after the existing `fixes_indicator` field):

```rust
    /// Axis-2 lifecycle status. `#[serde(default)]` => a pre-v2 record with no
    /// `status` key loads as `Active`, so slice-0/A/B records parse unchanged.
    #[serde(default)]
    pub status: SessionStatus,
```

In **all three** constructors (`spec`, `impl_`, `fix`), replace `schema_version: 1,` with `schema_version: SCHEMA_VERSION,` and add `status: SessionStatus::Active,` to the struct literal.

Add the methods in an `impl SessionRecord` block (extend the existing one, after `fix`):

```rust
    /// Is this session retired (Axis 2)?
    pub fn is_archived(&self) -> bool {
        self.status == SessionStatus::Archived
    }

    /// Retire the session. Normalizes `schema_version` — a record carrying a
    /// `status` field is v2 by definition.
    pub fn archive(&mut self) {
        self.status = SessionStatus::Archived;
        self.schema_version = SCHEMA_VERSION;
    }

    /// Return the session to active rotation. Normalizes `schema_version`.
    pub fn unarchive(&mut self) {
        self.status = SessionStatus::Active;
        self.schema_version = SCHEMA_VERSION;
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib session::tests 2>&1 | tail -20`
Expected: PASS — all session tests green (existing + 4 new).

- [ ] **Step 5: Commit**

```bash
git add src/session/mod.rs
git commit -m "feat(m2c): add SessionStatus + status field (schema v2)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `GitPort::remove_worktree` + `delete_branch`

**Files:**
- Modify: `src/ports.rs` (trait + `FakeGit` test impl)
- Modify: `src/adapters/git.rs` (impl + tests)

**Interfaces:**
- Consumes: existing `GitError`, `GitPort`, `Git`, `parse_worktree_porcelain`.
- Produces: `GitPort::remove_worktree(&self, path: &Path, force: bool) -> Result<(), Self::Error>` and `GitPort::delete_branch(&self, branch: &str, force: bool) -> Result<(), Self::Error>`.

- [ ] **Step 1: Write the failing adapter tests**

Add to the `#[cfg(test)] mod tests` block in `src/adapters/git.rs`:

```rust
    #[test]
    fn remove_worktree_removes_a_clean_worktree_keeping_branch() {
        let (d, git) = init_repo();
        git.create_branch("impl/x", "main").unwrap();
        let wt = d.path().join("wt-x");
        git.add_worktree("impl/x", &wt).unwrap();
        assert!(wt.exists());

        git.remove_worktree(&wt, false).unwrap();
        assert!(!wt.exists(), "worktree dir should be gone");
        // Branch survives worktree removal.
        assert!(git.branch_facts("impl/x", "main").unwrap().exists);
    }

    #[test]
    fn remove_worktree_refuses_dirty_without_force_then_succeeds_with_force() {
        let (d, git) = init_repo();
        git.create_branch("impl/x", "main").unwrap();
        let wt = d.path().join("wt-x");
        git.add_worktree("impl/x", &wt).unwrap();
        // An untracked file makes the worktree dirty (git refuses removal).
        std::fs::write(wt.join("scratch.txt"), "wip\n").unwrap();

        assert!(
            git.remove_worktree(&wt, false).is_err(),
            "dirty worktree must be refused without force"
        );
        assert!(wt.exists());
        git.remove_worktree(&wt, true).unwrap();
        assert!(!wt.exists());
    }

    #[test]
    fn delete_branch_removes_merged_with_d_and_unmerged_only_with_force() {
        let (d, git) = init_repo();
        let p = d.path();

        // Merged branch (fresh, == main): -d (force=false) succeeds.
        git.create_branch("merged", "main").unwrap();
        git.delete_branch("merged", false).unwrap();
        assert!(!git.branch_facts("merged", "main").unwrap().exists);

        // Un-merged branch (a commit ahead): -d refuses, -D succeeds.
        git.create_branch("feat", "main").unwrap();
        git_raw(p, &["worktree", "add", "-q", "wt", "feat"]);
        std::fs::write(p.join("wt/new.txt"), "x\n").unwrap();
        git_raw(&p.join("wt"), &["add", "new.txt"]);
        git_raw(&p.join("wt"), &["commit", "-qm", "work"]);

        assert!(
            git.delete_branch("feat", false).is_err(),
            "un-merged branch must be refused without force"
        );
        assert!(git.branch_facts("feat", "main").unwrap().exists);
        git.delete_branch("feat", true).unwrap();
        assert!(!git.branch_facts("feat", "main").unwrap().exists);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib adapters::git 2>&1 | head -30`
Expected: FAIL — `no method named remove_worktree`/`delete_branch` for `Git` (and trait does not declare them).

- [ ] **Step 3: Extend the `GitPort` trait and `FakeGit`**

In `src/ports.rs`, add to the `GitPort` trait (after `list_worktrees`):

```rust
    /// Remove a worktree dir. `force` removes a dirty/locked worktree
    /// (`git worktree remove --force`). The branch is never touched.
    fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), Self::Error>;
    /// Delete a branch. `force` (`git branch -D`) deletes an un-merged branch;
    /// without it (`-d`) git refuses an un-merged branch.
    fn delete_branch(&self, branch: &str, force: bool) -> Result<(), Self::Error>;
```

In the same file's `FakeGit` test impl (in `mod tests`), add:

```rust
        fn remove_worktree(&self, _path: &Path, _force: bool) -> Result<(), Self::Error> {
            Ok(())
        }
        fn delete_branch(&self, _branch: &str, _force: bool) -> Result<(), Self::Error> {
            Ok(())
        }
```

- [ ] **Step 4: Implement both methods on `Git`**

In `src/adapters/git.rs`, inside `impl GitPort for Git`, add (after `list_worktrees`):

```rust
    fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), GitError> {
        let path_str = path.to_str().ok_or_else(|| GitError::Parse {
            output: path.display().to_string(),
            reason: "worktree path is not valid UTF-8".to_string(),
        })?;
        let mut args = vec!["worktree", "remove", path_str];
        if force {
            args.push("--force");
        }
        self.run(&args).map(|_| ())
    }

    fn delete_branch(&self, branch: &str, force: bool) -> Result<(), GitError> {
        let flag = if force { "-D" } else { "-d" };
        self.run(&["branch", flag, branch]).map(|_| ())
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib adapters::git 2>&1 | tail -20 && cargo test --lib ports 2>&1 | tail -5`
Expected: PASS — git adapter tests (existing + 3 new) and ports tests green.

- [ ] **Step 6: Commit**

```bash
git add src/ports.rs src/adapters/git.rs
git commit -m "feat(m2c): GitPort worktree-remove + branch-delete

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `(archived)` marker in the flow rail

**Files:**
- Modify: `src/flow/rail.rs` (`render_rail` signature + marker + tests)
- Modify: `src/main.rs:426-434` (the single `render_rail` call site — pass `s.is_archived()`)

**Interfaces:**
- Consumes: `SessionRecord::is_archived` (Task 1).
- Produces: `render_rail(node_id, kind, view, branch, facts, review, health, archived: bool)` — `archived` is the **new final parameter**.

- [ ] **Step 1: Write the failing test**

Add to `#[cfg(test)] mod tests` in `src/flow/rail.rs`:

```rust
    #[test]
    fn archived_session_renders_a_marker_active_does_not() {
        let view = StageView {
            stage: Stage::Done,
            forge_certain: true,
        };
        let archived = render_rail(
            "a",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(3),
            Some(ReviewState::Merged),
            Health::Sound,
            true,
        );
        assert!(archived.contains("(archived)"), "got: {archived}");

        let active = render_rail(
            "a",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(3),
            Some(ReviewState::Merged),
            Health::Sound,
            false,
        );
        assert!(!active.contains("(archived)"), "got: {active}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib flow::rail 2>&1 | head -30`
Expected: FAIL — `render_rail` takes 7 arguments but 8 supplied (signature mismatch across the file).

- [ ] **Step 3: Add the `archived` parameter and marker**

In `src/flow/rail.rs`, change the `render_rail` signature to add `archived: bool` as the final parameter:

```rust
pub fn render_rail(
    node_id: &str,
    kind: SessionKind,
    view: StageView,
    branch: Option<&str>,
    facts: &BranchFacts,
    review: Option<ReviewState>,
    health: Health,
    archived: bool,
) -> String {
```

Replace the `line1` construction with one that injects the marker after the kind tag:

```rust
    let status_marker = if archived { " (archived)" } else { "" };
    let line1 = format!("{node_id}  [{}]{status_marker}  {spine}{uncertain}", kind_label(kind));
```

- [ ] **Step 4: Update every existing `render_rail` call in the rail tests**

In `src/flow/rail.rs` `mod tests`, append `, false` as the final argument to the `render_rail(...)` call in **each** of these existing tests (they all render non-archived sessions):
`marks_the_current_stage_with_guillemets`, `line_one_has_node_id_and_kind`, `undeterminable_review_prints_pr_question_mark`, `known_no_pr_differs_from_undeterminable`, `commit_count_and_branch_name_appear`, `spec_session_without_branch_renders_no_branch`, `rail_contains_no_ansi_color_codes`, `changes_requested_renders_its_own_label`, `renders_fix_kind_and_done_as_current_stage`.

(Each call currently ends with `Health::<X>,\n        );` — change to `Health::<X>,\n            false,\n        );`.)

- [ ] **Step 5: Update the `main.rs` call site**

In `src/main.rs`, the `render_rail` call in `run_flow` (currently ending `Health::Unknown,\n        ));`) — add the archived flag as the final argument:

```rust
        blocks.push(render_rail(
            &label,
            s.kind,
            view,
            s.branch.as_deref(),
            &facts.branch,
            facts.review,
            Health::Unknown,
            s.is_archived(),
        ));
```

- [ ] **Step 6: Run tests + full build to verify pass**

Run: `cargo test --lib flow::rail 2>&1 | tail -20 && cargo build 2>&1 | tail -5`
Expected: PASS — rail tests green; crate builds (main.rs call site compiles).

- [ ] **Step 7: Commit**

```bash
git add src/flow/rail.rs src/main.rs
git commit -m "feat(m2c): render (archived) marker in the flow rail

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `circuit session archive <id>` command

**Files:**
- Modify: `src/main.rs` (`SessionCommand` enum, `run_session` dispatch, new `run_session_archive`)
- Test: `tests/session_flow.rs`

**Interfaces:**
- Consumes: `SessionRecord::{archive, is_archived}` (Task 1); `GitPort::{list_worktrees, remove_worktree, delete_branch}` (Task 2); existing `resolve_session`, `Git`, `Workspace`, `require_initialized`.
- Produces: `circuit session archive <id> [--delete-branch] [--force]`.

- [ ] **Step 1: Write the failing integration test**

Add to `tests/session_flow.rs` (the helpers `circuit` and `init_git_repo` already exist in this file):

```rust
/// Read the single session record's ULID + raw TOML from a workspace.
fn read_only_session(dir: &Path) -> (String, String) {
    let entry = std::fs::read_dir(dir.join(".circuit").join("sessions"))
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().and_then(|x| x.to_str()) == Some("toml"))
        .expect("a session record was written");
    let ulid = entry.file_name().to_string_lossy().replace(".toml", "");
    let text = std::fs::read_to_string(entry.path()).unwrap();
    (ulid, text)
}

/// init repo + workspace + a spec + a DAG node + spawn one impl session.
/// Returns (repo dir, worktree-root dir) — keep both alive for the test.
fn spawn_one(branch: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let wt_root = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    circuit(dir.path()).arg("init").assert().success();
    circuit(dir.path())
        .args(["spec", "new", "checkout", "--title", "C", "--intent", "Pay."])
        .assert()
        .success();
    circuit(dir.path())
        .args([
            "dag", "add-node", "auth-slice", "--spec", "checkout", "--title", "Auth",
            "--branch", branch,
        ])
        .assert()
        .success();
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .success();
    (dir, wt_root)
}

#[test]
fn archive_frees_worktree_flips_status_keeps_branch_and_is_idempotent() {
    let (dir, wt_root) = spawn_one("impl/checkout-auth");
    let (ulid, _) = read_only_session(dir.path());
    let wt = wt_root.path().join(&ulid);
    assert!(wt.exists(), "spawn created the worktree");

    // Archive (clean worktree => no --force needed).
    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("archived"))
        .stdout(predicate::str::contains("agent session may now end"));

    // Worktree gone; status flipped; branch kept.
    assert!(!wt.exists(), "archive removed the worktree dir");
    let (_, text) = read_only_session(dir.path());
    assert!(text.contains("status = \"archived\""), "got: {text}");
    let branch_listed = Stdcmd::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["branch", "--list", "impl/checkout-auth"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&branch_listed.stdout).trim().is_empty(),
        "branch should be kept by default"
    );

    // Idempotent: archiving again is a no-op success.
    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already archived"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test session_flow archive_frees 2>&1 | head -30`
Expected: FAIL — `error: unrecognized subcommand 'archive'` (or compile error if run before build).

- [ ] **Step 3: Add the `Archive` subcommand variant**

In `src/main.rs`, add to the `SessionCommand` enum (after `Spawn`):

```rust
    /// Archive (retire) a session: tear down its worktree, optionally delete
    /// its branch, and flip status to `archived` (the durable agent-stop signal).
    Archive {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        /// Also delete the session's branch (default: keep it)
        #[arg(long)]
        delete_branch: bool,
        /// Remove a dirty/locked worktree and delete an un-merged branch
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

- [ ] **Step 4: Add the dispatch arm**

In `run_session`, add to the `match command` block:

```rust
        SessionCommand::Archive {
            id,
            delete_branch,
            force,
            path,
        } => run_session_archive(&id, delete_branch, force, &path),
```

- [ ] **Step 5: Implement `run_session_archive`**

Add this function to `src/main.rs` (after `run_session_spawn`):

```rust
/// Archive a session: locate + remove its worktree (path is never stored, so
/// we find it by branch in `git worktree list`), optionally delete the branch,
/// then flip status. Teardown precedes the status flip so a failed teardown
/// leaves the session truthfully `active`. Idempotent.
fn run_session_archive(id: &str, delete_branch: bool, force: bool, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;

    let mut record = resolve_session(&ws, id)?;
    if record.is_archived() {
        println!("Session {} already archived.", record.id);
        return Ok(());
    }

    let git = Git::new(ws.root());

    // 1. Tear down the worktree, located by branch (never stored). A dirty
    //    worktree is refused without --force — the finalizer analog: a live
    //    agent dirties its worktree, so plain archive refuses while it works.
    if let Some(branch) = &record.branch {
        let worktrees = git.list_worktrees().context("listing worktrees")?;
        if let Some(wt) = worktrees
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(branch.as_str()))
        {
            git.remove_worktree(&wt.path, force).with_context(|| {
                format!(
                    "removing worktree at {} (pass --force to discard uncommitted \
                     changes or unlock — stop the agent first if it is still running)",
                    wt.path.display()
                )
            })?;
        }
    }

    // 2. Optionally delete the branch (un-merged requires --force).
    if delete_branch {
        if let Some(branch) = &record.branch {
            git.delete_branch(branch, force).with_context(|| {
                format!("deleting branch {branch} (pass --force to delete an un-merged branch)")
            })?;
        }
    }

    // 3. Flip the durable status signal.
    record.archive();
    ws.save_session(&record)
        .with_context(|| format!("saving archived session {}", record.id))?;

    println!("Session {} archived — agent session may now end.", record.id);
    match (&record.branch, delete_branch) {
        (Some(b), true) => println!("  branch {b} deleted"),
        (Some(b), false) => println!("  branch {b} kept (use --delete-branch to remove)"),
        (None, _) => {}
    }
    Ok(())
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --test session_flow archive_frees 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 7: Add the dirty-worktree + delete-branch test**

Add to `tests/session_flow.rs`:

```rust
#[test]
fn archive_refuses_dirty_worktree_without_force_and_delete_branch_needs_force() {
    let (dir, wt_root) = spawn_one("impl/checkout-auth");
    let (ulid, _) = read_only_session(dir.path());
    let wt = wt_root.path().join(&ulid);

    // Dirty the worktree (untracked file) so removal is refused.
    std::fs::write(wt.join("scratch.txt"), "wip\n").unwrap();
    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
    // Status untouched on failed teardown.
    let (_, text) = read_only_session(dir.path());
    assert!(!text.contains("status = \"archived\""), "got: {text}");

    // --force discards the dirty worktree; --delete-branch + --force removes
    // the (un-merged once we commit? here still fresh) branch too.
    circuit(dir.path())
        .args(["session", "archive", "auth-slice", "--delete-branch", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("branch impl/checkout-auth deleted"));
    assert!(!wt.exists());
    let branch_listed = Stdcmd::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["branch", "--list", "impl/checkout-auth"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branch_listed.stdout).trim().is_empty(),
        "branch should be deleted"
    );
}
```

- [ ] **Step 8: Run test to verify it passes**

Run: `cargo test --test session_flow archive_refuses 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src/main.rs tests/session_flow.rs
git commit -m "feat(m2c): circuit session archive command

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `circuit session unarchive <id>` command

**Files:**
- Modify: `src/main.rs` (`SessionCommand` enum, `run_session` dispatch, new `run_session_unarchive`)
- Test: `tests/session_flow.rs`

**Interfaces:**
- Consumes: `SessionRecord::{unarchive, is_archived}` (Task 1); `GitPort::{branch_facts, add_worktree}`; existing `resolve_session`, `resolve_worktree_dir`, `Git`, `Workspace`.
- Produces: `circuit session unarchive <id>`.

- [ ] **Step 1: Write the failing integration test**

Add to `tests/session_flow.rs`:

```rust
#[test]
fn unarchive_flips_status_and_rehydrates_worktree() {
    let (dir, wt_root) = spawn_one("impl/checkout-auth");
    let (ulid, _) = read_only_session(dir.path());
    let wt = wt_root.path().join(&ulid);

    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .success();
    assert!(!wt.exists());

    // Unarchive (same worktree-root env so the path resolves identically).
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "unarchive", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("restored"));

    assert!(wt.exists(), "unarchive re-added the worktree");
    let (_, text) = read_only_session(dir.path());
    assert!(text.contains("status = \"active\""), "got: {text}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test session_flow unarchive_flips 2>&1 | head -30`
Expected: FAIL — `unrecognized subcommand 'unarchive'`.

- [ ] **Step 3: Add the `Unarchive` subcommand variant**

In `src/main.rs`, add to the `SessionCommand` enum (after `Archive`):

```rust
    /// Unarchive (restore) a session: flip status back to `active` and re-add
    /// the worktree from the kept branch.
    Unarchive {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

- [ ] **Step 4: Add the dispatch arm**

In `run_session`'s `match`:

```rust
        SessionCommand::Unarchive { id, path } => run_session_unarchive(&id, &path),
```

- [ ] **Step 5: Implement `run_session_unarchive`**

Add to `src/main.rs` (after `run_session_archive`):

```rust
/// Restore an archived session: flip status to active, then re-add the worktree
/// from the kept branch (resolving its path exactly as `spawn` does). If the
/// branch was deleted (archive --delete-branch), warn instead of recreating.
fn run_session_unarchive(id: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;

    let mut record = resolve_session(&ws, id)?;
    if !record.is_archived() {
        println!("Session {} is not archived.", record.id);
        return Ok(());
    }

    record.unarchive();
    ws.save_session(&record)
        .with_context(|| format!("saving restored session {}", record.id))?;
    println!("Session {} restored to active.", record.id);

    // Rehydrate the worktree from the kept branch.
    if let Some(branch) = &record.branch {
        let git = Git::new(ws.root());
        let base = ws.load_config().context("loading config.toml")?.base_branch;
        let exists = git
            .branch_facts(branch, &base)
            .with_context(|| format!("checking branch {branch}"))?
            .exists;
        if exists {
            let local = ws.load_local().context("loading local.toml")?;
            let env = std::env::var("CIRCUIT_WORKTREES_DIR").ok();
            let worktree =
                resolve_worktree_dir(env.as_deref(), &local, ws.root(), &record.id.to_string());
            git.add_worktree(branch, &worktree)
                .with_context(|| format!("re-adding worktree at {}", worktree.display()))?;
            println!("  worktree: {}", worktree.display());
        } else {
            println!(
                "  branch {branch} no longer exists — worktree not recreated; \
                 session derives Draft"
            );
        }
    }
    Ok(())
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test --test session_flow unarchive_flips 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs tests/session_flow.rs
git commit -m "feat(m2c): circuit session unarchive command

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: `circuit flow --all` archived filtering

**Files:**
- Modify: `src/main.rs` (`Command::Flow` variant, `main` dispatch, `run_flow` signature + filter)
- Test: `tests/session_flow.rs`

**Interfaces:**
- Consumes: `SessionRecord::is_archived` (Task 1).
- Produces: `circuit flow [session] [--all]`; `run_flow(selector: Option<&str>, all: bool, path: &Path)`.

- [ ] **Step 1: Write the failing integration test**

Add to `tests/session_flow.rs`:

```rust
#[test]
fn flow_hides_archived_by_default_and_all_shows_with_marker() {
    let (dir, _wt_root) = spawn_one("impl/checkout-auth");
    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .success();

    // Default `flow` (no selector) hides the archived impl session.
    circuit(dir.path())
        .arg("flow")
        .assert()
        .success()
        .stdout(predicate::str::contains("[impl]").not());

    // `flow --all` shows it, with the (archived) marker.
    circuit(dir.path())
        .args(["flow", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[impl]"))
        .stdout(predicate::str::contains("(archived)"));

    // An explicit selector shows the archived session regardless of --all.
    circuit(dir.path())
        .args(["flow", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(archived)"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test session_flow flow_hides 2>&1 | head -30`
Expected: FAIL — `unexpected argument '--all'` (and/or the default `flow` still prints `[impl]`).

- [ ] **Step 3: Add the `--all` flag to the `Flow` command**

In `src/main.rs`, add to the `Command::Flow` variant:

```rust
    /// Render the per-session flow rail
    Flow {
        /// Session id (ULID) or unique DAG-node name; omit to show all sessions
        session: Option<String>,
        /// Include archived sessions in the no-selector list
        #[arg(long)]
        all: bool,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

- [ ] **Step 4: Update the `main` dispatch**

In `main`, change the `Flow` arm:

```rust
        Command::Flow { session, all, path } => run_flow(session.as_deref(), all, &path),
```

- [ ] **Step 5: Add `all` to `run_flow` and filter**

Change the `run_flow` signature and the session-selection block in `src/main.rs`:

```rust
fn run_flow(selector: Option<&str>, all: bool, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;

    let sessions = match selector {
        // An explicit selector always shows the named session, even archived.
        Some(sel) => vec![resolve_session(&ws, sel)?],
        None => {
            let mut listed = ws.list_sessions().context("listing sessions")?;
            // Hide archived sessions by default; --all includes them.
            if !all {
                listed.retain(|s| !s.is_archived());
            }
            listed
        }
    };
```

(The rest of `run_flow` is unchanged — it already passes `s.is_archived()` to `render_rail` from Task 3.)

- [ ] **Step 6: Run test + full suite to verify pass**

Run: `cargo test --test session_flow flow_hides 2>&1 | tail -20 && cargo test 2>&1 | tail -15`
Expected: PASS — the new test and the entire suite green.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs tests/session_flow.rs
git commit -m "feat(m2c): flow --all filters archived sessions

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Verification + clippy + fmt

**Files:** none (verification only)

- [ ] **Step 1: Full build with unsafe forbidden**

Run: `cargo build 2>&1 | tail -10`
Expected: builds clean; no `unsafe_code` warnings/errors.

- [ ] **Step 2: Full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: all unit + integration tests PASS.

- [ ] **Step 3: Lint + format**

Run: `cargo clippy --all-targets 2>&1 | tail -20 && cargo fmt --check`
Expected: no clippy warnings; `fmt --check` clean (run `cargo fmt` and re-commit if it reports diffs).

- [ ] **Step 4: Commit any fmt fixes (only if needed)**

```bash
cargo fmt
git add -A
git commit -m "style(m2c): cargo fmt

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- §4 data model (SessionStatus, status field, SCHEMA_VERSION, helpers, v1 back-compat) → Task 1 ✓
- §5 GitPort remove_worktree/delete_branch + adapter tests + FakeGit → Task 2 ✓
- §6 archive command (idempotent, teardown-before-flip, dirty-refuse, --delete-branch, --force, located-by-branch) → Task 4 ✓
- §6.3 finalizer-analog (dirty refusal) → Task 4 test `archive_refuses_dirty...` ✓
- §7 unarchive (flip + rehydrate + deleted-branch warning) → Task 5 ✓ (deleted-branch warning path coded; happy path tested)
- §8 flow --all filtering + explicit-selector-always-shows + (archived) marker → Task 3 (marker) + Task 6 (filter) ✓
- §12 exit criteria → covered across Tasks 1/4/5/6 + Task 7 (forbid(unsafe_code), full suite)

**Placeholder scan:** No TBD/TODO/"handle edge cases" — every code step shows complete code; the one mechanical instruction (Task 3 Step 4) names each affected test and the exact token to add.

**Type consistency:** `render_rail`'s new final `archived: bool` is consistent across Task 3 (definition, test calls, main.rs call site). `run_flow(selector, all, path)` consistent between Task 6 Steps 4–5. `SessionStatus`/`SCHEMA_VERSION`/`is_archived`/`archive`/`unarchive` defined in Task 1 and consumed with matching names in Tasks 3–6. `remove_worktree(path, force)`/`delete_branch(branch, force)` consistent between Task 2 and Task 4.

**Note for executor:** Task 4 Step 7's branch-delete assertion deletes a *fresh* (merged-into-base) branch, which `-d` would accept — but the command passes `--force` so `-D` is used; the assertion only checks the branch is gone, so it holds regardless.
