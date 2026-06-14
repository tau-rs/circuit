# Circuit M2 — Slice B: Forge + Checkpoints + PR Actions — Design

**Status:** Approved for implementation
**Date:** 2026-06-14
**Type:** Implementation-slice design (M2, slice B)
**Builds on:** `2026-06-14-circuit-m2-session-model-design.md` (the M2 design — §6 ports, §5.2/§5.3 facts, §3.2 checkpoint schema, §7 actions) and the merged foundation slice (`ports.rs`, `flow/facts.rs`, `session/`, `model/store.rs`).

---

## 1. Scope

Implement the two review-state backends and the PR automation actions that the
foundation declared as port traits:

- **`adapters/forge.rs`** — `ForgePort` by shelling out to the GitHub `gh` CLI:
  `review_state` + actions `create_pr` / `merge` / `update_from_base`.
- **`adapters/checkpoints.rs`** — `CheckpointStore`, the no-remote substitute,
  reading `.circuit/checkpoints/<session>.toml` and mapping to the **same**
  `ReviewState` (`self-review→Open`, `accepted→Merged`, `archived→Closed`).
- **CLI**: `circuit pr create|merge|update-from-base <session>` (gh actions) and
  `circuit checkpoint <session> --state <self-review|accepted|archived>` (local).

**Out of scope** (other slices): git adapter, `session spawn`, the stage machine
(`derive_stage`, already merged), flow rail / DAG board renderers, health rollup,
and wiring `review_state` into `DeliveryFacts`/the rail. This slice produces the
backends and the action commands; consumption lives elsewhere.

The signatures in `ports.rs` and `flow/facts.rs` are **fixed** — this slice
implements against them and does not change them.

---

## 2. Module layout (all additive)

```
src/
  adapters/
    mod.rs          # pub mod forge; pub mod checkpoints;
    forge.rs        # GhForge<R: CommandRunner> : ForgePort  + ForgeError
    checkpoints.rs  # FsCheckpointStore : CheckpointStore + CheckpointRecord + CheckpointError
  app.rs            # port-generic orchestration (pr_*, write_checkpoint); anyhow internally
  model/store.rs    # + checkpoints_dir() / checkpoint_path()  (mirrors sessions_dir)
  lib.rs            # + pub mod adapters; pub mod app;
  main.rs           # + Pr / Checkpoint subcommands -> delegate to app.rs
Cargo.toml          # + serde_json (parse `gh --json` output)
```

`#![forbid(unsafe_code)]` continues. `thiserror` at adapter boundaries
(`ForgeError`, `CheckpointError`); `anyhow` inside `app.rs` and `main.rs`.

---

## 3. Forge adapter (`adapters/forge.rs`)

### 3.1 Testable shell-out seam

```rust
pub trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<std::process::Output>;
}

pub struct SystemRunner;            // production: std::process::Command
pub struct GhForge<R = SystemRunner> { runner: R }
```

`GhForge::new()` uses `SystemRunner`; tests use `GhForge::with_runner(FakeRunner)`.
The runner is the seam that makes JSON parsing, `ReviewState` mapping, and exact
argument construction unit-testable **offline**. This is hexagonal: the bug-prone
pure logic (parse + map) is isolated from the process spawn.

### 3.2 `review_state(branch)`

Runs `gh pr list --head <branch> --state all --json number,state,reviewDecision`.

| gh result | `ReviewState` |
|---|---|
| `[]` (exit 0, no PR) | `Ok(None)` — a *known* no-PR (§5.3 distinct from undeterminable) |
| first PR `state == "MERGED"` | `Ok(Merged)` |
| `state == "CLOSED"` | `Ok(Closed)` |
| `state == "OPEN"` + `reviewDecision == "APPROVED"` | `Ok(Approved)` |
| `state == "OPEN"` (otherwise) | `Ok(Open)` |
| non-zero exit (auth/network) | `Err(ForgeError::Command{..})` |

The empty-array signal (exit 0) cleanly separates "no PR" from "forge
unreachable" without fragile stderr matching. The caller maps `Err` to
`DeliveryFacts.review = None` (undeterminable); `Ok(ReviewState::None)` is a known
fact. If multiple PRs share the head, the first (gh's default newest-first) wins.

### 3.3 Actions

| Method | gh invocation |
|---|---|
| `create_pr(branch, base, title, body)` | `gh pr create --head <branch> --base <base> --title <title> --body <body>` |
| `merge(branch)` | `gh pr merge <branch> --merge` (non-interactive strategy) |
| `update_from_base(branch, _base)` | `gh pr update-branch <branch>` (gh updates the PR head from its base) |

Each maps a non-zero exit to `ForgeError::Command { args, status, stderr }`. A
spawn failure (gh missing) maps to `ForgeError::Spawn { program, source }`.

### 3.4 `ForgeError` (thiserror)

```rust
#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("failed to launch {program}: {source}")]
    Spawn { program: String, #[source] source: std::io::Error },
    #[error("`gh {args}` failed (status {status}): {stderr}")]
    Command { args: String, status: String, stderr: String },
    #[error("failed to parse gh output: {source}")]
    Parse { #[source] source: serde_json::Error },
}
```

---

## 4. Checkpoint store (`adapters/checkpoints.rs`)

### 4.1 Schema (§3.2) and storage model

One file per session, current-state (**Model B**): `.circuit/checkpoints/<session-id>.toml`,
overwritten on each `circuit checkpoint`. No history, no clock read — review_state
is keyed by session, so this maps directly. (The M2 design's dated-filename example
is a log/Model-A illustration; M2 consumes only the *current* state, so Model B is
the YAGNI choice and keeps the foundation's "isolate the one clock read" discipline.)

```rust
#[derive(Serialize, Deserialize, ...)]
pub struct CheckpointRecord {
    pub schema_version: u32,         // 1
    pub session: String,             // session id (== filename stem; self-describing)
    pub commit: String,              // sha (authored by the caller)
    pub state: CheckpointState,      // self-review | accepted | archived
    #[serde(default)] pub note: Option<String>,
}

#[serde(rename_all = "kebab-case")]
pub enum CheckpointState { SelfReview, Accepted, Archived }
```

### 4.2 `CheckpointStore::review_state(session)`

Load `.circuit/checkpoints/<session>.toml`. Absent file → `Ok(ReviewState::None)`
(the trait's documented contract). Present → map:

| `CheckpointState` | `ReviewState` |
|---|---|
| `SelfReview` | `Open` |
| `Accepted` | `Merged` |
| `Archived` | `Closed` |

### 4.3 `CheckpointError` (thiserror)

A local `thiserror` enum with `Io { path, source }` and `Parse { path, source }`
variants (mirroring `model::ModelError`, kept local so the adapter owns its failure
modes per the port's associated-`Error` design). A missing file is **not** an
error — it is `Ok(ReviewState::None)`.

---

## 5. Application layer (`app.rs`) — port-generic orchestration

Thin functions, generic over the ports, returning `anyhow::Result<()>`. This is
where `<session>` is resolved to a branch and the port is called — the unit under
test with a **fake `ForgePort`**.

```rust
pub fn pr_create<F: ForgePort>(ws: &Workspace, forge: &F, session: &str, title: Option<String>, body: Option<String>) -> Result<()>;
pub fn pr_merge<F: ForgePort>(ws: &Workspace, forge: &F, session: &str) -> Result<()>;
pub fn pr_update_from_base<F: ForgePort>(ws: &Workspace, forge: &F, session: &str) -> Result<()>;
pub fn write_checkpoint(ws: &Workspace, session: &str, state: CheckpointState, commit: String, note: Option<String>) -> Result<()>;
```

**Session → branch resolution:** `ws.load_session(session)` → `SessionRecord.branch`.
`None` → error "session <id> has no branch yet". Missing session file → error
"no such session". `base` comes from `ws.load_config().base_branch`.

**PR title/body for `pr_create`:** default to the session's DAG node — load
`session.dag_node` → `DagNode { title, intent }` → title/body. `--title`/`--body`
flags override. (Authored intent becomes the PR; no new prose invented.)

`main.rs` `run_pr`/`run_checkpoint` construct `GhForge::new()` / read the commit
sha argument and delegate. The `pr` commands always use the real forge; the
`checkpoint` command always writes the local store — they are alternatives
(remote vs no-remote), not an auto-fallback.

---

## 6. CLI surface (`main.rs`, additive)

```
circuit pr create <session> [--title <t>] [--body <b>] [--path <p>]
circuit pr merge <session> [--path <p>]
circuit pr update-from-base <session> [--path <p>]
circuit checkpoint <session> --state <self-review|accepted|archived> --commit <sha> [--note <n>] [--path <p>]
```

All require an initialized workspace (`require_initialized`). `--path` defaults to
`.` for parity with the existing commands.

---

## 7. Testing strategy

- **`forge.rs` (unit, offline, `FakeRunner`):** all five `review_state` outcomes
  incl. `[]→None`; non-zero exit → `Err`; exact arg vectors for
  `create_pr`/`merge`/`update_from_base`; parse error path.
- **`checkpoints.rs` (unit, tempdir):** each `CheckpointState`→`ReviewState`
  round-trip via written files; absent file → `Ok(None)`; TOML round-trip.
- **`app.rs` (unit, `FakeForge` + tempdir `Workspace`):** session→branch
  resolution; no-branch error; missing-session error; `pr_create` derives
  title/body from the DAG node; `write_checkpoint` writes the expected file.
  *This is the brief's "exercise logic through a fake ForgePort."*
- **e2e (`assert_cmd`, offline):** `circuit checkpoint` happy path (file written,
  content asserted); `circuit pr create` failure paths that never reach gh
  (missing session, branchless session) → non-zero exit + clear stderr.
- **real-`gh` smoke (1 test):** checks `gh --version`; **skipped with `eprintln!`
  when absent** (logged, never silently passing); when present, exercises the real
  `SystemRunner` path of `review_state` and asserts a graceful `Result` (no panic).

No network or real GitHub in the suite.

---

## 8. Traceability

| M2 design § | This slice |
|---|---|
| §6 `ForgePort` / `CheckpointStore` shell-out behind ports | §3, §4 |
| §5.3 determinism honesty (known `None` vs undeterminable `Err`) | §3.2 |
| §3.2 checkpoint schema, same `ReviewState` either backend | §4 |
| §7 automation actions; checkpoints as synthetic PRs (local strategy) | §3.3, §4, §5 |
| §13 fake-forge domain tests + skipped real-`gh` smoke | §7 |
