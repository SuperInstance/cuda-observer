/*!
# cuda-observer

Reactive observer pattern for agents.

Agents react to changes. This crate provides observable values,
subscription management, signal propagation, and change detection
with dependency tracking.

- Observable values with change notification
- Subscribe/unsubscribe
- Signal propagation (fan-out)
- Dependency tracking (who depends on what)
- Batched updates (coalesce multiple changes)
- Change history
*/

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// An observed change
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Change {
    pub source_id: String,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: String,
    pub timestamp: u64,
}

/// A subscription
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub subscriber_id: String,
    pub source_id: String,
    pub field_filter: Option<String>, // None = all fields
    pub created: u64,
    pub notifications_received: u64,
    pub active: bool,
}

/// An observable signal
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub value: String,
    pub version: u64,
    pub subscribers: HashSet<String>, // subscription IDs
    pub dependents: HashSet<String>,  // other signal IDs that depend on this
    pub change_count: u64,
    pub created: u64,
}

impl Signal {
    pub fn new(id: &str, initial_value: &str) -> Self {
        Signal { id: id.to_string(), value: initial_value.to_string(), version: 0, subscribers: HashSet::new(), dependents: HashSet::new(), change_count: 0, created: now() }
    }
}

/// The observer system
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObserverSystem {
    pub signals: HashMap<String, Signal>,
    pub subscriptions: HashMap<String, Subscription>,
    pub changes: Vec<Change>,
    pub dependencies: HashMap<String, Vec<String>>, // signal → [signals it depends on]
    pub pending: Vec<Change>,
    pub batching: bool,
    pub total_notifications: u64,
}

impl ObserverSystem {
    pub fn new() -> Self { ObserverSystem { signals: HashMap::new(), subscriptions: HashMap::new(), changes: vec![], dependencies: HashMap::new(), pending: vec![], batching: false, total_notifications: 0 } }

    /// Create a signal
    pub fn create_signal(&mut self, id: &str, value: &str) {
        self.signals.insert(id.to_string(), Signal::new(id, value));
    }

    /// Update a signal and notify
    pub fn update(&mut self, signal_id: &str, new_value: &str) -> Vec<&Change> {
        let signal = match self.signals.get_mut(signal_id) { Some(s) => s, None => return vec![] };
        let old = signal.value.clone();
        if old == new_value { return vec![]; }
        signal.value = new_value.to_string();
        signal.version += 1;
        signal.change_count += 1;

        let change = Change { source_id: signal_id.to_string(), field: "value".into(), old_value: Some(old), new_value: new_value.to_string(), timestamp: now() };

        if self.batching {
            self.pending.push(change);
            vec![]
        } else {
            self.propagate(change)
        }
    }

    /// Propagate a change to subscribers
    fn propagate(&mut self, change: Change) -> Vec<&Change> {
        let signal_id = change.source_id.clone();
        let mut notified = vec![];
        let subs: Vec<String> = self.subscriptions.values()
            .filter(|s| s.active && s.source_id == signal_id)
            .filter(|s| s.field_filter.as_ref().map_or(true, |f| f == &change.field))
            .map(|s| s.id.clone())
            .collect();

        for sub_id in subs {
            if let Some(sub) = self.subscriptions.get_mut(&sub_id) {
                sub.notifications_received += 1;
            }
            self.total_notifications += 1;
            notified.push(self.changes.len());
            self.changes.push(change.clone());
        }

        // Propagate to dependents
        let dependents: Vec<String> = self.signals.get(&signal_id).map(|s| s.dependents.iter().cloned().collect()).unwrap_or_default();
        for dep_id in dependents {
            if let Some(dep) = self.signals.get(&dep_id) {
                // Mark dependent as dirty (could trigger recompute)
                self.changes.push(Change { source_id: dep_id.clone(), field: "dirty".into(), old_value: None, new_value: "true".into(), timestamp: now() });
            }
        }

        notified.iter().filter_map(|&i| self.changes.get(i)).collect()
    }

    /// Subscribe to a signal
    pub fn subscribe(&mut self, subscriber_id: &str, signal_id: &str, field_filter: Option<&str>) -> Option<String> {
        let sub_id = format!("sub_{}", self.subscriptions.len() + 1);
        let sub = Subscription { id: sub_id.clone(), subscriber_id: subscriber_id.to_string(), source_id: signal_id.to_string(), field_filter: field_filter.map(|f| f.to_string()), created: now(), notifications_received: 0, active: true };
        self.subscriptions.insert(sub_id.clone(), sub);
        if let Some(signal) = self.signals.get_mut(signal_id) { signal.subscribers.insert(sub_id.clone()); }
        Some(sub_id)
    }

    /// Unsubscribe
    pub fn unsubscribe(&mut self, sub_id: &str) {
        if let Some(sub) = self.subscriptions.remove(sub_id) {
            if let Some(signal) = self.signals.get_mut(&sub.source_id) { signal.subscribers.remove(sub_id); }
        }
    }

    /// Add dependency: dep_id depends on signal_id
    pub fn add_dependency(&mut self, dep_id: &str, depends_on: &str) {
        self.dependencies.entry(dep_id.to_string()).or_default().push(depends_on.to_string());
        if let Some(signal) = self.signals.get_mut(depends_on) { signal.dependents.insert(dep_id.to_string()); }
    }

    /// Start batch mode
    pub fn begin_batch(&mut self) { self.batching = true; }

    /// End batch mode and flush pending
    pub fn end_batch(&mut self) -> Vec<&Change> {
        self.batching = false;
        let pending = std::mem::take(&mut self.pending);
        let mut all = vec![];
        for change in pending { all.extend(self.propagate(change)); }
        all
    }

    /// Get change history for a signal
    pub fn history_for(&self, signal_id: &str) -> Vec<&Change> {
        self.changes.iter().filter(|c| c.source_id == signal_id).collect()
    }

    /// Get dependents of a signal
    pub fn dependents_of(&self, signal_id: &str) -> Vec<&str> {
        self.signals.get(signal_id).map(|s| s.dependents.iter().map(|d| d.as_str()).collect()).unwrap_or_default()
    }

    /// Summary
    pub fn summary(&self) -> String {
        format!("Observer: {} signals, {} subscriptions, {} changes, {} notifications, dependencies={}",
            self.signals.len(), self.subscriptions.len(), self.changes.len(), self.total_notifications,
            self.dependencies.values().map(|v| v.len()).sum::<usize>())
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_update() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("temp", "20");
        obs.subscribe("a1", "temp", None);
        let notified = obs.update("temp", "25");
        assert_eq!(notified.len(), 1);
    }

    #[test]
    fn test_no_change_same_value() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("s", "v");
        obs.subscribe("a1", "s", None);
        let notified = obs.update("s", "v");
        assert_eq!(notified.len(), 0);
    }

    #[test]
    fn test_multiple_subscribers() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("x", "0");
        obs.subscribe("a1", "x", None);
        obs.subscribe("a2", "x", None);
        let notified = obs.update("x", "1");
        assert_eq!(notified.len(), 2);
    }

    #[test]
    fn test_unsubscribe() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("s", "0");
        let sub_id = obs.subscribe("a1", "s", None).unwrap();
        obs.unsubscribe(&sub_id);
        let notified = obs.update("s", "1");
        assert_eq!(notified.len(), 0);
    }

    #[test]
    fn test_field_filter() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("s", "0");
        obs.subscribe("a1", "s", Some("value"));
        let notified = obs.update("s", "1");
        // field is "value" in the change, filter matches
        assert_eq!(notified.len(), 1);
    }

    #[test]
    fn test_dependency_tracking() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("a", "1");
        obs.create_signal("b", "2");
        obs.add_dependency("b", "a");
        let deps = obs.dependents_of("a");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_batch_mode() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("s", "0");
        obs.subscribe("a1", "s", None);
        obs.begin_batch();
        obs.update("s", "1"); // queued
        obs.update("s", "2"); // queued (overwrites 1)
        let notified = obs.end_batch();
        assert_eq!(notified.len(), 2); // 2 pending changes flushed
    }

    #[test]
    fn test_change_history() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("s", "0");
        obs.subscribe("a1", "s", None);
        obs.update("s", "1");
        obs.update("s", "2");
        let history = obs.history_for("s");
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_inactive_subscription() {
        let mut obs = ObserverSystem::new();
        obs.create_signal("s", "0");
        obs.subscribe("a1", "s", None);
        obs.subscriptions.iter_mut().next().unwrap().1.active = false;
        let notified = obs.update("s", "1");
        assert_eq!(notified.len(), 0);
    }

    #[test]
    fn test_summary() {
        let obs = ObserverSystem::new();
        let s = obs.summary();
        assert!(s.contains("0 signals"));
    }
}
