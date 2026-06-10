use std::collections::HashMap;
use std::sync::Mutex;

use tokio::task::AbortHandle;

static REGISTRY: Mutex<Option<HashMap<String, AbortHandle>>> = Mutex::new(None);

fn registry() -> std::sync::MutexGuard<'static, Option<HashMap<String, AbortHandle>>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Register an `AbortHandle` for `task_id`.
///
/// If a handle already exists for `task_id`, logs a warning and aborts the
/// displaced handle before inserting the new one.
pub fn register(task_id: &str, handle: AbortHandle) {
    let mut guard = registry();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(old) = map.insert(task_id.to_string(), handle) {
        log::warn!("[run_registry] overwrote live handle for task={task_id}; aborting old");
        old.abort();
    }
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
        log::debug!("[run_registry] deregistered task={task_id}");
    }
}

/// Return `true` if a handle for `task_id` is currently registered.
///
/// Note: returns `true` if the key is registered, which may include tasks that
/// have already finished if `deregister` was not called.
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

    static TEST_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn register_and_cancel_removes_handle() {
        let _g = TEST_GUARD.lock().unwrap();
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
        let _g = TEST_GUARD.lock().unwrap();
        assert!(!cancel("nonexistent-task"));
    }

    #[test]
    fn list_running_reflects_state() {
        let _g = TEST_GUARD.lock().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let abort = rt.block_on(async {
            let h = tokio::task::spawn(async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await
            });
            let a = h.abort_handle();
            a.abort();
            a
        });
        register("task-list-test", abort);
        assert!(list_running().contains(&"task-list-test".to_string()));
        cancel("task-list-test");
        assert!(!list_running().contains(&"task-list-test".to_string()));
    }
}
