use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use crate::cli::SessionFilter;
use crate::{config, host, ipc};
use tui_test::monitoring::{protocol::Request, SessionId};
use tui_test::{ErrorKind, TuiTestError};

#[derive(Clone)]
pub(crate) enum Candidate {
    Daemon { name: String, id: SessionId },
    Process(Box<host::DiscoveredHostSession>),
}

impl Candidate {
    pub(crate) fn id(&self) -> SessionId {
        match self {
            Self::Daemon { id, .. } => *id,
            Self::Process(candidate) => candidate.session.id,
        }
    }

    fn session(&self) -> &str {
        match self {
            Self::Daemon { name, .. } => name,
            Self::Process(candidate) => &candidate.session.session,
        }
    }

    fn failed(&self) -> bool {
        matches!(self, Self::Process(candidate) if candidate.session.outcome.as_deref() == Some("failed"))
    }

    fn waiting(&self) -> bool {
        matches!(self, Self::Process(candidate) if candidate.session.status == "waiting-for-attach")
    }

    fn timestamp(&self) -> u64 {
        match self {
            Self::Daemon { .. } => 0,
            Self::Process(candidate) => candidate
                .session
                .completed_at
                .unwrap_or(candidate.session.started_at),
        }
    }

    fn in_directory(&self, directory: &Path) -> bool {
        matches!(self, Self::Process(candidate) if candidate.session.cwd.as_deref().is_some_and(|cwd| same_directory(Path::new(cwd), directory)))
    }

    fn rank(&self, cwd: &Path) -> u8 {
        if self.waiting() && self.failed() {
            0
        } else if self.waiting() {
            1
        } else if matches!(self, Self::Process(candidate) if candidate.session.status == "running")
            && self.in_directory(cwd)
        {
            2
        } else {
            3
        }
    }

    fn description(&self, cwd: &Path) -> String {
        match self {
            Self::Daemon { name, .. } => format!("live  {name}"),
            Self::Process(candidate) => {
                let info = &candidate.session;
                let label = info
                    .label
                    .as_deref()
                    .or(info.test_name.as_deref())
                    .unwrap_or(&info.session);
                let file = info
                    .test_file
                    .as_deref()
                    .map(Path::new)
                    .map(|path| path.strip_prefix(cwd).unwrap_or(path).display().to_string())
                    .unwrap_or_else(|| "-".into());
                let age = host::now_ms().saturating_sub(self.timestamp()) / 1000;
                let text = format!(
                    "{}{}  {}  {}  worker={}  {}s  {}  {}",
                    info.status,
                    if info.interactive_clients > 0 {
                        " [interactive]"
                    } else {
                        ""
                    },
                    label,
                    file,
                    info.worker.as_deref().unwrap_or("-"),
                    age,
                    info.session,
                    info.tags.join(" "),
                );
                text.chars()
                    .map(|ch| if ch.is_control() { ' ' } else { ch })
                    .collect()
            }
        }
    }

    fn detail(&self) -> serde_json::Result<serde_json::Value> {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ProcessDetail<'a> {
            #[serde(flatten)]
            session: &'a tui_test::monitoring::protocol::HostSession,
            attach: String,
        }
        match self {
            Self::Daemon { name, id } => Ok(serde_json::json!({
                "id": id, "session": name, "status": "live",
                "attach": host::monitor_command_by_id(&self.id(), true),
            })),
            Self::Process(candidate) => serde_json::to_value(ProcessDetail {
                session: &candidate.session,
                attach: host::monitor_command_by_id(&self.id(), true),
            }),
        }
    }
}

fn same_directory(a: &Path, b: &Path) -> bool {
    let a = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

pub(crate) fn running_daemons() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(config::home_dir()) else {
        return Vec::new();
    };
    let mut names = entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let name = file.strip_suffix(".pid")?;
            ipc::is_running(&config::socket_name(name)).then(|| name.to_string())
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn filter_candidates(
    mut candidates: Vec<Candidate>,
    filter: &SessionFilter,
    cwd: &Path,
) -> Vec<Candidate> {
    let directory = filter.cwd.as_ref().map(|value| {
        if value == "current" {
            cwd.to_path_buf()
        } else {
            cwd.join(value)
        }
    });
    candidates.retain(|candidate| {
        (!filter.failed || candidate.failed())
            && (!filter.waiting || candidate.waiting())
            && directory
                .as_deref()
                .is_none_or(|path| candidate.in_directory(path))
    });
    candidates.sort_by_key(|candidate| {
        (
            candidate.rank(cwd),
            Reverse(candidate.timestamp()),
            candidate.id(),
        )
    });
    candidates
}

fn discover(filter: &SessionFilter, cwd: &Path) -> Vec<Candidate> {
    let candidates = running_daemons()
        .into_iter()
        .filter_map(|name| {
            let connection = ipc::connect(&config::socket_name(&name)).ok()?;
            let response =
                ipc::exchange_timeout(connection, &Request::Ping, Duration::from_secs(1)).ok()?;
            let id = serde_json::from_value(response.data?).ok()?;
            Some(Candidate::Daemon { name, id })
        })
        .chain(
            host::discover()
                .into_iter()
                .map(|candidate| Candidate::Process(Box::new(candidate))),
        )
        .collect();
    filter_candidates(candidates, filter, cwd)
}

pub(crate) fn select(
    session: Option<&str>,
    id: Option<SessionId>,
    latest: bool,
    filter: &SessionFilter,
    interactive: bool,
    picker: bool,
) -> Result<Option<Candidate>, TuiTestError> {
    let cwd = std::env::current_dir().map_err(|error| TuiTestError::internal(error.to_string()))?;
    let candidates = discover(filter, &cwd)
        .into_iter()
        .filter(|candidate| {
            (id.is_some() || session.is_none_or(|name| candidate.session() == name))
                && id.is_none_or(|id| candidate.id() == id)
        })
        .collect::<Vec<_>>();
    choose(
        candidates,
        session.is_some() || id.is_some(),
        latest,
        interactive,
        picker,
        &cwd,
    )
}

fn choose(
    mut candidates: Vec<Candidate>,
    explicit: bool,
    latest: bool,
    interactive: bool,
    picker: bool,
    cwd: &Path,
) -> Result<Option<Candidate>, TuiTestError> {
    if candidates.is_empty() {
        return Err(TuiTestError::new(
            ErrorKind::NoSession,
            "no matching active session; enable process-local monitoring or run `tui-test open`",
        ));
    }
    if latest && (!explicit || candidates.len() == 1) {
        candidates.sort_by_key(|candidate| (Reverse(candidate.timestamp()), candidate.id()));
        return Ok(candidates.into_iter().next());
    }
    if explicit && candidates.len() == 1 {
        return Ok(candidates.into_iter().next());
    }
    if !explicit && picker {
        let labels = candidates
            .iter()
            .map(|candidate| candidate.description(cwd))
            .collect::<Vec<_>>();
        let selected = dialoguer::FuzzySelect::new()
            .with_prompt("Select a terminal session")
            .items(&labels)
            .interact_opt()
            .map_err(|error| TuiTestError::internal(error.to_string()))?;
        return Ok(selected.map(|index| candidates.swap_remove(index)));
    }
    if candidates.len() == 1 {
        return Ok(candidates.into_iter().next());
    }
    let choices = candidates
        .iter()
        .map(|candidate| {
            format!(
                "  {}  # {}",
                host::monitor_command_by_id(&candidate.id(), interactive),
                candidate.description(cwd)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Err(TuiTestError::usage(format!(
        "session selection is ambiguous; use an exact --id:\n{choices}"
    )))
}

pub(crate) fn print_sessions(filter: &SessionFilter, json: bool) -> i32 {
    let result = (|| -> anyhow::Result<()> {
        let cwd = std::env::current_dir()?;
        let candidates = discover(filter, &cwd);
        if json {
            let sessions = candidates
                .iter()
                .map(Candidate::session)
                .collect::<Vec<_>>();
            let details = candidates
                .iter()
                .map(Candidate::detail)
                .collect::<Result<Vec<_>, _>>()?;
            println!(
                "{}",
                serde_json::json!({ "sessions": sessions, "details": details })
            );
        } else if candidates.is_empty() {
            println!("no active sessions");
        } else {
            let mut counts = HashMap::new();
            for candidate in &candidates {
                *counts.entry(candidate.session()).or_insert(0) += 1;
            }
            for candidate in &candidates {
                let disambiguation = if counts[candidate.session()] > 1 {
                    format!("  {}", candidate.id())
                } else {
                    String::new()
                };
                println!("{}{disambiguation}", candidate.description(&cwd));
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("session discovery failed: {error}");
            5
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(name: &str, status: &str, failed: bool, timestamp: u64) -> Candidate {
        let descriptor = host::new_descriptor();
        let session = tui_test::monitoring::protocol::HostSession {
            id: SessionId::new_v4(),
            session: name.into(),
            pid: descriptor.pid,
            label: Some(format!("scenario {name}")),
            test_file: Some(
                Path::new("tests")
                    .join("terminal.rs")
                    .to_string_lossy()
                    .into_owned(),
            ),
            test_name: Some(name.into()),
            framework: Some("test".into()),
            worker: Some("1".into()),
            tags: vec!["inspection".into()],
            status: status.into(),
            outcome: failed.then(|| "failed".into()),
            child_exited: false,
            exit_code: None,
            clients: 0,
            interactive_clients: 0,
            started_at: timestamp,
            completed_at: None,
            cwd: descriptor.cwd.clone(),
        };
        Candidate::Process(Box::new(host::DiscoveredHostSession {
            descriptor,
            session,
        }))
    }

    #[test]
    fn monitor_discovery_prioritizes_failures_and_filters_metadata() {
        let cwd = std::env::current_dir().unwrap();
        let failed = process("failed", "waiting-for-attach", true, 1);
        let waiting = process("waiting", "waiting-for-attach", false, 10);
        let running = process("running", "running", false, 20);
        let all = vec![running, waiting, failed];
        let ranked = filter_candidates(all.clone(), &SessionFilter::default(), &cwd);
        assert_eq!(
            ranked.iter().map(Candidate::session).collect::<Vec<_>>(),
            ["failed", "waiting", "running"]
        );
        let filtered = filter_candidates(
            all,
            &SessionFilter {
                failed: true,
                waiting: true,
                cwd: Some("current".into()),
            },
            &cwd,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].session(), "failed");
        assert!(filtered[0].description(&cwd).contains("inspection"));
    }

    #[test]
    fn monitor_duplicate_names_require_an_exact_id_even_with_a_daemon() {
        let cwd = std::env::current_dir().unwrap();
        let hosted = process("login", "running", false, 1);
        let id = hosted.id();
        let daemon_id = SessionId::new_v4();
        let error = choose(
            vec![
                Candidate::Daemon {
                    name: "login".into(),
                    id: daemon_id,
                },
                hosted,
            ],
            true,
            false,
            true,
            true,
            &cwd,
        )
        .err()
        .expect("ambiguous session");
        assert_eq!(error.kind, ErrorKind::Usage);
        assert!(error.message.contains(&id.to_string()));
        assert!(error.message.contains(&daemon_id.to_string()));
    }

    #[test]
    fn monitor_ids_are_uuids_without_owner_metadata() {
        let candidate = process("login", "running", false, 1);
        let detail = candidate.detail().unwrap();
        assert_eq!(detail["session"], "login");
        assert_eq!(detail["id"], candidate.id().to_string());
        for removed in ["owner", "ownerType", "generation"] {
            assert!(detail.get(removed).is_none());
        }
        let cwd = std::env::current_dir().unwrap();
        let duplicate = process("login", "running", false, 2);
        assert!(choose(vec![candidate, duplicate], true, true, false, false, &cwd).is_err());
    }

    #[test]
    fn monitor_latest_selects_by_time_not_picker_priority() {
        let cwd = std::env::current_dir().unwrap();
        let selected = choose(
            vec![
                process("old-failure", "waiting-for-attach", true, 1),
                process("recent", "running", false, 100),
            ],
            false,
            true,
            true,
            false,
            &cwd,
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.session(), "recent");
    }
}
