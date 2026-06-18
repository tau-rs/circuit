# Circuit H1 — Hexagonal Migration of the Application Layer

**Status:** Draft for review
**Date:** 2026-06-16
**Type:** Architecture migration (behavior-preserving)
**Depends on:** M2 slice B (#10, this branch)
**First of a 3-slice program:** **H1 (this)** → C (PR-action + checkpoint-write CLI) → D (session archival)

---

## 1. Goal

Make the whole CLI application hexagonal: move every use-case out of the `main.rs`
driving adapter into a port-generic application layer (`src/app.rs`), and put the
two remaining un-ported I/O concerns — `.circuit/` persistence and forge/remote
detection — behind ports. **No behavior changes**: the CLI's observable output is
identical; the existing `tests/` integration suite is the safety net.

After H1, Circuit's own repo should satisfy Circuit's own Dependency-rule and
Ports-&-adapters indicators — the product dogfooding its thesis.

## 2. Current state (the two violations)

The domain is already pure (`session`, `flow`, `cockpit`, `dag`, `graph`,
`indicators`, model *types*) and `GitPort`/`ForgePort`/`CheckpointStore` already
front their adapters. Two violations remain:

1. **Orchestration lives in the driving adapter.** `main.rs` holds
   `run_analyze/init/spec/dag/session_spawn/flow/board` — the use-cases are inside
   the CLI.
2. **`Workspace` persistence and detection are called directly, port-less.**
   `Workspace` (`src/model/store.rs`) is invoked straight from that orchestration;
   `gh_available()`/`has_github_remote()` are inline shell-outs. Neither can be
   substituted, so the use-cases are not unit-testable offline.

## 3. Target architecture

```
main.rs (driving adapter)   clap · build concrete adapters · render output. THIN.
   │ calls use-cases
   ▼
src/app.rs (application)     analyze/init/spec_new/dag_*/session_spawn/flow/board
   │ generic over ports      — no clap, no fs, no gh/git
   ▼
ports.rs   GitPort · ForgePort · CheckpointStore         (unchanged)
           + SettingsRepo · SpecRepo · DagRepo · SessionRepo · DeliveryProbe   (new)
   ▲ implemented by
driven adapters   adapters/{git,forge,checkpoints} · adapters/store (Workspace, relocated)
                  · adapters/probe (DeliveryProbe) · lang/builder (source parsing)
domain (pure)     session · flow · cockpit · dag · graph · indicators · model types
```

Dependencies point inward: `main → app → ports ← adapters`; `app → domain`.

## 4. New ports

All persistence ports follow the existing port convention — an associated
`type Error: std::error::Error + Send + Sync + 'static` — so they stay
adapter-agnostic. The one `Workspace` adapter implements all four with
`type Error = ModelError`.

### 4.1 Segregated repositories (Q2 decision: B)

Grouped so each use-case depends only on what it touches:

```rust
pub trait SettingsRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn is_initialized(&self) -> bool;
    fn load_config(&self) -> Result<Config, Self::Error>;
    fn save_config(&self, c: &Config) -> Result<(), Self::Error>;
    fn load_glossary(&self) -> Result<Glossary, Self::Error>;
    fn save_glossary(&self, g: &Glossary) -> Result<(), Self::Error>;
    fn load_local(&self) -> Result<LocalConfig, Self::Error>;
}

pub trait SpecRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_spec(&self, id: &str) -> Result<SpecRecord, Self::Error>;
    fn save_spec(&self, s: &SpecRecord) -> Result<(), Self::Error>;
}

pub trait DagRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_dag_node(&self, id: &str) -> Result<DagNode, Self::Error>;
    fn save_dag_node(&self, n: &DagNode) -> Result<(), Self::Error>;
    fn list_dag_nodes(&self) -> Result<Vec<DagNode>, Self::Error>;
}

pub trait SessionRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_session(&self, id: &str) -> Result<SessionRecord, Self::Error>;
    fn save_session(&self, s: &SessionRecord) -> Result<(), Self::Error>;
    fn list_sessions(&self) -> Result<Vec<SessionRecord>, Self::Error>;
}
```

These mirror `Workspace`'s existing method signatures exactly, so the adapter impl
is a one-line delegation per method. The path-builder methods (`root`,
`circuit_dir`, `*_path`, `specs_dir`, …) are **not** on any port — they are wiring
`main.rs` uses to construct adapters (`Git::new(ws.root())`), not use-case concerns.

### 4.2 `DeliveryProbe` (Q3 decision: A1, name `DeliveryProbe`)

```rust
pub trait DeliveryProbe {
    fn gh_available(&self) -> bool;
    fn has_github_remote(&self) -> bool;
}
```

The port reports raw facts; the **pure** `delivery::resolve(gh, remote)` (already
unit-tested in slice B) makes the decision in the domain. No associated `Error` —
detection degrades to `false`, never errors (matches today's `.unwrap_or(false)`).

## 5. The application layer (`src/app.rs`)

One port-generic function per use-case. Each takes the ports it needs (per the Q2
matrix) plus already-resolved domain inputs; it never reads args, the filesystem,
or shells out. Signatures (illustrative — exact arg lists finalized in the plan):

| use-case | depends on |
|---|---|
| `init` | `SettingsRepo` |
| `spec_new` | `SettingsRepo` (init check) + `SpecRepo` |
| `dag_add_node` / `dag_list` | `SettingsRepo` + `DagRepo` |
| `session_spawn` | `SettingsRepo` + `DagRepo` + `SessionRepo` + `GitPort` |
| `flow` | `SettingsRepo` + `SessionRepo` + `GitPort` + `ForgePort` + `CheckpointStore` + `DeliveryProbe` |
| `board` | `SettingsRepo` + `DagRepo` + `SessionRepo` |
| `analyze` | (see §7 — source parsing, deferred port) |

Use-cases return domain values / view-models; `main.rs` owns all `println!`
rendering. Where a use-case currently prints (e.g. `flow` builds rail strings),
the string-building already lives in pure `render`/`rail` code — the use-case
returns those strings and `main.rs` prints them.

## 6. `main.rs` after H1 (driving adapter)

Per command: parse args → construct the concrete adapters (`Workspace`, `Git`,
`Forge`, `Checkpoints`, `SystemDeliveryProbe`, all from `ws.root()`) → call the
`app::*` use-case → print the returned view. No business logic, no resolution
logic beyond reading clap values.

## 7. Scope boundary: source-parsing port deferred

`run_analyze` derives the architecture graph via `builder::build_graph(path)`,
which reads and tree-sitter-parses `.rs` files — a source-parsing I/O concern with
no port today. Fully hexagonalizing it means a `SourceTree`/parser port.

**Decision:** **defer** the parsing port. `analyze` is a read-only leaf with no
other dependencies, covered end-to-end by `tests/cli.rs`. In H1, `app::analyze`
orchestrates the existing `builder`/`indicators`/`render` pipeline and returns the
report; `build_graph`'s filesystem read remains inside that pipeline. This is the
**one** residual inward-pointing FS dependency after H1, explicitly recorded as a
follow-up (a `SourceTree` port) rather than silently left. Everything else is fully
ported.

## 8. Error handling

- Repo ports carry associated `Error`; `Workspace` sets `type Error = ModelError`
  (unchanged error type — no new variants).
- `app::*` use-cases return `Result<_, E>` where `E` is the relevant port/domain
  error; `main.rs` keeps its `anyhow` context wrapping at the CLI edge exactly as
  today, so user-facing error messages are unchanged.
- `DeliveryProbe` never errors (degrades to `false`).

## 9. Testing strategy

- **Behavior preserved (the safety net):** the existing integration suite
  (`tests/cli.rs`, `tests/board.rs`, `tests/session_flow.rs`, `tests/data_model.rs`)
  exercises every command end-to-end and must stay green throughout, unchanged.
- **New app-layer unit tests:** each use-case gets unit tests against **fake**
  ports (in-memory `SettingsRepo`/`SpecRepo`/`DagRepo`/`SessionRepo`, `FakeProbe`,
  and the existing fake `ForgePort`/`CheckpointStore` patterns). This is the payoff:
  orchestration becomes testable offline, with no temp git repos.
- The pure `delivery::resolve` keeps its slice-B tests (now fed by the probe).

## 10. Migration approach

All-at-once (Q4 decision), executed **one command per task** (TDD, commit each):
define the ports → implement them on `Workspace` (relocate to `adapters/store.rs`)
→ add the `DeliveryProbe` adapter → migrate `init`, then `spec`, `dag`, `board`,
`session_spawn`, `flow`, `analyze` one at a time, thinning `main.rs` as each lands
→ final pass removes any now-dead `main.rs` helpers. The integration suite gates
every step.

## 11. File plan

| File | Change |
|---|---|
| `src/ports.rs` | **add** `SettingsRepo`, `SpecRepo`, `DagRepo`, `SessionRepo`, `DeliveryProbe` |
| `src/adapters/store.rs` | **new** — `Workspace` relocated here (with its existing path-builder methods), implementing the 4 repo ports by delegating to those methods |
| `src/model/store.rs` | **removed**; all `circuit::model::store::Workspace` imports updated to `circuit::adapters::store::Workspace` (no re-export shim — a clean move) |
| `src/adapters/probe.rs` | **new** — `SystemDeliveryProbe::new(root)` implementing `DeliveryProbe` |
| `src/adapters/mod.rs` | declare `store`, `probe` |
| `src/app.rs` | **new** — all use-cases, port-generic, with fake-port unit tests |
| `src/lib.rs` | declare `pub mod app;` |
| `src/main.rs` | thinned to clap + adapter wiring + render; orchestration removed |
| `tests/*` | unchanged (the behavior-preserving safety net) |

## 12. Deferred follow-ups

1. **`SourceTree` parsing port** for `analyze` (§7) — the one residual FS dependency.
2. **Slice C** — PR-action + checkpoint-write CLI (`CheckpointWriter` port) on this base.
3. **Slice D** — session archival (Axis 2).

## 13. Exit criteria

- Every `run_*` orchestration lives in `app::*`, generic over ports; `main.rs`
  contains no persistence/detection calls beyond constructing adapters.
- `Workspace` reaches the app layer only through the four repo port traits;
  detection only through `DeliveryProbe`.
- Every use-case has offline unit tests against fake ports.
- The full existing integration suite passes unchanged; `cargo build`/`clippy`
  clean; `#![forbid(unsafe_code)]` intact.
