# Ozygram Official Documentation

Welcome to the official, modular documentation for **Ozygram** (Dual-Tier Engine v1.1.0), the persistent cognitive memory operating system and code graph engine for AI coding agents and development assistants.

---

## 📚 Documentation Index

### 1. [Dual-Tier Architecture (`docs/architecture.md`)](architecture.md)
- Two-lane decoupling: **Fast Lane (Rust)** and **Power Lane (Python)**.
- Monorepo crate breakdown (`crates/`, `python/`).
- Single source of transactional truth with local SQLite ACID storage.
- Transactional Outbox Pattern (`memory_outbox`) with native SQLite triggers and asynchronous draining.
- Resilience: Background daemon auto-spawning and deterministic Circuit Breaker fallback.

### 2. [Hybrid Semantic Search & RRF Fusion (`docs/semantic-search.md`)](semantic-search.md)
- Fusing sparse lexical search (SQLite FTS5 / BM25) and dense semantic search.
- Local `FastEmbed` engine with C++ ONNX Runtime (`BAAI/bge-m3` / `bge-base-en-v1.5`).
- Local vector storage with ChromaDB collections.
- Mathematical **Reciprocal Rank Fusion (RRF)**: formula, ranking weights, and benefits.

### 3. [Cognitive Supervision & Deterministic Validation (`docs/supervision-and-validation.md`)](supervision-and-validation.md)
- Co-pilot roles: `SupervisorAgent` and the adversarial critic `RiskCriticAgent`.
- Deterministic zero-LLM validation ($0 API cost, <5 ms):
  - Git Churn telemetry and Hotspot detection with `DuckDB` + `Polars`.
  - Destructive DDL/DML security guards (`DROP TABLE`, `DELETE FROM`, etc.).
  - Blast Radius containment (>8 files threshold).
  - Mathematical memory pruning via exponential time decay ($S = C \cdot e^{-\lambda \Delta t}$).
- Optional semantic supervision with free tiers: Google AI Studio (`Gemini 2.0 Flash`), local Ollama (`qwen2.5-coder`), and OpenRouter.
- Strict token budgeting (*Zero Token Bloat*).

### 4. [High-Performance Tooling ("The Dream Team") (`docs/dream-team-tools.md`)](dream-team-tools.md)
- `tgrep` integration (Microsoft): Sub-millisecond trigram regex search across large monorepos.
- `rtk` integration (Rust Token Killer): Payload compression, ANSI stripping, and token budget preservation.
- `fastembed` integration: Local dense vector inference with zero API quota consumption.

### 5. [MCP Integration & Tool Reference (`docs/mcp-integration.md`)](mcp-integration.md)
- Setup guide for Antigravity IDE, Claude Desktop, Cursor, and VS Code.
- Complete catalog of MCP tools: memory, code graph, cognitive brain, diagnostics, and code health.
- MCP Resources (`ozymem://summary`, `recent-lessons`, etc.) and dynamic resource subscriptions.

### 6. [Engram System & Speculative Prefill (`docs/engram_system.md`)](engram_system.md)
- Deterministic $O(1)$ symbol contract table using `rkyv` and `memmap2` (~15 ns lookups).
- Predictive prefill to maximize LLM prompt cache hit rates (>90%).
- Test-time sandboxed diff verification (`ozy_verify_diff`).
- Decentralized P2P synchronization with Git Notes (`refs/notes/ozymem`).

### 7. [Dream-RSI & Monte Carlo Tree Search v1.1.0 (`docs/dream-rsi.md`)](dream-rsi.md)
- Continuous Recursive Self-Improvement (MCTS) exploration engine.
- Smart auto-parenting in Rust Core: automatic leaf chaining without manual node hash propagation.
- Atomic milestone batch recording (`record_batch`).
- Objective reward auditing (anti-hallucination) and automatic failure-guided self-pruning.
- Deadlock-free trajectory bottleneck diagnostics (`diagnose` and `ozymem dream diagnose`).
- Automatic trajectory resumption (`resume`) for seamless continuation after context compactions.
- Offline counterfactual replay simulation ("dreaming") at $0 LLM token cost.

### 8. [Changelog & Release Notes (`docs/changelog.md`)](changelog.md)
- Comprehensive chronological log of all architectural milestones and enhancements from v0.2.0 to v1.1.0.

---

## ⚡ Quick Start Installation

### Windows (PowerShell)
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

### Linux / macOS (Bash)
```bash
chmod +x ./install.sh
./install.sh
```

### Basic MCP Configuration
```json
{
  "mcpServers": {
    "ozymem": {
      "command": "ozymem-server",
      "args": []
    }
  }
}
```
