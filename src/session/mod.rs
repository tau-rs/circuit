//! Session identity and the authored session record (`.circuit/sessions/<id>.toml`).
//! Pure: serde + a single clock-reading id generator, nothing else.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// A session's stable, ULID-style identity. It **precedes the branch**: a session
/// exists at `Draft` before any branch is cut, which is why the branch name
/// cannot be the identity (§4 of the M2 design).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(Ulid);

impl SessionId {
    /// Mint a fresh id. This is the ONLY clock-reading call in the foundation
    /// slice; the impurity is isolated here so everything else stays pure.
    pub fn generate() -> Self {
        Self(Ulid::new())
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Canonical 26-char Crockford base32 form.
        write!(f, "{}", self.0)
    }
}

impl FromStr for SessionId {
    type Err = ulid::DecodeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ulid::from_string(s).map(Self)
    }
}

/// The three session kinds (the fractal model of §4.2). Serializes lowercase
/// (`"spec" | "impl" | "fix"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Spec,
    Impl,
    Fix,
}

/// `.circuit/sessions/<id>.toml` — a session's authored intent. Only intent is
/// stored: no stage, no worktree path, no branch *state* (all derived, §3.3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub schema_version: u32,
    pub id: SessionId,
    pub kind: SessionKind,
    /// Spec id (impl/fix sessions); `None` for a spec session.
    #[serde(default)]
    pub parent: Option<String>,
    /// DAG node id this session executes (impl/fix); `None` for a spec session.
    #[serde(default)]
    pub dag_node: Option<String>,
    /// Authored branch bridge; `None` until spawned (a Draft session, or a spec
    /// session, owns no branch). The worktree path is never stored.
    #[serde(default)]
    pub branch: Option<String>,
    /// For fix sessions: the non-green sub-indicator this session targets.
    #[serde(default)]
    pub fixes_indicator: Option<String>,
}

impl SessionRecord {
    /// A spec session: owns the DAG, writes no code, has no branch.
    pub fn spec(id: SessionId) -> Self {
        Self {
            schema_version: 1,
            id,
            kind: SessionKind::Spec,
            parent: None,
            dag_node: None,
            branch: None,
            fixes_indicator: None,
        }
    }

    /// An implementation session executing one DAG node on its own branch.
    pub fn impl_(
        id: SessionId,
        parent: impl Into<String>,
        dag_node: impl Into<String>,
        branch: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            id,
            kind: SessionKind::Impl,
            parent: Some(parent.into()),
            dag_node: Some(dag_node.into()),
            branch: Some(branch.into()),
            fixes_indicator: None,
        }
    }

    /// A fix session: a scoped child targeting one non-green sub-indicator.
    pub fn fix(
        id: SessionId,
        parent: impl Into<String>,
        dag_node: impl Into<String>,
        branch: impl Into<String>,
        fixes_indicator: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            id,
            kind: SessionKind::Fix,
            parent: Some(parent.into()),
            dag_node: Some(dag_node.into()),
            branch: Some(branch.into()),
            fixes_indicator: Some(fixes_indicator.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A canonical, valid 26-char ULID for hand-authored parse tests.
    const SAMPLE_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    #[test]
    fn session_id_round_trips_through_string() {
        let id = SessionId::generate();
        let s = id.to_string();
        assert_eq!(s.len(), 26);
        let parsed: SessionId = s.parse().unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn session_id_rejects_an_invalid_string() {
        assert!("not-a-ulid".parse::<SessionId>().is_err());
    }

    #[test]
    fn spec_session_has_no_parent_dag_node_or_branch() {
        let s = SessionRecord::spec(SessionId::generate());
        assert_eq!(s.kind, SessionKind::Spec);
        assert!(s.parent.is_none());
        assert!(s.dag_node.is_none());
        assert!(s.branch.is_none());
        assert!(s.fixes_indicator.is_none());
    }

    #[test]
    fn impl_session_round_trips_through_toml() {
        let s = SessionRecord::impl_(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "impl/checkout-auth",
        );
        let text = toml::to_string_pretty(&s).unwrap();
        let parsed: SessionRecord = toml::from_str(&text).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn fix_session_records_its_indicator() {
        let s = SessionRecord::fix(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "fix/checkout-auth-cycles",
            "cycles",
        );
        assert_eq!(s.kind, SessionKind::Fix);
        assert_eq!(s.fixes_indicator.as_deref(), Some("cycles"));
    }

    #[test]
    fn parses_a_hand_authored_impl_session() {
        let text = format!(
            r#"
            schema_version = 1
            id = "{SAMPLE_ULID}"
            kind = "impl"
            parent = "checkout"
            dag_node = "auth-slice"
            branch = "impl/checkout-auth"
            "#
        );
        let s: SessionRecord = toml::from_str(&text).unwrap();
        assert_eq!(s.kind, SessionKind::Impl);
        assert_eq!(s.parent.as_deref(), Some("checkout"));
        assert_eq!(s.branch.as_deref(), Some("impl/checkout-auth"));
        assert!(s.fixes_indicator.is_none());
    }

    #[test]
    fn parses_a_spec_session_with_options_omitted() {
        let text = format!(
            "schema_version = 1\nid = \"{SAMPLE_ULID}\"\nkind = \"spec\"\n"
        );
        let s: SessionRecord = toml::from_str(&text).unwrap();
        assert_eq!(s.kind, SessionKind::Spec);
        assert!(s.parent.is_none());
        assert!(s.branch.is_none());
    }
}
