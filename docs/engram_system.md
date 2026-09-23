# Deterministic Engram System, Speculative Prefill & Test-Time Validation

This document details the architectural design of **Engrams**, **Speculative Context Prefill**, **Test-Time Sandbox Validation**, and **P2P Decentralized Sync via Git Notes** in Ozygram.

---

## 1. Deterministic $O(1)$ Storage with `rkyv` & `memmap2`

### Motivation
For massive codebases or high-frequency editing sessions by AI agents, querying signatures and symbol contracts through standard SQLite queries or on-the-fly text parsing introduces cumulative roundtrip latency.

### Implementation
- **`IncrementalEngramStore` / `FastEngramReader`**: Zero-copy binary storage engine based on `rkyv` mapped into virtual memory via `memmap2`.
- **Contract Schema (`EngramContract`)**:
  - `symbol_path`: Canonical qualified path (`crate::module::func`).
  - `signature_hash`: BLAKE3/SHA-256 signature hash for instant invalidation checks.
  - `input_types` & `return_type`: Normalized input argument and return types.
  - `doc_summary`: Semantic docstring summary of the contract.
  - `outgoing_calls` / `type_dependencies`: Direct links to dependent types and callsites.
- **Performance**: Symbol lookups execute in $\approx 15\text{ ns}$ with constant $O(1)$ memory usage and zero heap allocations.

---

## 2. Predictive Prefill & Speculative Decoding

### Motivation
AI agents typically require multiple consecutive roundtrips (`get_file_context` $\rightarrow$ `find_symbol` $\rightarrow$ `lookup_engram`) to discover type contracts across imported modules.

### Implementation
- When calling `get_file_context` or `context_for_task`, `ozymem-core` leverages the `petgraph` dependency graph to identify immediate 1st-order neighbor files and recent modifications via `GitBackend`.
- The MCP server injects a deterministic `[ENGRAM_CACHE: Deterministic Symbol Contracts]` header containing high-probability adjacent symbol contracts ahead of the target file body.
- **Benefit**: Eliminates redundant agent tool calls and maximizes prefix **Prompt Cache** hit rates (>90%) on models with prompt caching support (Claude 3.7 / GPT-4o / DeepSeek V3).

---

## 3. Closed-Loop Test-Time Validation Sandbox (`ozy_verify_diff`)

### Verification Pipeline
```text
┌─────────────────────────────────────────────────────────────┐
│                    MCP Agent / Tool Call                    │
│                     `ozy_verify_diff`                       │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                 Sandbox Fast-Path (Rust)                    │
│  - Tree-Sitter syntax verification                          │
│  - Engram contract validation                               │
│  - AST integrity check without modifying target file on disk │
└──────────────────────────────┬──────────────────────────────┘
                               │ (If warnings or changes detected)
                               ▼
┌─────────────────────────────────────────────────────────────┐
│                Heuristic Reflection (Python)                │
│  - `reflector.py` isolates root causes                      │
│  - Procedural rule extraction: [TRIGGER] -> [ACTION]        │
└─────────────────────────────────────────────────────────────┘
```

- **`ozy_verify_diff` Tool**: Allows an AI agent to submit proposed patches or unified diffs for verification prior to writing changes to disk.
- **Fast Detection**: Detects broken contracts, type incompatibilities, and AST syntax parse errors in $< 200\text{ ms}$.

---

## 4. Decentralized P2P Sync with Git Notes (`refs/notes/ozymem`)

### Motivation
Enable multiple coding agents, subagents, or developer branches to share procedural rules, lessons, and architectural insights without deploying external database services.

### Workflow
- **Export (`ozy_export_memory_notes`)**: Bundles episodic lessons, engram contracts, and procedural rules into a JSON payload and writes it to `refs/notes/ozymem` attached to the current Git commit.
- **Import (`ozy_import_memory_notes`)**: Reads the note at the specified or HEAD commit, deduplicates entries against local SQLite storage, and merges new lessons automatically.
- **Portability**: Synchronizes seamlessly via standard Git commands: `git push origin refs/notes/ozymem` and `git fetch origin refs/notes/ozymem:refs/notes/ozymem`.
