# Ozygram Workspace - Multi-Project Architecture & Agents Guide

## Workspace Overview
Contains core system modules, memory engines, and CRM integrations:
- `ozygram/`: Ozy memory engine, Rust crates (`ozymem-parser`, `ozymem-core`), Python brain (`ozy-brain`, Dreamer RSI, outbox consumers).
- `crmnew/api-geofal-crm/`: Central FastAPI backend for Geofal CRM.
- `crmnew/crm-geofal/`: Next.js 16 App Router frontend dashboard.

## Active Conventions
- SOLID, DRY, KISS principles applied across all modules.
- Non-destructive changes: Always backup database tables and files before structural modifications.
- Conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`.
- Test coverage standard: Minimum 80% coverage with idempotency and edge cases.
- Property Graph Memory v1.3.0: Connected memory vertices (`memory_nodes`), typed directed edges (`memory_edges`: `APPLIES_TO`, `COUPLED_WITH`, `SUPERSEDES`, `REINFORCES`), multi-hop BFS with distance decay, and cognitive auto-wiring (`ozymem dream graph|run`, `ozy_graph(action="memory_neighborhood|render_mermaid")`).
- Reverse Dependency & Blast Radius Analysis (v1.5.0): AST import inversion (`ozy_graph(action="incoming_dependencies")`), deep cross-file symbol references (`ozy_find_references`), visual Mermaid impact diagrams (`ozy_graph(action="impact_mermaid")`), and automated pre-flight vetoes in `RiskCriticAgent` (> 8 dependent files).
- Resilient Backend & Outbox Pattern: SQL migration idempotency linter (`ozy_doctor(action="audit_migrations")`), `UniversalOutbox` (SQLite WAL mode + Postgres with exponential backoff), and FastAPI background workers with sub-50ms endpoint guarantees (`app/services/outbox.py`).
- Frontend UI Sandbox & Layout Verification: Headless Playwright runner (`scripts/headless_runner.mjs`), deterministic TanStack Table stress fixtures (`src/lib/mockTableFixtures.ts`), and `getBoundingClientRect` column width verifier (`scripts/verify_layout.mjs`).
- Literal Search Guidelines: Mandatory use of exact text matching (`grep_search` with literal strings or `rg -F`) for symbols, routes, and paths to avoid regex compilation bugs.
- Visual Verification Protocol: Quantitative inspection of column bounding rects and overflow detection prior to committing CSS changes.
- Zero-Emoji Standard: All logs, tags, commit messages, Mermaid diagrams, and MCP outputs strictly use textual badges (`[ALERT: STALE_MEMORY]`, `[COUPLED_WITH]`, `[STATUS: ACTIVE]`, etc.).
- Cognitive Evolution Architecture v2.0.0:
  - Token Hyper-Optimization (Fase 5): `view_skeleton` AST pruning, multi-level LOD (`macro`, `contracts`, `full`), $\pm 10$ line extractive windows, session SHA-256 deduplication, and `ozymem stats` CLI.
  - Cross-Language Frontier (Fase 6): Next.js/TSX endpoint consumer extractor (`parse_frontend_endpoint_consumers`), bidirectional `CONSUMES_ENDPOINT` edges (`endpoint_dependents`), SQL DDL lineage extractor (`table_lineage`), and live filesystem watcher (`notify`).
  - Speculative Ghost Patching in RAM (Fase 7): Ephemeral Copy-on-Write volatile RAM sandbox (`simulate_patch`, < 100ms timeout), caller arity checker (`ghost_signature.rs`), `ozy_simulate_patch` MCP tool, and invariant vetoes (`[CRITICAL_VETO: INVARIANT_VIOLATION]`).
  - Dual System Architecture & Offline Consolidation (Fase 8): Dual System Router (System 1 fast reflex < 150ms vs System 2 deliberative committee), 3-agent dialectic committee (`SupervisorAgent.deliberate_committee`), idle offline consolidation (`ozymem dream run` with temporal decay `prune_decayed_memories` and postmortem vaccine `vaccinate_postmortems`), and proactive outbox diagnostics (`ProactiveDiagnosticConsumer`).
- Knowledge as Code (Git-First Markdown Memories): Team gotchas, conventions, and architectural lessons are stored in atomic markdown files under `.ozy/memories/<kind>/*.md` with YAML Frontmatter (`id`, `kind`, `topic_key`, `title`, `author`, `date`, `severity`, `applies_to`, `tags`). Merge-conflict-free across Git branches and 100% auditable in Pull Requests.
- Preventive Gotchas Injection: `ozy_context(action="file")` automatically extracts and prepends `[ALERT: GOTCHA_PREVENTION]` for any matching files before code inspection, ensuring agents and developers do not repeat previously discovered bugs.
- Neuro-Cognitive Architecture & Offline Dreaming v2.2.0:
  - Amygdala Reflex Substrate: Preemptive SQLite WAL reflex cache (`amygdala_reflexes`) with sub-millisecond (< 1ms) `ozy_context` interception (`[ALERT: AMYGDALA_REFLEX - HIGH_FRAGILITY]`).
  - Glial & Glymphatic Homeostasis: Background phagocytosis of dead file vectors/synapses across SQLite and ChromaDB, plus periodic DuckDB WAL compaction (`CHECKPOINT;`).
  - Basal Ganglia Sequence Playbooks & STDP: Co-occurrence choreography mining over Git commits with automatic sequence injection (`[PROCEDURAL_PLAYBOOK_DETECTED]`) and Spike-Timing-Dependent Plasticity ($\Delta w_{ij} = A_+ e^{-\Delta t / \tau_+}$) in `analytics.duckdb`.
  - Autonomous 30-Minute Idle Dreaming Watcher: Zero-CPU Rust inactivity watcher (`idle_watcher.rs`) with edge-triggered latch triggering `DreamOrchestrator` to generate `.ozymem/dream_latest_insights.json`, exposed proactively via `ozygram://dream/insights` MCP resource.
  - Thalamic Selective Attention: Dynamic attention modulation (`thalamus.rs`) balancing lexical BM25 ($\alpha$), Amygdala threat scars ($\beta$), and AST graph topology ($\gamma$) based on task intent (`HOTFIX_DEBUG` vs `ARCHITECTURE_REFACTOR` vs `BALANCED`).
- Neuro-Cognitive Substrate v2.3.0 (Fase 5):
  - Continuous STDP Hebbian Plasticity & SQLite Export: Chronological Git commit sequence mining in DuckDB with automatic materialization of strong causal synapses ($w \ge 0.65$) into SQLite `memory_edges` (`COUPLED_WITH`) for sub-millisecond graph traversals.
  - Neuromodulator Chemical Regulation: Global 4D vector $\mathbf{N} = [D, NE, ACh, 5\text{-HT}]$ dynamically modulating RPE reward, flashbulb one-shot consolidation ($D \ge 0.75$), and Amygdala threat sensitivity ($NE \ge 0.60$).
  - Cerebellar Forward Model & Code Dysmetria: Kinematic motor model with Kullback-Leibler divergence ($D_{KL}$) detecting motor overshoot (`[ALERT: CODE_DYSMETRIA - MOTOR_OVERSHOOT]`) and missing coupled dependencies (`[ALERT: CODE_DYSMETRIA - UNDER_COUPLED]`).
  - Corpus Callosum & Blood-Brain Barrier (BBB): Multi-workspace meta-pattern transfer between `crmnew` and `default` protected by Shannon entropy filtering ($H(S) \ge 4.5$) and regex secret/path scrubbing.
- Distributed Neural Mesh & Collective Intelligence v2.4.0 (Fase 6):
  - Git-Native Heuristics Transport (`refs/notes/ozy-heuristics`): Conflict-free JSONL notes transport (`cat_sort_uniq`) synchronizing team lessons, patterns, and conventions without polluting working tree.
  - Immunological BBB & Anti-Injection Defense: SHA-256 cryptographic provenance signatures, prompt-injection linter (`scan_context_poisoning`), and Bayesian consensus reputation automatically purging harmful rules via apoptosis (`[ALERT: HEURISTIC_APOPTOSIS_PURGED]`).
  - Distributed Semantic Merger & Deduplication: Token-cosine similarity engine (`SemanticHeuristicMerger`, $\cos \ge 0.88$) that fuses duplicate peer heuristics, merges complementary rules, and boosts synaptic confidence weights ($w_{\text{new}} = \min(1.0, w_{\text{old}} + 0.08)$).
  - Autonomous Dream-RSI Federated Synchronization: Non-blocking offline-safe Git fetch with 4.0s timeout during the 30-minute idle sleep cycle, reconciling peer heuristics, updating `.ozymem/dream_latest_insights.json`, and exposing action `corpus_federated_sync` in MCP stdio.

- SOTA Cognitive Frontiers v0.1.9:
  - Neural ONNX Re-Ranker: BGE-Reranker-Base cross-encoder executed locally via `fastembed`, with deterministic token-overlap fallback (< 0.2ms).
  - Incremental Tree-Sitter AST: Differential tree editing via `Tree::edit` (< 40Î¼s re-index vs 3ms full re-parse) preserving unchanged subtrees.
  - GraphRAG & Leiden Community Partitioning: Modularity $Q$ clustering identifying architectural macro-domains and emitting `[CROSS_DOMAIN_BLAST_RADIUS]` alerts.
  - Ephemeral Git Worktree Self-Repair Sandbox: Detached volatile worktree sandboxes (`EphemeralWorktree`) with compile validation and RAII zero-leakage cleanup.

- Live Dream-RSI Centinela & Multi-Repo Contract Bridge v2.5.0:
  - Synaptic Tagging & Capture (STC): Ephemeral 30-minute memory marks on modified AST nodes, exponential decay pruning, and PRP compute allocation.
  - Multi-Repo Workspace Scanner: In-memory harvesting of working tree diffs and 24h commits across consolidated sub-repos (`api-geofal-crm` [FastAPI] + `crm-geofal` [Next.js 16]).
  - Full-Stack Cross-Repo Contract Tracer: Bidirectional AST verification connecting `@router` endpoints with `resilientFetch`/`useQuery` hooks, emitting `[ALERT: CROSS_REPO_CONTRACT_DRIFT]`.
  - Transitive Reverse Blast Radius: Inverted AST import graph BFS determining direct and indirect component/view breakage.
  - Counterfactual MCTS Solver: RAM-based decision tree evaluating candidate remediations (`BackendFallbackAdapter`, `FrontendCallerSync`, `DualActiveContract`) with multi-dimensional reward optimization.
  - Ergonomic Compute Conmutator: `OZY_MAX_COMPUTE=1` (massive Rayon parallelism, 60 simulations) vs `OZY_LIGHTWEIGHT_MODE=1` (cooperative CPU yield, <5% CPU, 10 simulations).
  - Hybrid Memory Persistence: Zero-latency SQLite WAL (`.ozymem/memory.db`) + Git-First Markdown (`.ozy/memories/gotchas/*.md`) with optional enterprise PostgreSQL + `pgvector` adapter.

- Warm-Started Branching MCTS Plan Optimizer v2.6.0:
  - Intermediate State Bifurcation: Warm-starts MCTS tree search ($s_0$) directly from intermediate completed phases (`completed_phases`, `current_phase`, `working_edits`, STC synaptic tags) with $< 1\text{ ms}$ latency, eliminating cold-start re-exploration overhead.
  - SOTA Multi-Dimensional Reward ($Q(s, a)$): Evaluates candidate branches via $w_1 \cdot \mathcal{R}_{\text{swr}} + w_2 \cdot \mathcal{R}_{\text{blast}} + w_3 \cdot e^{-\mathcal{F}_{\text{drift}}} - w_4 \cdot \mathcal{R}_{\text{churn}}$, favoring non-breaking `DualActiveContract` ($Q \ge 0.85$) over uncoordinated breaking cutovers ($Q < 0.65$).
  - Noradrenergic PUCT Exploration Modulation: Locus Coeruleus dynamically modulates $c_{\text{puct}}$ ($c_{\text{puct}} = 1.414$ for disciplined leaf/validation contracts vs $c_{\text{puct}} = 2.400$ for divergent domain service exploration).
  - Bidirectional Re-injection & Offline Sleep Pre-Optimization: Automatically enriches `ozybrain/plan` steps and recommendations with `[OPTIMIZED_BY_DREAM_RSI]`, pre-computes fragile file branches during the 30-min idle sleep cycle into `.ozymem/dream_latest_insights.json`, and synthesizes Git markdown Gotcha vaccines via `persist_mitigation_vaccine`.

- Tri-Hybrid Neuro-Cognitive Memory Substrate v2.8.0:
  - Complementary Learning Systems (CLS 2.0): Coordinates three non-redundant storage engines based on their neurobiological specializations:
    - L1 SQLite WAL (Fast Reflex / Amygdala): Sub-millisecond (< 1ms) threat cache, drift alerts, and local transactional outbox with zero network overhead.
    - L2 ChromaDB (Vector Lake): Local in-memory and disk C++ cosine KNN across 4 cognitive collections (`semantic_heuristics`, `code_graph_ast`, `dream_insights`, `episodic_raw`) powered by deterministic 384-dimensional L2-normalized embeddings (< 0.05ms, ||v||_2 = 1.0) with FastEmbed fallback.
    - L3 PostgreSQL + pgvector + ltree (Neocortex / Collaborative Cluster): Persistent MCTS Trajectory Forest with hierarchical `ltree` queries (`<@`), global SWR Replay Lake with HNSW `vector_cosine_ops` indexing, and recursive CTE Reverse Blast Radius traversal across full-stack repositories.
  - Synaptic Tagging & Capture (STC) Promotion: High-reward MCTS branches ($Q \ge 0.85$, e.g. `DualActiveContract`) are automatically captured and consolidated into ChromaDB `dream_insights` and SQLite WAL gotchas as persistent invariant vaccines.
  - 3-Way Federated Reciprocal Rank Fusion (RRF): Merges SQLite lexical BM25 matching, ChromaDB local dense cosine KNN, and PostgreSQL pgvector SWR ripples into a unified rank score: $\text{RRF}(d) = \sum_{m \in \{\text{sqlite}, \text{chroma}, \text{postgres}\}} \frac{1}{60 + r_m(d)}$.
  - Resilient Write-Through & Offline Graceful Degradation: Writes to SQLite WAL and ChromaDB synchronously, buffering or replicating to PostgreSQL if online. In offline environments or network interruptions, all tools continue functioning smoothly with 0% data loss or crashes.

- Refined Neuro-Cognitive Planner & Contract Bridge (v2.9.0):
  - Auto-Phase Inference: Automatically infers execution phase (`leaf_contracts`, `domain_services`, `entrypoints`, `validation`) from goal semantics and candidate file types (`auto_infer_phase`), eliminating manual flags.
  - Zero-Flag MCTS Dream-RSI Activation: Refactoring and modularization goals automatically trigger warm-started MCTS in RAM, evaluating candidate branch strategies ($Q(s,a)$) and promoting non-breaking `DualActiveContract` ($Q \ge 0.85$) with `[OPTIMIZED_BY_DREAM_RSI]`.
  - Volatile AST Contract Preview: Lightweight RAM-cached extractor (`extract_file_contract_skeleton`) previews exported contracts (`[EXPORTS: ...]`) in topological sequence steps in $< 1\text{ ms}$ for TypeScript/TSX, Python, and Rust.
  - Tri-Hybrid Unified Telemetry (`tri_hybrid_status`): Real-time diagnostic endpoint inspecting L1 SQLite WAL size, L2 ChromaDB collections, L3 PostgreSQL connection/ltree status, and neuromodulator chemical vector $\mathbf{N} = [D, NE, ACh, 5\text{-HT}]$.
  - Deep L3 PostgreSQL Reverse Blast Radius: Connects recursive CTE graph traversal (`WITH RECURSIVE reverse_blast ...`) over `ozy_ast_dependency_edges` to calculate transitive callers with sub-5ms latency and zero-latency local fallback.

- SOTA Neuro-Cognitive Systems Architecture & Centinela Auto-Repair (v3.0.0):
  - Ultra-Low Latency IPC (Fase 1): Sub-millisecond ($< 0.1\text{ ms}$, measured at $63\ \mu\text{s}$) IPC between Rust Core (`ozymem-server`) and Python Brain (`ozy-brain`) with `TCP_NODELAY`, compact JSON serialization, and Unix domain socket fallback (`/tmp/ozymem_brain.sock`).
  - Continuous AST Crawler & Cross-Repo Deep Lineage in PostgreSQL L3 (Fase 2): Continuous dependency extraction (`extract_ast_dependency_edges`) in `ozymem-parser` combined with full-stack endpoint consumption resolution (`CONSUMES_ENDPOINT`) into PostgreSQL L3 `ozy_ast_dependency_edges`, enabling multi-repo recursive CTE reverse blast radius across frontend components and backend routers in $< 3\text{ ms}$.
  - Centinela Self-Repair Loop & Ephemeral Worktree Sandbox (Fase 3): Automatic non-breaking `DualActiveContractShimGenerator` for orphan endpoints (`@router.api_route` fallback) and HTTP method mismatches, validated in volatile RAM sandboxes (`EphemeralWorktree`) without polluting user workspace branches, exposed via MCP `repair_drift`.
  - ChromaDB Semantic Heuristic Deduplication & CLI Tri-Hybrid Dashboard (Fase 4): Idle offline deduplication ($\cos \ge 0.90$) pruning redundant heuristics in ChromaDB, and `ozymem doctor --tri-hybrid` displaying formatted ASCII/JSON status across L1 SQLite WAL, L2 ChromaDB, L3 PostgreSQL pgvector + ltree, and neuromodulators $\mathbf{N} = [D, NE, ACh, 5\text{-HT}]$.

- Algorithmic Neurocognitive Fusion & Active Inference Architecture (v3.2.0):
  - 16D Poincaré Hyperbolic Manifold (\ozymem-parser::poincare\): Non-Euclidean hyperbolic embedding representing tree-structured ASTs and file hierarchies in the open unit ball $\\mathbb{B}^{16}$ with exact distance metric {\\mathbb{B}}(u,v) = \\operatorname{arcosh}\\left(1 + 2\\frac{\\|u-v\\|^2}{(1-\\|u\\|^2)(1-\\|v\\|^2)}\\right)$ computed in $< 50\\text{ ns}$ in Rust Core, eliminating Euclidean tree distortion with 95% less RAM.
  - Topologically-Gated Vector Retrieval (TGVR) in PostgreSQL L3 (\postgres_engine.py\): Single-query transactional fusion combining recursive CTE dependency cones over \ozy_ast_dependency_edges\ with \pgvector\ HNSW cosine similarity (\<=>\), eliminating semantic hallucinations and ensuring $< 5\\text{ ms}$ retrieval with automatic KNN fallback.
  - Active Inference EFE-MCTS with RAM Code World Model (\generative_mcts.py\): Solves tree exploration via Expected Free Energy ((\\pi) = -\\mathcal{V}_{\\text{prag}} - \\mathcal{V}_{\\text{epist}}$), balancing invariant contract compliance with epistemic information gain. Volatile in-memory simulator (GIF-MCTS) prunes invalid syntax and breaking cutovers in RAM in $< 15\\text{ ms}$, saving $> 85\\%$ of LLM tokens.
  - Hippocampal STDP Sleep Replay & Offline Vaccine Consolidation (\orchestrator.py\): Dreamer RSI simulates offline trajectory transitions over high-fragility nodes during the 30-minute idle sleep phase, reinforcing successful non-breaking patterns and materializing durable Gotcha Markdown vaccines under \.ozy/memories/gotchas/\ and in \dream_latest_insights.json\.

- Tool Refinement & MCP Meta-Tools Consolidation (v3.3.0):
  - Fuzzy AST Block Patching: Whitespace-tolerant exact and normalized token matching with auto-indentation and CRLF/LF preservation in `ozymem-parser::patching`, preventing fragile patch failures from minor indentation or formatting variations.
  - Unified 5 Meta-Tools Architecture: Consolidates 25+ fragmented MCP tools into 5 high-level meta-tools (`ozy_patch`, `ozy_plan`, `ozy_context`, `ozy_memory`, `ozy_doctor`), reducing LLM schema prompt token consumption by > 1,500 tokens per turn while providing 100% backward-compatible aliases.
  - Native Rust Active Inference EFE-MCTS (`ozymem-core::mcts`): Zero-Python in-memory Language Agent Tree Search (LATS) engine executing 25 simulations in < 1 ms, evaluating Expected Free Energy $G(\pi) = -V_{\text{prag}} - V_{\text{epist}}$ with Amygdala threat scar penalties and RAM World Model pruning.
  - Hippocampal Theta-Gamma Working Memory Buffer (`ozymem-core::theta_gamma`): 4 strictly ordered Theta phase bins (`leaf_contracts`, `domain_services`, `entrypoints`, `validation`) bounding Gamma item slots to extractive $\pm 10$ line windows, achieving > 75% context compression and eliminating 'lost in the middle' attention dilution.
  - Native Rust Fast-Lane Fallback: `ozymem-server` automatically executes native MCTS and Theta-Gamma working memory packing if Python power-lane is offline, guaranteeing uninterrupted `[OPTIMIZED_BY_DREAM_RSI]` responses.

## Tool Precedence Protocol (Native Ozygram First)
To avoid context bloat, syntax breaks, and latency, agents must adhere to the following 5 Meta-Tools hierarchy:
1. `ozy_patch(action="apply"|"simulate"|"replace_symbol")` > `replace_file_content`: Fuzzy token matching, auto-indentation, Tree-Sitter AST syntax verification, and volatile RAM simulation.
2. `ozy_plan(goal="...")`: Native Rust Active Inference EFE-MCTS with topological edit sequence and Hippocampal Theta-Gamma working memory packing.
3. `ozy_context(action="file"|"task"|"project", lod="contracts")` > raw `view_file`: AST skeletons, transitive caller cones, and automatic `[ALERT: GOTCHA_PREVENTION]` injection.
4. `ozy_memory(action="search"|"save"|"sync_markdown"|"export_git_notes")`: Git-First markdown sync, Amygdala reflexes, and TGVR vector retrieval.
5. `ozy_doctor(action="status"|"code"|"audit_migrations")`: SQLite WAL integrity, outbox health, consolidated duplicate detection, and SQL migration idempotency linter.


- Frontier AI Engineering & Cognitive Upgrades (v3.4.0):
  - Kimi Mooncake AST Prefill Cache: Thread-safe, bounded (max_entries = 2048) xxHash64 content-addressed cache (AstPrefillCache) serving pruned skeletons and contracts in < 50us with zero Tree-Sitter re-parsing overhead.
  - DeepSeek-R1 Verifiable Ground-Truth Rewards: Deterministic compiler/contract reward feedback (R_compiler in {-1.0, -0.8, 0.5, 1.0}) integrated into Expected Free Energy G(pi), heavily penalizing syntax errors and breaking cutovers.
  - OpenAI o1/o3 Dynamic Test-Time Compute Scaling: Adaptive budget calculation (N_sim in [5, 80]) modulated dynamically by blast radius ln(1 + 10 * r_blast) and Amygdala threat scars beta_threat, with deliberate backtracking away from broken AST states.
  - Single-Binary Monolith MCP Injection: Automated injection of ozygram serve across AI coding clients (Cursor, Claude, Windsurf, Antigravity) with zero process contention and clean single-binary lifecycle.

- Non-Redundant Strands Integration & Architecture Elevation (v3.5.0):
  - EphemeralWorktree Shell Mediation (Default-Deny): Hardened sandboxed execution (`run_sandboxed_command`) with strict whitelist of compilers and tools (`cargo`, `git`, `npm`, `python`, `node`, `pytest`, `tsc`, `rg`), automatic credential scrubbing from child environment variables, and execution timeout watchdog.
  - In-Flight Result Compactor & Spillover Blob Store: Automatic compaction of large MCP tool payloads (> 14,000 characters / ~3,500 tokens) with SHA-256 persistence into `.ozymem/blobs/<short_hash>.txt`, preview extraction (head 40 lines + tail 10 lines), and lossless pointer retrieval via `ozymem://blob/<hash>` resource template.
  - Agents-as-Tools in `ozy_plan`: Dialectic critique integration directly within `ozy_plan(goal="...", critique_role="architect"|"security_critic"|"code_reviewer"|"test_engineer")`, injecting specialist invariant evaluations in RAM without creating parallel redundant MCP tools.
