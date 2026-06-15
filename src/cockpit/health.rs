//! Health roll-up. Health is derived (run M1's indicators against a branch's
//! worktree), never stored (§9). This module is the pure roll-up logic only.

/// Session health. `Ord` is derived from declaration order, so the ascending
/// chain `Sound < Warn < Critical < Unknown` makes `Unknown` sort highest;
/// `children.max()` then lets an unmeasurable child dominate a spec's roll-up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    /// No violations.
    Sound,
    /// Reserved: no advisory-only sub-indicator produces this yet.
    Warn,
    /// At least one cycle or dependency-rule violation.
    Critical,
    /// Unmeasurable (e.g. no worktree to run indicators against).
    Unknown,
}

/// The two M1 indicator counts for one impl session's worktree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionHealth {
    pub cycles: usize,
    pub dep_violations: usize,
}

impl SessionHealth {
    /// Impl-session roll-up: any cycle or dependency-rule violation => Critical,
    /// else Sound. Never yields Unknown (the adapter supplies that when a
    /// worktree is absent) and never Warn (no producer yet).
    pub fn rollup(&self) -> Health {
        if self.cycles > 0 || self.dep_violations > 0 {
            Health::Critical
        } else {
            Health::Sound
        }
    }
}

/// Spec-session roll-up: worst-of-children = `children.max()`. Empty => Sound
/// (a spec with no children is vacuously sound); an `Unknown` child dominates.
pub fn rollup_children(children: &[Health]) -> Health {
    children.iter().copied().max().unwrap_or(Health::Sound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_sorts_highest() {
        assert!(Health::Unknown > Health::Critical);
        assert!(Health::Critical > Health::Warn);
        assert!(Health::Warn > Health::Sound);
    }

    #[test]
    fn rollup_is_sound_when_no_violations() {
        assert_eq!(SessionHealth::default().rollup(), Health::Sound);
    }

    #[test]
    fn rollup_is_critical_on_any_violation() {
        assert_eq!(
            SessionHealth {
                cycles: 1,
                dep_violations: 0
            }
            .rollup(),
            Health::Critical
        );
        assert_eq!(
            SessionHealth {
                cycles: 0,
                dep_violations: 3
            }
            .rollup(),
            Health::Critical
        );
    }

    #[test]
    fn rollup_children_empty_is_sound() {
        assert_eq!(rollup_children(&[]), Health::Sound);
    }

    #[test]
    fn rollup_children_takes_the_worst() {
        assert_eq!(
            rollup_children(&[Health::Sound, Health::Critical, Health::Sound]),
            Health::Critical
        );
    }

    #[test]
    fn rollup_children_unknown_dominates_critical() {
        assert_eq!(
            rollup_children(&[Health::Sound, Health::Critical, Health::Unknown]),
            Health::Unknown
        );
    }
}
