# Cognitive Supervision & Deterministic Validation

Ozygram includes an architectural supervision subsystem designed to act as an unyielding adversarial code reviewer (*Adversarial Critic*) that audits plans, detects regressions, and halts destructive operations before touching repository files.

---

## 1. The Supervisor and the Adversarial Critic

The system is organized into two complementary roles:

- **`SupervisorAgent`** ([`supervisor.py`](../python/ozy-brain/ozy_brain/agents/supervisor.py)):
  - Orchestrates cognitive requests from the IDE.
  - Coordinates repository telemetry, memory consolidation, and risk audits.
  - Returns structured responses with mitigation plans, confidence scores, and actionable recommendations.
- **`RiskCriticAgent`** ([`risk_critic.py`](../python/ozy-brain/ozy_brain/agents/risk_critic.py)):
  - Adopts an adversarial posture: assumes every proposed change will cause production regressions until proven otherwise.
  - Uncovers subtle regression vectors, signature mismatches, and dangerous coupling.

---

## 2. Deterministic Validation Without LLMs (Offline Mode / $0 Cost)

One of Ozygram's key strengths is that **it does not require a paid language model to safeguard the codebase**. When offline or without API keys configured, `_audit_offline` takes over:

```text
               Proposed Plan or Code Diff
                             │
                             ▼
 ┌───────────────────────────────────────────────────────────────┐
 │               DETERMINISTIC VALIDATION ENGINE                 │
 ├───────────────────────────────────────────────────────────────┤
 │  1. Git Telemetry (DuckDB + Polars)                           │
 │     → Cross-reference with high churn files & historical hotspots│
 │                                                               │
 │  2. Blast Radius Boundary Control                             │
 │     → Does it touch > 8 files concurrently?                  │
 │                                                               │
 │  3. DDL/DML Anti-Destruction Guard                            │
 │     → Regex veto: DROP TABLE, DELETE FROM, TRUNCATE, etc.    │
 │                                                               │
 │  4. Memory Exponential Time Decay                             │
 │     → S = C · e^(-λ·Δt) for mathematical context pruning     │
 └───────────────────────────┬───────────────────────────────────┘
                             │
                             ▼
         Verdict: [LOW | MEDIUM | HIGH | CRITICAL]
         (Preventive operation block if is_blocked=True)
```

### A. Hotspot Telemetry with DuckDB + Polars ([`data_engine.py`](../python/ozy-brain/ozy_brain/data_engine.py))
The analytical engine parses the local Git commit log to index historical metrics for each repository file:
- **Churn Score**: Accumulated volume of lines added and deleted.
- **Fix Commits**: Frequency of file association with commit messages containing `fix`, `bug`, `issue`, or `patch`.
- **Author Churn**: Count of distinct contributors who have modified the file.

When a plan proposes modifying a file flagged as a **CRITICAL HOTSPOT**, the system triggers a warning and requires targeted regression tests.

### B. DDL/DML Anti-Destruction Guard
The supervisor inspects the plan and diff text for destructive database statements:
`DROP TABLE`, `DELETE FROM`, `ALTER TABLE`, `TRUNCATE`, `DROP COLUMN`.
If detected:
- **Blocks the operation (`is_blocked = True`)**.
- Elevates the risk level to **`CRITICAL`**.
- Demands non-destructive migration scripts with explicit rollback safeguards.

### C. Blast Radius Control
If an implementation plan touches more than **8 files concurrently**:
- Flags the risk level as `HIGH` due to architectural dispersion.
- Issues a mandatory recommendation to decompose the work into atomic, independent subtasks.

### D. Memory Consolidation via Exponential Decay
To prune and summarize memory collections without passing thousands of tokens to an LLM:
$$S = C \cdot e^{-\lambda \Delta t}$$
Where:
- $S$: Residual relevance score of the lesson.
- $C$: Initial confidence score.
- $\lambda$: Temporal decay factor.
- $\Delta t$: Days elapsed since the last observation or update.

---

## 3. Optional Semantic Supervision with Free Models

To add advanced semantic reasoning where the supervisor acts as a *Senior Tech Lead*, zero-cost LLM providers are supported:

### Option A: Gemini 2.0 Flash (Google AI Studio Free Tier)
- **Official Free Quota**: 15 requests per minute, 1,000,000 tokens per minute.
- **Performance**: Sub-second latency (~500 ms) with high JSON schema adherence.
- **Activation**:
  ```powershell
  $env:GEMINI_API_KEY = "your_google_ai_studio_key"
  ```

### Option B: Local Ollama (`qwen2.5-coder` / `gemma2`)
- **100% Local & Private**: Runs directly on your machine's CPU or GPU.
- **Activation**: Ozygram automatically detects active Ollama instances at `http://localhost:11434` or configured via `$env:OLLAMA_HOST`.

### Option C: OpenRouter Free Models
- Access models such as `openrouter/nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free` or `openrouter/cohere/north-mini-code:free`.
- **Activation**:
  ```powershell
  $env:OPENROUTER_API_KEY = "your_openrouter_key"
  ```

---

## 4. Strict Token Budget (*Zero Token Bloat*)

Ozygram is designed to **minimize context consumption and conserve API limits**:

1. **Analytical Pre-Filtering**: Before querying any language model, DuckDB and `tgrep` condense repository context into a minimal diff and numerical metrics summary (< 2 KB).
2. **Strict Output Limits**: Configured by default to `max_tokens = 1500` with `temperature = 0.2` for concise, deterministic evaluations.
3. **Structured Pure JSON**: Audits are returned as strongly-typed JSON without verbose conversational filler, minimizing both prompt and completion token counts.
