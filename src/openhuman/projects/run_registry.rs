use std::collections::HashMap;
use std::sync::Mutex;

use tokio::task::AbortHandle;

static REGISTRY: Mutex<Option<HashMap<String, AbortHandle>>> = Mutex::new(None);

fn registry() -> std::sync::MutexGuard<'static, Option<HashMap<String, AbortHandle>>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Register an `AbortHandle` for `task_id`. Overwrites any previous entry.
pub fn register(task_id: &str, handle: AbortHandle) {
    let mut guard = registry();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(task_id.to_string(), handle);
    log::debug!("[run_registry] registered task={task_id}");
}

/// Abort the task and remove its entry. Returns `true` if a handle was found.
pub fn cancel(task_id: &str) -> bool {
    let mut guard = registry();
    if let Some(map) = guard.as_mut() {
        if let Some(handle) = map.remove(task_id) {
            handle.abort();
            log::debug!("[run_registry] cancelled task={task_id}");
            return true;
        }
    }
    false
}

/// Remove a finished task's entry without aborting.
pub fn deregister(task_id: &str) {
    let mut guard = registry();
    if let Some(map) = guard.as_mut() {
        map.remove(task_id);
    }
}

/// Return `true` if a handle for `task_id` is currently registered.
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
    use tokio::task;

    #[tokio::test]
    async fn register_and_cancel_removes_handle() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let handle = task::spawn(async move {
            let _ = rx.await;
        });
        let abort = handle.abort_handle();
        register("task-1", abort);
        assert!(is_running("task-1"));
        let found = cancel("task-1");
        assert!(found);
        assert!(!is_running("task-1"));
        let _ = tx.send(());
    }

    #[test]
    fn cancel_unknown_returns_false() {
        assert!(!cancel("nonexistent-task"));
    }
}
