// Package daemon implements the central OzyMem daemon that manages
// multiple project watchers, communicates via named pipe/socket,
// and uses SQLite (registry.db) as its control plane.
package daemon

import (
	"database/sql"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"runtime"

	_ "github.com/ncruces/go-sqlite3/driver"
	_ "github.com/ncruces/go-sqlite3/embed"
)

// Project represents a project record from the SQLite registry.
type Project struct {
	ID          int64
	Name        string
	Path        string
	Status      string
	Scale       string
	FileCount   int64
	VectorCount int64
	GrowthRate  float64
	WatcherPID  *int32
	LastOpened  *string
	LastScan    *string
	CreatedAt   string
	UpdatedAt   string
}

// PendingSync represents a queued file sync operation.
type PendingSync struct {
	ID        int64
	ProjectID int64
	Action    string
	FilePath  string
	QueuedAt  string
	SyncedAt  *string
}

// Registry provides read/write access to the SQLite registry.db.
type Registry struct {
	db *sql.DB
}

// dbPath returns the path to the registry database file (~/.ozymem/registry.db).
func dbPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("cannot determine home directory: %w", err)
	}
	return filepath.Join(home, ".ozymem", "registry.db"), nil
}

// OpenRegistry opens the SQLite registry. It expects the database to already
// exist (created by the Rust CLI or ozymem-core). If the file does not exist,
// it creates the directory and a minimal schema.
func OpenRegistry() (*Registry, error) {
	path, err := dbPath()
	if err != nil {
		return nil, err
	}

	// Ensure parent directory exists
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return nil, fmt.Errorf("cannot create directory %s: %w", dir, err)
	}

	db, err := sql.Open("sqlite3", path+"?_journal_mode=WAL&_busy_timeout=5000")
	if err != nil {
		return nil, fmt.Errorf("cannot open SQLite at %s: %w", path, err)
	}

	// Verify connectivity
	if err := db.Ping(); err != nil {
		db.Close()
		return nil, fmt.Errorf("cannot ping SQLite: %w", err)
	}

	reg := &Registry{db: db}

	// Ensure tables exist (idempotent, in case Rust hasn't initialized yet)
	if err := reg.migrate(); err != nil {
		db.Close()
		return nil, fmt.Errorf("migration failed: %w", err)
	}

	log.Printf("[REGISTRY] Opened SQLite registry at %s", path)
	return reg, nil
}

// Close cleanly closes the database connection.
func (r *Registry) Close() error {
	return r.db.Close()
}

// migrate creates tables if they don't exist.
func (r *Registry) migrate() error {
	_, err := r.db.Exec(`
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

		INSERT OR IGNORE INTO config (key, value) VALUES ('mode', 'fluid');
		INSERT OR IGNORE INTO config (key, value) VALUES ('sleep_timeout_hours', '4');
		INSERT OR IGNORE INTO config (key, value) VALUES ('purge_after_days', '90');
	`)
	return err
}

// --------------------------------------------------------------------------
// Project Queries
// --------------------------------------------------------------------------

// ListProjects returns all registered projects.
func (r *Registry) ListProjects() ([]Project, error) {
	return r.queryProjects("SELECT id, name, path, status, scale, file_count, vector_count, growth_rate, watcher_pid, last_opened, last_scan, created_at, updated_at FROM projects ORDER BY name")
}

// GetActiveProjects returns only projects with status = 'ACTIVE'.
func (r *Registry) GetActiveProjects() ([]Project, error) {
	return r.queryProjects("SELECT id, name, path, status, scale, file_count, vector_count, growth_rate, watcher_pid, last_opened, last_scan, created_at, updated_at FROM projects WHERE status = 'ACTIVE' ORDER BY name")
}

// GetProjectByPath finds a project by path (case-insensitive).
func (r *Registry) GetProjectByPath(path string) (*Project, error) {
	normalized := normalizePath(path)
	projects, err := r.queryProjects("SELECT id, name, path, status, scale, file_count, vector_count, growth_rate, watcher_pid, last_opened, last_scan, created_at, updated_at FROM projects WHERE LOWER(path) = LOWER(?)", normalized)
	if err != nil {
		return nil, err
	}
	if len(projects) == 0 {
		return nil, nil
	}
	return &projects[0], nil
}

// GetProjectByName finds a project by name.
func (r *Registry) GetProjectByName(name string) (*Project, error) {
	projects, err := r.queryProjects("SELECT id, name, path, status, scale, file_count, vector_count, growth_rate, watcher_pid, last_opened, last_scan, created_at, updated_at FROM projects WHERE name = ?", name)
	if err != nil {
		return nil, err
	}
	if len(projects) == 0 {
		return nil, nil
	}
	return &projects[0], nil
}

// queryProjects is a helper that scans rows into Project slices.
func (r *Registry) queryProjects(query string, args ...interface{}) ([]Project, error) {
	rows, err := r.db.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var projects []Project
	for rows.Next() {
		var p Project
		err := rows.Scan(
			&p.ID, &p.Name, &p.Path, &p.Status, &p.Scale,
			&p.FileCount, &p.VectorCount, &p.GrowthRate,
			&p.WatcherPID, &p.LastOpened, &p.LastScan,
			&p.CreatedAt, &p.UpdatedAt,
		)
		if err != nil {
			return nil, err
		}
		projects = append(projects, p)
	}
	return projects, rows.Err()
}

// --------------------------------------------------------------------------
// Lifecycle State Transitions
// --------------------------------------------------------------------------

// WakeProject sets a project to ACTIVE with the given PID.
func (r *Registry) WakeProject(name string, pid int32) error {
	result, err := r.db.Exec(
		"UPDATE projects SET status = 'ACTIVE', watcher_pid = ?, last_opened = datetime('now'), updated_at = datetime('now') WHERE name = ?",
		pid, name,
	)
	if err != nil {
		return err
	}
	n, _ := result.RowsAffected()
	if n == 0 {
		return fmt.Errorf("project '%s' not found in registry", name)
	}
	return nil
}

// SleepProject sets a project to SLEEPING and clears the PID.
func (r *Registry) SleepProject(name string) error {
	result, err := r.db.Exec(
		"UPDATE projects SET status = 'SLEEPING', watcher_pid = NULL, updated_at = datetime('now') WHERE name = ?",
		name,
	)
	if err != nil {
		return err
	}
	n, _ := result.RowsAffected()
	if n == 0 {
		return fmt.Errorf("project '%s' not found in registry", name)
	}
	return nil
}

// RegisterProject registers a new project. Returns the created project.
func (r *Registry) RegisterProject(name, path string) (*Project, error) {
	_, err := r.db.Exec(
		"INSERT INTO projects (name, path, status, last_opened, updated_at) VALUES (?, ?, 'SLEEPING', datetime('now'), datetime('now'))",
		name, path,
	)
	if err != nil {
		return nil, fmt.Errorf("failed to register project '%s': %w", name, err)
	}
	return r.GetProjectByName(name)
}

// UpdateScale updates the scale and file count for a project.
func (r *Registry) UpdateScale(name, scale string, fileCount int64) error {
	_, err := r.db.Exec(
		"UPDATE projects SET scale = ?, file_count = ?, updated_at = datetime('now') WHERE name = ?",
		scale, fileCount, name,
	)
	return err
}

// --------------------------------------------------------------------------
// Sync Queue
// --------------------------------------------------------------------------

// EnqueueSync adds a file change to the sync queue.
func (r *Registry) EnqueueSync(projectID int64, action, filePath string) error {
	_, err := r.db.Exec(
		"INSERT INTO pending_syncs (project_id, action, file_path) VALUES (?, ?, ?)",
		projectID, action, filePath,
	)
	return err
}

// DequeueSyncs returns up to batchSize unprocessed syncs for a project.
func (r *Registry) DequeueSyncs(projectID int64, batchSize int) ([]PendingSync, error) {
	rows, err := r.db.Query(
		"SELECT id, project_id, action, file_path, queued_at, synced_at FROM pending_syncs WHERE project_id = ? AND synced_at IS NULL ORDER BY queued_at ASC LIMIT ?",
		projectID, batchSize,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var syncs []PendingSync
	for rows.Next() {
		var s PendingSync
		if err := rows.Scan(&s.ID, &s.ProjectID, &s.Action, &s.FilePath, &s.QueuedAt, &s.SyncedAt); err != nil {
			return nil, err
		}
		syncs = append(syncs, s)
	}
	return syncs, rows.Err()
}

// MarkSynced marks a sync entry as completed.
func (r *Registry) MarkSynced(syncID int64) error {
	_, err := r.db.Exec(
		"UPDATE pending_syncs SET synced_at = datetime('now') WHERE id = ?",
		syncID,
	)
	return err
}

// --------------------------------------------------------------------------
// Configuration
// --------------------------------------------------------------------------

// GetConfig reads a config value from the config table.
func (r *Registry) GetConfig(key string) (string, error) {
	var value string
	err := r.db.QueryRow("SELECT value FROM config WHERE key = ?", key).Scan(&value)
	if err == sql.ErrNoRows {
		return "", nil
	}
	return value, err
}

// --------------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------------

// normalizePath removes Windows canonicalization prefix and trailing separators.
func normalizePath(p string) string {
	if len(p) > 4 && p[:4] == `\\?\` {
		p = p[4:]
	}
	// Trim trailing separators
	for len(p) > 0 && (p[len(p)-1] == '\\' || p[len(p)-1] == '/') {
		p = p[:len(p)-1]
	}
	return p
}

// SocketPath returns the platform-specific socket path for daemon communication.
func SocketPath() string {
	if runtime.GOOS == "windows" {
		return `\\.\pipe\ozymem-daemon`
	}
	return "/tmp/ozymem-daemon.sock"
}
