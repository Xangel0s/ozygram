pub mod state;
pub mod formatters;
pub mod doctor;
pub mod sql_linter;
pub mod skills;
pub mod brain;
pub mod projects;
pub mod packages;
pub mod git;
pub mod schemas;
pub mod resources;
pub mod prompts;
pub mod memory;
pub mod graph;
pub mod unified;
pub mod tools;
pub mod verifier;
pub mod exploration;
pub mod dispatch;
pub mod symbols;

use ozymem_core::graph_backend::GraphBackend;
use ozymem_core::mcp_common;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

pub use dispatch::handle_request;
pub use state::Notifier;

pub async fn run_mcp_server() -> anyhow::Result<()> {
    let backend: Arc<Mutex<Option<GraphBackend>>> = Arc::new(Mutex::new(None));
    let (notifier, rx) = Notifier::new();
    let subscribed: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    let mut stdin = BufReader::new(io::stdin());

    let (stdout_tx, mut stdout_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let stdout_tx_notifier = stdout_tx.clone();

    let writer_handle = tokio::spawn(async move {
        let mut stdout = io::stdout();
        while let Some(msg) = stdout_rx.recv().await {
            let _ = stdout.write_all(msg.as_bytes()).await;
            let _ = stdout.write_all(b"\n").await;
            let _ = stdout.flush().await;
        }
    });

    let stop_notifier = Arc::new(AtomicBool::new(false));
    let stop_notifier_clone = stop_notifier.clone();
    let notifier_handle = tokio::spawn(async move {
        let mut rx = rx;
        while !stop_notifier_clone.load(Ordering::Relaxed) {
            match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
                Ok(Some(payload)) => {
                    let _ = stdout_tx_notifier.send(payload);
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
    });

    let mut line = String::new();

    while {
        line.clear();
        stdin.read_line(&mut line).await? > 0
    } {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(request) = serde_json::from_str::<mcp_common::JsonRpcRequest>(trimmed) {
            let req_id = request.id.clone();
            match handle_request(&backend, request, Some(&notifier), Some(&subscribed)).await {
                Ok(Some(response)) => {
                    if let Ok(payload) = serde_json::to_string(&response) {
                        let _ = stdout_tx.send(payload);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    if let Some(id) = req_id {
                        let err_resp = state::error_response(id, -32603, &e.to_string());
                        if let Ok(payload) = serde_json::to_string(&err_resp) {
                            let _ = stdout_tx.send(payload);
                        }
                    }
                }
            }
        } else {
            eprintln!("[ozymem-server] invalid JSON-RPC: {trimmed}");
        }
    }

    stop_notifier.store(true, Ordering::Relaxed);
    let _ = notifier_handle.await;
    drop(stdout_tx);
    let _ = writer_handle.await;

    Ok(())
}
