use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};

use super::*;
use crate::terminal::integration::CommandTracker;

#[test]
fn fish_source_uses_a_single_literal_argument() {
    assert_eq!(
        fish_literal("C:\\a $HOME `echo` '雪' \"quoted\""),
        "'C:\\\\a $HOME `echo` \\'雪\\' \"quoted\"'"
    );
}

#[test]
fn nushell_source_uses_a_non_interpolating_literal() {
    assert_eq!(
        nu_literal("C:\\a $HOME `echo` '雪' \"quoted\"\n\r\t"),
        "\"C:\\\\a $HOME `echo` '雪' \\\"quoted\\\"\\n\\r\\t\""
    );
}

#[test]
fn powershell_passes_the_script_as_a_file_not_as_code() {
    let dir = Path::new("space $HOME `echo` '雪'");
    for shell in [Shell::Powershell, Shell::Pwsh] {
        let launch = launch_with_scripts(shell, dir).unwrap();
        assert_eq!(
            launch.args,
            [
                "-NoLogo",
                "-NoProfile",
                "-NoExit",
                "-File",
                &path_str(&dir.join("shellIntegration.ps1")),
            ]
        );
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(shell: Shell) -> Self {
        let mut name = "space $HOME `echo` '雪' %2F %25 #".to_string();
        if cfg!(unix) {
            name.push_str(" \"quoted\" \\ newline\n");
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("shell-tests")
            .join(format!("{}-{}", shell.as_str(), std::process::id()))
            .join(name);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        for _ in 0..20 {
            match std::fs::remove_dir_all(self.0.parent().unwrap()) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

struct TestShell {
    child: Box<dyn Child + Send + Sync>,
    _master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    output: Receiver<Vec<u8>>,
    tracker: CommandTracker,
    bytes: Vec<u8>,
    shell: Shell,
}

impl TestShell {
    fn start(shell: Shell, directory: &Path, changed: &Path) -> Self {
        let scripts = directory.join("shell");
        write_scripts_to(&scripts).unwrap();
        if matches!(shell, Shell::Powershell | Shell::Pwsh) {
            std::fs::OpenOptions::new()
                .append(true)
                .open(scripts.join("shellIntegration.ps1"))
                .unwrap()
                .write_all(b"\nif (Get-Command Set-PSReadLineOption -ErrorAction SilentlyContinue) {\nSet-PSReadLineOption -HistorySavePath (Join-Path $env:__SU_TEST_HOME 'history.txt')\n}\n")
                .unwrap();
        }
        let launch = launch_with_scripts(shell, &scripts).unwrap();
        let pair = portable_pty::native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(&launch.target);
        command.args(&launch.args);
        if shell == Shell::Nushell {
            command.args(["--no-config-file", "--no-history"]);
        }
        if shell == Shell::Elvish {
            command.arg("-db");
            command.arg(directory.join("elvish.db"));
        }
        command.cwd(directory);
        for (key, value) in launch.env {
            command.env(key, value);
        }
        // Keep shell history, caches and user startup files inside this fixture.
        for key in [
            "HOME",
            "USERPROFILE",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
        ] {
            command.env(key, directory);
        }
        command.env("USER_ZDOTDIR", directory);
        command.env("XONSH_DATA_DIR", directory);
        command.env("XONSH_CACHE_SCRIPTS", "false");
        command.env("TERM", "xterm-256color");
        command.env("__SU_TEST_HOME", directory);
        command.env("__SU_TEST_CWD", normalized_path(changed));
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (tx, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut result = Self {
            child,
            _master: pair.master,
            writer,
            output,
            tracker: CommandTracker::new(),
            bytes: Vec::new(),
            shell,
        };
        result.wait_until(CommandTracker::is_ready);
        result
    }

    fn wait_until(&mut self, condition: impl Fn(&CommandTracker) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !condition(&self.tracker) {
            let chunk = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| {
                    panic!(
                        "{} did not report shell integration: {error}\n{:?}",
                        self.shell.as_str(),
                        String::from_utf8_lossy(&self.bytes)
                    )
                });
            self.tracker.feed(&chunk);
            let query_start = self.bytes.len().saturating_sub(3);
            self.bytes.extend(chunk);
            // ConPTY waits for a cursor-position response before starting its child.
            for query in self.bytes[query_start..].windows(4) {
                if query == b"\x1b[6n" {
                    self.writer.write_all(b"\x1b[1;1R").unwrap();
                    self.writer.flush().unwrap();
                }
            }
        }
    }

    fn run(&mut self, command: &str, expected_exit: i32) {
        let before = self.tracker.finished_count();
        self.writer.write_all(command.as_bytes()).unwrap();
        self.writer
            .write_all(self.shell.return_char().as_bytes())
            .unwrap();
        self.writer.flush().unwrap();
        self.wait_until(|tracker| tracker.finished_count() > before && tracker.is_ready());
        assert_eq!(
            self.tracker.last_exit(),
            Some(expected_exit),
            "{}: {command}\n{:?}",
            self.shell.as_str(),
            String::from_utf8_lossy(&self.bytes),
        );
    }
}

impl Drop for TestShell {
    fn drop(&mut self) {
        let _ = self.writer.write_all(b"exit");
        let _ = self.writer.write_all(self.shell.return_char().as_bytes());
        let _ = self.writer.flush();
        for _ in 0..40 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn available(shell: Shell) -> bool {
    match shell {
        Shell::Bash if cfg!(windows) => git_bash_path().is_ok(),
        Shell::Xonsh => which("python").is_some_and(|python| {
            Command::new(python)
                .args(["-c", "import xonsh"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        }),
        Shell::Nushell => which("nu").is_some(),
        _ => which(shell.as_str()).is_some(),
    }
}

fn check_integration(shell: Shell) {
    if !available(shell) {
        eprintln!("skipping {}: shell is not installed", shell.as_str());
        return;
    }
    let directory = TestDirectory::new(shell);
    let changed = directory.0.join("changed %2F %25 é");
    std::fs::create_dir(&changed).unwrap();
    let mut session = TestShell::start(shell, &directory.0, &changed);
    let cd = match shell {
        Shell::Powershell | Shell::Pwsh => "Set-Location -LiteralPath $env:__SU_TEST_CWD",
        Shell::Elvish => "cd $E:__SU_TEST_CWD",
        Shell::Nushell => "cd $env.__SU_TEST_CWD",
        Shell::Xonsh => "cd $__SU_TEST_CWD",
        _ => "cd -- \"$__SU_TEST_CWD\"",
    };
    session.run(cd, 0);
    let mut expected = normalized_path(&changed);
    if cfg!(windows) && shell == Shell::Bash {
        expected = format!("/{}{}", expected[..1].to_lowercase(), &expected[2..]);
    }
    assert_eq!(session.tracker.cwd(), Some(expected.as_str()));
    let output = String::from_utf8_lossy(&session.bytes);
    assert!(output.contains("%252F"), "literal percent was not encoded");
    assert!(output.contains("%2525"), "literal percent was not encoded");
    assert!(output.contains("%C3%A9"), "UTF-8 was not percent-encoded");

    if matches!(shell, Shell::Powershell | Shell::Pwsh) {
        session.run("Set-StrictMode -Version Latest", 0);
        session.run("Write-Output '__su_initial_strict_success'", 0);
        let native_failure = if cfg!(windows) {
            "cmd /c exit 7"
        } else {
            "sh -c 'exit 7'"
        };
        session.run(native_failure, 7);
        session.run(
            "Get-Item -LiteralPath './__su_missing_item' -ErrorAction Continue",
            1,
        );
        session.run(native_failure, 7);
        session.run(
            "Get-Item -LiteralPath './__su_missing_item' -ErrorAction Ignore",
            1,
        );
        session.run(native_failure, 7);
        session.run(
            "Get-Item -LiteralPath './__su_missing_item' -ErrorAction Stop",
            1,
        );
        session.run(native_failure, 7);
        session.run("throw '__su_expected_failure'", 1);
        session.run(native_failure, 7);
        session.run(native_failure, 7);
        session.run("Write-Output '__su_success'", 0);
        session.run(
            "if ($LASTEXITCODE -ne 7) { throw 'native exit code was overwritten' }",
            0,
        );
        session.run("$Error.Clear(); Set-StrictMode -Version Latest", 0);
        session.run("Write-Output '__su_strict_success'", 0);
        if shell == Shell::Pwsh {
            session.run("$PSNativeCommandUseErrorActionPreference = $true", 0);
            session.run(native_failure, 7);
            session.run("Write-Error '__su_ignored_failure' -ErrorAction Ignore", 1);
        }
    }
    if shell == Shell::Xonsh {
        assert!(!session.bytes.contains(&1), "SOH must not be rendered");
        assert!(!session.bytes.contains(&2), "STX must not be rendered");
        session.run("print('__su_xonsh_ready')", 0);
    }
}

fn normalized_path(path: &Path) -> String {
    if cfg!(windows) {
        path_str(path).replace('\\', "/")
    } else {
        path_str(path)
    }
}

macro_rules! integration_test {
    ($name:ident, $shell:ident) => {
        #[test]
        fn $name() {
            check_integration(Shell::$shell);
        }
    };
}

integration_test!(bash_integration, Bash);
integration_test!(zsh_integration, Zsh);
integration_test!(fish_integration, Fish);
integration_test!(powershell_integration, Powershell);
integration_test!(pwsh_integration, Pwsh);
integration_test!(xonsh_integration, Xonsh);
integration_test!(elvish_integration, Elvish);
integration_test!(nushell_integration, Nushell);
