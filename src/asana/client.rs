use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use crate::asana::request::{NameValue, PreparedRequest};
use crate::error::{Error, Result};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseView {
    pub status: u16,
    pub success: bool,
    pub headers: Vec<NameValue>,
    pub body: Value,
}

pub async fn execute(request: &PreparedRequest, token: &str) -> Result<ResponseView> {
    let method = Method::from_bytes(request.view.method.as_bytes())
        .map_err(|error| Error::Command(format!("invalid HTTP method: {error}")))?;
    let client = reqwest::Client::new();
    let mut builder = client
        .request(method, &request.view.url)
        .bearer_auth(token)
        .header("Accept", "application/json")
        .header(
            "User-Agent",
            format!("asana-cli/{}", env!("CARGO_PKG_VERSION")),
        );

    if let Some(body) = &request.json_body {
        builder = builder.json(body);
    } else if request.view.multipart.is_some() {
        let mut form = reqwest::multipart::Form::new();
        for field in &request.form_fields {
            form = form.text(field.name.clone(), field.value.clone());
        }
        if let Some(file) = &request.file {
            let bytes = std::fs::read(&file.path).map_err(|error| {
                Error::Command(format!(
                    "failed to open multipart file for field {}: {error}",
                    file.field_name
                ))
            })?;
            let part = reqwest::multipart::Part::bytes(bytes).file_name(file.filename.clone());
            form = form.part(file.field_name.clone(), part);
        }
        builder = builder.multipart(form);
    }

    let response = builder
        .send()
        .await
        .map_err(|error| Error::Command(format!("failed to call Asana API: {error}")))?;
    let status = response.status();
    let headers = response_headers(&response);
    let text = response
        .text()
        .await
        .map_err(|error| Error::Command(format!("failed to read Asana response: {error}")))?;
    let body = serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text));

    Ok(ResponseView {
        status: status.as_u16(),
        success: status.is_success(),
        headers,
        body,
    })
}

fn response_headers(response: &reqwest::Response) -> Vec<NameValue> {
    let keep = [
        "x-ratelimit-limit",
        "x-ratelimit-remaining",
        "x-ratelimit-reset",
        "retry-after",
        "asana-change",
        "deprecation",
        "sunset",
        "x-hook-secret",
    ];

    keep.iter()
        .filter_map(|name| {
            response
                .headers()
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .map(|value| NameValue {
                    name: (*name).to_string(),
                    value: value.to_string(),
                })
        })
        .collect()
}
