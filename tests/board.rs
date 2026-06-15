use assert_cmd::Command;
use predicates::prelude::*;

/// Drive the exit-criteria walk for slice C: init, spec, two DAG nodes (one
/// depending on the other), then `circuit board`. With the no-op git adapter the
/// board is honest — `?` stages, `?` health, `?/n` tasks — and flow stays colorless.
#[test]
fn board_renders_colorless_mermaid_and_honest_unknowns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path();

    let run = |args: &[&str]| {
        Command::cargo_bin("circuit").unwrap().args(args).current_dir(path).assert().success();
    };

    run(&["init"]);
    run(&["spec", "new", "checkout", "--title", "Checkout", "--intent", "Pay."]);
    run(&["dag", "add-node", "auth", "--spec", "checkout", "--title", "Auth", "--branch", "impl/auth"]);
    run(&[
        "dag", "add-node", "pay", "--spec", "checkout", "--title", "Pay",
        "--branch", "impl/pay", "--depends-on", "auth",
    ]);

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["board", "checkout"])
        .current_dir(path)
        .assert()
        .success()
        .stdout(predicate::str::contains("graph TD"))
        // precedence edge: prerequisite -> dependent
        .stdout(predicate::str::contains("auth --> pay"))
        // colorless styling present
        .stdout(predicate::str::contains("classDef flow"))
        // no-op adapter => honest unknowns in the labels
        .stdout(predicate::str::contains("auth · ? · ?"))
        // traceability undeterminable without git
        .stdout(predicate::str::contains("Tasks: ?/2"));
}
