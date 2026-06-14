#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Domain,
    Application,
    Adapter,
    Unknown,
}

/// Assign a layer to a top-level module name by convention.
pub fn layer_of(module: &str) -> Layer {
    match module {
        "domain" | "entities" | "model" => Layer::Domain,
        "application" | "app" | "usecase" | "usecases" | "use_cases" => Layer::Application,
        "adapters" | "adapter" | "infra" | "infrastructure" | "persistence" | "cli" | "render"
        | "lang" => Layer::Adapter,
        _ => Layer::Unknown,
    }
}

/// Inward-ness rank: lower = more inner. `None` means "unranked" (skip in rules).
pub fn rank(layer: Layer) -> Option<u8> {
    match layer {
        Layer::Domain => Some(1),
        Layer::Application => Some(2),
        Layer::Adapter => Some(3),
        Layer::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_names_to_layers() {
        assert_eq!(layer_of("domain"), Layer::Domain);
        assert_eq!(layer_of("usecase"), Layer::Application);
        assert_eq!(layer_of("adapters"), Layer::Adapter);
        assert_eq!(layer_of("render"), Layer::Adapter);
    }

    #[test]
    fn unknown_names_are_unknown() {
        assert_eq!(layer_of("graph"), Layer::Unknown);
        assert_eq!(layer_of("widgets"), Layer::Unknown);
    }

    #[test]
    fn rank_orders_inner_below_outer() {
        assert!(rank(Layer::Domain) < rank(Layer::Adapter));
        assert_eq!(rank(Layer::Unknown), None);
    }
}
