//! Application layer — port-generic use-cases. Each function takes only the
//! ports it needs and returns domain/view values; `main.rs` does all printing.
//! No clap, no filesystem, no shell-outs here.

use anyhow::Context;

use crate::dag::{self, DagError};
use crate::model::config::Config;
use crate::model::glossary::Glossary;
use crate::model::node::DagNode;
use crate::model::spec::SpecRecord;
use crate::ports::{DagRepo, SettingsRepo, SpecRepo};

/// Outcome of `init`, so `main.rs` can print the right line.
pub enum InitOutcome {
    AlreadyInitialized,
    Initialized,
}

/// Fail fast if `.circuit/` was never initialized. (Port-level guard for
/// use-cases + tests; the CLI keeps its own path-aware guard for the message.)
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
    specs.save_spec(&spec).with_context(|| format!("writing spec {id}"))?;
    Ok(())
}

/// Initialize `.circuit/` settings. Returns whether it was already present.
/// The `.gitignore` side-effect and printing stay in the CLI edge.
pub fn init<S: SettingsRepo>(settings: &S) -> anyhow::Result<InitOutcome> {
    if settings.is_initialized() {
        return Ok(InitOutcome::AlreadyInitialized);
    }
    settings.save_config(&Config::default()).context("writing config.toml")?;
    settings.save_glossary(&Glossary::default()).context("writing glossary.toml")?;
    Ok(InitOutcome::Initialized)
}

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
    dag_repo.save_dag_node(&node).with_context(|| format!("writing dag node {id}"))?;
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
    dag_repo.save_dag_node(&node).with_context(|| format!("writing dag node {from}"))?;
    Ok(())
}

/// Validate the whole DAG; returns the error list plus the node count.
pub fn dag_check<R: DagRepo>(dag_repo: &R) -> anyhow::Result<(Vec<DagError>, usize)> {
    let nodes = dag_repo.list_dag_nodes().context("reading dag nodes")?;
    let count = nodes.len();
    Ok((dag::validate(&nodes), count))
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
        let store = MemStore { initialized: true, ..Default::default() };
        assert!(matches!(init(&store).unwrap(), InitOutcome::AlreadyInitialized));
    }

    #[test]
    fn spec_new_requires_init() {
        let store = MemStore::default();
        let err = spec_new(&store, &store, "checkout", "C".into(), "pay".into(), vec![]).unwrap_err();
        assert!(err.to_string().contains("circuit init"));
    }

    #[test]
    fn spec_new_saves_spec_with_contexts() {
        let store = MemStore { initialized: true, ..Default::default() };
        spec_new(&store, &store, "checkout", "Checkout".into(), "Pay.".into(), vec!["billing".into()]).unwrap();
        let saved = store.specs.borrow().get("checkout").cloned().unwrap();
        assert_eq!(saved.bounded_contexts, vec!["billing".to_string()]);
    }

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
        dag_link(&store, &store, "a", "b").unwrap();
        let n = store.nodes.borrow().get("a").cloned().unwrap();
        assert_eq!(n.depends_on, vec!["b".to_string()]);
    }

    #[test]
    fn dag_check_returns_validation_errors() {
        let store = MemStore { initialized: true, ..Default::default() };
        dag_add_node(&store, &store, "a", "s".into(), "A".into(), "impl/a".into(), "".into(), vec!["ghost".into()]).unwrap();
        let (errs, count) = dag_check(&store).unwrap();
        assert_eq!(count, 1);
        assert!(!errs.is_empty());
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use crate::model::config::Config;
    use crate::model::glossary::Glossary;
    use crate::model::local::LocalConfig;
    use crate::model::node::DagNode;
    use crate::model::spec::SpecRecord;
    use crate::ports::{DagRepo, DeliveryProbe, SessionRepo, SettingsRepo, SpecRepo};
    use crate::session::SessionRecord;

    #[derive(Debug)]
    pub struct FakeErr(pub String);
    impl std::fmt::Display for FakeErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for FakeErr {}

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
        fn is_initialized(&self) -> bool { self.initialized }
        fn load_config(&self) -> Result<Config, FakeErr> { Ok(self.config.clone()) }
        fn save_config(&self, _c: &Config) -> Result<(), FakeErr> { Ok(()) }
        fn load_glossary(&self) -> Result<Glossary, FakeErr> { Ok(self.glossary.clone()) }
        fn save_glossary(&self, _g: &Glossary) -> Result<(), FakeErr> { Ok(()) }
        fn load_local(&self) -> Result<LocalConfig, FakeErr> { Ok(self.local.clone()) }
    }
    impl SpecRepo for MemStore {
        type Error = FakeErr;
        fn load_spec(&self, id: &str) -> Result<SpecRecord, FakeErr> {
            self.specs.borrow().get(id).cloned().ok_or_else(|| FakeErr(format!("no spec {id}")))
        }
        fn save_spec(&self, s: &SpecRecord) -> Result<(), FakeErr> {
            self.specs.borrow_mut().insert(s.id.clone(), s.clone()); Ok(())
        }
    }
    impl DagRepo for MemStore {
        type Error = FakeErr;
        fn load_dag_node(&self, id: &str) -> Result<DagNode, FakeErr> {
            self.nodes.borrow().get(id).cloned().ok_or_else(|| FakeErr(format!("no node {id}")))
        }
        fn save_dag_node(&self, n: &DagNode) -> Result<(), FakeErr> {
            self.nodes.borrow_mut().insert(n.id.clone(), n.clone()); Ok(())
        }
        fn list_dag_nodes(&self) -> Result<Vec<DagNode>, FakeErr> {
            Ok(self.nodes.borrow().values().cloned().collect())
        }
    }
    impl SessionRepo for MemStore {
        type Error = FakeErr;
        fn load_session(&self, id: &str) -> Result<SessionRecord, FakeErr> {
            self.sessions.borrow().get(id).cloned().ok_or_else(|| FakeErr(format!("no session {id}")))
        }
        fn save_session(&self, s: &SessionRecord) -> Result<(), FakeErr> {
            self.sessions.borrow_mut().insert(s.id.to_string(), s.clone()); Ok(())
        }
        fn list_sessions(&self) -> Result<Vec<SessionRecord>, FakeErr> {
            Ok(self.sessions.borrow().values().cloned().collect())
        }
    }

    pub struct FakeProbe { pub gh: bool, pub remote: bool }
    impl DeliveryProbe for FakeProbe {
        fn gh_available(&self) -> bool { self.gh }
        fn has_github_remote(&self) -> bool { self.remote }
    }
}
