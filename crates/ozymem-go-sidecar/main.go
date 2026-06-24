package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"ozymem-go-sidecar/daemon"
	"ozymem-go-sidecar/extractor"
	"ozymem-go-sidecar/graph"
	"ozymem-go-sidecar/vector"
	"ozymem-go-sidecar/watcher"
)

func main() {
	// Determine mode: daemon (multi-project) or legacy (single-project)
	mode := os.Getenv("OZYMEM_SIDECAR_MODE")
	if mode == "" {
		mode = "daemon" // Default to new daemon mode
	}

	switch mode {
	case "daemon":
		runDaemon()
	case "legacy":
		runLegacy()
	default:
		log.Fatalf("[ERROR] Unknown sidecar mode: %s (expected 'daemon' or 'legacy')", mode)
	}
}

// runDaemon starts the multi-project daemon with SQLite registry + socket.
func runDaemon() {
	log.Println("[INFO] Starting OzyMem Daemon (multi-project mode)...")

	cfg := daemon.Config{
		MemgraphURI:      getEnvOrDefault("MEMGRAPH_URI", "bolt://localhost:7687"),
		MemgraphUser:     getEnvOrDefault("MEMGRAPH_USER", "admin"),
		MemgraphPassword: getEnvOrDefault("MEMGRAPH_PASSWORD", ""),
		TenantID:         getEnvOrDefault("OZYBASE_MCP_TOKEN", "local"),
		GeminiAPIKey:     os.Getenv("GEMINI_API_KEY"),
		CLIPath:          resolveCLIPath(),
		VectorDir:        os.Getenv("OZYMEM_VECTOR_DIR"),
	}

	d, err := daemon.NewDaemon(cfg)
	if err != nil {
		log.Fatalf("[ERROR] Failed to create daemon: %v", err)
	}

	if err := d.Start(); err != nil {
		log.Fatalf("[ERROR] Failed to start daemon: %v", err)
	}

	// Wait for shutdown signal
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)

	sig := <-sigChan
	log.Printf("[INFO] Shutdown signal received (%v). Stopping daemon...", sig)
	d.Stop()
	log.Println("[INFO] Daemon stopped successfully.")
}

// runLegacy runs the original single-project watcher mode for backwards compatibility.
func runLegacy() {
	log.Println("[INFO] Starting OzyMem Go Sidecar (legacy single-project mode)...")

	// 1. Basic configuration
	projectPath := os.Getenv("OZYMEM_PROJECT_PATH")
	if projectPath == "" {
		projectPath = "."
	}

	cliPath := resolveCLIPath()

	memgraphURI := getEnvOrDefault("MEMGRAPH_URI", "bolt://localhost:7687")
	memgraphUser := getEnvOrDefault("MEMGRAPH_USER", "admin")
	memgraphPassword := getEnvOrDefault("MEMGRAPH_PASSWORD", "")

	tenantID := getEnvOrDefault("OZYBASE_MCP_TOKEN", "local")
	geminiAPIKey := os.Getenv("GEMINI_API_KEY")

	// 2. Connect to Memgraph
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	memgraphClient, err := graph.NewMemgraphClient(memgraphURI, memgraphUser, memgraphPassword, "")
	if err != nil {
		log.Fatalf("[ERROR] Could not initialize Memgraph client: %v", err)
	}
	defer memgraphClient.Close(context.Background())

	err = memgraphClient.Ping(ctx)
	if err != nil {
		log.Printf("[WARNING] Could not connect to Memgraph at %s (Ping failed): %v", memgraphURI, err)
		log.Println("[INFO] Sidecar will continue in deferred/resilient mode.")
	} else {
		log.Println("[SUCCESS] Connection established with Memgraph.")
	}

	// 3. Initialize LanceDB (VectorDB)
	vectorDir := filepath.Join(projectPath, ".ozymem", "vectors")
	vectorDB, err := vector.NewVectorDB(vectorDir)
	if err != nil {
		log.Fatalf("[ERROR] Could not initialize VectorDB: %v", err)
	}
	log.Printf("[SUCCESS] VectorDB initialized at %s", vectorDir)

	embedGen := vector.NewEmbeddingsGenerator(geminiAPIKey)

	// 4. Initialize Watcher and Extractor
	ignoreDirs := []string{"node_modules", ".git", "target", "dist", "vendor", ".ozymem"}
	fileWatcher, err := watcher.NewWatcher(ignoreDirs)
	if err != nil {
		log.Fatalf("[ERROR] Could not create Watcher: %v", err)
	}
	defer fileWatcher.Close()

	fileExtractor := extractor.NewExtractor(cliPath)

	// 5. Start Watcher Loop
	err = fileWatcher.Start(projectPath)
	if err != nil {
		log.Fatalf("[ERROR] Could not start Watcher at %s: %v", projectPath, err)
	}
	log.Printf("[INFO] Monitoring real-time changes at: %s", projectPath)

	// Signal handling for graceful shutdown
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)

	doneChan := make(chan struct{})

	go func() {
		for {
			select {
			case filePath, ok := <-fileWatcher.Events:
				if !ok {
					return
				}
				log.Printf("[EVENT] Change detected: %s", filePath)

				// Processing and indexing
				go func(fp string) {
					processCtx, processCancel := context.WithTimeout(context.Background(), 30*time.Second)
					defer processCancel()

					// Step A: AST Extraction
					res, err := fileExtractor.ParseFile(fp)
					if err != nil {
						log.Printf("[ERROR] Extractor failed for %s: %v", fp, err)
						return
					}

					log.Printf("[INFO] Extracted symbols (%d functions) for %s", len(res.DefinitionMap.Functions), fp)

					// Step B: Save file definition and functions to Memgraph
					err = memgraphClient.SaveFileDefinition(processCtx, tenantID, &res.DefinitionMap)
					if err != nil {
						log.Printf("[ERROR] Failed to save definition to Memgraph for %s: %v", fp, err)
					} else {
						log.Printf("[SUCCESS] Definitions registered in Memgraph for %s", fp)
					}

					// Step C: Generate embeddings and index in VectorDB
					for _, fn := range res.DefinitionMap.Functions {
						representation := fmt.Sprintf("File: %s\nLanguage: %s\nSymbol: %s (%s)\nLines: %d-%d",
							fp, res.DefinitionMap.Language, fn.Name, fn.Kind, fn.StartLine, fn.EndLine)

						emb, err := embedGen.GenerateEmbedding(processCtx, representation)
						if err != nil {
							log.Printf("[ERROR] Failed to generate embeddings for %s: %v", fn.Name, err)
							continue
						}

						err = vectorDB.Upsert(vector.VectorRecord{
							ID:            fmt.Sprintf("%s::%s", fp, fn.Name),
							Category:      "fact",
							Project:       filepath.Base(projectPath),
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
							log.Printf("[ERROR] Failed VectorDB upsert for %s::%s: %v", fp, fn.Name, err)
						}
					}

					// Step D: Save dependency relations
					for _, hint := range res.DependencyHints {
						if hint.Label != "" && filepath.IsAbs(hint.Label) {
							err = memgraphClient.SaveDependencyRelation(processCtx, tenantID, fp, hint.Label)
							if err != nil {
								log.Printf("[ERROR] Failed to save dependency in Memgraph: %v", err)
							}
						}
					}

				}(filePath)

			case err, ok := <-fileWatcher.Errors:
				if !ok {
					return
				}
				log.Printf("[WARNING] Watcher error: %v", err)
			case <-doneChan:
				return
			}
		}
	}()

	// Wait for termination signal
	sig := <-sigChan
	log.Printf("[INFO] Stop signal received (%v). Shutting down sidecar...", sig)
	close(doneChan)
	log.Println("[INFO] Sidecar stopped successfully.")
}

// --------------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------------

func getEnvOrDefault(key, defaultValue string) string {
	if val := os.Getenv(key); val != "" {
		return val
	}
	return defaultValue
}

func resolveCLIPath() string {
	cliPath := os.Getenv("OZYMEM_CLI_PATH")
	if cliPath != "" {
		return cliPath
	}

	// Assume compiled binary in target/debug of the monorepo
	wd, _ := os.Getwd()
	candidate := filepath.Join(wd, "..", "..", "target", "debug", "ozymem.exe")
	if _, err := os.Stat(candidate); err == nil {
		return candidate
	}

	return "ozymem" // Fallback to system PATH
}
