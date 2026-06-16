//! `ForgePort` implemented by shelling out to the `gh` CLI (GitHub). Review
//! state comes from `gh pr view`; write actions wrap `gh pr create/merge/
//! update-branch` (Task 4). Forge-unreachable maps to the caller's `None`
//! (undeterminable) — never a fake verdict (§5).

use std::path::PathBuf;
use std::process::{Command, Output};

use thiserror::Error;

use crate::flow::facts::ReviewState;
use crate::ports::ForgePort;

/// Errors from shelling out to `gh`.
#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("failed to run gh (is it installed and on PATH?): {0}")]
    Spawn(#[source] std::io::Error),
    #[error("gh produced non-UTF8 output: {0}")]
    Utf8(#[source] std::string::FromUtf8Error),
    #[error("gh failed (exit {code}): {stderr}")]
    Command { code: String, stderr: String },
    #[error("could not parse gh output `{output}`: {reason}")]
    Parse { output: String, reason: String },
}

/// `ForgePort` over the `gh` CLI, rooted at a working tree. Commands run with
/// `current_dir(root)` so the adapter is independent of the process CWD.
pub struct Forge {
    root: PathBuf,
}

impl Forge {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Run a gh subcommand, capturing the full output for the caller to inspect.
    fn run(&self, args: &[&str]) -> Result<Output, ForgeError> {
        Command::new("gh")
            .current_dir(&self.root)
            .args(args)
            .output()
            .map_err(ForgeError::Spawn)
    }

    /// Run a gh subcommand that must succeed; discard stdout.
    fn run_checked(&self, args: &[&str]) -> Result<(), ForgeError> {
        let out = self.run(args)?;
        if !out.status.success() {
            return Err(ForgeError::Command {
                code: out
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(())
    }
}

/// Map a `gh pr view ... --jq '.state + "|" + (.reviewDecision // "")'` result
/// into a ReviewState. Exit-success with a parseable `STATE|DECISION` line =>
/// a concrete state. A non-zero exit whose stderr reports no PR => a *known*
/// `None`. Any other non-zero exit is undeterminable => Err (caller renders
/// `PR ?`). Pure — fully testable from canned (exit code, stdout, stderr).
/// `exit_code` is the process exit (`None` if killed by signal); success is
/// exactly `Some(0)`.
fn parse_review_state(
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Result<ReviewState, ForgeError> {
    if exit_code != Some(0) {
        let s = stderr.to_lowercase();
        if s.contains("no pull requests found") || s.contains("no pull request found") {
            return Ok(ReviewState::None);
        }
        return Err(ForgeError::Command {
            code: exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            stderr: stderr.trim().to_string(),
        });
    }
    let line = stdout.trim();
    let (state, decision) = line.split_once('|').ok_or_else(|| ForgeError::Parse {
        output: line.to_string(),
        reason: "expected `STATE|DECISION`".to_string(),
    })?;
    let review = match state {
        "MERGED" => ReviewState::Merged,
        "CLOSED" => ReviewState::Closed,
        // `decision` is gh's `reviewDecision`; the catch-all folds the
        // non-actionable values (`REVIEW_REQUIRED`, `DISMISSED`, null/"") into
        // a plain open PR. Only APPROVED / CHANGES_REQUESTED change the stage.
        "OPEN" => match decision {
            "APPROVED" => ReviewState::Approved,
            "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
            _ => ReviewState::Open,
        },
        other => {
            return Err(ForgeError::Parse {
                output: other.to_string(),
                reason: "unknown PR state".to_string(),
            })
        }
    };
    Ok(review)
}

impl ForgePort for Forge {
    type Error = ForgeError;

    fn review_state(&self, branch: &str) -> Result<ReviewState, ForgeError> {
        let out = self.run(&[
            "pr",
            "view",
            branch,
            "--json",
            "state,reviewDecision",
            "--jq",
            r#".state + "|" + (.reviewDecision // "")"#,
        ])?;
        let stdout = String::from_utf8(out.stdout).map_err(ForgeError::Utf8)?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        parse_review_state(out.status.code(), &stdout, &stderr)
    }

    fn create_pr(
        &self,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<(), ForgeError> {
        self.run_checked(&create_pr_args(branch, base, title, body))
    }

    fn merge(&self, branch: &str) -> Result<(), ForgeError> {
        self.run_checked(&merge_args(branch))
    }

    fn update_from_base(&self, branch: &str, base: &str) -> Result<(), ForgeError> {
        self.run_checked(&update_from_base_args(branch, base))
    }
}

/// Build the `gh` argv for opening a PR. Pure — asserted in tests.
fn create_pr_args<'a>(branch: &'a str, base: &'a str, title: &'a str, body: &'a str) -> Vec<&'a str> {
    vec![
        "pr", "create", "--head", branch, "--base", base, "--title", title, "--body", body,
    ]
}

/// Build the `gh` argv for merging a PR (merge-commit strategy).
fn merge_args(branch: &str) -> Vec<&str> {
    vec!["pr", "merge", branch, "--merge"]
}

/// Build the `gh` argv for updating a PR branch from its base.
fn update_from_base_args<'a>(branch: &'a str, _base: &'a str) -> Vec<&'a str> {
    vec!["pr", "update-branch", branch]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_no_decision_is_open() {
        assert_eq!(
            parse_review_state(Some(0),"OPEN|", "").unwrap(),
            ReviewState::Open
        );
    }

    #[test]
    fn open_review_required_is_open() {
        assert_eq!(
            parse_review_state(Some(0),"OPEN|REVIEW_REQUIRED", "").unwrap(),
            ReviewState::Open
        );
    }

    #[test]
    fn open_approved_is_approved() {
        assert_eq!(
            parse_review_state(Some(0),"OPEN|APPROVED", "").unwrap(),
            ReviewState::Approved
        );
    }

    #[test]
    fn open_changes_requested_is_changes_requested() {
        assert_eq!(
            parse_review_state(Some(0),"OPEN|CHANGES_REQUESTED", "").unwrap(),
            ReviewState::ChangesRequested
        );
    }

    #[test]
    fn merged_is_merged() {
        assert_eq!(
            parse_review_state(Some(0),"MERGED|", "").unwrap(),
            ReviewState::Merged
        );
    }

    #[test]
    fn closed_is_closed() {
        assert_eq!(
            parse_review_state(Some(0),"CLOSED|", "").unwrap(),
            ReviewState::Closed
        );
    }

    #[test]
    fn no_pr_stderr_is_known_none() {
        let r = parse_review_state(Some(1),"", "no pull requests found for branch \"impl/x\"");
        assert_eq!(r.unwrap(), ReviewState::None);
    }

    #[test]
    fn other_nonzero_exit_is_error() {
        // Auth/network failure must be undeterminable (Err), NOT a known None.
        let r = parse_review_state(Some(1),"", "gh: not authenticated");
        assert!(matches!(r, Err(ForgeError::Command { .. })));
    }

    #[test]
    fn unknown_state_is_parse_error() {
        let r = parse_review_state(Some(0),"WAT|", "");
        assert!(matches!(r, Err(ForgeError::Parse { .. })));
    }

    #[test]
    fn missing_delimiter_is_parse_error() {
        let r = parse_review_state(Some(0),"OPEN", "");
        assert!(matches!(r, Err(ForgeError::Parse { .. })));
    }

    #[test]
    fn create_pr_args_are_well_formed() {
        let a = create_pr_args("impl/x", "main", "Add x", "body text");
        assert_eq!(
            a,
            vec![
                "pr", "create",
                "--head", "impl/x",
                "--base", "main",
                "--title", "Add x",
                "--body", "body text",
            ]
        );
    }

    #[test]
    fn merge_args_use_merge_strategy() {
        assert_eq!(merge_args("impl/x"), vec!["pr", "merge", "impl/x", "--merge"]);
    }

    #[test]
    fn update_from_base_args_target_the_branch() {
        assert_eq!(
            update_from_base_args("impl/x", "main"),
            vec!["pr", "update-branch", "impl/x"]
        );
    }

    // Live test against real `gh` + a real repo/PR. Never runs in CI; run
    // manually with `cargo test -- --ignored forge_live_review_state`.
    #[test]
    #[ignore]
    fn forge_live_review_state() {
        let forge = Forge::new(".");
        // Adjust the branch to one with a known PR before running manually.
        let _ = forge.review_state("main");
    }
}
