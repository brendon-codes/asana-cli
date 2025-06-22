use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn repo() -> TempDir {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    fs::create_dir(temp.path().join(".git")).expect(".git marker should be created");
    temp
}

fn run_make_skill(repo: &Path, target: &str) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("asana").expect("binary should build");
    cmd.current_dir(repo)
        .args(["util", "make-skill", target])
        .assert()
}

#[test]
fn skill_files_exist_with_valid_frontmatter() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let skill = "asana-cli";
    let skill_path = manifest.join(".codex/skills").join(skill).join("SKILL.md");
    let contents = fs::read_to_string(&skill_path).expect("skill should be readable");

    assert!(
        contents.starts_with("---\n"),
        "{} should start with YAML frontmatter",
        skill_path.display()
    );
    assert!(
        contents.contains(&format!("name: {skill}")),
        "{} should declare its name",
        skill_path.display()
    );
    assert!(
        contents.contains("description:"),
        "{} should declare a description",
        skill_path.display()
    );
    assert!(
        contents.contains("Use this skill to answer end-user questions"),
        "{} should focus on CLI end users",
        skill_path.display()
    );
}

#[test]
fn agents_md_mentions_project_skill_triggers() {
    let contents = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("AGENTS.md"))
        .expect("AGENTS.md should be readable");

    assert!(contents.contains("Use `$asana-cli`"));
    assert!(contents.contains("Use `$rust`"));
    assert!(!contents.contains("Use `$asana-api`"));
}

#[test]
fn make_skill_codex_copies_project_skill() {
    let repo = repo();

    run_make_skill(repo.path(), "codex")
        .success()
        .stdout(predicate::str::contains(r#""target": "codex""#))
        .stdout(predicate::str::contains("asana-cli"));

    assert_copied_skill(repo.path(), ".codex/skills", "asana-cli");
    assert!(!repo.path().join(".codex/skills/asana-api").exists());
}

#[test]
fn make_skill_claude_copies_project_skill() {
    let repo = repo();

    run_make_skill(repo.path(), "claude")
        .success()
        .stdout(predicate::str::contains(r#""target": "claude""#))
        .stdout(predicate::str::contains("asana-cli"));

    assert_copied_skill(repo.path(), ".claude/skills", "asana-cli");
    assert!(!repo.path().join(".claude/skills/asana-api").exists());
}

#[test]
fn make_skill_replaces_existing_destination_skill() {
    let repo = repo();
    let existing = repo.path().join(".codex/skills/asana-cli");
    fs::create_dir_all(&existing).expect("existing skill dir should be created");
    fs::write(existing.join("SKILL.md"), "stale skill").expect("stale skill should be written");
    fs::write(existing.join("stale.txt"), "stale extra file")
        .expect("stale extra file should be written");

    run_make_skill(repo.path(), "codex")
        .success()
        .stdout(predicate::str::contains(r#""target": "codex""#))
        .stdout(predicate::str::contains("asana-cli"));

    let skill_md = fs::read_to_string(existing.join("SKILL.md")).expect("SKILL.md should exist");
    assert!(skill_md.contains("name: asana-cli"));
    assert!(skill_md.contains("Use this skill to answer end-user questions"));
    assert!(!existing.join("stale.txt").exists());
}

fn assert_copied_skill(repo: &Path, target_root: &str, skill: &str) {
    let skill_dir = repo.join(target_root).join(skill);
    let skill_md = fs::read_to_string(skill_dir.join("SKILL.md")).expect("SKILL.md should exist");

    assert!(skill_md.contains(&format!("name: {skill}")));
    assert!(!skill_dir.join("references").exists());
}
