//! End-to-end CLI tests for the checkpoint command and the offline failure paths
//! of `circuit pr …` (paths that never reach `gh`), plus a real-`gh` smoke test
//! that is skipped-with-log when `gh` is unavailable.

use assert_cmd::Command;
use predicates::prelude::*;

fn circuit(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("circuit").unwrap();
    cmd.current_dir(dir);
    cmd
}

/// Hand-write a minimal impl-session record so checkpoint/pr commands have a
/// session to resolve (no `session spawn` command exists in this slice).
fn write_impl_session(dir: &std::path::Path, id: &str) {
    let sessions = dir.join(".circuit/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let toml = format!(
        "schema_version = 1\nid = \"{id}\"\nkind = \"impl\"\nparent = \"checkout\"\ndag_node = \"auth-slice\"\nbranch = \"impl/checkout-auth\"\n"
    );
    std::fs::write(sessions.join(format!("{id}.toml")), toml).unwrap();
}

const SAMPLE_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

#[test]
fn checkpoint_writes_a_record_file() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();
    write_impl_session(dir.path(), SAMPLE_ID);

    circuit(dir.path())
        .args(["checkpoint", SAMPLE_ID])
        .args(["--state", "self-review"])
        .args(["--commit", "a1b2c3d"])
        .args(["--note", "first pass"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Checkpoint recorded"));

    let path = dir
        .path()
        .join(format!(".circuit/checkpoints/{SAMPLE_ID}.toml"));
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("state = \"self-review\""));
    assert!(text.contains("commit = \"a1b2c3d\""));
    assert!(text.contains("first pass"));
}

#[test]
fn checkpoint_requires_init() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path())
        .args([
            "checkpoint",
            SAMPLE_ID,
            "--state",
            "accepted",
            "--commit",
            "x",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("circuit init"));
}

#[test]
fn pr_create_fails_for_missing_session() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();
    // Never reaches gh: session resolution fails first.
    circuit(dir.path())
        .args(["pr", "create", "01J-does-not-exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no such session"));
}

#[test]
fn pr_create_fails_for_branchless_session() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();
    // A spec session has no branch -> resolution fails before gh.
    let sessions = dir.path().join(".circuit/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join(format!("{SAMPLE_ID}.toml")),
        format!("schema_version = 1\nid = \"{SAMPLE_ID}\"\nkind = \"spec\"\n"),
    )
    .unwrap();

    circuit(dir.path())
        .args(["pr", "create", SAMPLE_ID])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no branch"));
}

#[test]
fn real_gh_review_state_smoke() {
    use circuit::adapters::forge::GhForge;
    use circuit::ports::ForgePort;

    // Skip-with-log when gh is unavailable — never silently passing.
    if std::process::Command::new("gh")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("SKIP real_gh_review_state_smoke: `gh` not found on PATH");
        return;
    }

    // gh present: exercise the real SystemRunner shell-out + parse path. The
    // ambient git/gh context decides Ok(None)/Err; the point is no panic.
    let forge = GhForge::new();
    let result = forge.review_state("circuit-smoke-nonexistent-branch-xyz");
    // Either outcome is acceptable; assert the call completed without panicking.
    let _ = result.is_ok();
}
