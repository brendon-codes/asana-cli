use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn top_level_help_names_every_command_family() {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("Usage: asana <COMMAND>")
            .and(predicate::str::contains("cmd"))
            .and(predicate::str::contains("server"))
            .and(predicate::str::contains("util")),
    );
}

#[test]
fn command_family_help_is_specific_and_actionable() {
    for (args, expected) in [
        (
            vec!["cmd", "--help"],
            vec![
                "Usage: asana cmd",
                "247 operations",
                "--json|--markdown|--text",
                "createAttachmentForObject",
            ],
        ),
        (
            vec!["server", "--help"],
            vec![
                "Run a local mock Asana REST API server",
                "--host",
                "--port",
                "--data-dir",
            ],
        ),
        (
            vec!["util", "--help"],
            vec!["make-config", "validate-config", "status", "make-skill"],
        ),
    ] {
        let mut cmd = Command::cargo_bin("asana").expect("binary should build");
        let mut assert = cmd.args(args).assert().success();
        for text in expected {
            assert = assert.stdout(predicate::str::contains(text));
        }
    }
}

#[test]
fn operation_help_covers_json_body_query_and_multipart_syntax() {
    let mut create_task = Command::cargo_bin("asana").expect("binary should build");
    create_task
        .args(["cmd", "createTask", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("POST"))
        .stdout(predicate::str::contains("/tasks"))
        .stdout(predicate::str::contains("--body <json> (required)"));

    let mut get_tasks = Command::cargo_bin("asana").expect("binary should build");
    get_tasks
        .args(["cmd", "getTasks", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--workspace"))
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--opt_fields"));

    let mut attachment = Command::cargo_bin("asana").expect("binary should build");
    attachment
        .args(["cmd", "createAttachmentForObject", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("multipart/form-data"))
        .stdout(predicate::str::contains("--parent"))
        .stdout(predicate::str::contains("--file <path>"));
}
