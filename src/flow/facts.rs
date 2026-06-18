//! Plain delivery facts consumed by the pure stage machine. No IO lives here;
//! the git/forge adapters populate these and `derive_stage` reads them (§5.2).

/// Git-derived facts about a session's branch relative to the base branch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BranchFacts {
    /// The branch ref exists in git.
    pub exists: bool,
    /// `git rev-list base..branch` — informational rail decoration, not a stage gate.
    pub commits_ahead_of_base: usize,
    /// Non-empty diff vs the merge-base — the Project -> Implement boundary.
    pub has_substantive_changes: bool,
    /// The branch tip is an ancestor of base (git-only, offline-derivable).
    pub merged_into_base: bool,
}

/// Review state of a session's branch, from the forge OR a local checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState {
    /// No PR / checkpoint exists — a *known* fact (distinct from undeterminable).
    None,
    /// A PR (or `self-review` checkpoint) is open.
    Open,
    /// PR open, reviewer requested changes — ball is back with the developer.
    ChangesRequested,
    /// Approved / mergeable, not yet landed.
    Approved,
    /// Merged via the forge.
    Merged,
    /// Closed without merging.
    Closed,
}

/// Everything `derive_stage` needs. `review` is `Option` so "undeterminable"
/// (forge unreachable AND no checkpoint) is distinct from a known `None` — the
/// determinism-honesty primitive (§5.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeliveryFacts {
    pub branch: BranchFacts,
    /// `Some(state)` when known; `None` when review state is undeterminable.
    pub review: Option<ReviewState>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_facts_default_is_empty() {
        let b = BranchFacts::default();
        assert!(!b.exists);
        assert_eq!(b.commits_ahead_of_base, 0);
        assert!(!b.has_substantive_changes);
        assert!(!b.merged_into_base);
    }

    #[test]
    fn delivery_facts_default_has_undeterminable_review() {
        let d = DeliveryFacts::default();
        assert_eq!(d.branch, BranchFacts::default());
        assert_eq!(d.review, None);
    }

    #[test]
    fn known_no_pr_differs_from_undeterminable() {
        let known_no_pr: Option<ReviewState> = Some(ReviewState::None);
        let undeterminable: Option<ReviewState> = None;
        assert_ne!(known_no_pr, undeterminable);
    }
}
