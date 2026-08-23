pub mod state;
pub mod formatters;
pub mod doctor;
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
pub mod dispatch;

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
    notifier.log(
        "info",
        "[ozymem-server] MCP server ready (petgraph + SQLite, per-project DB)".into(),
    );

    let mut stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_clone = stop_flag.clone();
    let writer_handle = tokio::spawn(async move {
        let mut rx = rx;
        let mut stdout = io::stdout();
        while !stop_flag_clone.load(Ordering::Relaxed) {
            match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
                Ok(Some(payload)) => {
                    let _ = stdout.write_all(payload.as_bytes()).await;
                    let _ = stdout.write_all(b"
").await;
                    let _ = stdout.flush().await;
                }
                Ok(None) => break,
                Err(_) => { /* timeout, loop and check flag */ }
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
            if let Some(response) =
                handle_request(&backend, request, Some(&notifier), Some(&subscribed)).await?
            {
                let payload = serde_json::to_string(&response)?;
                stdout.write_all(payload.as_bytes()).await?;
                stdout.write_all(b"
").await?;
                stdout.flush().await?;
            }
        } else {
            notifier.log(
                "error",
                format!("[ozymem-server] invalid JSON-RPC: {trimmed}"),
            );
        }
    }

    stop_flag.store(true, Ordering::Relaxed);
    let _ = writer_handle.await;

    Ok(())
}
