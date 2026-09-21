use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
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

/// Marcadores que identifican la raíz de un proyecto de software.
pub const PROJECT_MARKERS: &[&str] = &[
    ".git",
    "AGENTS.md",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
];

/// Comprueba si un directorio contiene algún marcador de proyecto.
pub fn is_project_root(path: &Path) -> bool {
    // Si es la carpeta del usuario (home), no considerarla raíz de proyecto
    // a menos que contenga un repositorio git explícito.
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        if path == Path::new(&home) {
            return path.join(".git").exists();
        }
    }
    PROJECT_MARKERS.iter().any(|marker| path.join(marker).exists())
}

/// Detecta si una ruta corresponde a la instalación de un IDE (origen común de CWD drift).
pub fn is_ide_install_dir(path: &Path) -> bool {
    let s = path.to_string_lossy().to_lowercase();
    s.contains("appdata\\local\\programs")
        || s.contains("appdata/local/programs")
        || s.contains("program files")
        || s.contains("antigravity ide")
        || s.ends_with("cursor")
        || s.ends_with("code")
}

/// Busca la raíz del proyecto recorriendo los ancestros desde `start_path`.
/// Se detiene antes de cruzar la raíz del sistema o la carpeta home del usuario.
pub fn find_project_root_from(start_path: &Path) -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from);

    for ancestor in start_path.ancestors() {
        if let Some(ref h) = home {
            if ancestor == h {
                if is_project_root(ancestor) {
                    return Some(ancestor.to_path_buf());
                }
                break;
            }
        }
        if is_project_root(ancestor) {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

pub fn workspace_uri_to_path(uri: &str) -> Option<PathBuf> {
    let decoded = urlencoding::decode(uri).ok()?;
    let stripped = decoded.strip_prefix("file://")?;
    let path_str = if cfg!(windows) {
        let p = stripped.trim_start_matches('/');
        if p.len() >= 2 && p.as_bytes()[1] == b':' {
            p.replace('/', "\\")
        } else {
            stripped.replace('/', "\\")
        }
    } else {
        stripped.to_string()
    };
    Some(PathBuf::from(path_str))
}

/// Resuelve la raíz del workspace a partir de los parámetros de MCP `initialize` o entorno.
pub fn resolve_workspace_from_params(params: Option<&Value>) -> Option<PathBuf> {
    let p = params?;
    // 1. workspaceFolders
    if let Some(folders) = p.get("workspaceFolders").and_then(Value::as_array) {
        if let Some(first) = folders.first() {
            if let Some(uri) = first.get("uri").and_then(Value::as_str) {
                if let Some(path) = workspace_uri_to_path(uri) {
                    return Some(find_project_root_from(&path).unwrap_or(path));
                }
            }
        }
    }
    // 2. rootUri
    if let Some(uri) = p.get("rootUri").and_then(Value::as_str) {
        if let Some(path) = workspace_uri_to_path(uri) {
            return Some(find_project_root_from(&path).unwrap_or(path));
        }
    }
    // 3. rootPath
    if let Some(path_str) = p.get("rootPath").and_then(Value::as_str) {
        let path = PathBuf::from(path_str);
        return Some(find_project_root_from(&path).unwrap_or(path));
    }
    // 4. initializationOptions (project_path o workspace_root)
    if let Some(opts) = p.get("initializationOptions") {
        for key in &["project_path", "workspace_root", "projectRoot", "root"] {
            if let Some(val) = opts.get(key).and_then(Value::as_str) {
                let path = if val.starts_with("file://") {
                    workspace_uri_to_path(val).unwrap_or_else(|| PathBuf::from(val))
                } else {
                    PathBuf::from(val)
                };
                return Some(find_project_root_from(&path).unwrap_or(path));
            }
        }
    }
    // 5. Variables de entorno comunes
    for var in &["WORKSPACE_ROOT", "INIT_CWD", "PROJECT_ROOT"] {
        if let Ok(env_path) = std::env::var(var) {
            if !env_path.trim().is_empty() {
                let path = PathBuf::from(env_path);
                return Some(find_project_root_from(&path).unwrap_or(path));
            }
        }
    }
    None
}

pub fn resolve_project_root(
    explicit_path: Option<String>,
    workspace_folders: Option<&Value>,
) -> Result<PathBuf, String> {
    if let Some(p) = explicit_path {
        let pb = PathBuf::from(&p);
        let can = pb
            .canonicalize()
            .map_err(|e| format!("Invalid project_path `{p}`: {e}"))?;
        return Ok(find_project_root_from(&can).unwrap_or(can));
    }
    if let Some(folders) = workspace_folders.and_then(Value::as_array) {
        if let Some(first) = folders.first() {
            if let Some(uri) = first.get("uri").and_then(Value::as_str) {
                if let Some(path) = workspace_uri_to_path(uri) {
                    return Ok(find_project_root_from(&path).unwrap_or(path));
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if is_ide_install_dir(&cwd) {
            for var in &["WORKSPACE_ROOT", "INIT_CWD", "PROJECT_ROOT"] {
                if let Ok(env_path) = std::env::var(var) {
                    let pb = PathBuf::from(env_path);
                    return Ok(find_project_root_from(&pb).unwrap_or(pb));
                }
            }
        }
        return Ok(find_project_root_from(&cwd).unwrap_or(cwd));
    }
    Err("could not determine workspace root directory".to_string())
}
