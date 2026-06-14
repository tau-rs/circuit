use serde::{Deserialize, Serialize};

/// `.circuit/dag/<id>.toml` — one DAG node = one vertical slice.
/// `branch` is the authored bridge to git (the worktree path is never stored).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagNode {
    pub schema_version: u32,
    pub id: String,
    pub spec: String,
    pub title: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub branch: String,
}

impl DagNode {
    /// Construct a v1 DAG node with no dependencies and an empty intent.
    pub fn new(
        id: impl Into<String>,
        spec: impl Into<String>,
        title: impl Into<String>,
        branch: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            id: id.into(),
            spec: spec.into(),
            title: title.into(),
            intent: String::new(),
            depends_on: Vec::new(),
            branch: branch.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let mut n = DagNode::new("auth-slice", "checkout", "Authentication slice", "impl/checkout-auth");
        n.depends_on = vec!["cart-slice".to_string()];
        n.intent = "Log in and gate checkout.".to_string();
        let text = toml::to_string_pretty(&n).unwrap();
        let parsed: DagNode = toml::from_str(&text).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn depends_on_and_intent_default_when_omitted() {
        let text = r#"
            schema_version = 1
            id = "auth-slice"
            spec = "checkout"
            title = "Auth"
            branch = "impl/checkout-auth"
        "#;
        let n: DagNode = toml::from_str(text).unwrap();
        assert!(n.depends_on.is_empty());
        assert_eq!(n.intent, "");
    }
}
