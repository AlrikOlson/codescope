/**
 * E2E Agent Test — validates CodeScope MCP tools via Claude Agent SDK.
 *
 * Tests two categories:
 *   1. Tool mechanics — cs_status, cs_search, cs_grep, cs_read all work
 *   2. Semantic relevance — the agent is given ground-truth file→purpose
 *      mappings and must craft its OWN natural language queries (no filename
 *      keywords allowed) to test whether semantic search surfaces the right
 *      files. The agent decides how to probe; we validate the results.
 *
 * Exit codes: 0 = pass, 1 = fail
 */

import { runAgent, writeStepSummary } from "./lib/agent.mjs";

const MAX_TURNS = 30;
const MAX_BUDGET_USD = 3.0;

// Ground truth: files and their conceptual purpose.
// The agent must craft queries that describe the purpose WITHOUT using
// words from the filename, then check if the file ranks in top 3.
const GROUND_TRUTH = [
  { file: "server/src/semantic.rs", purpose: "BERT embedding generation, vector similarity, model loading, embedding cache" },
  { file: "server/src/watch.rs", purpose: "filesystem change detection, live re-indexing when files are modified" },
  { file: "server/src/stubs.rs", purpose: "structural code outline extraction, function/class signature summarization" },
  { file: "server/src/fuzzy.rs", purpose: "approximate string matching algorithm for filename search" },
  { file: "server/src/budget.rs", purpose: "token budget allocation and context window management" },
];

const MIN_PROBES = GROUND_TRUTH.length;

const outputSchema = {
  type: "object",
  properties: {
    status_ok: { type: "boolean" },
    search_ok: { type: "boolean" },
    grep_ok: { type: "boolean" },
    read_ok: { type: "boolean" },
    repos_indexed: { type: "number" },
    total_files: { type: "number" },
    languages: { type: "array", items: { type: "string" } },
    grep_match_count: { type: "number" },
    semantic_probes: {
      type: "array",
      items: {
        type: "object",
        properties: {
          query: { type: "string" },
          expected_file: { type: "string" },
          found_in_top_n: { type: "boolean" },
          actual_rank: { type: "number" },
          top_3_files: { type: "array", items: { type: "string" } },
          used_semantic: { type: "boolean" },
        },
        required: ["query", "expected_file", "found_in_top_n", "actual_rank", "top_3_files", "used_semantic"],
      },
    },
    errors: { type: "array", items: { type: "string" } },
  },
  required: ["status_ok", "search_ok", "grep_ok", "read_ok", "repos_indexed", "total_files", "semantic_probes"],
};

const groundTruthTable = GROUND_TRUTH.map(
  (g) => `  - ${g.file} — ${g.purpose}`
).join("\n");

const systemPrompt = `You are a QA agent testing whether CodeScope's semantic search produces relevant results.
You have 4 tools: cs_status, cs_search, cs_grep, cs_read.

## Phase 1: Setup
Call cs_status to verify the index is ready and semantic search is available.

## Phase 2: Semantic Relevance Testing (the core of this test)
Below are ground-truth files and what they do:
${groundTruthTable}

For EACH file above, you must:
1. Craft a natural language search query that describes what the file does.
2. Your query MUST NOT contain words from the filename (e.g. for fuzzy.rs, don't use "fuzzy").
3. Call cs_search with your query.
4. Check if the expected file appears in the top 3 results.
5. Note whether results are tagged [[semantic]] or [[both]] (proving embeddings were used, not just keywords).

Be creative with your queries — describe the concept, not the implementation. Think about what a developer would search for when looking for this functionality.

## Phase 3: Mechanical Checks
- cs_search: set search_ok=true if ALL semantic probes above returned results (cs_search is the tool used for every probe).
- cs_grep with query "pub fn" and ext "rs" — verify matching lines are shown.
- cs_read with path "server/src/main.rs" and mode "stubs" — verify structural outline is returned.

## Reporting
For each semantic probe, report:
- query: the exact query you used
- expected_file: which ground-truth file you were targeting
- found_in_top_n: did it appear in top 3?
- actual_rank: 1-based position (0 if not found at all)
- top_3_files: paths of the first 3 results
- used_semantic: true if any results had [[semantic]] or [[both]] tags

Do NOT retry failed tools. If a tool errors or returns empty, mark it failed and move on.`;

const prompt = `Test CodeScope's semantic search relevance by probing all ${GROUND_TRUTH.length} ground-truth files. Report structured findings.`;

async function main() {
  console.error(`[e2e] Starting agent test (maxTurns=${MAX_TURNS}, maxBudget=$${MAX_BUDGET_USD})`);
  console.error(`[e2e] Ground truth files: ${GROUND_TRUTH.map((g) => g.file).join(", ")}`);
  const start = Date.now();

  let agentResult;
  try {
    agentResult = await runAgent({
      prompt,
      systemPrompt,
      maxTurns: MAX_TURNS,
      maxBudgetUsd: MAX_BUDGET_USD,
      codeScopeOnly: true,
      logLabel: "e2e-test",
      outputFormat: { type: "json_schema", schema: outputSchema },
    });
  } catch (err) {
    console.error(`[e2e] Agent failed: ${err.message}`);
    writeStepSummary(`## E2E Agent Test\n\n**Status:** FAIL — ${err.message}\n`);
    process.exit(1);
  }

  const { usage, structured_output: result } = agentResult;
  const elapsed = ((Date.now() - start) / 1000).toFixed(1);
  console.error(`[e2e] Agent completed in ${elapsed}s`);

  if (!result) {
    console.error("[e2e] FAIL: No structured output from agent");
    writeStepSummary(`## E2E Agent Test\n\n**Status:** FAIL — no structured output\n**Turns:** ${usage.turns} | **Cost:** $${usage.totalCostUsd.toFixed(2)}\n`);
    process.exit(1);
  }

  // Write result for CI artifact collection
  console.log(JSON.stringify(result, null, 2));

  // ── Validate tool mechanics ──
  const mechChecks = ["status_ok", "search_ok", "grep_ok", "read_ok"];
  const mechFailures = mechChecks.filter((t) => !result[t]);

  // ── Validate semantic relevance ──
  const probes = result.semantic_probes || [];
  const semFailures = [];

  if (probes.length < MIN_PROBES) {
    semFailures.push(`Only ${probes.length}/${MIN_PROBES} ground-truth files probed (expected all ${MIN_PROBES})`);
  }

  let semHits = 0;
  for (const probe of probes) {
    if (!probe.found_in_top_n) {
      semFailures.push(`"${probe.query}" → expected ${probe.expected_file} in top 3, got rank ${probe.actual_rank} (top: ${probe.top_3_files.join(", ")})`);
    } else {
      semHits++;
    }
    if (!probe.used_semantic) {
      semFailures.push(`"${probe.query}" → no semantic tags (keyword-only fallback — is the index loaded?)`);
    }
  }

  // ── Step summary ──
  const icon = (ok) => (ok ? "pass" : "FAIL");
  const probeRows = probes
    .map(
      (p) =>
        `| \`${p.query.substring(0, 45)}\` | ${p.expected_file.replace("server/src/", "")} | ${p.found_in_top_n ? `#${p.actual_rank}` : "MISS"} | ${p.used_semantic ? "yes" : "no"} | ${icon(p.found_in_top_n && p.used_semantic)} |`
    )
    .join("\n");

  writeStepSummary(
    [
      `## E2E Agent Test`,
      ``,
      `### Tool Mechanics`,
      `| Check | Status |`,
      `|-------|--------|`,
      `| cs_status | ${icon(result.status_ok)} |`,
      `| cs_search | ${icon(result.search_ok)} |`,
      `| cs_grep | ${icon(result.grep_ok)} |`,
      `| cs_read | ${icon(result.read_ok)} |`,
      ``,
      `### Semantic Relevance (${semHits}/${probes.length} hits)`,
      `| Query | Expected | Rank | Semantic? | Status |`,
      `|-------|----------|------|-----------|--------|`,
      probeRows,
      ``,
      `### Metrics`,
      `| Metric | Value |`,
      `|--------|-------|`,
      `| Duration | ${elapsed}s |`,
      `| Turns | ${usage.turns} |`,
      `| Tokens | ${usage.inputTokens.toLocaleString()} in / ${usage.outputTokens.toLocaleString()} out |`,
      `| Cost | $${usage.totalCostUsd.toFixed(2)} |`,
      `| Repos | ${result.repos_indexed} |`,
      `| Files | ${result.total_files} |`,
    ].join("\n")
  );

  // ── Verdict ──
  const allFailures = [...mechFailures, ...semFailures];

  if (result.repos_indexed < 1) allFailures.push("No repos indexed");
  if (result.total_files < 5) allFailures.push(`Suspiciously few files: ${result.total_files}`);

  if (allFailures.length > 0) {
    console.error(`[e2e] FAIL: ${allFailures.length} check(s) failed:`);
    for (const f of allFailures) console.error(`  - ${f}`);
    if (result.errors?.length) {
      console.error(`[e2e] Errors: ${result.errors.join("; ")}`);
    }
    process.exit(1);
  }

  console.error(
    `[e2e] PASS: mechanics OK, semantic ${semHits}/${probes.length} hits | ` +
      `${result.repos_indexed} repo(s), ${result.total_files} files`
  );
}

main();
