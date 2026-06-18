# Circuit M2 Slice C — Session Archival (Lifecycle Axis 2)

**Status:** Draft for review
**Date:** 2026-06-16
**Milestone:** M2 (Session model + flow + git)
**Depends on:** M2 foundation (slice 0, #5), M2 slice A (git adapter + spawn + flow rail, #9),
M2 slice B (forge + checkpoint adapters, #10)
**Companion to:** `2026-06-16-circuit-m2-slice-b-forge-checkpoint-design.md` (§9.1 parks this work)

---

## 1. Goal

Let a session be **archived** (retired) — normally after it reaches `Done`, or to **abandon**
an un-merged session — and **unarchived** (restored) again. Archival is a session-status
change on **Axis 2**, orthogonal to the flow stage (Axis 1): it is **not** a flow stage and
**not** a review verdict. This is the immediate follow-up slice B §9.1 recorded.

This slice closes the lifecycle write side: a session can be taken out of active rotation
(freeing its git worktree, signalling its agent it may stop) and brought back, without ever
silently destroying committed work.

## 2. The two axes (recap)

A session carries two independent properties:

- **Axis 1 — Flow stage** (`Draft → Project → Implement → Review → Merge → Done`): *where is the
  work?* Derived, never stored (`derive_stage`). **Untouched by this slice.**
- **Axis 2 — Session status** (`active | archived`): *is this session still live?* Authored on
  `SessionRecord`. **This slice adds it.** Cancellation/abandonment is an Axis-2 status change,
  which is why slice B deliberately gave the checkpoint review states no "cancelled"/"closed"
  value, and why this slice **subsumes the earlier "Abandoned" terminal-stage idea** — there is
  no `Stage::Abandoned`; a cancelled session is an `active`-stage session flipped to `archived`.

## 3. Prior-art grounding (why this shape)

Researched against git-worktree agent orchestrators (Conductor, Crystal, vibe-kanban,
claude-squad, container-use, uzi) and agent CLIs / infra (Cursor, Devin, OpenHands, Codex,
Claude Code; Kubernetes, systemd, Sidekiq). The prevailing convention, which this design
follows:

1. **Archive = a soft, reversible status flip; the record is kept, not deleted.** Conductor,
   Crystal, vibe-kanban, claude-squad `Pause`, Cursor, and Devin all keep the record + branch
   and allow restore/unarchive. Hard-delete-on-end is the exception (lean CLIs only).
2. **The branch survives by default; deleting it is a deliberate, separate, opt-in choice.**
   `git worktree remove` never touches the branch. vibe-kanban/Crystal preserve the branch
   even on hard delete ("your code is safe").
3. **The durable status field IS the machine-readable "session ended" signal.** Cursor
   (`FINISHED`/`CANCELLED` polled via API), Devin (`status`), OpenHands (`idle/running/paused`),
   Sidekiq (Redis state), Kubernetes (`deletionTimestamp` reconciled by the kubelet) all make
   lifecycle observable through *persisted state a supervisor reads* — never via exit codes,
   signals, or sentinel files between peers. We do **not** invent an IPC/exit-code protocol.
4. **Stop the agent before tearing down its worktree.** Source-verified tools (uzi,
   claude-squad) kill/detach the agent session *first*, then run git teardown. Kubernetes makes
   this a structural invariant (finalizers + grace period). See §6.3 for how we approximate it.
5. **Archive is idempotent** (Cursor makes this an explicit guarantee): re-archiving an
   already-archived session is a no-op success, not an error.

## 4. Data model: `SessionStatus` + `status` field

`src/session/mod.rs`. Additive; old records keep parsing.

```rust
/// Axis-2 lifecycle status. Serializes lowercase (`"active" | "archived"`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    #[default]
    Active,
    Archived,
}

pub const SCHEMA_VERSION: u32 = 2;   // bumped from 1 (slice 0)

pub struct SessionRecord {
    pub schema_version: u32,
    pub id: SessionId,
    pub kind: SessionKind,
    // ... existing optional fields ...
    /// Axis-2 lifecycle. `#[serde(default)]` => a v1 record with no `status`
    /// key parses as `Active`, so slice-0/A/B records load unchanged.
    #[serde(default)]
    pub status: SessionStatus,
}
```

- All three constructors (`spec`/`impl_`/`fix`) stamp `schema_version: SCHEMA_VERSION` and
  `status: SessionStatus::Active`.
- Helpers (pure field operations, no IO): `is_archived(&self) -> bool`,
  `archive(&mut self)`, `unarchive(&mut self)`. `archive`/`unarchive` also normalize
  `schema_version = SCHEMA_VERSION` — a record that now carries a `status` field *is* v2, so
  any write that touches status keeps the on-disk version coherent with the shape.
- **Schema bump rationale.** `schema_version` is stored, not validated, in M2; the bump is
  documentary — it records that the on-disk shape grew a field. Back-compat is provided by
  `#[serde(default)]`, not by version-gated parsing. A v1 file (no `status`) loads as `Active`;
  the first `archive`/`unarchive` that re-saves it normalizes `schema_version` to 2 (§4 helpers).

### 4.1 Tests (`session/mod.rs`)

- `active` and `archived` records round-trip through TOML.
- A **hand-authored v1 record with no `status` key** parses as `Active` (back-compat guard).
- `status` serializes as the lowercase string `"active"` / `"archived"`.
- `archive()`/`unarchive()` flip the field, set `schema_version = 2`, and `is_archived()`
  reflects the result (incl. a v1-loaded record gaining `schema_version = 2` after `archive()`).

## 5. Git port extension

Worktree teardown and (opt-in) branch deletion need two new `GitPort` methods. Additive to the
trait — the exhaustive nature forces `FakeGit` (ports tests) to implement them, the intended
forcing function (mirrors slice B's `ReviewState` extension).

`src/ports.rs`:

```rust
pub trait GitPort {
    type Error: std::error::Error + Send + Sync + 'static;
    // ... existing methods ...
    /// `git worktree remove [--force] <path>`. `force` removes a dirty/locked worktree.
    fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), Self::Error>;
    /// `git branch -d|-D <branch>`. `force` (`-D`) deletes an un-merged branch.
    fn delete_branch(&self, branch: &str, force: bool) -> Result<(), Self::Error>;
}
```

`src/adapters/git.rs` — thin wrappers over the existing `run` helper:

- `remove_worktree`: `["worktree", "remove", path, ...("--force" if force)]`.
- `delete_branch`: `["branch", if force {"-D"} else {"-d"}, branch]`.

The **worktree path is located, never stored** (it is machine-local, §6 of the foundation):
the CLI calls `list_worktrees()` and finds the entry whose `branch == record.branch`, reusing
the existing `parse_worktree_porcelain`. No path is persisted or re-derived from config for
teardown.

### 5.1 Tests (`adapters/git.rs`)

- `remove_worktree` removes a clean added worktree (dir gone, branch still exists afterward).
- `remove_worktree` on a **dirty** worktree fails without `force`, succeeds with `force`.
- `delete_branch` deletes a merged branch with `-d`; **refuses an un-merged branch** without
  `force`; deletes it with `force` (`-D`).
- `FakeGit` in `ports.rs` tests gains both methods (returning `Ok(())`).

## 6. `circuit session archive <id> [--delete-branch] [--force]`

`src/main.rs`, new `SessionCommand::Archive` variant.

### 6.1 Flags

- `--delete-branch`: also delete the session's branch (default: keep it — §3.2).
- `--force`: (a) remove a dirty/locked worktree, and (b) delete an un-merged branch. One flag
  governs both destructive escalations, for consistency.

### 6.2 Order of operations

Teardown happens **before** the status flip, so a failed teardown leaves `status` truthfully
`active` (the session is not yet retired). Re-running after fixing the cause is safe.

```
1. Resolve session (reuse resolve_session: ULID, else unique DAG-node name).
2. Idempotent guard: already archived -> print "<id> already archived", exit 0. No teardown.
3. Worktree teardown:
     locate via git.list_worktrees() where branch == record.branch
     if found -> git.remove_worktree(path, force)
                 (on failure without --force, bail with an actionable
                  "worktree has changes or is locked — pass --force" hint)
     if absent (spec session, or already removed) -> skip, no error
4. If --delete-branch and record.branch.is_some():
     git.delete_branch(branch, force)   // un-merged requires --force (-D)
5. record.archive(); ws.save_session(&record)   // the durable agent-stop signal
6. Print: archived line + branch disposition + "agent session may now end."
```

### 6.3 Stop-agent-before-teardown (the finalizer analog)

Circuit is a stateless CLI with no running supervisor in M2, so we cannot enforce a
Kubernetes-style finalizer/grace-period. The lightweight guard we **do** get for free: a live
agent dirties its worktree, so a plain `circuit session archive` (no `--force`) will **refuse**
at step 3 while an agent is actively working there. That converts "stop the agent first" from
prose etiquette into an actual gate — the operator must either stop the agent (clean worktree
→ removal succeeds) or consciously pass `--force` to discard live work.

**Contract:** `archive` assumes the agent has already stopped (or is being deliberately killed
via `--force`). *Actively* reaping a live agent process belongs to whatever supervises tau /
Claude Code; that supervisor reads the `status = archived` field and reaps. Building that
supervisor is out of scope for this slice.

## 7. `circuit session unarchive <id>`

`src/main.rs`, new `SessionCommand::Unarchive` variant. Restores a session to active rotation.

```
1. Resolve session.
2. Idempotent guard: already active -> print "<id> not archived", exit 0.
3. record.unarchive(); ws.save_session(&record).
4. Worktree rehydrate:
     if record.branch is Some AND the branch still exists (git.branch_facts(..).exists):
         resolve worktree dir (CIRCUIT_WORKTREES_DIR / local.toml, mirroring spawn)
         git.add_worktree(branch, path)
         print restored worktree path
     else if branch was deleted (--delete-branch on archive):
         status is flipped to active, but print a WARNING that the branch is gone so
         no worktree was recreated — the session derives Draft honestly (branch !exists).
     spec sessions (no branch): just the status flip, no worktree.
```

Rehydrate mirrors `spawn`'s worktree path resolution exactly (`resolve_worktree_dir` +
`CIRCUIT_WORKTREES_DIR`), so a restored session lands where a fresh one would.

## 8. Flow filtering

`src/main.rs` `run_flow` + `src/flow/rail.rs`.

- `circuit flow` gains `--all`. The **no-selector list hides archived sessions by default**;
  `--all` includes them. An **explicit selector always shows the named session** regardless of
  status (if you ask for an archived session by id, you want to see it).
- `render_rail` gains an `archived: bool` parameter. Archived sessions render an `(archived)`
  marker on line 1 (after the kind tag), colorless. The single call site in `run_flow` and the
  rail unit tests are updated.

## 9. Error handling

- New `GitPort` methods reuse `GitError` (no new error type needed).
- `main.rs` uses `anyhow` context as today. A worktree-teardown failure without `--force`
  produces a clear, actionable bail; it does not corrupt state (status not yet flipped).
- `forbid(unsafe_code)` holds; no new `unsafe`.

## 10. Out of scope (unchanged from slice B §9, plus)

- A running supervisor / agent-process reaper (§6.3) — this slice only flips the durable signal.
- Hard delete of a session record (archival is the soft-retire path; we never delete records).
- `delivery` config override, forge write CLI verbs, mode-aware local wording (slice B §9.2–9.4).
- Any change to `derive_stage` / the six-stage spine (Axis 1 is untouched).

## 11. File plan

| File | Change |
|---|---|
| `src/session/mod.rs` | `SessionStatus` enum, `status` field (`#[serde(default)]`), `SCHEMA_VERSION = 2`, `is_archived`/`archive`/`unarchive`, tests |
| `src/ports.rs` | `GitPort::remove_worktree` + `delete_branch`; `FakeGit` impls |
| `src/adapters/git.rs` | impl both methods + unit tests |
| `src/main.rs` | `SessionCommand::{Archive, Unarchive}`; `--all` on `Flow`; archived filtering; worktree-locate-by-branch |
| `src/flow/rail.rs` | `archived: bool` param + marker + test |
| `tests/session_flow.rs` | integration: archive frees worktree + flips status; flow hides archived / `--all` shows; unarchive rehydrates; `--delete-branch`; dirty worktree refused without `--force` |

## 12. Exit criteria

- `circuit session archive <id>` on a Done (or any) session flips `status = archived` and
  removes its worktree; the branch is kept by default and deleted only with `--delete-branch`.
- A dirty worktree is refused without `--force`; `--force` removes it. An un-merged branch is
  refused without `--force`; `--force` deletes it.
- Archiving an already-archived session is a no-op success (idempotent).
- `circuit session unarchive <id>` flips back to `active` and re-adds the worktree from the
  kept branch (or warns when the branch was deleted).
- `circuit flow` hides archived sessions by default; `--all` shows them with an `(archived)`
  marker; an explicit selector shows an archived session regardless.
- A hand-authored v1 `SessionRecord` (no `status`) loads as `Active`.
- Every new mapping/method has a unit test; the integration tests pass; the crate builds with
  `forbid(unsafe_code)`.
