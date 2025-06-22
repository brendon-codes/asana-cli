use std::fs;
use std::net::SocketAddr;

use asana_cli::asana::operation;
use asana_cli::mock;
use reqwest::StatusCode;
use serde_json::{Value, json};
use tempfile::TempDir;

async fn server() -> (TempDir, mock::server::MockServerHandle) {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let data_dir = temp.path().join("mock-data");
    let handle = mock::server::spawn(
        "127.0.0.1:0".parse::<SocketAddr>().expect("addr parses"),
        data_dir,
    )
    .await
    .expect("mock server should start");
    (temp, handle)
}

fn bearer(client: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    client.bearer_auth("mock-token")
}

#[tokio::test(flavor = "multi_thread")]
async fn health_and_auth_behave_deterministically() {
    let (_temp, handle) = server().await;
    let client = reqwest::Client::new();

    let health: Value = client
        .get(format!(
            "{}/-/health",
            handle.base_url.trim_end_matches("/api/1.0")
        ))
        .send()
        .await
        .expect("health request should send")
        .json()
        .await
        .expect("health should be json");
    assert_eq!(health["data"]["ok"], true);

    let unauthorized = client
        .get(format!("{}/workspaces", handle.base_url))
        .send()
        .await
        .expect("request should send");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let unauthorized: Value = unauthorized.json().await.expect("401 should be json");
    assert_eq!(unauthorized["errors"][0]["message"], "Not Authorized");

    handle.shutdown().await.expect("server should shut down");
}

#[tokio::test(flavor = "multi_thread")]
async fn startup_and_shutdown_reset_only_managed_mock_data() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let data_dir = temp.path().join("mock-data");
    fs::create_dir_all(data_dir.join("workspaces/old")).expect("old workspace should be created");
    fs::write(data_dir.join("workspaces/old/task.json"), "{}").expect("old file should be written");
    fs::write(data_dir.join("state.json"), "{}").expect("old state should be written");
    fs::write(data_dir.join("keep.json"), "{}").expect("unmanaged file should be written");

    let handle = mock::server::spawn(
        "127.0.0.1:0".parse::<SocketAddr>().expect("addr parses"),
        data_dir.clone(),
    )
    .await
    .expect("mock server should start");

    assert!(!data_dir.join("workspaces/old/task.json").exists());
    assert!(data_dir.join("keep.json").exists());
    assert!(data_dir.join("state.json").exists());

    let client = reqwest::Client::new();
    bearer(client.post(format!("{}/tasks", handle.base_url)))
        .json(&json!({"data":{"workspace":"1200123456789","name":"Reset me"}}))
        .send()
        .await
        .expect("create task should send");
    assert!(data_dir.join("workspaces/1200123456789/tasks").exists());

    handle.shutdown().await.expect("server should shut down");
    assert!(data_dir.join("keep.json").exists());
    assert!(!data_dir.join("workspaces/1200123456789/tasks").exists());
    assert!(
        data_dir
            .join("workspaces/1200123456789/workspace.json")
            .exists()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn project_and_task_flow_persists_to_json_files_during_one_run() {
    let (_temp, handle) = server().await;
    let client = reqwest::Client::new();

    let project: Value = bearer(client.post(format!("{}/projects", handle.base_url)))
        .json(&json!({"data":{"workspace":"1200123456789","name":"Mock Project"}}))
        .send()
        .await
        .expect("create project should send")
        .json()
        .await
        .expect("project should be json");
    let project_gid = project["data"]["gid"]
        .as_str()
        .expect("project gid should exist");

    let task: Value = bearer(client.post(format!("{}/tasks", handle.base_url)))
        .json(&json!({"data":{"workspace":"1200123456789","name":"Mock Task","projects":[{"gid":project_gid}]}}))
        .send()
        .await
        .expect("create task should send")
        .json()
        .await
        .expect("task should be json");
    let task_gid = task["data"]["gid"].as_str().expect("task gid should exist");

    let fetched: Value = bearer(client.get(format!("{}/tasks/{task_gid}", handle.base_url)))
        .send()
        .await
        .expect("get task should send")
        .json()
        .await
        .expect("task should be json");
    assert_eq!(fetched["data"]["name"], "Mock Task");

    assert!(
        handle
            .data_dir()
            .join(format!(
                "workspaces/1200123456789/projects/{project_gid}/project.json"
            ))
            .exists()
    );
    assert!(
        handle
            .data_dir()
            .join(format!(
                "workspaces/1200123456789/tasks/{task_gid}/task.json"
            ))
            .exists()
    );

    handle.shutdown().await.expect("server should shut down");
}

#[tokio::test(flavor = "multi_thread")]
async fn attachment_upload_accepts_multipart_and_writes_attachment_record() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let upload = temp.path().join("sample.txt");
    fs::write(&upload, "attachment body").expect("upload should be written");
    let data_dir = temp.path().join("mock-data");
    let handle = mock::server::spawn(
        "127.0.0.1:0".parse::<SocketAddr>().expect("addr parses"),
        data_dir,
    )
    .await
    .expect("mock server should start");
    let client = reqwest::Client::new();

    let part = reqwest::multipart::Part::bytes(
        fs::read(&upload).expect("upload bytes should be readable"),
    )
    .file_name("sample.txt");
    let form = reqwest::multipart::Form::new()
        .text("parent", "task-123")
        .part("file", part);
    let attachment: Value = bearer(client.post(format!("{}/attachments", handle.base_url)))
        .multipart(form)
        .send()
        .await
        .expect("attachment request should send")
        .json()
        .await
        .expect("attachment should be json");

    assert_eq!(attachment["data"]["name"], "sample.txt");
    assert_eq!(attachment["data"]["parent"]["gid"], "task-123");
    let stored = fs::read_to_string(
        handle
            .data_dir()
            .join("workspaces/1200123456789/tasks/task-123/attachments.json"),
    )
    .expect("attachments should be stored");
    assert!(stored.contains("sample.txt"));

    handle.shutdown().await.expect("server should shut down");
}

#[tokio::test(flavor = "multi_thread")]
async fn webhook_create_get_delete_returns_deterministic_mock_data() {
    let (_temp, handle) = server().await;
    let client = reqwest::Client::new();

    let created = bearer(client.post(format!("{}/webhooks", handle.base_url)))
        .json(&json!({"data":{"resource":"task-123","target":"https://example.test/hook"}}))
        .send()
        .await
        .expect("webhook create should send");
    assert_eq!(
        created
            .headers()
            .get("x-hook-secret")
            .and_then(|value| value.to_str().ok()),
        Some("mock-webhook-secret")
    );
    let created: Value = created.json().await.expect("webhook should be json");
    let webhook_gid = created["data"]["gid"]
        .as_str()
        .expect("webhook gid should exist");

    let fetched: Value = bearer(client.get(format!("{}/webhooks/{webhook_gid}", handle.base_url)))
        .send()
        .await
        .expect("webhook get should send")
        .json()
        .await
        .expect("webhook should be json");
    assert_eq!(fetched["data"]["target"], "https://example.test/hook");

    let deleted: Value =
        bearer(client.delete(format!("{}/webhooks/{webhook_gid}", handle.base_url)))
            .send()
            .await
            .expect("webhook delete should send")
            .json()
            .await
            .expect("delete should be json");
    assert_eq!(deleted["data"]["deleted"], true);

    handle.shutdown().await.expect("server should shut down");
}

#[tokio::test(flavor = "multi_thread")]
async fn all_registry_operations_return_deterministic_mock_responses() {
    let (_temp, handle) = server().await;
    let client = reqwest::Client::new();

    for operation in &operation::registry().operations {
        let method =
            reqwest::Method::from_bytes(operation.method.as_bytes()).expect("method should parse");
        let url = format!("{}{}", handle.base_url, materialize_path(&operation.path));
        let mut request = bearer(client.request(method, url));

        if operation.accepts_multipart() {
            let part = reqwest::multipart::Part::bytes(b"mock attachment".to_vec())
                .file_name("registry.txt");
            request = request.multipart(
                reqwest::multipart::Form::new()
                    .text("parent", "registry-task")
                    .part("file", part),
            );
        } else if operation.accepts_json_body() {
            request = request.json(&json!({"data":{
                "workspace":"1200123456789",
                "name":"Registry Coverage",
                "resource":"registry-task",
                "target":"https://example.test/registry"
            }}));
        }

        let response = request.send().await.unwrap_or_else(|error| {
            panic!("{} request should send: {error}", operation.operation_id)
        });
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_else(|error| {
            panic!(
                "{} response should be json: {error}",
                operation.operation_id
            )
        });
        assert!(
            status.is_success(),
            "{} should return success, got {status} with {body}",
            operation.operation_id
        );
        assert!(
            body.get("data").is_some(),
            "{} should use Asana data envelope: {body}",
            operation.operation_id
        );
    }

    handle.shutdown().await.expect("server should shut down");
}

fn materialize_path(path: &str) -> String {
    let mut rendered = String::new();
    let mut chars = path.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut name = String::new();
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                name.push(inner);
            }
            rendered.push_str(&mock_gid(&name));
        } else {
            rendered.push(ch);
        }
    }
    rendered
}

fn mock_gid(name: &str) -> String {
    match name {
        "workspace_gid" => "1200123456789".to_string(),
        "task_gid" => "registry-task".to_string(),
        "project_gid" => "registry-project".to_string(),
        "team_gid" => "1200000000002".to_string(),
        "user_gid" => "1200000000001".to_string(),
        "webhook_gid" => "registry-webhook".to_string(),
        "attachment_gid" => "registry-attachment".to_string(),
        _ => format!("mock-{name}"),
    }
}
