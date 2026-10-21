<div align="center">

# Ozygram

### *Persistent Cognitive Memory, Code Graph & Autonomous Self-Improvement for AI Coding Agents*

[![Release](https://img.shields.io/badge/Release-v0.1.6-blueviolet?style=for-the-badge&logo=github)](https://github.com/Xangel0s/ozygram/releases)
[![Fast Lane](https://img.shields.io/badge/Fast_Lane-Rust_<5ms-orange?style=for-the-badge&logo=rust)](crates/ozymem-server)
[![Standalone Brain](https://img.shields.io/badge/Brain-Standalone_Zero--Python-blue?style=for-the-badge)](python/ozy-brain)
[![Storage](https://img.shields.io/badge/Storage-SQLite_ACID_+_Outbox-003B57?style=for-the-badge&logo=sqlite)](docs/architecture.md)
[![Vector](https://img.shields.io/badge/Search-FastEmbed_ONNX_+_RRF-success?style=for-the-badge)](docs/semantic-search.md)
[![Self-Improvement](https://img.shields.io/badge/MCTS-Dream--RSI-purple?style=for-the-badge)](docs/dream-rsi.md)
[![Platform](https://img.shields.io/badge/Platform-Windows_|_Linux_|_macOS-lightgrey?style=for-the-badge)](https://github.com/Xangel0s/ozygram)
[![License](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)](LICENSE)

<br/>

<p align="center">
  <a href="docs/INDEX.md"><strong>Docs Hub</strong></a> &bull;
  <a href="#quickstart"><strong>Quickstart</strong></a> &bull;
  <a href="docs/architecture.md"><strong>Architecture</strong></a> &bull;
  <a href="#works-with-your-agent"><strong>Agent Setup</strong></a> &bull;
  <a href="#mcp-tools-reference"><strong>MCP Tools</strong></a> &bull;
  <a href="docs/ROADMAP_COGNITIVE_EVOLUTION.md"><strong>Cognitive Evolution</strong></a> &bull;
  <a href="docs/changelog.md"><strong>Changelog</strong></a>
</p>

<br/>

<p>
<strong>ozygram</strong> <code>/ˈoʊ.zi.ɡræm/</code> — <em>neuro-symbolic engineering</em>: the persistent cognitive trace, AST code graph, and offline self-improvement engine for AI coding agents.
</p>

<p>
Stop restarting from zero.<br/>
<strong>Empower your AI agent with continuous, self-improving intelligence.</strong>
</p>

<p><sub>A neuro-symbolic engine that maps code graphs in microseconds, simulates patches in RAM before touching disk, and dreams offline via Dream-RSI to turn past mistakes into permanent architectural instincts at $0 API cost.</sub></p>

</div>

---

<div align="center">

<sub><strong>WORKS WITH THE AGENT YOU ALREADY USE</strong></sub><br/><br/>

<strong><a href="#antigravity-ide">Antigravity IDE</a></strong> &bull;
<strong><a href="#claude-code--desktop">Claude Code / Desktop</a></strong> &bull;
<strong><a href="#cursor">Cursor</a></strong> &bull;
<strong><a href="#windsurf">Windsurf</a></strong> &bull;
<strong><a href="#vs-code-cline--copilot">VS Code (Cline / Copilot)</a></strong> &bull;
<strong><a href="#gemini-cli">Gemini CLI</a></strong> &bull;
<strong><a href="#codex--qwen">Codex / Qwen</a></strong> &bull;
<strong><a href="#aider">Aider</a></strong>

<sub>Compatible with any client implementing the Model Context Protocol (MCP) via stdio &bull; <a href="#agent-configuration">Quick Setup Guide &rarr;</a></sub>

</div>

---

## [ARCH: DUAL-TIER MODEL]

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│  AI Coding Agent (Antigravity / Claude Code / Cursor / Windsurf / VS Code / ...)        │
└───────────────────────────────────────────┬────────────────────────────────────────────┘
                                            │ MCP stdio JSON-RPC (< 5ms)
┌───────────────────────────────────────────▼────────────────────────────────────────────┐
│  FAST LANE: Rust Core (ozymem-server & ozymem-cli)                                     │
│  • SQLite ACID WAL Storage (.ozymem/memory.db) with Transactional Outbox Triggers      │
│  • Native AST Code Graph (Tree-Sitter: Rust, Python, TS/TSX, JS, Go, SQL)              │
│  • O(1) Binary Engram Contracts (rkyv + memmap2 zero-copy, ~15ns lookups)              │
│  • In-Memory Speculative Ghost Patch Sandbox (Copy-on-Write RAM, caller arity checker)  │
│  • Live File Watcher (notify debounced incremental indexing)                           │
│  • Token Hyper-Optimization (view_skeleton AST pruning, LOD 0/1/2, extractive windows) │
└───────────────────────────────────────────┬────────────────────────────────────────────┘
                                            │ Transactional Outbox (SQLite WAL / IPC)
┌───────────────────────────────────────────▼────────────────────────────────────────────┐
│  POWER LANE: Python Cognitive Engine (ozy-brain)                                       │
│  • Dense Vector Inference (FastEmbed ONNX local) + SQLite FTS5 BM25 Lexical Ranking    │
│  • Reciprocal Rank Fusion (RRF) Semantic Search with Cross-Encoder Neural Re-Ranker     │
│  • Dual System Cognitive Router (System 1 fast reflex < 150ms vs System 2 deliberative)│
│  • 3-Agent Dialectic Committee (Implementer + Risk Critic + Supervisor consensus)     │
│  • Proactive Diagnostic Consumer (instant compiler & runtime repair prescriptions)     │
│  • Offline Dream-RSI Consolidation (MCTS replay, postmortem vaccines, temporal decay)  │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## [INTERACTIVE: CAPABILITY ROUTER]

Find the exact capability, technical mechanism, and documentation for your task:

| What You Need | Technical Mechanism | Documentation | Recommended Tool / Command |
| :--- | :--- | :--- | :--- |
| **Search past fixes, gotchas & rules** | Hybrid Reciprocal Rank Fusion (FTS5 BM25 + FastEmbed ONNX) | [Semantic Search](docs/semantic-search.md) | `ozy_memory(action="search")` |
| **Inspect symbol contracts without loading full files** | Zero-copy $O(1)$ `rkyv` memory-mapped contract table | [Engram System](docs/engram_system.md) | `ozy_lookup_engram` / `ozymem symbol` |
| **Analyze Blast Radius before editing code** | Inverted directional AST import traversal & reverse dependencies | [Supervision Guide](docs/supervision-and-validation.md) | `ozy_graph(action="incoming_dependencies")` |
| **Pre-flight test code patches safely in RAM** | Ephemeral Copy-on-Write RAM sandbox & caller arity validator | [Cognitive Evolution](docs/ROADMAP_COGNITIVE_EVOLUTION.md) | `ozy_simulate_patch` |
| **Surgical AST exploration with 80% token savings** | Tree-Sitter method skeletonization with multi-level LOD | [Token Optimization](docs/ROADMAP_COGNITIVE_EVOLUTION.md) | `view_skeleton` / `ozy_file_tree` |
| **Audit SQL migrations for safety & idempotency** | AST DDL idempotency linter (`IF NOT EXISTS`, drop guards) | [Resilient Backend](docs/architecture.md) | `ozy_doctor(action="audit_migrations")` |
| **End-to-end frontend to backend tracing** | Bidirectional Next.js/TSX endpoint resolution & SQL lineage | [Cross-Language](docs/ROADMAP_COGNITIVE_EVOLUTION.md) | `ozy_graph(action="endpoint_dependents")` |
| **Deliberate high-risk code changes with AI consensus** | 3-agent dialectic committee (Implementer + Critic + Supervisor) | [Dialectic Engine](docs/ROADMAP_COGNITIVE_EVOLUTION.md) | `dialectic_committee` |
| **Autonomous offline self-improvement** | MCTS counterfactual replay dreaming & temporal memory decay | [Dream-RSI Guide](docs/dream-rsi.md) | `ozymem dream run` |

---

## [QUICKSTART: 1-MINUTE INSTALL]

### 1-Click Autonomous Binary (`ozygram.exe`)
Download `ozygram.exe` (Windows) or `ozygram` (Linux / macOS) from the latest [Release](https://github.com/Xangel0s/ozygram/releases) and execute it (or simply double-click on Windows):
- Auto-deploys binaries to `~/.ozymem/bin/`.
- Automatically injects `~/.ozymem/bin` into your User `PATH`.
- Auto-detects and writes the MCP configuration directly into **Antigravity IDE**, **Claude Desktop**, **Cursor**, **Windsurf**, and **VS Code**.
- Prints the verified JSON configuration for any other custom MCP client.

### Windows (PowerShell Script)
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```
*Or download and execute directly via PowerShell:*
```powershell
irm https://raw.githubusercontent.com/Xangel0s/ozygram/main/install.ps1 | iex
```

### Linux & macOS (Bash)
```bash
chmod +x ./install.sh && ./install.sh
```
*Or download directly via curl:*
```bash
curl -fsSL https://raw.githubusercontent.com/Xangel0s/ozygram/main/install.sh | bash
```

The installer installs the native binaries (`ozymem`, `ozygram`, `ozymem-server`, and the standalone `ozy-brain` runtime) to `~/.ozymem/bin/`, adds it to `PATH`, and automatically runs `ozymem init --auto` to configure your installed AI editors.

> [!TIP]
> **Zero-Python Standalone Runtime**: `ozy-brain` is distributed as a self-contained executable. Developers no longer need Python, DuckDB, or pip installed on their systems.

---

## [ONE-CLICK SETUP: OZY INIT]

Ozygram includes an interactive and autonomous setup wizard that detects your installed AI editors and automatically injects the MCP configuration:

```bash
ozymem init
# or simply:
ozy init
```

To run in completely hands-off auto mode (recommended for CI or quick install):
```bash
ozymem init --auto
```

This automatically scans, detects, and configures:
- **Cursor** (Global `~/.cursor/mcp.json` and Workspace `.cursor/mcp.json`)
- **Antigravity IDE** (`~/.gemini/antigravity-ide/mcp_config.json`)
- **Claude Desktop** (`claude_desktop_config.json` on macOS/Windows)
- **Windsurf** (`~/.codeium/windsurf/mcp_config.json`)
- **VS Code** (Cline `cline_mcp_settings.json` and Roo Code `mcp_settings.json`)
- **Zed** (`~/.config/zed/settings.json`)

---

## [MANUAL MCP CONFIGURATION]

If you prefer to configure your client manually, add Ozygram to your agent's MCP settings configuration:

### Antigravity IDE
File: `~/.gemini/antigravity-ide/mcp_config.json`
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

### Claude Code / Desktop
File: `claude_desktop_config.json` (or `~/.claude/mcp_config.json`)
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

### Cursor
File: `~/.cursor/mcp.json`
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

### Windsurf & VS Code (Cline / Copilot)
File: `cline_mcp_settings.json` or VS Code MCP settings:
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

---

## [MCP TOOLS REFERENCE]

Ozygram provides a comprehensive suite of 30+ native MCP tools organized by cognitive domain:

### 1. Memory & Knowledge Layer
| Tool | Primary Parameters | Description |
| :--- | :--- | :--- |
| `ozy_context` | `action`, `file_path` | Unified context: AST symbols, adjacent rules, gotchas, and lessons. |
| `ozy_memory` | `action`, `query`, `kind`, `title`, `content` | Store, query, and rank persistent memories, decisions, and lessons. |
| `ozy_lookup_engram` | `symbol`, `file` | $O(1)$ zero-copy signature and contract lookup (~15ns). |
| `ozy_exploration` | `action`, `trajectory_id`, `objective` | Dream-RSI MCTS tree tracker with automatic leaf parenting and UCT scoring. |

### 2. Code Graph & AST Exploration
| Tool | Primary Parameters | Description |
| :--- | :--- | :--- |
| `ozy_graph` | `action`, `file_path`, `depth` | Reverse dependencies, blast radius, Mermaid impact diagrams, and table lineage. |
| `ozy_get_symbol` | `file_path`, `symbol_name` | Surgical extraction of a single method, class, or React component AST block. |
| `ozy_file_tree` | `subpath`, `max_depth` | Fast in-process directory and symbol tree explorer without subprocess overhead. |
| `ozy_parse` | `file_path`, `source` | Instant in-memory Tree-Sitter AST inspection with dependency hints. |
| `ozy_find_references`| `symbol_name`, `file_path`, `token_budget`| Deep cross-file caller, definition, and import resolver with token budgeting. |
| `ozy_search_literal` | `query`, `subpath`, `token_budget` | Blazing fast literal/regex search engine with strict token ceiling. |

### 3. Quality, Safety & Ghost Patching
| Tool | Primary Parameters | Description |
| :--- | :--- | :--- |
| `ozy_simulate_patch` | `file_path`, `patch` | Ephemeral RAM copy-on-write patch verification, arity checking, and invariant audit. |
| `ozy_diagnostics_quick`| `file_path`, `code` | Native in-memory syntax diagnostic linter (<5ms) for TS, TSX, Python, Rust, Go. |
| `ozy_ast_patch` | `file_path`, `target_code`, `replacement` | Indentation-aware surgical code replacer with automatic whitespace alignment. |
| `ozy_replace_symbol` | `file_path`, `symbol_name`, `replacement` | Surgical AST-level symbol replacement with pre-flight compiler syntax verification. |
| `ozy_verify_diff` | `file_path`, `diff` | Sandboxed diff validator ensuring syntax compliance prior to disk write. |

### 4. Cognitive Brain & Supervision
| Tool | Primary Parameters | Description |
| :--- | :--- | :--- |
| `ozy_brain` | `action`, `payload` | Dual System router, dialectic committee deliberation, and DuckDB churn audit. |
| `ozy_doctor` | `format`, `action` | Complete system diagnostics (SQLite integrity, outbox health, migration audit). |
| `ozy_code_doctor` | `subpath` | Structural code smell and duplicate block detector across monorepos. |
| `ozy_skills` | `query` | Query official agent skills catalog and load specialized workflow cheatsheets. |

---

## [CLI CHEAT SHEET]

```bash
# Check system status, active database paths, and indexing stats
ozymem status

# Recursively scan and index a project with AST and vector embeddings
ozymem scan .

# Display an AST structural tree of a file with line ranges
ozymem tree src/main.rs

# Surgically extract a single symbol's implementation without reading full file
ozymem symbol src/services/auth.ts authenticateUser

# Search persistent memories, lessons, and architectural decisions
ozymem search "database connection retry backoff"

# Run an offline MCTS counterfactual replay dream simulation ($0 tokens)
ozymem dream run

# Check MCTS exploration tree status and node visit distributions
ozymem dream status

# View detailed AST token optimization and session deduplication metrics
ozymem stats

# Run full project health check and SQL migration audit
ozymem doctor
```

---

## [DOCUMENTATION DIRECTORY]

For deep architectural dives and specialized workflows, refer to the official guides in [`docs/`](docs/INDEX.md):

* [**Architecture & Transactional Outbox**](docs/architecture.md): Dual-tier engine, SQLite WAL mode, and outbox event streams.
* [**Hybrid Semantic Search & RRF**](docs/semantic-search.md): FastEmbed ONNX local inference, FTS5 lexical ranking, and Reciprocal Rank Fusion.
* [**Supervision & Blast Radius**](docs/supervision-and-validation.md): DuckDB git churn analysis, reverse dependency graphs, and DDL guards.
* [**$O(1)$ Engram Contract System**](docs/engram_system.md): Zero-copy binary contracts, prompt cache hit optimization, and Git Notes sync.
* [**Dream-RSI & MCTS Exploration**](docs/dream-rsi.md): Continuous self-improvement, auto-parenting, and offline counterfactual replay.
* [**Cognitive Evolution Architecture (v2.0.0 Roadmap)**](docs/ROADMAP_COGNITIVE_EVOLUTION.md): Token hyper-optimization, ghost patching, and dialectic committee.
* [**The Dream Team (`tgrep` & `rtk`)**](docs/dream-team-tools.md): Trigram regex searching and terminal token compression.
* [**MCP Integration Reference**](docs/mcp-integration.md): Setup walkthroughs, JSON schemas, and client guidelines.
* [**Version History & Changelog**](docs/changelog.md): Complete chronological record of milestones and releases.

---

<div align="center">
  <sub>Ozygram &bull; Persistent Memory Operating System for Autonomous AI Agents &bull; Open Source under MIT License</sub>
</div>
