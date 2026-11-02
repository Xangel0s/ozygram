# Trajectory Replay & Exploration Tuner (v0.2.4) [EXPERIMENTAL]

> **Scientific Attribution**: The offline dreaming exploration concepts in this module are inspired by the framework established in *"Dream-RSI: Recursive Self-Improvement through Evolving Worlds"* (Zheng et al. / Google DeepMind, University of Maryland, University of Virginia, [arXiv:2609.14858](https://arxiv.org/abs/2609.14858)).

---

## 1. Implementation Status & Reality Matrix

| Capability | Status | Description | Verified Source in Repo |
| :--- | :--- | :--- | :--- |
| **SQLite WAL Decision Graph** | `[IMPLEMENTED]` | Persistent table `exploration_nodes` recording agent action hypotheses, parent-child edges, and token metrics. | `crates/ozymem-core/src/graph_backend/exploration.rs` |
| **Filesystem State Rollback** | `[IMPLEMENTED]` | `rollback_snapshot`, `rollback_node`, and `rollback_to_parent` for atomic branch backtracking. | `crates/ozymem-core/src/graph_backend/queries.rs` |
| **Isolated Execution Sandbox** | `[IMPLEMENTED]` | Default-Deny ephemeral worktree runner (`EphemeralWorktree`) with credential scrubbing and execution timeout. | `crates/ozymem-core/src/git_backend.rs` |
| **Objective Execution Evaluator** | `[IMPLEMENTED / ETAPA 1]` | Structured process receipts (`exit_code`, test counts) decoupled from agent-written observation text. | `crates/ozymem-core/src/graph_backend/evaluator.rs` |
| **Shadow Mode Tracking** | `[IMPLEMENTED / ETAPA 2]` | Non-intervening background telemetry measuring real branching factor, resume adherence, and token resource balance. | `crates/ozymem-core/src/graph_backend/exploration.rs` |
| **Frontier Replay Simulator** | `[IMPLEMENTED / ETAPA 3]` | Controlled simulation over synthetic task worlds (`SyntheticTaskWorld`) evaluating multi-step state transitions in RAM. | `python/ozy-brain/tests/test_dream_rsi.py` |
| **Holdout Promotion Gate** | `[IMPLEMENTED / ETAPA 4]` | Blind statistical evaluation gating policy promotions on unobserved tasks to prevent training episode overfitting. | `crates/ozymem-core/src/graph_backend/holdout_gate.rs` |
| **External 3-Arm Benchmark** | `[IMPLEMENTED / ETAPA 5]` | 30 reproducible multi-step tasks measuring base LLM vs greedy MCTS vs Trajectory Replay Tuner (67.6% token reduction). | `scripts/bench_external_3arm.py` |
| **Autonomous Policy Code Synthesis** | `[PLANNED]` | Closed-loop LLM rewriting of exploration policy source code evaluated against holdout test suites (Full Paper Milestone). | Under active research |

---

## 2. Architecture & Operational Pipeline (v0.2.4)

The Trajectory Replay Tuner operates across two complementary timescales: real-time in-session exploration and idle offline consolidation.

```mermaid
flowchart TD
    A[Agent Action Proposal] --> B[simulate_action / simulate_patch in RAM]
    B -->|Calculate Blast Radius Depth 2| C{Blast Radius & Caller Check}
    C -->|High Risk: Breaks Callers| D[[ALERT: HIGH_BLAST_RADIUS: VETO OR SNAPSHOT]]
    C -->|Low Risk: Compatible| E[Proceed with Hypothesis]
    
    D --> F[Capture File Snapshot: rollback_snapshot]
    E --> F
    
    F --> G[Execute Action in Codebase / Sandbox]
    G --> H{Objective Execution Evaluator}
    H -->|Test Passed exit 0| I[[OBJECTIVE: TEST_PASSED] +1.0 Reward]
    H -->|Syntax or Compiler Error| J[[ALERT: SYNTAX_OR_LINT_ERROR] -0.8 Reward]
    H -->|False Claim Without Receipts| K[[ALERT: FALSE_SOLUTION_REJECTED] -1.0 Reward]
    
    J --> L[Prune Node: is_pruned=true]
    K --> L
    L --> M[Atomic Backtracking: rollback_to_parent / rollback_node]
    M --> N[Disk State Cleaned & Files Restored]
    
    I --> O[Consolidate Solution Path]
    
    O --> P[get_tree: Clean Mermaid Decision Tree]
    N --> P
```

---

## 3. Scientific Grounding & The 5-Stage Audit (v0.2.4 Updates)

Version v0.2.4 formalizes the transition from metaphorical language to measurable, reproducible computer science foundations:

### Stage 1: Objective Execution Evaluator (`ObjectiveExecutionEvaluator`)
* **Problem**: Overly optimistic AI agents previously awarded themselves `reward_score: 1.0` by generating persuasive natural language text, even when builds crashed or tests failed.
* **v0.2.4 Solution**:
  - Replaces text-based LLM evaluations with verifiable ground-truth signals from the operating system (`exit_code == 0` yields `+1.0`, non-zero yields `-0.8`).
  - Tree-Sitter AST parser rejects syntax errors with a `-1.0` penalty.
  - `ObservationGuard` enforces a strict 32 KB truncation limit, preventing length-hacking attacks where agents inflate scores through excessive verbosity.

### Stage 2: Shadow Mode Telemetry (`ShadowExplorationMetrics`)
* **Problem**: Offline simulations previously asserted theoretical benefits without tracking whether real agent sessions actually followed the proposed exploration paths.
* **v0.2.4 Solution**:
  - Runs passively in the background without blocking interactive chat turns.
  - Measures **Real Branching Factor**: Quantifies the effective decision space explored per milestone.
  - Measures **Resume Adherence**: Tracks the exact percentage of times the agent resumes execution from the recommended frontier node.
  - Implements **Token Resource Accounting**: Verifies whether simulation compute costs are offset by token reductions during real editing turns.
  - Inspected via CLI: `ozymem dream shadow`.

### Stage 3: Frontier Replay Simulator & Synthetic Task Worlds (`SyntheticTaskWorld`)
* **Problem**: Replaying historical trajectories directly on a dirty working tree risks contaminating active code and producing non-deterministic outcomes.
* **v0.2.4 Solution**:
  - Implements controlled synthetic environments representing multi-step coding scenarios (`SyntaxRepair`, `MultiFileRefactor`, `BreakingApiMitigation`).
  - Evaluates candidate mutations against deterministic contract predicates in volatile RAM at $0 API cost.

### Stage 4: Holdout Promotion Gate (`HoldoutPromotionGate`)
* **Problem**: Heuristics discovered in one session could overfit to that specific error, promoting rules that break unrelated modules in subsequent sessions.
* **v0.2.4 Solution**:
  - Requires candidate exploration policies to be evaluated against unseen holdout partitions (*holdout split*).
  - Enforces a minimum statistical threshold ($p < 0.05$ or net reward gain $\ge 2\sigma$ over baseline) before promoting heuristics into persistent `.ozy/memories/gotchas/` vaccines.

### Stage 5: External 3-Arm Benchmark Suite (`bench_external_3arm.py`)
* **Replication**: Automated evaluation across 30 reproducible tasks comparing:
  - **Arm 1 (Base LLM)**: Blind search baseline without memory or MCTS (0% solved on syntax traps, 2,660 tokens).
  - **Arm 2 (Greedy MCTS)**: Greedy local search (100% solved, 940 tokens).
  - **Arm 3 (Trajectory Replay Tuner)**: RAM-pruned MCTS with bounded observation guards (100% solved, 305 tokens, **67.6% token reduction**).
* **Command to run**: `python scripts/bench_external_3arm.py`

---

## 4. Mathematical UCB1 / UCT Formulation

Each decision node balances exploitation of validated paths with exploration of uncertain branches using the Upper Confidence Bound for Trees:

$$\text{UCT}(v_i) = Q(v_i) + c \cdot \sqrt{\frac{\ln N(v)}{N(v_i)}} - \beta \cdot \text{threat\_scar} + \text{bonus}_{\text{solution}}$$

Where:
* $Q(v_i) = \frac{\sum R}{N(v_i)}$ is the running average empirical reward of the branch.
* $N(v)$ is the parent node's cumulative visit count.
* $N(v_i)$ is the candidate child node's visit count.
* $c$ is the exploration parameter (calibrated to $\sqrt{2} \approx 1.4142$ by default, adapted dynamically up to $2.400$ in high-entropy states).
* $\beta \cdot \text{threat\_scar}$ introduces risk-aversion penalty if the module has a history of recent compiler regressions.
* $\text{bonus}_{\text{solution}} = 2.0$ accelerates convergence once an objectively verified solution node is identified.

---

## 5. CLI Command Reference

Developers can manage, inspect, and diagnose trajectory trees using the native `ozymem` CLI:

```bash
# Query active MCTS exploration policy status and global metrics
ozymem dream status

# Output policy metrics in structured JSON for CI/CD pipelines
ozymem dream status --json

# View shadow mode exploration metrics (branching factor, token balance, adherence)
ozymem dream shadow

# Run an offline counterfactual replay dream simulation ($0 LLM tokens)
ozymem dream run

# Diagnose bottlenecks, latency, and wasted turns in a specific trajectory
ozymem dream diagnose traj_5bbd6b4e5c6f

# Export the MCTS decision trajectory forest as an interactive Mermaid diagram
ozymem dream tree
```

---

## 6. MCP Tool Reference (`ozy_plan` & `ozy_exploration`)

Modern agents invoke the Trajectory Replay Tuner through high-level meta-tools:

```json
{
  "name": "ozy_plan",
  "arguments": {
    "goal": "Refactor authentication service to use async connection pools without breaking existing callers",
    "critique_role": "architect | security_critic | code_reviewer | test_engineer"
  }
}
```

For granular decision tree manipulation, the unified `ozy_exploration` interface remains available:

```json
{
  "name": "ozy_exploration",
  "arguments": {
    "action": "start | record_step | record_batch | complete | get_tree | diagnose | list | delete | resume | rollback_snapshot | rollback_node | rollback_to_parent",
    "trajectory_id": "traj_123",
    "node_id": "node_456",
    "files": ["src/service.py", "src/models.py"],
    "task_description": "Task objective",
    "steps": [...],
    "parent_id": "optional (smart auto-parenting enabled)",
    "reward_score": 1.0,
    "is_solution": true,
    "is_pruned": false
  }
}
```

---

## 7. Citations & Scientific Attribution

The trajectory replay mechanisms in Ozygram are directly based on the algorithmic framework established by:

* **Paper**: *"Dream-RSI: Recursive Self-Improvement through Evolving Worlds"*
* **Authors**: Zheng et al. (Google Research, Google DeepMind, University of Maryland, University of Virginia)
* **ArXiv**: [arXiv:2609.14858](https://arxiv.org/abs/2609.14858)
* **Project Website**: [https://www.dream-rsi.com/](https://www.dream-rsi.com/)
* **Official Repository**: [https://github.com/zhengkid/Dream-RSI](https://github.com/zhengkid/Dream-RSI)

### BibTeX
```bibtex
@article{zheng2026dreamrsi,
  title={Dream-RSI: Recursive Self-Improvement through Evolving Worlds},
  author={Zheng, et al.},
  journal={arXiv preprint arXiv:2609.14858},
  year={2026},
  url={https://arxiv.org/abs/2609.14858}
}
```
