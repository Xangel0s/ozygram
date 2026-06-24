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

	"ozymem-go-sidecar/extractor"
	"ozymem-go-sidecar/graph"
	"ozymem-go-sidecar/watcher"
	"ozymem-go-sidecar/vector"
)

func main() {
	log.Println("[INFO] Iniciando OzyMem Go Sidecar...")

	// 1. Configuraciones básicas
	projectPath := os.Getenv("OZYMEM_PROJECT_PATH")
	if projectPath == "" {
		projectPath = "."
	}

	cliPath := os.Getenv("OZYMEM_CLI_PATH")
	if cliPath == "" {
		// Asumir binario compilado en target/debug del monorepo
		wd, _ := os.Getwd()
		cliPath = filepath.Join(wd, "..", "..", "target", "debug", "ozymem.exe")
		if _, err := os.Stat(cliPath); os.IsNotExist(err) {
			cliPath = "ozymem" // Fallback al PATH del sistema
		}
	}

	memgraphURI := os.Getenv("MEMGRAPH_URI")
	if memgraphURI == "" {
		memgraphURI = "bolt://localhost:7687"
	}
	memgraphUser := os.Getenv("MEMGRAPH_USER")
	if memgraphUser == "" {
		memgraphUser = "admin"
	}
	memgraphPassword := os.Getenv("MEMGRAPH_PASSWORD")
	if memgraphPassword == "" {
		memgraphPassword = ""
	}

	tenantID := os.Getenv("OZYBASE_MCP_TOKEN")
	if tenantID == "" {
		tenantID = "local"
	}

	geminiAPIKey := os.Getenv("GEMINI_API_KEY")

	// 2. Conectar a Memgraph
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	memgraphClient, err := graph.NewMemgraphClient(memgraphURI, memgraphUser, memgraphPassword, "")
	if err != nil {
		log.Fatalf("[ERROR] No se pudo inicializar cliente de Memgraph: %v", err)
	}
	defer memgraphClient.Close(context.Background())

	err = memgraphClient.Ping(ctx)
	if err != nil {
		log.Printf("[WARNING] No se pudo conectar a Memgraph en %s (Ping fallido): %v", memgraphURI, err)
		log.Println("[INFO] El sidecar continuará en modo diferido/resiliente.")
	} else {
		log.Println("[SUCCESS] Conexión establecida con Memgraph.")
	}

	// 3. Inicializar LanceDB (VectorDB)
	vectorDir := filepath.Join(projectPath, ".ozymem", "vectors")
	vectorDB, err := vector.NewVectorDB(vectorDir)
	if err != nil {
		log.Fatalf("[ERROR] No se pudo inicializar VectorDB: %v", err)
	}
	log.Printf("[SUCCESS] VectorDB inicializada en %s", vectorDir)

	embedGen := vector.NewEmbeddingsGenerator(geminiAPIKey)

	// 4. Inicializar Watcher y Extractor
	ignoreDirs := []string{"node_modules", ".git", "target", "dist", "vendor", ".ozymem"}
	fileWatcher, err := watcher.NewWatcher(ignoreDirs)
	if err != nil {
		log.Fatalf("[ERROR] No se pudo crear Watcher: %v", err)
	}
	defer fileWatcher.Close()

	fileExtractor := extractor.NewExtractor(cliPath)

	// 5. Iniciar Loop de Watcher
	err = fileWatcher.Start(projectPath)
	if err != nil {
		log.Fatalf("[ERROR] No se pudo arrancar Watcher en %s: %v", projectPath, err)
	}
	log.Printf("[INFO] Monitoreando cambios en tiempo real en: %s", projectPath)

	// Manejo de señales para apagado elegante
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)

	// Canal para detener procesamiento asíncrono
	doneChan := make(chan struct{})

	go func() {
		for {
			select {
			case filePath, ok := <-fileWatcher.Events:
				if !ok {
					return
				}
				log.Printf("[EVENT] Cambio detectado en: %s", filePath)

				// Procesamiento e indexación
				go func(fp string) {
					processCtx, processCancel := context.WithTimeout(context.Background(), 30*time.Second)
					defer processCancel()

					// Paso A: Extracción AST
					res, err := fileExtractor.ParseFile(fp)
					if err != nil {
						log.Printf("[ERROR] Extractor falló para %s: %v", fp, err)
						return
					}

					log.Printf("[INFO] Símbolos extraídos (%d funciones) para %s", len(res.DefinitionMap.Functions), fp)

					// Paso B: Guardar definición de archivo y funciones en Memgraph
					err = memgraphClient.SaveFileDefinition(processCtx, tenantID, &res.DefinitionMap)
					if err != nil {
						log.Printf("[ERROR] Falló guardar definición en Memgraph para %s: %v", fp, err)
					} else {
						log.Printf("[SUCCESS] Definiciones registradas en Memgraph para %s", fp)
					}

					// Paso C: Generar embeddings e indexar en VectorDB
					for _, fn := range res.DefinitionMap.Functions {
						// Usar firma o contenido para generar engrama
						representation := fmt.Sprintf("File: %s\nLanguage: %s\nSymbol: %s (%s)\nLines: %d-%d",
							fp, res.DefinitionMap.Language, fn.Name, fn.Kind, fn.StartLine, fn.EndLine)

						emb, err := embedGen.GenerateEmbedding(processCtx, representation)
						if err != nil {
							log.Printf("[ERROR] Falló generación de embeddings para %s: %v", fn.Name, err)
							continue
						}

						err = vectorDB.Upsert(vector.VectorRecord{
							ID:        fmt.Sprintf("%s::%s", fp, fn.Name),
							FilePath:  fp,
							Symbol:    fn.Name,
							Code:      representation,
							Embedding: emb,
						})
						if err != nil {
							log.Printf("[ERROR] Falló upsert en VectorDB para %s::%s: %v", fp, fn.Name, err)
						}
					}

					// Paso D: Guardar relaciones de dependencia
					for _, hint := range res.DependencyHints {
						// Si la dependencia es local y tiene un path válido
						if hint.Label != "" && filepath.IsAbs(hint.Label) {
							err = memgraphClient.SaveDependencyRelation(processCtx, tenantID, fp, hint.Label)
							if err != nil {
								log.Printf("[ERROR] Falló guardar dependencia en Memgraph: %v", err)
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

	// Esperar señal de terminación
	sig := <-sigChan
	log.Printf("[INFO] Señal de parada recibida (%v). Finalizando sidecar...", sig)
	close(doneChan)
	log.Println("[INFO] Sidecar detenido con éxito.")
}
