use std::time::Duration;

use swayipc::{Connection, EventType};

const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);
const STREAM_END_RETRY_DELAY: Duration = Duration::from_millis(200);

pub(crate) fn run_event_loop<FShouldStop, FOnEvent>(
    module: &'static str,
    event_types: &[EventType],
    should_stop: FShouldStop,
    mut on_event: FOnEvent,
) where
    FShouldStop: Fn() -> bool,
    FOnEvent: FnMut(),
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
            debug_log(module, &format!("event={event:?}"));
            on_event();
        }

        debug_log(module, "event stream ended, reconnecting");
        std::thread::sleep(STREAM_END_RETRY_DELAY);
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
