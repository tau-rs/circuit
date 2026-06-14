//! GitHub forge adapter — `ForgePort` by shelling out to the `gh` CLI (M2b §3).
//! The command runner is injected so JSON-parse and argument-construction logic
//! is unit-testable offline; only `SystemRunner` spawns a real process.

use serde::Deserialize;
use thiserror::Error;

use crate::flow::facts::ReviewState;
use crate::ports::ForgePort;

/// Output of a finished command, owned so fakes need not build a `process::Output`.
pub struct CommandOutput {
    pub success: bool,
    pub status: String,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// The injectable process boundary. Production is `SystemRunner`; tests supply a
/// fake returning canned `CommandOutput`.
pub trait CommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<CommandOutput>;
}

/// Spawns real processes via `std::process::Command`.
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<CommandOutput> {
        let out = std::process::Command::new(program).args(args).output()?;
        Ok(CommandOutput {
            success: out.status.success(),
            status: out.status.to_string(),
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }
}

/// Errors at the forge boundary.
#[derive(Debug, Error)]
pub enum ForgeError {
    #[error("failed to launch {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("`gh {args}` failed ({status}): {stderr}")]
    Command {
        args: String,
        status: String,
        stderr: String,
    },
    #[error("failed to parse gh output: {source}")]
    Parse {
        #[source]
        source: serde_json::Error,
    },
}

/// One PR row from `gh pr list --json state,reviewDecision`.
#[derive(Debug, Deserialize)]
struct GhPr {
    state: String,
    #[serde(rename = "reviewDecision", default)]
    review_decision: Option<String>,
}

/// GitHub forge over the `gh` CLI, generic over the command runner.
pub struct GhForge<R = SystemRunner> {
    runner: R,
}

impl GhForge<SystemRunner> {
    pub fn new() -> Self {
        Self {
            runner: SystemRunner,
        }
    }
}

impl Default for GhForge<SystemRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: CommandRunner> GhForge<R> {
    pub fn with_runner(runner: R) -> Self {
        Self { runner }
    }

    /// Run `gh <args>`, returning stdout on success or a typed error.
    fn gh(&self, args: &[&str]) -> Result<Vec<u8>, ForgeError> {
        let out = self
            .runner
            .run("gh", args)
            .map_err(|source| ForgeError::Spawn {
                program: "gh".to_string(),
                source,
            })?;
        if !out.success {
            return Err(ForgeError::Command {
                args: args.join(" "),
                status: out.status,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        Ok(out.stdout)
    }
}

impl<R: CommandRunner> ForgePort for GhForge<R> {
    type Error = ForgeError;

    fn review_state(&self, branch: &str) -> Result<ReviewState, Self::Error> {
        let stdout = self.gh(&[
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "all",
            "--json",
            "state,reviewDecision",
        ])?;
        let prs: Vec<GhPr> =
            serde_json::from_slice(&stdout).map_err(|source| ForgeError::Parse { source })?;
        let Some(pr) = prs.into_iter().next() else {
            return Ok(ReviewState::None);
        };
        Ok(match pr.state.as_str() {
            "MERGED" => ReviewState::Merged,
            "CLOSED" => ReviewState::Closed,
            "OPEN" if pr.review_decision.as_deref() == Some("APPROVED") => ReviewState::Approved,
            // OPEN (no approval) and any unknown future state: conservative Open.
            _ => ReviewState::Open,
        })
    }

    fn create_pr(
        &self,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<(), Self::Error> {
        self.gh(&[
            "pr", "create", "--head", branch, "--base", base, "--title", title, "--body", body,
        ])?;
        Ok(())
    }

    fn merge(&self, branch: &str) -> Result<(), Self::Error> {
        self.gh(&["pr", "merge", branch, "--merge"])?;
        Ok(())
    }

    fn update_from_base(&self, branch: &str, _base: &str) -> Result<(), Self::Error> {
        // `gh pr update-branch` updates the PR head from its base branch.
        self.gh(&["pr", "update-branch", branch])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records the args of each call and returns a preset outcome.
    struct FakeRunner {
        out: Option<CommandOutput>,
        spawn_err: bool,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn ok(stdout: &str) -> Self {
            Self {
                out: Some(CommandOutput {
                    success: true,
                    status: "exit status: 0".to_string(),
                    stdout: stdout.as_bytes().to_vec(),
                    stderr: Vec::new(),
                }),
                spawn_err: false,
                calls: RefCell::new(Vec::new()),
            }
        }
        fn fail(stderr: &str) -> Self {
            Self {
                out: Some(CommandOutput {
                    success: false,
                    status: "exit status: 1".to_string(),
                    stdout: Vec::new(),
                    stderr: stderr.as_bytes().to_vec(),
                }),
                spawn_err: false,
                calls: RefCell::new(Vec::new()),
            }
        }
        fn missing() -> Self {
            Self {
                out: None,
                spawn_err: true,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, _program: &str, args: &[&str]) -> std::io::Result<CommandOutput> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            if self.spawn_err {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no gh"));
            }
            let o = self.out.as_ref().unwrap();
            Ok(CommandOutput {
                success: o.success,
                status: o.status.clone(),
                stdout: o.stdout.clone(),
                stderr: o.stderr.clone(),
            })
        }
    }

    fn last_call(f: &GhForge<FakeRunner>) -> Vec<String> {
        f.runner.calls.borrow().last().cloned().unwrap()
    }

    #[test]
    fn empty_pr_list_is_known_none() {
        let f = GhForge::with_runner(FakeRunner::ok("[]"));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::None);
    }

    #[test]
    fn open_pr_without_approval_is_open() {
        // `CHANGES_REQUESTED` is a real gh reviewDecision value meaning "not approved";
        // it must fall through the conservative `_ => Open` arm.
        let f = GhForge::with_runner(FakeRunner::ok(
            r#"[{"state":"OPEN","reviewDecision":"CHANGES_REQUESTED"}]"#,
        ));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Open);
    }

    #[test]
    fn open_pr_with_null_review_decision_is_open() {
        let f = GhForge::with_runner(FakeRunner::ok(
            r#"[{"state":"OPEN","reviewDecision":null}]"#,
        ));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Open);
    }

    #[test]
    fn malformed_json_is_parse_error() {
        let f = GhForge::with_runner(FakeRunner::ok("not json"));
        assert!(matches!(
            f.review_state("b").unwrap_err(),
            ForgeError::Parse { .. }
        ));
    }

    #[test]
    fn approved_open_pr_is_approved() {
        let f = GhForge::with_runner(FakeRunner::ok(
            r#"[{"state":"OPEN","reviewDecision":"APPROVED"}]"#,
        ));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Approved);
    }

    #[test]
    fn merged_pr_is_merged() {
        let f = GhForge::with_runner(FakeRunner::ok(
            r#"[{"state":"MERGED","reviewDecision":"APPROVED"}]"#,
        ));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Merged);
    }

    #[test]
    fn closed_pr_is_closed() {
        let f = GhForge::with_runner(FakeRunner::ok(
            r#"[{"state":"CLOSED","reviewDecision":""}]"#,
        ));
        assert_eq!(f.review_state("b").unwrap(), ReviewState::Closed);
    }

    #[test]
    fn review_state_query_uses_head_and_all_states() {
        let f = GhForge::with_runner(FakeRunner::ok("[]"));
        f.review_state("impl/x").unwrap();
        let call = last_call(&f);
        assert_eq!(
            call,
            vec![
                "pr",
                "list",
                "--head",
                "impl/x",
                "--state",
                "all",
                "--json",
                "state,reviewDecision"
            ]
        );
    }

    #[test]
    fn nonzero_exit_is_command_error() {
        let f = GhForge::with_runner(FakeRunner::fail("could not authenticate"));
        let err = f.review_state("b").unwrap_err();
        assert!(matches!(err, ForgeError::Command { .. }));
        assert!(err.to_string().contains("could not authenticate"));
    }

    #[test]
    fn missing_gh_is_spawn_error() {
        let f = GhForge::with_runner(FakeRunner::missing());
        let err = f.review_state("b").unwrap_err();
        assert!(matches!(err, ForgeError::Spawn { .. }));
    }

    #[test]
    fn create_pr_builds_expected_args() {
        let f = GhForge::with_runner(FakeRunner::ok(""));
        f.create_pr("impl/x", "main", "My title", "My body")
            .unwrap();
        assert_eq!(
            last_call(&f),
            vec![
                "pr", "create", "--head", "impl/x", "--base", "main", "--title", "My title",
                "--body", "My body"
            ]
        );
    }

    #[test]
    fn merge_builds_expected_args() {
        let f = GhForge::with_runner(FakeRunner::ok(""));
        f.merge("impl/x").unwrap();
        assert_eq!(last_call(&f), vec!["pr", "merge", "impl/x", "--merge"]);
    }

    #[test]
    fn update_from_base_builds_expected_args() {
        let f = GhForge::with_runner(FakeRunner::ok(""));
        f.update_from_base("impl/x", "main").unwrap();
        assert_eq!(last_call(&f), vec!["pr", "update-branch", "impl/x"]);
    }
}
