use crate::graph_backend::is_noise_dir;
use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use notify::RecommendedWatcher;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// Default maximum file size for hot delta indexing (256 KB).
pub const DEFAULT_MAX_DELTA_FILE_BYTES: u64 = 256 * 1024;

/// Result of indexing a single file delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeltaIndexResult {
    Indexed { symbols: usize, dependencies: usize },
    Unchanged,
    SkippedNoise,
    SkippedTooLarge { size_bytes: u64 },
    SkippedBinary,
    Deleted,
}

/// Filter decision for delta indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseFilterDecision {
    Process,
    NoiseDir,
    MinifiedOrBundle,
    LockFile,
    LargeOrBinaryData,
    ExceedsSizeLimit(u64),
}

/// Checks if a file path is noise, a lockfile, minified, or exceeds the size limit.
pub fn check_noise_or_huge_file(path: &Path, max_bytes: u64) -> NoiseFilterDecision {
    // 1. Check parent directories for noise dirs
    for ancestor in path.ancestors() {
        if is_noise_dir(ancestor) {
            return NoiseFilterDecision::NoiseDir;
        }
    }

    let file_name = match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.to_lowercase(),
        None => return NoiseFilterDecision::NoiseDir,
    };

    // 2. Lock files
    if matches!(
        file_name.as_str(),
        "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "cargo.lock"
            | "poetry.lock"
            | "composer.lock"
            | "gemfile.lock"
    ) {
        return NoiseFilterDecision::LockFile;
    }

    // 3. Minified or bundled files
    if file_name.ends_with(".min.js")
        || file_name.ends_with(".min.css")
        || file_name.ends_with(".bundle.js")
        || file_name.ends_with(".chunk.js")
        || file_name.ends_with(".map")
    {
        return NoiseFilterDecision::MinifiedOrBundle;
    }

    // 4. Heavy data, logs or compiled binaries
    if file_name.ends_with(".log")
        || file_name.ends_with(".sqlite")
        || file_name.ends_with(".sqlite3")
        || file_name.ends_with(".db")
        || file_name.ends_with(".csv")
        || file_name.ends_with(".tsv")
        || file_name.ends_with(".parquet")
        || file_name.ends_with(".dump")
        || file_name.ends_with(".bin")
        || file_name.ends_with(".exe")
        || file_name.ends_with(".dll")
        || file_name.ends_with(".so")
        || file_name.ends_with(".dylib")
        || file_name.ends_with(".wasm")
    {
        return NoiseFilterDecision::LargeOrBinaryData;
    }

    // 5. File size check if the file exists on disk
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.is_file() && metadata.len() > max_bytes {
            return NoiseFilterDecision::ExceedsSizeLimit(metadata.len());
        }
    }

    NoiseFilterDecision::Process
}

/// Convenience boolean check.
pub fn is_noise_or_huge_file(path: &Path, max_bytes: u64) -> bool {
    check_noise_or_huge_file(path, max_bytes) != NoiseFilterDecision::Process
}

/// Events emitted by the live file watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeltaFileEvent {
    Modified(PathBuf),
    Removed(PathBuf),
}

/// Live filesystem watcher with automatic debouncing.
pub struct LiveWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
    rx: Receiver<DeltaFileEvent>,
}

impl LiveWatcher {
    /// Starts watching a root directory with a default 300ms debouncing window.
    pub fn watch(root_path: &Path, debounce_duration: Option<Duration>) -> Result<Self> {
        let (tx, rx): (Sender<DeltaFileEvent>, Receiver<DeltaFileEvent>) = channel();
        let timeout = debounce_duration.unwrap_or_else(|| Duration::from_millis(300));

        let mut debouncer = new_debouncer(timeout, move |res: DebounceEventResult| {
            if let Ok(events) = res {
                for event in events {
                    let path = event.path;
                    if path.exists() {
                        let _ = tx.send(DeltaFileEvent::Modified(path));
                    } else {
                        let _ = tx.send(DeltaFileEvent::Removed(path));
                    }
                }
            }
        })
        .context("failed to initialize live filesystem debouncer")?;

        debouncer
            .watcher()
            .watch(root_path, notify::RecursiveMode::Recursive)
            .with_context(|| format!("failed to watch directory: {}", root_path.display()))?;

        Ok(Self {
            _debouncer: debouncer,
            rx,
        })
    }

    /// Try to receive the next delta file event without blocking.
    pub fn try_recv(&self) -> Option<DeltaFileEvent> {
        self.rx.try_recv().ok()
    }

    /// Receive the next delta file event with a timeout.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<DeltaFileEvent> {
        self.rx.recv_timeout(timeout).ok()
    }
}

/// Reads a file from disk with exponential backoff retries to mitigate Windows file locks (EBUSY / ERROR_SHARING_VIOLATION).
pub fn read_file_with_backoff(path: &Path, max_retries: usize) -> std::io::Result<String> {
    let mut delay = Duration::from_millis(5);
    for attempt in 0..max_retries {
        match std::fs::read_to_string(path) {
            Ok(content) => return Ok(content),
            Err(_) if attempt + 1 < max_retries => {
                std::thread::sleep(delay);
                delay *= 2;
            }
            Err(e) => return Err(e),
        }
    }
    std::fs::read_to_string(path)
}
