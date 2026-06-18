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
