use assert_cmd::Command;
use predicates::prelude::*;

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
