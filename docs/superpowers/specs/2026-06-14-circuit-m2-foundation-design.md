# Circuit M2 — Foundation (slice 0) — Design

**Status:** Draft for review
**Date:** 2026-06-14
**Type:** Slice design (M2, slice 0 of 4)
**Parent design:** `2026-06-14-circuit-m2-session-model-design.md` (source of truth — §4 sessions, §5 stage machine, §6 ports, §9 health)
**Builds on:** M2a (`2026-06-14-circuit-m2a-data-model-dag.md`, PR #4) — the `.circuit/` model, `Workspace`, and DAG validation. Mirrors its TDD style.

---

## 1. Goal

Deliver the **shared contracts** every other M2 slice (git adapter, forge/checkpoint adapter,
spawn/renderers/CLI) depends on: the session record schema + identity, the delivery-facts
data, the pure stage machine, the health rollup, and the IO port traits. Everything here is
**pure and fully unit-tested with data literals — no external IO** (no git, no `gh`, no
network). The adapters and CLI that *use* these contracts are out of scope (slices A/B/C).

This is the foundation: get the types and the one pure function (`derive_stage`) exactly right
so the slices built on top never have to revise a contract.

---

## 2. Scope

**In scope (this slice):**
- `src/session/` — `SessionId` (ULID-style identity), `SessionKind`, `SessionRecord` serde schema.
- `src/flow/facts.rs` — `BranchFacts`, `ReviewState`, `DeliveryFacts` (plain data).
- `src/flow/stage.rs` — `Stage`, `StageView`, `derive_stage` (pure, exhaustively tested).
- `src/cockpit/health.rs` — `Health` (with `Ord`), `SessionHealth::rollup`, `rollup_children`.
- `src/ports.rs` — `GitPort`, `ForgePort`, `CheckpointStore` traits — **signatures only**.
- `src/model/store.rs` — `Workspace` session methods, mirroring `list_dag_nodes`.

**Out of scope (other slices, noted for forward-compat):**
- Any `GitPort` / `ForgePort` / `CheckpointStore` *implementation*; the `git` / `gh` shell-out.
- `circuit session spawn`, worktree orchestration, the flow rail / DAG-board renderers, CLI commands.
- Computing `DeliveryFacts` from a real repo (that is the git adapter, slice A).

The module skeletons are created here; the adapters/CLI fill them in later slices.

---

## 3. Session domain (`src/session/`)

### 3.1 Identity — `SessionId`

A session's `id` is **ULID-style, stable, and precedes the branch** (§4 of the parent design):
a session exists at `Draft` before any branch is cut, which is why the branch name cannot be
the identity.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(ulid::Ulid);

impl SessionId {
    /// Mint a fresh id. This is the ONLY clock-reading call in the foundation slice;
    /// the impurity is isolated here so everything else stays pure and deterministic.
    pub fn generate() -> Self;
}
// + FromStr (validates Crockford base32), Display (canonical 26-char form).
```

**Crate choice:** the `ulid` crate (`features = ["serde"]`). It is the honest reading of
"ULID-style," gives lexicographically-sortable ids, and keeps the only impurity (`Ulid::new()`
reads the wall clock) to a single function. The *type*, serde round-trip, and `from_str`
validation — the surface every other slice depends on — are pure and exhaustively unit-tested.
`uuid` v7 was rejected (it is a 128-bit hex UUID, not a ULID, contradicting the spec's `01J…`
example ids); hand-rolling was rejected (re-implementing a published format is needless bug
surface, against the "host existing ecosystems" stance).

DAG-node and spec ids stay plain `String` (they are human-authored kebab slugs — a different
kind of identifier). Only the session's own `id` is a `SessionId`.

### 3.2 `SessionKind`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind { Spec, Impl, Fix }   // serializes as "spec" | "impl" | "fix"
```

### 3.3 `SessionRecord` — `.circuit/sessions/<id>.toml`

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub schema_version: u32,                          // = 1
    pub id: SessionId,
    pub kind: SessionKind,
    #[serde(default)] pub parent: Option<String>,          // spec id (impl/fix); None for spec session
    #[serde(default)] pub dag_node: Option<String>,        // node id (impl/fix); None for spec session
    #[serde(default)] pub branch: Option<String>,          // authored bridge; None until spawned (Draft)
    #[serde(default)] pub fixes_indicator: Option<String>, // fix sessions: the sub-indicator targeted
}
```

Constructors mirror M2a's `SpecRecord::new` / `DagNode::new` style:

```rust
impl SessionRecord {
    pub fn spec(id: SessionId) -> Self;                                  // parent/dag_node/branch = None
    pub fn impl_(id, parent, dag_node, branch) -> Self;                 // kind = Impl
    pub fn fix(id, parent, dag_node, branch, fixes_indicator) -> Self;  // kind = Fix
}
```

**Why `branch` is `Option`:** identity precedes the branch. A spec session never owns a branch
(it writes no application code); a Draft impl/fix session exists before its branch is cut. The
field records *authored intent* (the branch name), never derived state. Whether that branch
actually exists **in git** is a separate, derived fact (`BranchFacts.exists`, §4) — the two are
deliberately distinct: a session can name a branch that git has not yet created (→ `Draft`).

Only **intent** is stored (§3.3 of the parent design). No stage, no worktree path, no branch
*state* is ever written.

---

## 4. Delivery facts (`src/flow/facts.rs`) — plain data, no IO

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchFacts {
    pub exists: bool,                  // branch ref present in git
    pub commits_ahead_of_base: usize,  // git rev-list base..branch (rail decoration)
    pub has_substantive_changes: bool, // non-empty diff vs merge-base (the Project→Implement gate)
    pub merged_into_base: bool,        // branch tip is ancestor of base (git-only, offline)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState { None, Open, Approved, Merged, Closed }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeliveryFacts {
    pub branch: BranchFacts,
    pub review: Option<ReviewState>,   // None ⇒ forge unreachable AND no checkpoint (undeterminable)
}
```

**`review: Option<ReviewState>` is the determinism-honesty primitive (§5.3).** `ReviewState::None`
is a *known* fact ("no PR exists"); `Option::None` means "the forge was unreachable and there is
no checkpoint, so review state is **undeterminable**." Conflating the two would let `derive_stage`
fake a verdict. The alternative — a `ReviewState::Unknown` variant — was rejected: it pollutes a
clean forge-state enum with an adapter concern and forces every match to handle it.

These are populated by the adapters (slices A/B); the domain consumes them as literals.

---

## 5. Stage machine (`src/flow/stage.rs`) — pure derivation

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage { Draft, Project, Implement, Review, Merge, Done }   // NO Unknown variant

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageView {
    pub stage: Stage,         // best git-determinable stage (the "git floor")
    pub forge_certain: bool,  // false ⇒ Review/Merge/Done refinement could NOT be confirmed
}

pub fn derive_stage(session: &SessionRecord, facts: &DeliveryFacts) -> StageView;
```

Stage uncertainty lives **outside** the `Stage` enum — in `StageView.forge_certain` — so `Stage`
stays the clean six-stage spine. `forge_certain == false` is the honest "it's *at least* this
stage; I cannot confirm the forge-gated refinement" signal (§5.3). We never fake
Review/Merge/Done.

`forge_certain` is a `bool`, not a richer `enum Confidence`: there is exactly one source of
forge-gated uncertainty in M2 (review undeterminable), so a "why" field would be a single-variant
enum today. When M3 adds projection-gated refinement, that is the moment to widen it — one
mechanical change the compiler will flag at every call site.

### 5.1 Truth table

First matching row wins. The stage gate keys on `has_substantive_changes` (the §5.1
Project→Implement boundary), **not** `commits_ahead_of_base` (that is rail decoration only).

| # | Condition | → `stage` | `forge_certain` |
|---|---|---|---|
| 1 | `!branch.exists` | `Draft` | `true` (git-settled) |
| 2 | `branch.merged_into_base` | `Done` | `true` (git-only, offline-confident §5.3) |
| 3 | `!branch.has_substantive_changes` | `Project` | `true` |
| 4 | substantive & not merged, `review == None` | `Implement` | **`false`** ← the one honest-Unknown case |
| 5 | … `Some(ReviewState::None)` | `Implement` | `true` |
| 6 | … `Some(Open)` | `Review` | `true` |
| 7 | … `Some(Approved)` | `Merge` | `true` |
| 8 | … `Some(Merged)` | `Done` | `true` |
| 9 | … `Some(Closed)` | `Implement` | `true` |

Row 2 (git-only `merged_into_base`) precedes the forge rows: a merged branch is `Done` with
certainty even offline. Row 9 (`Closed` = PR closed without merging) reads as abandoned review →
back to `Implement`, and we are *certain* because the forge told us (not Unknown).

The `session` parameter is **reserved**: §5.1 of the parent design notes that M3's
projection-approved marker will refine the Project→Implement gate using session/projection state.
In M2 it is unused; keeping it in the signature now avoids a breaking change later.

### 5.2 Testing

Every row above is a unit test built from `DeliveryFacts`/`BranchFacts` literals — no git, no
forge, exactly like M1's indicator tests. This is the bulk of the slice's test value.

---

## 6. Health rollup (`src/cockpit/health.rs`)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health { Sound, Warn, Critical, Unknown }   // declaration order = ascending ⇒ Unknown highest

pub struct SessionHealth { pub cycles: usize, pub dep_violations: usize }

impl SessionHealth {
    /// Impl session: any cycle or dependency-rule violation ⇒ Critical, else Sound (§9).
    pub fn rollup(&self) -> Health;     // yields only Sound | Critical
}

/// Spec session: worst-of-children = children.max() (§9, §6.7 vision).
/// Empty ⇒ Sound; an Unknown child dominates (Unknown is the max).
pub fn rollup_children(children: &[Health]) -> Health;
```

`#[derive(Ord)]` over the declaration order `Sound < Warn < Critical < Unknown` gives `Unknown`
the highest sort, so `children.max()` lets an unmeasurable child dominate — we never report a
green spec when a child was unmeasurable (§9).

`Warn` is **reserved** (no advisory-only sub-indicator produces it yet) but present so the
ordering is complete. `Unknown` (a child with no worktree) is supplied *by the adapter slice* as
an input to `rollup_children`; it is not minted here — `rollup` itself only yields Sound/Critical.

---

## 7. Ports (`src/ports.rs`) — trait signatures only, NO implementations

```rust
pub struct Worktree { pub path: PathBuf, pub branch: Option<String> }

pub trait GitPort {
    type Error: std::error::Error + Send + Sync + 'static;
    fn branch_facts(&self, branch: &str, base: &str) -> Result<BranchFacts, Self::Error>;
    fn create_branch(&self, branch: &str, base: &str) -> Result<(), Self::Error>;
    fn add_worktree(&self, branch: &str, path: &Path) -> Result<(), Self::Error>;
    fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error>;
}

pub trait ForgePort {
    type Error: std::error::Error + Send + Sync + 'static;
    fn review_state(&self, branch: &str) -> Result<ReviewState, Self::Error>; // Err ⇒ caller sets review = None
    fn create_pr(&self, branch: &str, base: &str, title: &str, body: &str) -> Result<(), Self::Error>;
    fn merge(&self, branch: &str) -> Result<(), Self::Error>;
    fn update_from_base(&self, branch: &str, base: &str) -> Result<(), Self::Error>;
}

pub trait CheckpointStore {
    type Error: std::error::Error + Send + Sync + 'static;
    fn review_state(&self, session: &str) -> Result<ReviewState, Self::Error>; // no checkpoint ⇒ Ok(ReviewState::None)
}
```

**Associated `Error` types** (rather than a concrete `PortError` enum) keep this a true
contracts-only slice: we do not yet know the adapters' failure modes, and inventing variants now
would be a speculative edit. Each adapter (slices A/B) brings its own `thiserror` type bounded by
`std::error::Error + Send + Sync + 'static`. `thiserror` lives at that boundary, per the repo
convention; the foundation declares the boundary without crossing it.

**Honesty wiring (documented, enforced by callers in later slices):** `ForgePort::review_state`
returns `Result<ReviewState, _>` — the forge *knows* its state; an `Err` (unreachable) is mapped
by the caller into `DeliveryFacts.review = None` (undeterminable). `CheckpointStore::review_state`
returns `Ok(ReviewState::None)` when no checkpoint exists (a known "no review"), reserving `Err`
for store-read failure.

**Testing:** the only test a contracts module gets is a tiny `#[cfg(test)]` fake impl of each
trait, proving the signatures are actually implementable and usable.

---

## 8. Persistence (`src/model/store.rs`) — `Workspace` session methods

Extend the existing `Workspace` (M2a) with session paths and typed load/save/list, mirroring
`dag_node_path` / `load_dag_node` / `save_dag_node` / `list_dag_nodes` exactly:

```rust
impl Workspace {
    pub fn sessions_dir(&self) -> PathBuf;                 // .circuit/sessions
    pub fn session_path(&self, id: &str) -> PathBuf;       // .circuit/sessions/<id>.toml
    pub fn load_session(&self, id: &str) -> Result<SessionRecord, ModelError>;
    pub fn save_session(&self, s: &SessionRecord) -> Result<(), ModelError>;  // uses s.id (Display)
    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>, ModelError>;    // sorted by path
}
```

`store.rs` imports `crate::session::SessionRecord` — a clean inward dependency; `session` never
imports `store`. No new error variants: the existing `ModelError` (Io/Parse/Serialize) covers it.

---

## 9. Wiring

- `Cargo.toml`: add `ulid = { version = "1", features = ["serde"] }`. (`serde`, `toml`,
  `thiserror`, `tempfile` already present from M2a.)
- `src/lib.rs`: add `pub mod cockpit; pub mod flow; pub mod ports; pub mod session;`
  (alphabetical: builder, cockpit, dag, flow, graph, indicators, lang, layer, model, ports,
  render, session).
- `src/flow/mod.rs` declares `pub mod facts; pub mod stage;`; `src/cockpit/mod.rs` declares
  `pub mod health;`.
- `#![forbid(unsafe_code)]` is inherited crate-wide; no `unsafe` is introduced.

---

## 10. Module / dependency layout (hexagonal — dependencies point inward)

```
src/
  session/mod.rs      # SessionId, SessionKind, SessionRecord  (pure)
  flow/
    mod.rs
    facts.rs          # BranchFacts, ReviewState, DeliveryFacts  (pure data)
    stage.rs          # Stage, StageView, derive_stage           (pure fn)  → depends on facts + session
  cockpit/
    mod.rs
    health.rs         # Health, SessionHealth, rollup_children   (pure)
  ports.rs            # GitPort, ForgePort, CheckpointStore traits (signatures) → depends on flow::facts
  model/store.rs      # Workspace session methods (IO)            → depends on session
```

No module here depends on an adapter or the CLI. `ports.rs` references only the pure
`flow::facts` types. `store.rs` is the single IO boundary, and it only does filesystem reads/writes
(M2a's `load_toml`/`save_toml`), no git/forge.

---

## 11. Execution plan (TDD, mirroring M2a PR #4)

Seven tasks, two-stage review each:

1. **Scaffold** — add `ulid` dep; declare modules in `lib.rs`, `flow/mod.rs`, `cockpit/mod.rs`;
   stub files so the crate compiles.
2. **`session/`** — `SessionId` (+ generate/FromStr/Display), `SessionKind`, `SessionRecord`
   (serde round-trip, hand-authored parse, id round-trip). *Parallel-eligible.*
3. **`flow/facts.rs`** — the three data types (round-trip / Default). *Parallel-eligible.*
4. **`cockpit/health.rs`** — `Health` Ord, `rollup`, `rollup_children` (max / empty / Unknown
   dominates). *Parallel-eligible.*
5. **`flow/stage.rs`** — `Stage`, `StageView`, `derive_stage` + the exhaustive truth-table tests.
   Depends on Tasks 2 + 3.
6. **`ports.rs`** — traits + `Worktree` + fake-impl tests. Depends on Task 3.
7. **`Workspace` session methods** in `model/store.rs` (round-trip + sorted-list tests).
   Depends on Task 2.

Branch `m2-foundation`; PR base `main`.

---

## 12. Testing strategy

- **Pure domain** (`session`, `flow::facts`, `flow::stage`, `cockpit::health`): unit tests with
  data literals — no IO. `derive_stage` is exhaustive over the truth table; `Health` ordering and
  both rollups are explicit.
- **`ports`**: compile-only fake impls proving the contracts are implementable.
- **`Workspace` session methods**: round-trip through a `tempfile::tempdir()`, plus a
  sorted-`list_sessions` test — mirroring M2a's `store.rs` tests.
- No git, no `gh`, no network anywhere in this slice.

---

## 13. Traceability to the parent design

| Parent § | Foundation coverage |
|---|---|
| §4 sessions (3 kinds, identity precedes branch) | §3 (`SessionId`, `SessionKind`, `SessionRecord`) |
| §5.1 stage mapping | §5.1 truth table |
| §5.2 facts (plain data, no IO) | §4 |
| §5.3 determinism honesty (Unknown, no faked verdicts) | §4 (`review: Option`), §5 (`forge_certain`) |
| §6 ports (Git/Forge/Checkpoint, signatures) | §7 (traits, associated errors, no impls) |
| §9 health rollup (Ord, worst-of-children, Unknown dominates) | §6 |
| §3.2 session record schema (`schema_version`) | §3.3 |
| §3.3 derived-vs-authored (only intent stored) | §3.3 (no stage/path/branch-state fields) |
| §12 hexagonal module layout | §10 |
```
