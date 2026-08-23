use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use ozymem_core::mcp_common;

#[derive(Clone)]
pub struct Notifier {
    pub tx: mpsc::UnboundedSender<String>,
}

impl Notifier {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Notifier { tx }, rx)
    }

    pub fn log(&self, level: &str, message: String) {
        let notification = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/message",
            "params": {
                "level": level,
                "logger": "ozymem-server",
                "data": message
            }
        }))
        .unwrap_or_default();
        let _ = self.tx.send(notification);
    }

    pub fn progress(&self, progress_token: &Value, progress: u64, total: Option<u64>) {
        let mut params = json!({
            "progressToken": progress_token,
            "progress": progress,
        });
        if let Some(t) = total {
            params["total"] = json!(t);
        }
        let notification = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": params
        }))
        .unwrap_or_default();
        let _ = self.tx.send(notification);
    }

    pub fn raw(&self, method: &str, params: Value) {
        let notification = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .unwrap_or_default();
        let _ = self.tx.send(notification);
    }
}

pub fn log_spawn(level: &str, msg: String, notifier: &Option<Notifier>) {
    if let Some(n) = notifier {
        n.log(level, msg);
    }
}

pub fn notify_subscribed(
    subscribed: Option<&Arc<Mutex<HashSet<String>>>>,
    notifier: Option<&Notifier>,
    uris: &[&str],
) {
    if let Some(sub) = subscribed {
        let subs = sub.lock().unwrap();
        for uri in uris {
            if subs.contains(*uri) {
                if let Some(ref n) = notifier {
                    n.raw(
                        "notifications/methods/resources/updated",
                        json!({"uri": uri}),
                    );
                }
            }
        }
    }
}

pub fn ok_response(id: Value, result: Value) -> mcp_common::JsonRpcResponse {
    mcp_common::JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

pub fn error_response(id: Value, code: i64, message: &str) -> mcp_common::JsonRpcResponse {
    mcp_common::JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(mcp_common::JsonRpcError {
            code,
            message: message.to_string(),
            data: None,
        }),
    }
}

pub fn workspace_uri_to_path(uri: &str) -> Option<PathBuf> {
    let stripped = uri.strip_prefix("file://")?;
    let path_str = if cfg!(windows) {
        let p = stripped.trim_start_matches('/');
        if p.len() >= 2 && p.as_bytes()[1] == b':' {
            p.to_string()
        } else {
            stripped.to_string()
        }
    } else {
        stripped.to_string()
    };
    let decoded = urlencoding::decode(&path_str).ok()?;
    Some(PathBuf::from(decoded.into_owned()))
}

pub fn resolve_project_root(
    explicit_path: Option<String>,
    workspace_folders: Option<&Value>,
) -> Result<PathBuf, String> {
    if let Some(p) = explicit_path {
        let pb = PathBuf::from(&p);
        return pb
            .canonicalize()
            .map_err(|e| format!("Invalid project_path `{p}`: {e}"));
    }
    if let Some(folders) = workspace_folders.and_then(Value::as_array) {
        if let Some(first) = folders.first() {
            if let Some(uri) = first.get("uri").and_then(Value::as_str) {
                if let Some(path) = workspace_uri_to_path(uri) {
                    return Ok(path);
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        return Ok(cwd);
    }
    Err("could not determine workspace root directory".to_string())
}
