use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const DEFAULT_WORKSPACE_GID: &str = "1200123456789";
const DEFAULT_USER_GID: &str = "1200000000001";
const DEFAULT_TEAM_GID: &str = "1200000000002";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MockState {
    pub next_gid: u64,
    pub workspaces: BTreeMap<String, Value>,
    pub users: BTreeMap<String, Value>,
    pub teams: BTreeMap<String, Value>,
    pub projects: BTreeMap<String, Value>,
    pub sections: BTreeMap<String, Vec<Value>>,
    pub tasks: BTreeMap<String, Value>,
    pub stories: BTreeMap<String, Vec<Value>>,
    pub attachments: BTreeMap<String, Vec<Value>>,
    pub webhooks: BTreeMap<String, Value>,
}

impl MockState {
    pub fn initial() -> Self {
        let workspace = json!({
            "gid": DEFAULT_WORKSPACE_GID,
            "resource_type": "workspace",
            "name": "Mock Workspace"
        });
        let user = json!({
            "gid": DEFAULT_USER_GID,
            "resource_type": "user",
            "name": "Mock User",
            "email": "mock.user@example.test",
            "workspaces": [{"gid": DEFAULT_WORKSPACE_GID, "resource_type": "workspace", "name": "Mock Workspace"}]
        });
        let team = json!({
            "gid": DEFAULT_TEAM_GID,
            "resource_type": "team",
            "name": "Mock Team",
            "organization": {"gid": DEFAULT_WORKSPACE_GID, "resource_type": "workspace", "name": "Mock Workspace"}
        });

        let mut workspaces = BTreeMap::new();
        workspaces.insert(DEFAULT_WORKSPACE_GID.to_string(), workspace);
        let mut users = BTreeMap::new();
        users.insert(DEFAULT_USER_GID.to_string(), user);
        let mut teams = BTreeMap::new();
        teams.insert(DEFAULT_TEAM_GID.to_string(), team);

        Self {
            next_gid: 1200000001000,
            workspaces,
            users,
            teams,
            projects: BTreeMap::new(),
            sections: BTreeMap::new(),
            tasks: BTreeMap::new(),
            stories: BTreeMap::new(),
            attachments: BTreeMap::new(),
            webhooks: BTreeMap::new(),
        }
    }

    pub fn next_gid(&mut self) -> String {
        let gid = self.next_gid.to_string();
        self.next_gid += 1;
        gid
    }

    pub fn default_workspace_gid(&self) -> String {
        self.workspaces
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| DEFAULT_WORKSPACE_GID.to_string())
    }

    pub fn default_team_gid(&self) -> String {
        self.teams
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| DEFAULT_TEAM_GID.to_string())
    }
}
