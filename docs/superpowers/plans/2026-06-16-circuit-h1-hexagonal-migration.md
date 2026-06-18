# Circuit H1 — Hexagonal Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move every CLI use-case out of `main.rs` into a port-generic `src/app.rs`, and put `.circuit/` persistence and forge/remote detection behind ports — with zero behavior change.

**Architecture:** Hexagonal. Add four segregated repository ports + a `DeliveryProbe` port to `src/ports.rs`. Relocate `Workspace` to `src/adapters/store.rs` implementing the repo ports; add `src/adapters/probe.rs`. Each `run_*` becomes an `app::*` use-case generic over the ports it needs; `main.rs` shrinks to clap parsing + adapter construction + printing. The existing `tests/` integration suite is the behavior-preserving safety net.

**Tech Stack:** Rust, `thiserror` (adapter errors), `anyhow` (app + main internal), `clap`, existing `Workspace`/`Git`/`Forge`/`Checkpoints` adapters. `#![forbid(unsafe_code)]`.

**Spec:** `docs/superpowers/specs/2026-06-16-circuit-h1-hexagonal-migration-design.md`

## Global Constraints

- `#![forbid(unsafe_code)]` stays on `lib.rs` and `main.rs`.
- **No behavior change**: CLI output and error messages are byte-for-byte preserved; `tests/cli.rs`, `tests/board.rs`, `tests/session_flow.rs`, `tests/data_model.rs` must stay green and UNCHANGED throughout.
- Repo ports carry an associated `type Error: std::error::Error + Send + Sync + 'static`; the `Workspace` adapter sets `type Error = ModelError`.
- `app::*` use-cases return `anyhow::Result<…>` (app is "internal" per the engineering defaults); they never call `println!`, read args, or shell out. `main.rs` owns all printing and keeps its `.context(...)` wrapping so messages are unchanged.
- Path-builder methods (`root`, `circuit_dir`, `*_path`, `*_dir`) are NOT on any port — they remain inherent methods on `Workspace`, used by `main.rs` wiring only.
- The source-parsing port for `analyze` is OUT OF SCOPE (deferred per spec §7); `app::analyze` wraps the existing `builder` pipeline as-is.

---

## Task 1: Repo ports + `DeliveryProbe` port + `app` module scaffold + test fakes

**Files:**
- Modify: `src/ports.rs` (append trait defs)
- Create: `src/app.rs` (module + in-memory test fakes only; use-cases land in later tasks)
- Modify: `src/lib.rs` (declare `pub mod app;`)

**Interfaces:**
- Produces: the five new port traits (`SettingsRepo`, `SpecRepo`, `DagRepo`, `SessionRepo`, `DeliveryProbe`) and a `#[cfg(test)] pub(crate)` fakes module in `app.rs` exposing `MemStore` (implements all four repo traits, `type Error = FakeErr`) and `FakeProbe { gh: bool, remote: bool }`.

- [ ] **Step 1: Add the port traits**

In `src/ports.rs`, add these imports at the top (next to the existing `use` lines):

```rust
use crate::model::config::Config;
use crate::model::glossary::Glossary;
use crate::model::local::LocalConfig;
use crate::model::node::DagNode;
use crate::model::spec::SpecRecord;
use crate::session::SessionRecord;
```

Append at the end of `src/ports.rs` (before the `#[cfg(test)]` module if present, else at end):

```rust
/// Authored settings: config, glossary, machine-local config, and the
/// init-check. (`.circuit/` config.toml / glossary.toml / local.toml.)
pub trait SettingsRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn is_initialized(&self) -> bool;
    fn load_config(&self) -> Result<Config, Self::Error>;
    fn save_config(&self, c: &Config) -> Result<(), Self::Error>;
    fn load_glossary(&self) -> Result<Glossary, Self::Error>;
    fn save_glossary(&self, g: &Glossary) -> Result<(), Self::Error>;
    fn load_local(&self) -> Result<LocalConfig, Self::Error>;
}

/// Spec-session records (`.circuit/specs/<id>.toml`).
pub trait SpecRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_spec(&self, id: &str) -> Result<SpecRecord, Self::Error>;
    fn save_spec(&self, s: &SpecRecord) -> Result<(), Self::Error>;
}

/// Task-DAG nodes (`.circuit/dag/<id>.toml`).
pub trait DagRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_dag_node(&self, id: &str) -> Result<DagNode, Self::Error>;
    fn save_dag_node(&self, n: &DagNode) -> Result<(), Self::Error>;
    fn list_dag_nodes(&self) -> Result<Vec<DagNode>, Self::Error>;
}

/// Session records (`.circuit/sessions/<ulid>.toml`).
pub trait SessionRepo {
    type Error: std::error::Error + Send + Sync + 'static;
    fn load_session(&self, id: &str) -> Result<SessionRecord, Self::Error>;
    fn save_session(&self, s: &SessionRecord) -> Result<(), Self::Error>;
    fn list_sessions(&self) -> Result<Vec<SessionRecord>, Self::Error>;
}

/// Forge/remote detection facts (the inputs to `delivery::resolve`). Never
/// errors — detection degrades to `false`.
pub trait DeliveryProbe {
    fn gh_available(&self) -> bool;
    fn has_github_remote(&self) -> bool;
}
```

- [ ] **Step 2: Declare the app module**

In `src/lib.rs`, add `pub mod app;` in alphabetical position (after `pub mod adapters;`).

- [ ] **Step 3: Create `src/app.rs` with the test fakes**

```rust
//! Application layer — port-generic use-cases. Each function takes only the
//! ports it needs and returns domain/view values; `main.rs` does all printing.
//! No clap, no filesystem, no shell-outs here.

#[cfg(test)]
pub(crate) mod fakes {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use crate::model::config::Config;
    use crate::model::glossary::Glossary;
    use crate::model::local::LocalConfig;
    use crate::model::node::DagNode;
    use crate::model::spec::SpecRecord;
    use crate::ports::{DeliveryProbe, DagRepo, SessionRepo, SettingsRepo, SpecRepo};
    use crate::session::SessionRecord;

    #[derive(Debug)]
    pub struct FakeErr(pub String);
    impl std::fmt::Display for FakeErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for FakeErr {}

    /// In-memory implementation of all four repo ports for use-case tests.
    #[derive(Default)]
    pub struct MemStore {
        pub initialized: bool,
        pub config: Config,
        pub local: LocalConfig,
        pub glossary: Glossary,
        pub specs: RefCell<HashMap<String, SpecRecord>>,
        pub nodes: RefCell<HashMap<String, DagNode>>,
        pub sessions: RefCell<HashMap<String, SessionRecord>>,
    }

    impl SettingsRepo for MemStore {
        type Error = FakeErr;
        fn is_initialized(&self) -> bool {
            self.initialized
        }
        fn load_config(&self) -> Result<Config, FakeErr> {
            Ok(self.config.clone())
        }
        fn save_config(&self, _c: &Config) -> Result<(), FakeErr> {
            Ok(())
        }
        fn load_glossary(&self) -> Result<Glossary, FakeErr> {
            Ok(self.glossary.clone())
        }
        fn save_glossary(&self, _g: &Glossary) -> Result<(), FakeErr> {
            Ok(())
        }
        fn load_local(&self) -> Result<LocalConfig, FakeErr> {
            Ok(self.local.clone())
        }
    }

    impl SpecRepo for MemStore {
        type Error = FakeErr;
        fn load_spec(&self, id: &str) -> Result<SpecRecord, FakeErr> {
            self.specs
                .borrow()
                .get(id)
                .cloned()
                .ok_or_else(|| FakeErr(format!("no spec {id}")))
        }
        fn save_spec(&self, s: &SpecRecord) -> Result<(), FakeErr> {
            self.specs.borrow_mut().insert(s.id.clone(), s.clone());
            Ok(())
        }
    }

    impl DagRepo for MemStore {
        type Error = FakeErr;
        fn load_dag_node(&self, id: &str) -> Result<DagNode, FakeErr> {
            self.nodes
                .borrow()
                .get(id)
                .cloned()
                .ok_or_else(|| FakeErr(format!("no node {id}")))
        }
        fn save_dag_node(&self, n: &DagNode) -> Result<(), FakeErr> {
            self.nodes.borrow_mut().insert(n.id.clone(), n.clone());
            Ok(())
        }
        fn list_dag_nodes(&self) -> Result<Vec<DagNode>, FakeErr> {
            Ok(self.nodes.borrow().values().cloned().collect())
        }
    }

    impl SessionRepo for MemStore {
        type Error = FakeErr;
        fn load_session(&self, id: &str) -> Result<SessionRecord, FakeErr> {
            self.sessions
                .borrow()
                .get(id)
                .cloned()
                .ok_or_else(|| FakeErr(format!("no session {id}")))
        }
        fn save_session(&self, s: &SessionRecord) -> Result<(), FakeErr> {
            self.sessions
                .borrow_mut()
                .insert(s.id.to_string(), s.clone());
            Ok(())
        }
        fn list_sessions(&self) -> Result<Vec<SessionRecord>, FakeErr> {
            Ok(self.sessions.borrow().values().cloned().collect())
        }
    }

    pub struct FakeProbe {
        pub gh: bool,
        pub remote: bool,
    }
    impl DeliveryProbe for FakeProbe {
        fn gh_available(&self) -> bool {
            self.gh
        }
        fn has_github_remote(&self) -> bool {
            self.remote
        }
    }
}
```

> NOTE: confirm the field names used by the fakes match the real types — `SpecRecord.id: String`, `DagNode.id: String`, `SessionRecord.id: SessionId` (has `.to_string()`). If `Config`/`Glossary`/`LocalConfig` are not `Default + Clone`, derive or construct them explicitly in the fake. Read `src/model/{config,glossary,local,spec,node}.rs` and `src/session/mod.rs` to confirm before writing.

- [ ] **Step 4: Verify it compiles (warnings about unused traits are expected until later tasks)**

Run: `cargo build && cargo test --lib app::`
Expected: builds; `app::` has no tests yet (0 run) — that's fine. Unused-trait warnings are acceptable at this step only; they clear as use-cases land.

- [ ] **Step 5: Commit**

```bash
git add src/ports.rs src/app.rs src/lib.rs
git commit -m "feat(h1): add repo + DeliveryProbe ports and app-layer scaffold"
```

---

## Task 2: Relocate `Workspace` to a driven adapter + implement the ports

**Files:**
- Create: `src/adapters/store.rs` (move `Workspace` here)
- Modify: `src/model/store.rs` (delete file) and `src/model/mod.rs` (drop `pub mod store;`)
- Create: `src/adapters/probe.rs` (`SystemDeliveryProbe`)
- Modify: `src/adapters/mod.rs` (declare `store`, `probe`)
- Modify: every file importing `circuit::model::store::Workspace` or `crate::model::store::Workspace` (at least `src/main.rs`)

**Interfaces:**
- Consumes: the five ports from Task 1.
- Produces: `circuit::adapters::store::Workspace` (unchanged inherent API) now also `impl`ing `SettingsRepo + SpecRepo + DagRepo + SessionRepo` with `type Error = ModelError`; `circuit::adapters::probe::SystemDeliveryProbe::new(root: impl Into<PathBuf>)` implementing `DeliveryProbe`.

- [ ] **Step 1: Find all references to the old path**

Run: `grep -rn "model::store" src tests`
Expected: a list (at minimum `src/main.rs:20`, `src/model/mod.rs`). Note each — every one gets updated in Step 4.

- [ ] **Step 2: Move the file**

```bash
git mv src/model/store.rs src/adapters/store.rs
```

Then in `src/model/mod.rs`, remove the `pub mod store;` line. In `src/adapters/mod.rs`, add (alphabetical):

```rust
pub mod probe;
pub mod store;
```

`src/adapters/store.rs` keeps every existing inherent method unchanged. Fix its internal `use crate::...` paths if any were relative to `model::` (they reference `crate::model::{config,glossary,...}` which still resolve).

- [ ] **Step 3: Implement the four repo ports on `Workspace`**

Append to `src/adapters/store.rs` (these delegate to the inherent methods that already exist on `Workspace`):

```rust
use crate::ports::{DagRepo, SessionRepo, SettingsRepo, SpecRepo};

impl SettingsRepo for Workspace {
    type Error = ModelError;
    fn is_initialized(&self) -> bool {
        Workspace::is_initialized(self)
    }
    fn load_config(&self) -> Result<Config, ModelError> {
        Workspace::load_config(self)
    }
    fn save_config(&self, c: &Config) -> Result<(), ModelError> {
        Workspace::save_config(self, c)
    }
    fn load_glossary(&self) -> Result<Glossary, ModelError> {
        Workspace::load_glossary(self)
    }
    fn save_glossary(&self, g: &Glossary) -> Result<(), ModelError> {
        Workspace::save_glossary(self, g)
    }
    fn load_local(&self) -> Result<LocalConfig, ModelError> {
        Workspace::load_local(self)
    }
}

impl SpecRepo for Workspace {
    type Error = ModelError;
    fn load_spec(&self, id: &str) -> Result<SpecRecord, ModelError> {
        Workspace::load_spec(self, id)
    }
    fn save_spec(&self, s: &SpecRecord) -> Result<(), ModelError> {
        Workspace::save_spec(self, s)
    }
}

impl DagRepo for Workspace {
    type Error = ModelError;
    fn load_dag_node(&self, id: &str) -> Result<DagNode, ModelError> {
        Workspace::load_dag_node(self, id)
    }
    fn save_dag_node(&self, n: &DagNode) -> Result<(), ModelError> {
        Workspace::save_dag_node(self, n)
    }
    fn list_dag_nodes(&self) -> Result<Vec<DagNode>, ModelError> {
        Workspace::list_dag_nodes(self)
    }
}

impl SessionRepo for Workspace {
    type Error = ModelError;
    fn load_session(&self, id: &str) -> Result<SessionRecord, ModelError> {
        Workspace::load_session(self, id)
    }
    fn save_session(&self, s: &SessionRecord) -> Result<(), ModelError> {
        Workspace::save_session(self, s)
    }
    fn list_sessions(&self) -> Result<Vec<SessionRecord>, ModelError> {
        Workspace::list_sessions(self)
    }
}
```

> NOTE: ensure `ModelError`, `Config`, `Glossary`, `LocalConfig`, `SpecRecord`, `DagNode`, `SessionRecord` are in scope in `store.rs` (they already are, since the inherent methods use them). Add `use` lines only if the compiler reports them missing.

- [ ] **Step 4: Create the probe adapter**

`src/adapters/probe.rs`:

```rust
//! `DeliveryProbe` implemented by probing the host: `gh --version` for CLI
//! availability and `git remote -v` for a github.com remote. Detection failures
//! degrade to `false` (never errors).

use std::path::PathBuf;
use std::process::Command;

use crate::ports::DeliveryProbe;

/// Probes the real host environment, rooted at a working tree.
pub struct SystemDeliveryProbe {
    root: PathBuf,
}

impl SystemDeliveryProbe {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl DeliveryProbe for SystemDeliveryProbe {
    fn gh_available(&self) -> bool {
        Command::new("gh")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn has_github_remote(&self) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["remote", "-v"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("github.com"))
            .unwrap_or(false)
    }
}
```

- [ ] **Step 5: Update all imports found in Step 1**

Replace `model::store::Workspace` → `adapters::store::Workspace` everywhere (e.g. `src/main.rs` line 20: `use circuit::adapters::store::Workspace;`). Do NOT yet remove `main.rs`'s direct `Workspace` method calls — those migrate per-command in later tasks; `Workspace`'s inherent methods still exist.

- [ ] **Step 6: Build + full suite (behavior unchanged)**

Run: `cargo build && cargo test`
Expected: builds; ALL tests pass exactly as before (the relocation is behavior-neutral). Unused-trait warnings may remain until use-cases consume them.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(h1): relocate Workspace to adapters/store, impl repo ports + probe"
```

---

## Task 3: Migrate `init`

**Files:**
- Modify: `src/app.rs` (add `init` use-case + test)
- Modify: `src/main.rs` (`run_init` calls `app::init`; printing stays in main)

**Interfaces:**
- Consumes: `SettingsRepo` (Task 1), `MemStore` fake (Task 1).
- Produces: `app::init<S: SettingsRepo>(settings: &S) -> anyhow::Result<InitOutcome>` where `pub enum InitOutcome { AlreadyInitialized, Initialized }`. The `.gitignore` write and all printing stay in `main.rs` (filesystem side-effect tied to the CLI edge).

- [ ] **Step 1: Write the failing test**

In `src/app.rs`, add a top-level `use` block and a test:

```rust
use anyhow::Context;

use crate::ports::SettingsRepo;
use crate::model::config::Config;
use crate::model::glossary::Glossary;

/// Outcome of `init`, so `main.rs` can print the right line.
pub enum InitOutcome {
    AlreadyInitialized,
    Initialized,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::fakes::MemStore;

    #[test]
    fn init_on_fresh_store_reports_initialized() {
        let store = MemStore::default();
        assert!(matches!(init(&store).unwrap(), InitOutcome::Initialized));
    }

    #[test]
    fn init_on_initialized_store_is_noop() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        assert!(matches!(
            init(&store).unwrap(),
            InitOutcome::AlreadyInitialized
        ));
    }
}
```

- [ ] **Step 2: Run to verify it fails (no `init` fn)**

Run: `cargo test --lib app::tests::init`
Expected: FAIL — `init` not found.

- [ ] **Step 3: Implement the use-case**

In `src/app.rs` (above the test module):

```rust
/// Initialize `.circuit/` settings. Returns whether it was already present.
/// The `.gitignore` side-effect and printing stay in the CLI edge.
pub fn init<S: SettingsRepo>(settings: &S) -> anyhow::Result<InitOutcome> {
    if settings.is_initialized() {
        return Ok(InitOutcome::AlreadyInitialized);
    }
    settings
        .save_config(&Config::default())
        .context("writing config.toml")?;
    settings
        .save_glossary(&Glossary::default())
        .context("writing glossary.toml")?;
    Ok(InitOutcome::Initialized)
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib app::tests::init`
Expected: PASS (2 tests).

- [ ] **Step 5: Rewire `main.rs`**

Replace the body of `run_init` with:

```rust
fn run_init(path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    match circuit::app::init(&ws)? {
        circuit::app::InitOutcome::AlreadyInitialized => {
            println!("Already initialized at {}", ws.circuit_dir().display());
        }
        circuit::app::InitOutcome::Initialized => {
            ensure_gitignored(path, ".circuit/local.toml").context("updating .gitignore")?;
            println!("Initialized .circuit/ at {}", ws.circuit_dir().display());
        }
    }
    Ok(())
}
```

(`SettingsRepo` must be in scope in `main.rs` for the call to dispatch — add `use circuit::ports::SettingsRepo;` to the ports import line.)

- [ ] **Step 6: Full suite (behavior unchanged)**

Run: `cargo test`
Expected: all pass, including `tests/cli.rs`'s init assertions and the 2 new app tests.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "refactor(h1): migrate init into app::init use-case"
```

---

## Task 4: Migrate `spec new`

**Files:**
- Modify: `src/app.rs` (add `spec_new`), `src/main.rs` (`run_spec`)

**Interfaces:**
- Consumes: `SettingsRepo` + `SpecRepo`, `MemStore`.
- Produces: `app::spec_new<S: SettingsRepo, R: SpecRepo>(settings: &S, specs: &R, id: &str, title: String, intent: String, contexts: Vec<String>) -> anyhow::Result<()>`. The shared init-guard becomes `app::require_initialized<S: SettingsRepo>(settings: &S) -> anyhow::Result<()>` (used by this and later tasks).

- [ ] **Step 1: Write the failing tests**

In `src/app.rs` test module add:

```rust
    use crate::ports::SpecRepo;

    #[test]
    fn spec_new_requires_init() {
        let store = MemStore::default(); // not initialized
        let err = spec_new(&store, &store, "checkout", "C".into(), "pay".into(), vec![])
            .unwrap_err();
        assert!(err.to_string().contains("circuit init"));
    }

    #[test]
    fn spec_new_saves_spec_with_contexts() {
        let store = MemStore {
            initialized: true,
            ..Default::default()
        };
        spec_new(
            &store,
            &store,
            "checkout",
            "Checkout".into(),
            "Pay.".into(),
            vec!["billing".into()],
        )
        .unwrap();
        let saved = store.specs.borrow().get("checkout").cloned().unwrap();
        assert_eq!(saved.bounded_contexts, vec!["billing".to_string()]);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::spec_new`
Expected: FAIL — `spec_new` / `require_initialized` not found.

- [ ] **Step 3: Implement**

In `src/app.rs`:

```rust
use crate::ports::SpecRepo;
use crate::model::spec::SpecRecord;

/// Fail fast if `.circuit/` was never initialized. Message preserved verbatim.
pub fn require_initialized<S: SettingsRepo>(settings: &S) -> anyhow::Result<()> {
    if !settings.is_initialized() {
        anyhow::bail!("no .circuit/ workspace — run `circuit init` first");
    }
    Ok(())
}

/// Create a spec session record.
pub fn spec_new<S: SettingsRepo, R: SpecRepo>(
    settings: &S,
    specs: &R,
    id: &str,
    title: String,
    intent: String,
    contexts: Vec<String>,
) -> anyhow::Result<()> {
    require_initialized(settings)?;
    let mut spec = SpecRecord::new(id, title, intent);
    spec.bounded_contexts = contexts;
    specs
        .save_spec(&spec)
        .with_context(|| format!("writing spec {id}"))?;
    Ok(())
}
```

> NOTE: the original `require_initialized` message included the workspace path (`ws.root().display()`). Path interpolation belongs to the CLI edge. To preserve the exact user-facing message, `main.rs` keeps its own path-aware `require_initialized` for the bail message OR the message is simplified. Decision: **keep `main.rs`'s path-aware check as the user-facing guard** (it already exists and prints the path); `app::require_initialized` is the port-level guard for unit tests + non-CLI callers. `main.rs` calls its local guard first (unchanged message), so `tests/` see the identical error. Do NOT delete `main.rs::require_initialized` in this task.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app::tests::spec_new`
Expected: PASS.

- [ ] **Step 5: Rewire `main.rs`**

Replace `run_spec`'s `SpecCommand::New` arm body with:

```rust
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::spec_new(&ws, &ws, &id, title, intent, contexts)?;
            println!("Created spec session: {id}");
            Ok(())
```

- [ ] **Step 6: Full suite**

Run: `cargo test`
Expected: all pass (incl. `tests/data_model.rs` / `tests/cli.rs` spec assertions).

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "refactor(h1): migrate spec new into app::spec_new"
```

---

## Task 5: Migrate `dag` (add-node, link, check)

**Files:**
- Modify: `src/app.rs` (`dag_add_node`, `dag_link`, `dag_check`), `src/main.rs` (`run_dag`)

**Interfaces:**
- Consumes: `SettingsRepo` + `DagRepo`, `MemStore`, and the pure `dag::validate` / `dag::DagError`.
- Produces:
  - `app::dag_add_node<S,R>(settings:&S, dag:&R, id:&str, spec:String, title:String, branch:String, intent:String, depends_on:Vec<String>) -> anyhow::Result<()>`
  - `app::dag_link<S,R>(settings:&S, dag:&R, from:&str, to:&str) -> anyhow::Result<()>`
  - `app::dag_check<R: DagRepo>(dag:&R) -> anyhow::Result<Vec<dag::DagError>>` (returns the errors; `main.rs` prints + sets exit code).

- [ ] **Step 1: Write the failing tests**

```rust
    use crate::ports::DagRepo;

    #[test]
    fn dag_add_node_saves_with_deps() {
        let store = MemStore { initialized: true, ..Default::default() };
        dag_add_node(&store, &store, "auth", "checkout".into(), "Auth".into(),
            "impl/auth".into(), "do auth".into(), vec!["base".into()]).unwrap();
        let n = store.nodes.borrow().get("auth").cloned().unwrap();
        assert_eq!(n.branch, "impl/auth");
        assert_eq!(n.depends_on, vec!["base".to_string()]);
    }

    #[test]
    fn dag_link_appends_dependency_once() {
        let store = MemStore { initialized: true, ..Default::default() };
        dag_add_node(&store, &store, "a", "s".into(), "A".into(), "impl/a".into(), "".into(), vec![]).unwrap();
        dag_link(&store, &store, "a", "b").unwrap();
        dag_link(&store, &store, "a", "b").unwrap(); // idempotent
        let n = store.nodes.borrow().get("a").cloned().unwrap();
        assert_eq!(n.depends_on, vec!["b".to_string()]);
    }

    #[test]
    fn dag_check_returns_validation_errors() {
        let store = MemStore { initialized: true, ..Default::default() };
        // a node depending on a missing node -> dangling ref
        dag_add_node(&store, &store, "a", "s".into(), "A".into(), "impl/a".into(), "".into(), vec!["ghost".into()]).unwrap();
        let (errs, count) = dag_check(&store).unwrap();
        assert_eq!(count, 1);
        assert!(!errs.is_empty());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::dag`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement**

```rust
use crate::ports::DagRepo;
use crate::dag::{self, DagError};
use crate::model::node::DagNode;

pub fn dag_add_node<S: SettingsRepo, R: DagRepo>(
    settings: &S,
    dag_repo: &R,
    id: &str,
    spec: String,
    title: String,
    branch: String,
    intent: String,
    depends_on: Vec<String>,
) -> anyhow::Result<()> {
    require_initialized(settings)?;
    let mut node = DagNode::new(id, spec, title, branch);
    node.intent = intent;
    node.depends_on = depends_on;
    dag_repo
        .save_dag_node(&node)
        .with_context(|| format!("writing dag node {id}"))?;
    Ok(())
}

pub fn dag_link<S: SettingsRepo, R: DagRepo>(
    settings: &S,
    dag_repo: &R,
    from: &str,
    to: &str,
) -> anyhow::Result<()> {
    require_initialized(settings)?;
    let mut node = dag_repo
        .load_dag_node(from)
        .with_context(|| format!("loading dag node {from}"))?;
    if !node.depends_on.contains(&to.to_string()) {
        node.depends_on.push(to.to_string());
    }
    dag_repo
        .save_dag_node(&node)
        .with_context(|| format!("writing dag node {from}"))?;
    Ok(())
}

/// Validate the whole DAG; returns the error list plus the node count (the CLI
/// edge needs the count for the success line and prints + sets the exit code).
pub fn dag_check<R: DagRepo>(dag_repo: &R) -> anyhow::Result<(Vec<DagError>, usize)> {
    let nodes = dag_repo.list_dag_nodes().context("reading dag nodes")?;
    let count = nodes.len();
    Ok((dag::validate(&nodes), count))
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app::tests::dag`
Expected: PASS.

- [ ] **Step 5: Rewire `main.rs`**

Replace `run_dag`'s three arms to call the use-cases, keeping all printing + the `std::process::exit(1)` in `main.rs`:

```rust
        DagCommand::AddNode { id, spec, title, branch, intent, depends_on, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::dag_add_node(&ws, &ws, &id, spec, title, branch, intent, depends_on)?;
            println!("Added DAG node: {id}");
            Ok(())
        }
        DagCommand::Link { from, to, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::dag_link(&ws, &ws, &from, &to)?;
            println!("Linked {from} → {to}");
            Ok(())
        }
        DagCommand::Check { path } => {
            let ws = Workspace::new(&path);
            let (errors, count) = circuit::app::dag_check(&ws)?;
            if errors.is_empty() {
                println!("DAG sound — {count} node(s), no problems");
                return Ok(());
            }
            for e in &errors {
                match e {
                    DagError::Cycle(c) => println!("  cycle: {}", c.join(" → ")),
                    DagError::DanglingRef { node, missing } => {
                        println!("  dangling ref: {node} → {missing} (no such node)")
                    }
                    DagError::DuplicateBranch { branch, nodes } => {
                        println!("  duplicate branch {branch}: {}", nodes.join(", "))
                    }
                }
            }
            std::process::exit(1);
        }
```

(`DagRepo` must be in scope in `main.rs`: add to the `use circuit::ports::{...}` line.)

- [ ] **Step 6: Full suite**

Run: `cargo test`
Expected: all pass (incl. `tests/data_model.rs` DAG-check assertions).

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "refactor(h1): migrate dag add-node/link/check into app use-cases"
```

---

## Task 6: Migrate `session spawn`

**Files:**
- Modify: `src/app.rs` (`session_spawn`), `src/main.rs` (`run_session_spawn`)

**Interfaces:**
- Consumes: `SettingsRepo` + `DagRepo` + `SessionRepo` + `GitPort`, plus `resolve_worktree_dir` (pure) and `SessionId`.
- Produces: `app::session_spawn<S,D,Se,G>(settings:&S, dag:&D, sessions:&Se, git:&G, dag_node:&str, worktrees_env: Option<&str>, repo_root:&Path) -> anyhow::Result<SpawnOutcome>` where `pub struct SpawnOutcome { pub session_id: String, pub dag_node: String, pub branch: String, pub worktree: PathBuf }`. `G: GitPort` with `G::Error: std::error::Error + Send + Sync + 'static`.

- [ ] **Step 1: Write the failing test (branch-clobber guard, the real logic)**

```rust
    use crate::ports::GitPort;
    use crate::adapters::git::Git; // not used; see note
    use std::path::Path;

    // Minimal fake GitPort: branch already exists -> spawn must refuse.
    struct ExistingBranchGit;
    impl GitPort for ExistingBranchGit {
        type Error = crate::app::fakes::FakeErr;
        fn branch_facts(&self, _b: &str, _base: &str)
            -> Result<crate::flow::facts::BranchFacts, Self::Error> {
            Ok(crate::flow::facts::BranchFacts { exists: true, ..Default::default() })
        }
        fn create_branch(&self, _b: &str, _base: &str) -> Result<(), Self::Error> { Ok(()) }
        fn add_worktree(&self, _b: &str, _p: &Path) -> Result<(), Self::Error> { Ok(()) }
        fn list_worktrees(&self) -> Result<Vec<crate::ports::Worktree>, Self::Error> { Ok(vec![]) }
    }

    #[test]
    fn spawn_refuses_existing_branch() {
        let store = MemStore { initialized: true, ..Default::default() };
        store.nodes.borrow_mut().insert(
            "auth".into(),
            crate::model::node::DagNode::new("auth", "checkout".to_string(), "Auth".to_string(), "impl/auth".to_string()),
        );
        let err = session_spawn(&store, &store, &store, &ExistingBranchGit,
            "auth", None, Path::new("/tmp/repo")).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }
```

> NOTE: remove the unused `use crate::adapters::git::Git;` — it's only listed to flag that the real adapter is NOT used in the unit test. Keep imports minimal.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::spawn`
Expected: FAIL — `session_spawn` not found.

- [ ] **Step 3: Implement**

```rust
use std::path::{Path, PathBuf};
use crate::ports::{GitPort, SessionRepo};
use crate::model::local::resolve_worktree_dir;
use crate::session::{SessionId, SessionRecord};

pub struct SpawnOutcome {
    pub session_id: String,
    pub dag_node: String,
    pub branch: String,
    pub worktree: PathBuf,
}

#[allow(clippy::too_many_arguments)]
pub fn session_spawn<S, D, Se, G>(
    settings: &S,
    dag_repo: &D,
    sessions: &Se,
    git: &G,
    dag_node: &str,
    worktrees_env: Option<&str>,
    repo_root: &Path,
) -> anyhow::Result<SpawnOutcome>
where
    S: SettingsRepo,
    D: DagRepo,
    Se: SessionRepo,
    G: GitPort,
{
    require_initialized(settings)?;

    let node = dag_repo
        .load_dag_node(dag_node)
        .with_context(|| format!("loading dag node {dag_node}"))?;
    let config = settings.load_config().context("loading config.toml")?;
    let base = &config.base_branch;

    if git
        .branch_facts(&node.branch, base)
        .with_context(|| format!("checking branch {}", node.branch))?
        .exists
    {
        anyhow::bail!(
            "branch {} already exists — refusing to spawn over it",
            node.branch
        );
    }

    let id = SessionId::generate();
    let record = SessionRecord::impl_(id, node.spec.clone(), node.id.clone(), node.branch.clone());
    sessions
        .save_session(&record)
        .with_context(|| format!("writing session {id}"))?;

    let local = settings.load_local().context("loading local.toml")?;
    let worktree = resolve_worktree_dir(worktrees_env, &local, repo_root, &id.to_string());

    git.create_branch(&node.branch, base)
        .with_context(|| format!("creating branch {}", node.branch))?;
    git.add_worktree(&node.branch, &worktree)
        .with_context(|| format!("adding worktree at {}", worktree.display()))?;

    Ok(SpawnOutcome {
        session_id: id.to_string(),
        dag_node: node.id.clone(),
        branch: node.branch.clone(),
        worktree,
    })
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app::tests::spawn`
Expected: PASS.

- [ ] **Step 5: Rewire `main.rs`**

Replace `run_session_spawn`'s body:

```rust
fn run_session_spawn(dag_node: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let env = std::env::var("CIRCUIT_WORKTREES_DIR").ok();
    let out = circuit::app::session_spawn(
        &ws, &ws, &ws, &git, dag_node, env.as_deref(), ws.root(),
    )?;
    println!(
        "Spawned session {} for node {} (stage: Project)",
        out.session_id, out.dag_node
    );
    println!("  branch:   {}", out.branch);
    println!("  worktree: {}", out.worktree.display());
    Ok(())
}
```

- [ ] **Step 6: Full suite**

Run: `cargo test`
Expected: all pass — `tests/session_flow.rs` (spawn success, branch-clobber refusal) green.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "refactor(h1): migrate session spawn into app::session_spawn"
```

---

## Task 7: Migrate `flow` (+ `resolve_session`)

**Files:**
- Modify: `src/app.rs` (`flow`, `resolve_session`), `src/main.rs` (`run_flow`, drop `gh_available`/`has_github_remote`)

**Interfaces:**
- Consumes: `SettingsRepo` + `SessionRepo` + `GitPort` + `ForgePort` + `CheckpointStore` + `DeliveryProbe`; pure `delivery::resolve`, `derive_stage`, `render_rail`, `Health::Unknown`.
- Produces:
  - `app::resolve_session<Se: SessionRepo>(sessions:&Se, selector:&str) -> anyhow::Result<SessionRecord>`
  - `app::flow<S,Se,G,F,C,P>(settings:&S, sessions:&Se, git:&G, forge:&F, checkpoints:&C, probe:&P, selector: Option<&str>) -> anyhow::Result<String>` returning the full text block (or `"No sessions yet."`).

- [ ] **Step 1: Write the failing tests (resolution disambiguation — the real logic)**

```rust
    use crate::session::SessionId;

    fn impl_session(node: &str) -> SessionRecord {
        SessionRecord::impl_(SessionId::generate(), "spec", node, &format!("impl/{node}"))
    }

    #[test]
    fn resolve_session_by_dag_node_name() {
        let store = MemStore { initialized: true, ..Default::default() };
        let s = impl_session("auth");
        store.sessions.borrow_mut().insert(s.id.to_string(), s.clone());
        let got = resolve_session(&store, "auth").unwrap();
        assert_eq!(got.dag_node.as_deref(), Some("auth"));
    }

    #[test]
    fn resolve_session_unknown_errs() {
        let store = MemStore { initialized: true, ..Default::default() };
        assert!(resolve_session(&store, "nope").is_err());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::resolve_session`
Expected: FAIL.

- [ ] **Step 3: Implement `resolve_session` then `flow`**

```rust
use crate::ports::{CheckpointStore, ForgePort};
use crate::adapters::delivery::{self, DeliveryMode};
use crate::cockpit::health::Health;
use crate::flow::facts::DeliveryFacts;
use crate::flow::rail::render_rail;
use crate::flow::stage::derive_stage;
use crate::ports::DeliveryProbe;

/// Resolve a selector: exact ULID, else a unique DAG-node-name match.
pub fn resolve_session<Se: SessionRepo>(
    sessions: &Se,
    selector: &str,
) -> anyhow::Result<SessionRecord> {
    if selector.parse::<SessionId>().is_ok() {
        if let Ok(s) = sessions.load_session(selector) {
            return Ok(s);
        }
    }
    let all = sessions.list_sessions().context("listing sessions")?;
    let mut matches: Vec<SessionRecord> = all
        .into_iter()
        .filter(|s| s.dag_node.as_deref() == Some(selector))
        .collect();
    match matches.len() {
        1 => Ok(matches.pop().unwrap()),
        0 => anyhow::bail!(
            "no session matches `{selector}` (not a known session id or DAG-node name)"
        ),
        n => anyhow::bail!(
            "`{selector}` matches {n} sessions — pass the session id (ULID) to disambiguate"
        ),
    }
}

/// Render the flow rail for one session or all. Returns the text to print.
#[allow(clippy::too_many_arguments)]
pub fn flow<S, Se, G, F, C, P>(
    settings: &S,
    sessions: &Se,
    git: &G,
    forge: &F,
    checkpoints: &C,
    probe: &P,
    selector: Option<&str>,
) -> anyhow::Result<String>
where
    S: SettingsRepo,
    Se: SessionRepo,
    G: GitPort,
    F: ForgePort,
    C: CheckpointStore,
    P: DeliveryProbe,
{
    let sessions_list = match selector {
        Some(sel) => vec![resolve_session(sessions, sel)?],
        None => sessions.list_sessions().context("listing sessions")?,
    };
    if sessions_list.is_empty() {
        return Ok("No sessions yet.".to_string());
    }

    let config = settings.load_config().context("loading config.toml")?;
    let mode = delivery::resolve(probe.gh_available(), probe.has_github_remote());

    let mut blocks = Vec::new();
    for s in &sessions_list {
        let branch_facts = match &s.branch {
            Some(b) => git
                .branch_facts(b, &config.base_branch)
                .with_context(|| format!("deriving facts for {b}"))?,
            None => Default::default(),
        };
        let review = match (&s.branch, mode) {
            (Some(b), DeliveryMode::Forge) => forge.review_state(b).ok(),
            (Some(_), DeliveryMode::Local) => checkpoints.review_state(&s.id.to_string()).ok(),
            (None, _) => None,
        };
        let facts = DeliveryFacts { branch: branch_facts, review };
        let view = derive_stage(s, &facts);
        let label = s.dag_node.clone().unwrap_or_else(|| s.id.to_string());
        blocks.push(render_rail(
            &label,
            s.kind,
            view,
            s.branch.as_deref(),
            &facts.branch,
            facts.review,
            Health::Unknown,
        ));
    }
    Ok(blocks.join("\n\n"))
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app::tests::resolve_session`
Expected: PASS.

- [ ] **Step 5: Rewire `main.rs`**

Replace `run_flow` with:

```rust
fn run_flow(selector: Option<&str>, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let forge = Forge::new(ws.root());
    let checkpoints = Checkpoints::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::flow(&ws, &ws, &git, &forge, &checkpoints, &probe, selector)?;
    println!("{out}");
    Ok(())
}
```

Delete `gh_available`, `has_github_remote`, and `resolve_session` from `main.rs` (now in `app`/the probe adapter). Update imports: drop `delivery`, `DeliveryMode`, `DeliveryFacts`, `render_rail`, `derive_stage`, `Health` from `main.rs` use-lines (now used only inside `app`); add `use circuit::adapters::probe::SystemDeliveryProbe;`. Keep `Forge`, `Checkpoints`, `Git`, the port traits needed for dispatch (`ForgePort`, `CheckpointStore`, `GitPort`, `SessionRepo`, `SettingsRepo`).

- [ ] **Step 6: Full suite**

Run: `cargo test`
Expected: all pass — `tests/session_flow.rs` (incl. `local_checkpoint_drives_flow_to_review`, `no PR` assertion) green; no dead-code warnings.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "refactor(h1): migrate flow + resolve_session into app layer"
```

---

## Task 8: Migrate `board`

**Files:**
- Modify: `src/app.rs` (`board`), `src/main.rs` (`run_board`)

**Interfaces:**
- Consumes: `SettingsRepo` + `DagRepo` + `SessionRepo` + `GitPort`; the existing `cockpit::rollup::{node_health, traceability}`, `cockpit::health::rollup_children`, `render::dag_board::{Board, BoardNode, render, stage_cell, glyph}`, `derive_stage`, `DeliveryFacts`, `SessionId`/`SessionRecord`.
- Produces: `app::board<S,D,Se,G>(settings:&S, dag:&D, sessions:&Se, git:&G, spec:&str) -> anyhow::Result<String>` returning the full rendered board text (board + `--- nodes ---` readout + spec health + tasks line), byte-identical to today.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn board_renders_empty_when_no_nodes_for_spec() {
        let store = MemStore { initialized: true, ..Default::default() };
        // Use the real Git adapter against a non-repo path is overkill; board with
        // zero matching nodes exercises the assembly without git calls.
        let git = crate::adapters::git::Git::new(".");
        let out = board(&store, &store, &store, &git, "nonexistent-spec").unwrap();
        assert!(out.contains("Spec health"));
        assert!(out.contains("Tasks:"));
    }
```

> NOTE: zero matching nodes means the per-node loop never calls git — the test is offline-safe. (Verify `dag_board::render(&Board{nodes:vec![]})` doesn't panic on empty; if it does, seed one node and accept the single git call against `.`, which returns default facts.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::board`
Expected: FAIL — `board` not found.

- [ ] **Step 3: Implement** (transcribe `run_board`'s body, returning a `String` via `writeln!` into a buffer instead of `print!`/`println!`)

```rust
use std::fmt::Write as _;
use crate::render::dag_board::{self, Board, BoardNode};

pub fn board<S, D, Se, G>(
    settings: &S,
    dag_repo: &D,
    sessions_repo: &Se,
    git: &G,
    spec: &str,
) -> anyhow::Result<String>
where
    S: SettingsRepo,
    D: DagRepo,
    Se: SessionRepo,
    G: GitPort,
{
    let base = settings.load_config().context("reading config.toml")?.base_branch;
    let nodes: Vec<DagNode> = dag_repo
        .list_dag_nodes()
        .context("reading dag nodes")?
        .into_iter()
        .filter(|n| n.spec == spec)
        .collect();
    let sessions = sessions_repo.list_sessions().context("reading sessions")?;

    let mut board_nodes = Vec::new();
    for n in &nodes {
        let stage = match git.branch_facts(&n.branch, &base) {
            Ok(branch) => {
                let session = sessions
                    .iter()
                    .find(|s| s.dag_node.as_deref() == Some(n.id.as_str()))
                    .cloned()
                    .unwrap_or_else(|| {
                        SessionRecord::impl_(SessionId::generate(), &n.spec, &n.id, &n.branch)
                    });
                let facts = DeliveryFacts { branch, review: None };
                Some(derive_stage(&session, &facts))
            }
            Err(_) => None,
        };
        let health = crate::cockpit::rollup::node_health(git, &n.branch);
        board_nodes.push(BoardNode {
            id: n.id.clone(),
            depends_on: n.depends_on.clone(),
            stage,
            health,
        });
    }

    let board = Board { nodes: board_nodes };
    let mut out = String::new();
    write!(out, "{}", dag_board::render(&board)).unwrap();

    out.push_str("\n--- nodes ---\n");
    let mut sorted: Vec<&BoardNode> = board.nodes.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let healths: Vec<_> = sorted.iter().map(|n| n.health).collect();
    for n in &sorted {
        writeln!(
            out,
            "  {}  {}  {}",
            n.id,
            dag_board::stage_cell(&n.stage),
            dag_board::glyph(n.health)
        )
        .unwrap();
    }

    let spec_health = crate::cockpit::health::rollup_children(&healths);
    let trace = crate::cockpit::rollup::traceability(git, &nodes, &base);
    let m = trace
        .merged
        .map(|count| count.to_string())
        .unwrap_or_else(|| "?".to_string());
    write!(out, "\nSpec health: {}\n", dag_board::glyph(spec_health)).unwrap();
    write!(out, "Tasks: {}/{} done", m, trace.total).unwrap();
    Ok(out)
}
```

> NOTE — exact output: the original uses `print!("{}", render)` (no leading newline) then `println!("\n--- nodes ---")`. The buffer above reproduces that: `render(...)` then `"\n--- nodes ---\n"`. `main.rs` will `print!("{out}")` then a trailing `println!()` to match the original final newline behavior. Diff `cargo run -- board <spec>` output against the pre-migration commit if `tests/board.rs` is strict; adjust spacing to match exactly.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app::tests::board`
Expected: PASS.

- [ ] **Step 5: Rewire `main.rs`**

```rust
fn run_board(spec: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let out = circuit::app::board(&ws, &ws, &ws, &git, spec)?;
    println!("{out}");
    Ok(())
}
```

- [ ] **Step 6: Full suite — board output must match `tests/board.rs` exactly**

Run: `cargo test`
Expected: all pass. If `tests/board.rs` fails on whitespace, adjust the trailing-newline handling in `main.rs` (`print!` vs `println!`) until byte-identical — do NOT change `tests/board.rs`.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "refactor(h1): migrate board into app::board"
```

---

## Task 9: Migrate `analyze`

**Files:**
- Modify: `src/app.rs` (`analyze`), `src/main.rs` (`run_analyze`)

**Interfaces:**
- Consumes: `circuit::builder::build_graph`, `indicators::{cycles, dependency_rule}`, `render::mermaid` (all existing). No ports — source parsing is the deferred boundary (spec §7).
- Produces: `app::analyze(path: &Path) -> anyhow::Result<String>` returning the full report text (indicators + `--- mermaid ---` + diagram), byte-identical to today.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn analyze_self_emits_report_with_mermaid() {
        // Analyze this crate's own src — deterministic, no fixtures needed.
        let out = analyze(std::path::Path::new("src")).unwrap();
        assert!(out.contains("Architecture — Dependency rule:"));
        assert!(out.contains("--- mermaid ---"));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib app::tests::analyze`
Expected: FAIL — `analyze` not found.

- [ ] **Step 3: Implement** (transcribe `run_analyze` into a buffer-returning fn)

```rust
pub fn analyze(path: &Path) -> anyhow::Result<String> {
    let graph = crate::builder::build_graph(path)?;
    let cycles = crate::indicators::cycles::find_cycles(&graph);
    let violations = crate::indicators::dependency_rule::violations(&graph);

    let mut out = String::new();
    writeln!(
        out,
        "Architecture — No-cycles (ADP): {}",
        if cycles.is_empty() {
            "● SOUND".to_string()
        } else {
            format!("⛔ {} cyclic group(s)", cycles.len())
        }
    )
    .unwrap();
    for c in &cycles {
        writeln!(out, "  cycle: {}", c.join(" → ")).unwrap();
    }
    writeln!(
        out,
        "Architecture — Dependency rule: {}",
        if violations.is_empty() {
            "● SOUND".to_string()
        } else {
            format!("⛔ {} violation(s)", violations.len())
        }
    )
    .unwrap();
    for v in &violations {
        writeln!(
            out,
            "  {} ({:?}) → {} ({:?})  VIOLATION",
            v.from, v.from_layer, v.to, v.to_layer
        )
        .unwrap();
    }
    writeln!(out, "\n--- mermaid ---").unwrap();
    write!(
        out,
        "{}",
        crate::render::mermaid::render(&graph, &violations, &cycles)
    )
    .unwrap();
    Ok(out)
}
```

(`use std::fmt::Write as _;` already added in Task 8; if Task 9 is implemented first in a different order, add it.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib app::tests::analyze`
Expected: PASS.

- [ ] **Step 5: Rewire `main.rs`**

```rust
fn run_analyze(path: &Path) -> Result<()> {
    println!("{}", circuit::app::analyze(path)?);
    Ok(())
}
```

- [ ] **Step 6: Full suite**

Run: `cargo test`
Expected: all pass — `tests/cli.rs` analyze assertions green.

- [ ] **Step 7: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "refactor(h1): migrate analyze into app::analyze"
```

---

## Task 10: Cleanup + final verification

**Files:**
- Modify: `src/main.rs` (remove any now-dead helpers/imports)

- [ ] **Step 1: Prune dead code in `main.rs`**

`main.rs` should now contain: the clap structs, `main()`, the thin `run_*` wrappers, `require_initialized` (the path-aware CLI guard, still used), and `ensure_gitignored`. Remove any imports no longer referenced. Confirm `resolve_session`, `gh_available`, `has_github_remote` are gone (moved in Task 7).

- [ ] **Step 2: Build with warnings denied**

Run: `RUSTFLAGS="-D warnings" cargo build`
Expected: clean (no unused imports/dead code).

- [ ] **Step 3: Clippy + format**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: clean. (If `cargo fmt --check` reports diffs, run `cargo fmt` and include in the commit.)

- [ ] **Step 4: Full suite + a manual spot-check vs pre-migration**

Run: `cargo test`
Expected: every test passes. Then manually diff one command's output against the pre-migration commit to confirm byte-identical behavior:

```bash
git stash list >/dev/null 2>&1 # no-op guard
cargo run -- analyze src | head -5    # compare against main pre-H1 by eye
```

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "refactor(h1): prune dead main.rs helpers; main is now thin wiring"
```

---

## Self-review notes (for the implementer)

- **Behavior preservation is the contract.** The `tests/` integration suite is unchanged and must stay green at EVERY task. If a test fails, the migration changed behavior — fix the use-case/wiring, never the test.
- **`require_initialized` lives in two places by design:** `main.rs`'s path-aware version (user-facing message with the workspace path, unchanged) and `app::require_initialized` (port-level, for unit tests + non-CLI callers). `main.rs` calls its own first, so messages are identical.
- **Error type plumbing:** `app::*` returns `anyhow::Result`; port errors convert via `?`/`.context()` because every port `Error` is `std::error::Error + Send + Sync + 'static`. Fakes' `FakeErr` satisfies this.
- **Trait-in-scope for dispatch:** calling `ws.save_spec(..)` etc. from `main.rs` requires the relevant repo trait in scope — but after migration `main.rs` no longer calls repo methods directly (the use-cases do), so `main.rs` needs the traits in scope only where it still dispatches (it shouldn't, post-Task-9). Keep the `use circuit::ports::{...}` line minimal at Task 10.
- **Deferred (not this slice):** the `SourceTree` parsing port for `analyze`; Slice C (`CheckpointWriter` + PR/checkpoint CLI); Slice D (session archival).
