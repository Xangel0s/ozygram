package extractor

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os/exec"
)

// ExtractedSymbol represents a parsed function, class, type, struct etc.
type ExtractedSymbol struct {
	Name      string `json:"name"`
	Kind      string `json:"kind"`
	StartLine int    `json:"start_line"`
	EndLine   int    `json:"end_line"`
}

// FileDefinitionMap contains metadata about symbols mapped inside a file.
type FileDefinitionMap struct {
	FilePath  string            `json:"file_path"`
	Language  string            `json:"language"`
	Strategy  string            `json:"strategy"`
	Functions []ExtractedSymbol `json:"functions"`
}

// DependencyHint represents a reference or import in a file.
type DependencyHint struct {
	FilePath  string `json:"file_path"`
	Kind      string `json:"kind"`
	Label     string `json:"label"`
	RawText   string `json:"raw_text"`
	StartLine int    `json:"start_line"`
	EndLine   int    `json:"end_line"`
}

// ParseResult holds the parsed definition map and dependency hints.
type ParseResult struct {
	DefinitionMap   FileDefinitionMap `json:"definition_map"`
	DependencyHints []DependencyHint  `json:"dependency_hints"`
}

// Extractor executes the Rust CLI to parse source files.
type Extractor struct {
	binaryPath string
}

// NewExtractor initializes the extractor with the path to the ozymem binary.
func NewExtractor(binaryPath string) *Extractor {
	return &Extractor{binaryPath: binaryPath}
}

// ParseFile runs the parser command and returns the JSON output mapped to Go structs.
func (e *Extractor) ParseFile(filePath string) (*ParseResult, error) {
	cmd := exec.Command(e.binaryPath, "parse", filePath)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err := cmd.Run()
	if err != nil {
		return nil, fmt.Errorf("failed to run parser cli (stderr: %s): %w", stderr.String(), err)
	}

	var result ParseResult
	err = json.Unmarshal(stdout.Bytes(), &result)
	if err != nil {
		return nil, fmt.Errorf("failed to parse JSON result: %w (raw output: %s)", err, stdout.String())
	}

	return &result, nil
}
