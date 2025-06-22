use std::fs;
use std::net::SocketAddr;
use std::path::Path;

use asana_cli::mock;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("temp dir should be created")
}

fn write_config(home: &Path, contents: &str) {
    let config_dir = home.join(".asana");
    fs::create_dir_all(&config_dir).expect("config dir should be created");
    fs::write(config_dir.join("asana.jsonc"), contents).expect("config should be written");
}

fn write_repo_local_config(repo: &Path, contents: &str) {
    let config_dir = repo.join(".asana");
    fs::create_dir_all(&config_dir).expect("repo config dir should be created");
    fs::write(config_dir.join("asana.jsonc"), contents).expect("repo config should be written");
}

fn asana(home: &TempDir, cwd: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");
    cmd.current_dir(cwd.path()).env("HOME", home.path());
    cmd
}

fn valid_config(token: &str) -> String {
    format!(
        r#"{{
  // comments are supported
  "asanaAccessToken": "{token}",
  "asanaWorkspaceGid": "1200123456789",
  "asanaBaseUrl": "https://app.asana.com/api/1.0",
  "mode": "dryrun"
}}"#
    )
}

fn live_config(token: &str, base_url: &str) -> String {
    format!(
        r#"{{
  "asanaAccessToken": "{token}",
  "asanaWorkspaceGid": "1200123456789",
  "asanaBaseUrl": "{base_url}",
  "mode": "live"
}}"#
    )
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name);
    fs::read_to_string(path).expect("fixture should be readable")
}

#[test]
fn valid_jsonc_with_comments_passes_validation() {
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(home.path(), &fixture("valid.jsonc"));

    asana(&home, &cwd)
        .args(["util", "validate-config"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            home.path().join(".asana/asana.jsonc").display().to_string(),
        ))
        .stdout(predicate::str::contains(r#""valid": true"#))
        .stdout(predicate::str::contains("fixture-secret-token").not());
}

#[test]
fn missing_required_fields_fail_validation() {
    for (field, contents) in [
        (
            "asanaAccessToken",
            r#"{"asanaWorkspaceGid":"1200","mode":"dryrun"}"#,
        ),
        (
            "asanaWorkspaceGid",
            r#"{"asanaAccessToken":"token","mode":"dryrun"}"#,
        ),
        (
            "mode",
            r#"{"asanaAccessToken":"token","asanaWorkspaceGid":"1200"}"#,
        ),
    ] {
        let home = temp_dir();
        let cwd = temp_dir();
        write_config(home.path(), contents);

        asana(&home, &cwd)
            .args(["util", "validate-config"])
            .assert()
            .failure()
            .stderr(predicate::str::contains(field));
    }
}

#[test]
fn empty_required_fields_fail_validation() {
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(
        home.path(),
        r#"{"asanaAccessToken":" ","asanaWorkspaceGid":"1200","mode":"dryrun"}"#,
    );

    asana(&home, &cwd)
        .args(["util", "validate-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "asanaAccessToken must not be empty",
        ));
}

#[test]
fn empty_workspace_gid_fails_validation() {
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(
        home.path(),
        r#"{"asanaAccessToken":"token","asanaWorkspaceGid":" ","mode":"dryrun"}"#,
    );

    asana(&home, &cwd)
        .args(["util", "validate-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "asanaWorkspaceGid must not be empty",
        ));
}

#[test]
fn removed_default_gid_fields_fail_validation() {
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(
        home.path(),
        r#"{
  "asanaAccessToken": "token",
  "asanaWorkspaceGid": "1200",
  "mode": "dryrun",
  "defaultProjectGid": "project",
  "defaultTeamGid": "team",
  "defaultUserGid": "user"
}"#,
    );

    asana(&home, &cwd)
        .args(["util", "validate-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "removed config field(s) are no longer supported",
        ))
        .stderr(predicate::str::contains("defaultProjectGid"))
        .stderr(predicate::str::contains("defaultTeamGid"))
        .stderr(predicate::str::contains("defaultUserGid"));
}

#[test]
fn omitted_base_url_defaults_to_asana_api_url() {
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(
        home.path(),
        r#"{"asanaAccessToken":"token","asanaWorkspaceGid":"1200","mode":"dryrun"}"#,
    );

    asana(&home, &cwd)
        .args(["util", "validate-config"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            r#""baseUrl": "https://app.asana.com/api/1.0""#,
        ));
}

#[test]
fn invalid_mode_fails_validation() {
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(
        home.path(),
        r#"{"asanaAccessToken":"token","asanaWorkspaceGid":"1200","mode":"preview"}"#,
    );

    asana(&home, &cwd)
        .args(["util", "validate-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("preview"));
}

#[test]
fn invalid_base_url_fails_validation() {
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(
        home.path(),
        r#"{"asanaAccessToken":"token","asanaWorkspaceGid":"1200","asanaBaseUrl":"ftp://example.test","mode":"dryrun"}"#,
    );

    asana(&home, &cwd)
        .args(["util", "validate-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "asanaBaseUrl must use http or https",
        ));
}

#[test]
fn make_config_creates_default_file_under_home_outside_git_repository() {
    let home = temp_dir();
    let cwd = temp_dir();

    asana(&home, &cwd)
        .args(["util", "make-config"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            home.path().join(".asana/asana.jsonc").display().to_string(),
        ))
        .stdout(predicate::str::contains(r#""created": true"#))
        .stdout(predicate::str::contains("gitignorePath").not());

    let config_path = home.path().join(".asana/asana.jsonc");
    let contents = fs::read_to_string(&config_path).expect("config should exist");
    assert!(contents.contains(r#""mode": "dryrun""#));
    assert!(!contents.contains("defaultProjectGid"));
    assert!(!contents.contains("defaultTeamGid"));
    assert!(!contents.contains("defaultUserGid"));
    assert!(!home.path().join(".asana/.gitignore").exists());
}

#[test]
fn make_config_does_not_overwrite_existing_file() {
    let home = temp_dir();
    let cwd = temp_dir();
    let existing = r#"{"asanaAccessToken":"keep","asanaWorkspaceGid":"keep","mode":"dryrun"}"#;
    write_config(home.path(), existing);

    asana(&home, &cwd)
        .args(["util", "make-config"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""alreadyExisted": true"#));

    let contents =
        fs::read_to_string(home.path().join(".asana/asana.jsonc")).expect("config should exist");
    assert_eq!(contents, existing);
}

#[test]
fn validate_config_does_not_fallback_to_repo_local_config() {
    let home = temp_dir();
    let repo = temp_dir();
    fs::create_dir(repo.path().join(".git")).expect(".git marker should be created");
    write_repo_local_config(repo.path(), &valid_config("repo-local-token"));

    asana(&home, &repo)
        .args(["util", "validate-config"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            home.path().join(".asana/asana.jsonc").display().to_string(),
        ))
        .stderr(predicate::str::contains("repo-local-token").not());
}

#[test]
fn status_dryrun_redacts_token_and_skips_network() {
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(home.path(), &valid_config("very-secret-token"));

    asana(&home, &cwd)
        .args(["util", "status", "--base-url", "http://127.0.0.1:9/api/1.0"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            home.path().join(".asana/asana.jsonc").display().to_string(),
        ))
        .stdout(predicate::str::contains(r#""mode": "dryrun""#))
        .stdout(predicate::str::contains(r#""token": "<redacted>""#))
        .stdout(predicate::str::contains("very-secret-token").not());
}

#[tokio::test(flavor = "multi_thread")]
async fn status_live_checks_workspace_against_mock_server_and_redacts_token() {
    let data = tempfile::tempdir().expect("temp dir should be created");
    let handle = mock::server::spawn(
        "127.0.0.1:0".parse::<SocketAddr>().expect("addr parses"),
        data.path().join("mock-data"),
    )
    .await
    .expect("mock server should start");
    let home = temp_dir();
    let cwd = temp_dir();
    write_config(
        home.path(),
        &live_config("very-secret-token", &handle.base_url),
    );

    asana(&home, &cwd)
        .args(["util", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""mode": "live""#))
        .stdout(predicate::str::contains(r#""token": "<redacted>""#))
        .stdout(predicate::str::contains(r#""httpStatus": 200"#))
        .stdout(predicate::str::contains(r#""ok": true"#))
        .stdout(predicate::str::contains(&handle.base_url))
        .stdout(predicate::str::contains("very-secret-token").not());

    handle.shutdown().await.expect("server should shut down");
}
