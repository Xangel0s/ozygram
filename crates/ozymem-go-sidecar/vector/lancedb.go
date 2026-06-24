package vector

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"math"
	"net/http"
	"os"
	"path/filepath"
	"sync"
)

// VectorRecord represents a code snippet and its embedding vector.
type VectorRecord struct {
	ID        string    `json:"id"`
	FilePath  string    `json:"file_path"`
	Symbol    string    `json:"symbol"`
	Code      string    `json:"code"`
	Embedding []float32 `json:"embedding"`
}

// VectorDB represents a pure-Go embedded vector store.
type VectorDB struct {
	mu       sync.Mutex
	filePath string
	records  []VectorRecord
}

func NewVectorDB(dirPath string) (*VectorDB, error) {
	err := os.MkdirAll(dirPath, 0755)
	if err != nil {
		return nil, err
	}

	dbPath := filepath.Join(dirPath, "vectors.json")
	db := &VectorDB{
		filePath: dbPath,
		records:  []VectorRecord{},
	}

	// Load existing records if they exist
	if _, err := os.Stat(dbPath); err == nil {
		data, err := os.ReadFile(dbPath)
		if err == nil {
			_ = json.Unmarshal(data, &db.records)
		}
	}

	return db, nil
}

// Upsert adds or updates a vector record.
func (db *VectorDB) Upsert(rec VectorRecord) error {
	db.mu.Lock()
	defer db.mu.Unlock()

	found := false
	for i, r := range db.records {
		if r.ID == rec.ID {
			db.records[i] = rec
			found = true
			break
		}
	}
	if !found {
		db.records = append(db.records, rec)
	}

	data, err := json.MarshalIndent(db.records, "", "  ")
	if err != nil {
		return err
	}

	return os.WriteFile(db.filePath, data, 0644)
}

// Search similarity.
func (db *VectorDB) Search(queryVector []float32, limit int) []VectorRecord {
	db.mu.Lock()
	defer db.mu.Unlock()

	type match struct {
		record VectorRecord
		score  float32
	}

	var matches []match
	for _, rec := range db.records {
		score := CosineSimilarity(queryVector, rec.Embedding)
		matches = append(matches, match{record: rec, score: score})
	}

	// Sort matches descending by score
	for i := 0; i < len(matches); i++ {
		for j := i + 1; j < len(matches); j++ {
			if matches[i].score < matches[j].score {
				matches[i], matches[j] = matches[j], matches[i]
			}
		}
	}

	if limit > len(matches) {
		limit = len(matches)
	}

	result := make([]VectorRecord, limit)
	for i := 0; i < limit; i++ {
		result[i] = matches[i].record
	}
	return result
}

// CosineSimilarity computes cosine similarity between two vectors.
func CosineSimilarity(a, b []float32) float32 {
	if len(a) != len(b) || len(a) == 0 {
		return 0.0
	}
	var dotProduct, normA, normB float64
	for i := 0; i < len(a); i++ {
		dotProduct += float64(a[i]) * float64(b[i])
		normA += float64(a[i]) * float64(a[i])
		normB += float64(b[i]) * float64(b[i])
	}
	if normA == 0 || normB == 0 {
		return 0.0
	}
	return float32(dotProduct / (math.Sqrt(normA) * math.Sqrt(normB)))
}

// EmbeddingsGenerator generates vector embeddings.
type EmbeddingsGenerator struct {
	apiKey string
}

func NewEmbeddingsGenerator(apiKey string) *EmbeddingsGenerator {
	return &EmbeddingsGenerator{apiKey: apiKey}
}

// GenerateMockEmbedding creates a deterministic dummy vector for testing when no API key is supplied.
func (eg *EmbeddingsGenerator) GenerateMockEmbedding(text string) []float32 {
	dimension := 1536
	vector := make([]float32, dimension)
	var hash int
	for _, c := range text {
		hash = int(c) + (hash << 5) - hash
	}
	for i := 0; i < dimension; i++ {
		vector[i] = float32(math.Sin(float64(hash+i))) / float32(i+1)
	}
	return vector
}

// GenerateEmbedding calls Gemini Embeddings API if apiKey is provided; otherwise returns mock.
func (eg *EmbeddingsGenerator) GenerateEmbedding(ctx context.Context, text string) ([]float32, error) {
	if eg.apiKey == "" {
		return eg.GenerateMockEmbedding(text), nil
	}

	// Example request to Google Gemini API
	url := fmt.Sprintf("https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:embedContent?key=%s", eg.apiKey)
	reqBody, _ := json.Marshal(map[string]any{
		"model": "models/text-embedding-004",
		"content": map[string]any{
			"parts": []map[string]any{
				{"text": text},
			},
		},
	})

	req, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewReader(reqBody))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("gemini API returned status %d", resp.StatusCode)
	}

	var res struct {
		Embedding struct {
			Values []float32 `json:"values"`
		} `json:"embedding"`
	}

	err = json.NewDecoder(resp.Body).Decode(&res)
	if err != nil {
		return nil, err
	}

	return res.Embedding.Values, nil
}

// Support bytes.NewReader
type bytesReaderCloser struct {
	*bytes.Reader
}

func (b bytesReaderCloser) Close() error { return nil }

var _ = bytesReaderCloser{}
