/**
 * E2E Agent Test — validates CodeScope MCP tools via Claude Agent SDK.
 *
 * A Claude agent connects to CodeScope MCP, explores the codebase using all
 * available tools, and reports structured findings. The test validates:
 *   1. All 4 core tools respond correctly (cs_status, cs_search, cs_grep, cs_read)
 *   2. Results are coherent (correct languages, file counts, content)
 *   3. Agent completes within token/turn budget
 *
 * Exit codes: 0 = pass, 1 = fail
 */

import { runAgent, parseAgentJson } from "./lib/agent.mjs";

const MAX_TURNS = 6;
const MODEL = "claude-sonnet-4-6";

const systemPrompt = `You are a QA agent validating that CodeScope MCP tools work correctly.
You have 4 tools: cs_status, cs_search, cs_grep, cs_read.

Your job:
1. Call cs_status — verify repos are indexed, note file count and languages.
2. Call cs_search with a meaningful query (e.g. "MCP server tool dispatch") — verify results are returned and ranked.
3. Call cs_grep for an exact pattern (e.g. "pub fn") — verify matching lines are shown.
4. Call cs_read on one file in stubs mode — verify structural outline is returned.

After using all 4 tools, output a JSON object with your findings:
{
  "status_ok": true/false,
  "search_ok": true/false,
  "grep_ok": true/false,
  "read_ok": true/false,
  "repos_indexed": <number>,
  "total_files": <number>,
  "languages": ["rs", "ts", ...],
  "search_result_count": <number>,
  "grep_match_count": <number>,
  "errors": ["any errors encountered"]
}

Be concise. Use each tool exactly once. Output ONLY the JSON at the end.`;

const prompt = `Validate the CodeScope MCP server by testing all 4 tools against this codebase. Report your findings as the JSON object described in your instructions.`;

async function main() {
  console.error(`[e2e] Starting agent test (model=${MODEL}, maxTurns=${MAX_TURNS})`);
  const start = Date.now();

  let output;
  try {
    output = await runAgent({
      prompt,
      systemPrompt,
      model: MODEL,
      maxTurns: MAX_TURNS,
      codeScopeOnly: true,
    });
  } catch (err) {
    console.error(`[e2e] Agent failed: ${err.message}`);
    process.exit(1);
  }

  const elapsed = ((Date.now() - start) / 1000).toFixed(1);
  console.error(`[e2e] Agent completed in ${elapsed}s`);

  // Parse structured JSON from agent output
  const result = parseAgentJson(output, [
    "status_ok",
    "search_ok",
    "grep_ok",
    "read_ok",
  ]);

  if (!result) {
    console.error("[e2e] FAIL: Could not parse structured JSON from agent output");
    console.error("[e2e] Raw output:", output.slice(-500));
    process.exit(1);
  }

  // Write result for CI artifact collection
  console.log(JSON.stringify(result, null, 2));

  // Validate all tools passed
  const tools = ["status_ok", "search_ok", "grep_ok", "read_ok"];
  const failures = tools.filter((t) => !result[t]);

  if (failures.length > 0) {
    console.error(`[e2e] FAIL: ${failures.length} tool(s) failed: ${failures.join(", ")}`);
    if (result.errors?.length) {
      console.error(`[e2e] Errors: ${result.errors.join("; ")}`);
    }
    process.exit(1);
  }

  // Sanity checks on the data
  if (result.repos_indexed < 1) {
    console.error("[e2e] FAIL: No repos indexed");
    process.exit(1);
  }
  if (result.total_files < 5) {
    console.error(`[e2e] FAIL: Suspiciously few files indexed (${result.total_files})`);
    process.exit(1);
  }

  console.error(
    `[e2e] PASS: All 4 tools OK | ${result.repos_indexed} repo(s), ${result.total_files} files, ${result.languages?.length || "?"} languages`
  );
}

main();
