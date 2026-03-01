use std::{
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use swayipc::{Connection, Event, EventType, Node, Workspace};

use crate::modules::broadcaster::BackendRegistry;

const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);
const STREAM_END_RETRY_DELAY: Duration = Duration::from_millis(200);
const EVENT_IDLE_TIMEOUT: Duration = Duration::from_millis(500);
const EVENT_COALESCE_WINDOW: Duration = Duration::from_millis(40);
const SNAPSHOT_CACHE_TTL: Duration = Duration::from_millis(100);
const GLOBAL_EVENT_TYPES: &[EventType] = &[
    EventType::Workspace,
    EventType::Output,
    EventType::Mode,
    EventType::Window,
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SharedEventsKey;

#[derive(Debug, Clone)]
pub(crate) struct SwaySnapshot {
    pub(crate) workspaces: Option<Vec<Workspace>>,
    pub(crate) mode: Option<String>,
    pub(crate) tree: Option<Node>,
}

struct EventFanout {
    subscribers: Mutex<Vec<std::sync::mpsc::Sender<EventType>>>,
}

impl EventFanout {
    fn new() -> Self {
        Self {
            subscribers: Mutex::new(Vec::new()),
        }
    }

    fn subscribe(&self) -> std::sync::mpsc::Receiver<EventType> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.subscribers
            .lock()
            .expect("sway event fanout mutex poisoned")
            .push(tx);
        rx
    }

    fn broadcast(&self, event_type: EventType) {
        self.subscribers
            .lock()
            .expect("sway event fanout mutex poisoned")
            .retain(|sender| sender.send(event_type).is_ok());
    }

    fn subscriber_count(&self) -> usize {
        self.subscribers
            .lock()
            .expect("sway event fanout mutex poisoned")
            .len()
    }
}

#[derive(Default)]
struct SnapshotCache {
    updated_at: Option<Instant>,
    snapshot: Option<Arc<SwaySnapshot>>,
}

fn shared_events_registry() -> &'static BackendRegistry<SharedEventsKey, EventFanout> {
    static REGISTRY: OnceLock<BackendRegistry<SharedEventsKey, EventFanout>> = OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
}

fn shared_command_connection() -> &'static Mutex<Option<Connection>> {
    static CONNECTION: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();
    CONNECTION.get_or_init(|| Mutex::new(None))
}

fn snapshot_cache() -> &'static Mutex<SnapshotCache> {
    static CACHE: OnceLock<Mutex<SnapshotCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(SnapshotCache::default()))
}

pub(crate) fn subscribe_shared_events() -> std::sync::mpsc::Receiver<EventType> {
    let key = SharedEventsKey;
    let (fanout, start_worker) =
        shared_events_registry().get_or_create(key.clone(), EventFanout::new);
    let receiver = fanout.subscribe();

    if start_worker {
        start_shared_events_worker(key, fanout);
    }

    receiver
}

fn start_shared_events_worker(key: SharedEventsKey, fanout: Arc<EventFanout>) {
    std::thread::spawn(move || {
        run_event_loop(
            "ipc",
            GLOBAL_EVENT_TYPES,
            || {
                if fanout.subscriber_count() == 0 {
                    shared_events_registry().remove(&key, &fanout);
                    return true;
                }
                false
            },
            |event_type| {
                invalidate_snapshot_cache();
                fanout.broadcast(event_type)
            },
        );
    });
}

pub(crate) fn run_event_loop<FShouldStop, FOnEvent>(
    module: &'static str,
    event_types: &[EventType],
    should_stop: FShouldStop,
    mut on_event: FOnEvent,
) where
    FShouldStop: Fn() -> bool,
    FOnEvent: FnMut(EventType),
{
    loop {
        if should_stop() {
            return;
        }

        let connection = match Connection::new() {
            Ok(conn) => conn,
            Err(err) => {
                debug_log(module, &format!("failed to connect for events: {err}"));
                std::thread::sleep(CONNECT_RETRY_DELAY);
                continue;
            }
        };

        let stream = match connection.subscribe(event_types) {
            Ok(stream) => stream,
            Err(err) => {
                debug_log(module, &format!("failed to subscribe to events: {err}"));
                std::thread::sleep(CONNECT_RETRY_DELAY);
                continue;
            }
        };

        for event in stream {
            if should_stop() {
                return;
            }
            match event {
                Ok(event) => {
                    debug_log(module, &format!("event={event:?}"));
                    if let Some(event_type) = event_type_from_event(&event) {
                        on_event(event_type);
                    }
                }
                Err(err) => {
                    debug_log(module, &format!("event stream read failed: {err}"));
                    break;
                }
            }
        }

        debug_log(module, "event stream ended, reconnecting");
        std::thread::sleep(STREAM_END_RETRY_DELAY);
    }
}

pub(crate) fn recv_relevant_event_coalesced(
    events: &std::sync::mpsc::Receiver<EventType>,
    relevant: &[EventType],
) -> Result<bool, std::sync::mpsc::RecvTimeoutError> {
    let first = loop {
        match events.recv_timeout(EVENT_IDLE_TIMEOUT) {
            Ok(event_type) if relevant.contains(&event_type) => break Some(event_type),
            Ok(_) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
            }
        }
    };

    let Some(_) = first else {
        return Ok(false);
    };

    let deadline = Instant::now() + EVENT_COALESCE_WINDOW;
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(now);
        match events.recv_timeout(remaining) {
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
            }
        }
    }

    Ok(true)
}

pub(crate) fn query_snapshot() -> Arc<SwaySnapshot> {
    {
        let cache = snapshot_cache()
            .lock()
            .expect("sway snapshot cache mutex poisoned");
        if let (Some(updated_at), Some(snapshot)) = (cache.updated_at, cache.snapshot.as_ref()) {
            if updated_at.elapsed() <= SNAPSHOT_CACHE_TTL {
                return Arc::clone(snapshot);
            }
        }
    }

    let snapshot = Arc::new(query_snapshot_uncached());

    let mut cache = snapshot_cache()
        .lock()
        .expect("sway snapshot cache mutex poisoned");
    cache.updated_at = Some(Instant::now());
    cache.snapshot = Some(Arc::clone(&snapshot));

    snapshot
}

fn query_snapshot_uncached() -> SwaySnapshot {
    query_with_connection("ipc", "snapshot query", |connection| {
        Ok(SwaySnapshot {
            workspaces: connection.get_workspaces().ok(),
            mode: connection.get_binding_state().ok(),
            tree: connection.get_tree().ok(),
        })
    })
    .unwrap_or(SwaySnapshot {
        workspaces: None,
        mode: None,
        tree: None,
    })
}

pub(crate) fn query_with_connection<T, F>(
    module: &'static str,
    action: &'static str,
    query: F,
) -> Option<T>
where
    F: Fn(&mut Connection) -> Result<T, swayipc::Error>,
{
    let mut connection = shared_command_connection()
        .lock()
        .expect("sway shared command connection mutex poisoned");

    ensure_connection(&mut connection, module, action)?;

    match query(
        connection
            .as_mut()
            .expect("connection should be initialized"),
    ) {
        Ok(value) => Some(value),
        Err(err) => {
            debug_log(module, &format!("{action} failed (retrying): {err}"));
            *connection = None;

            ensure_connection(&mut connection, module, action)?;

            match query(
                connection
                    .as_mut()
                    .expect("connection should be initialized"),
            ) {
                Ok(value) => Some(value),
                Err(err) => {
                    debug_log(module, &format!("{action} failed after reconnect: {err}"));
                    *connection = None;
                    None
                }
            }
        }
    }
}

fn ensure_connection(
    connection: &mut Option<Connection>,
    module: &str,
    action: &str,
) -> Option<()> {
    if connection.is_some() {
        return Some(());
    }

    match Connection::new() {
        Ok(conn) => {
            *connection = Some(conn);
            Some(())
        }
        Err(err) => {
            debug_log(module, &format!("failed to connect for {action}: {err}"));
            None
        }
    }
}

fn invalidate_snapshot_cache() {
    let mut cache = snapshot_cache()
        .lock()
        .expect("sway snapshot cache mutex poisoned");
    cache.updated_at = None;
    cache.snapshot = None;
}

fn event_type_from_event(event: &Event) -> Option<EventType> {
    match event {
        Event::Workspace(_) => Some(EventType::Workspace),
        Event::Output(_) => Some(EventType::Output),
        Event::Mode(_) => Some(EventType::Mode),
        Event::Window(_) => Some(EventType::Window),
        Event::BarConfigUpdate(_) => Some(EventType::BarConfigUpdate),
        Event::Binding(_) => Some(EventType::Binding),
        Event::Shutdown(_) => Some(EventType::Shutdown),
        Event::Tick(_) => Some(EventType::Tick),
        Event::BarStateUpdate(_) => Some(EventType::BarStateUpdate),
        Event::Input(_) => Some(EventType::Input),
        _ => None,
    }
}

fn debug_log(module: &str, message: &str) {
    if debug_enabled() {
        eprintln!("vibar/{module}: {message}");
    }
}

fn debug_enabled() -> bool {
    std::env::var("VIBAR_DEBUG_SWAY_IPC")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}
