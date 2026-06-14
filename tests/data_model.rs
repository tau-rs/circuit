use assert_cmd::Command;
use predicates::prelude::*;

/// Run `circuit` with args in a given working directory.
fn circuit(dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("circuit").unwrap();
    cmd.current_dir(dir);
    cmd
}

#[test]
fn init_scaffolds_circuit_directory() {
    let dir = tempfile::tempdir().unwrap();

    circuit(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));

    assert!(dir.path().join(".circuit/config.toml").exists());
    assert!(dir.path().join(".circuit/glossary.toml").exists());

    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".circuit/local.toml"));
}

#[test]
fn spec_new_writes_a_spec_record() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();

    circuit(dir.path())
        .args(["spec", "new", "checkout"])
        .args(["--title", "Checkout & payment"])
        .args(["--intent", "Let a customer pay for a basket."])
        .args(["--context", "billing"])
        .args(["--context", "cart"])
        .assert()
        .success()
        .stdout(predicate::str::contains("checkout"));

    let text = std::fs::read_to_string(dir.path().join(".circuit/specs/checkout.toml")).unwrap();
    assert!(text.contains("title = \"Checkout & payment\""));
    assert!(text.contains("billing"));
    assert!(text.contains("cart"));
}

#[test]
fn dag_add_node_and_link_build_the_graph() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();

    circuit(dir.path())
        .args(["dag", "add-node", "cart-slice"])
        .args(["--spec", "checkout"])
        .args(["--title", "Cart slice"])
        .args(["--branch", "impl/checkout-cart"])
        .assert()
        .success();

    circuit(dir.path())
        .args(["dag", "add-node", "auth-slice"])
        .args(["--spec", "checkout"])
        .args(["--title", "Auth slice"])
        .args(["--branch", "impl/checkout-auth"])
        .args(["--depends-on", "cart-slice"])
        .assert()
        .success();

    // Link adds an extra dependency edge to an existing node.
    circuit(dir.path())
        .args(["dag", "link", "auth-slice", "cart-slice"])
        .assert()
        .success();

    let auth = std::fs::read_to_string(dir.path().join(".circuit/dag/auth-slice.toml")).unwrap();
    assert!(auth.contains("branch = \"impl/checkout-auth\""));
    assert!(auth.contains("cart-slice"));
}

#[test]
fn dag_check_reports_sound_and_cycles() {
    let dir = tempfile::tempdir().unwrap();
    circuit(dir.path()).arg("init").assert().success();

    circuit(dir.path())
        .args(["dag", "add-node", "cart-slice", "--spec", "checkout", "--title", "Cart", "--branch", "impl/cart"])
        .assert()
        .success();
    circuit(dir.path())
        .args(["dag", "add-node", "auth-slice", "--spec", "checkout", "--title", "Auth", "--branch", "impl/auth", "--depends-on", "cart-slice"])
        .assert()
        .success();

    // Sound DAG.
    circuit(dir.path())
        .args(["dag", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DAG sound"));

    // Introduce a cycle: cart-slice now depends on auth-slice.
    circuit(dir.path())
        .args(["dag", "link", "cart-slice", "auth-slice"])
        .assert()
        .success();

    circuit(dir.path())
        .args(["dag", "check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("cycle"));
}

#[test]
fn authoring_requires_init() {
    let dir = tempfile::tempdir().unwrap();
    // No `init` first: spec new must fail and not create a half-formed .circuit/.
    circuit(dir.path())
        .args(["spec", "new", "checkout", "--title", "C", "--intent", "pay"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("circuit init"));
    assert!(!dir.path().join(".circuit/specs/checkout.toml").exists());
}
