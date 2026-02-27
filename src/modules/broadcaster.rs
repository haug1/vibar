use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

/// Fan-out broadcaster that sends updates to multiple subscribers.
///
/// Each subscriber receives updates via a standard mpsc channel.
/// New subscribers immediately receive the latest cached value (if any).
/// Dead subscribers are pruned on each broadcast.
pub(crate) struct Broadcaster<U: Clone + Send> {
    latest: Mutex<Option<U>>,
    pub(crate) subscribers: Mutex<Vec<std::sync::mpsc::Sender<U>>>,
}

impl<U: Clone + Send> Broadcaster<U> {
    pub(crate) fn new() -> Self {
        Self {
            latest: Mutex::new(None),
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// Creates a new subscriber channel. If a latest value exists, it is
    /// immediately sent to the new subscriber (replay).
    pub(crate) fn subscribe(&self) -> std::sync::mpsc::Receiver<U> {
        let (sender, receiver) = std::sync::mpsc::channel();

        if let Some(latest) = self
            .latest
            .lock()
            .expect("broadcaster latest mutex poisoned")
            .clone()
        {
            let _ = sender.send(latest);
        }

        self.subscribers
            .lock()
            .expect("broadcaster subscribers mutex poisoned")
            .push(sender);

        receiver
    }

    /// Sends an update to all live subscribers, prunes dead ones, and caches
    /// the value for future subscribers.
    pub(crate) fn broadcast(&self, update: U) {
        *self
            .latest
            .lock()
            .expect("broadcaster latest mutex poisoned") = Some(update.clone());

        self.subscribers
            .lock()
            .expect("broadcaster subscribers mutex poisoned")
            .retain(|sender| sender.send(update.clone()).is_ok());
    }

    /// Returns the number of currently live subscribers.
    pub(crate) fn subscriber_count(&self) -> usize {
        self.subscribers
            .lock()
            .expect("broadcaster subscribers mutex poisoned")
            .len()
    }
}

/// Static deduplication registry that maps keys to shared backends.
///
/// `get_or_create` returns an existing backend for the given key, or creates
/// a new one via the provided closure. The returned `bool` indicates whether
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
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn broadcaster_sends_to_all_subscribers() {
        let bc = Broadcaster::new();
        let rx_a = bc.subscribe();
        let rx_b = bc.subscribe();

        bc.broadcast("hello".to_string());

        assert_eq!(
            rx_a.recv_timeout(Duration::from_millis(100)).unwrap(),
            "hello"
        );
        assert_eq!(
            rx_b.recv_timeout(Duration::from_millis(100)).unwrap(),
            "hello"
        );
    }

    #[test]
    fn broadcaster_replays_latest_to_new_subscriber() {
        let bc = Broadcaster::new();
        bc.broadcast("cached".to_string());

        let rx = bc.subscribe();
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(100)).unwrap(),
            "cached"
        );
    }

    #[test]
    fn broadcaster_prunes_dead_subscribers() {
        let bc = Broadcaster::new();
        let (dead_tx, _dead_rx) = mpsc::channel::<String>();
        drop(_dead_rx);
        bc.subscribers.lock().unwrap().push(dead_tx);

        let _alive_rx = bc.subscribe();
        assert_eq!(bc.subscriber_count(), 2);

        bc.broadcast("test".to_string());
        assert_eq!(bc.subscriber_count(), 1);
    }

    #[test]
    fn broadcaster_subscriber_count_tracks_subscribers() {
        let bc = Broadcaster::<String>::new();
        assert_eq!(bc.subscriber_count(), 0);

        let _rx1 = bc.subscribe();
        assert_eq!(bc.subscriber_count(), 1);

        let _rx2 = bc.subscribe();
        assert_eq!(bc.subscriber_count(), 2);
    }

    #[test]
    fn broadcaster_no_replay_when_no_value_yet() {
        let bc = Broadcaster::<String>::new();
        let rx = bc.subscribe();
        assert!(rx.try_recv().is_err());
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
