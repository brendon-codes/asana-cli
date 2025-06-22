use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn home(mode: &str, token: &str, base_url: &str) -> TempDir {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    write_config(temp.path(), mode, token, base_url);
    temp
}

fn cwd() -> TempDir {
    tempfile::tempdir().expect("temp dir should be created")
}

fn write_config(home: &Path, mode: &str, token: &str, base_url: &str) {
    let config_dir = home.join(".asana");
    fs::create_dir_all(&config_dir).expect("config dir should be created");
    fs::write(
        config_dir.join("asana.jsonc"),
        format!(
            r#"{{
  "asanaAccessToken": "{token}",
  "asanaWorkspaceGid": "1200123456789",
  "asanaBaseUrl": "{base_url}",
  "mode": "{mode}"
}}"#
        ),
    )
    .expect("config should be written");
}

fn asana(home: &TempDir, cwd: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");
    cmd.current_dir(cwd.path()).env("HOME", home.path());
    cmd
}

#[test]
fn registry_operation_ids_match_openapi_snapshot() {
    let reference: Value = serde_json::from_str(include_str!("../references/asana-openapi.json"))
        .expect("OpenAPI snapshot should parse");
    let registry: Value = serde_json::from_str(include_str!("../src/asana/operations.json"))
        .expect("registry should parse");

    let mut reference_ids = Vec::new();
    for path in reference["paths"]
        .as_object()
        .expect("paths should be an object")
        .values()
    {
        for method in ["get", "post", "put", "patch", "delete"] {
            if let Some(operation) = path.get(method) {
                reference_ids.push(
                    operation["operationId"]
                        .as_str()
                        .expect("operation should have operationId")
                        .to_string(),
                );
            }
        }
    }
    reference_ids.sort();

    let mut registry_ids = registry["operations"]
        .as_array()
        .expect("operations should be an array")
        .iter()
        .map(|operation| {
            operation["operationId"]
                .as_str()
                .expect("registry operation should have operationId")
                .to_string()
        })
        .collect::<Vec<_>>();
    registry_ids.sort();

    assert_eq!(reference_ids.len(), 247);
    assert_eq!(registry["operationCount"], 247);
    assert_eq!(registry_ids, reference_ids);
}

#[test]
fn cmd_help_lists_registry_operations() {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");

    cmd.args(["cmd", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("247 operations"))
        .stdout(predicate::str::contains("getTask"))
        .stdout(predicate::str::contains("createAttachmentForObject"));
}

#[test]
fn operation_help_is_generated_from_registry() {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");

    cmd.args(["cmd", "createTask", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("POST"))
        .stdout(predicate::str::contains("/tasks"))
        .stdout(predicate::str::contains("--body <json> (required)"));
}

#[test]
fn representative_operations_dry_run_successfully() {
    let home = home("dryrun", "very-secret-token", "http://127.0.0.1:9/api/1.0");
    let cwd = cwd();
    let attachment = cwd.path().join("sample.txt");
    fs::write(&attachment, "attachment body").expect("attachment fixture should be written");
    let attachment = attachment.to_string_lossy().to_string();

    let cases: Vec<Vec<&str>> = vec![
        vec!["cmd", "getTask", "--task_gid", "123"],
        vec!["cmd", "getTasks", "--workspace", "1200123456789"],
        vec![
            "cmd",
            "createTask",
            "--body",
            r#"{"data":{"workspace":"1200123456789","name":"Sample task"}}"#,
        ],
        vec![
            "cmd",
            "updateTask",
            "--task_gid",
            "123",
            "--body",
            r#"{"data":{"name":"Renamed task"}}"#,
        ],
        vec!["cmd", "deleteTask", "--task_gid", "123"],
        vec!["cmd", "getProjects", "--workspace", "1200123456789"],
        vec![
            "cmd",
            "createProject",
            "--body",
            r#"{"data":{"workspace":"1200123456789","name":"Sample project"}}"#,
        ],
        vec![
            "cmd",
            "createWebhook",
            "--body",
            r#"{"data":{"resource":"123","target":"https://example.com/hook"}}"#,
        ],
        vec![
            "cmd",
            "createAttachmentForObject",
            "--parent",
            "123",
            "--file",
            &attachment,
        ],
    ];

    for case in cases {
        asana(&home, &cwd)
            .args(case)
            .assert()
            .success()
            .stdout(predicate::str::contains(r#""mode": "dryrun""#))
            .stdout(predicate::str::contains(r#""Authorization""#))
            .stdout(predicate::str::contains("very-secret-token").not());
    }
}

#[test]
fn missing_required_args_and_unknown_args_fail() {
    let home = home("dryrun", "very-secret-token", "http://127.0.0.1:9/api/1.0");
    let cwd = cwd();

    asana(&home, &cwd)
        .args(["cmd", "getTask"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "missing required argument --task_gid",
        ));

    asana(&home, &cwd)
        .args(["cmd", "getTask", "--task_gid", "123", "--bogus", "true"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown argument --bogus"));
}

#[test]
fn query_types_and_output_formats_are_validated() {
    let home = home("dryrun", "very-secret-token", "http://127.0.0.1:9/api/1.0");
    let cwd = cwd();

    asana(&home, &cwd)
        .args([
            "cmd",
            "--markdown",
            "getTasks",
            "--workspace",
            "1200123456789",
            "--limit",
            "10",
            "--opt_pretty",
            "true",
            "--opt_fields",
            "gid,name",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("# getTasks"))
        .stdout(predicate::str::contains("workspace=1200123456789"))
        .stdout(predicate::str::contains("opt_fields=gid%2Cname"));

    asana(&home, &cwd)
        .args(["cmd", "--text", "getTask", "--task_gid", "123"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# getTask"));

    asana(&home, &cwd)
        .args([
            "cmd",
            "getTasks",
            "--workspace",
            "1200123456789",
            "--limit",
            "not-an-int",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--limit must be an integer"));
}

#[test]
fn malformed_json_body_fails_before_request_building() {
    let home = home("dryrun", "very-secret-token", "http://127.0.0.1:9/api/1.0");
    let cwd = cwd();

    asana(&home, &cwd)
        .args(["cmd", "createTask", "--body", "{not-json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--body must be valid JSON"));
}

#[test]
fn live_non_success_status_prints_response_and_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener.local_addr().expect("test server should have addr");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test server should accept");
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request).expect("request should be read");
        let body = r#"{"errors":[{"message":"bad request"}]}"#;
        write!(
            stream,
            "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("response should be written");
    });

    let home = home(
        "live",
        "very-secret-token",
        &format!("http://{addr}/api/1.0"),
    );
    let cwd = cwd();

    asana(&home, &cwd)
        .args(["cmd", "getTask", "--task_gid", "123"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(r#""status": 400"#))
        .stdout(predicate::str::contains("bad request"))
        .stderr(predicate::str::contains("non-success HTTP status"));

    handle.join().expect("server thread should finish");
}

#[test]
fn dry_run_multipart_redacts_file_path_and_reports_metadata() {
    let home = home("dryrun", "very-secret-token", "http://127.0.0.1:9/api/1.0");
    let cwd = cwd();
    let attachment = cwd.path().join("private/path/sample.txt");
    fs::create_dir_all(attachment.parent().expect("fixture parent should exist"))
        .expect("fixture parent should be created");
    fs::write(&attachment, "hello").expect("attachment fixture should be written");

    asana(&home, &cwd)
        .args([
            "cmd",
            "createAttachmentForObject",
            "--parent",
            "123",
            "--file",
            &attachment.to_string_lossy(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""filename": "sample.txt""#))
        .stdout(predicate::str::contains(r#""sizeBytes": 5"#))
        .stdout(predicate::str::contains("private/path").not());
}

#[test]
fn base_url_routes_live_requests_to_local_server() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
    let addr = listener.local_addr().expect("test server should have addr");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test server should accept");
        let mut request = [0_u8; 4096];
        let bytes = stream.read(&mut request).expect("request should be read");
        let request = String::from_utf8_lossy(&request[..bytes]);
        assert!(request.starts_with("GET /api/1.0/tasks/123?opt_fields=name HTTP/1.1"));
        assert!(request.contains("authorization: Bearer very-secret-token"));
        let body = r#"{"data":{"gid":"123","name":"From mock"}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nx-ratelimit-remaining: 42\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("response should be written");
    });

    let home = home(
        "live",
        "very-secret-token",
        &format!("http://{addr}/api/1.0"),
    );
    let cwd = cwd();

    asana(&home, &cwd)
        .args([
            "cmd",
            "getTask",
            "--task_gid",
            "123",
            "--opt_fields",
            "name",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""mode": "live""#))
        .stdout(predicate::str::contains(r#""status": 200"#))
        .stdout(predicate::str::contains("From mock"))
        .stdout(predicate::str::contains("very-secret-token").not());

    handle.join().expect("server thread should finish");
}
