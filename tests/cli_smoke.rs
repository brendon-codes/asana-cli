use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_starts_successfully() {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");

    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "A staged Rust CLI for Asana REST API operations",
        ));
}

#[test]
fn help_lists_required_top_level_commands() {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("cmd")
            .and(predicate::str::contains("server"))
            .and(predicate::str::contains("util")),
    );
}

#[test]
fn command_family_help_is_available() {
    for family in ["cmd", "server", "util"] {
        let mut cmd = Command::cargo_bin("asana").expect("binary should build");

        cmd.args([family, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Usage: asana"));
    }
}
