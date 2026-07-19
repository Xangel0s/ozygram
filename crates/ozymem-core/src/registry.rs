// registry.rs — SQLite Control Plane for OzyMem project lifecycle management.
//
// Replaces:
//   - ~/.ozymem.toml [projects] section → `projects` table
//   - ~/.ozymem-{name}.pid files        → `watcher_pid` column
//   - .ozymem_wal files                 → `pending_syncs` table
//
// Location: ~/.ozymem/registry.db

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Directory name under user home for all OzyMem state.
const OZYMEM_DIR: &str = ".ozymem";
/// SQLite database filename.
const REGISTRY_DB: &str = "registry.db";

// ---------------------------------------------------------------------------
// Domain Types
// ---------------------------------------------------------------------------

/// Scale classification for resource profiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scale {
    Unknown,
    Small,
    Medium,
    Large,
    Enterprise,
}

impl Scale {
    /// Classify a project by its file count (excluding noise directories).
    pub fn from_file_count(count: i64) -> Self {
        match count {
            0..=99 => Scale::Small,
            100..=999 => Scale::Medium,
            1000..=9999 => Scale::Large,
            _ => Scale::Enterprise,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Scale::Unknown => "UNKNOWN",
            Scale::Small => "SMALL",
            Scale::Medium => "MEDIUM",
            Scale::Large => "LARGE",
            Scale::Enterprise => "ENTERPRISE",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "SMALL" => Scale::Small,
            "MEDIUM" => Scale::Medium,
            "LARGE" => Scale::Large,
            "ENTERPRISE" => Scale::Enterprise,
            _ => Scale::Unknown,
        }
    }

    /// Returns an emoji indicator for display.
    pub fn icon(&self) -> &'static str {
        match self {
            Scale::Unknown => "❓",
            Scale::Small => "📄",
            Scale::Medium => "📁",
            Scale::Large => "🏗️",
            Scale::Enterprise => "🏢",
        }
    }
}

/// Project status in the lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectStatus {
    Active,
    Sleeping,
    Scanning,
}

impl ProjectStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectStatus::Active => "ACTIVE",
            ProjectStatus::Sleeping => "SLEEPING",
            ProjectStatus::Scanning => "SCANNING",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "ACTIVE" => ProjectStatus::Active,
            "SCANNING" => ProjectStatus::Scanning,
            _ => ProjectStatus::Sleeping,
        }
    }

    /// Returns an emoji indicator for display.
    pub fn icon(&self) -> &'static str {
        match self {
            ProjectStatus::Active => "🟢",
            ProjectStatus::Sleeping => "💤",
            ProjectStatus::Scanning => "🔍",
        }
    }
}

/// A registered project in the SQLite registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub status: ProjectStatus,
    pub scale: Scale,
    pub file_count: i64,
    pub vector_count: i64,
    pub growth_rate: f64,
    pub watcher_pid: Option<u32>,
    pub last_opened: Option<String>,
    pub last_scan: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A pending sync operation (replaces .ozymem_wal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSync {
    pub id: i64,
    pub project_id: i64,
    pub action: String,
    pub file_path: String,
    pub queued_at: String,
    pub synced_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Legacy TOML structures for migration
// ---------------------------------------------------------------------------

/// Minimal representation of the legacy ~/.ozymem.toml for migration.
#[derive(Debug, Deserialize)]
struct LegacyConfig {
    #[serde(default)]
    projects: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// ProjectRegistry — the main API
// ---------------------------------------------------------------------------

/// Central SQLite-backed registry for managing OzyMem project lifecycle.
///
/// Provides CRUD operations, lifecycle transitions (wake/sleep), a sync queue
/// that replaces `.ozymem_wal`, auto-discovery via project markers, scale-based
/// resource classification, and garbage collection for stale/orphan projects.
pub struct ProjectRegistry {
    db: Connection,
}

impl ProjectRegistry {
    /// Opens (or creates) the registry database at `~/.ozymem/registry.db`.
    /// Automatically runs migrations to ensure the schema is up to date.
    pub fn open() -> Result<Self> {
        let db_path = Self::db_path()?;

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        let db = Connection::open(&db_path)
            .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

        // Enable WAL mode for concurrent read performance
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;

        let registry = Self { db };
        registry.migrate()?;
        Ok(registry)
    }

    /// Returns the absolute path to the registry database file.
    pub fn db_path() -> Result<PathBuf> {
        let home = home::home_dir()
            .context("Could not determine home directory")?;
        Ok(home.join(OZYMEM_DIR).join(REGISTRY_DB))
    }

    /// Returns the base directory for OzyMem state (`~/.ozymem/`).
    pub fn base_dir() -> Result<PathBuf> {
        let home = home::home_dir()
            .context("Could not determine home directory")?;
        Ok(home.join(OZYMEM_DIR))
    }

    // -----------------------------------------------------------------------
    // Schema Migration
    // -----------------------------------------------------------------------

    /// Creates all required tables if they don't exist.
    fn migrate(&self) -> Result<()> {
        self.db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS projects (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                name         TEXT    NOT NULL UNIQUE,
                path         TEXT    NOT NULL UNIQUE,
                status       TEXT    NOT NULL DEFAULT 'SLEEPING',
                scale        TEXT    NOT NULL DEFAULT 'UNKNOWN',
                file_count   INTEGER NOT NULL DEFAULT 0,
                vector_count INTEGER NOT NULL DEFAULT 0,
                growth_rate  REAL    NOT NULL DEFAULT 0.0,
                watcher_pid  INTEGER,
                last_opened  TEXT,
                last_scan    TEXT,
                created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
                updated_at   TEXT    NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS pending_syncs (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                action     TEXT    NOT NULL,
                file_path  TEXT    NOT NULL,
                queued_at  TEXT    NOT NULL DEFAULT (datetime('now')),
                synced_at  TEXT
            );

            CREATE TABLE IF NOT EXISTS config (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Default configuration values (INSERT OR IGNORE to avoid overwriting)
            INSERT OR IGNORE INTO config (key, value) VALUES ('mode', 'fluid');
            INSERT OR IGNORE INTO config (key, value) VALUES ('sleep_timeout_hours', '4');
            INSERT OR IGNORE INTO config (key, value) VALUES ('purge_after_days', '90');
            ",
        )?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Migration from legacy ~/.ozymem.toml
    // -----------------------------------------------------------------------

    /// Imports projects from the legacy `~/.ozymem.toml` file into SQLite.
    /// Returns the number of projects successfully imported.
    /// Safe to call multiple times — uses INSERT OR IGNORE.
    pub fn import_from_toml(&self) -> Result<usize> {
        let home = home::home_dir()
            .context("Could not determine home directory")?;
        let toml_path = home.join(".ozymem.toml");

        if !toml_path.exists() {
            return Ok(0);
        }

        let content = std::fs::read_to_string(&toml_path)
            .with_context(|| format!("Failed to read {}", toml_path.display()))?;

        let config: LegacyConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", toml_path.display()))?;

        let mut imported = 0;
        for (name, path) in &config.projects {
            let result = self.db.execute(
                "INSERT OR IGNORE INTO projects (name, path, status) VALUES (?1, ?2, 'SLEEPING')",
                params![name, path],
            );
            if let Ok(changes) = result {
                if changes > 0 {
                    imported += 1;
                }
            }
        }

        Ok(imported)
    }

    // -----------------------------------------------------------------------
    // CRUD Operations
    // -----------------------------------------------------------------------

    /// Registers a new project. Returns the created Project.
    /// If a project with the same name or path already exists, returns an error.
    pub fn register(&self, name: &str, path: &str) -> Result<Project> {
        self.db.execute(
            "INSERT INTO projects (name, path, status, last_opened, updated_at) \
             VALUES (?1, ?2, 'SLEEPING', datetime('now'), datetime('now'))",
            params![name, path],
        )?;

        self.get_project_by_name(name)?
            .context("Project was inserted but could not be retrieved")
    }

    /// Removes a project registration. Returns true if a project was removed.
    pub fn deregister(&self, name: &str) -> Result<bool> {
        let changes = self.db.execute(
            "DELETE FROM projects WHERE name = ?1",
            params![name],
        )?;
        Ok(changes > 0)
    }

    /// Finds a project by its filesystem path (case-insensitive on Windows).
    pub fn get_project_by_path(&self, path: &str) -> Result<Option<Project>> {
        let normalized = normalize_path(path);
        self.db
            .query_row(
                "SELECT id, name, path, status, scale, file_count, vector_count, \
                 growth_rate, watcher_pid, last_opened, last_scan, created_at, updated_at \
                 FROM projects WHERE LOWER(path) = LOWER(?1)",
                params![normalized],
                |row| row_to_project(row),
            )
            .optional()
            .context("Failed to query project by path")
    }

    /// Finds a project by its registered name.
    pub fn get_project_by_name(&self, name: &str) -> Result<Option<Project>> {
        self.db
            .query_row(
                "SELECT id, name, path, status, scale, file_count, vector_count, \
                 growth_rate, watcher_pid, last_opened, last_scan, created_at, updated_at \
                 FROM projects WHERE name = ?1",
                params![name],
                |row| row_to_project(row),
            )
            .optional()
            .context("Failed to query project by name")
    }

    /// Lists all registered projects, ordered by name.
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, path, status, scale, file_count, vector_count, \
             growth_rate, watcher_pid, last_opened, last_scan, created_at, updated_at \
             FROM projects ORDER BY name",
        )?;

        let projects = stmt
            .query_map([], |row| row_to_project(row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    // -----------------------------------------------------------------------
    // Lifecycle Transitions
    // -----------------------------------------------------------------------

    /// Wakes a project: sets status to ACTIVE, records PID and last_opened timestamp.
    pub fn wake_project(&self, name: &str, pid: u32) -> Result<()> {
        let changes = self.db.execute(
            "UPDATE projects SET status = 'ACTIVE', watcher_pid = ?1, \
             last_opened = datetime('now'), updated_at = datetime('now') \
             WHERE name = ?2",
            params![pid, name],
        )?;

        if changes == 0 {
            anyhow::bail!("Project '{}' not found in registry", name);
        }
        Ok(())
    }

    /// Puts a project to sleep: sets status to SLEEPING, clears PID.
    pub fn sleep_project(&self, name: &str) -> Result<()> {
        let changes = self.db.execute(
            "UPDATE projects SET status = 'SLEEPING', watcher_pid = NULL, \
             updated_at = datetime('now') WHERE name = ?1",
            params![name],
        )?;

        if changes == 0 {
            anyhow::bail!("Project '{}' not found in registry", name);
        }
        Ok(())
    }

    /// Updates the scale classification and file count for a project.
    pub fn update_scale(&self, name: &str, scale: Scale, file_count: i64) -> Result<()> {
        self.db.execute(
            "UPDATE projects SET scale = ?1, file_count = ?2, updated_at = datetime('now') \
             WHERE name = ?3",
            params![scale.as_str(), file_count, name],
        )?;
        Ok(())
    }

    /// Updates file and vector counts for a project.
    pub fn update_stats(&self, name: &str, file_count: i64, vector_count: i64) -> Result<()> {
        self.db.execute(
            "UPDATE projects SET file_count = ?1, vector_count = ?2, updated_at = datetime('now') \
             WHERE name = ?3",
            params![file_count, vector_count, name],
        )?;
        Ok(())
    }

    /// Records the completion of a full scan for a project.
    pub fn mark_scanned(&self, name: &str) -> Result<()> {
        self.db.execute(
            "UPDATE projects SET last_scan = datetime('now'), updated_at = datetime('now') \
             WHERE name = ?1",
            params![name],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Sync Queue (replaces .ozymem_wal)
    // -----------------------------------------------------------------------

    /// Enqueues a file change for later synchronization (e.g., embedding regeneration).
    pub fn enqueue_sync(&self, project_id: i64, action: &str, file_path: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO pending_syncs (project_id, action, file_path) VALUES (?1, ?2, ?3)",
            params![project_id, action, file_path],
        )?;
        Ok(())
    }

    /// Dequeues a batch of unprocessed sync entries for a project.
    /// Returns up to `batch_size` entries ordered chronologically.
    pub fn dequeue_syncs(&self, project_id: i64, batch_size: usize) -> Result<Vec<PendingSync>> {
        let mut stmt = self.db.prepare(
            "SELECT id, project_id, action, file_path, queued_at, synced_at \
             FROM pending_syncs \
             WHERE project_id = ?1 AND synced_at IS NULL \
             ORDER BY queued_at ASC \
             LIMIT ?2",
        )?;

        let syncs = stmt
            .query_map(params![project_id, batch_size as i64], |row| {
                Ok(PendingSync {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    action: row.get(2)?,
                    file_path: row.get(3)?,
                    queued_at: row.get(4)?,
                    synced_at: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(syncs)
    }

    /// Marks a sync entry as completed.
    pub fn mark_synced(&self, sync_id: i64) -> Result<()> {
        self.db.execute(
            "UPDATE pending_syncs SET synced_at = datetime('now') WHERE id = ?1",
            params![sync_id],
        )?;
        Ok(())
    }

    /// Returns the count of pending (unsynced) entries for a project.
    pub fn pending_sync_count(&self, project_id: i64) -> Result<i64> {
        self.db
            .query_row(
                "SELECT COUNT(*) FROM pending_syncs WHERE project_id = ?1 AND synced_at IS NULL",
                params![project_id],
                |row| row.get(0),
            )
            .context("Failed to count pending syncs")
    }

    /// Removes all completed (synced) entries older than the given number of days.
    pub fn purge_completed_syncs(&self, older_than_days: i64) -> Result<i64> {
        let changes = self.db.execute(
            "DELETE FROM pending_syncs \
             WHERE synced_at IS NOT NULL \
             AND synced_at < datetime('now', ?1)",
            params![format!("-{} days", older_than_days)],
        )?;
        Ok(changes as i64)
    }

    // -----------------------------------------------------------------------
    // Auto-Discovery
    // -----------------------------------------------------------------------

    /// Discovers a project name and root path by walking up from the given directory.
    /// Looks for project markers: `.git`, `Cargo.toml`, `package.json`, `go.mod`,
    /// `pyproject.toml`, `setup.py`, `.ozymem`.
    ///
    /// Returns `(inferred_name, root_path)` or `None` if no marker is found.
    pub fn discover_project(start_path: &Path) -> Option<(String, PathBuf)> {
        let markers = [
            ".git",
            "Cargo.toml",
            "package.json",
            "go.mod",
            "pyproject.toml",
            "setup.py",
            ".ozymem",
        ];

        let mut current = if start_path.is_file() {
            start_path.parent()?.to_path_buf()
        } else {
            start_path.to_path_buf()
        };

        loop {
            for marker in &markers {
                if current.join(marker).exists() {
                    let name = current
                        .file_name()?
                        .to_str()?
                        .to_string();
                    return Some((name, current));
                }
            }

            if !current.pop() {
                break;
            }

            // Safety: don't go above 2 components (e.g. `C:\Users`)
            if current.components().count() <= 2 {
                break;
            }
        }

        None
    }

    /// Registers a project via auto-discovery if it's not already registered.
    /// Returns the project (existing or newly created).
    pub fn ensure_project(&self, path: &str) -> Result<Project> {
        // Check if already registered
        if let Some(project) = self.get_project_by_path(path)? {
            return Ok(project);
        }

        // Try auto-discovery
        let abs_path = std::fs::canonicalize(path)
            .unwrap_or_else(|_| PathBuf::from(path));

        if let Some((name, root)) = Self::discover_project(&abs_path) {
            let root_str = normalize_path(&root.to_string_lossy());

            // Check if root is already registered (path might differ from input)
            if let Some(project) = self.get_project_by_path(&root_str)? {
                return Ok(project);
            }

            // Ensure unique name
            let final_name = self.unique_name(&name)?;
            return self.register(&final_name, &root_str);
        }

        // Fallback: use the directory itself as the project
        let name = abs_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unnamed")
            .to_string();

        let final_name = self.unique_name(&name)?;
        self.register(&final_name, &normalize_path(&abs_path.to_string_lossy()))
    }

    /// Generates a unique project name by appending a numeric suffix if needed.
    fn unique_name(&self, base_name: &str) -> Result<String> {
        if self.get_project_by_name(base_name)?.is_none() {
            return Ok(base_name.to_string());
        }

        for i in 2..=100 {
            let candidate = format!("{}-{}", base_name, i);
            if self.get_project_by_name(&candidate)?.is_none() {
                return Ok(candidate);
            }
        }

        anyhow::bail!("Could not generate a unique name for project '{}'", base_name);
    }

    // -----------------------------------------------------------------------
    // Garbage Collection
    // -----------------------------------------------------------------------

    /// Returns projects that have been SLEEPING for more than `days` days.
    pub fn get_stale_projects(&self, days: i64) -> Result<Vec<Project>> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, path, status, scale, file_count, vector_count, \
             growth_rate, watcher_pid, last_opened, last_scan, created_at, updated_at \
             FROM projects \
             WHERE status = 'SLEEPING' \
             AND updated_at < datetime('now', ?1) \
             ORDER BY updated_at ASC",
        )?;

        let cutoff = format!("-{} days", days);
        let projects = stmt
            .query_map(params![cutoff], |row| row_to_project(row))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    /// Returns projects whose registered path no longer exists on disk.
    pub fn get_orphan_projects(&self) -> Result<Vec<Project>> {
        let all = self.list_projects()?;
        Ok(all
            .into_iter()
            .filter(|p| !Path::new(&p.path).exists())
            .collect())
    }

    // -----------------------------------------------------------------------
    // Configuration
    // -----------------------------------------------------------------------

    /// Reads a configuration value from the `config` table.
    pub fn get_config(&self, key: &str) -> Result<Option<String>> {
        self.db
            .query_row(
                "SELECT value FROM config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .context("Failed to read config value")
    }

    /// Sets a configuration value in the `config` table (upsert).
    pub fn set_config(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Returns the configured mode ('fluid' or 'strict').
    pub fn mode(&self) -> Result<String> {
        Ok(self.get_config("mode")?.unwrap_or_else(|| "fluid".to_string()))
    }

    /// Returns the sleep timeout in hours.
    pub fn sleep_timeout_hours(&self) -> Result<u64> {
        let val = self.get_config("sleep_timeout_hours")?
            .unwrap_or_else(|| "4".to_string());
        val.parse::<u64>()
            .context("Invalid sleep_timeout_hours config value")
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Maps a SQLite row to a Project struct.
fn row_to_project(row: &rusqlite::Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        status: ProjectStatus::from_str(&row.get::<_, String>(3)?),
        scale: Scale::from_str(&row.get::<_, String>(4)?),
        file_count: row.get(5)?,
        vector_count: row.get(6)?,
        growth_rate: row.get(7)?,
        watcher_pid: row.get(8)?,
        last_opened: row.get(9)?,
        last_scan: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

/// Normalizes a filesystem path for consistent storage.
/// Re-exported from `crate::normalize_path`.
fn normalize_path(path: &str) -> String {
    crate::normalize_path(path)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Creates an in-memory registry for testing.
    fn test_registry() -> ProjectRegistry {
        let db = Connection::open_in_memory().expect("Failed to open in-memory SQLite");
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .expect("Failed to set pragmas");
        let registry = ProjectRegistry { db };
        registry.migrate().expect("Migration failed");
        registry
    }

    #[test]
    fn migration_creates_tables() {
        let reg = test_registry();
        let count: i64 = reg
            .db
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn register_and_list_projects() {
        let reg = test_registry();

        let p = reg.register("myapp", "C:\\Users\\dev\\myapp").unwrap();
        assert_eq!(p.name, "myapp");
        assert_eq!(p.status, ProjectStatus::Sleeping);
        assert_eq!(p.scale, Scale::Unknown);

        let all = reg.list_projects().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "myapp");
    }

    #[test]
    fn register_duplicate_name_fails() {
        let reg = test_registry();
        reg.register("api", "C:\\projects\\api").unwrap();
        assert!(reg.register("api", "C:\\projects\\api2").is_err());
    }

    #[test]
    fn deregister_removes_project() {
        let reg = test_registry();
        reg.register("temp", "C:\\temp\\project").unwrap();
        assert!(reg.deregister("temp").unwrap());
        assert!(reg.get_project_by_name("temp").unwrap().is_none());
        assert!(!reg.deregister("nonexistent").unwrap());
    }

    #[test]
    fn wake_and_sleep_lifecycle() {
        let reg = test_registry();
        reg.register("backend", "C:\\dev\\backend").unwrap();

        // Wake
        reg.wake_project("backend", 12345).unwrap();
        let p = reg.get_project_by_name("backend").unwrap().unwrap();
        assert_eq!(p.status, ProjectStatus::Active);
        assert_eq!(p.watcher_pid, Some(12345));
        assert!(p.last_opened.is_some());

        // Sleep
        reg.sleep_project("backend").unwrap();
        let p = reg.get_project_by_name("backend").unwrap().unwrap();
        assert_eq!(p.status, ProjectStatus::Sleeping);
        assert_eq!(p.watcher_pid, None);
    }

    #[test]
    fn wake_nonexistent_project_fails() {
        let reg = test_registry();
        assert!(reg.wake_project("ghost", 999).is_err());
    }

    #[test]
    fn update_scale_and_stats() {
        let reg = test_registry();
        reg.register("app", "C:\\app").unwrap();

        reg.update_scale("app", Scale::Large, 1500).unwrap();
        let p = reg.get_project_by_name("app").unwrap().unwrap();
        assert_eq!(p.scale, Scale::Large);
        assert_eq!(p.file_count, 1500);

        reg.update_stats("app", 1600, 450).unwrap();
        let p = reg.get_project_by_name("app").unwrap().unwrap();
        assert_eq!(p.file_count, 1600);
        assert_eq!(p.vector_count, 450);
    }

    #[test]
    fn sync_queue_operations() {
        let reg = test_registry();
        let p = reg.register("proj", "C:\\proj").unwrap();

        // Enqueue
        reg.enqueue_sync(p.id, "upsert", "C:\\proj\\src\\main.rs").unwrap();
        reg.enqueue_sync(p.id, "upsert", "C:\\proj\\src\\lib.rs").unwrap();
        reg.enqueue_sync(p.id, "delete", "C:\\proj\\src\\old.rs").unwrap();

        // Check count
        assert_eq!(reg.pending_sync_count(p.id).unwrap(), 3);

        // Dequeue batch
        let batch = reg.dequeue_syncs(p.id, 2).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].file_path, "C:\\proj\\src\\main.rs");
        assert_eq!(batch[1].file_path, "C:\\proj\\src\\lib.rs");

        // Mark synced
        reg.mark_synced(batch[0].id).unwrap();
        assert_eq!(reg.pending_sync_count(p.id).unwrap(), 2);

        reg.mark_synced(batch[1].id).unwrap();
        assert_eq!(reg.pending_sync_count(p.id).unwrap(), 1);
    }

    #[test]
    fn config_get_and_set() {
        let reg = test_registry();

        // Default values from migration
        assert_eq!(reg.get_config("mode").unwrap(), Some("fluid".to_string()));
        assert_eq!(reg.sleep_timeout_hours().unwrap(), 4);

        // Override
        reg.set_config("mode", "strict").unwrap();
        assert_eq!(reg.mode().unwrap(), "strict");

        // New key
        reg.set_config("custom_key", "custom_value").unwrap();
        assert_eq!(
            reg.get_config("custom_key").unwrap(),
            Some("custom_value".to_string())
        );
    }

    #[test]
    fn scale_classification() {
        assert_eq!(Scale::from_file_count(0), Scale::Small);
        assert_eq!(Scale::from_file_count(50), Scale::Small);
        assert_eq!(Scale::from_file_count(99), Scale::Small);
        assert_eq!(Scale::from_file_count(100), Scale::Medium);
        assert_eq!(Scale::from_file_count(500), Scale::Medium);
        assert_eq!(Scale::from_file_count(1000), Scale::Large);
        assert_eq!(Scale::from_file_count(5000), Scale::Large);
        assert_eq!(Scale::from_file_count(10000), Scale::Enterprise);
    }

    #[test]
    fn discover_project_finds_git() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("myproject");
        fs::create_dir_all(project_dir.join(".git")).unwrap();
        fs::create_dir_all(project_dir.join("src")).unwrap();

        let result = ProjectRegistry::discover_project(&project_dir.join("src"));
        assert!(result.is_some());
        let (name, root) = result.unwrap();
        assert_eq!(name, "myproject");
        assert_eq!(root, project_dir);
    }

    #[test]
    fn discover_project_finds_cargo_toml() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("rustapp");
        fs::create_dir_all(&project_dir).unwrap();
        fs::write(project_dir.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        let result = ProjectRegistry::discover_project(&project_dir);
        assert!(result.is_some());
        let (name, _) = result.unwrap();
        assert_eq!(name, "rustapp");
    }

    #[test]
    fn discover_project_returns_none_for_root() {
        // Should not discover anything for a system root path
        let result = ProjectRegistry::discover_project(Path::new("C:\\"));
        assert!(result.is_none());
    }

    #[test]
    fn normalize_path_strips_prefix() {
        assert_eq!(
            normalize_path(r"\\?\C:\Users\dev\project"),
            r"C:\Users\dev\project"
        );
        assert_eq!(
            normalize_path(r"C:\Users\dev\project\"),
            r"C:\Users\dev\project"
        );
        assert_eq!(
            normalize_path("C:/Users/dev/project/"),
            "C:/Users/dev/project"
        );
    }

    #[test]
    fn unique_name_avoids_collisions() {
        let reg = test_registry();
        reg.register("api", "C:\\api").unwrap();

        let name = reg.unique_name("api").unwrap();
        assert_eq!(name, "api-2");

        reg.register("api-2", "C:\\api2").unwrap();
        let name = reg.unique_name("api").unwrap();
        assert_eq!(name, "api-3");
    }

    #[test]
    fn get_project_by_path_case_insensitive() {
        let reg = test_registry();
        reg.register("myapp", "C:\\Users\\Dev\\MyApp").unwrap();

        let found = reg
            .get_project_by_path("c:\\users\\dev\\myapp")
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "myapp");
    }
}
