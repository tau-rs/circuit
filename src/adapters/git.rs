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

impl GitPort for Git {
    type Error = GitError;

    fn branch_facts(&self, branch: &str, base: &str) -> Result<BranchFacts, GitError> {
        // A missing branch is Draft-shaped: report all-false defaults, not an error.
        let exists = self.run_bool(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{branch}^{{commit}}"),
        ])?;
        if !exists {
            return Ok(BranchFacts::default());
        }

        let ahead_raw = self.run(&["rev-list", "--count", &format!("{base}..{branch}")])?;
        let commits_ahead_of_base = ahead_raw.parse::<usize>().map_err(|e| GitError::Parse {
            output: ahead_raw.clone(),
            reason: e.to_string(),
        })?;

        // `diff --quiet base...branch` exits 1 when the merge-base..branch diff
        // is non-empty. run_bool: true => no diff, so substantive = !no_diff.
        let no_diff = self.run_bool(&["diff", "--quiet", &format!("{base}...{branch}")])?;
        let has_substantive_changes = !no_diff;

        // "Merged" = branch is an ancestor of base AND base has strictly
        // advanced beyond the branch tip (base is NOT also an ancestor of
        // branch). A freshly-created branch sitting exactly at base satisfies
        // is-ancestor in BOTH directions (equal tips), so it is NOT merged —
        // it must derive to Project, not Done (§7.1). A fast-forward merge that
        // leaves base == branch is indistinguishable from fresh via refs alone;
        // Circuit merges via PR/merge-commits, so base advances in practice.
        let branch_in_base = self.run_bool(&["merge-base", "--is-ancestor", branch, base])?;
        let base_in_branch = self.run_bool(&["merge-base", "--is-ancestor", base, branch])?;
        let merged_into_base = branch_in_base && !base_in_branch;

        Ok(BranchFacts {
            exists: true,
            commits_ahead_of_base,
            has_substantive_changes,
            merged_into_base,
        })
    }

    fn create_branch(&self, branch: &str, base: &str) -> Result<(), GitError> {
        // Create the ref without checking it out; add_worktree checks it out.
        self.run(&["branch", branch, base]).map(|_| ())
    }

    fn add_worktree(&self, branch: &str, path: &Path) -> Result<(), GitError> {
        let path_str = path.to_str().ok_or_else(|| GitError::Parse {
            output: path.display().to_string(),
            reason: "worktree path is not valid UTF-8".to_string(),
        })?;
        self.run(&["worktree", "add", path_str, branch]).map(|_| ())
    }

    fn list_worktrees(&self) -> Result<Vec<Worktree>, GitError> {
        let out = self.run(&["worktree", "list", "--porcelain"])?;
        Ok(parse_worktree_porcelain(&out))
    }
}

/// Parse `git worktree list --porcelain` into `Worktree` entries. Blocks are
/// separated by blank lines; each has a `worktree <path>` line and either a
/// `branch refs/heads/<name>` line or a `detached` line.
fn parse_worktree_porcelain(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;

    let mut flush = |path: &mut Option<PathBuf>, branch: &mut Option<String>| {
        if let Some(p) = path.take() {
            out.push(Worktree {
                path: p,
                branch: branch.take(),
            });
        } else {
            *branch = None;
        }
    };

    for line in text.lines() {
        if line.is_empty() {
            flush(&mut path, &mut branch);
        } else if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
        }
        // `HEAD <sha>` and `detached` lines need no handling (branch stays None).
    }
    flush(&mut path, &mut branch);
    out
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

    /// Helper: run a raw git command in the repo (test setup only).
    fn git_raw(p: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(p)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?} failed");
    }

    #[test]
    fn branch_facts_for_missing_branch_is_default() {
        let (_d, git) = init_repo();
        let f = git.branch_facts("nope", "main").unwrap();
        assert_eq!(f, BranchFacts::default());
        assert!(!f.exists);
    }

    #[test]
    fn branch_facts_for_branch_without_changes_is_project_shaped() {
        let (d, git) = init_repo();
        git_raw(d.path(), &["branch", "feat", "main"]);
        let f = git.branch_facts("feat", "main").unwrap();
        assert!(f.exists);
        assert_eq!(f.commits_ahead_of_base, 0);
        assert!(!f.has_substantive_changes);
        assert!(!f.merged_into_base);
    }

    #[test]
    fn branch_facts_for_branch_with_commits_has_changes() {
        let (d, git) = init_repo();
        let p = d.path();
        git_raw(p, &["branch", "feat", "main"]);
        git_raw(p, &["worktree", "add", "-q", "wt", "feat"]);
        std::fs::write(p.join("wt/new.txt"), "x\n").unwrap();
        git_raw(&p.join("wt"), &["add", "new.txt"]);
        git_raw(&p.join("wt"), &["commit", "-qm", "work"]);

        let f = git.branch_facts("feat", "main").unwrap();
        assert!(f.exists);
        assert_eq!(f.commits_ahead_of_base, 1);
        assert!(f.has_substantive_changes);
        assert!(!f.merged_into_base);
    }

    #[test]
    fn fresh_branch_at_base_is_not_merged() {
        // Regression: a branch created at base (no commits) is an ancestor of
        // base, but must NOT be reported merged — it derives to Project (§7.1).
        let (d, git) = init_repo();
        git_raw(d.path(), &["branch", "feat", "main"]);
        let f = git.branch_facts("feat", "main").unwrap();
        assert!(f.exists);
        assert!(!f.merged_into_base);
    }

    #[test]
    fn branch_facts_detects_merged_into_base() {
        let (d, git) = init_repo();
        let p = d.path();
        git_raw(p, &["branch", "feat", "main"]);
        git_raw(p, &["worktree", "add", "-q", "wt", "feat"]);
        std::fs::write(p.join("wt/new.txt"), "x\n").unwrap();
        git_raw(&p.join("wt"), &["add", "new.txt"]);
        git_raw(&p.join("wt"), &["commit", "-qm", "work"]);
        // Merge feat into main with a merge commit so main advances beyond feat
        // (the realistic PR-merge case; a fast-forward leaving equal tips is
        // indistinguishable from a fresh branch and is intentionally not "Done").
        git_raw(p, &["merge", "--no-ff", "-q", "-m", "merge feat", "feat"]);

        let f = git.branch_facts("feat", "main").unwrap();
        assert!(f.exists);
        assert!(f.merged_into_base);
    }

    #[test]
    fn create_branch_makes_a_new_ref() {
        let (_d, git) = init_repo();
        git.create_branch("impl/x", "main").unwrap();
        // The ref now exists.
        let f = git.branch_facts("impl/x", "main").unwrap();
        assert!(f.exists);
    }

    #[test]
    fn add_worktree_checks_out_the_branch_to_a_path() {
        let (d, git) = init_repo();
        git.create_branch("impl/x", "main").unwrap();
        let wt = d.path().join("wt-x");
        git.add_worktree("impl/x", &wt).unwrap();
        assert!(
            wt.join("base.txt").exists(),
            "worktree should contain base commit"
        );
    }

    #[test]
    fn list_worktrees_includes_added_worktree_with_branch() {
        let (d, git) = init_repo();
        git.create_branch("impl/x", "main").unwrap();
        let wt = d.path().join("wt-x");
        git.add_worktree("impl/x", &wt).unwrap();

        let list = git.list_worktrees().unwrap();
        // The main worktree plus the one we added.
        assert!(list.iter().any(|w| w.branch.as_deref() == Some("main")));
        let added = list
            .iter()
            .find(|w| w.branch.as_deref() == Some("impl/x"))
            .expect("added worktree should be listed");
        // git may canonicalize the path (e.g. /var -> /private/var on macOS);
        // compare on the final path component to stay portable.
        assert_eq!(added.path.file_name().unwrap(), wt.file_name().unwrap());
    }

    #[test]
    fn parse_worktree_porcelain_handles_branch_and_detached() {
        let text = "\
worktree /repo
HEAD aaaa
branch refs/heads/main

worktree /repo/wt
HEAD bbbb
branch refs/heads/impl/x

worktree /repo/detached
HEAD cccc
detached
";
        let ws = parse_worktree_porcelain(text);
        assert_eq!(ws.len(), 3);
        assert_eq!(ws[0].branch.as_deref(), Some("main"));
        assert_eq!(ws[1].branch.as_deref(), Some("impl/x"));
        assert_eq!(ws[1].path, PathBuf::from("/repo/wt"));
        assert_eq!(ws[2].branch, None);
    }
}
