# Circuit M2 — Forge Automation CLI Verbs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose the three tested `ForgePort` write actions (`create_pr`, `merge`, `update_from_base`) as `circuit session pr|merge|update <id>` subcommands.

**Architecture:** Pure hexagonal extension. Three app-layer orchestration functions in `src/app.rs` over the existing `ForgePort`, fronted by three `SessionCommand` variants in `src/main.rs`. A shared private gate (`forge_preconditions`) enforces mode + branch; each verb adds its PR-state precondition. The only new pure logic is `compose_pr_body`. No new ports, adapters, or foundation types.

**Tech Stack:** Rust, `anyhow` (app-layer errors), existing in-module test fakes (`MemStore`, `FakeProbe`, `FakeErr`). `#![forbid(unsafe_code)]`.

**Spec:** `docs/superpowers/specs/2026-06-19-circuit-m2-forge-cli-verbs-design.md`

## Global Constraints

- Hexagonal: app layer is generic over port traits; no `gh`/git shell-out in `app.rs`.
- App-layer errors use `anyhow` internally (matches `session_archive`/`session_unarchive`).
- A failed precondition gate returns before any `forge`/`gh` call.
- `review_state` returning `Err` (forge unreachable) must propagate as an error — a write verb never proceeds on an undeterminable state.
- TDD: failing test → minimal impl → green → commit. Conventional commit messages.
- Run `cargo fmt` before each commit.

## File structure

| File | Responsibility |
|---|---|
| `src/app.rs` | **Modify** — `compose_pr_body` (pure); `forge_preconditions` (shared gate); `session_pr`/`session_merge`/`session_update`; `PrOutcome`/`MergeOutcome`/`UpdateOutcome`; `SpyForge` test fake + tests |
| `src/main.rs` | **Modify** — 3 `SessionCommand` variants; 3 `run_session_*` glue fns; dispatch arms |

**Consumed unchanged:** `resolve_session`, `require_initialized` (`src/app.rs`); `ForgePort`, `SettingsRepo`, `SessionRepo`, `DagRepo`, `DeliveryProbe` (`src/ports.rs`); `delivery::resolve`, `DeliveryMode` (`src/flow/delivery.rs`); `ReviewState` (`src/flow/facts.rs`); `DagNode` (`src/model/node.rs`); `Forge`, `SystemDeliveryProbe` (`src/adapters/`).

---

## Task 1: `compose_pr_body` (pure)

**Files:**
- Modify: `src/app.rs` (add the fn near the other free fns; tests in the existing `mod tests`)

**Interfaces:**
- Produces: `fn compose_pr_body(node: &crate::model::node::DagNode) -> String` (module-private)

- [ ] **Step 1: Write the failing tests**

In `src/app.rs`, inside `#[cfg(test)] mod tests`, add (the test module already has `use super::*;` and `use crate::model::node::DagNode;` is reachable via `super`):

```rust
    fn node_with_intent(intent: &str) -> crate::model::node::DagNode {
        let mut n = crate::model::node::DagNode::new("auth-login", "auth", "Add login flow", "impl/auth-login");
        n.intent = intent.to_string();
        n
    }

    #[test]
    fn pr_body_includes_intent_then_footer() {
        let body = compose_pr_body(&node_with_intent("Implements OAuth2 login."));
        assert_eq!(
            body,
            "Implements OAuth2 login.\n\n---\n🔁 Circuit · spec `auth` · node `auth-login`"
        );
    }

    #[test]
    fn pr_body_empty_intent_is_footer_only() {
        let body = compose_pr_body(&node_with_intent("   "));
        assert_eq!(body, "---\n🔁 Circuit · spec `auth` · node `auth-login`");
    }

    #[test]
    fn pr_body_footer_always_carries_spec_and_node() {
        let body = compose_pr_body(&node_with_intent(""));
        assert!(body.contains("spec `auth`"));
        assert!(body.contains("node `auth-login`"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib app::tests::pr_body`
Expected: FAIL — `compose_pr_body` not found.

- [ ] **Step 3: Write the implementation**

In `src/app.rs`, add as a free function (near `resolve_session`, outside the test module):

```rust
/// PR body = node intent (when non-empty) + a provenance footer tying the PR
/// back to its spec + DAG node. The footer is always present. Pure.
fn compose_pr_body(node: &DagNode) -> String {
    let footer = format!("---\n🔁 Circuit · spec `{}` · node `{}`", node.spec, node.id);
    if node.intent.trim().is_empty() {
        footer
    } else {
        format!("{}\n\n{}", node.intent.trim(), footer)
    }
}
```

(`DagNode` is already imported at the top of `app.rs`: `use crate::model::node::DagNode;`.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib app::tests::pr_body`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/app.rs
git commit -m "feat(m2): compose_pr_body — PR body from DAG node intent + provenance footer"
```

---

## Task 2: `session_pr` + shared `forge_preconditions` gate + `SpyForge` fake

**Files:**
- Modify: `src/app.rs` (gate fn, `PrOutcome`, `session_pr`; `SpyForge` + tests in `mod tests`)

**Interfaces:**
- Consumes: `compose_pr_body` (Task 1); `resolve_session`, `require_initialized`; `delivery::resolve`, `DeliveryMode`; `ReviewState`.
- Produces:
  - `fn forge_preconditions<S: SettingsRepo, Se: SessionRepo, P: DeliveryProbe>(settings: &S, sessions: &Se, probe: &P, selector: &str) -> anyhow::Result<(SessionRecord, String, String)>` — returns `(record, branch, base)`; module-private. Used by Tasks 3 & 4.
  - `pub struct PrOutcome { pub session_id: SessionId, pub branch: String, pub base: String, pub title: String }`
  - `pub fn session_pr<S: SettingsRepo, Se: SessionRepo, D: DagRepo, F: ForgePort, P: DeliveryProbe>(settings: &S, sessions: &Se, dag: &D, forge: &F, probe: &P, selector: &str) -> anyhow::Result<PrOutcome>`
  - Test fake `SpyForge` (in `mod tests`) — reused by Tasks 3 & 4.

- [ ] **Step 1: Add imports for the gate**

In `src/app.rs`, the existing import block already has `use crate::flow::delivery::{self, DeliveryMode};`. Add `ReviewState` to the `flow::facts` import — change:

```rust
use crate::flow::facts::DeliveryFacts;
```
to:
```rust
use crate::flow::facts::{DeliveryFacts, ReviewState};
```

- [ ] **Step 2: Write the `SpyForge` test fake**

In `src/app.rs`, inside `#[cfg(test)] mod tests`, add (alongside `NoopForge`):

```rust
    use std::cell::RefCell;

    /// Records write-action arguments and returns a configurable review state.
    struct SpyForge {
        review: crate::flow::facts::ReviewState,
        review_fails: bool,
        action_fails: bool,
        created: RefCell<Vec<(String, String, String, String)>>,
        merged: RefCell<Vec<String>>,
        updated: RefCell<Vec<(String, String)>>,
    }
    impl SpyForge {
        fn with_review(review: crate::flow::facts::ReviewState) -> Self {
            SpyForge {
                review,
                review_fails: false,
                action_fails: false,
                created: RefCell::new(vec![]),
                merged: RefCell::new(vec![]),
                updated: RefCell::new(vec![]),
            }
        }
    }
    impl crate::ports::ForgePort for SpyForge {
        type Error = crate::app::fakes::FakeErr;
        fn review_state(&self, _b: &str) -> Result<crate::flow::facts::ReviewState, Self::Error> {
            if self.review_fails {
                Err(crate::app::fakes::FakeErr("forge unreachable".into()))
            } else {
                Ok(self.review)
            }
        }
        fn create_pr(&self, b: &str, base: &str, t: &str, body: &str) -> Result<(), Self::Error> {
            if self.action_fails {
                return Err(crate::app::fakes::FakeErr("gh create failed".into()));
            }
            self.created
                .borrow_mut()
                .push((b.into(), base.into(), t.into(), body.into()));
            Ok(())
        }
        fn merge(&self, b: &str) -> Result<(), Self::Error> {
            if self.action_fails {
                return Err(crate::app::fakes::FakeErr("gh merge failed".into()));
            }
            self.merged.borrow_mut().push(b.into());
            Ok(())
        }
        fn update_from_base(&self, b: &str, base: &str) -> Result<(), Self::Error> {
            if self.action_fails {
                return Err(crate::app::fakes::FakeErr("gh update failed".into()));
            }
            self.updated.borrow_mut().push((b.into(), base.into()));
            Ok(())
        }
    }

    fn forge_store_with_impl_session(node: &str, intent: &str) -> (MemStore, String) {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        let mut dag_node =
            crate::model::node::DagNode::new(node, "auth", "Add login flow", format!("impl/{node}"));
        dag_node.intent = intent.to_string();
        store.nodes.borrow_mut().insert(node.into(), dag_node);
        let s = impl_session(node);
        let id = s.id.to_string();
        store.sessions.borrow_mut().insert(id.clone(), s);
        (store, id)
    }

    fn forge_probe() -> crate::app::fakes::FakeProbe {
        crate::app::fakes::FakeProbe { gh: true, remote: true }
    }
```

(`impl_session` and `MemStore` already exist in the test module.)

- [ ] **Step 3: Write the failing `session_pr` tests**

In `src/app.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn session_pr_happy_path_creates_pr_with_derived_args() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "Implements login.");
        let forge = SpyForge::with_review(ReviewState::None);
        let out = session_pr(&store, &store, &store, &forge, &forge_probe(), "auth-login").unwrap();
        assert_eq!(out.branch, "impl/auth-login");
        assert_eq!(out.base, "main");
        assert_eq!(out.title, "Add login flow");
        let created = forge.created.borrow();
        assert_eq!(created.len(), 1);
        let (b, base, title, body) = &created[0];
        assert_eq!(b, "impl/auth-login");
        assert_eq!(base, "main");
        assert_eq!(title, "Add login flow");
        assert!(body.starts_with("Implements login."));
        assert!(body.contains("node `auth-login`"));
    }

    #[test]
    fn session_pr_local_mode_is_refused() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::None);
        let probe = crate::app::fakes::FakeProbe { gh: false, remote: false };
        let err = session_pr(&store, &store, &store, &forge, &probe, "auth-login").unwrap_err();
        assert!(err.to_string().contains("require a GitHub forge"), "got: {err}");
        assert!(forge.created.borrow().is_empty());
    }

    #[test]
    fn session_pr_no_branch_is_refused() {
        let (store, id) = forge_store_with_impl_session("auth-login", "");
        // Strip the branch off the stored session.
        {
            let mut sessions = store.sessions.borrow_mut();
            let s = sessions.get_mut(&id).unwrap();
            s.branch = None;
        }
        let forge = SpyForge::with_review(ReviewState::None);
        let err = session_pr(&store, &store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(err.to_string().contains("no branch"), "got: {err}");
    }

    #[test]
    fn session_pr_existing_pr_is_refused() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::Open);
        let err = session_pr(&store, &store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(err.to_string().contains("already exists"), "got: {err}");
        assert!(forge.created.borrow().is_empty());
    }

    #[test]
    fn session_pr_forge_unreachable_propagates() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let mut forge = SpyForge::with_review(ReviewState::None);
        forge.review_fails = true;
        let err = session_pr(&store, &store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(err.to_string().contains("forge unreachable") || err.to_string().contains("PR state"));
    }
```

- [ ] **Step 4: Run to verify failure**

Run: `cargo test --lib app::tests::session_pr`
Expected: FAIL — `session_pr` / `forge_preconditions` not found.

- [ ] **Step 5: Implement the gate, `PrOutcome`, and `session_pr`**

In `src/app.rs`, add (outside the test module, near the other session fns):

```rust
/// Shared precondition gate for the forge write verbs: resolve the session,
/// require a branch, require Forge delivery mode, and load the base branch.
/// Returns `(record, branch, base)`. Runs before any forge call.
fn forge_preconditions<S, Se, P>(
    settings: &S,
    sessions: &Se,
    probe: &P,
    selector: &str,
) -> anyhow::Result<(SessionRecord, String, String)>
where
    S: SettingsRepo,
    Se: SessionRepo,
    P: DeliveryProbe,
{
    require_initialized(settings)?;
    let record = resolve_session(sessions, selector)?;
    let branch = record
        .branch
        .clone()
        .ok_or_else(|| anyhow::anyhow!("session {} has no branch — spawn it first", record.id))?;
    if delivery::resolve(probe.gh_available(), probe.has_github_remote()) != DeliveryMode::Forge {
        anyhow::bail!("PR actions require a GitHub forge; this repo uses local checkpoints");
    }
    let base = settings
        .load_config()
        .context("loading config.toml")?
        .base_branch;
    Ok((record, branch, base))
}

/// Outcome of `session_pr`.
pub struct PrOutcome {
    pub session_id: SessionId,
    pub branch: String,
    pub base: String,
    pub title: String,
}

/// Open a PR for the session's branch. Title comes from the session's DAG node;
/// body from `compose_pr_body`. Refused unless mode is Forge, the session has a
/// branch and a DAG node, and no PR exists yet.
pub fn session_pr<S, Se, D, F, P>(
    settings: &S,
    sessions: &Se,
    dag: &D,
    forge: &F,
    probe: &P,
    selector: &str,
) -> anyhow::Result<PrOutcome>
where
    S: SettingsRepo,
    Se: SessionRepo,
    D: DagRepo,
    F: ForgePort,
    P: DeliveryProbe,
{
    let (record, branch, base) = forge_preconditions(settings, sessions, probe, selector)?;
    let node_id = record.dag_node.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "session {} has no DAG node — cannot derive PR title/body",
            record.id
        )
    })?;
    let node = dag
        .load_dag_node(&node_id)
        .with_context(|| format!("loading DAG node {node_id}"))?;
    match forge
        .review_state(&branch)
        .with_context(|| format!("checking PR state for {branch}"))?
    {
        ReviewState::None => {}
        other => anyhow::bail!("a PR for {branch} already exists (state: {other:?})"),
    }
    forge
        .create_pr(&branch, &base, &node.title, &compose_pr_body(&node))
        .with_context(|| format!("opening PR for {branch}"))?;
    Ok(PrOutcome {
        session_id: record.id,
        branch,
        base,
        title: node.title,
    })
}
```

- [ ] **Step 6: Run to verify pass**

Run: `cargo test --lib app::tests::session_pr`
Expected: PASS (5 tests).

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add src/app.rs
git commit -m "feat(m2): session_pr app verb + shared forge_preconditions gate"
```

---

## Task 3: `session_merge`

**Files:**
- Modify: `src/app.rs` (`MergeOutcome`, `session_merge`; tests in `mod tests`)

**Interfaces:**
- Consumes: `forge_preconditions` (Task 2); `ReviewState`; `SpyForge`, `forge_store_with_impl_session`, `forge_probe` test helpers (Task 2).
- Produces:
  - `pub struct MergeOutcome { pub session_id: SessionId, pub branch: String, pub base: String }`
  - `pub fn session_merge<S: SettingsRepo, Se: SessionRepo, F: ForgePort, P: DeliveryProbe>(settings: &S, sessions: &Se, forge: &F, probe: &P, selector: &str) -> anyhow::Result<MergeOutcome>`

- [ ] **Step 1: Write the failing tests**

In `src/app.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn session_merge_approved_merges() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::Approved);
        let out = session_merge(&store, &store, &forge, &forge_probe(), "auth-login").unwrap();
        assert_eq!(out.branch, "impl/auth-login");
        assert_eq!(*forge.merged.borrow(), vec!["impl/auth-login".to_string()]);
    }

    #[test]
    fn session_merge_not_approved_is_refused() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::ChangesRequested);
        let err = session_merge(&store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(err.to_string().contains("not Approved"), "got: {err}");
        assert!(forge.merged.borrow().is_empty());
    }

    #[test]
    fn session_merge_local_mode_is_refused() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::Approved);
        let probe = crate::app::fakes::FakeProbe { gh: false, remote: false };
        let err = session_merge(&store, &store, &forge, &probe, "auth-login").unwrap_err();
        assert!(err.to_string().contains("require a GitHub forge"), "got: {err}");
    }

    #[test]
    fn session_merge_forge_error_propagates() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let mut forge = SpyForge::with_review(ReviewState::Approved);
        forge.action_fails = true;
        let err = session_merge(&store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(err.to_string().contains("merge"), "got: {err}");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::session_merge`
Expected: FAIL — `session_merge` not found.

- [ ] **Step 3: Implement**

In `src/app.rs`, add (near `session_pr`):

```rust
/// Outcome of `session_merge`.
pub struct MergeOutcome {
    pub session_id: SessionId,
    pub branch: String,
    pub base: String,
}

/// Merge the session's PR (merge-commit strategy in the adapter). Refused unless
/// mode is Forge, the session has a branch, and review state is `Approved`.
pub fn session_merge<S, Se, F, P>(
    settings: &S,
    sessions: &Se,
    forge: &F,
    probe: &P,
    selector: &str,
) -> anyhow::Result<MergeOutcome>
where
    S: SettingsRepo,
    Se: SessionRepo,
    F: ForgePort,
    P: DeliveryProbe,
{
    let (record, branch, base) = forge_preconditions(settings, sessions, probe, selector)?;
    match forge
        .review_state(&branch)
        .with_context(|| format!("checking PR state for {branch}"))?
    {
        ReviewState::Approved => {}
        other => anyhow::bail!("cannot merge {branch} — review state is {other:?}, not Approved"),
    }
    forge
        .merge(&branch)
        .with_context(|| format!("merging PR for {branch}"))?;
    Ok(MergeOutcome {
        session_id: record.id,
        branch,
        base,
    })
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app::tests::session_merge`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/app.rs
git commit -m "feat(m2): session_merge app verb (requires Approved review state)"
```

---

## Task 4: `session_update`

**Files:**
- Modify: `src/app.rs` (`UpdateOutcome`, `session_update`; tests in `mod tests`)

**Interfaces:**
- Consumes: `forge_preconditions` (Task 2); `ReviewState`; `SpyForge`, `forge_store_with_impl_session`, `forge_probe` test helpers (Task 2).
- Produces:
  - `pub struct UpdateOutcome { pub session_id: SessionId, pub branch: String, pub base: String }`
  - `pub fn session_update<S: SettingsRepo, Se: SessionRepo, F: ForgePort, P: DeliveryProbe>(settings: &S, sessions: &Se, forge: &F, probe: &P, selector: &str) -> anyhow::Result<UpdateOutcome>`

- [ ] **Step 1: Write the failing tests**

In `src/app.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn session_update_open_pr_updates() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::Open);
        let out = session_update(&store, &store, &forge, &forge_probe(), "auth-login").unwrap();
        assert_eq!(out.base, "main");
        assert_eq!(
            *forge.updated.borrow(),
            vec![("impl/auth-login".to_string(), "main".to_string())]
        );
    }

    #[test]
    fn session_update_changes_requested_updates() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::ChangesRequested);
        session_update(&store, &store, &forge, &forge_probe(), "auth-login").unwrap();
        assert_eq!(forge.updated.borrow().len(), 1);
    }

    #[test]
    fn session_update_no_open_pr_is_refused() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::None);
        let err = session_update(&store, &store, &forge, &forge_probe(), "auth-login").unwrap_err();
        assert!(err.to_string().contains("no open PR"), "got: {err}");
        assert!(forge.updated.borrow().is_empty());
    }

    #[test]
    fn session_update_local_mode_is_refused() {
        let (store, _id) = forge_store_with_impl_session("auth-login", "");
        let forge = SpyForge::with_review(ReviewState::Open);
        let probe = crate::app::fakes::FakeProbe { gh: false, remote: false };
        let err = session_update(&store, &store, &forge, &probe, "auth-login").unwrap_err();
        assert!(err.to_string().contains("require a GitHub forge"), "got: {err}");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::session_update`
Expected: FAIL — `session_update` not found.

- [ ] **Step 3: Implement**

In `src/app.rs`, add (near `session_merge`):

```rust
/// Outcome of `session_update`.
pub struct UpdateOutcome {
    pub session_id: SessionId,
    pub branch: String,
    pub base: String,
}

/// Update the session's branch from base. Refused unless mode is Forge, the
/// session has a branch, and a PR is open (Open / ChangesRequested / Approved).
pub fn session_update<S, Se, F, P>(
    settings: &S,
    sessions: &Se,
    forge: &F,
    probe: &P,
    selector: &str,
) -> anyhow::Result<UpdateOutcome>
where
    S: SettingsRepo,
    Se: SessionRepo,
    F: ForgePort,
    P: DeliveryProbe,
{
    let (record, branch, base) = forge_preconditions(settings, sessions, probe, selector)?;
    match forge
        .review_state(&branch)
        .with_context(|| format!("checking PR state for {branch}"))?
    {
        ReviewState::Open | ReviewState::ChangesRequested | ReviewState::Approved => {}
        other => anyhow::bail!("no open PR for {branch} to update (state: {other:?})"),
    }
    forge
        .update_from_base(&branch, &base)
        .with_context(|| format!("updating {branch} from {base}"))?;
    Ok(UpdateOutcome {
        session_id: record.id,
        branch,
        base,
    })
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app::tests::session_update`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add src/app.rs
git commit -m "feat(m2): session_update app verb (requires an open PR)"
```

---

## Task 5: Wire the three `session` subcommands into the CLI

**Files:**
- Modify: `src/main.rs` (`SessionCommand` enum; `run_session` dispatch; 3 `run_session_*` fns)

**Interfaces:**
- Consumes: `circuit::app::{session_pr, session_merge, session_update}` and their outcome structs (Tasks 2–4); `Forge`, `SystemDeliveryProbe`, `Workspace` (already imported in `main.rs`).

- [ ] **Step 1: Add the three `SessionCommand` variants**

In `src/main.rs`, in `enum SessionCommand` (after the `Unarchive` variant, before the closing `}`), add:

```rust
    /// Open a PR for the session's branch (title/body from its DAG node).
    Pr {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Merge the session's approved PR (merge-commit strategy).
    Merge {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Update the session's branch from its base branch.
    Update {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
```

- [ ] **Step 2: Add the dispatch arms**

In `src/main.rs`, in `fn run_session`, add to the `match command` block (after the `Unarchive` arm):

```rust
        SessionCommand::Pr { id, path } => run_session_pr(&id, &path),
        SessionCommand::Merge { id, path } => run_session_merge(&id, &path),
        SessionCommand::Update { id, path } => run_session_update(&id, &path),
```

- [ ] **Step 3: Add the three glue functions**

In `src/main.rs`, add (after `run_session_unarchive`, near the other `run_session_*` fns):

```rust
/// Open a PR for the session's branch via the forge adapter.
fn run_session_pr(id: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let forge = Forge::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::session_pr(&ws, &ws, &ws, &forge, &probe, id)?;
    println!(
        "Opened PR for session {} (node {})",
        out.session_id,
        id
    );
    println!("  branch: {} → base: {}", out.branch, out.base);
    println!("  title:  {}", out.title);
    Ok(())
}

/// Merge the session's approved PR via the forge adapter.
fn run_session_merge(id: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let forge = Forge::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::session_merge(&ws, &ws, &forge, &probe, id)?;
    println!(
        "Merged PR for session {} ({} → {})",
        out.session_id, out.branch, out.base
    );
    Ok(())
}

/// Update the session's branch from base via the forge adapter.
fn run_session_update(id: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let forge = Forge::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::session_update(&ws, &ws, &forge, &probe, id)?;
    println!("Updated {} from {}", out.branch, out.base);
    Ok(())
}
```

Note: `run_session_pr` prints the caller's `id` selector as the node label (the
outcome carries `session_id`, `branch`, `base`, `title`; the node name is the
selector the user passed). `require_initialized` here is the CLI's path-aware
guard already used by the other `run_session_*` fns (not the `app::` one).

- [ ] **Step 4: Build and run the full suite**

Run: `cargo build && cargo test`
Expected: builds clean; all tests pass (new app + pure tests included; no regressions).

- [ ] **Step 5: Verify the CLI surface**

Run: `cargo run -- session --help`
Expected: lists `spawn`, `archive`, `unarchive`, `pr`, `merge`, `update`.

Run: `cargo run -- session pr --help`
Expected: shows the `<id>` positional and `--path`.

- [ ] **Step 6: Manual smoke (local-mode rejection — no GitHub needed)**

In any `circuit init`-ed repo with an impl session that has a branch but no GitHub remote:

```bash
cargo run -- session pr <SESSION_ID_OR_NODE>
```
Expected: errors with `PR actions require a GitHub forge; this repo uses local checkpoints` (proves the gate fires before any `gh` call).

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add src/main.rs
git commit -m "feat(m2): wire circuit session pr|merge|update subcommands"
```

---

## Self-review notes (for the implementer)

- **`forge_preconditions` is the single source of the mode + branch gate** — all three verbs call it, so the Local-mode and no-branch messages stay identical. Don't inline the checks per verb.
- **`review_state` Err propagates** (via `?` + `with_context`) — a write verb must never act on an undeterminable state. This is the deliberate opposite of `flow`, which degrades Err to `PR ?`.
- **No live `gh` test in CI.** If you want one, add an `#[ignore]`d smoke per verb mirroring `adapters::forge::tests::forge_live_review_state`; it is optional and not required for the suite to pass.
- **`{other:?}` uses `ReviewState`'s `Debug`** (e.g. `state: Open`). That's intentional — the enum has no `Display`.
- **No PR URL in output** — `create_pr` returns `Result<(), _>`; adding URL capture is a separate adapter change, explicitly out of scope (spec non-goals).
- **Run `cargo clippy`** before the final commit if it's used elsewhere in the repo.
