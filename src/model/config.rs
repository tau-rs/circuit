use serde::{Deserialize, Serialize};

/// Enforcement-rigor tier. Authored now; the rigor consumer is M3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Full,
    Light,
    Cli,
}

/// Project capabilities. Authored now; gating consumers (e.g. UI-match) are M3.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub has_ui: bool,
}

/// `.circuit/config.toml`. The `base_branch` field is the one live M2 consumer
/// (stage derivation needs it for merge-base / rev-list).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    pub tier: Tier,
    pub base_branch: String,
    #[serde(default)]
    pub capabilities: Capabilities,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tier: Tier::Full,
            base_branch: "main".to_string(),
            capabilities: Capabilities::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_toml() {
        let c = Config::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn parses_a_hand_authored_file() {
        let text = r#"
            schema_version = 1
            tier = "light"
            base_branch = "develop"

            [capabilities]
            has_ui = true
        "#;
        let c: Config = toml::from_str(text).unwrap();
        assert_eq!(c.tier, Tier::Light);
        assert_eq!(c.base_branch, "develop");
        assert!(c.capabilities.has_ui);
    }

    #[test]
    fn capabilities_default_when_omitted() {
        let text = "schema_version = 1\ntier = \"full\"\nbase_branch = \"main\"\n";
        let c: Config = toml::from_str(text).unwrap();
        assert!(!c.capabilities.has_ui);
    }
}
