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
fn local_checkpoint_drives_flow_to_review() {
    // End-to-end Local path (§10/§12): a no-remote repo with a `self-review`
    // checkpoint keyed on the session ULID renders Review + `PR open`. Also
    // pins that `s.id.to_string()` matches the checkpoint store's lookup key.
    let dir = tempfile::tempdir().unwrap();
    let wt_root = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());

    circuit(dir.path()).arg("init").assert().success();
    circuit(dir.path())
        .args(["spec", "new", "checkout", "--title", "C", "--intent", "Pay."])
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
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .success();

    // The session ULID is the stored record's filename.
    let ulid = std::fs::read_dir(dir.path().join(".circuit").join("sessions"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().replace(".toml", ""))
        .next()
        .expect("a session record was written");

    // Make substantive changes on the branch so the stage passes Project; the
    // branch is checked out in the worktree at <wt_root>/<ULID>.
    let wt = wt_root.path().join(&ulid);
    std::fs::write(wt.join("auth.rs"), "auth code\n").unwrap();
    let git = |args: &[&str], cwd: &Path| {
        assert!(Stdcmd::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    };
    git(&["add", "auth.rs"], &wt);
    git(&["commit", "-qm", "auth"], &wt);

    // Drop a self-review checkpoint keyed on the ULID.
    let cp_dir = dir.path().join(".circuit").join("checkpoints");
    std::fs::create_dir_all(&cp_dir).unwrap();
    std::fs::write(cp_dir.join(format!("{ulid}.toml")), "state = \"self-review\"\n").unwrap();

    circuit(dir.path())
        .args(["flow", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("‹Review›"))
        .stdout(predicate::str::contains("PR open"));
}

/// Read the single session record's ULID + raw TOML from a workspace.
fn read_only_session(dir: &Path) -> (String, String) {
    let entry = std::fs::read_dir(dir.join(".circuit").join("sessions"))
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().and_then(|x| x.to_str()) == Some("toml"))
        .expect("a session record was written");
    let ulid = entry.file_name().to_string_lossy().replace(".toml", "");
    let text = std::fs::read_to_string(entry.path()).unwrap();
    (ulid, text)
}

/// init repo + workspace + a spec + a DAG node + spawn one impl session.
/// Returns (repo dir, worktree-root dir) — keep both alive for the test.
fn spawn_one(branch: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let wt_root = tempfile::tempdir().unwrap();
    init_git_repo(dir.path());
    circuit(dir.path()).arg("init").assert().success();
    circuit(dir.path())
        .args(["spec", "new", "checkout", "--title", "C", "--intent", "Pay."])
        .assert()
        .success();
    circuit(dir.path())
        .args([
            "dag", "add-node", "auth-slice", "--spec", "checkout", "--title", "Auth",
            "--branch", branch,
        ])
        .assert()
        .success();
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "spawn", "auth-slice"])
        .assert()
        .success();
    (dir, wt_root)
}

#[test]
fn archive_frees_worktree_flips_status_keeps_branch_and_is_idempotent() {
    let (dir, wt_root) = spawn_one("impl/checkout-auth");
    let (ulid, _) = read_only_session(dir.path());
    let wt = wt_root.path().join(&ulid);
    assert!(wt.exists(), "spawn created the worktree");

    // Archive (clean worktree => no --force needed).
    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("archived"))
        .stdout(predicate::str::contains("agent session may now end"));

    // Worktree gone; status flipped; branch kept.
    assert!(!wt.exists(), "archive removed the worktree dir");
    let (_, text) = read_only_session(dir.path());
    assert!(text.contains("status = \"archived\""), "got: {text}");
    let branch_listed = Stdcmd::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["branch", "--list", "impl/checkout-auth"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&branch_listed.stdout).trim().is_empty(),
        "branch should be kept by default"
    );

    // Idempotent: archiving again is a no-op success.
    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already archived"));
}

#[test]
fn archive_refuses_dirty_worktree_without_force_and_delete_branch_needs_force() {
    let (dir, wt_root) = spawn_one("impl/checkout-auth");
    let (ulid, _) = read_only_session(dir.path());
    let wt = wt_root.path().join(&ulid);

    // Dirty the worktree (untracked file) so removal is refused.
    std::fs::write(wt.join("scratch.txt"), "wip\n").unwrap();
    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--force"));
    // Status untouched on failed teardown.
    let (_, text) = read_only_session(dir.path());
    assert!(!text.contains("status = \"archived\""), "got: {text}");

    // --force discards the dirty worktree; --delete-branch + --force removes
    // the (un-merged once we commit? here still fresh) branch too.
    circuit(dir.path())
        .args(["session", "archive", "auth-slice", "--delete-branch", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("branch impl/checkout-auth deleted"));
    assert!(!wt.exists());
    let branch_listed = Stdcmd::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["branch", "--list", "impl/checkout-auth"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&branch_listed.stdout).trim().is_empty(),
        "branch should be deleted"
    );
}

#[test]
fn unarchive_flips_status_and_rehydrates_worktree() {
    let (dir, wt_root) = spawn_one("impl/checkout-auth");
    let (ulid, _) = read_only_session(dir.path());
    let wt = wt_root.path().join(&ulid);

    circuit(dir.path())
        .args(["session", "archive", "auth-slice"])
        .assert()
        .success();
    assert!(!wt.exists());

    // Unarchive (same worktree-root env so the path resolves identically).
    circuit(dir.path())
        .env("CIRCUIT_WORKTREES_DIR", wt_root.path())
        .args(["session", "unarchive", "auth-slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("restored"));

    assert!(wt.exists(), "unarchive re-added the worktree");
    let (_, text) = read_only_session(dir.path());
    assert!(text.contains("status = \"active\""), "got: {text}");
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
