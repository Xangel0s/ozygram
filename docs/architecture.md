# Ozygram Dual-Tier Architecture

Ozygram implements an **Asymmetric Dual-Tier Architecture** designed to maximize coding assistant response velocity, eliminate startup latency, and ensure that critical IDE operations are never blocked by heavy cognitive computations.

---

## 1. Overview: The Two Lanes

```text
       ┌────────────────────────────────────────────────────────┐
       │             MCP Client (IDE / LLM Assistant)           │
       └──────────────────────────┬─────────────────────────────┘
                                  │ JSON-RPC (Stdio)
                                  ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │                   FAST LANE (Rust Core & MCP)                          │
 │                                                                        │
 │  • Ultra-low latency MCP Server (< 5ms)                                │
 │  • Persistent ACID storage in local SQLite                             │
 │  • Dependency graph with Petgraph and native AST (Tree-Sitter)         │
 │  • Deterministic O(1) Engram Table with rkyv + memmap2                 │
 │  • Transactional Outbox Pattern (SQLite Triggers)                      │
 │  • Circuit Breaker and Auxiliary Daemon Auto-Spawn                     │
 └───────────────────┬────────────────────────────────┬───────────────────┘
                     │ Triggers                       │ Fallback / RPC
                     ▼                                ▼
       ┌──────────────────────────┐     ┌─────────────────────────────────┐
       │   memory_outbox Table    │     │      POWER LANE (Python)        │
       │   (Transactional SQLite) │     │                                 │
       └─────────────┬────────────┘     │  • SupervisorAgent              │
                     │ Async            │  • RiskCriticAgent              │
                     ▼ Consumption      │  • DataEngine (DuckDB + Polars) │
       ┌──────────────────────────┐     │  • OutboxConsumer & FastEmbed   │
       │   ONNX Vector Engine     │◄────┤  • Local ChromaDB               │
       │   (bge-m3 / FastEmbed)   │     │  • Exponential Time Decay       │
       └──────────────────────────┘     └─────────────────────────────────┘
```

---

## 2. Monorepo Structure

```text
ozygram/
├── crates/
│   ├── ozymem-core/       # Persistent storage (SQLite), Outbox triggers, Petgraph graph, Engram store
│   ├── ozymem-parser/     # Native multi-language Tree-Sitter AST parsers (Rust, Python, TS/JS, Go, SQL)
│   ├── ozymem-cli/        # Standalone CLI with subcommands (scan, dashboard, projects, etc.)
│   └── ozymem-server/     # High-concurrency, low-latency stdio MCP server (< 5ms)
├── python/
│   └── ozy-brain/         # Cognitive engine: Supervisor, RiskCritic, DataEngine (DuckDB), OutboxConsumer
└── docs/                  # Modular technical documentation by domain
```

---

## 3. Fast Lane (Rust)

The fast lane consists of `ozymem-core`, `ozymem-parser`, `ozymem-server`, and `ozymem-cli`:

1. **Sub-Millisecond Latency**:
   - Responds immediately to IDE context requests.
   - Signature lookups, dependency checks, and file rule reads resolve in $\approx 15\text{ ns}$ using memory-mapped zero-copy deserialization (`memmap2` and `rkyv`).
2. **Single Transactional Authority**:
   - SQLite (`{project}/.ozymem/memory.db`) is the **Single Source of Truth** (SSOT).
   - No write operation depends on an external service or running Python daemon. If Python is unresponsive or not installed, Rust persistence operates without degradation.
3. **Hot Syntactic Indexing**:
   - Multi-language AST parser (Rust, TypeScript, JavaScript, Python, Go, SQL) extracts functions, structs, classes, and HTTP routes without requiring external language runtimes or compilers.

---

## 4. Transactional Outbox Pattern (`memory_outbox`)

To prevent phantom vectors and desynchronization between relational state and the vector database, Ozygram implements a **Transactional Outbox** pattern governed by native SQLite triggers:

### Table Schema
```sql
CREATE TABLE IF NOT EXISTS memory_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,       -- 'UPSERT' or 'DELETE'
    entity_type TEXT NOT NULL,      -- 'lesson' or 'observation'
    entity_id INTEGER NOT NULL,     -- ID in source table
    payload TEXT NOT NULL,          -- JSON-serialized entity payload
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    processed_at TIMESTAMP NULL     -- NULL = pending vectorization
);
```

### Automatic Triggers
- **Insert & Update**: Whenever a lesson or observation is created or modified in SQLite, an automated trigger inserts an `UPSERT` event into `memory_outbox`.
- **Delete / Soft-Delete**: Whenever a record is deleted from SQLite, a corresponding `DELETE` event is queued in the outbox.

### Outbox Pattern Advantages
- **Full Atomicity**: If a database transaction performs a `ROLLBACK`, the vectorization event never reaches the outbox.
- **Zero Orphan Vectors**: ChromaDB never retains memories that have been deleted from the primary database.
- **Temporal Decoupling**: Rust commits to disk in 1ms; Python handles dense embeddings in background batches without impacting user interactions.

---

## 5. Power Lane (Python `ozy-brain`)

The auxiliary Python engine handles complex analytical tasks leveraging the modern data science and ML ecosystem:

1. **`OutboxConsumer`**:
   - Background daemon thread (10s polling interval) that flushes pending events from `memory_outbox`.
   - Generates local dense embeddings via `FastEmbed` and synchronizes `ChromaDB` collections.
2. **`SupervisorAgent` & `RiskCriticAgent`**:
   - Orchestrates adversarial auditing of implementation plans and proposed code diffs.
   - Operates in offline deterministic mode by default; evaluates subtle regression vectors with configured LLM providers.
3. **`DataEngine` (DuckDB + Polars)**:
   - Analyzes Git commit history and file modifications to compute *Churn* scores, detect hotspot files prone to regression, and correlate architectural risk.
4. **`MemoryConsolidationAgent`**:
   - Applies exponential time decay mathematical formulas ($S = C \cdot e^{-\lambda \Delta t}$) to prune stale or superseded memories without burning LLM tokens.

---

## 6. Resilience: Daemon Auto-Spawn & Circuit Breaker

Communication between the Rust MCP server and the Python engine features automated self-healing mechanisms:

### Daemon Auto-Spawn (`ensure_ozy_brain_running`)
When the Rust server receives a cognitive or deep semantic search request (`ozy_brain` or `deep_semantic_search`) and the Python daemon is inactive:
1. Automatically detects the local project virtual environment or system Python runtime.
2. Spawns the `ozy-brain` process as a detached background worker (`DETACHED_PROCESS` on Windows).
3. Awaits RPC health-check confirmation before routing the payload.

### Circuit Breaker & Deterministic Fallback
If the Python process fails to respond, encounters an exception, or exceeds the timeout threshold (3s):
- The **Circuit Breaker** triggers immediately.
- Returns a deterministic fallback result computed via **SQLite FTS5 + BM25 local ranking**.
- The MCP agent receives a valid response without catastrophic crashes or unhandled client exceptions.

---

## 7. Property Graph Memory Architecture (v1.3.0)

Ozygram v1.3.0 transitions from flat, isolated lesson records to a **Property Graph Memory Architecture**. Memories, files, symbols, and exploration milestones are modeled as vertices in a connected knowledge network with typed, directed edges.

```text
                  ┌────────────────────────┐
                  │      Memory Node       │
                  │  [RULE: Campaign Sync] │
                  └─────┬────────────┬─────┘
           APPLIES_TO   │            │   COUPLED_WITH
         (weight: 1.0)  │            │  (weight: 0.9)
                        ▼            ▼
   ┌───────────────────────┐      ┌────────────────────────┐
   │       File Node       │      │       File Node        │
   │   (Backend Router)    │      │    (Frontend Form)     │
   │       router.py       │      │     OrdenForm.tsx      │
   └───────────────────────┘      └────────────────────────┘
```

### 1. Vertices & Typed Edges
- **Nodes (`memory_nodes`)**: Persisted in SQLite with FTS5 virtual indexing and mirrored in RAM (`petgraph::DiGraph`).
- **Edges (`memory_edges`)**:
  - `APPLIES_TO`: Direct link between a rule/gotcha/convention and target source file or symbol.
  - `COUPLED_WITH`: Cross-layer dependency between components (e.g. backend endpoint coupled with frontend form).
  - `CAUSES_REGRESSION`: Flags modifications that triggered regressions or test failures.
  - `SUPERSEDES`: Directional replacement of deprecated or conflicting directives.
  - `REINFORCES`: Synergistic consolidation between mutually supporting memories.
  - `DERIVED_FROM`: Provenance link tracking code generation or refactoring origins.

### 2. Multi-Hop BFS with Distance Decay
- When querying `ozy_context` or `ozy_graph(action="memory_neighborhood")`, the topological engine traverses in-memory petgraph with depth $\le 2$.
- Applies an exponential decay factor: $\text{Weight}_{\text{eff}} = \text{Weight}_{\text{base}} \times 0.6^{\text{depth}-1}$. Direct rules remain at full weight (1.0), while secondary coupled components are injected at attenuated weight (~0.54) to reveal blast radius without context bloat.

### 3. Cascade Stale Invalidation
- When a file modification is detected on disk, `propagate_stale_invalidation` traverses incoming `APPLIES_TO` edges to flag associated memories as `[ALERT: STALE_MEMORY]`.
- The invalidation cascades recursively through `SUPERSEDES` and `DERIVED_FROM` outgoing edges, halving confidence scores and ensuring the LLM never relies on outdated guidance.

### 4. Autonomous Cognitive Auto-Wiring
- During offline consolidation (`ozymem dream run`), `auto_wire_memories` calculates semantic distance across memory clusters.
- If similarity exceeds threshold ($\ge 0.80$), `RiskCriticArbitrator` evaluates timestamps and deprecation keywords:
  - Mutually compatible memories receive a `REINFORCES` edge.
  - Contradictory or obsolete rules are resolved with a `SUPERSEDES` edge, automatically suppressing dead conventions from prompt prefill.

### 5. Emoji-Free Mermaid Visualization
- Interactive visualization available via CLI `ozymem dream graph [--module <name>]` and MCP `ozy_graph(action="render_mermaid")`.
- Clean, syntax-compliant Markdown output with textual badges (`[CONVENTION]`, `[APPLIES_TO]`, `[COUPLED_WITH]`).

