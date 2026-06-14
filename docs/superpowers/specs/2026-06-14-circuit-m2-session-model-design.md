# Circuit M2 — Session Model + Flow + Git — Design

**Status:** Draft for review
**Date:** 2026-06-14
**Type:** Milestone design (M2)
**Companion to:** `2026-06-14-circuit-vision-design.md`, `2026-06-14-circuit-roadmap.md`
**Builds on:** M1 walking skeleton (`2026-06-14-circuit-m1-walking-skeleton.md`) — merged.

---

## 1. Goal

Turn M1's read-only analyzer into a workflow shell. M2 delivers the two-tier session
model, the lifecycle flow surfaces, the git/forge adapter with automation actions, and
the authored `.circuit/` data model on disk — all CLI-first and deterministically testable,
with no GUI (the Tauri shell is M5, a future adapter over the same pure domain).

**Exit criteria (from the roadmap):** create a spec session, decompose it into a DAG,
spawn implementation sessions, see lifecycle + git status drive each session's flow, with
all authored state committed under `.circuit/` and round-tripped.

---

## 2. Surface — CLI-first, mirroring M1

M2 follows M1's proven shape: a pure domain (sessions, DAG, data model, stage machine,
health rollup, renderers) isolated from IO adapters (git CLI, `gh` CLI, filesystem), wired
through a thin `clap` CLI, with `assert_cmd` end-to-end tests.

Flow is rendered, not pixel-painted:
- **DAG board** → a new deterministic **mermaid** renderer (own module, not a reuse of the
  architecture renderer — it's a different graph: nodes are sessions, edges are task deps).
- **Per-session rail** → a structured **text** readout (the six-stage spine with the current
  stage marked, branch/PR facts, traceability `m/n`).

The GUI in M5 becomes another adapter over the same pure `FlowView` / `DagBoard` values.

---

## 3. The `.circuit/` authored data model

### 3.1 Layout — one file per entity

```
.circuit/
  config.toml              # project tier, capabilities, base_branch
  glossary.toml            # ubiquitous-language terms
  specs/<id>.toml          # spec session: intent + bounded contexts
  dag/<node-id>.toml       # one DAG node = one vertical slice
  sessions/<id>.toml       # session record (spec | impl | fix)
  checkpoints/<id>.toml    # local synthetic-PR snapshots (no-remote fallback)
  local.toml               # GITIGNORED — machine-local settings (worktrees dir)
```

**Why one file per entity (not one big file):** parallel impl sessions run on sibling
branches in separate worktrees. With one file per entity, two sessions editing different
nodes touch different files, so their branches merge back without conflict. A single-file
model would conflict constantly. This is the strongest argument for the layout and it is
driven by the worktree model (§7).

**Format: TOML.** Matches the repo's existing idiom (`Cargo.toml`), allows comments, and
diffs cleanly in PR review (§10 of the vision: authored state must be reviewable in PRs).

### 3.2 Schemas

Singletons:

```toml
# .circuit/config.toml
schema_version = 1
tier = "full"            # full | light | cli  (authored now; rigor consumer is M3)
base_branch = "main"     # LIVE consumer: stage derivation (merge-base, rev-list)

[capabilities]
has_ui = false           # authored now; gating consumer (UI-match) is M3
```

```toml
# .circuit/glossary.toml  (authored now; naming-indicator consumer is M3)
schema_version = 1

[[term]]
name = "Order"
definition = "A customer's confirmed basket, billed as one unit."
```

```toml
# .circuit/local.toml  (GITIGNORED — never committed, machine-local only)
worktrees_dir = "/Users/me/circuit-worktrees"   # optional; default is convention (§7.2)
```

Collections:

```toml
# .circuit/specs/checkout.toml
schema_version = 1
id = "checkout"
title = "Checkout & payment"
intent = "Let a customer pay for a basket and receive confirmation."
bounded_contexts = ["billing", "cart"]
```

```toml
# .circuit/dag/auth-slice.toml
schema_version = 1
id = "auth-slice"
spec = "checkout"
title = "Authentication slice"
intent = "Log in and gate checkout behind a session."
depends_on = []                  # other dag node ids; must form a DAG (validated)
branch = "impl/checkout-auth"    # authored bridge to git
```

```toml
# .circuit/sessions/01J....toml
schema_version = 1
id = "01J..."                    # ULID-style, stable, precedes the branch
kind = "impl"                    # spec | impl | fix
parent = "checkout"              # spec id (impl/fix) or null (spec session)
dag_node = "auth-slice"          # which slice this session executes (impl/fix)
branch = "impl/checkout-auth"    # authored bridge; worktree PATH is never stored
fixes_indicator = null           # for fix sessions: the sub-indicator violation it targets
```

```toml
# .circuit/checkpoints/2026-06-14-auth-self-review.toml
schema_version = 1
session = "01J..."
commit = "a1b2c3d"
state = "self-review"            # self-review | accepted | archived
note = "first pass on login flow"
```

Every schema carries `schema_version` for forward-compatible migration. Loading is via
`serde` + `toml`; `thiserror` types at the load boundary, `anyhow` internally.

### 3.3 Derived vs authored discipline

Only **intent** is stored under `.circuit/`. Everything about "where is this work" and "is
the code good" is **derived** from git/forge/indicators and never written to disk. The
session record stores the branch *name* (authored intent) but not the branch's *state* or
the worktree *path* (both derived/machine-local).

---

## 4. Session domain

Three session kinds, the same object at different scopes (the fractal model of §4.2):

- **Spec session** — one per feature; holds intent + bounded contexts; owns the DAG. Writes
  no application code. `parent = null`, `dag_node = null`.
- **Implementation session** — one per DAG node; executes a vertical slice on its own branch.
- **Fix session** — a scoped child spawned from a single non-green sub-indicator
  (`fixes_indicator` records which one). Same shape as an impl session.

**Identity.** A session's `id` (ULID-style) is authored in its entity file and is stable
across worktrees and commits. It **precedes the branch** — a session exists at `Draft`
before any branch is created — which is why the branch name cannot be the identity. The
branch name is the durable git-side **bridge**; the worktree path is machine-local and
ephemeral and is therefore never stored (it is discovered at runtime via `git worktree list`,
matched on the branch name).

---

## 5. Lifecycle stage — pure derivation

The flow stage is a **pure function**, never stored:

```
derive_stage(session: &Session, facts: &DeliveryFacts) -> StageView
```

over the spine `Draft › Project › Implement › Review › Merge › Done`.

### 5.1 Mapping

| Stage | Derived from |
|---|---|
| **Draft** | session authored, **no branch exists** |
| **Project** | branch exists, **no substantive commits** beyond base |
| **Implement** | branch has commits, **no PR/checkpoint** open |
| **Review** | PR open (or local: `self-review` checkpoint) |
| **Merge** | PR approved/mergeable, not yet landed |
| **Done** | branch merged into base (or local: `accepted` checkpoint) |

The **Project → Implement** boundary is derived purely from "are there substantive commits
yet?" in M2. When M3's projection lands, the projection-approved marker refines this gate.
No stored stage field exists, so there is nothing to drift (M4 reconciliation has less to
clean up).

### 5.2 Facts (plain data, no IO)

```rust
struct DeliveryFacts { branch: BranchFacts, review: ReviewState }

struct BranchFacts {
    exists: bool,
    commits_ahead_of_base: usize,     // git rev-list base..branch
    has_substantive_changes: bool,    // non-empty diff vs merge-base
    merged_into_base: bool,           // branch tip is ancestor of base (git-only, offline)
}

enum ReviewState { None, Open, Approved, Merged, Closed }  // + checks; from forge OR checkpoints
```

`derive_stage` consumes these literals — unit-tested with no git, exactly like M1's
indicators. The adapters populate them.

### 5.3 Determinism honesty offline

Almost everything is derivable from **git alone**, including `merged_into_base` (Done) —
worktrees share the object store, so `rev-list`/`merge-base` see every sibling branch's refs
without checking anything out. Only `Review`/`Merge` (PR open/approved/checks) need the
forge. When the forge is unreachable, `derive_stage` returns the best git-only stage and
marks forge-gated refinements **`Unknown`** — it never fakes `Review`/`Merge`/`Done`. This
is the §5 determinism-honesty invariant applied to flow.

### 5.4 Cross-worktree derivation needs no sibling filesystem access

A spec session derives its N children's stages purely from ref/object queries against the
shared store (git sees `impl/...` branches from any worktree) plus forge lookups by branch
name. The authored DAG supplies each node's branch; git+forge supply the live state. No
sibling-worktree disk reads.

---

## 6. Adapters — shell-out, behind ports

All IO is behind small traits so the domain stays pure and swappable:

- **`GitPort`** — `branch_facts(branch, base)`, plus worktree ops `create_branch`,
  `add_worktree`, `list_worktrees`, `remove_worktree`. Implemented by shelling out to the
  `git` CLI. Chosen over `git2`: no native build dependency, worktree/auth "just work",
  matches the "host existing ecosystems" stance (§12).
- **`ForgePort`** — `review_state(branch)` + actions `create_pr`, `merge`,
  `update_from_base`. Implemented by shelling out to the **`gh` CLI** (GitHub only for M2;
  `gh` is pre-authenticated and the port keeps GitLab/others a later adapter, not a rewrite).
- **`CheckpointStore`** — serves `ReviewState` from `.circuit/checkpoints/` when there is no
  remote. Same enum either way, so `derive_stage` is backend-agnostic.

**Testing:** the pure domain is tested with `DeliveryFacts` literals (no IO). The git/forge
adapters get thin tests against temp repos / a fake forge. No network or real GitHub in the
suite.

---

## 7. Spawn and worktree ownership

**Circuit owns worktree orchestration itself.** Conductor is only the development environment
for *building* Circuit; Circuit's end users will not have it, so parallel-sessions-as-
worktrees is a first-class capability Circuit must provide.

### 7.1 `circuit session spawn <dag-node>`

1. Allocate `id`, write `.circuit/sessions/<id>.toml` (authored: id, kind=impl, parent,
   dag_node, branch name) — the committed, diff-reviewable record.
2. Create the branch + worktree via `GitPort` (`create_branch`, `add_worktree`).
3. Stage now derives to **Project** (branch exists, no commits).

Creating the worktree is an *action*; the resulting path stays **derived** (discovered via
`git worktree list` on the branch name). The "paths are never stored" invariant holds.

### 7.2 Worktree location (machine-local)

The worktree root is inherently machine-local and must **not** go in committed `config.toml`.
Default: a sibling dir `../<repo>-worktrees/<session-id>/` (outside the main tree to avoid
nesting), overridable via gitignored `.circuit/local.toml` (`worktrees_dir`) or a
`CIRCUIT_WORKTREES_DIR` env var. This establishes the "machine-local settings live in
gitignored `.circuit/local.toml`" pattern.

---

## 8. Flow surfaces

No health colors on flow (§7 of the vision): a healthy codebase can have an open PR; a
broken one can be merged. Flow shows *where* and *what's next*, never *how good*.

### 8.1 Per-session rail (text)

```
auth-slice  [impl]  Draft › Project › ‹Implement› › Review › Merge › Done
            branch impl/checkout-auth · 3 commits · no PR · health ◍ (2 violations)
```

### 8.2 Spec-level DAG board (mermaid)

Its own deterministic renderer (`render/dag_board.rs`). Nodes are sessions showing their
stage in the label and a **separate** health glyph; edges are task dependencies. The node
fill is health-independent by construction:

```
graph TD
  auth_slice["auth-slice · Implement · ◍"]
  pay_slice["pay-slice · Review · ●"]
  auth_slice --> pay_slice
classDef flow fill:#fff,stroke:#999;
class auth_slice,pay_slice flow;
```

A renderer test asserts no health-derived color ever appears in a `classDef`/`style` line —
encoding the "flow never wears a health color" rule as an actual test.

### 8.3 Traceability (planning integrity, the cheap honest half)

The spec rail shows `Tasks: m/n done` by counting DAG nodes whose branch is merged into
base. **Scope-creep is deferred to M3** — saying "this change belongs to no task" needs
projection-defined slice boundaries that do not exist until M3; faking it would violate
determinism honesty.

---

## 9. Cockpit roll-up (health)

M2 invents no indicators — it consumes M1's two (`cycles`, `dependency_rule`) and rolls them
up per session. Health is **derived** (run M1's `build_graph` + indicators against a branch's
worktree), never stored.

```rust
enum Health { Sound, Warn, Critical, Unknown }  // Ord: Unknown sorts highest

// impl session: any cycle or dependency-rule violation => Critical, else Sound.
// spec session: worst-of-children = children.max() — so an Unknown child dominates
//               (we never report a green spec when a child was unmeasurable).
```

Glyphs: `●` Sound · `◍` Critical · `?`/Unknown. **Warn** is reserved (no advisory-only
sub-indicators exist yet).

**Computing a child's health:** run indicators against the child's worktree, discovered via
`git worktree list`. If no worktree is present, health is **`Unknown`** — M2 does not
reconstruct a tree from bare git (`git archive` gymnastics deferred). Honest and simple.

**Out of scope (no consumers yet):** the other ~25 sub-indicators of §6, debounce / severity-
weighting machinery (overkill for two indicators), drill-down popovers (GUI, M5).

---

## 10. DAG validation

A DAG is trustworthy only if it is **acyclic**, its dependency refs **resolve**, and its
branch names are **unique**. The acyclicity check **reuses M1's Tarjan SCC**
(`indicators::cycles`) — a clean dogfood: the same cycle detector that guards architecture
guards the task graph. `circuit dag check` reports any cycle, dangling ref, or duplicate
branch.

---

## 11. CLI commands (M2 additions)

```
circuit init                       # scaffold .circuit/ (config, glossary, gitignore local.toml)
circuit spec new <id>              # create a spec session
circuit dag add-node <id> --spec <s> [--depends-on ...] [--branch ...]
circuit dag link <from> <to>       # add a dependency edge
circuit dag check                  # validate (acyclic, refs, unique branches)
circuit session spawn <dag-node>   # write record + create branch + worktree (-> Project)
circuit flow [<session>]           # per-session rail (text)
circuit board <spec>               # spec-level DAG board (mermaid) with stage + health
circuit pr create <session>        # forge action (gh) / checkpoint when no remote
circuit pr merge <session>
circuit update-from-base <session>
circuit checkpoint <session> --state <self-review|accepted|archived>
```

(`circuit analyze` from M1 is unchanged.)

---

## 12. Module layout (hexagonal)

Pure domain inward, IO outward — extends M1's structure:

```
src/
  model/        # serde schemas + load/save for the whole .circuit/ model
  session/      # session kinds, identity
  flow/         # Stage, DeliveryFacts, derive_stage, rail rendering
  cockpit/      # Health, rollup
  dag/          # DAG model + validation (reuses indicators::cycles)
  render/
    mermaid.rs      # M1 (unchanged)
    dag_board.rs    # NEW: DAG board renderer (colorless flow)
  adapters/
    git.rs          # GitPort impl (shell-out)
    forge.rs        # ForgePort impl (gh)
    checkpoints.rs  # CheckpointStore (.circuit/checkpoints)
  ports.rs      # GitPort, ForgePort, CheckpointStore traits
```

`#![forbid(unsafe_code)]` continues. `thiserror` at adapter/load boundaries, `anyhow`
internally.

---

## 13. Testing strategy

- **Pure domain** (stage machine, rollup, DAG validation, renderers): unit tests with data
  literals — no IO. The bulk of the suite, mirroring M1.
- **Adapters** (git, checkpoints): tests against temp repos via `tempfile`. The `gh` forge
  adapter is exercised through a fake `ForgePort` in domain tests; the real `gh` shell-out
  gets one thin smoke test that is skipped when `gh` is unavailable (clearly logged, never
  silently passing).
- **End-to-end**: `assert_cmd` flows that `init`, create a spec, add DAG nodes, `dag check`,
  spawn a session in a temp repo, and assert the rail/board output — the exit-criteria walk.

---

## 14. Scope boundary

**In M2:** `.circuit/` schemas + load/validate/round-trip + `init` scaffolding · session
domain (3 kinds, identity) · pure derived stage machine · git adapter (facts + worktree ops,
shell-out) · GitHub forge adapter via `gh` (create PR / merge / update-from-base) · local
checkpoint fallback · flow rail + DAG board renderer (colorless) · health rollup reusing M1
indicators · traceability counting · DAG validation reusing the cycle detector.

**Explicitly deferred (noted as forward-compat, not built):**
- Auto-DAG-proposal → **M5** (needs an agent; the artifact is identical, so M5 adds a writer,
  not a new model).
- Scope-creep detection → **M3** (needs projection-defined slice boundaries).
- Tier-aware enforcement rigor, glossary-driven naming, `has_ui` gating → **M3** (their
  indicators/projection do not exist yet; the authored files exist and round-trip now).
- Incremental / cached derivation keyed on git state → forward-compat note only; `DeliveryFacts`
  collection is structured so a HEAD-sha-keyed, worktree-local cache can slot in later (§15.2).
- Reconciliation-vs-active-session conflict handling → **M4** (note for forward-compat only).
- `git2`, GitLab/other forges, re-run-checks/spawn-next as forge actions → later adapters.

---

## 15. Traceability to the vision spec

| Vision § | M2 coverage |
|---|---|
| §4.2 two-tier sessions, vertical-slice fan-out, editable DAG | §4, §10 (manual author/edit; proposal deferred) |
| §4.3 three separated concerns (state/flow/integrity) | §8 (flow colorless), §9 (health), §8.3 (traceability) |
| §6.7 spec-session cockpit = worst-of-children | §9 |
| §7 flow rail + DAG board, no health color, automation bar | §8, §11 |
| §7 local strategy = checkpoints as synthetic PRs | §3.2, §6 |
| §10 git-is-the-database, authored under `.circuit/`, derived never stored | §3, §5 |
| §5 determinism honesty (no faked verdicts) | §5.3, §9 (Unknown) |
| §15.3 `.circuit/` schema diff-friendliness | §3.1 (one file per entity, TOML) |
| §15.2 incremental recompute / cache | §14 (forward-compat note) |
| §15.4 reconciliation conflict | §14 (forward-compat note) |
