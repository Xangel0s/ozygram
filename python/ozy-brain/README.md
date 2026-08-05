# Ozy Brain — Hybrid Reasoning Engine for Ozygram / Ozymem

`ozy-brain` is the local Python reasoning worker used by the Rust `ozymem-server` MCP server. It acts as an **advisory cognitive layer** for AI agents, providing planning, deep memory recall, risk reviews, failure reflection, mental model synthesis, and pattern detection.

---

## 🏛️ Architecture & Security Guarantees

```text
MCP Client
   ↓
ozymem-server (Rust — Authority & Side-Effects Controller)
   ↓ (Stdio JSON Payload with context, graph, memories, files)
ozy-brain (Python — Isolated Reasoning Worker)
   ↓ (Stdio JSON Response with structured plans & recommendations)
ozymem-server (Rust)
   ↓ (Formatted Markdown)
MCP Client
```

### Security Principles
1. **Rust is Authority**: Rust controls SQLite database access, MCP tools, filesystem operations, git commands, and process security.
2. **Python is Advisory**: Python code runs in an isolated subprocess (`cmd /c python -m ozy_brain --action <action>`).
3. **No Direct Side-Effects**: Python cannot write files directly, delete database entries, or execute arbitrary shell commands. It only returns advisory recommendations.

---

## 📦 Package Structure

```text
python/ozy-brain/
├─ ozy_brain/
│  ├─ __init__.py        # Package initialization
│  ├─ __main__.py        # Python CLI entrypoint (-m ozy_brain)
│  ├─ main.py            # CLI argument parsing & stdio IO
│  ├─ brain.py           # Central action router/dispatcher
│  ├─ schemas.py         # Data structures (BrainResponse, StructuredPlan, etc.)
│  ├─ planner.py         # Multi-phase planning & candidate file scoring
│  ├─ reflector.py       # Reflection loop, root cause analysis & scope creep
│  ├─ risk.py            # Risk classification & critical path detection
│  ├─ summaries.py       # Mental model synthesizer & session compression
│  ├─ memory.py          # Memory ranking, deduplication & deep recall
│  └─ patterns.py        # Domain rules, user preferences & pattern detection
├─ tests/
│  └─ test_brain.py      # Unit test suite
├─ pyproject.toml
└─ README.md
```

---

## ⚙️ Supported MCP Actions (`ozy_brain`)

| Action | Module | Objective |
| :--- | :--- | :--- |
| `plan` | `planner.py` | Generates a 5-phase structured plan with candidate file scoring & exit conditions |
| `reflect` / `analyze_failure` | `reflector.py` | Analyzes execution failures, root causes, scope creep, and extracts gotchas |
| `risk_review` | `risk.py` | Evaluates auth, data-loss, migration, and architectural risks before edits |
| `build_mental_model` | `summaries.py` | Synthesizes project architecture, module boundaries, data flows & entrypoints |
| `recall_deep` | `memory.py` | Combines observations, lessons, graph summary, and files in priority order |
| `summarize_project` | `summaries.py` | Compact summary of project purpose and graph statistics |
| `detect_patterns` | `patterns.py` | Detects user preferences, domain rules (e.g. CRM), and platform constraints |
| `suggest_next_steps` | `patterns.py` | Recommends smallest safe next milestones |
| `compress_session` | `summaries.py` | Compacts session context for context resets |
| `rank_memories` | `memory.py` | Groups and ranks memories by topic stability and relevance |

---

## 🧪 Testing

Run the Python unit test suite:

```bash
python -m unittest discover -s python/ozy-brain/tests -v
```

Run Rust integration tests:

```bash
cargo test -p ozymem-server ozy_brain -- --nocapture
```
