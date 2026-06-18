//! Delivery-mode selection: forge (GitHub via `gh`) vs local checkpoints. The
//! decision is a pure function of two detected facts so it is unit-testable
//! without shelling out; detection itself lives in the CLI (§7.1). Resolved
//! once per `circuit flow` run and applied repo-wide.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryMode {
    Forge,
    Local,
}

/// Forge iff `gh` is usable AND the repo has a GitHub remote; else Local.
pub fn resolve(gh_available: bool, has_github_remote: bool) -> DeliveryMode {
    if gh_available && has_github_remote {
        DeliveryMode::Forge
    } else {
        DeliveryMode::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_and_remote_selects_forge() {
        assert_eq!(resolve(true, true), DeliveryMode::Forge);
    }

    #[test]
    fn no_gh_selects_local() {
        assert_eq!(resolve(false, true), DeliveryMode::Local);
    }

    #[test]
    fn no_remote_selects_local() {
        assert_eq!(resolve(true, false), DeliveryMode::Local);
    }

    #[test]
    fn neither_selects_local() {
        assert_eq!(resolve(false, false), DeliveryMode::Local);
    }
}
