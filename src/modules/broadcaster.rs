use std::collections::HashMap;
use std::hash::Hash;
use std::os::unix::io::RawFd;
use std::sync::{Arc, Mutex};

use gtk::glib;
use gtk::glib::IOCondition;
use gtk::prelude::*;

/// Fan-out broadcaster that sends updates to multiple subscribers.
///
/// Each subscriber receives updates via an mpsc channel paired with a unix
/// pipe for event-driven wakeup.  The pipe integrates with the GTK main loop
/// via `unix_fd_add_local` — callbacks fire only when data arrives, with zero
/// polling overhead.
pub(crate) struct Broadcaster<U: Clone + Send> {
    latest: Mutex<Option<U>>,
    subscribers: Mutex<Vec<SubscriberSlot<U>>>,
}

struct SubscriberSlot<U> {
    sender: std::sync::mpsc::Sender<U>,
    /// Write-end of the notification pipe.  A single byte is written on
    /// each broadcast to wake the GTK main loop via `unix_fd_add_local`.
    notify_fd: RawFd,
}

impl<U> Drop for SubscriberSlot<U> {
    fn drop(&mut self) {
        unsafe { libc::close(self.notify_fd) };
    }
}

/// Returned by [`Broadcaster::subscribe`].  Holds the mpsc receiver and the
/// read-end of the notification pipe.
pub(crate) struct Subscription<U> {
    pub(crate) receiver: std::sync::mpsc::Receiver<U>,
    pub(crate) notify_fd: RawFd,
}

impl<U> Drop for Subscription<U> {
    fn drop(&mut self) {
        unsafe { libc::close(self.notify_fd) };
    }
}

impl<U: Clone + Send> Broadcaster<U> {
    pub(crate) fn new() -> Self {
        Self {
            latest: Mutex::new(None),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Creates a new subscriber.  Returns a [`Subscription`] containing the
    /// mpsc receiver and a notification pipe fd.
    ///
    /// If a latest value exists it is immediately queued (replay).
    /// Registration and replay are atomic.
    pub(crate) fn subscribe(&self) -> Subscription<U> {
        let (sender, receiver) = std::sync::mpsc::channel();

        let mut fds = [0i32; 2];
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        assert_eq!(rc, 0, "failed to create notification pipe");
        let read_fd = fds[0];
        let write_fd = fds[1];

        let latest = self
            .latest
            .lock()
            .expect("broadcaster latest mutex poisoned");

        if let Some(value) = latest.clone() {
            let _ = sender.send(value);
            // Notify that data is available
            let _ = nix_write_byte(write_fd);
        }

        self.subscribers
            .lock()
            .expect("broadcaster subscribers mutex poisoned")
            .push(SubscriberSlot {
                sender,
                notify_fd: write_fd,
            });

        Subscription {
            receiver,
            notify_fd: read_fd,
        }
    }

    /// Sends an update to all live subscribers, prunes dead ones, and caches
    /// the value for future subscribers.
    pub(crate) fn broadcast(&self, update: U) {
        let mut latest = self
            .latest
            .lock()
            .expect("broadcaster latest mutex poisoned");
        *latest = Some(update.clone());

        self.subscribers
            .lock()
            .expect("broadcaster subscribers mutex poisoned")
            .retain(|slot| {
                if slot.sender.send(update.clone()).is_ok() {
                    let _ = nix_write_byte(slot.notify_fd);
                    true
                } else {
                    false
                }
            });
    }

    /// Returns the number of currently live subscribers.
    pub(crate) fn subscriber_count(&self) -> usize {
        self.subscribers
            .lock()
            .expect("broadcaster subscribers mutex poisoned")
            .len()
    }
}

fn nix_write_byte(fd: RawFd) -> std::io::Result<()> {
    let buf = [1u8];
    // SAFETY: fd is a valid pipe write-end, buf is a stack buffer.
    let rc = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, 1) };
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn drain_pipe(fd: RawFd) {
    let mut buf = [0u8; 64];
    loop {
        let rc = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if rc <= 0 {
            break;
        }
    }
}

/// Wires a [`Subscription`] to the GTK main loop via `unix_fd_add_local`.
///
/// `apply_fn` is called for each update.  When the widget is destroyed the
/// source is automatically removed and the subscription dropped, which
/// closes the pipe and lets the broadcaster prune the dead sender.
pub(crate) fn attach_subscription<W, U>(
    widget: &W,
    subscription: Subscription<U>,
    mut apply_fn: impl FnMut(&W, U) + 'static,
) where
    W: gtk::prelude::IsA<gtk::Widget> + Clone + 'static,
    U: 'static,
{
    use std::cell::RefCell;
    use std::rc::Rc;

    let widget_weak = widget.downgrade();
    let fd = subscription.notify_fd;

    // Wrap subscription in Rc<RefCell> so the destroy handler can also drop
    // it.  This ensures cleanup even if no broadcast arrives after the
    // widget is destroyed.
    let sub_cell = Rc::new(RefCell::new(Some(subscription)));
    let sub_cell_for_destroy = Rc::clone(&sub_cell);

    let source_id_cell: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let source_id_for_destroy = Rc::clone(&source_id_cell);

    let source_id = glib::unix_fd_add_local(fd, IOCondition::IN, move |_, _| {
        drain_pipe(fd);
        let Some(widget) = widget_weak.upgrade() else {
            // Widget gone — drop subscription, closing the pipe and
            // letting the broadcaster prune on next broadcast.
            sub_cell.borrow_mut().take();
            return glib::ControlFlow::Break;
        };
        if let Some(sub) = sub_cell.borrow().as_ref() {
            while let Ok(update) = sub.receiver.try_recv() {
                apply_fn(&widget, update);
            }
        }
        glib::ControlFlow::Continue
    });

    *source_id_cell.borrow_mut() = Some(source_id);

    widget.connect_destroy(move |_| {
        // Drop the subscription immediately, closing the pipe read-end.
        sub_cell_for_destroy.borrow_mut().take();
        // Remove the fd source so the closure is dropped too.
        if let Some(id) = source_id_for_destroy.borrow_mut().take() {
            id.remove();
        }
    });
}

/// Static deduplication registry that maps keys to shared backends.
///
/// `get_or_create` returns an existing backend for the given key, or creates
/// a new one via the provided closure.  The returned `bool` indicates whether
/// the backend was newly created (caller should start the worker thread only
/// if `true`).
pub(crate) struct BackendRegistry<K: Eq + Hash, B> {
    backends: Mutex<HashMap<K, Arc<B>>>,
}

impl<K: Eq + Hash + Clone, B> BackendRegistry<K, B> {
    pub(crate) fn new() -> Self {
        Self {
            backends: Mutex::new(HashMap::new()),
        }
    }

    /// Returns an existing backend for `key`, or creates one via `init_fn`.
    /// The `bool` is `true` when newly created (caller should start worker).
    pub(crate) fn get_or_create(&self, key: K, init_fn: impl FnOnce() -> B) -> (Arc<B>, bool) {
        let mut backends = self
            .backends
            .lock()
            .expect("backend registry mutex poisoned");

        if let Some(existing) = backends.get(&key) {
            return (Arc::clone(existing), false);
        }

        let backend = Arc::new(init_fn());
        backends.insert(key, Arc::clone(&backend));
        (backend, true)
    }

    /// Removes the backend for `key` if it is the same instance as `backend`.
    pub(crate) fn remove(&self, key: &K, backend: &Arc<B>) {
        let mut backends = self
            .backends
            .lock()
            .expect("backend registry mutex poisoned");

        if let Some(existing) = backends.get(key) {
            if Arc::ptr_eq(existing, backend) {
                backends.remove(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn broadcaster_sends_to_all_subscribers() {
        let bc = Broadcaster::new();
        let sub_a = bc.subscribe();
        let sub_b = bc.subscribe();

        bc.broadcast("hello".to_string());

        assert_eq!(
            sub_a
                .receiver
                .recv_timeout(Duration::from_millis(100))
                .unwrap(),
            "hello"
        );
        assert_eq!(
            sub_b
                .receiver
                .recv_timeout(Duration::from_millis(100))
                .unwrap(),
            "hello"
        );
    }

    #[test]
    fn broadcaster_replays_latest_to_new_subscriber() {
        let bc = Broadcaster::new();
        bc.broadcast("cached".to_string());

        let sub = bc.subscribe();
        assert_eq!(
            sub.receiver
                .recv_timeout(Duration::from_millis(100))
                .unwrap(),
            "cached"
        );
    }

    #[test]
    fn broadcaster_prunes_dead_subscribers() {
        let bc = Broadcaster::new();

        // Create a subscriber then drop it to simulate a dead one
        let sub = bc.subscribe();
        assert_eq!(bc.subscriber_count(), 1);
        drop(sub);

        // Create a live one
        let _alive_sub = bc.subscribe();
        assert_eq!(bc.subscriber_count(), 2);

        bc.broadcast("test".to_string());
        assert_eq!(bc.subscriber_count(), 1);
    }

    #[test]
    fn broadcaster_subscriber_count_tracks_subscribers() {
        let bc = Broadcaster::<String>::new();
        assert_eq!(bc.subscriber_count(), 0);

        let _sub1 = bc.subscribe();
        assert_eq!(bc.subscriber_count(), 1);

        let _sub2 = bc.subscribe();
        assert_eq!(bc.subscriber_count(), 2);
    }

    #[test]
    fn broadcaster_no_replay_when_no_value_yet() {
        let bc = Broadcaster::<String>::new();
        let sub = bc.subscribe();
        assert!(sub.receiver.try_recv().is_err());
    }

    #[test]
    fn backend_registry_creates_new_on_first_call() {
        let registry = BackendRegistry::<String, String>::new();
        let (backend, is_new) = registry.get_or_create("key".to_string(), || "value".to_string());
        assert!(is_new);
        assert_eq!(*backend, "value");
    }

    #[test]
    fn backend_registry_returns_existing_on_second_call() {
        let registry = BackendRegistry::<String, String>::new();
        let (first, is_new_1) = registry.get_or_create("key".to_string(), || "first".to_string());
        assert!(is_new_1);

        let (second, is_new_2) =
            registry.get_or_create("key".to_string(), || "should not be called".to_string());
        assert!(!is_new_2);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn backend_registry_remove_cleans_up_matching_entry() {
        let registry = BackendRegistry::<String, String>::new();
        let (backend, _) = registry.get_or_create("key".to_string(), || "value".to_string());

        registry.remove(&"key".to_string(), &backend);

        let (new_backend, is_new) =
            registry.get_or_create("key".to_string(), || "new_value".to_string());
        assert!(is_new);
        assert_eq!(*new_backend, "new_value");
    }

    #[test]
    fn backend_registry_remove_ignores_different_instance() {
        let registry = BackendRegistry::<String, String>::new();
        let (_backend, _) = registry.get_or_create("key".to_string(), || "value".to_string());

        let impostor = Arc::new("impostor".to_string());
        registry.remove(&"key".to_string(), &impostor);

        let (existing, is_new) =
            registry.get_or_create("key".to_string(), || "should not be called".to_string());
        assert!(!is_new);
        assert_eq!(*existing, "value");
    }

    #[test]
    fn backend_registry_independent_keys() {
        let registry = BackendRegistry::<String, String>::new();
        let (a, new_a) = registry.get_or_create("a".to_string(), || "alpha".to_string());
        let (b, new_b) = registry.get_or_create("b".to_string(), || "beta".to_string());

        assert!(new_a);
        assert!(new_b);
        assert_eq!(*a, "alpha");
        assert_eq!(*b, "beta");
        assert!(!Arc::ptr_eq(&a, &b));
    }
}
