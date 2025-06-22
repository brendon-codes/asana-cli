use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use reqwest::Url;
use serde::Serialize;
use serde_json::Value;

use crate::asana::operation::Operation;
use crate::error::{Error, Result};

#[derive(Debug)]
pub struct PreparedRequest {
    pub view: RequestView,
    pub json_body: Option<Value>,
    pub form_fields: Vec<FormFieldView>,
    pub file: Option<PreparedFile>,
}

#[derive(Debug)]
pub struct PreparedFile {
    pub field_name: String,
    pub path: PathBuf,
    pub filename: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestView {
    pub method: String,
    pub url: String,
    pub path: String,
    pub query: Vec<NameValue>,
    pub headers: Vec<NameValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multipart: Option<MultipartView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultipartView {
    pub fields: Vec<FormFieldView>,
    pub files: Vec<FileView>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NameValue {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormFieldView {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
}

pub type NamedArgs = BTreeMap<String, Vec<String>>;

pub fn build_request(
    operation: &Operation,
    base_url: &str,
    token: &str,
    args: &NamedArgs,
    body: Option<Value>,
    file: Option<PathBuf>,
) -> Result<PreparedRequest> {
    validate_known_args(operation, args)?;
    validate_required_args(operation, args, body.as_ref(), file.as_deref())?;

    let path = expand_path(operation, args)?;
    let mut url = join_url(base_url, &path)?;
    let mut query = Vec::new();
    for parameter in operation
        .parameters
        .iter()
        .filter(|parameter| parameter.location == "query")
    {
        if let Some(values) = args.get(&parameter.name) {
            let value = coerce_values(
                &parameter.name,
                &parameter.schema_type,
                parameter.array,
                values,
            )?;
            query.push(NameValue {
                name: parameter.name.clone(),
                value,
            });
        }
    }
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for item in &query {
            pairs.append_pair(&item.name, &item.value);
        }
    }

    if body.is_some() && !operation.accepts_json_body() {
        return Err(Error::Command(format!(
            "{} does not accept a JSON --body",
            operation.operation_id
        )));
    }

    let mut form_fields = Vec::new();
    for parameter in &operation.form_parameters {
        if parameter.format.as_deref() == Some("binary") {
            continue;
        }

        if let Some(values) = args.get(&parameter.name) {
            let value = coerce_values(
                &parameter.name,
                &parameter.schema_type,
                parameter.array,
                values,
            )?;
            form_fields.push(FormFieldView {
                name: parameter.name.clone(),
                value,
            });
        }
    }

    let prepared_file = prepare_file(operation, file)?;
    let multipart = if operation.accepts_multipart() {
        Some(MultipartView {
            fields: form_fields.clone(),
            files: file_view(prepared_file.as_ref())?.into_iter().collect(),
        })
    } else {
        if prepared_file.is_some() {
            return Err(Error::Command(format!(
                "{} does not accept --file",
                operation.operation_id
            )));
        }
        None
    };

    let content_type = if operation.accepts_json_body() && body.is_some() {
        Some("application/json")
    } else {
        None
    };
    let mut headers = vec![
        NameValue {
            name: "Authorization".to_string(),
            value: format!("Bearer {}", redact_token(token)),
        },
        NameValue {
            name: "Accept".to_string(),
            value: "application/json".to_string(),
        },
        NameValue {
            name: "User-Agent".to_string(),
            value: format!("asana-cli/{}", env!("CARGO_PKG_VERSION")),
        },
    ];
    if let Some(content_type) = content_type {
        headers.push(NameValue {
            name: "Content-Type".to_string(),
            value: content_type.to_string(),
        });
    } else if operation.accepts_multipart() {
        headers.push(NameValue {
            name: "Content-Type".to_string(),
            value: "multipart/form-data".to_string(),
        });
    }

    let view = RequestView {
        method: operation.method.clone(),
        url: url.to_string(),
        path,
        query,
        headers,
        body: body.clone(),
        multipart,
    };

    Ok(PreparedRequest {
        view,
        json_body: body,
        form_fields,
        file: prepared_file,
    })
}

fn file_view(file: Option<&PreparedFile>) -> Result<Option<FileView>> {
    let Some(file) = file else {
        return Ok(None);
    };

    Ok(Some(FileView {
        name: file.field_name.clone(),
        filename: file.filename.clone(),
        size_bytes: fs::metadata(&file.path)
            .map_err(|error| {
                Error::Command(format!(
                    "failed to read file metadata for multipart field {}: {error}",
                    file.field_name
                ))
            })?
            .len(),
    }))
}

fn validate_known_args(operation: &Operation, args: &NamedArgs) -> Result<()> {
    let known: BTreeSet<_> = operation
        .parameters
        .iter()
        .chain(operation.form_parameters.iter())
        .map(|parameter| parameter.name.as_str())
        .collect();

    if let Some(unknown) = args.keys().find(|name| !known.contains(name.as_str())) {
        return Err(Error::Command(format!(
            "unknown argument --{} for {}",
            unknown, operation.operation_id
        )));
    }

    Ok(())
}

fn validate_required_args(
    operation: &Operation,
    args: &NamedArgs,
    body: Option<&Value>,
    file: Option<&Path>,
) -> Result<()> {
    for parameter in operation
        .parameters
        .iter()
        .chain(operation.form_parameters.iter())
    {
        if parameter.required
            && parameter.format.as_deref() != Some("binary")
            && !args.contains_key(&parameter.name)
        {
            return Err(Error::Command(format!(
                "missing required argument --{} for {}",
                parameter.name, operation.operation_id
            )));
        }
    }

    if operation.request_body_required && operation.accepts_json_body() && body.is_none() {
        return Err(Error::Command(format!(
            "{} requires --body JSON",
            operation.operation_id
        )));
    }

    if file.is_some() && !operation.accepts_multipart() {
        return Err(Error::Command(format!(
            "{} does not accept --file",
            operation.operation_id
        )));
    }

    Ok(())
}

fn expand_path(operation: &Operation, args: &NamedArgs) -> Result<String> {
    let mut path = operation.path.clone();
    for parameter in operation
        .parameters
        .iter()
        .filter(|parameter| parameter.location == "path")
    {
        let values = args.get(&parameter.name).ok_or_else(|| {
            Error::Command(format!(
                "missing required argument --{} for {}",
                parameter.name, operation.operation_id
            ))
        })?;
        let value = values.first().ok_or_else(|| {
            Error::Command(format!(
                "missing required argument --{} for {}",
                parameter.name, operation.operation_id
            ))
        })?;
        path = path.replace(
            &format!("{{{}}}", parameter.name),
            &percent_encode_path_segment(value),
        );
    }
    Ok(path)
}

fn join_url(base_url: &str, path: &str) -> Result<Url> {
    let mut base = Url::parse(base_url)
        .map_err(|error| Error::Command(format!("invalid base URL {base_url}: {error}")))?;
    base.set_path(&format!(
        "{}/{}",
        base.path().trim_end_matches('/'),
        path.trim_start_matches('/')
    ));
    base.set_query(None);
    Ok(base)
}

fn coerce_values(name: &str, schema_type: &str, array: bool, values: &[String]) -> Result<String> {
    if array {
        let mut items = Vec::new();
        for value in values {
            items.extend(
                value
                    .split(',')
                    .filter(|item| !item.is_empty())
                    .map(str::to_string),
            );
        }
        return Ok(items.join(","));
    }

    let value = values.last().cloned().unwrap_or_default();
    match schema_type {
        "boolean" => match value.as_str() {
            "true" | "false" => Ok(value),
            _ => Err(Error::Command(format!("--{name} must be true or false"))),
        },
        "integer" => {
            value
                .parse::<i64>()
                .map_err(|_| Error::Command(format!("--{name} must be an integer")))?;
            Ok(value)
        }
        _ => Ok(value),
    }
}

fn prepare_file(operation: &Operation, file: Option<PathBuf>) -> Result<Option<PreparedFile>> {
    let Some(path) = file else {
        return Ok(None);
    };

    let Some(parameter) = operation
        .form_parameters
        .iter()
        .find(|parameter| parameter.format.as_deref() == Some("binary"))
    else {
        return Err(Error::Command(format!(
            "{} does not accept --file",
            operation.operation_id
        )));
    };

    let metadata = fs::metadata(&path).map_err(|error| {
        Error::Command(format!(
            "failed to read file for multipart field {}: {error}",
            parameter.name
        ))
    })?;
    if !metadata.is_file() {
        return Err(Error::Command(format!(
            "--file for {} must point to a regular file",
            operation.operation_id
        )));
    }

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Error::Command("--file path must have a valid UTF-8 file name".to_string()))?
        .to_string();

    Ok(Some(PreparedFile {
        field_name: parameter.name.clone(),
        path,
        filename,
    }))
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if keep {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn redact_token(token: &str) -> &'static str {
    let _ = token;
    "<redacted>"
}
