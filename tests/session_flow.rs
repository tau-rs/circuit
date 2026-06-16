use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use std::process::Command as Stdcmd;

/// Run `circuit` with args in a given working directory.
fn circuit(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("circuit").unwrap();
    cmd.current_dir(dir);
    cmd
}

/// Initialize a git repo with one base commit on `main`.
fn init_git_repo(dir: &Path) {
    let run = |args: &[&str]| {
        assert!(
            Stdcmd::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap()
                .status
                .success(),
            "git {args:?} failed"
        );
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["config", "user.email", "t@e.com"]);
    run(&["config", "user.name", "t"]);
    std::fs::write(dir.join("base.txt"), "base\n").unwrap();
    run(&["add", "base.txt"]);
    run(&["commit", "-qm", "base"]);
}

#[test]
fn spawn_creates_worktree_and_flow_shows_project() {
    let dir = tempfile::tempdir().unwrap();
    let wt_root = tempfile::tempdir().unwrap(); // controlled worktree location
    init_git_repo(dir.path());

    // init + author a spec and a DAG node.
    circuit(dir.path()).arg("init").assert().success();
    circuit(dir.path())
        .args([
            "spec", "new", "checkout", "--title", "Checkout", "--intent", "Pay.",
        ])
        .assert()
        .success();
    circuit(dir.path())
        .args([
            "dag",
            "add-node",
            "auth-slice",
            "--spec",
            "checkout",
            "--title",
            "Auth",
            "--branch",
            "impl/checkout-auth",
        ])
        .assert()
        .success();

    // spawn — pin the worktree location via the env override.
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stage: Project"))
        .stdout(predicate::str::contains("impl/checkout-auth"));

    // The branch and a worktree now exist.
    let branch_listed = Stdcmd::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&branch_listed.stdout);
    assert!(
        listing.contains("refs/heads/impl/checkout-auth"),
        "got: {listing}"
    );

    // flow by DAG-node name shows the Project stage for a fresh branch. With no
    // GitHub remote the delivery mode is Local; with no checkpoint file the
    // review state is a *known* `no PR` (not the undeterminable `PR ?`).
    circuit(dir.path())
        .args(["flow", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("auth-slice  [impl]"))
        .stdout(predicate::str::contains("‹Project›"))
        .stdout(predicate::str::contains("no PR"));

    // flow with no arg lists all sessions (the spec + the impl session).
    circuit(dir.path())
        .arg("flow")
        .assert()
        .success()
        .stdout(predicate::str::contains("auth-slice"));
}

#[test]
fn spawn_refuses_to_clobber_an_existing_branch() {
    let dir = tempfile::tempdir().unwrap();
    let wt_root = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    circuit(dir.path()).arg("init").assert().success();
    circuit(dir.path())
        .args(["spec", "new", "checkout", "--title", "C", "--intent", "x"])
        .assert()
        .success();
    circuit(dir.path())
        .args([
            "dag",
            "add-node",
            "auth-slice",
            "--spec",
            "checkout",
            "--title",
            "A",
            "--branch",
            "impl/x",
        ])
        .assert()
        .success();

    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .success();
    // Second spawn hits the existing branch and fails clearly.
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}
