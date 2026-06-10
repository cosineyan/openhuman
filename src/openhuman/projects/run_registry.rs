use std::collections::HashMap;
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

static REGISTRY: Mutex<Option<HashMap<String, CancellationToken>>> = Mutex::new(None);

fn registry() -> std::sync::MutexGuard<'static, Option<HashMap<String, CancellationToken>>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Register a `CancellationToken` for `task_id`. Overwrites any previous entry.
///
/// If a token already exists for `task_id`, logs a warning and cancels the
/// displaced token before inserting the new one.
pub fn register(task_id: &str, token: CancellationToken) {
    let mut guard = registry();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(old) = map.insert(task_id.to_string(), token) {
        log::warn!("[run_registry] overwrote live token for task={task_id}; cancelling old");
        old.cancel();
    }
    log::debug!("[run_registry] registered task={task_id}");
}

/// Cancel the task's token and remove its entry. Returns `true` if a token was found.
pub fn cancel(task_id: &str) -> bool {
    let mut guard = registry();
    if let Some(map) = guard.as_mut() {
        if let Some(token) = map.remove(task_id) {
            token.cancel();
            log::debug!("[run_registry] cancelled task={task_id}");
            return true;
        }
    }
    false
}

/// Remove a finished task's entry without cancelling.
pub fn deregister(task_id: &str) {
    let mut guard = registry();
    if let Some(map) = guard.as_mut() {
        map.remove(task_id);
        log::debug!("[run_registry] deregistered task={task_id}");
    }
}

/// Return `true` if a token for `task_id` is currently registered.
///
/// Note: may return `true` for tasks that have completed if `deregister` was not called.
pub fn is_running(task_id: &str) -> bool {
    let guard = registry();
    guard
        .as_ref()
        .map(|m| m.contains_key(task_id))
        .unwrap_or(false)
}

/// Return all currently-registered task IDs.
pub fn list_running() -> Vec<String> {
    let guard = registry();
    guard
        .as_ref()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn register_and_cancel_removes_handle() {
        let _g = TEST_GUARD.lock().unwrap();
        let token = CancellationToken::new();
        register("task-1", token.clone());
        assert!(is_running("task-1"));
        let found = cancel("task-1");
        assert!(found);
        assert!(token.is_cancelled());
        assert!(!is_running("task-1"));
    }

    #[test]
    fn cancel_unknown_returns_false() {
        let _g = TEST_GUARD.lock().unwrap();
        assert!(!cancel("nonexistent-task"));
    }

    #[test]
    fn list_running_reflects_state() {
        let _g = TEST_GUARD.lock().unwrap();
        let token = CancellationToken::new();
        register("task-list-test", token);
        assert!(list_running().contains(&"task-list-test".to_string()));
        cancel("task-list-test");
        assert!(!list_running().contains(&"task-list-test".to_string()));
    }
}
