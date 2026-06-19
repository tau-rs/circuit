# Circuit M2 — Forge Automation CLI Verbs Design

**Status:** Approved for planning
**Date:** 2026-06-19
**Milestone:** M2 (Session model + flow + git) — deferred tail from slice B (spec §9: "forge write CLI verbs")

## Goal

Expose the three already-implemented, already-tested `ForgePort` write actions —
`create_pr`, `merge`, `update_from_base` (`src/adapters/forge.rs`) — as user-facing
`circuit session` subcommands. Today they are reachable only through the port and
its tests; no CLI verb invokes them. The M2 roadmap lists these as a component
("git/forge adapter: … automation actions (create PR, merge, update-from-base)").
This slice closes that gap.

## Non-goals

- **No new ports or adapters.** `ForgePort` and the `gh` adapter are unchanged.
- **No foundation changes.** No new domain types beyond app-layer outcome structs.
- **No PR URL in output.** `create_pr` returns `Result<(), _>`; capturing the URL
  would be a separate adapter change, out of scope.
- **No auto-archive on merge.** `session merge` stays single-purpose; archival
  remains the separate `session archive` verb (slice C).
- **No `--title` / `--body` overrides.** Title/body derive from the DAG node; flags
  are deferred until a need is demonstrated (YAGNI).
- **No `delivery` config override / mode-aware wording.** Still deferred (slice B §9).

## Architecture

Pure hexagonal extension. Three app-layer orchestration functions over the existing
`ForgePort`, each fronted by a `circuit session` subcommand. The only new pure logic
is `compose_pr_body`.

```
main.rs (CLI glue)                  app.rs (orchestration, generic over traits)     ports/adapters (exist)
──────────────────                  ───────────────────────────────────────────    ──────────────────────
session pr     <id> ─► run_session_pr     ─► app::session_pr    ─┐
session merge  <id> ─► run_session_merge  ─► app::session_merge ─┼─► resolve_session ─► SessionRepo
session update <id> ─► run_session_update ─► app::session_update─┘   load DagNode    ─► DagRepo
                                                                     load Config      ─► SettingsRepo
                                                                     DeliveryProbe    ─► resolve mode
                                                                     forge.{create_pr,merge,update_from_base} ─► ForgePort → gh
```

### Files touched

| File | Change |
|---|---|
| `src/app.rs` | Add `session_pr` / `session_merge` / `session_update`; pure `compose_pr_body`; `PrOutcome` / `MergeOutcome` / `UpdateOutcome` structs; tests |
| `src/main.rs` | Add 3 `SessionCommand` variants; 3 `run_session_*` glue fns; dispatch arms |

## Command surface

```
circuit
└── session
    ├── spawn      <dag_node>                          (exists)
    ├── archive    <id> [--delete-branch] [--force]    (exists)
    ├── unarchive  <id>                                (exists)
    ├── pr         <id>   ← NEW   open a PR for the session's branch
    ├── merge      <id>   ← NEW   merge the session's approved PR
    └── update     <id>   ← NEW   update the session's branch from base
```

`<id>` is a session selector — a ULID or a unique DAG-node name — resolved by the
existing `resolve_session` (same as `archive` / `unarchive`).

## Preconditions (strict)

Every verb runs a shared gate before any `gh` call; the first failure returns a
first-class `anyhow` error (no `gh` invocation occurs):

```
1. resolve_session(selector)   → not found  ► "no session matches '<sel>'"
2. session.branch == Some(b)   → None        ► "session <id> has no branch — spawn it first"
3. resolve(probe) == Forge     → Local       ► "PR actions require a GitHub forge; this repo uses local checkpoints"
4. load Config.base_branch
```

Then verb-specific preconditions and action. Only `pr` needs the DAG node (for
title/body); `merge` and `update` operate on branch + base alone:

```
pr      : session.dag_node == Some(n)  → None ► "session <id> has no DAG node — cannot derive PR title/body"
          load DagNode(n)
          forge.review_state(branch) == None
              else ► "a PR for <branch> already exists (state: <S>)"
          → forge.create_pr(branch, base, node.title, compose_pr_body(node))

merge   : forge.review_state(branch) == Approved
              else ► "cannot merge <branch> — review state is <S>, not Approved"
          → forge.merge(branch)            # adapter uses `gh pr merge --merge` (merge commit)

update  : forge.review_state(branch) ∈ { Open, ChangesRequested, Approved }
              else ► "no open PR for <branch> to update"
          → forge.update_from_base(branch, base)
```

`Circuit` owns mode + branch + PR-state validation; `gh` remains the executor. The
`pr` precondition requires `review_state == None` (no PR in any state — Open,
ChangesRequested, Approved, Merged, or Closed); re-opening against a Merged/Closed
PR is left to a future slice.

## `compose_pr_body` (only new pure function)

```rust
/// PR body = node intent (when non-empty) + a provenance footer tying the PR back
/// to its spec + DAG node. The footer is always present. Pure.
fn compose_pr_body(node: &DagNode) -> String {
    let footer = format!("---\n🔁 Circuit · spec `{}` · node `{}`", node.spec, node.id);
    if node.intent.trim().is_empty() {
        footer
    } else {
        format!("{}\n\n{}", node.intent.trim(), footer)
    }
}
```

Guarantees a non-empty, traceable body. Title is `node.title` verbatim.

## Outcomes & CLI output

```rust
pub struct PrOutcome     { pub session_id: SessionId, pub branch: String, pub base: String, pub title: String }
pub struct MergeOutcome  { pub session_id: SessionId, pub branch: String, pub base: String }
pub struct UpdateOutcome { pub session_id: SessionId, pub branch: String, pub base: String }
```

```console
$ circuit session pr auth-login
Opened PR for session 01J2… (node auth-login)
  branch: impl/auth-login → base: main
  title:  Add login flow

$ circuit session merge auth-login
Merged PR for session 01J2… (impl/auth-login → main)

$ circuit session update auth-login
Updated impl/auth-login from main
```

## Error handling

- App layer uses `anyhow` internally (matches `session_archive` / `session_unarchive`).
- The four shared-gate failures and the three verb-specific precondition failures
  are first-class `anyhow` errors with the messages above.
- A `ForgeError` from the adapter (auth failure, network, `gh` non-zero) propagates
  up unchanged — surfaced to the user as the command error, not swallowed.
- `review_state` returning `Err` (forge unreachable) propagates as an error: unlike
  `flow` (which degrades to `PR ?`), a write verb must not proceed on an
  undeterminable state.

## Testing

- **Pure:** `compose_pr_body` — non-empty intent (`intent\n\n---\n…`); empty /
  whitespace-only intent (footer only); footer always carries spec + node id.
- **App layer (existing in-module fakes — `FakeForge`, fake repos in `app.rs`):**
  for each of the three verbs —
  - happy path (asserts the right `forge` method was called with derived args; for
    `pr`, that `create_pr` received `(branch, base, node.title, composed_body)`);
  - Local-mode rejection;
  - no-branch rejection;
  - PR-state precondition rejection (e.g. `merge` when state is `Open` /
    `ChangesRequested`; `pr` when a PR already exists);
  - `ForgeError` from the adapter propagates.
  The fake forge records calls so argument derivation is asserted.
- **No live `gh` test** in CI; an `#[ignore]`d smoke per verb may be added (mirrors
  slice B's `forge_live_review_state`).
- **CLI glue** (`run_session_*`) is thin over tested app fns — manual smoke only,
  per repo convention.

## Exit criteria

- `circuit session pr|merge|update <id>` invoke the corresponding `ForgePort`
  methods with arguments derived from the session, its DAG node, and config.
- All preconditions enforced with the specified errors; no `gh` call on a failed gate.
- All three previously-unreachable adapter methods are now CLI-reachable.
- `cargo test` green (new app + pure tests included); `cargo build` clean.
