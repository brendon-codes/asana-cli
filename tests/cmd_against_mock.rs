use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use asana_cli::mock;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn write_config(home: &Path, base_url: &str) {
    let config_dir = home.join(".asana");
    fs::create_dir_all(&config_dir).expect("config dir should be created");
    fs::write(
        config_dir.join("asana.jsonc"),
        format!(
            r#"{{
  "asanaAccessToken": "mock-token",
  "asanaWorkspaceGid": "1200123456789",
  "asanaBaseUrl": "{base_url}",
  "mode": "live"
}}"#
        ),
    )
    .expect("config should be written");
}

fn home(base_url: &str) -> TempDir {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    write_config(temp.path(), base_url);
    temp
}

fn cwd() -> TempDir {
    tempfile::tempdir().expect("temp dir should be created")
}

fn asana(home: &TempDir, cwd: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");
    cmd.current_dir(cwd.path()).env("HOME", home.path());
    cmd
}

#[tokio::test(flavor = "multi_thread")]
async fn cmd_base_url_runs_representative_operations_against_mock_server() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let handle = mock::server::spawn(
        "127.0.0.1:0".parse::<SocketAddr>().expect("addr parses"),
        temp.path().join("mock-data"),
    )
    .await
    .expect("mock server should start");
    let home = home("http://127.0.0.1:9/api/1.0");
    let cwd = cwd();
    let upload = cwd.path().join("sample.txt");
    fs::write(&upload, "attachment body").expect("upload should be written");

    asana(&home, &cwd)
        .args(["cmd", "--base-url", &handle.base_url, "getWorkspaces"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""mode": "live""#))
        .stdout(predicate::str::contains("Mock Workspace"))
        .stdout(predicate::str::contains("mock-token").not());

    asana(&home, &cwd)
        .args([
            "cmd",
            "--base-url",
            &handle.base_url,
            "createProject",
            "--body",
            r#"{"data":{"workspace":"1200123456789","name":"CLI Project"}}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("CLI Project"));

    asana(&home, &cwd)
        .args([
            "cmd",
            "--base-url",
            &handle.base_url,
            "createTask",
            "--body",
            r#"{"data":{"workspace":"1200123456789","name":"CLI Task"}}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("CLI Task"));

    asana(&home, &cwd)
        .args([
            "cmd",
            "--base-url",
            &handle.base_url,
            "createAttachmentForObject",
            "--parent",
            "task-123",
            "--file",
            &upload.to_string_lossy(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("sample.txt"));

    asana(&home, &cwd)
        .args([
            "cmd",
            "--base-url",
            &handle.base_url,
            "createWebhook",
            "--body",
            r#"{"data":{"resource":"task-123","target":"https://example.test/hook"}}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("mock-webhook-secret"))
        .stdout(predicate::str::contains("https://example.test/hook"));

    handle.shutdown().await.expect("server should shut down");
}
