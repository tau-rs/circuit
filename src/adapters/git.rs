//! `GitPort` implemented by shelling out to the `git` CLI. Offline-capable;
//! branch facts come from `rev-list`/`merge-base`/`diff` against the shared
//! object store, worktree ops from `git worktree` (§6, §7).

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::flow::facts::BranchFacts;
use crate::ports::{GitPort, Worktree};

/// Errors from shelling out to `git`.
#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to run git (is it installed and on PATH?): {0}")]
    Spawn(#[source] std::io::Error),
    #[error("git {args} failed (exit {code}): {stderr}")]
    Command {
        args: String,
        code: String,
        stderr: String,
    },
    #[error("git produced non-UTF8 output: {0}")]
    Utf8(#[source] std::string::FromUtf8Error),
    #[error("could not parse git output `{output}`: {reason}")]
    Parse { output: String, reason: String },
}

/// `GitPort` over the `git` CLI, rooted at a working tree. Every command runs
/// with `-C <root>` so the adapter is independent of the process CWD.
pub struct Git {
    root: PathBuf,
}

impl Git {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Run a git subcommand that must succeed; return captured stdout (trimmed).
    fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(GitError::Spawn)?;
        if !out.status.success() {
            return Err(GitError::Command {
                args: args.join(" "),
                code: out
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        String::from_utf8(out.stdout)
            .map(|s| s.trim().to_string())
            .map_err(GitError::Utf8)
    }

    /// Run a yes/no git query. Exit 0 => true, exit 1 => false (a valid
    /// negative answer for `--is-ancestor` / `diff --quiet` / `rev-parse --verify`).
    /// Any other exit code is a real error.
    fn run_bool(&self, args: &[&str]) -> Result<bool, GitError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(GitError::Spawn)?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            other => Err(GitError::Command {
                args: args.join(" "),
                code: other
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp repo on `main` with one base commit. Returns the tempdir
    /// (keep it alive for the test) and a `Git` rooted at it.
    fn init_repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let run = |args: &[&str]| {
            let ok = Command::new("git")
                .arg("-C")
                .arg(p)
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?} failed");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@e.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(p.join("base.txt"), "base\n").unwrap();
        run(&["add", "base.txt"]);
        run(&["commit", "-qm", "base"]);
        let git = Git::new(p);
        (dir, git)
    }

    #[test]
    fn run_returns_stdout_on_success() {
        let (_d, git) = init_repo();
        let head = git.run(&["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head.len(), 40, "expected a 40-char sha, got {head:?}");
    }

    #[test]
    fn run_errors_on_nonzero_exit() {
        let (_d, git) = init_repo();
        let err = git.run(&["rev-parse", "does-not-exist"]).unwrap_err();
        assert!(matches!(err, GitError::Command { .. }));
    }

    #[test]
    fn run_bool_maps_exit_codes() {
        let (_d, git) = init_repo();
        // HEAD is an ancestor of itself => true (exit 0).
        assert!(git
            .run_bool(&["merge-base", "--is-ancestor", "HEAD", "HEAD"])
            .unwrap());
    }
}
