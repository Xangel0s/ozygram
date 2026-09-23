# Master Development Plan: Ozygram Dual-Tier Engine (v0.4.0)

## 1. Dual-Tier Vision & Architecture

Ozygram is structured into **Two Complementary Cognitive Lanes**:

```text
┌─────────────────────────────────────────────────────────────┐
│  IDE / LLM Agent (Claude, Gemini, Cursor)                   │
└──────────────┬──────────────────────────────▲───────────────┘
               │ JSON-RPC (<100 ms)           │ Immediate Response
┌──────────────▼──────────────────────────────┴───────────────┐
│ LANE 1: Fast Lane (Rust Core - Lightweight & Concurrent)    │
│ • Instant MCP handshake (CWD, projects, tools)              │
│ • Hot cache in RAM + transactional SQLite WAL               │
│ • Hygiene filter: blocks files > 256KB, .venv, etc.         │
│ • Asynchronous queueing to Python Worker                    │
└──────────────┬──────────────────────────────▲───────────────┘
               │ IPC / Local Socket           │ Consolidated Lessons
┌──────────────▼──────────────────────────────┴───────────────┐
│ LANE 2: Power Lane (Python Heavy Daemon - Raw Power)        │
│ • ChromaDB: High-fidelity dense vectors                     │
│ • AI Noise Gate: Critic filtering logs and junk data        │
│ • Cross-Encoder Re-Ranker: Semantic relevance > 95%         │
│ • Multi-Agent Supervisor: Pydantic-AI + DuckDB OLAP         │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. Resolved Root Causes & Diagnostics

1. **Code Index Corruption (`files`, `functions`, `file_dependencies`):**
   - Cause: `full_scan()` in `indexing.rs` did not invoke `check_noise_or_huge_file`. Giant files (`.bundle.js`, 10MB `.sql`) and folders like `.venv`, `scratch/`, `.supabase/` were ingested into SQLite, blocking transactions.
   - Status: Memories survived because they reside in isolated tables (`observations`, `lessons`).
2. **Noise in Passive Capture:**
   - Cause: `passive_capture` in `lessons.rs` accepted any line with length >= 8 characters under `## Key Learnings`, absorbing raw terminal dumps and markdown tables.
3. **Latency in MCP Calls:**
   - Cause: Heavy synchronous processes blocked the JSON-RPC channel on the main thread.

---

## 3. Detailed Phase Roadmap (4 Phases × 4 Tasks)

### Phase 1: Hygiene Guardrails and Resilience in Rust Core (Fast Lane)
- [x] **Task 1.1:** Connect `check_noise_or_huge_file` inside `indexing.rs::full_scan()` and `reload_if_stale()`, discarding files > 256 KB.
- [x] **Task 1.2:** Expand `is_noise_dir()` in `helpers.rs` with `.venv`, `venv`, `env`, `.tox`, `scratch`, `.supabase`, `.turbo`, `coverage`, `.output`, `target`.
- [x] **Task 1.3:** Configure SQLite PRAGMAs in `schema.rs`: `PRAGMA busy_timeout = 5000;`, `PRAGMA synchronous = NORMAL;`, `PRAGMA journal_mode = WAL;`.
- [x] **Task 1.4:** Sanitize `passive_capture()` in `lessons.rs` to reject terminal dumps, markdown tables, and raw build outputs.

### Phase 2: Power Engine in Python (ChromaDB + Vector Store + Re-ranking)
- [x] **Task 2.1:** Configure persistent **ChromaDB** client in `python/ozy-brain` (storage in `.ozymem/chroma`).
- [x] **Task 2.2:** Implement dense high-dimensional embedding pipeline (`FastEmbed` / `sentence-transformers` with hardware acceleration).
- [x] **Task 2.3:** Integrate **Neural Cross-Encoder Re-ranker** (`bge-reranker`) model to filter false positives before returning responses to the LLM.
- [x] **Task 2.4:** Create unified `deep_semantic_search` action combining lexical FTS5 BM25 with dense vector search and re-ranking.

### Phase 3: AI Noise Gate & Anti-Noise Critic (Data Shield)
- [x] **Task 3.1:** Implement **AI Noise Gate** in `python/ozy-brain/agents/memory_agent.py` to classify and evaluate semantic quality of each memory prior to indexing.
- [x] **Task 3.2:** Route raw terminal dumps, stack traces, and logs to `analytics.duckdb` as telemetry, preventing vector index pollution.
- [x] **Task 3.3:** Create **Memory Clustering & Consolidation** routine to synthesize multiple observations into 1 canonical master rule.
- [x] **Task 3.4:** Implement temporal decay factor (*Memory Decay*) to discount relevance of stale lessons unconfirmed for over 90 days.

### Phase 4: Multi-Agent Orchestration & E2E Validation
- [x] **Task 4.1:** Wire `SupervisorAgent` in `brain.py` to coordinate `RiskCritic`, `MemoryConsolidationAgent`, and `DataEngine` (DuckDB + Polars).
- [x] **Task 4.2:** Format outputs for the LLM agent with "Token-Budget Aware" executive summaries (< 1 KB) to prevent context window saturation.
- [x] **Task 4.3:** Incorporate support for `.ozy.toml` manifest in repositories for frictionless zero-config discovery.
- [x] **Task 4.4:** Run cross-language test suites (`cargo test --workspace --tests`, `python -m unittest discover`) and validate latencies (< 100 ms in Fast Lane and < 2.5 s in Power Lane).

---

## 4. Success Metrics
1. **Fast Lane Latency:** Responses to `mem_current_project` and `mem_context` in under 100 ms.
2. **Zero Pollution:** 0 files larger than 256 KB or from virtual folders (`.venv`, `scratch`) ingested into `memory.db`.
3. **Semantic Relevance:** Re-ranking precision > 90% in retrieval tests with ambiguous queries.
4. **Database Integrity:** 100% of atomic transactions free from `database is locked` contention errors.
