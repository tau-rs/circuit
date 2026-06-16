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
/// `PR ?`). Pure — fully testable from canned (success, stdout, stderr).
fn parse_review_state(
    exit_ok: bool,
    stdout: &str,
    stderr: &str,
) -> Result<ReviewState, ForgeError> {
    if !exit_ok {
        let s = stderr.to_lowercase();
        if s.contains("no pull requests found") || s.contains("no pull request found") {
            return Ok(ReviewState::None);
        }
        return Err(ForgeError::Command {
            code: "nonzero".to_string(),
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
        parse_review_state(out.status.success(), &stdout, &stderr)
    }

    fn create_pr(
        &self,
        _branch: &str,
        _base: &str,
        _title: &str,
        _body: &str,
    ) -> Result<(), ForgeError> {
        unimplemented!("Task 4")
    }

    fn merge(&self, _branch: &str) -> Result<(), ForgeError> {
        unimplemented!("Task 4")
    }

    fn update_from_base(&self, _branch: &str, _base: &str) -> Result<(), ForgeError> {
        unimplemented!("Task 4")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_no_decision_is_open() {
        assert_eq!(
            parse_review_state(true, "OPEN|", "").unwrap(),
            ReviewState::Open
        );
    }

    #[test]
    fn open_review_required_is_open() {
        assert_eq!(
            parse_review_state(true, "OPEN|REVIEW_REQUIRED", "").unwrap(),
            ReviewState::Open
        );
    }

    #[test]
    fn open_approved_is_approved() {
        assert_eq!(
            parse_review_state(true, "OPEN|APPROVED", "").unwrap(),
            ReviewState::Approved
        );
    }

    #[test]
    fn open_changes_requested_is_changes_requested() {
        assert_eq!(
            parse_review_state(true, "OPEN|CHANGES_REQUESTED", "").unwrap(),
            ReviewState::ChangesRequested
        );
    }

    #[test]
    fn merged_is_merged() {
        assert_eq!(
            parse_review_state(true, "MERGED|", "").unwrap(),
            ReviewState::Merged
        );
    }

    #[test]
    fn closed_is_closed() {
        assert_eq!(
            parse_review_state(true, "CLOSED|", "").unwrap(),
            ReviewState::Closed
        );
    }

    #[test]
    fn no_pr_stderr_is_known_none() {
        let r = parse_review_state(false, "", "no pull requests found for branch \"impl/x\"");
        assert_eq!(r.unwrap(), ReviewState::None);
    }

    #[test]
    fn other_nonzero_exit_is_error() {
        // Auth/network failure must be undeterminable (Err), NOT a known None.
        let r = parse_review_state(false, "", "gh: not authenticated");
        assert!(matches!(r, Err(ForgeError::Command { .. })));
    }

    #[test]
    fn unknown_state_is_parse_error() {
        let r = parse_review_state(true, "WAT|", "");
        assert!(matches!(r, Err(ForgeError::Parse { .. })));
    }

    #[test]
    fn missing_delimiter_is_parse_error() {
        let r = parse_review_state(true, "OPEN", "");
        assert!(matches!(r, Err(ForgeError::Parse { .. })));
    }
}
