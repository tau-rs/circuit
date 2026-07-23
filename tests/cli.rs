use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn map_reports_layers() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("app")).unwrap();
    std::fs::write(
        src.join("main.rs"),
        "use crate::app::run;\nfn main() { run(); }",
    )
    .unwrap();
    std::fs::write(src.join("app/mod.rs"), "pub fn run() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("layers (inward →)"))
        .stdout(predicate::str::contains("[Application"));
}

#[test]
fn map_feature_highlights_path() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("app")).unwrap();
    std::fs::write(
        src.join("main.rs"),
        "use crate::app::run;\nfn main() { run(); }",
    )
    .unwrap();
    std::fs::write(src.join("app/mod.rs"), "pub fn run() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(dir.path())
        .arg("--feature")
        .arg("main")
        .assert()
        .success()
        .stdout(predicate::str::contains("feature · main"));
}

#[test]
fn map_mermaid_exports_flowchart() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(dir.path())
        .arg("--mermaid")
        .assert()
        .success()
        .stdout(predicate::str::contains("flowchart LR"));
}

#[test]
fn map_html_emits_self_contained_document() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("app")).unwrap();
    std::fs::write(
        src.join("main.rs"),
        "use crate::app::run;\nfn main() { run(); }",
    )
    .unwrap();
    std::fs::write(src.join("app/mod.rs"), "pub fn run() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(dir.path())
        .arg("--html")
        .assert()
        .success()
        .stdout(predicate::str::contains("<!DOCTYPE html>"))
        .stdout(predicate::str::contains("__CIRCUIT_DATA__").not());
}

#[test]
fn map_html_conflicts_with_mermaid() {
    Command::cargo_bin("circuit")
        .unwrap()
        .arg("map")
        .arg(".")
        .arg("--html")
        .arg("--mermaid")
        .assert()
        .failure();
}

#[test]
fn analyze_reports_indicators_and_mermaid() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("domain")).unwrap();
    // domain depends on adapters -> a dependency-rule violation
    std::fs::write(src.join("domain/order.rs"), "use crate::adapters::Db;").unwrap();
    std::fs::write(src.join("adapters.rs"), "pub struct Db;").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("analyze")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Dependency rule"))
        .stdout(predicate::str::contains("VIOLATION"))
        .stdout(predicate::str::contains("graph TD"));
}

#[test]
fn comprehend_lists_entry_points() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() { greet(); }\nfn greet() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("comprehend")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("[main] root::main"));
}

#[test]
fn impact_reports_dependents() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() { greet(); }\nfn greet() {}").unwrap();

    Command::cargo_bin("circuit")
        .unwrap()
        .arg("impact")
        .arg("greet")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("dependents"))
        .stdout(predicate::str::contains("root::main"));
}
