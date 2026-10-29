# Ozy Brain — Tri-Hybrid Neuro-Cognitive Reasoning Engine (v0.2.0)

`ozy-brain` is the local Python reasoning and offline consolidation engine used by the Rust `ozymem-server` MCP server and autonomous coding agents. It acts as an **advisory cognitive layer and persistent neocortex**, implementing Complementary Learning Systems (CLS 2.0), Warm-Started Branching MCTS Plan Optimization, and live Multi-Repo Contract Verification.

---

## Architecture: Tri-Hybrid Memory Substrate (CLS 2.0)

```text
MCP Client (Antigravity / Claude Code / Cursor / Windsurf / VS Code)
   │
   ▼ (MCP JSON-RPC Stdio)
ozymem-server (Rust Core — Authority & Side-Effects Controller)
   │
   ├─► L1 FAST REFLEX: SQLite WAL (.ozymem/memory.db)
   │   • Amygdala threat scars & gotcha pre-emption (< 1ms)
   │   • Contract drift cache & transactional outbox
   │
   ▼ (Subprocess JSON Stdio)
ozy-brain (Python Reasoning Worker & Centinela Dream-RSI)
   │
   ├─► L2 VECTOR LAKE: ChromaDB (Local Cosine KNN)
   │   • 4 Collections: semantic_heuristics, code_graph_ast, dream_insights, episodic_raw
   │   • Deterministic 384-dimensional unit embeddings (||v||_2 = 1.0)
   │
   ├─► L3 NEOCORTEX CLUSTER: PostgreSQL (pgvector + ltree)
   │   • Global SWR Replay Lake with HNSW vector_cosine_ops
   │   • Persistent MCTS Trajectory Forest with hierarchical ltree (<@)
   │   • Recursive CTE Reverse Blast Radius traversal
   │
   └─► 3-WAY FEDERATED RANKING: Reciprocal Rank Fusion (RRF)
       • RRF(d) = sum(1 / (60 + rank_m(d))) across SQLite, ChromaDB, and PostgreSQL
```

### Neurobiological Core Principles
1. **Complementary Learning Systems (CLS 2.0)**: Coordinates three non-redundant storage engines based on biological specializations (L1 SQLite WAL for sub-millisecond reflexes, L2 ChromaDB for fast vector KNN, and L3 PostgreSQL for global hierarchical forest persistence).
2. **Synaptic Tagging & Capture (STC)**: MCTS exploration branches achieving high reward ($Q \ge 0.85$, such as `DualActiveContract`) are automatically consolidated into ChromaDB `dream_insights` and SQLite gotchas as permanent invariant vaccines.
3. **Warm-Started Branching MCTS Plan Optimizer**: Eliminates cold-start re-exploration by bifurcating the search tree directly from intermediate completed phases with $< 0.02\text{ ms}$ latency and noradrenergic PUCT modulation.
4. **Graceful Offline Degradation**: Operates with 0% crashes and zero data loss if PostgreSQL is offline by buffering changes in SQLite WAL and ChromaDB.

---

## Package Structure

```text
python/ozy-brain/
├── ozy_brain/
│   ├── __init__.py                  # Package exports
│   ├── __main__.py                  # CLI runner (-m ozy_brain)
│   ├── main.py                      # JSON stdio parser
│   ├── brain.py                     # Central action dispatcher
│   ├── config.py                    # LLM router (Ollama / OpenRouter / Fallback)
│   ├── schemas.py                   # Data contracts (BrainResponse, StructuredPlan, etc.)
│   ├── planner.py                   # Multi-phase topological planner with MCTS branching
│   ├── vector_store.py              # Persistent ChromaDB vector collections & RRF
│   ├── cognitive_router.py          # Dual System Router (System 1 vs System 2)
│   ├── amygdala.py                  # Threat reflex cache and pre-flight interception
│   ├── glial.py                     # Glial & glymphatic homeostasis phagocytosis
│   ├── stdp_synapse.py              # Spike-Timing-Dependent Plasticity commit sequence miner
│   ├── neuromodulators.py           # 4D chemical regulation [D, NE, ACh, 5-HT]
│   ├── cerebellum.py                # Kinematic motor forward model & dysmetria detector
│   ├── corpus_callosum.py           # Git Notes cross-workspace federated transport
│   ├── leiden_communities.py        # GraphRAG Modularity Q community partitioning
│   ├── scanner/                     # Centinela Multi-Repo Contract Bridge
│   │   ├── multi_repo.py            # Multi-repo discovery & live AST diff harvester
│   │   ├── contract_tracer.py       # FastAPI @router <-> Next.js 16 hooks contract tracer
│   │   ├── blast_radius.py          # Transitive inverted import graph BFS
│   │   ├── counterfactual_solver.py # RAM counterfactual remediation solver
│   │   ├── postgres_engine.py       # PostgreSQL native engine (pgvector + ltree + CTEs)
│   │   ├── chroma_bridge.py         # 4-collection ChromaDB bridge with 384d embeddings
│   │   ├── tri_hybrid_orchestrator.py # Write-through coordinator & 3-way RRF ranker
│   │   ├── compute_config.py        # MonsterCompute vs Lightweight throttle
│   │   └── memory_store.py          # SQLite WAL, Markdown Git synchronizer, Postgres backend
│   ├── dream/                       # Autonomous Offline Dreaming (Dream-RSI)
│   │   ├── orchestrator.py          # 30-minute idle sleep consolidation orchestrator
│   │   ├── generative_mcts.py       # LATS speculative code exploration & vaccine generator
│   │   ├── plan_branching.py        # Warm-started branching MCTS plan optimizer
│   │   └── swr_replay.py            # Sharp-Wave Ripple offline counterfactual replay
│   └── agents/                      # Cognitive Dialectic Committee
│       ├── supervisor.py            # Hierarchical coordinator & deliberate committee
│       ├── risk_critic.py           # Adversarial critic with pre-flight vetoes
│       └── memory_agent.py          # Memory consolidation & temporal decay
├── tests/                           # Comprehensive test suite (> 35 automated tests)
├── pyproject.toml                   # Project metadata (v0.2.0)
└── README.md
```

---

## Supported Cognitive Actions (`ozy_brain`)

| Action | Subsystem | Objective |
| :--- | :--- | :--- |
| `plan` | `planner.py` | Multi-phase topological plan with warm-started MCTS branch optimization (`[OPTIMIZED_BY_DREAM_RSI]`) |
| `risk_review` | `risk.py` / `agents/` | Dialectic 3-agent committee (Implementer + Risk Critic + Executive Judge) with pre-flight vetoes |
| `dream_rsi` / `offline_dream` | `dream/orchestrator.py` | 30-min idle sleep cycle: Glial cleanup, STDP Hebbian mining, and tri-hybrid health consolidation |
| `federated_rrf_search` | `scanner/tri_hybrid_orchestrator.py` | 3-way Reciprocal Rank Fusion across SQLite WAL, ChromaDB, and PostgreSQL pgvector |
| `audit_contract_drift` | `scanner/contract_tracer.py` | Full-stack cross-repo AST verification (FastAPI endpoints vs Next.js 16 hooks) |
| `reverse_blast_radius` | `scanner/blast_radius.py` | Inverted AST dependency BFS & PostgreSQL recursive CTE caller resolution |
| `simulate_patch` | `scanner/counterfactual_solver.py` | Ephemeral Copy-on-Write RAM mutation and caller arity verification (< 100ms) |
| `get_repository_hotspots` | `data_engine.py` | DuckDB analytics over Git commits, churn hotspots, and bug density |

---

## Testing & Quality Bar

Run the full Python test suite (100% pass rate guaranteed):

```bash
# Run all tri-hybrid memory and centinela tests
python -m pytest tests/test_postgres_native_engine.py tests/test_chroma_vector_bridge.py tests/test_tri_hybrid_orchestrator.py tests/test_crmnew_tri_hybrid_integration.py tests/test_crmnew_live_centinela_and_mcts.py tests/test_e2e_plan_branching_integration.py tests/test_plan_branching_mcts_solver.py tests/test_dream_orchestrator.py -v
```

Run Rust server integration tests:

```bash
cargo test -p ozymem-server -- --nocapture
```
