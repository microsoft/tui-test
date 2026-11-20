use anyhow::{Context, Result};
use serde_json::{json, Value};

#[cfg(windows)]
mod windows;

const PORTABLE_PTY_VERSION: &str = env!("PORTABLE_PTY_VERSION");

pub fn render(json_output: bool) -> Result<String> {
    let xterm: Value =
        serde_json::from_str(include_str!("../../tui-test/assets/xtermjs/pinned.json"))
            .context("parse vendored xterm.js versions")?;
    let ghostty = ghostty_vt::build_info::version_string().context("read Ghostty build version")?;
    let ghostty = std::str::from_utf8(ghostty).context("decode Ghostty build version")?;
    let mut report = json!({
        "tui_test": env!("CARGO_PKG_VERSION"),
        "terminal_emulators": {
            "alacritty": { "version": env!("ALACRITTY_TERMINAL_VERSION"), "package": "alacritty_terminal" },
            "ghostty": { "version": ghostty, "package": "libghostty-vt", "package_version": env!("LIBGHOSTTY_VT_VERSION") },
            "rio": { "version": env!("RIO_VT_VERSION"), "package": "rio-vt" },
            "xtermjs": { "version": xterm["@xterm/headless"], "package": "@xterm/headless", "unicode11_version": xterm["@xterm/addon-unicode11"] },
        },
        "portable_pty": PORTABLE_PTY_VERSION,
    });
    add_conpty(&mut report)?;

    if json_output {
        return serde_json::to_string_pretty(&report).context("serialize versions");
    }
    let mut lines = vec![format!("tui-test {}", env!("CARGO_PKG_VERSION"))];
    for (name, emulator) in report["terminal_emulators"].as_object().unwrap() {
        let version = emulator["version"]
            .as_str()
            .with_context(|| format!("missing {name} version"))?;
        let package = emulator["package"].as_str().unwrap();
        let details = if let Some(binding) = emulator["package_version"].as_str() {
            format!("{package} {binding}")
        } else if let Some(addon) = emulator["unicode11_version"].as_str() {
            format!("{package}; @xterm/addon-unicode11 {addon}")
        } else {
            package.to_string()
        };
        lines.push(format!("{name}: {version} ({details})"));
    }
    lines.push(format!("portable-pty: {PORTABLE_PTY_VERSION}"));
    if let Some(conpty) = report.get("conpty") {
        lines.push(format!(
            "conpty: {} ({}; file {})",
            conpty["version"].as_str().unwrap(),
            conpty["source"].as_str().unwrap(),
            conpty["file_version"].as_str().unwrap()
        ));
    }
    Ok(lines.join("\n"))
}

fn add_conpty(report: &mut Value) -> Result<()> {
    #[cfg(windows)]
    {
        let package = conpty_mapping(PORTABLE_PTY_VERSION).with_context(|| {
            format!(
                "portable-pty {PORTABLE_PTY_VERSION} has no ConPTY mapping; \
                 review its Windows loader and update versions::conpty_mapping"
            )
        })?;
        anyhow::ensure!(
            package == tui_test::terminal::conpty::package_name(),
            "portable-pty ConPTY mapping does not match the bundled package"
        );
        report["conpty"] = windows::report(package);
    }
    #[cfg(not(windows))]
    let _ = report;
    Ok(())
}

#[cfg(any(windows, test))]
fn conpty_mapping(portable_pty_version: &str) -> Option<&'static str> {
    // This loader resolves the preloaded conpty.dll by basename.
    match portable_pty_version {
        "0.9.0" => Some("Microsoft.Windows.Console.ConPTY"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghostty_version_is_text_in_json_and_plain_output() {
        let report: Value = serde_json::from_str(&render(true).unwrap()).unwrap();
        let version = report["terminal_emulators"]["ghostty"]["version"]
            .as_str()
            .unwrap();
        assert!(!version.is_empty());
        assert!(render(false)
            .unwrap()
            .contains(&format!("ghostty: {version} (")));
    }

    #[test]
    fn resolved_portable_pty_version_has_a_conpty_mapping() {
        assert!(
            conpty_mapping(PORTABLE_PTY_VERSION).is_some(),
            "portable-pty {PORTABLE_PTY_VERSION} has no ConPTY mapping; \
             review its Windows loader and update versions::conpty_mapping"
        );
    }

    #[test]
    fn unknown_portable_pty_versions_do_not_silently_reuse_a_mapping() {
        assert!(conpty_mapping("unmapped").is_none());
    }
}
