use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRegistry {
    pub source: String,
    pub retrieved_at: String,
    pub operation_count: usize,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub operation_id: String,
    pub method: String,
    pub path: String,
    pub tag: String,
    pub summary: String,
    pub deprecated: bool,
    pub parameters: Vec<Parameter>,
    pub form_parameters: Vec<Parameter>,
    pub request_content_types: Vec<String>,
    pub has_request_body: bool,
    pub request_body_required: bool,
    pub response_content_types: Vec<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: String,
    pub required: bool,
    pub schema_type: String,
    pub array: bool,
    pub description: String,
    pub deprecated: bool,
    pub style: Option<String>,
    pub explode: Option<bool>,
    pub format: Option<String>,
}

static REGISTRY: LazyLock<OperationRegistry> = LazyLock::new(|| {
    let registry: OperationRegistry = serde_json::from_str(include_str!("operations.json"))
        .expect("checked-in Asana operation registry should be valid JSON");
    assert_eq!(
        registry.operation_count,
        registry.operations.len(),
        "checked-in Asana operation registry count should match operations"
    );
    registry
});

pub fn registry() -> &'static OperationRegistry {
    &REGISTRY
}

pub fn find_operation(operation_id: &str) -> Option<&'static Operation> {
    REGISTRY
        .operations
        .iter()
        .find(|operation| operation.operation_id == operation_id)
}

pub fn find_operation_case_insensitive(operation_id: &str) -> Result<&'static Operation, usize> {
    let matches: Vec<_> = REGISTRY
        .operations
        .iter()
        .filter(|operation| operation.operation_id.eq_ignore_ascii_case(operation_id))
        .collect();

    match matches.as_slice() {
        [operation] => Ok(operation),
        _ => Err(matches.len()),
    }
}

impl Operation {
    pub fn accepts_json_body(&self) -> bool {
        self.request_content_types
            .iter()
            .any(|content_type| content_type == "application/json")
    }

    pub fn accepts_multipart(&self) -> bool {
        self.request_content_types
            .iter()
            .any(|content_type| content_type == "multipart/form-data")
    }

    pub fn parameter(&self, name: &str) -> Option<&Parameter> {
        self.parameters
            .iter()
            .chain(self.form_parameters.iter())
            .find(|parameter| parameter.name == name)
    }
}
