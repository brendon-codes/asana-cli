use std::fs;
use std::path::Path;

use asana_cli::asana::operation::{self, Operation, Parameter};
use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn home() -> TempDir {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    fs::create_dir(temp.path().join(".asana")).expect("config dir should be created");
    fs::write(
        temp.path().join(".asana/asana.jsonc"),
        r#"{
  "asanaAccessToken": "all-operations-secret-token",
  "asanaWorkspaceGid": "1200123456789",
  "asanaBaseUrl": "http://127.0.0.1:9/api/1.0",
  "mode": "dryrun"
}"#,
    )
    .expect("config should be written");
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

#[test]
fn registry_matches_openapi_snapshot_without_missing_or_extra_operations() {
    let reference: Value = serde_json::from_str(include_str!("../references/asana-openapi.json"))
        .expect("OpenAPI snapshot should parse");
    let registry = operation::registry();

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

    let mut registry_ids = registry
        .operations
        .iter()
        .map(|operation| operation.operation_id.clone())
        .collect::<Vec<_>>();
    registry_ids.sort();

    assert_eq!(registry.operation_count, reference_ids.len());
    assert_eq!(registry.operations.len(), reference_ids.len());
    assert_eq!(registry_ids, reference_ids);
}

#[test]
fn every_registry_operation_has_generated_help() {
    let home = home();
    let cwd = cwd();

    for operation in &operation::registry().operations {
        assert!(
            !operation.summary.trim().is_empty(),
            "{} should have a summary for help output",
            operation.operation_id
        );

        let output = asana(&home, &cwd)
            .args(["cmd", &operation.operation_id, "--help"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let help = String::from_utf8(output).expect("help should be UTF-8");

        assert!(
            help.contains(&operation.operation_id),
            "{} help should include the operation ID",
            operation.operation_id
        );
        assert!(
            help.contains(&operation.method),
            "{} help should include the HTTP method",
            operation.operation_id
        );
        assert!(
            help.contains(&operation.path),
            "{} help should include the route path",
            operation.operation_id
        );
        for parameter in operation
            .parameters
            .iter()
            .chain(operation.form_parameters.iter())
            .filter(|parameter| parameter.required && parameter.format.as_deref() != Some("binary"))
        {
            assert!(
                help.contains(&format!("--{}", parameter.name)),
                "{} help should include required argument --{}",
                operation.operation_id,
                parameter.name
            );
        }
        if operation.accepts_json_body() {
            assert!(
                help.contains("--body <json>"),
                "{} help should include JSON body syntax",
                operation.operation_id
            );
        }
        if operation.accepts_multipart() {
            assert!(
                help.contains("--file <path>"),
                "{} help should include multipart file syntax",
                operation.operation_id
            );
        }
    }
}

#[test]
fn every_registry_operation_can_build_deterministic_dry_run_json() {
    let home = home();
    let cwd = cwd();
    let upload = cwd.path().join("registry-upload.txt");
    fs::write(&upload, "registry attachment").expect("upload fixture should be written");

    for operation in &operation::registry().operations {
        let args = dry_run_args(operation, &upload);
        let output = asana(&home, &cwd)
            .args(&args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let rendered = String::from_utf8(output).expect("output should be UTF-8");
        let value: Value = serde_json::from_str(&rendered).unwrap_or_else(|error| {
            panic!(
                "{} dry-run output should be JSON: {error}\n{rendered}",
                operation.operation_id
            )
        });

        assert_eq!(value["operationId"], operation.operation_id);
        assert_eq!(value["mode"], "dryrun");
        assert_eq!(value["dryRun"]["success"], true);
        assert_eq!(value["request"]["method"], operation.method);
        assert!(
            !rendered.contains("all-operations-secret-token"),
            "{} should redact access tokens",
            operation.operation_id
        );

        if operation.accepts_json_body() && operation.has_request_body {
            assert!(
                value["request"].get("body").is_some(),
                "{} should include the JSON fixture body",
                operation.operation_id
            );
        }
        if operation.accepts_multipart() {
            assert!(
                value["request"].get("multipart").is_some(),
                "{} should include the multipart fixture",
                operation.operation_id
            );
        }
    }
}

fn dry_run_args(operation: &Operation, upload: &Path) -> Vec<String> {
    let mut args = vec!["cmd".to_string(), operation.operation_id.clone()];
    for parameter in operation
        .parameters
        .iter()
        .chain(operation.form_parameters.iter())
        .filter(|parameter| parameter.required && parameter.format.as_deref() != Some("binary"))
    {
        args.push(format!("--{}", parameter.name));
        args.push(parameter_value(parameter));
    }

    if operation.accepts_json_body() && operation.has_request_body {
        args.push("--body".to_string());
        args.push(json_body(operation).to_string());
    }

    if operation.accepts_multipart() {
        args.push("--file".to_string());
        args.push(upload.display().to_string());
    }

    args
}

fn parameter_value(parameter: &Parameter) -> String {
    if parameter.array {
        return "gid,name".to_string();
    }

    match parameter.schema_type.as_str() {
        "boolean" => "true".to_string(),
        "integer" => "10".to_string(),
        _ => match parameter.name.as_str() {
            "workspace" | "workspace_gid" => "1200123456789".to_string(),
            "team" | "team_gid" => "1200000000002".to_string(),
            "user" | "user_gid" => "1200000000001".to_string(),
            "task" | "task_gid" | "parent" | "resource" | "target" => "task-123".to_string(),
            "project" | "project_gid" => "project-123".to_string(),
            "resource_type" | "type" => "task".to_string(),
            "target_task" => "task-456".to_string(),
            "target_project" => "project-456".to_string(),
            "target_section" => "section-456".to_string(),
            "redirect_uri" | "target_url" | "url" => "https://example.test/asana".to_string(),
            name if name.ends_with("_gid") => format!("{name}-123"),
            name => format!("{name}-value"),
        },
    }
}

fn json_body(operation: &Operation) -> Value {
    serde_json::json!({
        "data": {
            "workspace": "1200123456789",
            "name": format!("{} dry-run fixture", operation.operation_id),
            "resource": "task-123",
            "target": "https://example.test/webhook",
            "projects": [{"gid": "project-123"}],
            "followers": ["1200000000001"],
            "members": ["1200000000001"],
            "gid": "fixture-gid"
        }
    })
}
