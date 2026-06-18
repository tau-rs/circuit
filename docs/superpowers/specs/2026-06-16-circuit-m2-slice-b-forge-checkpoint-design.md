# Circuit M2 Slice B — Forge + Checkpoint Adapters + Flow Wiring

**Status:** Draft for review
**Date:** 2026-06-16
**Milestone:** M2 (Session model + flow + git)
**Depends on:** M2 foundation (slice 0, #5), M2 slice A (git adapter + spawn + flow rail, #9)
**Companion to:** `2026-06-14-circuit-m2-session-model-design.md`

---

## 1. Goal

Make `circuit flow` report the **real** review state of each session — from the GitHub
forge (`gh`) when a remote exists, or from a local checkpoint file when it does not —
instead of the hardcoded `review: None` / `PR ?` that slice A left in place. This closes
the read side of M2's exit criterion: *git status drives each session's flow*.

The two outbound port traits this slice implements (`ForgePort`, `CheckpointStore`) and the
shared `ReviewState` / `DeliveryFacts` types already exist from the foundation; this slice
provides the adapters behind them and wires them into the CLI read path.

## 2. Scope

**In scope (read-path):**

- A new `ReviewState::ChangesRequested` variant (additive foundation extension — see §4).
- A **forge adapter** (`gh` shell-out) implementing all four `ForgePort` methods, fully
  unit-tested. The write methods (`create_pr`, `merge`, `update_from_base`) are implemented
  and tested but **not** bound to any CLI verb in this slice.
- A **checkpoint store** implementing `CheckpointStore::review_state` over
  `.circuit/checkpoints/<session-ULID>.toml`.
- A **`DeliveryMode` resolver** and the `run_flow` wiring that selects forge-vs-checkpoint
  once per run and feeds real `ReviewState` into `derive_stage`.

**Explicitly out of scope (deferred — see §9):**

- Session archival / lifecycle status (Axis 2).
- Forge write CLI verbs (the automation action bar).
- Mode-aware local wording in the rail.
- A `delivery` config override.

## 3. The two axes (design framing)

A session carries two independent properties. This slice touches only the first.

- **Axis 1 — Flow stage** (`Draft → Project → Implement → Review → Merge → Done`): *where is
  the work?* Derived, never stored, computed by `derive_stage` from git facts + `ReviewState`.
- **Axis 2 — Session status** (`active | archived`): *is this session still live?* Authored on
  `SessionRecord`. **Not in this slice** (§9.1). Cancellation/abandonment of a session is an
  Axis-2 archival action, **not** a flow stage and **not** a review verdict — this is why the
  checkpoint review states do not include a "cancelled"/"closed" value (§6).

## 4. Foundation extension: `ReviewState::ChangesRequested`

A forge PR with `reviewDecision = CHANGES_REQUESTED` means the ball is back with the
developer. Collapsing it to `Open` (→ Review) hides the single most actionable review signal;
reusing `Closed` would print "PR closed" while the PR is open (a lie). Neither is honest.

We add one additive variant to `ReviewState` (`src/flow/facts.rs`):

```rust
pub enum ReviewState {
    None,
    Open,
    ChangesRequested,   // NEW
    Approved,
    Merged,
    Closed,
}
```

This is the **only** foundation file this slice modifies. It is additive, not a contract
break: the exhaustive `match`es in `flow/stage.rs` and `flow/rail.rs` fail to compile until
updated, which is the desired forcing function.

- `flow/stage.rs` — `ChangesRequested` maps to `Stage::Review` with `forge_certain = true`.
  The stage marker **stays at Review** (it does not jump backward to Implement), so the DAG
  board does not thrash as a reviewer toggles between requesting changes and approving.
- `flow/rail.rs` — `review_label(Some(ChangesRequested))` → `"PR changes requested"`.

## 5. Forge adapter (`src/adapters/forge.rs`)

Implements `ForgePort` by shelling out to `gh`, mirroring the structure of the existing
`src/adapters/git.rs` (a `run`/`run_bool` helper plus a pure parser split out for testing).

### 5.1 `review_state`

Invokes `gh pr view <branch> --json state,reviewDecision` and delegates the mapping to a
**pure** function:

```rust
fn parse_review_state(exit_code: i32, stdout: &str) -> Result<ReviewState, ForgeError>
```

| `gh` result | `ReviewState` | resulting Stage |
|---|---|---|
| no PR for branch (exit 1 / empty result) | `None` | Implement |
| `state = OPEN`, `reviewDecision = APPROVED` | `Approved` | Merge |
| `state = OPEN`, `reviewDecision = CHANGES_REQUESTED` | `ChangesRequested` | Review |
| `state = OPEN`, otherwise (`null` / `REVIEW_REQUIRED`) | `Open` | Review |
| `state = MERGED` | `Merged` | Done |
| `state = CLOSED` | `Closed` | Implement |

Honesty split: a *known* "no PR exists" (`gh` ran, returned empty) is `Ok(ReviewState::None)`;
a *failure to ask* (`gh` missing / unauthenticated / offline) is `Err(ForgeError)`, which the
caller maps to `review = None` (the undeterminable `PR ?` path, §7). These must stay distinct.

Note `gh state = MERGED` is the detector for **squash/rebase** merges that rewrite the SHA so
git ancestry (`merged_into_base`) cannot see them. Git's `merged_into_base` and the forge's
`MERGED` are complementary Done-detectors; either reaches `Stage::Done`.

### 5.2 Write methods

`create_pr`, `merge`, `update_from_base` are implemented as thin wrappers over pure
argv-builders (so the exact `gh` invocation is assertable in a unit test). They are **not**
called by any CLI command in this slice; they exist to complete the trait and to be exercised
by a later automation-actions slice.

## 6. Checkpoint store (`src/adapters/checkpoints.rs`)

Implements `CheckpointStore::review_state(session)` — the no-remote fallback. The `session`
key is the **session ULID** (globally unique, stable across renames; it is what `run_flow`
already holds). Reads `.circuit/checkpoints/<ULID>.toml`:

```toml
# Synthetic-PR state for a no-remote session. `state` is the only field read in
# this slice; a `snapshots` log is written by a later slice and ignored here.
state = "self-review"   # self-review | accepted   (absent file => ReviewState::None)
```

Mapping (pure `parse` + thin file read):

| checkpoint `state` | `ReviewState` | resulting Stage |
|---|---|---|
| *(file absent)* | `None` | Implement |
| `self-review` | `Open` | Review |
| `accepted` | `Approved` | Merge → (git merge) → Done |

There is deliberately **no** checkpoint state mapping to `Closed`. In local mode you do not
"close a PR"; you either accept the work, let git merge carry it to Done, or **archive the
session** (Axis 2, §9.1). An unknown/garbage `state` value is an error
(`Err(CheckpointError)`), mapped by the caller to `None` like any other undeterminable source.

The file is normally absent until the checkpoint-write slice lands, so `review_state` returns
`Ok(ReviewState::None)` in practice today; tests write fixture files to exercise the mappings.

## 7. Delivery-mode selection + `run_flow` wiring

Today `run_flow` (`src/main.rs`) hardcodes `review: None`. This slice replaces that with a
resolved source.

### 7.1 `DeliveryMode`

```rust
enum DeliveryMode { Forge, Local }
```

Resolved **once per `circuit flow` invocation**, repo-wide (delivery mode is a property of the
repo, not the session — you cannot have some sessions on forge and others on checkpoints in
one run):

- `gh` available **and** the repo has a GitHub remote → `Forge`
- otherwise → `Local`

Detection is two checks (`gh` presence; a GitHub remote on the repo) run once. The decision
logic is a pure function of those two booleans so it can be unit-tested without shelling out.
Within `Forge` mode, a per-session `gh` error still degrades honestly to `PR ?` (it does *not*
silently fall back to checkpoints — "forge had a hiccup" must stay distinct from "this is a
local-only project").

### 7.2 Per-session wiring

```
circuit flow → resolve DeliveryMode (once) → for each session:
    BranchFacts  := Git::branch_facts (already wired)
    ReviewState  := match mode {
                        Forge => forge.review_state(branch).ok-or-None,
                        Local => checkpoints.review_state(ulid).ok-or-None,
                    }
    DeliveryFacts{ branch, review } → derive_stage → render_rail
```

A session with no branch keeps `DeliveryFacts::default()` (review `None`) and renders Draft,
exactly as today. Any adapter `Err` maps to `review = None` → `PR ?` + `(forge state unknown)`.

## 8. Error handling

- Each adapter carries its own `thiserror` enum (`ForgeError`, `CheckpointError`) satisfying
  the port's associated-`Error` bound, per the hexagonal boundary convention.
- `main.rs` uses `anyhow` context as it does today.
- **Forge/checkpoint unreachable is not fatal**: it degrades to undeterminable review state
  (`PR ?`), never aborts `circuit flow`. Git errors remain fatal, as in slice A.

## 9. Deferred follow-ups

These are real, recorded so they are not lost. Each is its own future slice.

### 9.1 Session archival (Axis 2) — *required follow-up*

Retiring a session (normal: after Done; or abandon: from any active stage). Requires: a
`status: active | archived` field on `SessionRecord` (with `schema_version` bump), a
`circuit session archive <id>` write command, worktree teardown + agent-session end, and
`circuit flow` filtering of archived sessions (`--all` to show). This subsumes the earlier
idea of a dedicated "Abandoned" flow stage — cancellation is an Axis-2 status change, not a
stage. **Tracked as the immediate next session-lifecycle slice.**

### 9.2 Mode-aware local wording

`review_label` currently prints `PR …` in both modes. In a no-remote repo there is no PR;
honest wording would read `self-review` / `accepted`. Deferred because it requires threading a
mode flag through the otherwise mode-blind rail renderer.

### 9.3 `delivery` config override

A `delivery = "forge" | "local" | "auto"` setting (`config.toml` or `local.toml`) to force a
mode; `"auto"` = §7.1 detection. The resolver in §7.1 is structured so this is a small later
addition.

### 9.4 Forge write CLI verbs

`circuit pr create` / `merge` / `update` driving the §5.2 write methods (the §7 automation
action bar). Deferred because mutating a real remote needs a dedicated testing strategy.

## 10. Testing strategy

- **Pure parsers** (`parse_review_state`, checkpoint `parse`, `DeliveryMode` decision): the
  contested logic lives in pure functions, exhaustively table-tested with zero network — `gh`
  JSON fixtures for the forge mapping, temp `.toml` files for checkpoints, injected booleans
  for mode selection. This mirrors `git.rs`'s already-tested `parse_worktree_porcelain`.
- **Stage/rail extension**: the new `ChangesRequested` arm gets a `derive_stage` truth-table
  row and a `review_label` assertion.
- **Read-path integration**: a temp repo + a fixture checkpoint file exercises the `Local`
  branch of `run_flow` end-to-end (renders `PR open`).
- **Live smoke**: a single `#[ignore]`d test invoking real `gh` (run manually, never in CI).
- The forge write argv-builders get unit tests asserting exact `gh` arguments, even though no
  CLI verb calls them yet.

## 11. File plan

| File | Change |
|---|---|
| `src/flow/facts.rs` | add `ReviewState::ChangesRequested` (additive) |
| `src/flow/stage.rs` | match arm `ChangesRequested → Review` (certain) + truth-table test |
| `src/flow/rail.rs` | `review_label` arm → `"PR changes requested"` + test |
| `src/adapters/forge.rs` | **new** — `impl ForgePort` + pure `parse_review_state` + argv-builders |
| `src/adapters/checkpoints.rs` | **new** — `impl CheckpointStore` + pure checkpoint parse |
| `src/adapters/mod.rs` | declare the two new submodules |
| `src/main.rs` | `DeliveryMode` resolver + `run_flow` wiring to real review state |

## 12. Exit criteria

- `circuit flow` on a session with a real open/approved/changes-requested/merged PR renders
  the corresponding stage and label (no more blanket `PR ?`).
- `circuit flow` in a no-remote repo with a `self-review` / `accepted` checkpoint file renders
  Review / Merge respectively.
- `gh` absent or unauthenticated degrades to `PR ?` + `(forge state unknown)`, never aborts.
- Every mapping (forge table §5.1, checkpoint table §6, `ChangesRequested` stage) has a unit
  test; the read-path integration test passes; the crate builds with `forbid(unsafe_code)`.
