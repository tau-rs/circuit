//! The per-session flow rail: a colorless text render of the six-stage spine
//! with the current stage marked, plus a branch-facts line (§8.1).

use crate::cockpit::health::Health;
use crate::flow::facts::{BranchFacts, ReviewState};
use crate::flow::stage::{Stage, StageView};
use crate::session::SessionKind;

/// The six-stage spine, in order.
const SPINE: [Stage; 6] = [
    Stage::Draft,
    Stage::Project,
    Stage::Implement,
    Stage::Review,
    Stage::Merge,
    Stage::Done,
];

/// Line-2 indent: aligns the branch-facts line under the spine on line 1.
const LINE2_INDENT: &str = "            "; // 12 spaces

fn stage_label(s: Stage) -> &'static str {
    match s {
        Stage::Draft => "Draft",
        Stage::Project => "Project",
        Stage::Implement => "Implement",
        Stage::Review => "Review",
        Stage::Merge => "Merge",
        Stage::Done => "Done",
    }
}

fn kind_label(k: SessionKind) -> &'static str {
    match k {
        SessionKind::Spec => "spec",
        SessionKind::Impl => "impl",
        SessionKind::Fix => "fix",
    }
}

fn health_glyph(h: Health) -> &'static str {
    match h {
        Health::Sound => "●",
        Health::Warn => "◐",
        Health::Critical => "◍",
        Health::Unknown => "?",
    }
}

fn review_label(r: Option<ReviewState>) -> &'static str {
    match r {
        None => "PR ?",
        Some(ReviewState::None) => "no PR",
        Some(ReviewState::Open) => "PR open",
        Some(ReviewState::ChangesRequested) => "PR changes requested",
        Some(ReviewState::Approved) => "PR approved",
        Some(ReviewState::Merged) => "PR merged",
        Some(ReviewState::Closed) => "PR closed",
    }
}

/// Render one session's rail. Pure; colorless (§8). `review = None` means the
/// forge state is undeterminable (printed `PR ?`), distinct from a known `no PR`.
/// `health` is always rendered from its glyph; this slice passes `Unknown`.
pub fn render_rail(
    node_id: &str,
    kind: SessionKind,
    view: StageView,
    branch: Option<&str>,
    facts: &BranchFacts,
    review: Option<ReviewState>,
    health: Health,
) -> String {
    // Spine: current stage wrapped in guillemets, joined by " › ".
    let spine = SPINE
        .iter()
        .map(|&s| {
            if s == view.stage {
                format!("‹{}›", stage_label(s))
            } else {
                stage_label(s).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" › ");

    let uncertain = if view.forge_certain {
        ""
    } else {
        "  (forge state unknown)"
    };

    let line1 = format!("{node_id}  [{}]  {spine}{uncertain}", kind_label(kind));

    let line2 = match branch {
        Some(name) => format!(
            "{LINE2_INDENT}branch {name} · {} commits · {} · health {}",
            facts.commits_ahead_of_base,
            review_label(review),
            health_glyph(health),
        ),
        None => format!("{LINE2_INDENT}no branch · health {}", health_glyph(health)),
    };

    format!("{line1}\n{line2}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(commits: usize) -> BranchFacts {
        BranchFacts {
            exists: true,
            commits_ahead_of_base: commits,
            has_substantive_changes: commits > 0,
            merged_into_base: false,
        }
    }

    #[test]
    fn marks_the_current_stage_with_guillemets() {
        let view = StageView {
            stage: Stage::Project,
            forge_certain: true,
        };
        let out = render_rail(
            "auth-slice",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(0),
            Some(ReviewState::None),
            Health::Sound,
        );
        assert!(out.contains("‹Project›"), "got: {out}");
        // Other stages are unmarked.
        assert!(out.contains(" Draft "));
        assert!(out.contains("Done"));
        assert!(!out.contains("‹Draft›"));
    }

    #[test]
    fn line_one_has_node_id_and_kind() {
        let view = StageView {
            stage: Stage::Implement,
            forge_certain: true,
        };
        let out = render_rail(
            "auth-slice",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(3),
            Some(ReviewState::None),
            Health::Critical,
        );
        let line1 = out.lines().next().unwrap();
        assert!(line1.starts_with("auth-slice  [impl]"));
    }

    #[test]
    fn undeterminable_review_prints_pr_question_mark() {
        let view = StageView {
            stage: Stage::Implement,
            forge_certain: false,
        };
        let out = render_rail(
            "auth-slice",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(3),
            None,
            Health::Unknown,
        );
        assert!(out.contains("PR ?"), "got: {out}");
        assert!(out.contains("(forge state unknown)"), "got: {out}");
        assert!(out.contains("health ?"), "got: {out}");
    }

    #[test]
    fn known_no_pr_differs_from_undeterminable() {
        let view = StageView {
            stage: Stage::Implement,
            forge_certain: true,
        };
        let out = render_rail(
            "a",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(1),
            Some(ReviewState::None),
            Health::Unknown,
        );
        assert!(out.contains("no PR"));
        assert!(!out.contains("PR ?"));
    }

    #[test]
    fn commit_count_and_branch_name_appear() {
        let view = StageView {
            stage: Stage::Implement,
            forge_certain: true,
        };
        let out = render_rail(
            "a",
            SessionKind::Impl,
            view,
            Some("impl/checkout-auth"),
            &facts(3),
            Some(ReviewState::None),
            Health::Sound,
        );
        assert!(out.contains("branch impl/checkout-auth"));
        assert!(out.contains("3 commits"));
        assert!(out.contains("health ●"));
    }

    #[test]
    fn spec_session_without_branch_renders_no_branch() {
        let view = StageView {
            stage: Stage::Draft,
            forge_certain: true,
        };
        let out = render_rail(
            "checkout",
            SessionKind::Spec,
            view,
            None,
            &BranchFacts::default(),
            None,
            Health::Unknown,
        );
        assert!(out.contains("no branch"));
        assert!(!out.contains("commits"));
    }

    #[test]
    fn rail_contains_no_ansi_color_codes() {
        let view = StageView {
            stage: Stage::Review,
            forge_certain: true,
        };
        let out = render_rail(
            "a",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(2),
            Some(ReviewState::Open),
            Health::Sound,
        );
        // The colorless invariant (§8): no ESC byte anywhere.
        assert!(!out.contains('\u{1b}'), "rail must be colorless");
    }

    #[test]
    fn spine_lists_all_six_stages_and_stays_in_sync_with_stage() {
        // Exhaustive match: if `Stage` gains a variant this fails to compile,
        // forcing both `stage_label` and `SPINE` to be updated together.
        for s in SPINE {
            match s {
                Stage::Draft
                | Stage::Project
                | Stage::Implement
                | Stage::Review
                | Stage::Merge
                | Stage::Done => {}
            }
        }
        assert_eq!(SPINE.len(), 6, "SPINE must list all six stages");
    }

    #[test]
    fn changes_requested_renders_its_own_label() {
        let view = StageView {
            stage: Stage::Review,
            forge_certain: true,
        };
        let out = render_rail(
            "a",
            SessionKind::Impl,
            view,
            Some("impl/x"),
            &facts(2),
            Some(ReviewState::ChangesRequested),
            Health::Sound,
        );
        assert!(out.contains("PR changes requested"), "got: {out}");
    }

    #[test]
    fn renders_fix_kind_and_done_as_current_stage() {
        let view = StageView {
            stage: Stage::Done,
            forge_certain: true,
        };
        let out = render_rail(
            "auth-slice",
            SessionKind::Fix,
            view,
            Some("fix/x"),
            &facts(5),
            Some(ReviewState::Merged),
            Health::Sound,
        );
        assert!(out.contains("[fix]"), "got: {out}");
        assert!(out.contains("‹Done›"), "got: {out}");
        assert!(out.contains("PR merged"));
    }
}
