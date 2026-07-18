use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

/// Author a tiny repo whose `model` module depends on `adapters`, plus a
/// projection mapping billing->model and ghx->adapters with a single allowed edge.
/// `allowed_from`/`allowed_to` are component names for the one projection edge.
fn scaffold(path: &std::path::Path, allowed_from: &str, allowed_to: &str) {
    let src = path.join("src");
    fs::create_dir_all(src.join("model")).unwrap();
    fs::create_dir_all(src.join("adapters")).unwrap();
    // edge: model -> adapters
    fs::write(src.join("model/x.rs"), "use crate::adapters::Thing;").unwrap();
    fs::write(src.join("adapters/y.rs"), "pub struct Thing;").unwrap();

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

    // overwrite the skeleton with an authored projection
    let toml = format!(
        r#"schema_version = 1
spec = "checkout"

[[component]]
name = "billing"
layer = "domain"
module = "model"

[[component]]
name = "ghx"
layer = "adapter"
module = "adapters"

[[edge]]
from = "{allowed_from}"
to = "{allowed_to}"
"#
    );
    fs::write(path.join(".circuit/projections/checkout.toml"), toml).unwrap();
}

#[test]
fn conformance_passes_when_edge_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    // allow billing->ghx == model->adapters, which is exactly what the code has
    scaffold(dir.path(), "billing", "ghx");

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["conformance", "checkout"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Violations (0)"))
        .stdout(predicate::str::contains("Sound"));
}

#[test]
fn conformance_fails_on_a_broken_edge() {
    let dir = tempfile::tempdir().unwrap();
    // allow ghx->billing == adapters->model; code has model->adapters -> violation
    scaffold(dir.path(), "ghx", "billing");

    Command::cargo_bin("circuit")
        .unwrap()
        .args(["conformance", "checkout"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("Violations (1)"))
        .stdout(predicate::str::contains("billing"));
}
