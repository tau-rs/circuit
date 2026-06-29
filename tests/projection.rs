use assert_cmd::Command;
use predicates::prelude::*;

/// Exit-criteria walk for M3 slice A: init the workspace, create a spec session,
/// init its projection, then `projection show` round-trips the skeleton.
#[test]
fn projection_init_then_show_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();

    let run = |args: &[&str]| {
        Command::cargo_bin("circuit")
            .unwrap()
            .args(args)
            .current_dir(path)
            .assert()
            .success();
    };

    run(&["init"]);
    run(&[
        "spec", "new", "checkout", "--title", "Checkout", "--intent", "Pay.",
    ]);
    run(&["projection", "init", "checkout"]);

    // The file landed where we expect.
    assert!(path.join(".circuit/projections/checkout.toml").exists());

    // `show` reports the skeleton honestly.
    Command::cargo_bin("circuit")
        .unwrap()
        .args(["projection", "show", "checkout"])
        .current_dir(path)
        .assert()
        .success()
        .stdout(predicate::str::contains("Projection: checkout"))
        .stdout(predicate::str::contains("Components (0)"))
        .stdout(predicate::str::contains("(none)"));
}

/// `projection init` refuses when the spec session does not exist.
#[test]
fn projection_init_without_spec_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["init"])
        .current_dir(path)
        .assert()
        .success();

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["projection", "init", "ghost"])
        .current_dir(path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no spec 'ghost'"));
}
