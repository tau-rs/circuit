use serde::{Deserialize, Serialize};

/// `.circuit/specs/<id>.toml` — a spec session's authored intent.
/// A spec session writes no application code; it owns the DAG.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecRecord {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub intent: String,
    #[serde(default)]
    pub bounded_contexts: Vec<String>,
}

impl SpecRecord {
    /// Construct a v1 spec record.
    pub fn new(id: impl Into<String>, title: impl Into<String>, intent: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            id: id.into(),
            title: title.into(),
            intent: intent.into(),
            bounded_contexts: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let mut s = SpecRecord::new("checkout", "Checkout & payment", "Let a customer pay.");
        s.bounded_contexts = vec!["billing".to_string(), "cart".to_string()];
        let text = toml::to_string_pretty(&s).unwrap();
        let parsed: SpecRecord = toml::from_str(&text).unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn bounded_contexts_default_to_empty() {
        let text = r#"
            schema_version = 1
            id = "checkout"
            title = "Checkout"
            intent = "Pay for a basket."
        "#;
        let s: SpecRecord = toml::from_str(text).unwrap();
        assert!(s.bounded_contexts.is_empty());
    }
}
