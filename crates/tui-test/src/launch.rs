//! Native child launch control: program, arguments, environment, and directory.
//!
//! A launch keeps native operating-system strings from the caller to the
//! child. Arguments never join a shell command, and environment values never
//! pass through lossy Unicode conversion.
//!
//! Resolution order for the child environment:
//!
//! 1. Select the inherited parent environment or an empty environment.
//! 2. Remove the named variables.
//! 3. Apply the mandatory tui-test terminal defaults.
//! 4. Apply the explicit caller overrides.
//!
//! The mandatory tui-test terminal default is `TERM=xterm-256color`. Caller
//! overrides win over this default.
//!
//! The child environment is resolved once, before the first spawn. Restart
//! reuses the resolved values and never reads changed parent values.
//!
//! Transport defaults from `portable-pty` 0.9.0: tui-test clears the command
//! builder before filling the resolved values, so a cleared launch does not
//! inherit the parent process environment through the transport. On Unix the
//! transport still forces `SHELL`: it keeps the resolved `SHELL` value when
//! that value names an executable file, and otherwise it uses the shell from
//! the password database, or `/bin/sh` as a last resort. A cleared Unix
//! environment therefore always contains `SHELL`. On Windows the transport
//! passes exactly the resolved values to the child process. A cleared
//! environment never promises the absence of these transport defaults.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::api::TuiTestError;

/// Mandatory tui-test terminal defaults, applied after removals and before
/// explicit caller overrides.
pub(crate) const MANDATORY_ENV_DEFAULTS: [(&str, &str); 1] = [("TERM", "xterm-256color")];

/// Whether a launch inherits the parent process environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchEnvMode {
    /// Start from the parent process environment (`std::env::vars_os`).
    #[default]
    Inherited,
    /// Start from an empty environment. Transport defaults still apply.
    Cleared,
}

/// Native launch description with private fields.
///
/// The program is a native path. Arguments, environment keys, and environment
/// values are native strings. The working directory is a native path. No
/// value passes through shell parsing or Unicode replacement.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    env_mode: LaunchEnvMode,
    removals: Vec<OsString>,
    overrides: Vec<(OsString, OsString)>,
}

impl LaunchOptions {
    /// Select the program to run. Validation runs at [`Self::resolve`].
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env_mode: LaunchEnvMode::Inherited,
            removals: Vec::new(),
            overrides: Vec::new(),
        }
    }

    /// Program to run, as a native string.
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Arguments to pass, as native strings without shell parsing.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Working directory for the child, when set.
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// Whether the launch inherits the parent process environment.
    pub fn env_mode(&self) -> LaunchEnvMode {
        self.env_mode
    }

    /// Environment variables to remove after inherited-or-empty selection.
    pub fn removed_env(&self) -> &[OsString] {
        &self.removals
    }

    /// Explicit environment overrides, applied last.
    pub fn overridden_env(&self) -> &[(OsString, OsString)] {
        &self.overrides
    }

    /// Append one argument.
    pub fn with_arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append several arguments.
    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        for arg in args {
            self.args.push(arg.into());
        }
        self
    }

    /// Set the working directory for the child.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Select the inherited or cleared environment mode.
    pub fn with_env_mode(mut self, mode: LaunchEnvMode) -> Self {
        self.env_mode = mode;
        self
    }

    /// Remove one variable after inherited-or-empty selection.
    pub fn with_remove_env(mut self, key: impl Into<OsString>) -> Self {
        self.removals.push(key.into());
        self
    }

    /// Remove several variables after inherited-or-empty selection.
    pub fn with_remove_envs(mut self, keys: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        for key in keys {
            self.removals.push(key.into());
        }
        self
    }

    /// Override one variable. Overrides win over inherited values, removals,
    /// and mandatory terminal defaults.
    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.overrides.push((key.into(), value.into()));
        self
    }

    /// Override several variables.
    pub fn with_envs(
        mut self,
        vars: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>,
    ) -> Self {
        for (key, value) in vars {
            self.overrides.push((key.into(), value.into()));
        }
        self
    }

    /// Resolve the full child environment in the specified order:
    /// inherited-or-empty, removals, mandatory terminal defaults, overrides.
    ///
    /// Native values are kept unchanged. Key comparison obeys the platform:
    /// exact bytes on Unix, case-insensitive Unicode names on Windows.
    pub fn resolve(&self) -> Result<ResolvedLaunch, TuiTestError> {
        validate_program(&self.program)?;
        for arg in &self.args {
            validate_arg(arg)?;
        }
        for key in &self.removals {
            validate_env_key(key)?;
        }
        for (key, value) in &self.overrides {
            validate_env_key(key)?;
            validate_env_value(key, value)?;
        }

        let mut env: Vec<(OsString, OsString)> = Vec::new();
        if self.env_mode == LaunchEnvMode::Inherited {
            env.extend(std::env::vars_os());
        }
        for removal in &self.removals {
            env.retain(|(key, _)| !env_keys_equal(key, removal));
        }
        for (key, value) in MANDATORY_ENV_DEFAULTS {
            upsert_env(&mut env, OsString::from(key), OsString::from(value));
        }
        for (key, value) in &self.overrides {
            upsert_env(&mut env, key.clone(), value.clone());
        }

        Ok(ResolvedLaunch {
            program: self.program.clone(),
            args: self.args.clone(),
            cwd: self.cwd.clone(),
            env,
        })
    }
}

/// A resolved launch: native program, arguments, directory, and the full
/// child environment. Restart reuses these values unchanged.
#[derive(Debug, Clone)]
pub struct ResolvedLaunch {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
}

impl ResolvedLaunch {
    /// Program to run, as a native string.
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// Arguments to pass, as native strings without shell parsing.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Working directory for the child, when set.
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// Full child environment in resolution order.
    pub fn env(&self) -> &[(OsString, OsString)] {
        &self.env
    }

    /// Record the frozen absolute working directory for this spawn. The
    /// engine resolves the launch directory once (falling back to the
    /// parent directory when unset) and stores it here, so restart reuses
    /// the same directory even after the parent moves.
    pub(crate) fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }
}

/// Compare environment keys with the platform rule: exact bytes on Unix,
/// Unicode case-insensitive names on Windows. Non-Unicode names on Windows
/// compare by exact bytes, as the transport does.
pub(crate) fn env_keys_equal(left: &OsStr, right: &OsStr) -> bool {
    #[cfg(windows)]
    {
        match (left.to_str(), right.to_str()) {
            (Some(left), Some(right)) => left.to_lowercase() == right.to_lowercase(),
            _ => left == right,
        }
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn upsert_env(env: &mut Vec<(OsString, OsString)>, key: OsString, value: OsString) {
    if let Some(slot) = env.iter_mut().find(|(slot, _)| env_keys_equal(slot, &key)) {
        slot.1 = value;
    } else {
        env.push((key, value));
    }
}

fn validate_program(program: &OsStr) -> Result<(), TuiTestError> {
    if program.is_empty() {
        return Err(TuiTestError::usage("launch program must not be empty"));
    }
    if program.as_encoded_bytes().contains(&0) {
        return Err(TuiTestError::usage("launch program must not contain NUL"));
    }
    Ok(())
}

fn validate_arg(arg: &OsStr) -> Result<(), TuiTestError> {
    if arg.as_encoded_bytes().contains(&0) {
        return Err(TuiTestError::usage("launch arguments must not contain NUL"));
    }
    Ok(())
}

fn validate_env_key(key: &OsStr) -> Result<(), TuiTestError> {
    if key.is_empty() {
        return Err(TuiTestError::usage(
            "launch environment names must not be empty",
        ));
    }
    let bytes = key.as_encoded_bytes();
    if bytes.contains(&b'=') {
        return Err(TuiTestError::usage(
            "launch environment names must not contain '='",
        ));
    }
    if bytes.contains(&0) {
        return Err(TuiTestError::usage(
            "launch environment names must not contain NUL",
        ));
    }
    Ok(())
}

fn validate_env_value(key: &OsStr, value: &OsStr) -> Result<(), TuiTestError> {
    if value.as_encoded_bytes().contains(&0) {
        return Err(TuiTestError::usage(format!(
            "launch environment value for {} must not contain NUL",
            key.to_string_lossy()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleared_resolution_contains_only_defaults_and_overrides() {
        let resolved = LaunchOptions::new("prog")
            .with_env_mode(LaunchEnvMode::Cleared)
            .with_env("ALPHA", "one")
            .resolve()
            .unwrap();
        assert_eq!(resolved.program(), OsStr::new("prog"));
        assert!(resolved.args().is_empty());
        assert!(resolved.cwd().is_none());
        let mut names: Vec<String> = resolved
            .env()
            .iter()
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["ALPHA".to_string(), "TERM".to_string()]);
    }

    #[test]
    fn overrides_win_over_mandatory_defaults() {
        let resolved = LaunchOptions::new("prog")
            .with_env_mode(LaunchEnvMode::Cleared)
            .with_env("TERM", "dumb")
            .resolve()
            .unwrap();
        let term = resolved
            .env()
            .iter()
            .find(|(key, _)| key == "TERM")
            .map(|(_, value)| value.clone());
        assert_eq!(term, Some(OsString::from("dumb")));
    }

    #[test]
    fn invalid_native_values_are_usage_errors() {
        use crate::api::ErrorKind;
        let nul = OsString::from("A\0");
        assert_eq!(
            LaunchOptions::new("prog")
                .with_env("BAD", nul.clone())
                .resolve()
                .unwrap_err()
                .kind,
            ErrorKind::Usage
        );
        assert_eq!(
            LaunchOptions::new("prog")
                .with_env("A=B", "value")
                .resolve()
                .unwrap_err()
                .kind,
            ErrorKind::Usage
        );
        assert_eq!(
            LaunchOptions::new("prog")
                .with_arg(nul)
                .resolve()
                .unwrap_err()
                .kind,
            ErrorKind::Usage
        );
    }
}
