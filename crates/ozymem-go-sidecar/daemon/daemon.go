package daemon

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"path/filepath"
	"sync"
	"time"

	"ozymem-go-sidecar/extractor"
	"ozymem-go-sidecar/graph"
	"ozymem-go-sidecar/vector"
	"ozymem-go-sidecar/watcher"
)

// ProjectWatcher represents an active watcher goroutine for a single project.
type ProjectWatcher struct {
	Name       string
	Path       string
	Scale      string
	Watcher    *watcher.Watcher
	Cancel     context.CancelFunc
	LastEvent  time.Time
}

// Daemon is the central orchestrator that manages multiple project watchers.
// It reads state from the SQLite registry and listens for commands over socket.
type Daemon struct {
	registry  *Registry
	watchers  map[string]*ProjectWatcher // key = project name
	socket    *SocketServer
	memgraph  *graph.MemgraphClient
	vectorDB  *vector.VectorDB
	embedGen  *vector.EmbeddingsGenerator
	extractor *extractor.Extractor
	mu        sync.RWMutex
	done      chan struct{}
}

// Config holds the daemon's configuration.
type Config struct {
	MemgraphURI      string
	MemgraphUser     string
	MemgraphPassword string
	TenantID         string
	GeminiAPIKey     string
	CLIPath          string
	VectorDir        string
}

// NewDaemon creates a new Daemon with the given configuration.
func NewDaemon(cfg Config) (*Daemon, error) {
	// 1. Open SQLite registry
	reg, err := OpenRegistry()
	if err != nil {
		return nil, fmt.Errorf("failed to open registry: %w", err)
	}

	// 2. Connect to Memgraph
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	memgraphClient, err := graph.NewMemgraphClient(cfg.MemgraphURI, cfg.MemgraphUser, cfg.MemgraphPassword, "")
	if err != nil {
		log.Printf("[WARNING] Could not initialize Memgraph client: %v (continuing in offline mode)", err)
	} else if err := memgraphClient.Ping(ctx); err != nil {
		log.Printf("[WARNING] Could not ping Memgraph at %s: %v (continuing in offline mode)", cfg.MemgraphURI, err)
	} else {
		log.Println("[SUCCESS] Connected to Memgraph.")
	}

	// 3. Initialize VectorDB
	var vectorDB *vector.VectorDB
	if cfg.VectorDir != "" {
		vectorDB, err = vector.NewVectorDB(cfg.VectorDir)
		if err != nil {
			log.Printf("[WARNING] Could not initialize VectorDB at %s: %v", cfg.VectorDir, err)
		} else {
			log.Printf("[SUCCESS] VectorDB initialized at %s", cfg.VectorDir)
		}
	}

	// 4. Initialize embeddings generator
	embedGen := vector.NewEmbeddingsGenerator(cfg.GeminiAPIKey)

	// 5. Initialize extractor
	fileExtractor := extractor.NewExtractor(cfg.CLIPath)

	d := &Daemon{
		registry:  reg,
		watchers:  make(map[string]*ProjectWatcher),
		memgraph:  memgraphClient,
		vectorDB:  vectorDB,
		embedGen:  embedGen,
		extractor: fileExtractor,
		done:      make(chan struct{}),
	}

	return d, nil
}

// Start launches the daemon: socket server, reconciliation loop,
// and watchers for all ACTIVE projects.
func (d *Daemon) Start() error {
	// 1. Start socket server
	server, err := NewSocketServer(d.handleCommand)
	if err != nil {
		return fmt.Errorf("failed to start socket server: %w", err)
	}
	d.socket = server

	// 2. Reconcile: start watchers for all ACTIVE projects
	if err := d.reconcile(); err != nil {
		log.Printf("[WARNING] Initial reconciliation error: %v", err)
	}

	// 3. Start reconciliation loop (every 30s)
	go d.reconciliationLoop()

	// 4. Start inactivity checker (every 5 minutes)
	go d.inactivityLoop()

	log.Println("[DAEMON] OzyMem Daemon started successfully.")
	return nil
}

// Stop gracefully shuts down the daemon: stops all watchers, closes socket
// and registry.
func (d *Daemon) Stop() {
	close(d.done)

	// Stop all watchers
	d.mu.Lock()
	for name, pw := range d.watchers {
		log.Printf("[DAEMON] Stopping watcher for '%s'...", name)
		pw.Cancel()
		pw.Watcher.Close()
		delete(d.watchers, name)
	}
	d.mu.Unlock()

	// Close socket server
	if d.socket != nil {
		d.socket.Close()
	}

	// Close registry
	if d.registry != nil {
		d.registry.Close()
	}

	if d.memgraph != nil {
		d.memgraph.Close(context.Background())
	}

	log.Println("[DAEMON] Stopped.")
}

// Wait blocks until the daemon receives a stop signal.
func (d *Daemon) Wait() {
	<-d.done
}

// --------------------------------------------------------------------------
// Command Handler (dispatched by socket server)
// --------------------------------------------------------------------------

func (d *Daemon) handleCommand(cmd SocketCommand) SocketResponse {
	switch cmd.Cmd {
	case "wake":
		return d.cmdWake(cmd)
	case "sleep":
		return d.cmdSleep(cmd)
	case "status":
		return d.cmdStatus(cmd)
	case "register":
		return d.cmdRegister(cmd)
	case "list":
		return d.cmdList()
	case "ping":
		return SocketResponse{Ok: true, Status: "PONG"}
	default:
		return SocketResponse{Ok: false, Error: fmt.Sprintf("unknown command: %s", cmd.Cmd)}
	}
}

// cmdWake activates a project: registers if needed, starts its watcher.
func (d *Daemon) cmdWake(cmd SocketCommand) SocketResponse {
	if cmd.Path == "" {
		return SocketResponse{Ok: false, Error: "path is required for wake command"}
	}

	// Find or register the project
	project, err := d.registry.GetProjectByPath(cmd.Path)
	if err != nil {
		return SocketResponse{Ok: false, Error: fmt.Sprintf("registry lookup failed: %v", err)}
	}

	if project == nil {
		// Auto-register with auto-discovered name
		name := cmd.Name
		if name == "" {
			name = filepath.Base(cmd.Path)
		}
		project, err = d.registry.RegisterProject(name, cmd.Path)
		if err != nil {
			return SocketResponse{Ok: false, Error: fmt.Sprintf("auto-registration failed: %v", err)}
		}
		log.Printf("[DAEMON] Auto-registered project '%s' at %s", project.Name, project.Path)
	}

	// Start watcher if not already running
	d.mu.RLock()
	_, running := d.watchers[project.Name]
	d.mu.RUnlock()

	if !running {
		if err := d.startWatcher(project); err != nil {
			return SocketResponse{Ok: false, Error: fmt.Sprintf("failed to start watcher: %v", err)}
		}
	}

	return SocketResponse{Ok: true, Status: "ACTIVE", Scale: project.Scale}
}

// cmdSleep puts a project to sleep: stops its watcher.
func (d *Daemon) cmdSleep(cmd SocketCommand) SocketResponse {
	// Find the project
	var project *Project
	var err error
	if cmd.Name != "" {
		project, err = d.registry.GetProjectByName(cmd.Name)
	} else if cmd.Path != "" {
		project, err = d.registry.GetProjectByPath(cmd.Path)
	} else {
		return SocketResponse{Ok: false, Error: "name or path is required for sleep command"}
	}

	if err != nil || project == nil {
		return SocketResponse{Ok: false, Error: "project not found"}
	}

	// Stop watcher
	d.mu.Lock()
	if pw, ok := d.watchers[project.Name]; ok {
		pw.Cancel()
		pw.Watcher.Close()
		delete(d.watchers, project.Name)
	}
	d.mu.Unlock()

	// Update registry
	if err := d.registry.SleepProject(project.Name); err != nil {
		return SocketResponse{Ok: false, Error: fmt.Sprintf("failed to update registry: %v", err)}
	}

	log.Printf("[DAEMON] Project '%s' put to sleep.", project.Name)
	return SocketResponse{Ok: true, Status: "SLEEPING"}
}

// cmdStatus returns the status of a specific project.
func (d *Daemon) cmdStatus(cmd SocketCommand) SocketResponse {
	var project *Project
	var err error
	if cmd.Name != "" {
		project, err = d.registry.GetProjectByName(cmd.Name)
	} else if cmd.Path != "" {
		project, err = d.registry.GetProjectByPath(cmd.Path)
	} else {
		return SocketResponse{Ok: false, Error: "name or path required"}
	}

	if err != nil || project == nil {
		return SocketResponse{Ok: false, Error: "project not found"}
	}

	return SocketResponse{Ok: true, Status: project.Status, Scale: project.Scale}
}

// cmdRegister registers a new project without starting its watcher.
func (d *Daemon) cmdRegister(cmd SocketCommand) SocketResponse {
	if cmd.Path == "" {
		return SocketResponse{Ok: false, Error: "path is required"}
	}
	name := cmd.Name
	if name == "" {
		name = filepath.Base(cmd.Path)
	}

	project, err := d.registry.RegisterProject(name, cmd.Path)
	if err != nil {
		return SocketResponse{Ok: false, Error: fmt.Sprintf("registration failed: %v", err)}
	}

	return SocketResponse{Ok: true, Status: "REGISTERED", Scale: project.Scale}
}

// cmdList returns a JSON list of all projects.
func (d *Daemon) cmdList() SocketResponse {
	projects, err := d.registry.ListProjects()
	if err != nil {
		return SocketResponse{Ok: false, Error: fmt.Sprintf("list failed: %v", err)}
	}

	data, _ := json.Marshal(projects)
	return SocketResponse{Ok: true, Data: string(data)}
}

// --------------------------------------------------------------------------
// Watcher Lifecycle
// --------------------------------------------------------------------------

// startWatcher launches a file watcher goroutine for a project.
func (d *Daemon) startWatcher(project *Project) error {
	ignoreDirs := []string{"node_modules", ".git", "target", "dist", "vendor", ".ozymem", "__pycache__", ".venv"}
	fileWatcher, err := watcher.NewWatcher(ignoreDirs)
	if err != nil {
		return fmt.Errorf("failed to create watcher for '%s': %w", project.Name, err)
	}

	if err := fileWatcher.Start(project.Path); err != nil {
		fileWatcher.Close()
		return fmt.Errorf("failed to start watching '%s': %w", project.Path, err)
	}

	ctx, cancel := context.WithCancel(context.Background())
	pw := &ProjectWatcher{
		Name:      project.Name,
		Path:      project.Path,
		Scale:     project.Scale,
		Watcher:   fileWatcher,
		Cancel:    cancel,
		LastEvent: time.Now(),
	}

	d.mu.Lock()
	d.watchers[project.Name] = pw
	d.mu.Unlock()

	// Update registry state
	// Use PID 0 since the watcher runs as a goroutine within this daemon
	if err := d.registry.WakeProject(project.Name, 0); err != nil {
		log.Printf("[WARNING] Failed to update registry for '%s': %v", project.Name, err)
	}

	// Launch event processing goroutine
	go d.processWatcherEvents(ctx, pw)

	log.Printf("[DAEMON] Watcher started for '%s' at %s", project.Name, project.Path)
	return nil
}

// processWatcherEvents handles file system events for a single project.
func (d *Daemon) processWatcherEvents(ctx context.Context, pw *ProjectWatcher) {
	tenantID := "local" // Default tenant for local development

	for {
		select {
		case <-ctx.Done():
			return

		case filePath, ok := <-pw.Watcher.Events:
			if !ok {
				return
			}

			pw.LastEvent = time.Now()
			log.Printf("[EVENT] [%s] Change detected: %s", pw.Name, filePath)

			// Process in a separate goroutine for non-blocking behavior
			go func(fp string) {
				processCtx, processCancel := context.WithTimeout(context.Background(), 30*time.Second)
				defer processCancel()

				// Step A: Parse file with the Rust CLI extractor
				res, err := d.extractor.ParseFile(fp)
				if err != nil {
					log.Printf("[ERROR] [%s] Extractor failed for %s: %v", pw.Name, fp, err)
					// Enqueue to sync queue for retry later
					if project, _ := d.registry.GetProjectByName(pw.Name); project != nil {
						d.registry.EnqueueSync(project.ID, "upsert", fp)
					}
					return
				}

				log.Printf("[INFO] [%s] Extracted %d symbols from %s", pw.Name, len(res.DefinitionMap.Functions), fp)

				// Step B: Save to Memgraph
				if d.memgraph != nil {
					if err := d.memgraph.SaveFileDefinition(processCtx, tenantID, &res.DefinitionMap); err != nil {
						log.Printf("[ERROR] [%s] Memgraph save failed for %s: %v", pw.Name, fp, err)
						if project, _ := d.registry.GetProjectByName(pw.Name); project != nil {
							d.registry.EnqueueSync(project.ID, "upsert", fp)
						}
					} else {
						log.Printf("[SUCCESS] [%s] Indexed in Memgraph: %s", pw.Name, fp)
					}
				}

				// Step C: Generate embeddings and index in VectorDB
				if d.vectorDB != nil && d.embedGen != nil {
					for _, fn := range res.DefinitionMap.Functions {
						representation := fmt.Sprintf("File: %s\nLanguage: %s\nSymbol: %s (%s)\nLines: %d-%d",
							fp, res.DefinitionMap.Language, fn.Name, fn.Kind, fn.StartLine, fn.EndLine)

						emb, err := d.embedGen.GenerateEmbedding(processCtx, representation)
						if err != nil {
							log.Printf("[ERROR] [%s] Embedding failed for %s: %v", pw.Name, fn.Name, err)
							continue
						}

						err = d.vectorDB.Upsert(vector.VectorRecord{
							ID:            fmt.Sprintf("%s::%s", fp, fn.Name),
							Category:      "fact",
							Project:       pw.Name,
							TenantID:      tenantID,
							SourcePath:    fp,
							Timestamp:     time.Now().Unix(),
							HitCount:      0,
							Text:          representation,
							Embedding:     emb,
							SchemaVersion: 1,
							ParentID:      nil,
						})
						if err != nil {
							log.Printf("[ERROR] [%s] VectorDB upsert failed for %s::%s: %v", pw.Name, fp, fn.Name, err)
						}
					}
				}

				// Step D: Save dependency relations
				if d.memgraph != nil {
					for _, hint := range res.DependencyHints {
						if hint.Label != "" && filepath.IsAbs(hint.Label) {
							if err := d.memgraph.SaveDependencyRelation(processCtx, tenantID, fp, hint.Label); err != nil {
								log.Printf("[ERROR] [%s] Dependency save failed: %v", pw.Name, err)
							}
						}
					}
				}
			}(filePath)

		case err, ok := <-pw.Watcher.Errors:
			if !ok {
				return
			}
			log.Printf("[WARNING] [%s] Watcher error: %v", pw.Name, err)
		}
	}
}

// --------------------------------------------------------------------------
// Reconciliation & Inactivity Loops
// --------------------------------------------------------------------------

// reconcile ensures that watchers match the desired state in SQLite.
// Projects marked ACTIVE should have running watchers; dead watchers get restarted.
func (d *Daemon) reconcile() error {
	activeProjects, err := d.registry.GetActiveProjects()
	if err != nil {
		return fmt.Errorf("failed to query active projects: %w", err)
	}

	d.mu.RLock()
	runningNames := make(map[string]bool)
	for name := range d.watchers {
		runningNames[name] = true
	}
	d.mu.RUnlock()

	// Start watchers for ACTIVE projects that aren't running
	for _, project := range activeProjects {
		if !runningNames[project.Name] {
			log.Printf("[RECONCILE] Restarting watcher for '%s'", project.Name)
			if err := d.startWatcher(&project); err != nil {
				log.Printf("[RECONCILE] Failed to restart watcher for '%s': %v", project.Name, err)
			}
		}
	}

	return nil
}

// reconciliationLoop runs reconciliation every 30 seconds.
func (d *Daemon) reconciliationLoop() {
	ticker := time.NewTicker(30 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-d.done:
			return
		case <-ticker.C:
			if err := d.reconcile(); err != nil {
				log.Printf("[RECONCILE] Error: %v", err)
			}
		}
	}
}

// inactivityLoop checks for projects with no file events for sleep_timeout_hours
// and automatically puts them to sleep.
func (d *Daemon) inactivityLoop() {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()

	for {
		select {
		case <-d.done:
			return
		case <-ticker.C:
			timeoutStr, _ := d.registry.GetConfig("sleep_timeout_hours")
			timeoutHours := 4 // default
			if timeoutStr != "" {
				fmt.Sscanf(timeoutStr, "%d", &timeoutHours)
			}
			timeout := time.Duration(timeoutHours) * time.Hour

			d.mu.RLock()
			var toSleep []string
			for name, pw := range d.watchers {
				if time.Since(pw.LastEvent) > timeout {
					toSleep = append(toSleep, name)
				}
			}
			d.mu.RUnlock()

			for _, name := range toSleep {
				log.Printf("[INACTIVITY] Project '%s' has been idle for >%dh, putting to sleep.", name, timeoutHours)
				d.handleCommand(SocketCommand{Cmd: "sleep", Name: name})
			}
		}
	}
}
