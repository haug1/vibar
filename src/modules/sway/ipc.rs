use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use swayipc::{Connection, Event, EventType};

use crate::modules::broadcaster::BackendRegistry;

const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);
const STREAM_END_RETRY_DELAY: Duration = Duration::from_millis(200);
const GLOBAL_EVENT_TYPES: &[EventType] = &[
    EventType::Workspace,
    EventType::Output,
    EventType::Mode,
    EventType::Window,
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SharedEventsKey;

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

fn shared_events_registry() -> &'static BackendRegistry<SharedEventsKey, EventFanout> {
    static REGISTRY: OnceLock<BackendRegistry<SharedEventsKey, EventFanout>> = OnceLock::new();
    REGISTRY.get_or_init(BackendRegistry::new)
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
            |event_type| fanout.broadcast(event_type),
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

pub(crate) fn query_with_connection<T, F>(
    module: &'static str,
    action: &'static str,
    query: F,
) -> Option<T>
where
    F: FnOnce(&mut Connection) -> Result<T, swayipc::Error>,
{
    let mut connection = match Connection::new() {
        Ok(conn) => conn,
        Err(err) => {
            debug_log(module, &format!("failed to connect for {action}: {err}"));
            return None;
        }
    };

    match query(&mut connection) {
        Ok(value) => Some(value),
        Err(err) => {
            debug_log(module, &format!("{action} failed: {err}"));
            None
        }
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
