Here's the full synthesis of what I came up with. These are 14 novel techniques organized from most impactful to most experimental, followed by a concrete architecture and implementation roadmap.

---

## Novel Techniques for a Local Stdio-First Codebase RAG MCP Server

### The Core Problem with Current Approaches

Today's codebase RAG servers treat code as flat text — they embed chunks by line count, return whole files, go stale instantly, can't answer structural queries ("what calls this?"), waste tokens on repeated retrievals, and have zero awareness of your team's conventions. Every one of the ideas below attacks a specific failure mode.

---

### 1. Code Graph Index (Multi-Layered, Not Flat)

Instead of just embedding text chunks, build a **four-layer graph** at index time using tree-sitter AST parsing:

- **AST Layer** — functions, classes, types, exports as nodes; "calls", "imports", "extends", "implements", "tests" as edges
- **Dependency Layer** — module-level and package-level dependency relationships; identify hub nodes vs leaf nodes
- **Embedding Layer** — vector embeddings at the *function/class level* (not arbitrary line ranges), each tagged with graph position
- **Annotation Layer** — git blame (who, when, how recently), test coverage signals, complexity metrics, comment density

Retrieval becomes **graph-aware**: searching "authentication middleware" returns not just the file, but its callers → its dependencies → its tests → its types, all ranked by relevance AND structural proximity.

### 2. Live Incremental Indexing via Filesystem Watchers

Use chokidar/watchdog to detect changes in real-time, but do **incremental graph updates** — not full re-index. On file save: re-parse only that file's AST, diff old vs new AST, update only changed nodes + edges, re-embed only changed chunks, propagate "staleness" signals up the dependency graph. Track the agent's own writes too — if the agent modifies a file via another tool, the RAG server should reflect that instantly via MCP notifications.

### 3. Progressive Disclosure with Context Budgeting

A tiered retrieval system inspired by Cloudflare's Code Mode:

- **Tier 0 — Codebase Map** (~200 tokens): Compressed structural overview, directory tree, module descriptions, entry points. Always available.
- **Tier 1 — Skeleton View** (~500 tokens/result): Signatures, class outlines, type defs, docstrings. No implementation bodies.
- **Tier 2 — Focused Chunks** (~1000 tokens/result): Specific function bodies. Loaded on demand.
- **Tier 3 — Full Context**: Complete files, git history, related tests. Only when explicitly needed.

Tools: `codebase_map()`, `search(query, depth)`, `expand(node_id)`, `related(node_id, relationship)`. The agent discovers level by level — easily 80-90% token savings vs. naive file retrieval.

### 4. Session-Aware Retrieval with Exploration Memory

The server maintains a **session context** — tracking every file, function, and type the agent has accessed. This enables: deprioritizing already-seen content, boosting "frontier" nodes (adjacent to explored but not yet visited), a `what_havent_i_seen(topic)` tool showing unexplored areas, and a `suggested_next()` tool that anticipates what context the agent needs based on its exploration pattern. Essentially a working memory that mirrors the agent's reasoning trajectory.

### 5. Convention Mining & Pattern Detection

At index time, detect the codebase's conventions: error handling patterns, naming conventions, file organization, testing patterns, API patterns, state management, import organization. Store as structured **"convention cards"** exposed as MCP resources (`conventions://error-handling`, `conventions://testing`). When the agent writes new code, it pulls the relevant convention card. Also detect anti-patterns and migration directions ("3 error handling styles exist — here's which is newest/most common").

### 6. Hybrid Retrieval with Automatic Strategy Fusion

Fuse five retrieval strategies behind a single smart `search()` tool:

1. **Semantic** (embeddings) — natural language queries
2. **Structural** (graph) — "all callers of X"
3. **Lexical** (ripgrep/trigram) — exact symbols, regex
4. **Type-aware** (AST) — "functions accepting Request returning Response"
5. **Git-aware** — "code changed this week related to payments"

Auto-route via query classification, or expose individual strategies for explicit control. Score fusion via Reciprocal Rank Fusion with weights that adapt to the codebase.

### 7. Code Mode for Complex Queries

Instead of 15+ tools, expose a `query_codebase(code)` tool where the agent writes a script against a typed SDK executed in a sandboxed environment:

```typescript
const authFns = await index.search("auth", { kind: "function" });
const callers = await index.graph.callers(authFns[0].id, { depth: 2 });
const tests = await index.graph.related(authFns[0].id, "tested_by");
return { fn: authFns[0].skeleton(), callers: callers.map(c => c.signature()), tested: tests.length > 0 };
```

Single round-trip, complex logic, data filtered before entering context. Sandbox = restricted Node.js/Python subprocess with only the index SDK available.

### 8. Diff-Aware Impact Analysis

A `impact_analysis(diff)` tool that: parses diffs to identify changed symbols, walks the dependency graph outward, categorizes impact as direct/indirect/type-level, returns a structured risk report. Also `safe_to_change(symbol)` — risk assessment based on dependents, test coverage, recency. Critical for pre-commit checks, code review, and safe refactoring.

### 9. Semantic-Boundary Chunking with AST Fingerprinting

Chunk at **AST boundaries** (function + its types = one chunk, test describe block = one chunk), not line counts. Each chunk carries: graph position, importance score, semantic fingerprint (hash of AST structure, not text). The fingerprint means reformatting/whitespace changes don't invalidate the index — only structural changes trigger re-embedding. Massive efficiency gain for active codebases.

### 10. Contextual Prompt Templates (MCP Prompts Primitive)

Expose curated **MCP prompts** that dynamically assemble optimal context for common workflows:

- `prompts/implement-feature` — retrieves module structure, conventions, similar features, test patterns
- `prompts/review-code` — pulls impact analysis, conventions, related tests for a diff
- `prompts/debug-error` — given a stack trace, retrieves source, similar past errors, error handling patterns
- `prompts/write-tests` — testing conventions, fixtures, code under test, example tests
- `prompts/refactor-safely` — full dependency graph, coverage, impact analysis

Each prompt encodes a **retrieval strategy as a reusable pattern**.

### 11. Multi-Language Cross-Linking

Use tree-sitter (100+ languages, unified API) to parse polyglot codebases and build **cross-language edges**: TypeScript fetch() → Python API endpoint → SQL query. Parse API contracts in both frontend and backend, match by URL pattern. Link SQL usage in code to migration files. Link env variable usage across languages to .env files. Parse protobuf/gRPC definitions and link to implementations.

### 12. Structured Outputs with Self-Correction Metadata

Every response includes: `confidence` (high/medium/low from similarity scores), `coverage` (how comprehensive), `suggestions` (alternative queries if results are weak), `related_tools` (what else might help). The agent uses this to self-correct — low confidence triggers a different query strategy.

### 13. Git-Temporal Retrieval ("Time Travel")

- `evolution(symbol)` — how a function changed over time (diff timeline)
- `blame_context(file, lines)` — who, when, and WHY (commit messages are gold)
- `related_changes(symbol)` — commits that modified this symbol AND other things together, revealing **temporal coupling** invisible to static analysis
- `regression_search(test)` — binary-search git history for the commit that broke a test

### 14. Stale Documentation Detection

Cross-reference docs (README, JSDoc, wiki, comments) with current code via embedding similarity. Flag when doc descriptions have diverged from implementation. Expose `explain(symbol)` that returns best available docs with freshness indicators. If no docs exist, synthesize from code skeleton + test descriptions.

---

## Recommended Architecture

```
┌─────────────────────────────────────────────────┐
│                  MCP Server (stdio)              │
├─────────────┬───────────────┬───────────────────┤
│   TOOLS     │   RESOURCES   │     PROMPTS       │
│ search()    │ conventions:// │ implement-feature │
│ expand()    │ codebase-map://│ review-code       │
│ graph_query │ session://     │ debug-error       │
│ impact()    │               │ write-tests       │
│ query_code()│               │ refactor-safely   │
├─────────────┴───────────────┴───────────────────┤
│              SESSION MEMORY                      │
│  exploration tracking · anticipatory pre-fetch   │
├─────────────────────────────────────────────────┤
│              RETRIEVAL FUSION                    │
│  semantic · structural · lexical · type · git    │
├─────────────────────────────────────────────────┤
│              INDEX LAYER                         │
│  tree-sitter AST · sqlite-vec · FTS5 · git2     │
├─────────────────────────────────────────────────┤
│              LIVE SYNC                           │
│  filesystem watcher · incremental re-index       │
└─────────────────────────────────────────────────┘
```

**Tech stack for zero-dependency local operation:** Tree-sitter (AST), SQLite + FTS5 (lexical + metadata), sqlite-vec or usearch (vectors), bundled ONNX model like nomic-embed (offline embeddings), chokidar/watchdog (file watching), isolated-vm or RestrictedPython (Code Mode sandbox). Ship as a single `npx`-installable package — no Docker, no API keys, fully offline.

**Build order by impact/effort ratio:**
1. **Weeks 1-2**: Tree-sitter AST + hybrid search (ripgrep + embeddings + RRF) + progressive disclosure + live watcher
2. **Weeks 3-4**: Code graph + graph_query tool + impact analysis + convention detection
3. **Weeks 5-6**: Session memory + git-temporal retrieval + structured output metadata
4. **Weeks 7+**: Code Mode sandbox + cross-language linking + workflow prompts + staleness detection