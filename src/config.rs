use std::fs;
use std::path::{Path, PathBuf};

use jsonc_parser::{ParseOptions, parse_to_serde_value};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

pub const CONFIG_RELATIVE_PATH: &str = ".asana/asana.jsonc";
pub const DEFAULT_BASE_URL: &str = "https://app.asana.com/api/1.0";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub asana_access_token: String,
    pub asana_workspace_gid: String,
    #[serde(default = "default_base_url")]
    pub asana_base_url: String,
    pub mode: ConfigMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigMode {
    Live,
    Dryrun,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedConfig {
    pub config_dir: PathBuf,
    pub path: PathBuf,
    pub config: Config,
}

fn default_base_url() -> String {
    DEFAULT_BASE_URL.to_string()
}

pub fn find_repo_root_from(start: impl AsRef<Path>) -> Result<PathBuf> {
    let mut current = start
        .as_ref()
        .canonicalize()
        .map_err(|error| Error::Config(format!("failed to resolve current directory: {error}")))?;

    loop {
        if current.join(".git").exists() {
            return Ok(current);
        }

        if !current.pop() {
            return Err(Error::Config(
                "could not find a Git repository root; expected a .git directory in this directory or a parent".to_string(),
            ));
        }
    }
}

pub fn default_config_path_from_home(home: impl AsRef<Path>) -> Result<PathBuf> {
    let home = home.as_ref();
    if home.as_os_str().is_empty() {
        return Err(Error::Config(
            "HOME is unset or empty; cannot resolve ~/.asana/asana.jsonc".to_string(),
        ));
    }

    Ok(home.join(CONFIG_RELATIVE_PATH))
}

pub fn default_config_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| {
        Error::Config("HOME is unset or empty; cannot resolve ~/.asana/asana.jsonc".to_string())
    })?;
    default_config_path_from_home(home)
}

pub fn load_default_from_home(home: impl AsRef<Path>) -> Result<LoadedConfig> {
    let path = default_config_path_from_home(home)?;
    let config_dir = path.parent().map(Path::to_path_buf).ok_or_else(|| {
        Error::Config(format!(
            "failed to resolve config directory for {}",
            path.display()
        ))
    })?;
    let config = load_from_path(&path)?;
    Ok(LoadedConfig {
        config_dir,
        path,
        config,
    })
}

pub fn load_default() -> Result<LoadedConfig> {
    let path = default_config_path()?;
    let config_dir = path.parent().map(Path::to_path_buf).ok_or_else(|| {
        Error::Config(format!(
            "failed to resolve config directory for {}",
            path.display()
        ))
    })?;
    let config = load_from_path(&path)?;
    Ok(LoadedConfig {
        config_dir,
        path,
        config,
    })
}

pub fn load_from_path(path: impl AsRef<Path>) -> Result<Config> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path).map_err(|error| {
        Error::Config(format!("failed to read config {}: {error}", path.display()))
    })?;
    parse_str(&contents).map_err(|error| {
        Error::Config(format!("failed to load config {}: {error}", path.display()))
    })
}

pub fn parse_str(contents: &str) -> Result<Config> {
    let value: Value = parse_to_serde_value(contents, &ParseOptions::default())
        .map_err(|error| Error::Config(error.to_string()))?;
    reject_removed_fields(&value)?;

    let config: Config =
        serde_json::from_value(value).map_err(|error| Error::Config(error.to_string()))?;
    validate(&config)?;
    Ok(config)
}

pub fn validate(config: &Config) -> Result<()> {
    require_non_empty("asanaAccessToken", &config.asana_access_token)?;
    require_non_empty("asanaWorkspaceGid", &config.asana_workspace_gid)?;
    validate_base_url(&config.asana_base_url)?;
    Ok(())
}

pub fn validate_base_url(value: &str) -> Result<()> {
    let url = Url::parse(value).map_err(|error| {
        Error::Config(format!(
            "asanaBaseUrl must be an absolute HTTP or HTTPS URL: {error}"
        ))
    })?;

    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(Error::Config(format!(
            "asanaBaseUrl must use http or https, got {scheme}"
        ))),
    }
}

fn require_non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::Config(format!("{field} must not be empty")));
    }

    Ok(())
}

fn reject_removed_fields(value: &Value) -> Result<()> {
    const REMOVED_FIELDS: [&str; 3] = ["defaultProjectGid", "defaultTeamGid", "defaultUserGid"];

    if let Some(object) = value.as_object() {
        let removed = REMOVED_FIELDS
            .into_iter()
            .filter(|field| object.contains_key(*field))
            .collect::<Vec<_>>();

        if !removed.is_empty() {
            return Err(Error::Config(format!(
                "removed config field(s) are no longer supported: {}",
                removed.join(", ")
            )));
        }
    }

    Ok(())
}
