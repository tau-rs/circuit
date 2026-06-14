//! The pure flow-stage derivation. Stage is never stored; it is a function of
//! delivery facts (§5). Forge-gated stages are reported honestly: when review
//! state is undeterminable we return the git-floor stage with `forge_certain`
//! false rather than faking Review/Merge/Done (§5.3).

use crate::flow::facts::{DeliveryFacts, ReviewState};
use crate::session::SessionRecord;

/// The six-stage flow spine. No `Unknown` variant: stage uncertainty lives in
/// `StageView.forge_certain`, keeping `Stage` the clean spine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Draft,
    Project,
    Implement,
    Review,
    Merge,
    Done,
}

/// A derived stage plus whether forge-gated refinement could be confirmed.
/// `forge_certain == false` means "at least `stage`, but the Review/Merge/Done
/// refinement is undeterminable" — we never fake a forge-gated verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageView {
    pub stage: Stage,
    pub forge_certain: bool,
}

impl StageView {
    fn certain(stage: Stage) -> Self {
        Self {
            stage,
            forge_certain: true,
        }
    }
}

/// Derive a session's flow stage from delivery facts. Pure; never stored.
///
/// `_session` is reserved: M3's projection-approved marker will refine the
/// Project -> Implement gate using session/projection state (§5.1). Unused in M2.
pub fn derive_stage(_session: &SessionRecord, facts: &DeliveryFacts) -> StageView {
    let b = &facts.branch;

    // 1. No branch in git yet -> Draft (git-settled).
    if !b.exists {
        return StageView::certain(Stage::Draft);
    }
    // 2. Merged into base -> Done (git-only, offline-confident).
    if b.merged_into_base {
        return StageView::certain(Stage::Done);
    }
    // 3. Branch exists but no substantive commits -> Project.
    if !b.has_substantive_changes {
        return StageView::certain(Stage::Project);
    }
    // 4-9. Substantive changes, not merged: refine by review state.
    match facts.review {
        // Undeterminable: honest git-floor Implement, forge refinement Unknown.
        None => StageView {
            stage: Stage::Implement,
            forge_certain: false,
        },
        Some(ReviewState::None) => StageView::certain(Stage::Implement),
        Some(ReviewState::Open) => StageView::certain(Stage::Review),
        Some(ReviewState::Approved) => StageView::certain(Stage::Merge),
        Some(ReviewState::Merged) => StageView::certain(Stage::Done),
        Some(ReviewState::Closed) => StageView::certain(Stage::Implement),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::facts::BranchFacts;
    use crate::session::SessionId;

    fn session() -> SessionRecord {
        SessionRecord::impl_(
            SessionId::generate(),
            "checkout",
            "auth-slice",
            "impl/checkout-auth",
        )
    }

    /// Build facts from the four axes that drive the truth table.
    fn facts(
        exists: bool,
        substantive: bool,
        merged: bool,
        review: Option<ReviewState>,
    ) -> DeliveryFacts {
        DeliveryFacts {
            branch: BranchFacts {
                exists,
                // Not a stage gate (rail decoration only); fixed so these tests
                // never couple it to `has_substantive_changes`.
                commits_ahead_of_base: 0,
                has_substantive_changes: substantive,
                merged_into_base: merged,
            },
            review,
        }
    }

    // Row 1
    #[test]
    fn no_branch_is_draft() {
        let v = derive_stage(&session(), &facts(false, false, false, None));
        assert_eq!(v, StageView { stage: Stage::Draft, forge_certain: true });
    }

    // Row 2 — git-only, even with undeterminable review.
    #[test]
    fn merged_is_done_even_offline() {
        let v = derive_stage(&session(), &facts(true, true, true, None));
        assert_eq!(v, StageView { stage: Stage::Done, forge_certain: true });
    }

    // Row 3
    #[test]
    fn branch_without_substantive_changes_is_project() {
        let v = derive_stage(&session(), &facts(true, false, false, Some(ReviewState::None)));
        assert_eq!(v, StageView { stage: Stage::Project, forge_certain: true });
    }

    // Row 4 — the one honest-Unknown case.
    #[test]
    fn substantive_with_undeterminable_review_is_implement_uncertain() {
        let v = derive_stage(&session(), &facts(true, true, false, None));
        assert_eq!(v, StageView { stage: Stage::Implement, forge_certain: false });
    }

    // Row 5
    #[test]
    fn substantive_no_pr_is_implement() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::None)));
        assert_eq!(v, StageView { stage: Stage::Implement, forge_certain: true });
    }

    // Row 6
    #[test]
    fn open_pr_is_review() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::Open)));
        assert_eq!(v, StageView { stage: Stage::Review, forge_certain: true });
    }

    // Row 7
    #[test]
    fn approved_pr_is_merge() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::Approved)));
        assert_eq!(v, StageView { stage: Stage::Merge, forge_certain: true });
    }

    // Row 8
    #[test]
    fn forge_merged_is_done() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::Merged)));
        assert_eq!(v, StageView { stage: Stage::Done, forge_certain: true });
    }

    // Row 9
    #[test]
    fn closed_pr_is_implement() {
        let v = derive_stage(&session(), &facts(true, true, false, Some(ReviewState::Closed)));
        assert_eq!(v, StageView { stage: Stage::Implement, forge_certain: true });
    }
}
