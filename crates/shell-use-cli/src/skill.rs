use std::path::{Path, PathBuf};

use dialoguer::{theme::ColorfulTheme, Select};

const SKILL_NAME: &str = "shell-use";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallScope {
    Repository,
    Global,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentDirectory {
    Agents,
    Claude,
}

impl AgentDirectory {
    fn name(self) -> &'static str {
        match self {
            Self::Agents => ".agents",
            Self::Claude => ".claude",
        }
    }
}

pub fn add(manifest: &str) -> i32 {
    match add_interactive(manifest) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("shell-use skill: {message}");
            1
        }
    }
}

fn add_interactive(manifest: &str) -> Result<(), String> {
    let theme = ColorfulTheme::default();
    let scope_items = [
        "Repository local (current project)",
        "Global (all projects)",
    ];
    let scope = match Select::with_theme(&theme)
        .with_prompt("Install scope")
        .items(&scope_items)
        .default(0)
        .interact_opt()
        .map_err(|error| format!("could not read install scope: {error}"))?
    {
        Some(0) => InstallScope::Repository,
        Some(1) => InstallScope::Global,
        Some(_) => unreachable!("scope picker returned an unknown item"),
        None => return cancelled(),
    };

    let directory_items = [".agents (GitHub Copilot / Codex)", ".claude (Claude Code)"];
    let directory = match Select::with_theme(&theme)
        .with_prompt("Skills directory")
        .items(&directory_items)
        .default(0)
        .interact_opt()
        .map_err(|error| format!("could not read skills directory: {error}"))?
    {
        Some(0) => AgentDirectory::Agents,
        Some(1) => AgentDirectory::Claude,
        Some(_) => unreachable!("directory picker returned an unknown item"),
        None => return cancelled(),
    };

    let current_dir = std::env::current_dir()
        .map_err(|error| format!("could not resolve current directory: {error}"))?;
    let home = dirs::home_dir();
    let path = install_path(scope, directory, &current_dir, home.as_deref())?;
    write_manifest(&path, manifest)?;
    println!("Installed {SKILL_NAME} skill at {}", path.display());
    Ok(())
}

fn cancelled() -> Result<(), String> {
    println!("Skill installation cancelled.");
    Ok(())
}

fn install_path(
    scope: InstallScope,
    directory: AgentDirectory,
    current_dir: &Path,
    home: Option<&Path>,
) -> Result<PathBuf, String> {
    let root = match scope {
        InstallScope::Repository => current_dir,
        InstallScope::Global => home.ok_or_else(|| {
            "could not resolve the home directory for a global installation".to_string()
        })?,
    };
    Ok(root
        .join(directory.name())
        .join("skills")
        .join(SKILL_NAME)
        .join("SKILL.md"))
}

fn write_manifest(path: &Path, manifest: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid skill path: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    std::fs::write(path, manifest)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "shell-use-skill-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn repository_agents_path_is_under_the_current_directory() {
        let current_dir = Path::new("repo");
        let path = install_path(
            InstallScope::Repository,
            AgentDirectory::Agents,
            current_dir,
            None,
        )
        .expect("repository path");
        assert_eq!(
            path,
            current_dir
                .join(".agents")
                .join("skills")
                .join("shell-use")
                .join("SKILL.md")
        );
    }

    #[test]
    fn global_claude_path_is_under_the_home_directory() {
        let home = Path::new("home").join("tester");
        let path = install_path(
            InstallScope::Global,
            AgentDirectory::Claude,
            Path::new("repo"),
            Some(&home),
        )
        .expect("global path");
        assert_eq!(
            path,
            home.join(".claude")
                .join("skills")
                .join("shell-use")
                .join("SKILL.md")
        );
    }

    #[test]
    fn global_install_requires_a_home_directory() {
        let error = install_path(
            InstallScope::Global,
            AgentDirectory::Agents,
            Path::new("repo"),
            None,
        )
        .expect_err("missing home must fail");
        assert!(error.contains("home directory"));
    }

    #[test]
    fn writing_manifest_creates_directories_and_replaces_existing_content() {
        let root = unique_dir("write");
        let path = root
            .join(".agents")
            .join("skills")
            .join("shell-use")
            .join("SKILL.md");
        write_manifest(&path, "old").expect("initial write");
        write_manifest(&path, "new").expect("replacement write");
        assert_eq!(std::fs::read_to_string(&path).expect("read skill"), "new");
        std::fs::remove_dir_all(root).expect("remove temp directory");
    }
}
