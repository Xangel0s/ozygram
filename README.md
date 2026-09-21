<div align="center">

# ⚡ Ozygram / Ozymem ⚡

### *Persistent Cognitive Memory, Code Graph & Recursive Self-Improvement for AI Agents*

[![Engine](https://img.shields.io/badge/Architecture-Dual--Tier_v1.1.0-blueviolet?style=for-the-badge&logo=rust)](docs/architecture.md)
[![Fast Lane](https://img.shields.io/badge/Fast_Lane-Rust_<5ms-orange?style=for-the-badge&logo=rust)](crates/ozymem-server)
[![Power Lane](https://img.shields.io/badge/Power_Lane-Python_Cognitive-blue?style=for-the-badge&logo=python)](python/ozy-brain)
[![Storage](https://img.shields.io/badge/Storage-SQLite_ACID_+_Outbox-003B57?style=for-the-badge&logo=sqlite)](docs/architecture.md)
[![Vector](https://img.shields.io/badge/Search-FastEmbed_ONNX_+_RRF-success?style=for-the-badge)](docs/semantic-search.md)
[![MCTS](https://img.shields.io/badge/Self--Improvement-Dream--RSI_v1.1.0-purple?style=for-the-badge)](docs/dream-rsi.md)
[![Protocol](https://img.shields.io/badge/Protocol-MCP_Native-green?style=for-the-badge)](docs/mcp-integration.md)

<p align="center">
  <a href="docs/INDEX.md"><strong>Docs Hub</strong></a> &bull;
  <a href="docs/architecture.md"><strong>Architecture</strong></a> &bull;
  <a href="docs/semantic-search.md"><strong>Semantic Search (RRF)</strong></a> &bull;
  <a href="docs/dream-rsi.md"><strong>Dream-RSI (MCTS)</strong></a> &bull;
  <a href="docs/mcp-integration.md"><strong>MCP Reference</strong></a> &bull;
  <a href="docs/engram_system.md"><strong>O(1) Engrams</strong></a> &bull;
  <a href="docs/changelog.md"><strong>Changelog</strong></a>
</p>

</div>

---

> **ozygram** `/ˈoʊ.zi.ɡræm/` — _neuro-symbolic engineering_: the persistent cognitive trace, AST code graph, and offline self-improvement engine for AI coding agents.

Your AI coding agent forgets everything when the session ends. **Ozygram gives it a persistent brain.**

A high-performance **Dual-Tier engine** (ultra-fast Rust server `<5ms` + Python cognitive lane) communicating over native **Model Context Protocol (MCP)** via `stdio`. Zero cloud dependencies, zero external database setup: runs entirely on local SQLite ACID storage, memory-mapped binary contracts, and local FastEmbed ONNX inference at **$0 API cost**.

```text
Agent (Antigravity / Claude Code / Cursor / Windsurf / VS Code / Copilot / ...)
    ↓ MCP stdio (< 5ms)
ozymem-server (Rust Fast Lane)
    ├── SQLite ACID (.ozymem/memory.db) ───► FTS5 BM25 Full-Text Search
    ├── AST Code Graph (Tree-Sitter)   ───► Direct & Transitive Blast Radius
    ├── Deterministic Engrams O(1)     ───► rkyv + memmap2 (~15ns symbol contracts)
    ├── Dream-RSI Engine (MCTS v1.1.0) ───► Auto-parenting, batch milestones & UCT
    └── Transactional Outbox Triggers  ───► ozy-brain (Python Power Lane)
                                                 ├── FastEmbed ONNX Local Dense Vectors
                                                 ├── Reciprocal Rank Fusion (RRF)
                                                 └── DuckDB Git Churn & Hotspot Telemetry
```

---

## 🤖 For Agents (Operating Contract)

Treat Ozygram as a curated, high-precision project brain, not a raw transcript sink:

1. **Orient before writing:** Always call `ozy_context` (or `file_context`) to retrieve the AST symbol summary, adjacent rules, and known gotchas before modifying code.
2. **Search before repeating:** Use `deep_semantic_search` (RRF) or `ozy_memory` before attempting a non-trivial fix or architectural refactor.
3. **Inspect contracts in $O(1)$:** Use `lookup_engram` to check exact function signatures and types in $\approx 15\text{ ns}$ without wasting tokens loading full files.
4. **Track complex decisions with Dream-RSI:** For multi-step tasks or branching experiments, record trajectories with `ozy_exploration` (`start`, `record_step`, or `record_batch`). Rust handles auto-parenting to active leaves and flags objective failures deterministically.
5. **Save significant knowledge deliberately:** Save completed bug fixes, architectural decisions, and project conventions with `record_lesson`, `record_decision`, `record_gotcha`, or `record_convention`.
6. **Audit before applying changes:** Run `ozy_verify_diff` and `audit_changes_with_critic` to prevent DDL/DML accidents (`DROP TABLE`, `TRUNCATE`) and verify cross-file blast radius.
7. **Leave a clean handoff:** Persist session summaries so subsequent turns or compacted contexts resume work instantly.

---

## ✨ Key Features

| Capability | Technical Implementation | Value |
| :--- | :--- | :--- |
| **⚡ Fast Lane (<5ms)** | Native Rust MCP server with memory-mapped indices | Zero agent lag; responses delivered in micro/milliseconds. |
| **🧠 Persistent Memory** | Local SQLite ACID with FTS5 lexical BM25 search | Retains lessons, gotchas, and conventions across sessions. |
| **🔍 Local Hybrid RRF Search** | Reciprocal Rank Fusion (FTS5 + FastEmbed ONNX local) | Deep semantic search with **$0 API cost** and 100% offline privacy. |
| **📐 $O(1)$ Symbol Engrams** | Zero-copy deserialization via `rkyv` + `memmap2` | Instant AST contract prefill; >90% prompt cache hit rate. |
| **🌙 Dream-RSI v1.1.0** | Offline MCTS replay + Auto-parenting + Batch logging | Continuous self-improvement without token tax or manual hash tracking. |
| **🛡️ Safety & Blast Radius** | DuckDB git churn analysis + DDL/DML drop guards | Blocks destructive modifications and flags high-risk regressions. |
| **🚀 The Dream Team** | Bundled with Microsoft `tgrep` & `rtk` (Rust Token Killer) | Trigram regex speeds and aggressive terminal token compression. |

---

## ⚡ Quickstart (1-Minute Install)

### Windows (PowerShell)
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

### Linux / macOS (Bash)
```bash
chmod +x ./install.sh
./install.sh
```

The installer builds the release binaries (`ozymem.exe` and `ozymem-server.exe`), places them in `~/.ozymem/bin/` (added to your `PATH`), and sets up the Python cognitive virtual environment.

---

## ⚙️ MCP Server Setup (`mcpServers`)

Configure Ozygram in your client configuration (`claude_desktop_config.json`, `.gemini/antigravity-ide/mcp_config.json`, Cursor, Windsurf):

### Windows
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "C:\\Users\\YOUR_USER\\.ozymem\\bin\\ozymem-server.exe",
      "args": []
    }
  }
}
```

### Linux / macOS
```json
{
  "mcpServers": {
    "ozygram": {
      "command": "/home/YOUR_USER/.ozymem/bin/ozymem-server",
      "args": []
    }
  }
}
```

---

## 🔌 Core MCP Tools Reference

| Tool | Primary Actions / Params | Description |
| :--- | :--- | :--- |
| **`ozy_context`** | `action: "task"\|"file"\|"project"`, `file_path` | Unified context: AST symbols, adjacent contracts, rules, and lessons. |
| **`lookup_engram`** | `symbol`, `file` | $O(1)$ memory-mapped lookup of signatures and function contracts. |
| **`ozy_memory`** | `action: "record"\|"search"\|"timeline"`, `kind` | Unified memory for lessons, decisions, gotchas, and passive captures. |
| **`deep_semantic_search`** | `query`, `limit: 5` | Hybrid RRF semantic search over project knowledge (FTS5 + FastEmbed). |
| **`ozy_exploration`** | `action: "start"\|"record_step"\|"record_batch"\|"diagnose"` | Dream-RSI v1.1.0 MCTS tree tracking with zero-friction auto-parenting. |
| **`ozy_graph`** | `action: "summary"\|"neighbors"\|"impact"`, `depth` | AST architecture navigation, blast radius, and dependency paths. |
| **`ozy_brain`** | `action: "plan"\|"audit_changes_with_critic"\|"hotspots"` | Cognitive reasoning, 5-phase plans, DuckDB churn, and regression analysis. |
| **`ozy_verify_diff`** | `file_path`, `diff` | Test-time AST diff verification sandbox before saving edits to disk. |
| **`ozy_doctor`** | `format: "text"\|"json"` | System health check (SQLite integrity, vector model, watchers, projects). |

---

## 💻 CLI Cheat Sheet

```bash
# Check system status, active projects, and memory statistics
ozymem status

# Search memory using hybrid full-text search
ozymem search "authentication jwt refresh token"

# Run an offline Dream-RSI counterfactual replay simulation ($0 tokens)
ozymem dream run

# Check the active MCTS exploration policy and visit counts
ozymem dream status

# Diagnose latency and token bottlenecks in a specific trajectory
ozymem dream diagnose traj_5bbd6b4e5c6f

# Verify AST syntax and project health
ozymem doctor
```

---

## 📚 Complete Documentation

Explore the comprehensive modular guides in [`docs/`](docs/INDEX.md):

* 🏛️ [**Architecture (Dual-Tier & Outbox)**](docs/architecture.md): Rust fast lane, Python power lane, and SQLite triggers.
* 🔍 [**Semantic Search & RRF**](docs/semantic-search.md): In-depth guide to local FastEmbed ONNX, ChromaDB, and rank fusion.
* 🌙 [**Dream-RSI & MCTS v1.1.0**](docs/dream-rsi.md): Exploration trees, auto-parenting, batching, reward grounding, and offline replay.
* 🛡️ [**Supervision & Deterministic Safety**](docs/supervision-and-validation.md): DuckDB churn telemetry, DDL/DML guards, and blast radius.
* ⚡ [**$O(1)$ Engrams & Speculative Prefill**](docs/engram_system.md): High-speed binary symbol mapping and prompt cache optimization.
* 🚀 [**The Dream Team (`tgrep`, `rtk`)**](docs/dream-team-tools.md): Trigram regex searching and terminal token compression.
* 🔌 [**MCP Protocol Reference**](docs/mcp-integration.md): Endpoints, JSON schemas, dynamic resources, and client guides.
* 📜 [**Changelog**](docs/changelog.md): Evolution from v0.2.0 to v1.1.0.

---

<div align="center">
  <sub>Built with ❤️ and extreme performance obsession for the AI Agent Era.</sub>
</div>
