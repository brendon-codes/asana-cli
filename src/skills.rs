use clap::ValueEnum;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum SkillTarget {
    Codex,
    Claude,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkillFile {
    pub relative_path: &'static str,
    pub contents: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SkillTemplate {
    pub name: &'static str,
    pub files: &'static [SkillFile],
}

const ASANA_CLI_FILES: &[SkillFile] = &[SkillFile {
    relative_path: "SKILL.md",
    contents: include_str!("../.codex/skills/asana-cli/SKILL.md"),
}];

const PROJECT_SKILLS: &[SkillTemplate] = &[SkillTemplate {
    name: "asana-cli",
    files: ASANA_CLI_FILES,
}];

impl SkillTarget {
    pub fn directory(self) -> &'static str {
        match self {
            SkillTarget::Codex => ".codex/skills",
            SkillTarget::Claude => ".claude/skills",
        }
    }
}

pub fn project_skills() -> &'static [SkillTemplate] {
    PROJECT_SKILLS
}
