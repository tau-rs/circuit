use serde::{Deserialize, Serialize};

use crate::layer::Layer;

/// `.circuit/projections/<spec-id>.toml` — a spec session's system-level
/// projection: the intended architecture, context map, and inter-slice
/// contracts. Authored intent only; never diffed against code in this slice
/// (that is M3 slice C).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemProjection {
    pub schema_version: u32,
    /// Spec session id this projection belongs to (FK → `SpecRecord.id`).
    pub spec: String,
    #[serde(default)]
    pub component: Vec<Component>,
    #[serde(default)]
    pub edge: Vec<IntendedEdge>,
    #[serde(default)]
    pub context: Vec<Context>,
    #[serde(default)]
    pub relationship: Vec<Relationship>,
    #[serde(default)]
    pub contract: Vec<Contract>,
}

/// An intended module/component and the layer it is meant to live in. `layer`
/// reuses M1's `Layer` so slice C can diff projected layers against derived ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    pub name: String,
    pub layer: Layer,
    /// Which derived code module realizes this component (top-level module name,
    /// e.g. "model"). `None` ⇒ join on `name`. The design name and the code
    /// module live in different namespaces, so the link must be declared.
    #[serde(default)]
    pub module: Option<String>,
}

impl Component {
    /// The derived-graph module name this component joins to.
    pub fn effective_module(&self) -> &str {
        self.module.as_deref().unwrap_or(&self.name)
    }
}

/// An intended (allowed) dependency edge. Slice C diffs code edges against these.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntendedEdge {
    pub from: String,
    pub to: String,
}

/// A bounded context in the context map.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub name: String,
}

/// A directed relationship between two contexts. `kind` is a free string
/// (e.g. "customer-supplier", "conformist", "acl"), NOT a closed enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship {
    pub upstream: String,
    pub downstream: String,
    pub kind: String,
}

/// A named inter-slice contract (a port one context provides to others).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub consumers: Vec<String>,
}

impl SystemProjection {
    /// A v1 skeleton: identity only, all sections empty.
    pub fn new(spec: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            spec: spec.into(),
            component: Vec::new(),
            edge: Vec::new(),
            context: Vec::new(),
            relationship: Vec::new(),
            contract: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> SystemProjection {
        SystemProjection {
            schema_version: 1,
            spec: "checkout".into(),
            component: vec![
                Component { name: "billing".into(), layer: Layer::Domain, module: None },
                Component { name: "gh-adapter".into(), layer: Layer::Adapter, module: None },
            ],
            edge: vec![IntendedEdge {
                from: "gh-adapter".into(),
                to: "billing".into(),
            }],
            context: vec![
                Context {
                    name: "checkout".into(),
                },
                Context {
                    name: "payments".into(),
                },
            ],
            relationship: vec![Relationship {
                upstream: "payments".into(),
                downstream: "checkout".into(),
                kind: "customer-supplier".into(),
            }],
            contract: vec![Contract {
                name: "PaymentGateway".into(),
                provider: "payments".into(),
                consumers: vec!["checkout".into()],
            }],
        }
    }

    #[test]
    fn full_projection_round_trips_through_toml() {
        let p = populated();
        let text = toml::to_string_pretty(&p).unwrap();
        let parsed: SystemProjection = toml::from_str(&text).unwrap();
        assert_eq!(parsed, p);
    }

    #[test]
    fn skeleton_round_trips_with_empty_sections() {
        let p = SystemProjection::new("checkout");
        let text = toml::to_string_pretty(&p).unwrap();
        let parsed: SystemProjection = toml::from_str(&text).unwrap();
        assert_eq!(parsed, p);
        assert!(parsed.component.is_empty());
        assert!(parsed.contract.is_empty());
    }

    #[test]
    fn hand_authored_toml_with_sections_omitted_parses() {
        let text = r#"
            schema_version = 1
            spec = "checkout"
        "#;
        let p: SystemProjection = toml::from_str(text).unwrap();
        assert_eq!(p.spec, "checkout");
        assert!(p.component.is_empty());
        assert!(p.edge.is_empty());
        assert!(p.context.is_empty());
        assert!(p.relationship.is_empty());
        assert!(p.contract.is_empty());
    }

    #[test]
    fn effective_module_uses_module_then_falls_back_to_name() {
        let mapped = Component { name: "billing".into(), layer: Layer::Domain, module: Some("model".into()) };
        assert_eq!(mapped.effective_module(), "model");

        let unmapped = Component { name: "cart".into(), layer: Layer::Domain, module: None };
        assert_eq!(unmapped.effective_module(), "cart");
    }

    #[test]
    fn component_without_module_key_parses_and_defaults_to_none() {
        // A Slice A projection has no `module` key on its components.
        let text = r#"
            schema_version = 1
            spec = "checkout"
            [[component]]
            name = "billing"
            layer = "domain"
        "#;
        let p: SystemProjection = toml::from_str(text).unwrap();
        assert_eq!(p.component[0].module, None);
        assert_eq!(p.component[0].effective_module(), "billing");
    }
}
