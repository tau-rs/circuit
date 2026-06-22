use std::path::{Path, PathBuf};

use crate::model::{
    config::Config, glossary::Glossary, load_toml, local::LocalConfig, node::DagNode,
    projection::SystemProjection, save_toml, spec::SpecRecord, ModelError,
};
use crate::session::SessionRecord;

/// The `.circuit/` persistence boundary, rooted at a repo working tree.
/// All filesystem IO for the authored model lives here.
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn circuit_dir(&self) -> PathBuf {
        self.root.join(".circuit")
    }

    pub fn config_path(&self) -> PathBuf {
        self.circuit_dir().join("config.toml")
    }

    pub fn glossary_path(&self) -> PathBuf {
        self.circuit_dir().join("glossary.toml")
    }

    pub fn specs_dir(&self) -> PathBuf {
        self.circuit_dir().join("specs")
    }

    pub fn dag_dir(&self) -> PathBuf {
        self.circuit_dir().join("dag")
    }

    pub fn spec_path(&self, id: &str) -> PathBuf {
        self.specs_dir().join(format!("{id}.toml"))
    }

    pub fn dag_node_path(&self, id: &str) -> PathBuf {
        self.dag_dir().join(format!("{id}.toml"))
    }

    /// A workspace is initialized once its config file exists.
    pub fn is_initialized(&self) -> bool {
        self.config_path().exists()
    }

    pub fn load_config(&self) -> Result<Config, ModelError> {
        load_toml(&self.config_path())
    }

    pub fn save_config(&self, c: &Config) -> Result<(), ModelError> {
        save_toml(&self.config_path(), c)
    }

    pub fn local_path(&self) -> PathBuf {
        self.circuit_dir().join("local.toml")
    }

    /// Load `.circuit/local.toml`, or the all-`None` default when it is absent
    /// (the file is gitignored and may simply not exist on this machine).
    pub fn load_local(&self) -> Result<LocalConfig, ModelError> {
        let path = self.local_path();
        if path.exists() {
            load_toml(&path)
        } else {
            Ok(LocalConfig::default())
        }
    }

    pub fn load_glossary(&self) -> Result<Glossary, ModelError> {
        load_toml(&self.glossary_path())
    }

    pub fn save_glossary(&self, g: &Glossary) -> Result<(), ModelError> {
        save_toml(&self.glossary_path(), g)
    }

    pub fn load_spec(&self, id: &str) -> Result<SpecRecord, ModelError> {
        load_toml(&self.spec_path(id))
    }

    pub fn save_spec(&self, s: &SpecRecord) -> Result<(), ModelError> {
        save_toml(&self.spec_path(&s.id), s)
    }

    pub fn load_dag_node(&self, id: &str) -> Result<DagNode, ModelError> {
        load_toml(&self.dag_node_path(id))
    }

    pub fn save_dag_node(&self, n: &DagNode) -> Result<(), ModelError> {
        save_toml(&self.dag_node_path(&n.id), n)
    }

    pub fn projections_dir(&self) -> PathBuf {
        self.circuit_dir().join("projections")
    }

    pub fn projection_path(&self, spec: &str) -> PathBuf {
        self.projections_dir().join(format!("{spec}.toml"))
    }

    pub fn load_projection(&self, spec: &str) -> Result<SystemProjection, ModelError> {
        load_toml(&self.projection_path(spec))
    }

    pub fn save_projection(&self, p: &SystemProjection) -> Result<(), ModelError> {
        save_toml(&self.projection_path(&p.spec), p)
    }

    pub fn projection_exists(&self, spec: &str) -> bool {
        self.projection_path(spec).exists()
    }

    /// All DAG nodes, sorted by file path for deterministic order.
    pub fn list_dag_nodes(&self) -> Result<Vec<DagNode>, ModelError> {
        let dir = self.dag_dir();
        let mut nodes = Vec::new();
        if dir.is_dir() {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|source| ModelError::Io {
                    path: dir.display().to_string(),
                    source,
                })?
                // Best-effort: skip entries we can't stat (the open dir already succeeded).
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
                .collect();
            paths.sort();
            for p in paths {
                nodes.push(load_toml(&p)?);
            }
        }
        Ok(nodes)
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.circuit_dir().join("sessions")
    }

    pub fn session_path(&self, id: &str) -> PathBuf {
        self.sessions_dir().join(format!("{id}.toml"))
    }

    pub fn load_session(&self, id: &str) -> Result<SessionRecord, ModelError> {
        load_toml(&self.session_path(id))
    }

    pub fn save_session(&self, s: &SessionRecord) -> Result<(), ModelError> {
        save_toml(&self.session_path(&s.id.to_string()), s)
    }

    /// All session records, sorted by file path for deterministic order.
    pub fn list_sessions(&self) -> Result<Vec<SessionRecord>, ModelError> {
        let dir = self.sessions_dir();
        let mut sessions = Vec::new();
        if dir.is_dir() {
            let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(|source| ModelError::Io {
                    path: dir.display().to_string(),
                    source,
                })?
                // Best-effort: skip entries we can't stat (the open dir already succeeded).
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
                .collect();
            paths.sort();
            for p in paths {
                sessions.push(load_toml(&p)?);
            }
        }
        Ok(sessions)
    }
}

use crate::ports::{DagRepo, ProjectionRepo, SessionRepo, SettingsRepo, SpecRepo};

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
impl ProjectionRepo for Workspace {
    type Error = ModelError;
    fn load_projection(&self, spec: &str) -> Result<SystemProjection, ModelError> {
        Workspace::load_projection(self, spec)
    }
    fn save_projection(&self, p: &SystemProjection) -> Result<(), ModelError> {
        Workspace::save_projection(self, p)
    }
    fn projection_exists(&self, spec: &str) -> bool {
        Workspace::projection_exists(self, spec)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        assert!(!ws.is_initialized());

        let c = Config::default();
        ws.save_config(&c).unwrap();
        assert!(ws.is_initialized());
        assert_eq!(ws.load_config().unwrap(), c);
    }

    #[test]
    fn spec_and_dag_node_round_trip_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());

        let s = SpecRecord::new("checkout", "Checkout", "Pay for a basket.");
        ws.save_spec(&s).unwrap();
        assert_eq!(ws.load_spec("checkout").unwrap(), s);

        let n = DagNode::new("auth-slice", "checkout", "Auth", "impl/checkout-auth");
        ws.save_dag_node(&n).unwrap();
        assert_eq!(ws.load_dag_node("auth-slice").unwrap(), n);
    }

    #[test]
    fn list_dag_nodes_returns_sorted_and_empty_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        assert!(ws.list_dag_nodes().unwrap().is_empty());

        ws.save_dag_node(&DagNode::new("b-slice", "s", "B", "impl/b"))
            .unwrap();
        ws.save_dag_node(&DagNode::new("a-slice", "s", "A", "impl/a"))
            .unwrap();
        let ids: Vec<String> = ws
            .list_dag_nodes()
            .unwrap()
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids, vec!["a-slice".to_string(), "b-slice".to_string()]);
    }

    #[test]
    fn session_round_trips_through_disk() {
        use crate::session::{SessionId, SessionRecord};
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());

        let s = SessionRecord::impl_(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "impl/checkout-auth",
        );
        ws.save_session(&s).unwrap();
        assert_eq!(ws.load_session(&s.id.to_string()).unwrap(), s);
    }

    #[test]
    fn load_local_defaults_when_absent_and_round_trips_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        // Absent => default.
        assert_eq!(ws.load_local().unwrap(), LocalConfig::default());
        // Present => round-trips.
        let c = LocalConfig {
            worktrees_dir: Some(std::path::PathBuf::from("/tmp/wt")),
        };
        save_toml(&ws.local_path(), &c).unwrap();
        assert_eq!(ws.load_local().unwrap(), c);
    }

    #[test]
    fn list_sessions_is_sorted_and_empty_when_absent() {
        use crate::session::{SessionId, SessionRecord};
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());
        assert!(ws.list_sessions().unwrap().is_empty());

        // Spec sessions exercise the all-`None` options serialization path too.
        let a = SessionRecord::spec(SessionId::generate());
        let b = SessionRecord::spec(SessionId::generate());
        ws.save_session(&a).unwrap();
        ws.save_session(&b).unwrap();

        let got = ws.list_sessions().unwrap();
        assert_eq!(got.len(), 2);

        let mut expected_ids = vec![a.id.to_string(), b.id.to_string()];
        expected_ids.sort();
        let got_ids: Vec<String> = got.iter().map(|s| s.id.to_string()).collect();
        assert_eq!(got_ids, expected_ids);
    }

    #[test]
    fn projection_round_trips_through_disk_and_exists_flips() {
        use crate::model::projection::{Component, SystemProjection};
        use crate::layer::Layer;
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::new(dir.path());

        assert!(!ws.projection_exists("checkout"));

        let mut p = SystemProjection::new("checkout");
        p.component.push(Component { name: "billing".into(), layer: Layer::Domain });
        ws.save_projection(&p).unwrap();

        assert!(ws.projection_exists("checkout"));
        assert_eq!(ws.load_projection("checkout").unwrap(), p);
    }
}
