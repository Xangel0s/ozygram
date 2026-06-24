package daemon

import (
	"database/sql"
	"io/ioutil"
	"os"
	"path/filepath"
	"testing"

	_ "github.com/ncruces/go-sqlite3/driver"
	_ "github.com/ncruces/go-sqlite3/embed"
)

func createTestDB(t *testing.T) (string, *sql.DB) {
	tmpDir, err := ioutil.TempDir("", "ozymem-test")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}

	dbPath := filepath.Join(tmpDir, "registry.db")
	db, err := sql.Open("sqlite3", dbPath)
	if err != nil {
		t.Fatalf("failed to open sqlite: %v", err)
	}

	// Create tables matching Rust schema
	_, err = db.Exec(`
		CREATE TABLE IF NOT EXISTS projects (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			name TEXT NOT NULL UNIQUE,
			path TEXT NOT NULL UNIQUE,
			status TEXT NOT NULL DEFAULT 'SLEEPING',
			scale TEXT NOT NULL DEFAULT 'UNKNOWN',
			file_count INTEGER NOT NULL DEFAULT 0,
			vector_count INTEGER NOT NULL DEFAULT 0,
			growth_rate REAL NOT NULL DEFAULT 0.0,
			watcher_pid INTEGER,
			last_opened TEXT,
			last_scan TEXT,
			created_at TEXT NOT NULL DEFAULT (datetime('now')),
			updated_at TEXT NOT NULL DEFAULT (datetime('now'))
		);
		CREATE TABLE IF NOT EXISTS pending_syncs (
			id INTEGER PRIMARY KEY AUTOINCREMENT,
			project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
			action TEXT NOT NULL,
			file_path TEXT NOT NULL,
			queued_at TEXT NOT NULL DEFAULT (datetime('now')),
			synced_at TEXT
		);
	`)
	if err != nil {
		t.Fatalf("failed to schema db: %v", err)
	}

	return dbPath, db
}

func TestRegistryOperations(t *testing.T) {
	dbPath, db := createTestDB(t)
	defer os.RemoveAll(filepath.Dir(dbPath))
	defer db.Close()

	reg := &Registry{db: db}

	// Test GetActiveProjects initially empty
	active, err := reg.GetActiveProjects()
	if err != nil {
		t.Fatalf("failed to get active projects: %v", err)
	}
	if len(active) != 0 {
		t.Errorf("expected 0 active projects, got %d", len(active))
	}

	// Insert test project
	_, err = db.Exec(`
		INSERT INTO projects (name, path, scale, status)
		VALUES ('proj', '/path/to/proj', 'SMALL', 'ACTIVE')
	`)
	if err != nil {
		t.Fatalf("failed to insert: %v", err)
	}

	active, err = reg.GetActiveProjects()
	if err != nil {
		t.Fatalf("failed to get active projects: %v", err)
	}
	if len(active) != 1 {
		t.Errorf("expected 1 active project, got %d", len(active))
	}
	if active[0].Name != "proj" || active[0].Status != "ACTIVE" {
		t.Errorf("unexpected project: %+v", active[0])
	}

	// Wake project (Update status & PID)
	err = reg.WakeProject("proj", 1234)
	if err != nil {
		t.Fatalf("failed to wake project: %v", err)
	}

	// Read it back
	var pid sql.NullInt64
	err = db.QueryRow("SELECT watcher_pid FROM projects WHERE name = ?", "proj").Scan(&pid)
	if err != nil {
		t.Fatalf("failed to query PID: %v", err)
	}
	if !pid.Valid || pid.Int64 != 1234 {
		t.Errorf("expected pid 1234, got %v", pid)
	}

	// Sleep project
	err = reg.SleepProject("proj")
	if err != nil {
		t.Fatalf("failed to sleep project: %v", err)
	}

	var status string
	err = db.QueryRow("SELECT status FROM projects WHERE name = ?", "proj").Scan(&status)
	if err != nil {
		t.Fatalf("failed to query status: %v", err)
	}
	if status != "SLEEPING" {
		t.Errorf("expected status SLEEPING, got %s", status)
	}
}
