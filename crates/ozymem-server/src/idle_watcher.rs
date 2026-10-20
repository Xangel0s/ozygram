use ozymem_core::graph_backend::GraphBackend;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Global epoch seconds of the last recorded MCP tool or resource interaction.
pub static LAST_MCP_ACTIVITY_EPOCH_SECS: AtomicU64 = AtomicU64::new(0);

/// Flag indicating if an offline dreaming cycle is currently active in the background.
pub static DREAM_CYCLE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Edge-triggered latch: true once dreaming finishes for the current idle stretch,
/// preventing repeated re-execution until new human/MCP activity occurs.
pub static DREAM_COMPLETED_FOR_CURRENT_IDLE: AtomicBool = AtomicBool::new(false);

/// Helper returning current system time in epoch seconds.
pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Updates activity timestamp and resets idle completion latch.
/// Must be invoked on every incoming MCP request.
pub fn touch_activity() {
    LAST_MCP_ACTIVITY_EPOCH_SECS.store(now_epoch_secs(), Ordering::Relaxed);
    DREAM_COMPLETED_FOR_CURRENT_IDLE.store(false, Ordering::Relaxed);
}

/// Returns the elapsed idle duration in seconds since the last MCP activity.
pub fn get_idle_duration_secs() -> u64 {
    let last = LAST_MCP_ACTIVITY_EPOCH_SECS.load(Ordering::Relaxed);
    let now = now_epoch_secs();
    if last == 0 || now <= last {
        0
    } else {
        now - last
    }
}

/// Resolves the configured idle timeout threshold in seconds.
/// Default: 1800 seconds (30 minutes). Can be overridden via OZYMEM_IDLE_TIMEOUT_SECS.
pub fn resolve_idle_timeout_secs() -> u64 {
    std::env::var("OZYMEM_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(1800)
}

/// Spawns the low-overhead background inactivity daemon.
/// Runs at zero CPU overhead during normal agent coding sessions.
pub fn start_idle_watcher(backend: Arc<Mutex<Option<GraphBackend>>>) -> tokio::task::JoinHandle<()> {
    touch_activity();

    tokio::spawn(async move {
        let timeout_secs = resolve_idle_timeout_secs();
        let poll_interval = if timeout_secs <= 5 {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(15)
        };

        loop {
            tokio::time::sleep(poll_interval).await;

            let timeout_secs = resolve_idle_timeout_secs();
            let idle_duration = get_idle_duration_secs();

            if idle_duration >= timeout_secs
                && !DREAM_COMPLETED_FOR_CURRENT_IDLE.load(Ordering::Relaxed)
                && !DREAM_CYCLE_RUNNING.swap(true, Ordering::SeqCst)
            {
                let project_path_opt = {
                    let guard = backend.lock().unwrap();
                    guard.as_ref().and_then(|gb| gb.project_path())
                };

                if let Some(project_path) = project_path_opt {
                    eprintln!(
                        "[STATUS: IDLE_WATCHER] Inactivity threshold reached ({}s >= {}s). Triggering offline dream cycle...",
                        idle_duration, timeout_secs
                    );

                    let payload = json!({
                        "workspace_root": project_path,
                        "project": project_path,
                        "tenant_id": "local",
                        "max_commits": 100
                    });

                    // Execute dream cycle worker in blocking thread pool
                    let worker_res = tokio::task::spawn_blocking(move || {
                        crate::brain::call_ozy_brain_worker("dream_offline_cycle", &payload, 180_000)
                    })
                    .await;

                    match worker_res {
                        Ok(Ok(response)) => {
                            let duration_ms = response.get("duration_ms").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            eprintln!(
                                "[STATUS: OFFLINE_DREAM_CYCLE_COMPLETE] Idle consolidation succeeded in {:.2}ms.",
                                duration_ms
                            );
                        }
                        Ok(Err(e)) => {
                            eprintln!("[WARN: OFFLINE_DREAM_CYCLE_ERROR] Dream worker execution returned error: {}", e);
                        }
                        Err(join_err) => {
                            eprintln!("[ERROR: OFFLINE_DREAM_JOIN_ERROR] Background task panic or cancel: {}", join_err);
                        }
                    }

                    // Mark as completed for this idle stretch so we don't repeat every poll interval
                    DREAM_COMPLETED_FOR_CURRENT_IDLE.store(true, Ordering::Relaxed);
                }

                DREAM_CYCLE_RUNNING.store(false, Ordering::SeqCst);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_touch_activity_and_idle_duration() {
        touch_activity();
        assert_eq!(DREAM_COMPLETED_FOR_CURRENT_IDLE.load(Ordering::Relaxed), false);
        assert_eq!(DREAM_CYCLE_RUNNING.load(Ordering::Relaxed), false);

        let idle = get_idle_duration_secs();
        assert!(idle <= 2, "Freshly touched activity must report near zero idle seconds");
    }

    #[test]
    fn test_edge_triggered_idle_latch() {
        touch_activity();
        assert_eq!(DREAM_COMPLETED_FOR_CURRENT_IDLE.load(Ordering::Relaxed), false);

        // Simulate cycle completion
        DREAM_COMPLETED_FOR_CURRENT_IDLE.store(true, Ordering::Relaxed);
        assert_eq!(DREAM_COMPLETED_FOR_CURRENT_IDLE.load(Ordering::Relaxed), true);

        // New activity must reset latch
        touch_activity();
        assert_eq!(DREAM_COMPLETED_FOR_CURRENT_IDLE.load(Ordering::Relaxed), false);
    }
}
