# High-Performance Tooling ("The Dream Team")

Ozygram integrates with a curated set of high-performance tools written in Rust to form the **"Dream Team"** for AI agent assistance: instant response velocity, minimal memory footprint, and aggressive token economy.

---

## 1. Dream Team Composition

```text
 ┌──────────────────┬────────────────────────────────────────────────────────┐
 │ Tool             │ Strategic Role in Ozygram                              │
 ├──────────────────┼────────────────────────────────────────────────────────┤
 │ tgrep            │ Instant regular expression search using trigram        │
 │ (Microsoft)      │ inverted indexes for massive codebases.                │
 ├──────────────────┼────────────────────────────────────────────────────────┤
 │ rtk              │ Rust Token Killer: Aggressive payload compression,     │
 │ (rtk-ai)         │ ANSI escape stripping, and structural JSON reduction.  │
 ├──────────────────┼────────────────────────────────────────────────────────┤
 │ fastembed        │ Local dense embeddings with C++ ONNX Runtime           │
 │                  │ (zero PyTorch dependencies and zero API costs).        │
 └──────────────────┴────────────────────────────────────────────────────────┘
```

---

## 2. `tgrep` — Trigram Inverted Index Regex Search

Developed by Microsoft, `tgrep` accelerates complex pattern matching using inverted trigram indexes:

### Why It Excels for Ozygram
- **Trigram Indexing**: While traditional linear scanners (like `grep`) inspect every line sequentially, `tgrep` filters out 95% of irrelevant files before evaluating the regular expression.
- **Large Codebase Performance**: Ideal for monorepos with hundreds of thousands of lines where the AI agent needs to locate signatures or call sites within milliseconds.

### CLI Usage
```powershell
# Search using regex pattern
tgrep "fn [a-z_]+\(ctx: &Context\)"

# Case-insensitive symbol search
tgrep -i "class SupervisorAgent"
```

---

## 3. `rtk` (Rust Token Killer) — LLM Token Optimization

Developed by `rtk-ai`, `rtk` specializes in trimming token bloat injected into LLM context windows:

### Core Capabilities
1. **Escape Sequence Stripping**: Removes terminal ANSI colors, carriage returns, and control characters that consume tokens without providing semantic value.
2. **Structural Minification**: Compacts JSON responses, execution logs, and terminal outputs while preserving all syntactic and semantic data.
3. **Context Window Conservation**: Achieves 20% to 45% token reduction across tool result transcripts and environment prompt payloads.

### CLI Usage
```powershell
# Filter and compress long command outputs
cargo test | rtk

# Clean noisy build logs before sending to an LLM
Get-Content build.log | rtk
```

---

## 4. Architectural Synergy with Ozygram

When an AI agent operates alongside Ozygram:
1. **`ozymem-server`** provides structured memory and dependency graph context in < 5 ms.
2. **`tgrep`** allows the agent to trace cross-cutting patterns at trigram speed without filesystem thrashing.
3. **`rtk`** sanitizes and compresses verbose compiler and linter outputs before they enter the LLM context window.
4. **`fastembed`** resolves semantic and vector search requests locally without consuming API credits or risking network latency.
