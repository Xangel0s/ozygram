package graph

import (
	"context"
	"fmt"
	"ozymem-go-sidecar/extractor"

	"github.com/neo4j/neo4j-go-driver/v5/neo4j"
)

type MemgraphClient struct {
	driver neo4j.DriverWithContext
	dbName string
}

func NewMemgraphClient(uri, username, password, dbName string) (*MemgraphClient, error) {
	driver, err := neo4j.NewDriverWithContext(uri, neo4j.BasicAuth(username, password, ""))
	if err != nil {
		return nil, fmt.Errorf("failed to create neo4j driver: %w", err)
	}

	return &MemgraphClient{
		driver: driver,
		dbName: dbName,
	}, nil
}

func (m *MemgraphClient) Close(ctx context.Context) error {
	return m.driver.Close(ctx)
}

func (m *MemgraphClient) Ping(ctx context.Context) error {
	session := m.driver.NewSession(ctx, neo4j.SessionConfig{
		DatabaseName: m.dbName,
		AccessMode:   neo4j.AccessModeRead,
	})
	defer session.Close(ctx)

	_, err := session.Run(ctx, "RETURN 1", nil)
	return err
}

func (m *MemgraphClient) ClearFileSymbolsAndDependencies(ctx context.Context, tenantID, filePath string) error {
	session := m.driver.NewSession(ctx, neo4j.SessionConfig{
		DatabaseName: m.dbName,
		AccessMode:   neo4j.AccessModeWrite,
	})
	defer session.Close(ctx)

	query := `
		MATCH (f:File {path: $path, tenant_id: $tenant_id})
		OPTIONAL MATCH (f)-[r:DEPENDS_ON]->()
		OPTIONAL MATCH (f)-[:CONTAINS]->(fn:Function)
		DETACH DELETE r, fn
	`
	params := map[string]any{
		"path":      filePath,
		"tenant_id": tenantID,
	}

	_, err := session.Run(ctx, query, params)
	return err
}

func (m *MemgraphClient) SaveFileDefinition(ctx context.Context, tenantID string, def *extractor.FileDefinitionMap) error {
	session := m.driver.NewSession(ctx, neo4j.SessionConfig{
		DatabaseName: m.dbName,
		AccessMode:   neo4j.AccessModeWrite,
	})
	defer session.Close(ctx)

	var functionsList []map[string]any
	for _, fn := range def.Functions {
		kind := "Function"
		if fn.Kind == "Class" {
			kind = "Class"
		}
		functionsList = append(functionsList, map[string]any{
			"name":       fn.Name,
			"start_line": int64(fn.StartLine),
			"end_line":   int64(fn.EndLine),
			"strategy":   def.Strategy,
			"kind":       kind,
		})
	}

	query := `
		MERGE (f:File {path: $path, tenant_id: $tenant_id})
		SET f.language = $language
		WITH f
		OPTIONAL MATCH (f)-[:CONTAINS]->(old_fn:Function)
		DETACH DELETE old_fn
		WITH f
		UNWIND $functions AS fn_data
		MERGE (fn:Function {name: fn_data.name, start_line: fn_data.start_line, end_line: fn_data.end_line, tenant_id: $tenant_id})
		SET fn.strategy = fn_data.strategy,
		    fn.kind = fn_data.kind
		MERGE (f)-[:CONTAINS]->(fn)
	`
	params := map[string]any{
		"path":      def.FilePath,
		"language":  def.Language,
		"tenant_id": tenantID,
		"functions": functionsList,
	}

	_, err := session.Run(ctx, query, params)
	return err
}

func (m *MemgraphClient) SaveDependencyRelation(ctx context.Context, tenantID, originPath, destinationPath string) error {
	session := m.driver.NewSession(ctx, neo4j.SessionConfig{
		DatabaseName: m.dbName,
		AccessMode:   neo4j.AccessModeWrite,
	})
	defer session.Close(ctx)

	query := `
		MATCH (origen:File {path: $ruta_origen, tenant_id: $tenant_id}), (destino:File {path: $ruta_destino, tenant_id: $tenant_id})
		MERGE (origen)-[:DEPENDS_ON]->(destino)
	`
	params := map[string]any{
		"ruta_origen":      originPath,
		"ruta_destino":     destinationPath,
		"tenant_id":        tenantID,
	}

	_, err := session.Run(ctx, query, params)
	return err
}
