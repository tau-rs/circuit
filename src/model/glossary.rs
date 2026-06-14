use serde::{Deserialize, Serialize};

/// A single ubiquitous-language term.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Term {
    pub name: String,
    pub definition: String,
}

/// `.circuit/glossary.toml`. Authored now; the naming-indicator consumer is M3.
/// On disk each term is a `[[term]]` array-of-tables entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Glossary {
    pub schema_version: u32,
    #[serde(default, rename = "term")]
    pub terms: Vec<Term>,
}

impl Default for Glossary {
    fn default() -> Self {
        Self {
            schema_version: 1,
            terms: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips_through_toml() {
        let g = Glossary::default();
        let text = toml::to_string_pretty(&g).unwrap();
        let parsed: Glossary = toml::from_str(&text).unwrap();
        assert_eq!(parsed, g);
    }

    #[test]
    fn populated_round_trips_through_toml() {
        let mut g = Glossary::default();
        g.terms.push(Term {
            name: "Order".into(),
            definition: "A customer's confirmed basket.".into(),
        });
        let text = toml::to_string_pretty(&g).unwrap();
        // The serde rename must be honoured on write: `[[term]]`, not `[[terms]]`.
        assert!(text.contains("[[term]]"), "expected [[term]], got:\n{text}");
        let parsed: Glossary = toml::from_str(&text).unwrap();
        assert_eq!(parsed, g);
    }

    #[test]
    fn parses_terms_as_array_of_tables() {
        let text = r#"
            schema_version = 1

            [[term]]
            name = "Order"
            definition = "A customer's confirmed basket, billed as one unit."

            [[term]]
            name = "Cart"
            definition = "A mutable basket before checkout."
        "#;
        let g: Glossary = toml::from_str(text).unwrap();
        assert_eq!(g.terms.len(), 2);
        assert_eq!(g.terms[0].name, "Order");
        assert_eq!(g.terms[1].name, "Cart");
    }

    #[test]
    fn terms_default_to_empty_when_omitted() {
        let g: Glossary = toml::from_str("schema_version = 1\n").unwrap();
        assert!(g.terms.is_empty());
    }
}
