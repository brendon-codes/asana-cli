use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::mock::state::MockState;

#[derive(Debug)]
pub struct MockStorage {
    data_dir: PathBuf,
    pub state: MockState,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StateIndex {
    next_gid: u64,
    workspace_count: usize,
    user_count: usize,
    team_count: usize,
    project_count: usize,
    task_count: usize,
    webhook_count: usize,
}

impl MockStorage {
    pub fn reset_new(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let data_dir = data_dir.into();
        reset_managed_data(&data_dir)?;
        let storage = Self {
            data_dir,
            state: MockState::initial(),
        };
        storage.write_all()?;
        Ok(storage)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn reset(&mut self) -> Result<()> {
        reset_managed_data(&self.data_dir)?;
        self.state = MockState::initial();
        self.write_all()
    }

    pub fn persist(&self) -> Result<()> {
        self.write_all()
    }

    fn write_all(&self) -> Result<()> {
        reset_managed_data(&self.data_dir)?;
        fs::create_dir_all(&self.data_dir).map_err(|error| {
            Error::Command(format!(
                "failed to create mock data directory {}: {error}",
                self.data_dir.display()
            ))
        })?;

        write_json(
            &self.data_dir.join("state.json"),
            &StateIndex {
                next_gid: self.state.next_gid,
                workspace_count: self.state.workspaces.len(),
                user_count: self.state.users.len(),
                team_count: self.state.teams.len(),
                project_count: self.state.projects.len(),
                task_count: self.state.tasks.len(),
                webhook_count: self.state.webhooks.len(),
            },
        )?;

        for (workspace_gid, workspace) in &self.state.workspaces {
            let workspace_dir = self.data_dir.join("workspaces").join(workspace_gid);
            write_json(&workspace_dir.join("workspace.json"), workspace)?;
            write_json(
                &workspace_dir.join("users.json"),
                &self.state.users.values().cloned().collect::<Vec<_>>(),
            )?;

            for (team_gid, team) in &self.state.teams {
                write_json(
                    &workspace_dir.join("teams").join(team_gid).join("team.json"),
                    team,
                )?;
            }

            for (project_gid, project) in &self.state.projects {
                let project_dir = workspace_dir.join("projects").join(project_gid);
                write_json(&project_dir.join("project.json"), project)?;
                let sections = self
                    .state
                    .sections
                    .get(project_gid)
                    .cloned()
                    .unwrap_or_default();
                write_json(&project_dir.join("sections.json"), &sections)?;
            }

            let task_gids = self
                .state
                .tasks
                .keys()
                .chain(self.state.stories.keys())
                .chain(self.state.attachments.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            for task_gid in task_gids {
                let task_dir = workspace_dir.join("tasks").join(&task_gid);
                let task = self.state.tasks.get(&task_gid).cloned().unwrap_or_else(
                    || json!({"gid":task_gid,"resource_type":"task","name":"Mock task"}),
                );
                write_json(&task_dir.join("task.json"), &task)?;
                let stories = self
                    .state
                    .stories
                    .get(&task_gid)
                    .cloned()
                    .unwrap_or_default();
                write_json(&task_dir.join("stories.json"), &stories)?;
                let attachments = self
                    .state
                    .attachments
                    .get(&task_gid)
                    .cloned()
                    .unwrap_or_default();
                write_json(&task_dir.join("attachments.json"), &attachments)?;
            }

            write_json(
                &workspace_dir.join("webhooks.json"),
                &self.state.webhooks.values().cloned().collect::<Vec<_>>(),
            )?;
        }

        Ok(())
    }
}

pub fn reset_managed_data(data_dir: &Path) -> Result<()> {
    if !data_dir.exists() {
        fs::create_dir_all(data_dir).map_err(|error| {
            Error::Command(format!(
                "failed to create mock data directory {}: {error}",
                data_dir.display()
            ))
        })?;
        return Ok(());
    }

    let managed_file = data_dir.join("state.json");
    if managed_file.exists() {
        fs::remove_file(&managed_file).map_err(|error| {
            Error::Command(format!(
                "failed to remove mock data file {}: {error}",
                managed_file.display()
            ))
        })?;
    }

    let managed_dir = data_dir.join("workspaces");
    if managed_dir.exists() {
        fs::remove_dir_all(&managed_dir).map_err(|error| {
            Error::Command(format!(
                "failed to remove mock data directory {}: {error}",
                managed_dir.display()
            ))
        })?;
    }

    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Command(format!(
                "failed to create mock data directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    let rendered =
        serde_json::to_string_pretty(value).map_err(|error| Error::Unexpected(error.into()))?;
    fs::write(path, rendered).map_err(|error| {
        Error::Command(format!(
            "failed to write mock data file {}: {error}",
            path.display()
        ))
    })
}

pub fn resource_ref(gid: &str, resource_type: &str, name: &str) -> Value {
    json!({
        "gid": gid,
        "resource_type": resource_type,
        "name": name
    })
}
