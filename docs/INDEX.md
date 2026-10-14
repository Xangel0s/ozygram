# Ozygram Documentation Hub

Welcome to the official, modular documentation for **Ozygram**, the persistent cognitive memory operating system and code graph engine for AI coding agents and development assistants.

---

## [DOCUMENTATION DIRECTORY]

### 1. [Dual-Tier Architecture & Outbox (`docs/architecture.md`)](architecture.md)
- Two-lane decoupling: **Fast Lane (Rust)** and **Power Lane (Python)**.
- Monorepo crate breakdown (`crates/ozymem-core`, `crates/ozymem-parser`, `crates/ozymem-server`, `crates/ozymem-cli`, `python/ozy-brain`).
- Single source of transactional truth with local SQLite ACID storage.
- Transactional Outbox Pattern (`memory_outbox`) with native SQLite triggers and asynchronous background draining.
- Resilience: Background daemon auto-spawning and deterministic Circuit Breaker fallback.

### 2. [Hybrid Semantic Search & RRF Fusion (`docs/semantic-search.md`)](semantic-search.md)
- Fusing sparse lexical search (SQLite FTS5 / BM25) and dense semantic search.
- Local `FastEmbed` engine with C++ ONNX Runtime (`BAAI/bge-m3` / `bge-base-en-v1.5`).
- Local vector storage with ChromaDB collections.
- Mathematical **Reciprocal Rank Fusion (RRF)**: formula, ranking weights, and cross-encoder re-ranking.

### 3. [Cognitive Supervision & Deterministic Safety (`docs/supervision-and-validation.md`)](supervision-and-validation.md)
- Co-pilot roles: `SupervisorAgent` and the adversarial critic `RiskCriticAgent`.
- Deterministic zero-LLM validation ($0 API cost, <5 ms):
  - Git Churn telemetry and Hotspot detection with `DuckDB` + `Polars`.
  - Destructive DDL/DML security guards (`DROP TABLE`, `DELETE FROM`, etc.).
  - Blast Radius containment (>8 files threshold).
  - Mathematical memory pruning via exponential time decay ($S = C \cdot e^{-\lambda \Delta t}$).
- Optional semantic supervision with free tiers: Google AI Studio (`Gemini 2.0 Flash`), local Ollama (`qwen2.5-coder`), and OpenRouter.
- Strict token budgeting (*Zero Token Bloat*).

### 4. [Cognitive Evolution Architecture (`docs/ROADMAP_COGNITIVE_EVOLUTION.md`)](ROADMAP_COGNITIVE_EVOLUTION.md)
- **Token Hyper-Optimization (Phase 5)**: `view_skeleton` AST pruning, extractive context windows, session SHA-256 deduplication, and `ozymem stats`.
- **Cross-Language Frontier (Phase 6)**: Bidirectional TSX endpoint consumers (`CONSUMES_ENDPOINT`), SQL DDL table lineage, and debounced file watcher.
- **Speculative Ghost Patching (Phase 7)**: In-memory Copy-on-Write RAM sandbox, caller arity checker, and `ozy_simulate_patch` MCP tool.
- **Dual System Architecture & Offline Consolidation (Phase 8)**: Dual System Router (System 1 fast reflex < 150ms vs System 2 deliberative committee), 3-agent dialectic committee, and proactive outbox diagnostics.

### 5. [Engram System & Speculative Prefill (`docs/engram_system.md`)](engram_system.md)
- Deterministic $O(1)$ symbol contract table using `rkyv` and `memmap2` (~15 ns lookups).
- Predictive prefill to maximize LLM prompt cache hit rates (>90%).
- Test-time sandboxed diff verification (`ozy_verify_diff`).
- Decentralized P2P synchronization with Git Notes (`refs/notes/ozymem`).

### 6. [Dream-RSI & Monte Carlo Tree Search (`docs/dream-rsi.md`)](dream-rsi.md)
- Continuous Recursive Self-Improvement (MCTS) exploration engine.
- Smart auto-parenting in Rust Core: automatic leaf chaining without manual node hash propagation.
- Atomic milestone batch recording (`record_batch`).
- Objective reward auditing (anti-hallucination) and automatic failure-guided self-pruning.
- Deadlock-free trajectory bottleneck diagnostics (`diagnose` and `ozymem dream diagnose`).
- Automatic trajectory resumption (`resume`) for seamless continuation after context compactions.
- Offline counterfactual replay simulation ("dreaming") at $0 LLM token cost.

### 7. [MCP Integration & Tool Reference (`docs/mcp-integration.md`)](mcp-integration.md)
- Setup guide for Antigravity IDE, Claude Code, Cursor, Windsurf, and VS Code.
- Complete catalog of 30+ MCP tools: memory, code graph, cognitive brain, diagnostics, and code health.
- MCP Resources (`ozymem://summary`, `recent-lessons`, etc.) and dynamic resource subscriptions.

### 8. [High-Performance Tooling ("The Dream Team") (`docs/dream-team-tools.md`)](dream-team-tools.md)
- `tgrep` integration (Microsoft): Sub-millisecond trigram regex search across large monorepos.
- `rtk` integration (Rust Token Killer): Payload compression, ANSI stripping, and token budget preservation.
- `fastembed` integration: Local dense vector inference with zero API quota consumption.

### 9. [Version History & Changelog (`docs/changelog.md`)](changelog.md)
- Comprehensive chronological record of all architectural milestones, features, and releases.

---

## [INSTALLATION & SETUP]

### Windows (PowerShell)
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

### Linux & macOS (Bash)
```bash
chmod +x ./install.sh && ./install.sh
```

### Standard MCP Client Configuration
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "ozymem-server",
      "args": []
    }
  }
}
```
