use std::path::{Path, PathBuf};

use dialoguer::{theme::ColorfulTheme, Select};

const SKILL_NAME: &str = "tui-test";

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

pub fn add(manifest: &str, references: &[(&str, &str)]) -> i32 {
    match add_interactive(manifest, references) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("tui-test skill: {message}");
            1
        }
    }
}

pub fn render(manifest: &str, references: &[(&str, &str)]) -> String {
    let manifest = localize_links(manifest, references);
    let mut output = manifest.trim_end().to_string();
    for (_, contents) in references {
        let contents = localize_links(contents, references);
        output.push_str("\n\n---\n\n");
        output.push_str(contents.trim());
    }
    output.push('\n');
    output
}

fn localize_links(document: &str, references: &[(&str, &str)]) -> String {
    let mut output = document.replace("](../SKILL.md)", "](#tui-test)");
    for (path, contents) in references {
        output = output.replace(&format!("]({path}#"), "](#");
        if let Some(anchor) = first_heading_anchor(contents) {
            output = output.replace(&format!("]({path})"), &format!("](#{anchor})"));
        }
    }
    output
}

fn first_heading_anchor(document: &str) -> Option<String> {
    let heading = document.lines().find_map(|line| line.strip_prefix("# "))?;
    let anchor = heading
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if matches!(character, ' ' | '-') {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>();
    (!anchor.is_empty()).then_some(anchor)
}

fn add_interactive(manifest: &str, references: &[(&str, &str)]) -> Result<(), String> {
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
    write_skill(&path, manifest, references)?;
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

fn write_skill(
    manifest_path: &Path,
    manifest: &str,
    references: &[(&str, &str)],
) -> Result<(), String> {
    write_manifest(manifest_path, manifest)?;
    let root = manifest_path
        .parent()
        .ok_or_else(|| format!("invalid skill path: {}", manifest_path.display()))?;
    for (relative_path, contents) in references {
        write_manifest(&root.join(relative_path), contents)?;
    }
    Ok(())
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
            "tui-test-skill-{tag}-{}-{nanos}",
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
                .join("tui-test")
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
                .join("tui-test")
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
            .join("tui-test")
            .join("SKILL.md");
        write_manifest(&path, "old").expect("initial write");
        write_manifest(&path, "new").expect("replacement write");
        assert_eq!(std::fs::read_to_string(&path).expect("read skill"), "new");
        std::fs::remove_dir_all(root).expect("remove temp directory");
    }

    #[test]
    fn writing_skill_installs_manifest_and_references() {
        let root = unique_dir("references");
        let path = root
            .join(".agents")
            .join("skills")
            .join("tui-test")
            .join("SKILL.md");
        let references = [
            ("references/python.md", "python"),
            ("references/javascript.md", "javascript"),
        ];

        write_skill(&path, "router", &references).expect("write complete skill");

        let skill_root = path.parent().expect("skill root");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read skill"),
            "router"
        );
        assert_eq!(
            std::fs::read_to_string(skill_root.join("references/python.md"))
                .expect("read Python reference"),
            "python"
        );
        assert_eq!(
            std::fs::read_to_string(skill_root.join("references/javascript.md"))
                .expect("read JavaScript reference"),
            "javascript"
        );
        std::fs::remove_dir_all(root).expect("remove temp directory");
    }

    #[test]
    fn rendering_skill_includes_the_router_and_all_references() {
        let references = [
            (
                "references/cli.md",
                "# CLI reference\n\n[Router](../SKILL.md)",
            ),
            (
                "references/python.md",
                "# Python reference\n\n## Testing helpers\n",
            ),
        ];

        let rendered = render(
            "---\nname: tui-test\n---\n\n# tui-test\n\n\
             [CLI](references/cli.md)\n\
             [Testing](references/python.md#testing-helpers)\n",
            &references,
        );

        assert_eq!(
            rendered,
            "---\nname: tui-test\n---\n\n# tui-test\n\n\
             [CLI](#cli-reference)\n\
             [Testing](#testing-helpers)\n\n---\n\n\
             # CLI reference\n\n[Router](#tui-test)\n\n---\n\n\
             # Python reference\n\n## Testing helpers\n"
        );
    }
}
