use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::{Body, Bytes, to_bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;

use crate::asana::operation::{self, Operation};
use crate::mock::storage::{MockStorage, resource_ref};

#[derive(Clone)]
pub struct AppState {
    storage: Arc<Mutex<MockStorage>>,
}

#[derive(Clone, Debug)]
struct MatchedOperation {
    operation: &'static Operation,
    path: String,
    path_params: BTreeMap<String, String>,
    query: BTreeMap<String, Vec<String>>,
    body: Option<Value>,
    multipart: Option<MultipartRequest>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MultipartRequest {
    fields: BTreeMap<String, String>,
    files: Vec<MultipartFile>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MultipartFile {
    field: String,
    filename: String,
    size_bytes: usize,
}

pub fn router(storage: MockStorage) -> Router {
    Router::new()
        .route("/-/health", any(health))
        .fallback(any(handle_registry_operation))
        .layer(TraceLayer::new_for_http())
        .with_state(AppState {
            storage: Arc::new(Mutex::new(storage)),
        })
}

async fn health() -> Json<Value> {
    Json(json!({"data":{"ok":true,"service":"asana-cli mock server"}}))
}

async fn handle_registry_operation(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    if !is_authorized(&parts.headers) {
        return json_status(
            StatusCode::UNAUTHORIZED,
            json!({"errors":[{"message":"Not Authorized"}]}),
            HeaderMap::new(),
        );
    }

    let method = parts.method;
    let original_path = parts.uri.path().to_string();
    let path = normalize_api_path(&original_path);
    let query = parse_query(parts.uri.query().unwrap_or_default());
    let headers = parts.headers;

    let Some((operation, path_params)) = match_operation(&method, &path) else {
        return json_status(
            StatusCode::NOT_FOUND,
            json!({"errors":[{"message":format!("No mock route for {method} {original_path}")}]}),
            HeaderMap::new(),
        );
    };

    let body_bytes = match to_bytes(body, 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return json_status(
                StatusCode::BAD_REQUEST,
                json!({"errors":[{"message":format!("Failed to read request body: {error}")}]}),
                HeaderMap::new(),
            );
        }
    };
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let (body, multipart) = parse_body(&content_type, body_bytes);

    let matched = MatchedOperation {
        operation,
        path,
        path_params,
        query,
        body,
        multipart,
    };

    let mut storage = state.storage.lock().await;
    let response = dispatch(&mut storage, &matched);
    if let Err(error) = storage.persist() {
        return json_status(
            StatusCode::INTERNAL_SERVER_ERROR,
            json!({"errors":[{"message":error.to_string()}]}),
            HeaderMap::new(),
        );
    }

    let mut headers = HeaderMap::new();
    if matched.operation.operation_id == "createWebhook" {
        headers.insert(
            "x-hook-secret",
            "mock-webhook-secret"
                .parse()
                .expect("static header is valid"),
        );
    }
    json_status(StatusCode::OK, response, headers)
}

fn dispatch(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    match request.operation.operation_id.as_str() {
        "getWorkspaces" => list(storage.state.workspaces.values().cloned().collect()),
        "getWorkspace" => envelope(workspace(storage, request)),
        "getUsers" | "getUsersForWorkspace" | "getUsersForTeam" => {
            list(storage.state.users.values().cloned().collect())
        }
        "getTeamsForWorkspace" => list(storage.state.teams.values().cloned().collect()),
        "getProjects" | "getProjectsForWorkspace" | "getProjectsForTeam" => {
            list(storage.state.projects.values().cloned().collect())
        }
        "getProjectsForTask" => list(projects_for_task(storage, request)),
        "createProject" | "createProjectForWorkspace" | "createProjectForTeam" => {
            envelope(create_project(storage, request))
        }
        "getProject" => envelope(project(storage, request)),
        "updateProject" => envelope(update_project(storage, request)),
        "deleteProject" => envelope(delete_project(storage, request)),
        "getSectionsForProject" => list(sections_for_project(storage, request)),
        "getTasks"
        | "getTasksForProject"
        | "getTasksForSection"
        | "getTasksForTag"
        | "getTasksForUserTaskList" => list(tasks(storage, request)),
        "createTask" => envelope(create_task(storage, request)),
        "getTask" | "getTaskForCustomID" => envelope(task(storage, request)),
        "updateTask" => envelope(update_task(storage, request)),
        "deleteTask" => envelope(delete_task(storage, request)),
        "getStoriesForTask" => list(stories_for_task(storage, request)),
        "createAttachmentForObject" => envelope(create_attachment(storage, request)),
        "getAttachmentsForObject" => list(attachments_for_object(storage, request)),
        "getAttachment" => envelope(attachment(storage, request)),
        "deleteAttachment" => envelope(delete_attachment(storage, request)),
        "createWebhook" => envelope(create_webhook(storage, request)),
        "getWebhook" => envelope(webhook(storage, request)),
        "getWebhooks" => list(storage.state.webhooks.values().cloned().collect()),
        "deleteWebhook" => envelope(delete_webhook(storage, request)),
        _ => generic_response(request),
    }
}

fn workspace(storage: &MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "workspace_gid")
        .unwrap_or_else(|| storage.state.default_workspace_gid());
    storage
        .state
        .workspaces
        .get(&gid)
        .cloned()
        .unwrap_or_else(|| resource_ref(&gid, "workspace", &format!("Mock workspace {gid}")))
}

fn create_project(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = storage.state.next_gid();
    let data = request_data(request);
    let workspace_gid = data
        .get("workspace")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| path_param(request, "workspace_gid"))
        .unwrap_or_else(|| storage.state.default_workspace_gid());
    let team_gid = data
        .get("team")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| path_param(request, "team_gid"))
        .unwrap_or_else(|| storage.state.default_team_gid());
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Mock Project")
        .to_string();
    let mut project = data_object(data);
    project.insert("gid".to_string(), Value::String(gid.clone()));
    project.insert(
        "resource_type".to_string(),
        Value::String("project".to_string()),
    );
    project.insert("name".to_string(), Value::String(name));
    project.insert(
        "workspace".to_string(),
        resource_ref(&workspace_gid, "workspace", "Mock Workspace"),
    );
    project.insert(
        "team".to_string(),
        resource_ref(&team_gid, "team", "Mock Team"),
    );
    let project = Value::Object(project);
    storage.state.projects.insert(gid.clone(), project.clone());
    storage
        .state
        .sections
        .entry(gid.clone())
        .or_insert_with(|| vec![resource_ref(&format!("{gid}01"), "section", "Mock Section")]);
    project
}

fn project(storage: &MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "project_gid").unwrap_or_else(|| "mock-project".to_string());
    storage
        .state
        .projects
        .get(&gid)
        .cloned()
        .unwrap_or_else(|| resource_ref(&gid, "project", &format!("Mock project {gid}")))
}

fn update_project(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "project_gid").unwrap_or_else(|| "mock-project".to_string());
    let mut project = project(storage, request);
    merge_data(&mut project, request);
    storage.state.projects.insert(gid, project.clone());
    project
}

fn delete_project(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "project_gid").unwrap_or_else(|| "mock-project".to_string());
    storage.state.projects.remove(&gid);
    json!({"gid":gid,"resource_type":"project","deleted":true})
}

fn sections_for_project(storage: &MockStorage, request: &MatchedOperation) -> Vec<Value> {
    path_param(request, "project_gid")
        .and_then(|gid| storage.state.sections.get(&gid).cloned())
        .unwrap_or_default()
}

fn projects_for_task(storage: &MockStorage, request: &MatchedOperation) -> Vec<Value> {
    let Some(task_gid) = path_param(request, "task_gid") else {
        return Vec::new();
    };
    let Some(task) = storage.state.tasks.get(&task_gid) else {
        return Vec::new();
    };
    task.get("projects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn tasks(storage: &MockStorage, request: &MatchedOperation) -> Vec<Value> {
    let all = storage.state.tasks.values().cloned().collect::<Vec<_>>();
    if let Some(project_gid) =
        path_param(request, "project_gid").or_else(|| first_query(request, "project"))
    {
        return all
            .into_iter()
            .filter(|task| task_has_project(task, &project_gid))
            .collect();
    }
    all
}

fn create_task(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = storage.state.next_gid();
    let data = request_data(request);
    let workspace_gid = data
        .get("workspace")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| first_query(request, "workspace"))
        .unwrap_or_else(|| storage.state.default_workspace_gid());
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Mock Task")
        .to_string();
    let mut task = data_object(data);
    task.insert("gid".to_string(), Value::String(gid.clone()));
    task.insert(
        "resource_type".to_string(),
        Value::String("task".to_string()),
    );
    task.insert("name".to_string(), Value::String(name));
    task.insert(
        "workspace".to_string(),
        resource_ref(&workspace_gid, "workspace", "Mock Workspace"),
    );
    if !task.contains_key("projects") {
        task.insert("projects".to_string(), Value::Array(Vec::new()));
    }
    let task = Value::Object(task);
    storage.state.tasks.insert(gid, task.clone());
    task
}

fn task(storage: &MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "task_gid")
        .or_else(|| path_param(request, "custom_id"))
        .unwrap_or_else(|| "mock-task".to_string());
    storage
        .state
        .tasks
        .get(&gid)
        .cloned()
        .unwrap_or_else(|| resource_ref(&gid, "task", &format!("Mock task {gid}")))
}

fn update_task(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "task_gid").unwrap_or_else(|| "mock-task".to_string());
    let mut task = task(storage, request);
    merge_data(&mut task, request);
    storage.state.tasks.insert(gid, task.clone());
    task
}

fn delete_task(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "task_gid").unwrap_or_else(|| "mock-task".to_string());
    storage.state.tasks.remove(&gid);
    json!({"gid":gid,"resource_type":"task","deleted":true})
}

fn stories_for_task(storage: &MockStorage, request: &MatchedOperation) -> Vec<Value> {
    path_param(request, "task_gid")
        .and_then(|gid| storage.state.stories.get(&gid).cloned())
        .unwrap_or_default()
}

fn create_attachment(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = storage.state.next_gid();
    let parent = multipart_field(request, "parent")
        .or_else(|| first_query(request, "parent"))
        .unwrap_or_else(|| "mock-parent".to_string());
    let file = request
        .multipart
        .as_ref()
        .and_then(|multipart| multipart.files.first())
        .cloned();
    let filename = file
        .as_ref()
        .map(|file| file.filename.clone())
        .unwrap_or_else(|| "mock-attachment".to_string());
    let attachment = json!({
        "gid": gid,
        "resource_type": "attachment",
        "name": filename,
        "parent": {"gid": parent, "resource_type": "task"},
        "download_url": null,
        "size": file.map(|file| file.size_bytes).unwrap_or_default()
    });
    storage
        .state
        .attachments
        .entry(parent)
        .or_default()
        .push(attachment.clone());
    attachment
}

fn attachments_for_object(storage: &MockStorage, request: &MatchedOperation) -> Vec<Value> {
    path_param(request, "parent")
        .or_else(|| first_query(request, "parent"))
        .and_then(|parent| storage.state.attachments.get(&parent).cloned())
        .unwrap_or_default()
}

fn attachment(storage: &MockStorage, request: &MatchedOperation) -> Value {
    let gid =
        path_param(request, "attachment_gid").unwrap_or_else(|| "mock-attachment".to_string());
    storage
        .state
        .attachments
        .values()
        .flat_map(|attachments| attachments.iter())
        .find(|attachment| attachment.get("gid").and_then(Value::as_str) == Some(gid.as_str()))
        .cloned()
        .unwrap_or_else(|| resource_ref(&gid, "attachment", &format!("Mock attachment {gid}")))
}

fn delete_attachment(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid =
        path_param(request, "attachment_gid").unwrap_or_else(|| "mock-attachment".to_string());
    for attachments in storage.state.attachments.values_mut() {
        attachments.retain(|attachment| {
            attachment.get("gid").and_then(Value::as_str) != Some(gid.as_str())
        });
    }
    json!({"gid":gid,"resource_type":"attachment","deleted":true})
}

fn create_webhook(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = storage.state.next_gid();
    let data = request_data(request);
    let resource = data
        .get("resource")
        .and_then(Value::as_str)
        .unwrap_or("mock-resource")
        .to_string();
    let target = data
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("https://example.test/mock-webhook")
        .to_string();
    let webhook = json!({
        "gid": gid,
        "resource_type": "webhook",
        "resource": {"gid": resource},
        "target": target,
        "active": true
    });
    storage.state.webhooks.insert(gid, webhook.clone());
    webhook
}

fn webhook(storage: &MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "webhook_gid").unwrap_or_else(|| "mock-webhook".to_string());
    storage
        .state
        .webhooks
        .get(&gid)
        .cloned()
        .unwrap_or_else(|| json!({"gid":gid,"resource_type":"webhook","active":true}))
}

fn delete_webhook(storage: &mut MockStorage, request: &MatchedOperation) -> Value {
    let gid = path_param(request, "webhook_gid").unwrap_or_else(|| "mock-webhook".to_string());
    storage.state.webhooks.remove(&gid);
    json!({"gid":gid,"resource_type":"webhook","deleted":true})
}

fn generic_response(request: &MatchedOperation) -> Value {
    let mut data = json!({
        "operationId": request.operation.operation_id,
        "method": request.operation.method,
        "path": request.path,
        "tag": request.operation.tag,
        "pathParams": request.path_params,
        "query": request.query,
    });
    if let Some(body) = &request.body {
        data["body"] = body.clone();
    }
    if let Some(multipart) = &request.multipart {
        data["multipart"] = serde_json::to_value(multipart).unwrap_or(Value::Null);
    }

    if looks_like_collection(request.operation) {
        json!({"data":[data],"next_page":null})
    } else {
        envelope(data)
    }
}

fn envelope(data: Value) -> Value {
    json!({"data":data})
}

fn list(data: Vec<Value>) -> Value {
    json!({"data":data,"next_page":null})
}

fn json_status(status: StatusCode, body: Value, headers: HeaderMap) -> Response {
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().extend(headers);
    response
}

fn is_authorized(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.starts_with("Bearer ") && value["Bearer ".len()..].trim().len() > 0
        })
}

fn normalize_api_path(path: &str) -> String {
    let stripped = path.strip_prefix("/api/1.0").unwrap_or(path);
    if stripped.is_empty() {
        "/".to_string()
    } else {
        stripped.to_string()
    }
}

fn match_operation(
    method: &Method,
    path: &str,
) -> Option<(&'static Operation, BTreeMap<String, String>)> {
    operation::registry()
        .operations
        .iter()
        .find_map(|operation| {
            if operation.method != method.as_str() {
                return None;
            }
            match_path(&operation.path, path).map(|params| (operation, params))
        })
}

fn match_path(pattern: &str, actual: &str) -> Option<BTreeMap<String, String>> {
    let pattern_segments = segments(pattern);
    let actual_segments = segments(actual);
    if pattern_segments.len() != actual_segments.len() {
        return None;
    }

    let mut params = BTreeMap::new();
    for (pattern, actual) in pattern_segments.iter().zip(actual_segments.iter()) {
        if pattern.starts_with('{') && pattern.ends_with('}') {
            params.insert(
                pattern
                    .trim_start_matches('{')
                    .trim_end_matches('}')
                    .to_string(),
                (*actual).to_string(),
            );
        } else if pattern != actual {
            return None;
        }
    }

    Some(params)
}

fn segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn parse_query(query: &str) -> BTreeMap<String, Vec<String>> {
    let mut parsed = BTreeMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        parsed
            .entry(percent_decode(name))
            .or_insert_with(Vec::new)
            .push(percent_decode(value));
    }
    parsed
}

fn percent_decode(value: &str) -> String {
    let mut decoded = String::new();
    let mut bytes = value.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => decoded.push(' '),
            b'%' => {
                let high = bytes.next();
                let low = bytes.next();
                if let (Some(high), Some(low)) = (high, low)
                    && let Ok(hex) = std::str::from_utf8(&[high, low])
                    && let Ok(value) = u8::from_str_radix(hex, 16)
                {
                    decoded.push(value as char);
                }
            }
            _ => decoded.push(byte as char),
        }
    }
    decoded
}

fn parse_body(content_type: &str, bytes: Bytes) -> (Option<Value>, Option<MultipartRequest>) {
    if bytes.is_empty() {
        return (None, None);
    }
    if content_type.starts_with("application/json") {
        return (serde_json::from_slice(&bytes).ok(), None);
    }
    if content_type.starts_with("multipart/form-data") {
        return (None, parse_multipart(content_type, &bytes));
    }
    (
        Some(Value::String(String::from_utf8_lossy(&bytes).to_string())),
        None,
    )
}

fn parse_multipart(content_type: &str, bytes: &[u8]) -> Option<MultipartRequest> {
    let boundary = content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary="))?
        .trim_matches('"')
        .to_string();
    let marker = format!("--{boundary}");
    let body = String::from_utf8_lossy(bytes);
    let mut fields = BTreeMap::new();
    let mut files = Vec::new();

    for part in body.split(&marker) {
        let part = part.trim_matches('\r').trim_matches('\n');
        if part.is_empty() || part == "--" {
            continue;
        }
        let Some((headers, value)) = part.split_once("\r\n\r\n") else {
            continue;
        };
        let value = value
            .trim_end_matches("--")
            .trim_end_matches('\n')
            .trim_end_matches('\r');
        let Some(disposition) = headers.lines().find(|line| {
            line.to_ascii_lowercase()
                .starts_with("content-disposition:")
        }) else {
            continue;
        };
        let Some(name) = disposition_param(disposition, "name") else {
            continue;
        };
        if let Some(filename) = disposition_param(disposition, "filename") {
            files.push(MultipartFile {
                field: name,
                filename,
                size_bytes: value.as_bytes().len(),
            });
        } else {
            fields.insert(name, value.to_string());
        }
    }

    Some(MultipartRequest { fields, files })
}

fn disposition_param(header: &str, name: &str) -> Option<String> {
    header.split(';').map(str::trim).find_map(|part| {
        let (key, value) = part.split_once('=')?;
        if key == name {
            Some(value.trim_matches('"').to_string())
        } else {
            None
        }
    })
}

fn path_param(request: &MatchedOperation, name: &str) -> Option<String> {
    request.path_params.get(name).cloned()
}

fn first_query(request: &MatchedOperation, name: &str) -> Option<String> {
    request
        .query
        .get(name)
        .and_then(|values| values.first())
        .cloned()
}

fn multipart_field(request: &MatchedOperation, name: &str) -> Option<String> {
    request
        .multipart
        .as_ref()
        .and_then(|multipart| multipart.fields.get(name))
        .cloned()
}

fn request_data(request: &MatchedOperation) -> &Value {
    request
        .body
        .as_ref()
        .and_then(|body| body.get("data"))
        .unwrap_or(&Value::Null)
}

fn data_object(data: &Value) -> Map<String, Value> {
    data.as_object().cloned().unwrap_or_default()
}

fn merge_data(target: &mut Value, request: &MatchedOperation) {
    let data = request_data(request);
    if let (Some(target), Some(data)) = (target.as_object_mut(), data.as_object()) {
        for (key, value) in data {
            target.insert(key.clone(), value.clone());
        }
    }
}

fn task_has_project(task: &Value, project_gid: &str) -> bool {
    task.get("projects")
        .and_then(Value::as_array)
        .is_some_and(|projects| {
            projects
                .iter()
                .any(|project| project.get("gid").and_then(Value::as_str) == Some(project_gid))
        })
}

fn looks_like_collection(operation: &Operation) -> bool {
    operation.method == "GET"
        && (operation.operation_id.ends_with('s')
            || operation.operation_id.contains("For")
            || !operation.path.contains("_gid}"))
}

pub fn registry_routes_covered() -> bool {
    operation::registry()
        .operations
        .iter()
        .all(|operation| match_path(&operation.path, &operation.path).is_some())
}
