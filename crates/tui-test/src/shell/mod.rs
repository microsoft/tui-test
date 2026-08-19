use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::home_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    Bash,
    Powershell,
    Pwsh,
    Cmd,
    Fish,
    Zsh,
    Xonsh,
    Elvish,
    Nushell,
}

impl Shell {
    pub fn as_str(&self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Powershell => "powershell",
            Shell::Pwsh => "pwsh",
            Shell::Cmd => "cmd",
            Shell::Fish => "fish",
            Shell::Zsh => "zsh",
            Shell::Xonsh => "xonsh",
            Shell::Elvish => "elvish",
            Shell::Nushell => "nushell",
        }
    }

    /// The character used to submit a typed command line.
    pub fn return_char(&self) -> &'static str {
        match self {
            Shell::Xonsh => "\n",
            _ => "\r",
        }
    }
}

pub fn default_shell() -> Shell {
    if cfg!(windows) {
        Shell::Powershell
    } else if cfg!(target_os = "macos") {
        Shell::Zsh
    } else {
        Shell::Bash
    }
}

pub fn scripts_dir() -> PathBuf {
    home_dir().join("shell")
}

fn zdotdir() -> PathBuf {
    home_dir().join("zsh")
}

/// Materialize the bundled shell-integration scripts into the home directory.
pub fn write_integration_scripts() -> std::io::Result<()> {
    let dir = scripts_dir();
    std::fs::create_dir_all(&dir)?;
    let files: &[(&str, &str)] = &[
        (
            "shellIntegration.bash",
            include_str!("../../shell/shellIntegration.bash"),
        ),
        (
            "shellIntegration.fish",
            include_str!("../../shell/shellIntegration.fish"),
        ),
        (
            "shellIntegration.ps1",
            include_str!("../../shell/shellIntegration.ps1"),
        ),
        (
            "shellIntegration.xsh",
            include_str!("../../shell/shellIntegration.xsh"),
        ),
        (
            "shellIntegration.elv",
            include_str!("../../shell/shellIntegration.elv"),
        ),
        (
            "shellIntegration.nu",
            include_str!("../../shell/shellIntegration.nu"),
        ),
        (
            "shellIntegration-rc.zsh",
            include_str!("../../shell/shellIntegration-rc.zsh"),
        ),
        (
            "shellIntegration-profile.zsh",
            include_str!("../../shell/shellIntegration-profile.zsh"),
        ),
        (
            "shellIntegration-env.zsh",
            include_str!("../../shell/shellIntegration-env.zsh"),
        ),
        (
            "shellIntegration-login.zsh",
            include_str!("../../shell/shellIntegration-login.zsh"),
        ),
    ];
    for (name, body) in files {
        std::fs::write(dir.join(name), body)?;
    }
    Ok(())
}

fn setup_zsh_dotfiles() -> std::io::Result<()> {
    let dir = zdotdir();
    std::fs::create_dir_all(&dir)?;
    let src = scripts_dir();
    std::fs::copy(src.join("shellIntegration-rc.zsh"), dir.join(".zshrc"))?;
    std::fs::copy(
        src.join("shellIntegration-profile.zsh"),
        dir.join(".zprofile"),
    )?;
    std::fs::copy(src.join("shellIntegration-env.zsh"), dir.join(".zshenv"))?;
    std::fs::copy(src.join("shellIntegration-login.zsh"), dir.join(".zlogin"))?;
    Ok(())
}

pub struct Launch {
    pub target: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Compute how to launch a shell with integration wired in.
pub fn shell_launch(shell: Shell) -> anyhow::Result<Launch> {
    write_integration_scripts()?;
    let dir = scripts_dir();
    let mut env: Vec<(String, String)> = Vec::new();

    let (target, args) = match shell {
        Shell::Bash => {
            let target = if cfg!(windows) {
                git_bash_path()?
            } else {
                "bash".to_string()
            };
            (
                target,
                vec![
                    "--init-file".to_string(),
                    path_str(&dir.join("shellIntegration.bash")),
                ],
            )
        }
        Shell::Powershell | Shell::Pwsh => {
            let exe = if matches!(shell, Shell::Powershell) {
                "powershell"
            } else {
                "pwsh"
            };
            let target = windows_exe(exe);
            let script = path_str(&dir.join("shellIntegration.ps1"));
            (
                target,
                vec![
                    "-NoLogo".to_string(),
                    "-NoProfile".to_string(),
                    "-noexit".to_string(),
                    "-command".to_string(),
                    format!(". \"{script}\""),
                ],
            )
        }
        Shell::Fish => {
            let script = path_str(&dir.join("shellIntegration.fish"));
            (
                windows_exe("fish"),
                vec![
                    "--init-command".to_string(),
                    format!(". {}", script.replace(' ', "\\ ")),
                ],
            )
        }
        Shell::Zsh => {
            setup_zsh_dotfiles()?;
            let user_zdotdir = std::env::var("ZDOTDIR")
                .ok()
                .or_else(|| dirs::home_dir().map(|p| path_str(&p)))
                .unwrap_or_else(|| "~".to_string());
            env.push(("ZDOTDIR".to_string(), path_str(&zdotdir())));
            env.push(("USER_ZDOTDIR".to_string(), user_zdotdir));
            (windows_exe("zsh"), vec![])
        }
        Shell::Cmd => {
            env.push(("PROMPT".to_string(), "$G ".to_string()));
            (
                windows_exe("cmd"),
                vec!["/k".to_string(), "cls".to_string()],
            )
        }
        Shell::Xonsh => {
            let python = which("python").unwrap_or_else(|| "python".to_string());
            let mut args = vec!["-m".to_string(), "xonsh".to_string(), "--rc".to_string()];
            args.push(path_str(&dir.join("shellIntegration.xsh")));
            (python, args)
        }
        Shell::Elvish => (
            windows_exe("elvish"),
            vec![
                "-rc".to_string(),
                path_str(&dir.join("shellIntegration.elv")),
            ],
        ),
        Shell::Nushell => {
            let script = path_str(&dir.join("shellIntegration.nu")).replace('\\', "/");
            (
                windows_exe("nu"),
                vec!["--execute".to_string(), format!("source '{script}'")],
            )
        }
    };

    Ok(Launch { target, args, env })
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn windows_exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

pub(crate) fn which(cmd: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_string())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{cmd}{ext}"));
            if is_executable(&candidate) {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(all(test, unix))]
mod executable_tests {
    use super::is_executable;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn non_executable_path_candidate_is_rejected() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("executable-tests");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(format!("ffmpeg-{}", std::process::id()));
        std::fs::write(&path, b"not executable").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert!(!is_executable(&path));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&path));
        std::fs::remove_file(path).unwrap();
    }
}

fn git_bash_path() -> anyhow::Result<String> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(git) = which("git") {
        if let Some(bin) = Path::new(&git).parent().and_then(|p| p.parent()) {
            dirs.push(bin.to_path_buf());
        }
    }
    for var in ["ProgramW6432", "ProgramFiles", "ProgramFiles(X86)"] {
        if let Ok(v) = std::env::var(var) {
            dirs.push(PathBuf::from(v));
        }
    }
    if let Ok(local) = std::env::var("LocalAppData") {
        dirs.push(PathBuf::from(format!("{local}\\Program")));
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    for d in &dirs {
        candidates.push(d.join("Git\\bin\\bash.exe"));
        candidates.push(d.join("Git\\usr\\bin\\bash.exe"));
        candidates.push(d.join("usr\\bin\\bash.exe"));
    }
    if let Ok(profile) = std::env::var("UserProfile") {
        candidates.push(PathBuf::from(format!(
            "{profile}\\scoop\\apps\\git\\current\\bin\\bash.exe"
        )));
    }
    for c in candidates {
        if c.is_file() {
            return Ok(c.to_string_lossy().into_owned());
        }
    }
    anyhow::bail!("unable to find a git bash executable installed")
}
