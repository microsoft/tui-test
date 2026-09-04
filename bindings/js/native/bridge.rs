use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use interprocess::local_socket::traits::ListenerExt;
use interprocess::local_socket::Stream;
use tui_test::{ErrorKind, SessionMonitorTarget, TuiTestError};
use tui_test_cli::host::{self, HostDescriptor};
use tui_test_cli::protocol::{HostSession, Request, Response};
use tui_test_cli::{config, ipc, monitor};

#[derive(Clone, Default)]
pub struct Metadata {
    pub label: Option<String>,
    pub test_file: Option<String>,
    pub test_name: Option<String>,
    pub framework: Option<String>,
    pub worker: Option<String>,
}

#[derive(Clone)]
struct Entry {
    generation: u64,
    target: SessionMonitorTarget,
    metadata: Metadata,
    status: String,
    outcome: Option<String>,
    clients: u32,
    attachments: u64,
    wait_attachment_baseline: Option<u64>,
    wait_had_attachment: bool,
    started_at: u64,
}

struct State {
    sessions: HashMap<String, Entry>,
    next_generation: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            next_generation: 1,
        }
    }
}

struct Bridge {
    descriptor: HostDescriptor,
    state: Arc<(Mutex<State>, Condvar)>,
    started: Mutex<bool>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

impl Bridge {
    fn new() -> Self {
        Self {
            descriptor: host::new_descriptor().expect("create tui-test host descriptor"),
            state: Arc::new((Mutex::new(State::default()), Condvar::new())),
            started: Mutex::new(false),
        }
    }

    fn ensure_started(&self) -> Result<(), TuiTestError> {
        let mut started = self
            .started
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !*started {
            config::ensure_host_dir().map_err(bridge_error)?;
            let listener = ipc::listen(&self.descriptor.endpoint).map_err(bridge_error)?;
            let state = Arc::clone(&self.state);
            let descriptor = self.descriptor.clone();
            std::thread::spawn(move || serve(listener, state, descriptor));
            *started = true;
        }
        host::publish(&self.descriptor).map_err(bridge_error)?;
        Ok(())
    }
}

fn bridge() -> &'static Bridge {
    BRIDGE.get_or_init(Bridge::new)
}

fn bridge_error(error: impl std::fmt::Display) -> TuiTestError {
    TuiTestError::new(
        ErrorKind::Internal,
        format!("process monitor bridge failed: {error}"),
    )
}

pub fn register(
    name: &str,
    target: &SessionMonitorTarget,
    metadata: Metadata,
) -> Result<bool, TuiTestError> {
    if !target.is_current() {
        return Ok(false);
    }
    let bridge = bridge();
    bridge.ensure_started()?;
    let (state, changed) = &*bridge.state;
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(entry) = state.sessions.get_mut(name) {
        if entry.target.same_target(target) {
            entry.metadata = metadata;
            return Ok(true);
        }
    }
    if !target.is_current() {
        return Ok(false);
    }
    let generation = state.next_generation;
    state.next_generation = state.next_generation.wrapping_add(1).max(1);
    state.sessions.insert(
        name.to_string(),
        Entry {
            generation,
            target: target.clone(),
            metadata,
            status: "running".to_string(),
            outcome: None,
            clients: 0,
            attachments: 0,
            wait_attachment_baseline: None,
            wait_had_attachment: false,
            started_at: host::now_ms(),
        },
    );
    changed.notify_all();
    Ok(true)
}

pub fn unregister(name: &str, target: Option<&SessionMonitorTarget>) {
    let Some(bridge) = BRIDGE.get() else {
        return;
    };
    let (state, changed) = &*bridge.state;
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let remove = state.sessions.get(name).is_some_and(|entry| match target {
        Some(target) => entry.target.same_target(target),
        None => !entry.target.is_current(),
    });
    if remove {
        state.sessions.remove(name);
        changed.notify_all();
    }
}

pub fn invalidate_replaced(name: &str, target: &SessionMonitorTarget) {
    let Some(bridge) = BRIDGE.get() else {
        return;
    };
    let (state, changed) = &*bridge.state;
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let replaced = state
        .sessions
        .get(name)
        .is_some_and(|entry| !entry.target.same_target(target));
    if replaced {
        state.sessions.remove(name);
        changed.notify_all();
    }
}

pub fn clear_sessions() {
    let Some(bridge) = BRIDGE.get() else {
        return;
    };
    let (state, changed) = &*bridge.state;
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .clear();
    changed.notify_all();
}

pub fn begin_wait(name: &str, outcome: &str) -> Result<(String, u64), TuiTestError> {
    let bridge = bridge();
    let (state, changed) = &*bridge.state;
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = state.sessions.get_mut(name).ok_or_else(|| {
        TuiTestError::new(ErrorKind::NoSession, format!("no active session '{name}'"))
    })?;
    if !entry.target.is_current() {
        return Err(TuiTestError::new(
            ErrorKind::NoSession,
            format!("no active session '{name}'"),
        ));
    }
    entry.status = "waiting-for-attach".to_string();
    entry.outcome = Some(outcome.to_string());
    entry.wait_attachment_baseline = Some(entry.attachments);
    entry.wait_had_attachment = entry.clients > 0;
    let generation = entry.generation;
    changed.notify_all();
    Ok((format!("{}/{}", bridge.descriptor.owner, name), generation))
}

pub fn wait(
    name: &str,
    generation: u64,
    timeout: Option<Duration>,
    hold_while_attached: bool,
) -> Result<bool, TuiTestError> {
    let bridge = bridge();
    let (state, changed) = &*bridge.state;
    let mut state = state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = state
        .sessions
        .get(name)
        .filter(|entry| entry.generation == generation)
        .ok_or_else(|| {
            TuiTestError::new(ErrorKind::NoSession, format!("no active session '{name}'"))
        })?;
    if !entry.target.is_current() {
        return Err(TuiTestError::new(
            ErrorKind::NoSession,
            format!("no active session '{name}'"),
        ));
    }
    let initial_attachments = entry.wait_attachment_baseline.unwrap_or(entry.attachments);
    let attached_at_start = entry.wait_had_attachment;

    if waiting_for_attachment(
        &state,
        name,
        generation,
        initial_attachments,
        attached_at_start,
    ) {
        state = match timeout {
            Some(timeout) => {
                let (state, _) = changed
                    .wait_timeout_while(state, timeout, |state| {
                        waiting_for_attachment(
                            state,
                            name,
                            generation,
                            initial_attachments,
                            attached_at_start,
                        )
                    })
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state
            }
            None => changed
                .wait_while(state, |state| {
                    waiting_for_attachment(
                        state,
                        name,
                        generation,
                        initial_attachments,
                        attached_at_start,
                    )
                })
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        };
    }
    let attached = state
        .sessions
        .get(name)
        .filter(|entry| entry.generation == generation)
        .is_some_and(|entry| {
            attachment_observed(
                entry.clients,
                entry.attachments,
                initial_attachments,
                attached_at_start,
            )
        });
    if attached && hold_while_attached {
        state = changed
            .wait_while(state, |state| {
                state
                    .sessions
                    .get(name)
                    .filter(|entry| entry.generation == generation)
                    .is_some_and(|entry| entry.clients > 0)
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    if let Some(entry) = state
        .sessions
        .get_mut(name)
        .filter(|entry| entry.generation == generation)
    {
        entry.status = format!(
            "completed-{}",
            entry.outcome.as_deref().unwrap_or("unknown")
        );
        entry.wait_attachment_baseline = None;
        entry.wait_had_attachment = false;
    }
    Ok(attached)
}

fn waiting_for_attachment(
    state: &State,
    name: &str,
    generation: u64,
    initial_attachments: u64,
    attached_at_start: bool,
) -> bool {
    state
        .sessions
        .get(name)
        .filter(|entry| entry.generation == generation)
        .is_some_and(|entry| {
            !attachment_observed(
                entry.clients,
                entry.attachments,
                initial_attachments,
                attached_at_start,
            )
        })
}

pub fn wait_target(name: &str, generation: u64) -> Option<SessionMonitorTarget> {
    let bridge = BRIDGE.get()?;
    bridge
        .state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .get(name)
        .filter(|entry| entry.generation == generation)
        .map(|entry| entry.target.clone())
}

fn attachment_observed(
    clients: u32,
    attachments: u64,
    baseline: u64,
    attached_at_start: bool,
) -> bool {
    attached_at_start || clients > 0 || attachments > baseline
}

fn serve(
    listener: interprocess::local_socket::Listener,
    state: Arc<(Mutex<State>, Condvar)>,
    descriptor: HostDescriptor,
) {
    for connection in listener.incoming() {
        let Ok(connection) = connection else {
            continue;
        };
        let state = Arc::clone(&state);
        let descriptor = descriptor.clone();
        std::thread::spawn(move || handle(connection, state, descriptor));
    }
}

fn handle(mut connection: Stream, state: Arc<(Mutex<State>, Condvar)>, descriptor: HostDescriptor) {
    let Ok(request) = ipc::read_request(&connection) else {
        return;
    };
    match request {
        Request::Ping => {
            let _ = ipc::write_response(&mut connection, &Response::ok());
        }
        Request::HostSessions => {
            let response = Response::with(
                serde_json::to_value(host_sessions(&state, &descriptor))
                    .unwrap_or_else(|_| serde_json::json!([])),
            );
            let _ = ipc::write_response(&mut connection, &response);
        }
        Request::Routed {
            session,
            generation,
            request,
        } => match *request {
            Request::Monitor {
                cols,
                rows,
                interactive,
            } => stream_monitor(
                connection,
                state,
                session,
                generation,
                (cols, rows),
                interactive,
            ),
            Request::MonitorLeaseStream => {
                if let Some(attachment) = Attachment::new(Arc::clone(&state), &session, generation)
                {
                    if ipc::write_response(&mut connection, &Response::ok()).is_ok() {
                        hold_lease(connection, attachment);
                    }
                } else {
                    let _ = ipc::write_response(
                        &mut connection,
                        &Response::from_error(TuiTestError::new(
                            ErrorKind::NoSession,
                            format!("no monitorable session '{session}'"),
                        )),
                    );
                }
            }
            Request::MonitorInputStream => {
                if registered(&state, &session, generation) {
                    if ipc::write_response(&mut connection, &Response::ok()).is_ok() {
                        stream_input(connection, state, session, generation);
                    }
                } else {
                    let _ = ipc::write_response(
                        &mut connection,
                        &Response::from_error(TuiTestError::new(
                            ErrorKind::NoSession,
                            format!("no monitorable session '{session}'"),
                        )),
                    );
                }
            }
            _ => {
                let _ = ipc::write_response(
                    &mut connection,
                    &Response::from_error(TuiTestError::usage(
                        "process bridge currently supports monitor traffic only",
                    )),
                );
            }
        },
        _ => {
            let _ = ipc::write_response(
                &mut connection,
                &Response::from_error(TuiTestError::usage(
                    "process bridge request must be routed to a session",
                )),
            );
        }
    }
}

fn host_sessions(
    state: &Arc<(Mutex<State>, Condvar)>,
    descriptor: &HostDescriptor,
) -> Vec<HostSession> {
    let state = state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state
        .sessions
        .iter()
        .filter(|(_, entry)| entry.target.is_current())
        .map(|(name, entry)| {
            let frame = entry.target.frame();
            HostSession {
                id: format!("{}/{}", descriptor.owner, name),
                session: name.clone(),
                generation: entry.generation,
                owner: descriptor.owner.clone(),
                pid: descriptor.pid,
                label: entry.metadata.label.clone(),
                test_file: entry.metadata.test_file.clone(),
                test_name: entry.metadata.test_name.clone(),
                framework: entry.metadata.framework.clone(),
                worker: entry.metadata.worker.clone(),
                status: entry.status.clone(),
                outcome: entry.outcome.clone(),
                child_exited: frame.as_ref().is_some_and(|frame| frame.exited.is_some()),
                exit_code: frame.and_then(|frame| frame.exited),
                clients: entry.clients,
                started_at: entry.started_at,
                cwd: descriptor.cwd.clone(),
            }
        })
        .collect()
}

fn stream_monitor(
    mut connection: Stream,
    state: Arc<(Mutex<State>, Condvar)>,
    session: String,
    generation: u64,
    viewer: (u16, u16),
    interactive: bool,
) {
    let Some(target) = target(&state, &session, generation) else {
        return;
    };
    let mut modes = monitor::ModeMirror::default();
    loop {
        if !registered(&state, &session, generation) {
            break;
        }
        let frame = target.frame().map(|frame| monitor::Frame {
            grid: frame.grid,
            cursor: frame.cursor,
            size: frame.size,
            keyboard_mode: frame.keyboard_mode,
            bracketed_paste: frame.bracketed_paste,
            exited: frame.exited,
            shell: frame.shell,
        });
        let bytes =
            monitor::render_frame(frame.as_ref(), viewer, &session, interactive, &mut modes);
        if connection.write_all(&bytes).is_err() || connection.flush().is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(config::MONITOR_FRAME_MS));
    }
}

fn hold_lease(mut connection: Stream, _attachment: Attachment) {
    let mut buffer = [0; 1];
    while connection.read(&mut buffer).is_ok_and(|read| read > 0) {}
}

fn stream_input(
    mut connection: Stream,
    state: Arc<(Mutex<State>, Condvar)>,
    session: String,
    generation: u64,
) {
    let Some(target) = target(&state, &session, generation) else {
        return;
    };
    let mut buffer = [0; 16 * 1024];
    loop {
        let read = match connection.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        if !registered(&state, &session, generation) {
            break;
        }
        if target.write_monitor_input_raw(&buffer[..read]).is_err() {
            break;
        }
    }
}

fn target(
    state: &Arc<(Mutex<State>, Condvar)>,
    session: &str,
    generation: u64,
) -> Option<SessionMonitorTarget> {
    state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .get(session)
        .filter(|entry| {
            entry.generation == generation
                && !entry.status.starts_with("completed-")
                && entry.target.is_current()
        })
        .map(|entry| entry.target.clone())
}

fn registered(state: &Arc<(Mutex<State>, Condvar)>, session: &str, generation: u64) -> bool {
    state
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .sessions
        .get(session)
        .is_some_and(|entry| {
            entry.generation == generation
                && !entry.status.starts_with("completed-")
                && entry.target.is_current()
        })
}

struct Attachment {
    state: Arc<(Mutex<State>, Condvar)>,
    session: String,
    generation: u64,
}

impl Attachment {
    fn new(state: Arc<(Mutex<State>, Condvar)>, session: &str, generation: u64) -> Option<Self> {
        let (current, changed) = &*state;
        let mut current = current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = current.sessions.get_mut(session)?;
        if entry.generation != generation || !entry.target.is_current() {
            return None;
        }
        entry.clients += 1;
        entry.attachments += 1;
        entry.status = "attached".to_string();
        changed.notify_all();
        Some(Self {
            state: Arc::clone(&state),
            session: session.to_string(),
            generation,
        })
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        let (state, changed) = &*self.state;
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = state
            .sessions
            .get_mut(&self.session)
            .filter(|entry| entry.generation == self.generation)
        {
            entry.clients = entry.clients.saturating_sub(1);
            if entry.clients == 0 && entry.status == "attached" {
                entry.status = if entry.outcome.is_some() {
                    "waiting-for-attach".to_string()
                } else {
                    "running".to_string()
                };
            }
        }
        changed.notify_all();
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_present_when_wait_begins_remains_observed_after_disconnect() {
        assert!(attachment_observed(0, 1, 1, true));
    }

    #[test]
    fn attachment_between_begin_and_wait_is_observed_after_disconnect() {
        assert!(attachment_observed(0, 1, 0, false));
    }
}
