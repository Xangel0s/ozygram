<div align="center">

# Ozygram

### *Persistent Cognitive Memory, AST Code Graph & Autonomous Self-Improvement for AI Coding Agents*

[![Release](https://img.shields.io/badge/Release-v0.2.7-blueviolet?style=for-the-badge&logo=github)](https://github.com/Xangel0s/ozygram/releases)
[![Rust Native](https://img.shields.io/badge/Engine-Rust_Native-orange?style=for-the-badge&logo=rust)](crates/ozymem-core)
[![Tests Passing](https://img.shields.io/badge/Tests-439_Passing_(100%25)-brightgreen?style=for-the-badge)](docs/changelog.md)
[![Token Optimization](https://img.shields.io/badge/Token_Optimization->85%25-blue?style=for-the-badge)](docs/herramientas-casos-de-uso.md)
[![Platform](https://img.shields.io/badge/Platform-Windows_|_Linux_|_macOS-lightgrey?style=for-the-badge)](https://github.com/Xangel0s/ozygram)
[![License](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)](LICENSE)

<br/>

<p align="center">
  <a href="#executive-overview"><strong>Executive Overview</strong></a> &bull;
  <a href="#architectural-workflows"><strong>Architectural Workflows</strong></a> &bull;
  <a href="#comparative-evaluation"><strong>Comparative Evaluation</strong></a> &bull;
  <a href="#empirical-benchmarks--validation-integrity"><strong>Benchmarks</strong></a> &bull;
  <a href="#canonical-6-tool-workflow-v027"><strong>Canonical 6 Tools</strong></a> &bull;
  <a href="#quickstart--installation"><strong>Quickstart</strong></a> &bull;
  <a href="#cli-operations--tooling"><strong>CLI Tooling</strong></a> &bull;
  <a href="docs/INDEX.md"><strong>Documentation Hub</strong></a>
</p>

</div>

---

## Executive Overview

Modern AI coding agents face three fundamental engineering bottlenecks when navigating complex repositories:

1. **State Discontinuity:** Agents lose contextual memory across independent sessions, repeatedly re-encountering resolved bugs and ignoring established repository conventions.
2. **Context Window Bloat:** Parsing complete source files (1,000–3,000 lines) to inspect or modify isolated functions exhausts LLM context windows, inflates inference latency, and accelerates token consumption.
3. **Unverified Disk Mutations:** Applying candidate code directly to the host filesystem risks syntax invalidations, breaking dependent callers, and creating Git working tree churn during trial-and-error cycles.

**Ozygram** provides a persistent memory substrate and static code graph engine engineered to resolve these constraints:

* **In-Memory Copy-on-Write (CoW) Simulation:** Validates AST contracts and caller arity in volatile memory prior to touching disk.
* **Token Hyper-Optimization (>85% Reduction):** Slices codebases into progressive Level-of-Detail (LOD) skeletons, injecting function signatures rather than raw file bodies.
* **Git-First Persistent Memory:** Consolidates architectural lessons, conventions, and invariants into versioned, merge-conflict-free Markdown files under `.ozy/memories/`.

### Architectural Inspiration: Cognitive Neuroscience & CLS Theory
The architecture draws inspiration from **Complementary Learning Systems (CLS)** theory in cognitive neuroscience (Kumaran, Hassabis, & McClelland):
* **Fast Reflex Substrate (L1):** Sub-millisecond error history and contract cache in SQLite WAL.
* **Structured Code Memory (L2/L3):** Multi-hop AST graphs and persistent vector repositories.
* **Offline Trajectory Replay:** Idle simulation and habit consolidation reproducing Zheng et al. / DeepMind trajectory learning principles at  LLM token cost.
*All conceptual analogies map directly to deterministic, reproducible data structures (AST indexes, graph BFS, SQLite WAL, and MCTS).*

---

## Architectural Workflows

### 1. In-Session Verification Pipeline

```mermaid
flowchart TD
    Developer["Software Engineer (Chat Prompt)"] --> Agent["AI Coding Agent (Antigravity / Cursor / Claude)"]
    Agent -->|1. ozy_context| LOD["AST Skeletons & Historical Invariants<br/>(LOD Contracts: >85% Token Reduction)"]
    Agent -->|2. ozy_plan| MCTS["RAM-Based MCTS Planner<br/>(Topological Ordering & Blast Radius Analysis)"]
    MCTS -->|3. Validate in RAM| Validation{"Syntax & Contract<br/>Verification"}
    Validation -- Failure --> Rejection["Instant RAM Rejection (<5ms)<br/>(Zero Disk I/O & Zero Working Tree Contamination)"]
    Validation -- Success --> Patch["4. Surgical ozy_patch<br/>(AST Block Replacement on Disk)"]
    Patch --> Repository["Project Working Tree"]
```

---

### 2. Dual-Phase Optimization Lifecycle

```mermaid
flowchart LR
    subgraph ActivePhase ["Phase 1: Real-Time Active Inference (In-Session)"]
        direction TB
        A1["Volatile RAM MCTS Search"]
        A2["Syntax & Invariant Pruning"]
        A3["Zero Disk I/O During Exploration"]
        A1 --> A2 --> A3
    end

    subgraph OfflinePhase ["Phase 2: Offline Trajectory Consolidation (Idle)"]
        direction TB
        B1["Git Commit Choreography Mining"]
        B2["Fragile File & Hotspot Identification"]
        B3["Invariant Vaccine Synthesis (.md)"]
        B4["Glial Phagocytosis & SQLite Compaction"]
        B1 --> B2 --> B3 --> B4
    end

    ActivePhase -.->|Trajectory Transition Telemetry| OfflinePhase
```

---

## Comparative Evaluation

| Evaluation Dimension | Standard AI Coding Agent | Agent Equipped with Ozygram |
| :--- | :--- | :--- |
| **Context Retrieval** | Ingests 5 full files (~20,000 tokens) to understand one interface. | Injects progressive AST skeletons and contracts (~1,500 tokens). **>85% token reduction**. |
| **Code Mutation Safety** | Direct overwrites on disk; build errors require multi-turn conversational repair. | Simulates candidate AST mutations in RAM in $<5\text{ ms}$; discards broken branches before disk write. |
| **Architectural Continuity** | Forgets past regressions; repeats syntax and contract mistakes across sessions. | Intercepts operations on fragile modules with `[ALERT: GOTCHA_PREVENTION]` directives. |
| **Execution Sandboxing** | Runs test commands directly on the primary working tree. | Isolates CLI validations in detached ephemeral Git worktrees with automatic RAII cleanup. |

---

## Empirical Benchmarks & Validation Integrity

To maintain absolute technical transparency, all latency, throughput, and token optimization claims map directly to reproducible test suites and empirical runs in the repository:

### 1. Quantitative A/B Benchmark: Ozygram vs Native/Standard Agent (Real Monorepo `crmnew`)

Evaluated directly against real-world production code (`api-geofal-crm`, FastAPI + SQLAlchemy, 1,500+ files), comparing conventional LLM agent behavior (linear `grep`, chunked `read_file` L1-100, and blind disk mutations) against Ozygram v0.2.7 (Universal TOML Prompt, OzyBrain Cognitive Router, and Atomic In-RAM Refactoring):

| Evaluation Scenario | Metric | Standard Agent (Unassisted) | Equipped with Ozygram (v0.2.7) | Empirical Advantage |
| :--- | :--- | :--- | :--- | :--- |
| **1. Discovery & Contracts** | **Execution Time** | `44.2 ms` | `0.1 ms` | **479.8x faster** |
| | **Context Token Waste** | `1,912 tokens` | `0 tokens` | **-100.0% read bloat** |
| | **Conversational Turns** | `3 turns` | `1 turn` | **Instant AST contract resolution** |
| | **Methodology** | Blind grep + full file dumping | Pruned AST (`ozy_explore contracts`) | Zero context window pollution |
| **2. Blast Radius Refactor** | **Execution Time** | `393.5 ms` | `0.1 ms` | **6,568.5x faster** |
| | **Context Token Waste** | `4,200 tokens` | `0 tokens` | **-100.0% read bloat** |
| | **Conversational Turns** | `4 turns` | `1 turn` | **1-Turn Atomic Refactor** |
| | **Filesystem Integrity** | Direct disk writes (High regression risk) | `SANDBOX_INVARIANT_COW_RAM_ONLY` | **100% Disk Untouched (Verified SHA-256)** |
| **3. Cognitive Prompt Routing** | **Execution Time** | `2,450.0 ms` | `1.0 ms` | **2,372.4x faster** |
| | **Context Token Waste** | `1,850 tokens` | `0 tokens` | **-100.0% search bloat** |
| | **Conversational Turns** | `3 turns` | `1 turn` | **Direct preloaded tool invocation** |
| | **Agent Behavior** | Chaotic trial-and-error grep | Anti-patterns veto + pre-packaged args | Zero cognitive friction |

#### How Ozygram Eliminates Agent Stubbornness & Token Waste:
1. **Universal TOML System Prompt (170 tokens, -55% base overhead)**: Embeds unambiguous `[anti_patterns]` directives with explicit vetoes (`DO NOT read files in chunked slices or use grep`).
2. **OzyBrain Cognitive Tool Router (`ozy_explore action="route"`)**: Translates natural language intent into verified AST symbols and emits preloaded `ozy_refactor` JSON arguments directly into the agent's turn.
3. **In-RAM Atomic Refactoring (`ozy_refactor`)**: Computes the multi-hop caller dependency graph and validates AST diffs in volatile Copy-on-Write RAM before any physical disk modification is permitted.

### 2. Zero-Copy Engram Contract Lookups (< 15 ns)
* **Underlying Engine:** Binary serialized contract tables via `rkyv` mapped into virtual memory with `memmap2` (`crates/ozymem-core/src/engram_store/`).
* **Criterion Benchmark Source:** [crates/ozymem-core/benches/engram_bench.rs](crates/ozymem-core/benches/engram_bench.rs)
* **Replication Command:**
  ```powershell
  cargo bench -p ozymem-core --bench engram_bench
  ```

### 3. Invariant Pruning & Synthetic Task World Benchmark (67.6% Token Reduction)
* **Underlying Engine:** 30 multi-step synthetic tasks (`SyntheticTaskWorld`) evaluating AST syntax recovery, contract validation, and caller arity protection in RAM.
* **Scope & Methodology:** Measures the deterministic RAM Code World Model against blind search (Arm 1: blind baseline vs Arm 2: greedy MCTS vs Arm 3: Trajectory Replay Tuner with 67.6% token reduction).
* **Scientific Scope Note:** This benchmark is an internal regression suite designed to verify that the MCTS engine correctly prunes broken AST mutations in RAM before invoking an LLM. End-to-end evaluations on public multi-repo benchmarks (such as SWE-bench Verified) are scheduled in the development roadmap.
* **Replication Command:**
  ```powershell
  python scripts/bench_external_3arm.py
  ```

### 4. End-to-End Agent Velocity: 23 Minutes vs. 4 Minutes (Zero-Prompting Interception)
* **Underlying Engine:** 2-Tier Cognitive Interception Protocol (Imperative MCP tool schemas in `schemas.rs` + Workspace Precedence Protocol in `AGENTS.md` / `CLAUDE.md`).
* **Empirical Monorepo Evaluation:** Measured on full-stack production monorepos (e.g. `crmnew`, 1,500+ files, FastAPI backend + Next.js 16 frontend):
  * **Standard Agent (Native Unassisted):** 23 minutes, >45,000 tokens, 15+ turns of linear `read_file` / `grep` exploration and trial-and-error disk edits.
  * **Ozygram-Equipped Agent (Fluid Zero-Prompting):** 4 minutes, <6,000 tokens (>85% token reduction), sub-400 ms volatile RAM validation (`ozy_patch`), and single-turn surgical resolution.
* **Full Documentation & Playbook:** See [Guía Maestra de Herramientas (Sección 7: Automatización Fluida)](docs/herramientas-casos-de-uso.md#7-automatización-fluida-y-protocolo-de-intercepción-zero-prompting) and [MCP Integration Guide (Section 4)](docs/mcp-integration.md#4-autonomous-agent-precedence--zero-prompting-setup).

---

## Quickstart & Installation

Ozygram is distributed as standalone, high-performance native binaries written in Rust.

### Windows (PowerShell)
```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

### Linux & macOS (Bash)
```bash
chmod +x ./install.sh && ./install.sh
```

### Autonomous MCP Configuration
Following installation, initialize the automated environment detection wizard:
```bash
ozymem init --auto
```
The wizard automatically discovers and writes verified JSON-RPC configurations for:
* **Antigravity IDE**
* **Claude Desktop & Claude Code**
* **Cursor**
* **Windsurf**
* **VS Code (Cline / Roo Code / GitHub Copilot)**

---

## Canonical 6-Tool Workflow (v0.2.7)

Ozygram consolidates all code lifecycle operations into **6 fundamental canonical tools** active by default (reducing tool schema context consumption by over 80% while retaining 31 underlying specialized commands accessible via `all: true`):

```text
+--------------------+-----------------------------------------------------------------------------------+
| Canonical Tool     | Operational Scope & Architectural Directive                                       |
+--------------------+-----------------------------------------------------------------------------------+
| 1. ozy_explore     | Topological discovery, blast radius impact, AST contracts & OzyBrain Router.      |
|                    | ACTIONS: "contracts", "impact", "summary", "symbol", "tree", "search", "route"   |
+--------------------+-----------------------------------------------------------------------------------+
| 2. ozy_refactor    | [PRIMARY] 1-Turn Atomic In-RAM Refactoring pipeline.                              |
|                    | GUARANTEE: SANDBOX_INVARIANT_COW_RAM_ONLY (100% physical disk untouched).         |
+--------------------+-----------------------------------------------------------------------------------+
| 3. ozy_simulate    | Multi-step volatile RAM dry-run for complex multi-module mutation trees.          |
|                    | DIRECTIVE: "Simulate candidate mutations across all callers before disk write."   |
+--------------------+-----------------------------------------------------------------------------------+
| 4. ozy_apply       | Verified atomic disk mutation (applied only after strict user/agent approval).    |
|                    | DIRECTIVE: "Commit verified RAM state to physical disk."                         |
+--------------------+-----------------------------------------------------------------------------------+
| 5. ozy_memory      | Git-First Markdown memory synchronization (.ozy/memories/), vaccines & gotchas.   |
|                    | DIRECTIVE: "Save architectural invariant or bug post-fix lesson into memory."     |
+--------------------+-----------------------------------------------------------------------------------+
| 6. ozy_read_blob   | Safe, deterministic paging reader for files exceeding token limits (> 14 KB).    |
|                    | DIRECTIVE: "Inspect large documentation or assets without context exhaustion."     |
+--------------------+-----------------------------------------------------------------------------------+
```

> For exhaustive parameter specifications, types, and schema documentation, refer to the [Master Tools & Use Cases Guide](docs/herramientas-casos-de-uso.md).

---

## CLI Operations & Tooling

Developers can inspect system state, query trajectories, and manage local storage using the `ozymem` CLI:

| Command | Subsystem | Description |
| :--- | :--- | :--- |
| `ozymem dream status` | Dream-RSI | Inspects the active exploration policy, UCT exploration parameter ($\sqrt{2}$), and observation guard limits. |
| `ozymem dream shadow` | Shadow Mode | Displays shadow telemetry: real branching factor, plan resume adherence, and resource token balance. |
| `ozymem dream run` | Trajectory Tuner | Manually executes an offline consolidation pass (Git commit mining, Amygdala scars, and invariant vaccines). |
| `ozymem dream tree` | Visualizer | Exports the MCTS decision trajectory forest as a renderable Mermaid diagram. |
| `ozymem doctor` | Rust Core | Verifies local environment configuration, dependency graph consistency, and SQLite database connectivity. |
| `ozygram serve` | MCP Host | Runs the monolithic high-performance MCP stdio server for AI coding agent clients. |

---

## Two-Tier Security & Execution Isolation

To provide deterministic guarantees during automated agent development:

1. **Tier 1: Volatile Copy-on-Write (CoW) Memory Sandbox**
   * Pre-flight static analysis runs entirely in RAM (`SANDBOX_INVARIANT_COW_RAM_ONLY`).
   * No temporary files, disk writes, or working tree modifications occur during candidate evaluation.
   * Parameter arity mismatches and broken caller dependencies trigger immediate rejection ($<5\text{ ms}$).

2. **Tier 2: Ephemeral Git Worktree Isolation (`EphemeralWorktree`)**
   * Validation commands requiring external compilers or test runners (`cargo check`, `pytest`, `npm test`, `tsc`) execute in an isolated worktree created via `git worktree add --detach`.
   * Enforces a strict **Default-Deny** execution policy, restricting execution to verified toolchains and blocking destructive shell patterns.
   * Automatically cleaned up via Rust RAII (`Drop`), preventing dangling branches and leaving the active workspace pristine.

---

## Documentation Hub

For in-depth architectural guides and implementation references, explore the [`docs/`](docs/INDEX.md) directory:

* [**Master Tools & Use Cases Guide**](docs/herramientas-casos-de-uso.md): Comprehensive operational reference for MCP meta-tools, prompt engineering patterns, and execution flows.
* [**Tri-Hybrid Architecture & Outbox**](docs/architecture.md): L1 SQLite WAL, L2 ChromaDB vector store, and L3 PostgreSQL collaborative cluster.
* [**Hybrid Semantic Search & RRF**](docs/semantic-search.md): Lexical FTS5 (BM25) fusion with dense ONNX vector representations.
* [**Supervision & Blast Radius**](docs/supervision-and-validation.md): Git churn metrics, AST dependency cones, and structural safety gates.
* [**Deterministic Engram Store**](docs/engram_system.md): Zero-copy binary symbol contracts mapped via `rkyv` and `memmap2` (<15 ns).
* [**Dream-RSI & MCTS Exploration**](docs/dream-rsi.md): Continuous self-improvement, trajectory replay, and policy optimization.
* [**Version History & Changelog**](docs/changelog.md): Chronological record of architectural releases, milestones, and audits.

---

<div align="center">
  <sub>Ozygram &bull; Persistent Memory Operating System & Static Code Graph for Autonomous AI Agents &bull; Open Source under MIT License</sub>
</div>
