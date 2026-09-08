use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::{ListenerExt, Stream as _};

use super::host::{self, HostDescriptor};
use super::input::MouseRemapper;
use super::ipc::{self, Stream};
use super::lifecycle::{Hold, Lifecycle, Outcome};
use super::protocol::{
    HostSession, HostSnapshot, MonitorInput, MonitorInputReady, MonitorLeaseReady, Request,
    Response,
};
use super::render;
use crate::{SessionMonitorTarget, TuiTestError};

/// Optional discovery labels. Metadata never changes ownership of the child.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Metadata {
    pub label: Option<String>,
    pub test_file: Option<String>,
    pub test_name: Option<String>,
    pub framework: Option<String>,
    pub worker: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Clone)]
struct Lease {
    interactive: bool,
    input_open: bool,
    active: Arc<Mutex<bool>>,
}

#[derive(Clone)]
struct Entry {
    generation: u64,
    target: SessionMonitorTarget,
    metadata: Metadata,
    lifecycle: Lifecycle,
    outcome: Option<Outcome>,
    leases: HashMap<u64, Lease>,
    attachments: u64,
    started_at: u64,
    completed_at: Option<u64>,
}

#[derive(Default)]
struct State {
    sessions: HashMap<String, Entry>,
    clients: usize,
    stopped: bool,
}

struct Bridge {
    descriptor: HostDescriptor,
    state: Mutex<State>,
    changed: Condvar,
}

static BRIDGE: OnceLock<Mutex<Option<Arc<Bridge>>>> = OnceLock::new();
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn current_bridge() -> Option<Arc<Bridge>> {
    BRIDGE.get().and_then(|bridge| lock(bridge).clone())
}

fn bridge_error(error: impl std::fmt::Display) -> TuiTestError {
    TuiTestError::internal(format!("process monitor bridge failed: {error}"))
}

impl Bridge {
    fn start() -> Result<Arc<Self>, TuiTestError> {
        let descriptor = host::new_descriptor().map_err(bridge_error)?;
        host::ensure_host_dir().map_err(bridge_error)?;
        let listener = ipc::listen(&descriptor.endpoint).map_err(bridge_error)?;
        host::publish(&descriptor).map_err(bridge_error)?;
        let bridge = Arc::new(Self {
            descriptor,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });
        let server = bridge.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("tui-test-monitor".into())
            .spawn(move || serve(listener, server))
        {
            host::unpublish(&bridge.descriptor);
            return Err(bridge_error(error));
        }
        Ok(bridge)
    }

    fn stop_if_idle(self: &Arc<Self>, force: bool) {
        let Some(manager) = BRIDGE.get() else { return };
        let mut manager = lock(manager);
        if !manager
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, self))
        {
            return;
        }
        let removed = {
            let mut state = lock(&self.state);
            if !force && (!state.sessions.is_empty() || state.clients != 0) {
                return;
            }
            state.stopped = true;
            if force {
                Some(std::mem::take(&mut state.sessions))
            } else {
                None
            }
        };
        self.changed.notify_all();
        *manager = None;
        drop(manager);
        // Accept is deliberately blocking. A local wake connection ends it without polling.
        let _ = ipc::connect(&self.descriptor.endpoint);
        host::unpublish(&self.descriptor);
        drop(removed);
    }

    fn wait_changed<'a>(
        &self,
        state: MutexGuard<'a, State>,
        deadline: Option<Instant>,
    ) -> MutexGuard<'a, State> {
        match deadline {
            Some(deadline) => {
                self.changed
                    .wait_timeout(state, deadline.saturating_duration_since(Instant::now()))
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .0
            }
            None => self
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        }
    }
}

/// Register an existing child. The first registration lazily starts one bridge per process.
pub fn register(
    name: &str,
    target: &SessionMonitorTarget,
    metadata: Metadata,
) -> Result<bool, TuiTestError> {
    invalidate_replaced(name, target);
    target.register(|| {
        let manager = BRIDGE.get_or_init(|| Mutex::new(None));
        let mut manager = lock(manager);
        let bridge = match manager.as_ref() {
            Some(bridge) => bridge.clone(),
            None => {
                let bridge = Bridge::start()?;
                *manager = Some(bridge.clone());
                bridge
            }
        };
        let mut state = lock(&bridge.state);
        if let Some(entry) = state.sessions.get_mut(name) {
            if entry.target.same_target(target) {
                if !entry.lifecycle.accepts_attachments() {
                    return Ok(false);
                }
                entry.metadata = metadata;
                return Ok(true);
            }
            return Err(TuiTestError::usage(format!(
                "another live terminal is already monitored as '{name}'"
            )));
        }
        let replaced = state.sessions.insert(
            name.into(),
            Entry {
                generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
                target: target.clone(),
                metadata,
                lifecycle: Lifecycle::Running,
                outcome: None,
                leases: HashMap::new(),
                attachments: 0,
                started_at: host::now_ms(),
                completed_at: None,
            },
        );
        bridge.changed.notify_all();
        drop(state);
        drop(manager);
        drop(replaced);
        Ok(true)
    })
}

pub fn unregister(name: &str, target: Option<&SessionMonitorTarget>) {
    let Some(bridge) = current_bridge() else {
        return;
    };
    let candidate = lock(&bridge.state)
        .sessions
        .get(name)
        .map(|entry| entry.target.clone());
    let Some(candidate) = candidate else { return };
    let remove = target.map_or_else(
        || !candidate.is_current(),
        |target| candidate.same_target(target),
    );
    if remove {
        // Unregistration is not a normal-close escape hatch for an active hold or lease.
        let mut state = lock(&bridge.state);
        if state.sessions.get(name).is_some_and(|entry| {
            entry.target.same_target(&candidate)
                && !matches!(entry.lifecycle, Lifecycle::Holding(_))
                && entry.leases.is_empty()
        }) {
            state.sessions.remove(name);
        }
        bridge.changed.notify_all();
        drop(state);
        bridge.stop_if_idle(false);
    }
}

pub fn invalidate_replaced(name: &str, target: &SessionMonitorTarget) {
    let Some(bridge) = current_bridge() else {
        return;
    };
    let old = lock(&bridge.state)
        .sessions
        .get(name)
        .map(|entry| entry.target.clone());
    let Some(old) = old.filter(|old| !old.same_target(target) && !old.is_current()) else {
        return;
    };
    let mut state = lock(&bridge.state);
    let removed = if state
        .sessions
        .get(name)
        .is_some_and(|entry| entry.target.same_target(&old))
    {
        state.sessions.remove(name)
    } else {
        None
    };
    bridge.changed.notify_all();
    drop(state);
    drop(removed);
    bridge.stop_if_idle(false);
}

/// Force-cancel monitoring for interpreter/process exit. Normal cleanup must use session close.
pub fn clear_sessions() {
    let Some(bridge) = current_bridge() else {
        return;
    };
    bridge.stop_if_idle(true);
}

/// Mark completion before starting asynchronous cleanup; defaults to a 30-second first attachment.
pub fn begin_wait(name: &str, outcome: &str) -> Result<(String, u64), TuiTestError> {
    begin_wait_with_options(name, outcome, Some(Duration::from_secs(30)), true)
}

/// Atomically publish completion and its hold policy so concurrent native close cannot bypass it.
pub fn begin_wait_with_options(
    name: &str,
    outcome: &str,
    timeout: Option<Duration>,
    hold_while_attached: bool,
) -> Result<(String, u64), TuiTestError> {
    begin_wait_for(name, None, outcome, timeout, hold_while_attached)
}

/// Mark inspection only for the caller's captured target, never a replacement.
pub fn begin_wait_for_target_with_options(
    name: &str,
    target: &SessionMonitorTarget,
    outcome: &str,
    timeout: Option<Duration>,
    hold_while_attached: bool,
) -> Result<(String, u64), TuiTestError> {
    begin_wait_for(name, Some(target), outcome, timeout, hold_while_attached)
}

/// Atomically mark inspection, optionally requiring the caller's captured target.
/// Owned clients should pass `Some(target)`; `None` deliberately selects by name.
pub fn begin_wait_for(
    name: &str,
    expected: Option<&SessionMonitorTarget>,
    outcome: &str,
    timeout: Option<Duration>,
    hold_while_attached: bool,
) -> Result<(String, u64), TuiTestError> {
    let outcome = Outcome::parse(outcome)?;
    let bridge = current_bridge().ok_or_else(TuiTestError::no_session)?;
    let mut state = lock(&bridge.state);
    let entry = state
        .sessions
        .get_mut(name)
        .filter(|entry| expected.is_none_or(|target| entry.target.same_target(target)))
        .ok_or_else(TuiTestError::no_session)?;
    if matches!(entry.lifecycle, Lifecycle::Closing { .. }) {
        return Err(TuiTestError::usage(
            "monitoring session is already completing or closing",
        ));
    }
    if matches!(entry.lifecycle, Lifecycle::Running) {
        entry.lifecycle = Lifecycle::Holding(Hold::new(
            entry.attachments,
            entry.leases.len(),
            timeout,
            hold_while_attached,
        )?);
        entry.outcome = Some(outcome);
        entry.completed_at = Some(host::now_ms());
    } else if matches!(entry.lifecycle, Lifecycle::Completed { .. })
        && entry.outcome != Some(Outcome::Failed)
    {
        entry.outcome = Some(outcome);
    }
    let generation = entry.generation;
    bridge.changed.notify_all();
    Ok((format!("{}/{}", bridge.descriptor.owner, name), generation))
}

pub fn wait(
    name: &str,
    generation: u64,
    timeout: Option<Duration>,
    hold_while_attached: bool,
) -> Result<bool, TuiTestError> {
    let bridge = current_bridge().ok_or_else(TuiTestError::no_session)?;
    let mut state = lock(&bridge.state);
    if let Some(entry) = state
        .sessions
        .get_mut(name)
        .filter(|entry| entry.generation == generation)
    {
        if let Lifecycle::Holding(hold) = &mut entry.lifecycle {
            hold.configure(timeout, hold_while_attached)?;
        }
    } else {
        return Err(TuiTestError::no_session());
    }
    loop {
        let Some(entry) = state
            .sessions
            .get_mut(name)
            .filter(|entry| entry.generation == generation)
        else {
            return Ok(false);
        };
        if entry
            .lifecycle
            .advance(entry.attachments, entry.leases.len())
        {
            bridge.changed.notify_all();
        }
        let Lifecycle::Holding(hold) = &entry.lifecycle else {
            return Ok(entry
                .lifecycle
                .attached(entry.attachments, entry.leases.len()));
        };
        // Once attached, the finite deadline no longer applies.
        let deadline = if hold.observed(entry.attachments, entry.leases.len()) {
            None
        } else {
            hold.deadline
        };
        state = bridge.wait_changed(state, deadline);
    }
}

pub fn wait_target(name: &str, generation: u64) -> Option<SessionMonitorTarget> {
    let bridge = current_bridge()?;
    let target = lock(&bridge.state)
        .sessions
        .get(name)
        .filter(|entry| entry.generation == generation)
        .map(|entry| entry.target.clone());
    target
}

pub(super) fn target_identity(name: &str, target: &SessionMonitorTarget) -> Option<(String, u64)> {
    let bridge = current_bridge()?;
    let identity = lock(&bridge.state)
        .sessions
        .get(name)
        .filter(|entry| entry.target.same_target(target))
        .map(|entry| {
            (
                format!("{}/{}", bridge.descriptor.owner, name),
                entry.generation,
            )
        });
    identity
}

/// Explicitly abandon this generation's inspection hold, including attached-client
/// retention. A following target-specific close can then finish without client cooperation.
pub fn cancel_wait(name: &str, generation: u64) {
    let Some(bridge) = current_bridge() else {
        return;
    };
    let mut state = lock(&bridge.state);
    if let Some(entry) = state
        .sessions
        .get_mut(name)
        .filter(|entry| entry.generation == generation)
    {
        if entry.outcome.is_some() {
            let attached = entry
                .lifecycle
                .attached(entry.attachments, entry.leases.len());
            match &mut entry.lifecycle {
                Lifecycle::Holding(_) => {
                    entry.lifecycle = Lifecycle::Completed {
                        attached,
                        hold_while_attached: false,
                    };
                }
                Lifecycle::Completed {
                    hold_while_attached,
                    ..
                }
                | Lifecycle::Closing {
                    hold_while_attached,
                    ..
                } => *hold_while_attached = false,
                Lifecycle::Running => {}
            }
        }
    }
    bridge.changed.notify_all();
}

/// Explicit interruption before or during inspection. Cancel only this captured
/// target's retention without creating a wait, generation, or outcome.
/// Follow with target-specific close; ordinary cleanup must not call this implicitly.
pub fn cancel_target(name: &str, target: &SessionMonitorTarget) {
    let Some(bridge) = current_bridge() else {
        return;
    };
    let mut state = lock(&bridge.state);
    if let Some(entry) = state
        .sessions
        .get_mut(name)
        .filter(|entry| entry.target.same_target(target))
    {
        entry.lifecycle = Lifecycle::Closing {
            attached: entry
                .lifecycle
                .attached(entry.attachments, entry.leases.len()),
            hold_while_attached: false,
        };
    }
    bridge.changed.notify_all();
}

/// A new spawn replaces the old generation; its inspection must not block restart.
pub(crate) fn prepare_replace(name: &str, pty: &Arc<Mutex<crate::terminal::pty::Pty>>) {
    if let Some(bridge) = current_bridge() {
        let mut state = lock(&bridge.state);
        if let Some(entry) = state
            .sessions
            .get_mut(name)
            .filter(|entry| entry.target.same_pty(pty))
        {
            entry.lifecycle = Lifecycle::Closing {
                attached: entry
                    .lifecycle
                    .attached(entry.attachments, entry.leases.len()),
                hold_while_attached: false,
            };
        }
        bridge.changed.notify_all();
    }
    prepare_close(name, pty);
}

pub(crate) fn prepare_close(name: &str, pty: &Arc<Mutex<crate::terminal::pty::Pty>>) {
    let Some(bridge) = current_bridge() else {
        return;
    };
    let mut state = lock(&bridge.state);
    loop {
        let Some(entry) = state
            .sessions
            .get_mut(name)
            .filter(|entry| entry.target.same_pty(pty))
        else {
            return;
        };
        if entry
            .lifecycle
            .advance(entry.attachments, entry.leases.len())
        {
            bridge.changed.notify_all();
        }
        let deadline = match &entry.lifecycle {
            Lifecycle::Holding(hold) => {
                if hold.observed(entry.attachments, entry.leases.len()) {
                    None
                } else {
                    hold.deadline
                }
            }
            _ => {
                let hold_while_attached = entry.lifecycle.hold_while_attached();
                entry.lifecycle = Lifecycle::Closing {
                    attached: entry
                        .lifecycle
                        .attached(entry.attachments, entry.leases.len()),
                    hold_while_attached,
                };
                bridge.changed.notify_all();
                if !hold_while_attached {
                    let active = entry
                        .leases
                        .values()
                        .map(|lease| lease.active.clone())
                        .collect::<Vec<_>>();
                    drop(state);
                    // Revoke input before destroying the child, without waiting for clients
                    // to close their sockets or holding the bridge's lifecycle mutex.
                    for active in active {
                        *lock(&active) = false;
                    }
                    return;
                }
                if entry.leases.is_empty() {
                    return;
                }
                None
            }
        };
        state = bridge.wait_changed(state, deadline);
    }
}

pub(crate) fn closed(name: &str, pty: &Arc<Mutex<crate::terminal::pty::Pty>>) {
    let Some(bridge) = current_bridge() else {
        return;
    };
    let mut state = lock(&bridge.state);
    if state
        .sessions
        .get(name)
        .is_some_and(|entry| entry.target.same_pty(pty))
    {
        state.sessions.remove(name);
    }
    bridge.changed.notify_all();
    drop(state);
    bridge.stop_if_idle(false);
}

fn serve(listener: interprocess::local_socket::Listener, bridge: Arc<Bridge>) {
    for connection in listener.incoming() {
        if lock(&bridge.state).stopped {
            break;
        }
        let Ok(connection) = connection else { continue };
        let bridge = bridge.clone();
        let _ = std::thread::Builder::new()
            .name("tui-test-monitor-client".into())
            .spawn(move || handle(connection, bridge));
    }
}

fn handle(connection: Stream, bridge: Arc<Bridge>) {
    if connection.set_nonblocking(true).is_err() {
        return;
    }
    let mut reader = BufReader::new(MonitorConnection(connection));
    let Ok(request) = read_handshake(&mut reader, &bridge) else {
        return;
    };
    let result = match request {
        Request::Ping => write_response(&mut reader.get_mut().0, &bridge, &Response::ok()),
        Request::HostSessions => write_response(
            &mut reader.get_mut().0,
            &bridge,
            &Response::with(
                serde_json::to_value(host_sessions(&bridge)).expect("serializable host snapshot"),
            ),
        ),
        Request::Routed {
            session,
            generation,
            lease,
            request,
        } => match *request {
            Request::MonitorLeaseStream { interactive } => {
                match Attachment::new(bridge.clone(), &session, generation, interactive) {
                    Ok(attachment) => {
                        let response = Response::with(
                            serde_json::to_value(MonitorLeaseReady {
                                lease: attachment.token,
                            })
                            .unwrap(),
                        );
                        if write_response(&mut reader.get_mut().0, &bridge, &response).is_ok() {
                            hold_lease(reader, attachment);
                        }
                        return;
                    }
                    Err(error) => Err(error),
                }
            }
            Request::Monitor {
                cols,
                rows,
                interactive,
            } => match leased_target(&bridge, &session, generation, lease, interactive, false) {
                Ok((target, _)) if cols != 0 && rows != 0 => {
                    stream_monitor(
                        reader.into_inner().0,
                        bridge,
                        session,
                        generation,
                        lease.unwrap(),
                        target,
                        (cols, rows),
                        interactive,
                    );
                    return;
                }
                Ok(_) => Err(TuiTestError::usage(
                    "monitor dimensions must be greater than zero",
                )),
                Err(error) => Err(error),
            },
            Request::MonitorInputStream { cols, rows } => {
                match leased_target(&bridge, &session, generation, lease, true, true) {
                    Ok((target, active)) => {
                        stream_input(
                            reader,
                            bridge,
                            session,
                            generation,
                            lease.unwrap(),
                            target,
                            active,
                            (cols, rows),
                        );
                        return;
                    }
                    Err(error) => Err(error),
                }
            }
            _ => Err(TuiTestError::usage(
                "process bridge supports monitor traffic only",
            )),
        },
        _ => Err(TuiTestError::usage(
            "process bridge request must be routed to a session",
        )),
    };
    if let Err(error) = result {
        let _ = write_response(
            &mut reader.get_mut().0,
            &bridge,
            &Response::from_error(error),
        );
    }
    drain_reply(&mut reader, &bridge);
}

struct MonitorConnection(Stream);

impl Read for MonitorConnection {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        ipc::read_available(&mut self.0, buffer)
    }
}

fn drain_reply(reader: &mut BufReader<MonitorConnection>, bridge: &Bridge) {
    // A named-pipe server must keep its end open until the peer consumes the reply.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut buffer = [0u8; 1024];
    while Instant::now() < deadline && !lock(&bridge.state).stopped {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if idle_read(&error) => network_pause(bridge, Duration::from_millis(10)),
            Err(_) => break,
        }
    }
}

fn network_pause(bridge: &Bridge, duration: Duration) {
    let state = lock(&bridge.state);
    if !state.stopped {
        drop(
            bridge
                .changed
                .wait_timeout(state, duration)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}

fn read_handshake(
    reader: &mut BufReader<MonitorConnection>,
    bridge: &Bridge,
) -> Result<Request, TuiTestError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut line = Vec::new();
    loop {
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => return Err(bridge_error("connection closed before request")),
            Ok(_) if line.last() == Some(&b'\n') => {
                return serde_json::from_slice(&line).map_err(bridge_error)
            }
            Ok(_) => {}
            Err(error) if idle_read(&error) => network_pause(bridge, Duration::from_millis(10)),
            Err(error) => return Err(bridge_error(error)),
        }
        if line.len() > 1024 * 1024 || Instant::now() >= deadline || lock(&bridge.state).stopped {
            return Err(bridge_error("incomplete or oversized monitor handshake"));
        }
    }
}

fn write_bytes(
    connection: &mut Stream,
    bridge: &Bridge,
    mut bytes: &[u8],
) -> Result<(), TuiTestError> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut chunk = 4096;
    while !bytes.is_empty() {
        match connection.write(&bytes[..bytes.len().min(chunk)]) {
            // PIPE_NOWAIT can refuse an oversized write even when its buffer is empty.
            Ok(0) if chunk > 256 => chunk /= 2,
            Ok(0) => network_pause(bridge, Duration::from_millis(10)),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if idle_read(&error) => network_pause(bridge, Duration::from_millis(10)),
            Err(error) => return Err(bridge_error(error)),
        }
        if Instant::now() >= deadline || lock(&bridge.state).stopped {
            return Err(bridge_error(
                "monitor connection stopped or write timed out",
            ));
        }
    }
    Ok(())
}

fn write_response(
    connection: &mut Stream,
    bridge: &Bridge,
    response: &Response,
) -> Result<(), TuiTestError> {
    let mut bytes = serde_json::to_vec(response).map_err(bridge_error)?;
    bytes.push(b'\n');
    write_bytes(connection, bridge, &bytes)
}

fn host_sessions(bridge: &Bridge) -> HostSnapshot {
    // Do not hold discovery/lifecycle state while reading an emulator.
    let entries = lock(&bridge.state)
        .sessions
        .iter()
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect::<Vec<_>>();
    let mut sessions = entries
        .into_iter()
        .filter_map(|(name, entry)| {
            let frame = entry.target.frame()?;
            Some(HostSession {
                id: format!("{}/{}", bridge.descriptor.owner, name),
                session: name,
                generation: entry.generation,
                owner: bridge.descriptor.owner.clone(),
                pid: bridge.descriptor.pid,
                label: entry.metadata.label,
                test_file: entry.metadata.test_file,
                test_name: entry.metadata.test_name,
                framework: entry.metadata.framework,
                worker: entry.metadata.worker,
                tags: entry.metadata.tags,
                status: entry.lifecycle.status(entry.leases.len(), entry.outcome),
                outcome: entry.outcome.map(|outcome| outcome.as_str().into()),
                child_exited: frame.exited.is_some(),
                exit_code: frame.exited,
                clients: entry.leases.len() as u32,
                interactive_clients: entry
                    .leases
                    .values()
                    .filter(|lease| lease.interactive)
                    .count() as u32,
                started_at: entry.started_at,
                completed_at: entry.completed_at,
                cwd: bridge.descriptor.cwd.clone(),
            })
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| left.session.cmp(&right.session));
    HostSnapshot {
        protocol: host::HOST_PROTOCOL,
        owner: bridge.descriptor.owner.clone(),
        capabilities: host::HOST_CAPABILITIES
            .iter()
            .map(|value| (*value).into())
            .collect(),
        sessions,
    }
}

fn leased_target(
    bridge: &Bridge,
    session: &str,
    generation: u64,
    token: Option<u64>,
    interactive: bool,
    input: bool,
) -> Result<(SessionMonitorTarget, Arc<Mutex<bool>>), TuiTestError> {
    let mut state = lock(&bridge.state);
    let entry = state
        .sessions
        .get_mut(session)
        .filter(|entry| entry.generation == generation)
        .ok_or_else(TuiTestError::no_session)?;
    let lease = token
        .and_then(|token| entry.leases.get_mut(&token))
        .ok_or_else(|| {
            TuiTestError::usage("a live attachment lease for this generation is required")
        })?;
    if interactive && !lease.interactive {
        return Err(TuiTestError::usage(
            "interactive input requires an interactive attachment lease",
        ));
    }
    if input {
        if lease.input_open {
            return Err(TuiTestError::usage(
                "attachment already has an input stream",
            ));
        }
        lease.input_open = true;
    }
    Ok((entry.target.clone(), lease.active.clone()))
}

fn lease_active(bridge: &Bridge, session: &str, generation: u64, token: u64) -> bool {
    let state = lock(&bridge.state);
    !state.stopped
        && state.sessions.get(session).is_some_and(|entry| {
            entry.generation == generation && entry.leases.contains_key(&token)
        })
}

#[allow(clippy::too_many_arguments)]
fn stream_monitor(
    mut connection: Stream,
    bridge: Arc<Bridge>,
    session: String,
    generation: u64,
    token: u64,
    target: SessionMonitorTarget,
    viewer: (u16, u16),
    interactive: bool,
) {
    let mut modes = render::ModeMirror::default();
    while lease_active(&bridge, &session, generation, token) {
        let bytes = render::render_frame(
            target.frame().as_ref(),
            viewer,
            &session,
            interactive,
            &mut modes,
        );
        if write_bytes(&mut connection, &bridge, &bytes).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(host::MONITOR_FRAME_MS));
    }
}

fn hold_lease(mut reader: BufReader<MonitorConnection>, attachment: Attachment) {
    let mut buffer = [0; 1];
    while lease_active(
        &attachment.bridge,
        &attachment.session,
        attachment.generation,
        attachment.token,
    ) {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if idle_read(&error) => {
                network_pause(&attachment.bridge, Duration::from_millis(20))
            }
            Err(_) => break,
        }
    }
}

fn idle_read(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
    )
}

fn content_size(viewer: (u16, u16)) -> Result<(u16, u16), TuiTestError> {
    if viewer.0 == 0 || viewer.1 == 0 {
        return Err(TuiTestError::usage(
            "monitor dimensions must be greater than zero",
        ));
    }
    Ok((viewer.0, viewer.1.saturating_sub(1).max(1)))
}

#[allow(clippy::too_many_arguments)]
fn stream_input(
    mut reader: BufReader<MonitorConnection>,
    bridge: Arc<Bridge>,
    session: String,
    generation: u64,
    token: u64,
    target: SessionMonitorTarget,
    active: Arc<Mutex<bool>>,
    viewer: (u16, u16),
) {
    let result = (|| -> Result<(), TuiTestError> {
        let initial = lock(&active);
        if !*initial {
            return Err(TuiTestError::no_session());
        }
        let (cols, rows) = content_size(viewer)?;
        target.resize(cols, rows)?;
        let mut modes = render::ModeMirror::default();
        let initial_frame =
            render::render_frame(target.frame().as_ref(), viewer, &session, true, &mut modes);
        write_response(
            &mut reader.get_mut().0,
            &bridge,
            &Response::with(serde_json::to_value(MonitorInputReady { initial_frame }).unwrap()),
        )?;
        drop(initial);
        let mut remapper = MouseRemapper::new(viewer);
        let mut pending = Vec::new();
        let mut buffer = [0u8; 16 * 1024];
        let mut last_input = Instant::now();
        loop {
            if !lease_active(&bridge, &session, generation, token) {
                break;
            }
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    pending.extend_from_slice(&buffer[..read]);
                    last_input = Instant::now();
                }
                Err(error) if idle_read(&error) => {
                    let current = lock(&active);
                    if !*current {
                        break;
                    }
                    let mouse_size = target
                        .frame()
                        .filter(|frame| frame.mouse_mode != crate::terminal::emu::MouseMode::None)
                        .map(|frame| frame.size);
                    remapper.observe(mouse_size);
                    if last_input.elapsed() >= Duration::from_millis(50) {
                        target.write_monitor_input_raw(&remapper.on_idle())?;
                    }
                    drop(current);
                    network_pause(&bridge, Duration::from_millis(10));
                    continue;
                }
                Err(error) => return Err(bridge_error(error)),
            }
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                let message: MonitorInput =
                    serde_json::from_slice(&pending[..end]).map_err(bridge_error)?;
                pending.drain(..=end);
                let current = lock(&active);
                if !*current {
                    break;
                }
                if !lease_active(&bridge, &session, generation, token) {
                    break;
                }
                match message {
                    MonitorInput::Write { data } => {
                        let mouse_size = target
                            .frame()
                            .filter(|frame| {
                                frame.mouse_mode != crate::terminal::emu::MouseMode::None
                            })
                            .map(|frame| frame.size);
                        let bytes = remapper.push(&data, mouse_size);
                        target.write_monitor_input_raw(&bytes)?;
                    }
                    MonitorInput::Resize { cols, rows } => {
                        let size = content_size((cols, rows))?;
                        target.resize(size.0, size.1)?;
                        remapper.resize((cols, rows));
                    }
                }
            }
            if pending.len() > 1024 * 1024 {
                return Err(TuiTestError::usage("monitor input message is too large"));
            }
        }
        let current = lock(&active);
        if *current && lease_active(&bridge, &session, generation, token) {
            target.write_monitor_input_raw(&remapper.finish())?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = write_response(
            &mut reader.get_mut().0,
            &bridge,
            &Response::from_error(error),
        );
        drain_reply(&mut reader, &bridge);
    }
    let mut state = lock(&bridge.state);
    if let Some(lease) = state
        .sessions
        .get_mut(&session)
        .filter(|entry| entry.generation == generation)
        .and_then(|entry| entry.leases.get_mut(&token))
    {
        lease.input_open = false;
    }
}

struct Attachment {
    bridge: Arc<Bridge>,
    session: String,
    generation: u64,
    token: u64,
    active: Arc<Mutex<bool>>,
}

impl Attachment {
    fn new(
        bridge: Arc<Bridge>,
        session: &str,
        generation: u64,
        interactive: bool,
    ) -> Result<Self, TuiTestError> {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).map_err(bridge_error)?;
        let token = u64::from_ne_bytes(bytes);
        let mut state = lock(&bridge.state);
        if state.stopped {
            return Err(TuiTestError::no_session());
        }
        let entry = state
            .sessions
            .get_mut(session)
            .filter(|entry| entry.generation == generation && entry.lifecycle.accepts_attachments())
            .ok_or_else(TuiTestError::no_session)?;
        if interactive && entry.leases.values().any(|lease| lease.interactive) {
            return Err(TuiTestError::usage(
                "session already has an interactive monitor",
            ));
        }
        if entry.leases.contains_key(&token) {
            return Err(TuiTestError::internal(
                "monitor attachment token collision; retry attachment",
            ));
        }
        let active = Arc::new(Mutex::new(true));
        entry.leases.insert(
            token,
            Lease {
                interactive,
                input_open: false,
                active: active.clone(),
            },
        );
        entry.attachments = entry.attachments.wrapping_add(1);
        state.clients += 1;
        bridge.changed.notify_all();
        drop(state);
        Ok(Self {
            bridge,
            session: session.into(),
            generation,
            token,
            active,
        })
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        // Finish an in-flight resize/write before handing interactive ownership to another client.
        let mut active = lock(&self.active);
        *active = false;
        let mut state = lock(&self.bridge.state);
        if let Some(entry) = state
            .sessions
            .get_mut(&self.session)
            .filter(|entry| entry.generation == self.generation)
        {
            entry.leases.remove(&self.token);
        }
        state.clients = state.clients.saturating_sub(1);
        self.bridge.changed.notify_all();
        drop(state);
        drop(active);
        self.bridge.stop_if_idle(false);
    }
}
