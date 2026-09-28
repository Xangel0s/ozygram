# Dream-RSI & Monte Carlo Tree Search (MCTS) v1.2.0
 
**Dream-RSI** (*Recursive Self-Improvement via Offline MCTS Replay*) is Ozygram's continuous self-improvement subsystem. It elevates AI coding agents from flat, ephemeral chat histories into a **computable, structured, and optimizable exploration graph**.
 
---
 
## What's New in v1.2.0 (State Rollback, Action Simulation & Visual Decision Trees)

Version v1.2.0 establishes Dream-RSI as a proactive decision and recovery engine:

```mermaid
flowchart TD
    A[Agent Action Proposal] --> B[simulate_action / critique_hypothesis]
    B -->|Calculate Blast Radius Depth 2| C{Blast Radius Check}
    C -->|High Risk: Impacts Dependents| D[[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]]
    C -->|Low Risk| E[Proceed with Hypothesis]
    
    D --> F[Capture File Snapshot: rollback_snapshot]
    E --> F
    
    F --> G[Execute Action in Codebase]
    G --> H{Objective Evaluator}
    H -->|Test Passed exit 0| I[[OBJECTIVE: TEST_PASSED] +1.0 Reward]
    H -->|Syntax/Lint Error| J[[ALERT: SYNTAX_OR_LINT_ERROR] -0.8 Reward]
    H -->|False Solution Attempt| K[[ALERT: FALSE_SOLUTION_REJECTED] -1.0 Reward]
    
    J --> L[Prune Node: is_pruned=true]
    K --> L
    L --> M[Atomic Backtracking: rollback_to_parent / rollback_node]
    M --> N[Disk State Cleaned & Files Restored]
    
    I --> O[Consolidate Solution Path]
    
    O --> P[get_tree: Clean Mermaid Tree without emojis]
    N --> P
```

### 1. State Rollback & Backtracking with File Snapshots
* **The Problem**: When an agent explores a hypothesis by editing code and that path fails (`is_pruned = true`), discarded changes remain in the repository, polluting git status and breaking subsequent turns.
* **v1.2.0 Solution**:
  - `rollback_snapshot`: Captures file states associated with an exploration node.
  - `rollback_node`: Restores tracked files on disk to that node's recorded snapshot.
  - `rollback_to_parent`: Automatically reverts disk files to the parent node state upon pruning or backtracking.

### 2. Action Simulation & Blast Radius Pre-Flight (`simulate_action`)
* Pre-flights changes via `ozy_brain` before touching disk:
  - Computes AST dependency blast radius up to 2 hops.
  - Detects if changes impact core architecture or multiple dependent modules.
  - Issues explicit safety directives: `[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]`.
  - Proposes `recommended_resume_node` for safe fallback.

### 3. Strict Objective Auto-Reward & Anti-Hallucination Guard
* Eliminates subjective or false positive scoring:
  - Exit code 0 / clean test verification -> `+1.0` (`[OBJECTIVE: TEST_PASSED]`).
  - Syntax error, build crash, or lint failure -> `-0.8` (`[ALERT: SYNTAX_OR_LINT_ERROR]`) with UCT auto-prune.
  - False claim (`is_solution = true` despite failed tests) -> Clamped to `-1.0`, rejected (`[ALERT: FALSE_SOLUTION_REJECTED]`), and pruned immediately.

### 4. Emoji-Free Mermaid Decision Trees (`get_tree`)
* In `ozy_exploration get_tree` and CLI `ozymem dream tree`, outputs clean Mermaid diagrams with textual status tags:
  - `[STATUS: ACTIVE]` for in-progress nodes.
  - `[ALERT: PRUNED]` for dead-end or rejected branches.
  - `[OBJECTIVE: SOLVED / TEST_PASSED]` for verified solutions.
  - `[ALERT: HIGH_BLAST_RADIUS: SNAPSHOT REQUIRED]` for high-risk nodes.

### 5. Stale Memory Reality Check against File MTime
* In `ozy_brain` and `ozy_context`, when recalling lessons related to source files:
  - Compares the lesson timestamp against the target file's physical `mtime` and existence on disk.
  - If the file was modified more recently than the memory, inverts trust and flags `[ALERT: STALE_MEMORY]`, prompting the agent to verify the active code first.

---

## Why Dream-RSI?
 
Traditional AI coding agents repeat the same failure modes: when exploring solutions (e.g. testing a SQL query, trying a library, refactoring a function), if an attempt fails, they either discard the entire context or bury it in unstructured chat logs.
 
Dream-RSI solves this through two complementary phases:
1. **Online Phase (Live Execution)**: As the agent works toward a goal, it persists every decision in a **hierarchical MCTS Tree** backed by SQLite (`exploration_nodes`), with real-time upward backpropagation ($Q \leftarrow Q + \frac{R-Q}{N}$).
2. **Offline Phase ("Dreaming")**: In the background (or on-demand via `ozymem dream run`), a counterfactual replay simulator re-traverses the exploration tree **at $0 LLM token cost**, diagnoses wasted turns, tunes the UCB1 exploration policy, and promotes verified lessons without regression risks.
 
---
 
## What's New in v1.1.0 (Zero-Friction Engine)
 
Based on operational observations in production workflows, version v1.1.0 addresses core friction points in the agentic MCTS lifecycle:
 
```mermaid
flowchart TD
    A[MCP Agent / IDE] -->|Single step without parent_id| B(Rust Core Auto-Parenting)
    A -->|Milestone batch| C(Atomic record_batch)
    
    B --> D[(exploration_nodes in SQLite)]
    C --> D
    
    D --> E{Objective Failure Detector}
    E -->|Exit code != 0 / Timeout / Tests Failed| F[Auto-Pruning: is_pruned=true & clamp reward]
    E -->|Clean Execution| G[Standard MCTS Reward]
    
    F --> H[Backpropagation Q-Value]
    G --> H
    
    D --> I[ozymem dream diagnose / ozy_exploration diagnose]
    I --> J[Deadlock-Free Latency, Token & Bottleneck Report]

    D --> K[ozy_exploration resume]
    K --> L[Optimal Active Leaf for Resumption]
```
 
### 1. Smart Rust Core Auto-Parenting
* **Previous Pain Point**: Models had to track and propagate complex parent hashes like `node_18d76e...` across turns. If context was compacted, tree topology broke.
* **v1.1.0 Solution**: When `parent_id` is omitted in `record_step`, Rust Core automatically queries:
  ```sql
  SELECT id, depth FROM exploration_nodes 
  WHERE trajectory_id = ?1 AND is_pruned = 0
  ORDER BY depth DESC, created_at DESC LIMIT 1;
  ```
  It links the step to the latest active unpruned node and sets `depth = parent.depth + 1`. An explicit `parent_id` is only required when branching or backtracking.
 
### 2. Milestone Batch Mode (`record_batch`)
* **Previous Pain Point**: For 5 linear technical actions, the agent made 5 separate roundtrip MCP calls, multiplying latency and token overhead (*turn tax*).
* **v1.1.0 Solution**: Submit milestone chains atomically in a single MCP call:
  ```json
  {
    "action": "record_batch",
    "trajectory_id": "traj_123",
    "steps": [
      { "action_type": "audit", "observation": "Initial diagnostic complete", "cost_tokens": 120 },
      { "action_type": "refactor", "observation": "Role cache implemented", "cost_tokens": 300 },
      { "action_type": "test", "observation": "34 tests passing 100%", "reward_score": 1.0, "is_solution": true }
    ]
  }
  ```
  Nodes are chained sequentially within a single SQLite transaction.
 
### 3. Objective Failure Auditing & Self-Pruning
* **Previous Pain Point**: Overly optimistic models assigned `reward_score: 1.0` even when builds broke or tests failed.
* **v1.1.0 Solution**: The deterministic `detect_failure_signals` parser checks observations for failure indicators (`exit code != 0`, `FAILED`, `SyntaxError`, `TypeError`, `timed out`, `failures: `).
  - Automatically overrides inflated rewards to `< 0.0` with `objective_reward_override: true`.
  - Automatically flags `is_pruned: true` with `auto_pruned: true` in the action payload, pruning dead-end branches immediately without waiting for model introspection.
 
### 4. Deadlock-Free Native Trajectory Diagnostics (`diagnose`)
* Quantitative bottleneck audit via MCP or CLI with guaranteed cycle prevention:
  - High-latency nodes (>3000 ms).
  - High token cost nodes (>2000 tokens).
  - Pruning opportunities (negative reward nodes left unpruned).
  - Solution token efficiency percentage (tokens on winning path vs total trajectory tokens).
  - Dynamic UCB1 exploration constant $c$ calibration based on reward variance.
  - Automatic fallback to the most recent trajectory if `trajectory_id` is omitted.
 
### 5. Automatic Trajectory Resumption (`resume`)
* Recovers interrupted trajectories after context compactions or chat resets:
  - Queries the highest value active leaf node (`ORDER BY value_estimate DESC, depth DESC, created_at DESC`).
  - Returns `recommended_resume_node_id` so the agent can resume exploration from the most promising frontier.
 
---
 
## 🔬 Mathematical UCB1 / UCT Formulation
 
Each exploration node balances exploration and exploitation using the Upper Confidence Bound for Trees:
 
$$\text{UCT}(v_i) = Q(v_i) + c \cdot \sqrt{\frac{\ln N(v)}{N(v_i)}}$$
 
Where:
* $Q(v_i)$ is the running average value estimate of the branch.
* $N(v)$ is the parent node's visit count.
* $N(v_i)$ is the child node's visit count.
* $c$ is the exploration constant (default $\sqrt{2} \approx 1.4142$, dynamically calibrated).
 
---
 
## 💻 CLI Command Reference
 
```bash
# Query active policy status and global MCTS metrics
ozymem dream status

# Structured JSON output
ozymem dream status --json

# Run offline counterfactual simulation
ozymem dream run

# Diagnose bottlenecks in a specific trajectory
ozymem dream diagnose traj_5bbd6b4e5c6f

# Structured JSON diagnostic output for CI/CD pipelines
ozymem dream diagnose traj_5bbd6b4e5c6f --json
```
 
---
 
## MCP Tool Reference (`ozy_exploration` & `ozy_brain`)
 
```json
{
  "name": "ozy_exploration",
  "arguments": {
    "action": "start | record_step | record_batch | complete | get_tree | diagnose | list | delete | resume | rollback_snapshot | rollback_node | rollback_to_parent",
    "trajectory_id": "traj_123",
    "node_id": "optional target node id",
    "files": ["optional file paths list for snapshot/rollback"],
    "task_description": "Task objective",
    "steps": [...],
    "parent_id": "optional (auto-parenting enabled)",
    "reward_score": 1.0,
    "is_solution": true,
    "is_pruned": false
  }
}
```

```json
{
  "name": "ozy_brain",
  "arguments": {
    "action": "simulate_action | critique_hypothesis | ...",
    "hypothesis": "Proposed implementation or code change hypothesis",
    "action_type": "edit_code | refactor | run_test",
    "files_affected": ["src/service.py", "src/models.py"],
    "diff_preview": "Optional diff preview for AST blast radius analysis"
  }
}
```
