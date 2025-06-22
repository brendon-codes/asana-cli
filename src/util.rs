use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::config::{self, ConfigMode, LoadedConfig};
use crate::error::{Error, Result};
use crate::skills::{self, SkillTarget};

const EXAMPLE_CONFIG: &str = include_str!("../examples/.asana/asana.jsonc");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MakeSkillArgs {
    pub target: SkillTarget,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MakeConfigOutput {
    config_path: String,
    already_existed: bool,
    created: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidateConfigOutput {
    config_path: String,
    valid: bool,
    mode: ConfigMode,
    workspace_gid: String,
    base_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusOutput {
    config_path: String,
    workspace_gid: String,
    base_url: String,
    mode: ConfigMode,
    token: &'static str,
    live_check: Option<LiveCheckOutput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveCheckOutput {
    endpoint: String,
    http_status: u16,
    ok: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MakeSkillOutput {
    target: SkillTarget,
    target_root: String,
    skills: Vec<CreatedSkillOutput>,
    created: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreatedSkillOutput {
    name: &'static str,
    path: String,
    files: usize,
}

pub async fn make_config() -> Result<()> {
    make_config_at(config::default_config_path()?)
}

pub fn make_config_from_home(home: impl AsRef<Path>) -> Result<()> {
    make_config_at(config::default_config_path_from_home(home)?)
}

fn make_config_at(config_path: PathBuf) -> Result<()> {
    let config_dir = config_path.parent().ok_or_else(|| {
        Error::Command(format!(
            "failed to resolve config directory for {}",
            config_path.display()
        ))
    })?;
    fs::create_dir_all(&config_dir).map_err(|error| {
        Error::Command(format!(
            "failed to create config directory {}: {error}",
            config_dir.display()
        ))
    })?;

    let already_existed = config_path.exists();
    if !already_existed {
        fs::write(&config_path, EXAMPLE_CONFIG).map_err(|error| {
            Error::Command(format!(
                "failed to write config {}: {error}",
                config_path.display()
            ))
        })?;
    }

    print_json(&MakeConfigOutput {
        config_path: display_path(&config_path),
        already_existed,
        created: !already_existed,
    })
}

pub async fn validate_config() -> Result<()> {
    let loaded = config::load_default()?;
    print_json(&ValidateConfigOutput {
        config_path: display_path(&loaded.path),
        valid: true,
        mode: loaded.config.mode,
        workspace_gid: loaded.config.asana_workspace_gid,
        base_url: loaded.config.asana_base_url,
    })
}

pub async fn status(base_url: Option<String>) -> Result<()> {
    let mut loaded = config::load_default()?;
    if let Some(base_url) = base_url {
        config::validate_base_url(&base_url)?;
        loaded.config.asana_base_url = base_url;
    }

    let live_check = match loaded.config.mode {
        ConfigMode::Dryrun => None,
        ConfigMode::Live => Some(check_workspace(&loaded).await?),
    };

    print_json(&StatusOutput {
        config_path: display_path(&loaded.path),
        workspace_gid: loaded.config.asana_workspace_gid,
        base_url: loaded.config.asana_base_url,
        mode: loaded.config.mode,
        token: "<redacted>",
        live_check,
    })
}

pub async fn make_skill(args: MakeSkillArgs) -> Result<()> {
    let current_dir = std::env::current_dir()
        .map_err(|error| Error::Command(format!("failed to read current directory: {error}")))?;
    let output = make_skill_from(current_dir, args)?;
    print_json(&output)
}

fn make_skill_from(start: impl AsRef<Path>, args: MakeSkillArgs) -> Result<MakeSkillOutput> {
    let repo_root = config::find_repo_root_from(start)?;
    let target_root = repo_root.join(args.target.directory());

    let mut created = Vec::new();
    for skill in skills::project_skills() {
        let skill_dir = target_root.join(skill.name);
        if skill_dir.exists() {
            fs::remove_dir_all(&skill_dir).map_err(|error| {
                Error::Command(format!(
                    "failed to remove existing skill directory {}: {error}",
                    skill_dir.display()
                ))
            })?;
        }

        for file in skill.files {
            let path = skill_dir.join(file.relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    Error::Command(format!(
                        "failed to create skill directory {}: {error}",
                        parent.display()
                    ))
                })?;
            }

            fs::write(&path, file.contents).map_err(|error| {
                Error::Command(format!(
                    "failed to write skill file {}: {error}",
                    path.display()
                ))
            })?;
        }

        created.push(CreatedSkillOutput {
            name: skill.name,
            path: display_path(&skill_dir),
            files: skill.files.len(),
        });
    }

    Ok(MakeSkillOutput {
        target: args.target,
        target_root: display_path(&target_root),
        skills: created,
        created: true,
    })
}

async fn check_workspace(loaded: &LoadedConfig) -> Result<LiveCheckOutput> {
    let endpoint = format!(
        "{}/workspaces/{}",
        loaded.config.asana_base_url.trim_end_matches('/'),
        loaded.config.asana_workspace_gid
    );

    let response = reqwest::Client::new()
        .get(&endpoint)
        .bearer_auth(&loaded.config.asana_access_token)
        .send()
        .await
        .map_err(|error| {
            Error::Command(format!("failed to call Asana status endpoint: {error}"))
        })?;

    let status = response.status();
    if !status.is_success() {
        return Err(Error::Command(format!(
            "Asana status endpoint returned HTTP {}",
            status.as_u16()
        )));
    }

    Ok(LiveCheckOutput {
        endpoint,
        http_status: status.as_u16(),
        ok: true,
    })
}

fn print_json(output: &impl Serialize) -> Result<()> {
    let value = serde_json::to_value(output).map_err(|error| Error::Unexpected(error.into()))?;
    assert_no_token(&value)?;
    let rendered =
        serde_json::to_string_pretty(&value).map_err(|error| Error::Unexpected(error.into()))?;
    println!("{rendered}");
    Ok(())
}

fn assert_no_token(value: &Value) -> Result<()> {
    let rendered = serde_json::to_string(value).map_err(|error| Error::Unexpected(error.into()))?;
    if rendered.contains("asanaAccessToken") {
        return Err(Error::Command(
            "internal error: attempted to print access token field".to_string(),
        ));
    }

    Ok(())
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}
