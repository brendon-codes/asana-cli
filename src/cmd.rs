use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use crate::asana::client::{self, ResponseView};
use crate::asana::operation::{self, Operation};
use crate::asana::request::{self, NamedArgs, RequestView};
use crate::config::{self, ConfigMode};
use crate::error::{Error, Result};
use crate::output::OutputFormat;

#[derive(Debug)]
struct CmdArgs {
    operation_id: Option<String>,
    named: NamedArgs,
    output_format: OutputFormat,
    base_url: Option<String>,
    body: Option<String>,
    file: Option<PathBuf>,
    help: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CmdOutput {
    operation_id: String,
    mode: String,
    request: RequestView,
    #[serde(skip_serializing_if = "Option::is_none")]
    dry_run: Option<DryRunOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response: Option<ResponseView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DryRunOutput {
    success: bool,
}

pub async fn run_from(args: &[OsString]) -> Result<()> {
    let args = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .map(str::to_string)
                .ok_or_else(|| Error::Command("cmd arguments must be valid UTF-8".to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    let args = parse_args(&args)?;

    if args.help {
        match args.operation_id.as_deref() {
            Some(operation_id) => {
                let operation = resolve_operation(operation_id)?;
                print_operation_help(operation);
            }
            None => print_cmd_help(),
        }
        return Ok(());
    }

    let operation_id = args.operation_id.as_deref().ok_or_else(|| {
        Error::Command("missing operation ID; run `asana cmd --help`".to_string())
    })?;
    let operation = resolve_operation(operation_id)?;
    let body = parse_body(args.body.as_deref())?;

    let mut loaded = config::load_default()?;
    if let Some(base_url) = args.base_url {
        config::validate_base_url(&base_url)?;
        loaded.config.asana_base_url = base_url;
    }

    let request = request::build_request(
        operation,
        &loaded.config.asana_base_url,
        &loaded.config.asana_access_token,
        &args.named,
        body,
        args.file,
    )?;

    match loaded.config.mode {
        ConfigMode::Dryrun => {
            let output = CmdOutput {
                operation_id: operation.operation_id.clone(),
                mode: "dryrun".to_string(),
                request: request.view,
                dry_run: Some(DryRunOutput { success: true }),
                response: None,
            };
            print_output(&output, args.output_format)
        }
        ConfigMode::Live => {
            let response = client::execute(&request, &loaded.config.asana_access_token).await?;
            let success = response.success;
            let output = CmdOutput {
                operation_id: operation.operation_id.clone(),
                mode: "live".to_string(),
                request: request.view,
                dry_run: None,
                response: Some(response),
            };
            print_output(&output, args.output_format)?;
            if success {
                Ok(())
            } else {
                Err(Error::Command(format!(
                    "{} returned a non-success HTTP status",
                    operation.operation_id
                )))
            }
        }
    }
}

fn parse_args(values: &[String]) -> Result<CmdArgs> {
    let mut args = CmdArgs {
        operation_id: None,
        named: BTreeMap::new(),
        output_format: OutputFormat::Json,
        base_url: None,
        body: None,
        file: None,
        help: false,
    };

    let mut index = 0;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "-h" | "--help" => {
                args.help = true;
                index += 1;
            }
            "--json" => {
                args.output_format = OutputFormat::Json;
                index += 1;
            }
            "--markdown" => {
                args.output_format = OutputFormat::Markdown;
                index += 1;
            }
            "--text" => {
                args.output_format = OutputFormat::Text;
                index += 1;
            }
            "--base-url" => {
                args.base_url = Some(take_value(values, &mut index, "--base-url")?);
            }
            "--body" => {
                args.body = Some(take_value(values, &mut index, "--body")?);
            }
            "--file" => {
                args.file = Some(PathBuf::from(take_value(values, &mut index, "--file")?));
            }
            _ if value.starts_with("--base-url=") => {
                args.base_url = Some(value["--base-url=".len()..].to_string());
                index += 1;
            }
            _ if value.starts_with("--body=") => {
                args.body = Some(value["--body=".len()..].to_string());
                index += 1;
            }
            _ if value.starts_with("--file=") => {
                args.file = Some(PathBuf::from(value["--file=".len()..].to_string()));
                index += 1;
            }
            _ if value.starts_with("--") => {
                let (name, parsed_value, consumed) = parse_named_arg(values, index)?;
                args.named.entry(name).or_default().push(parsed_value);
                index += consumed;
            }
            _ => {
                if args.operation_id.is_some() {
                    return Err(Error::Command(format!(
                        "unexpected positional argument {value:?}; operation parameters must use --name value or --name=value"
                    )));
                }
                args.operation_id = Some(value.clone());
                index += 1;
            }
        }
    }

    Ok(args)
}

fn take_value(values: &[String], index: &mut usize, flag: &str) -> Result<String> {
    let value = values
        .get(*index + 1)
        .ok_or_else(|| Error::Command(format!("{flag} requires a value")))?;
    *index += 2;
    Ok(value.clone())
}

fn parse_named_arg(values: &[String], index: usize) -> Result<(String, String, usize)> {
    let flag = &values[index];
    let raw = flag.trim_start_matches("--");
    if raw.is_empty() {
        return Err(Error::Command("empty argument name".to_string()));
    }

    if let Some((name, value)) = raw.split_once('=') {
        validate_arg_name(name)?;
        return Ok((name.to_string(), value.to_string(), 1));
    }

    validate_arg_name(raw)?;
    if let Some(value) = values.get(index + 1)
        && !value.starts_with("--")
    {
        return Ok((raw.to_string(), value.clone(), 2));
    }

    Ok((raw.to_string(), "true".to_string(), 1))
}

fn validate_arg_name(name: &str) -> Result<()> {
    let valid = name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(Error::Command(format!("invalid argument name --{name}")))
    }
}

fn resolve_operation(operation_id: &str) -> Result<&'static Operation> {
    if let Some(operation) = operation::find_operation(operation_id) {
        return Ok(operation);
    }

    match operation::find_operation_case_insensitive(operation_id) {
        Ok(operation) => Ok(operation),
        Err(0) => Err(Error::Command(format!(
            "unknown operation ID {operation_id:?}"
        ))),
        Err(_) => Err(Error::Command(format!(
            "operation ID {operation_id:?} is ambiguous when matched case-insensitively"
        ))),
    }
}

fn parse_body(body: Option<&str>) -> Result<Option<Value>> {
    body.map(|body| {
        serde_json::from_str(body)
            .map_err(|error| Error::Command(format!("--body must be valid JSON: {error}")))
    })
    .transpose()
}

fn print_output(output: &CmdOutput, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => {
            let rendered = serde_json::to_string_pretty(output)
                .map_err(|error| Error::Unexpected(error.into()))?;
            println!("{rendered}");
        }
        OutputFormat::Markdown | OutputFormat::Text => print_markdown(output)?,
    }

    Ok(())
}

fn print_markdown(output: &CmdOutput) -> Result<()> {
    println!("# {}", output.operation_id);
    println!();
    println!("| Field | Value |");
    println!("| --- | --- |");
    println!("| Mode | {} |", output.mode);
    println!("| Method | {} |", output.request.method);
    println!("| URL | `{}` |", output.request.url);
    if let Some(response) = &output.response {
        println!("| HTTP status | {} |", response.status);
        println!("| Success | {} |", response.success);
    }
    if output.dry_run.is_some() {
        println!("| Dry run | true |");
    }
    println!();
    println!("## Request");
    println!();
    print_json_block(&output.request)?;
    if let Some(response) = &output.response {
        println!();
        println!("## Response");
        println!();
        print_json_block(response)?;
    }
    Ok(())
}

fn print_json_block(value: &impl Serialize) -> Result<()> {
    let rendered =
        serde_json::to_string_pretty(value).map_err(|error| Error::Unexpected(error.into()))?;
    println!("```json");
    println!("{rendered}");
    println!("```");
    Ok(())
}

fn print_cmd_help() {
    let registry = operation::registry();
    println!("Run Asana REST API command operations");
    println!();
    println!(
        "Registry: {} operations from {} retrieved {}",
        registry.operation_count, registry.source, registry.retrieved_at
    );
    println!();
    println!(
        "Usage: asana cmd [--json|--markdown|--text] [--base-url URL] <operationId> [--param value ...] [--body JSON] [--file PATH]"
    );
    println!();
    println!("Options:");
    println!("  --json              Print JSON output (default)");
    println!("  --markdown          Print Markdown output");
    println!("  --text              Alias for console-friendly Markdown output");
    println!("  --base-url URL      Override the configured Asana API base URL");
    println!("  --body JSON         JSON request body for application/json operations");
    println!("  --file PATH         File payload for multipart/form-data operations");
    println!();
    println!("Operations:");

    let mut groups: BTreeMap<&str, Vec<&Operation>> = BTreeMap::new();
    for operation in &registry.operations {
        groups.entry(&operation.tag).or_default().push(operation);
    }
    for (tag, operations) in groups {
        println!();
        println!("{tag}:");
        for operation in operations {
            println!(
                "  {:<42} {:<6} {:<45} {}",
                operation.operation_id, operation.method, operation.path, operation.summary
            );
        }
    }
}

fn print_operation_help(operation: &Operation) {
    println!("{}", operation.operation_id);
    println!();
    println!("{}", operation.summary);
    println!();
    println!(
        "Usage: asana cmd [--json|--markdown|--text] {} [arguments]",
        operation.operation_id
    );
    println!();
    println!("Request:");
    println!("  Method: {}", operation.method);
    println!("  Path: {}", operation.path);
    if operation.deprecated {
        println!("  Deprecated: true");
    }
    if !operation.request_content_types.is_empty() {
        println!(
            "  Request content types: {}",
            operation.request_content_types.join(", ")
        );
    }
    if !operation.response_content_types.is_empty() {
        println!(
            "  Response content types: {}",
            operation.response_content_types.join(", ")
        );
    }
    if !operation.scopes.is_empty() {
        println!("  OAuth scopes: {}", operation.scopes.join(", "));
    }

    let required: Vec<_> = operation
        .parameters
        .iter()
        .chain(operation.form_parameters.iter())
        .filter(|parameter| parameter.required)
        .collect();
    let optional: Vec<_> = operation
        .parameters
        .iter()
        .chain(operation.form_parameters.iter())
        .filter(|parameter| !parameter.required)
        .collect();

    if !required.is_empty() {
        println!();
        println!("Required arguments:");
        for parameter in required {
            print_parameter(parameter);
        }
    }

    if operation.request_body_required && operation.accepts_json_body() {
        println!("  --body <json> (required)");
    }

    if !optional.is_empty() {
        println!();
        println!("Optional arguments:");
        let mut seen = BTreeSet::new();
        for parameter in optional {
            if seen.insert(parameter.name.as_str()) {
                print_parameter(parameter);
            }
        }
    }

    if operation.accepts_json_body() && !operation.request_body_required {
        println!("  --body <json>");
    }
    if operation.accepts_multipart() {
        println!("  --file <path>");
    }
}

fn print_parameter(parameter: &crate::asana::operation::Parameter) {
    let repeated = if parameter.array { "[]" } else { "" };
    let deprecated = if parameter.deprecated {
        " deprecated"
    } else {
        ""
    };
    let style = parameter
        .style
        .as_deref()
        .map(|style| format!(" style={style}"))
        .unwrap_or_default();
    let explode = parameter
        .explode
        .map(|explode| format!(" explode={explode}"))
        .unwrap_or_default();
    let description = one_line(&parameter.description);
    println!(
        "  --{} <{}{}> ({}){}{}{}{}",
        parameter.name,
        parameter.schema_type,
        repeated,
        parameter.location,
        deprecated,
        style,
        explode,
        if description.is_empty() {
            String::new()
        } else {
            format!(" - {description}")
        }
    );
}

fn one_line(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect()
}
