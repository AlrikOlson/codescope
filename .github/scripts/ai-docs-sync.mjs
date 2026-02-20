#!/usr/bin/env node
// AI-powered documentation sync using Claude Agent SDK + CodeScope MCP.
// Reads current docs and source code, detects stale facts, produces updates.
// All outputs written to /tmp/ai-docs-sync-output.json.

import { gatherContext } from "./lib/git.mjs";
import { runAgent, parseAgentJson, writeStepSummary } from "./lib/agent.mjs";
import { hasDocRelevantChanges, writeDocSyncOutput } from "./lib/docs.mjs";

const OUTPUT_FILE = "/tmp/ai-docs-sync-output.json";

// Only allow updates to these files — scope guard against agent writing to unexpected paths
const ALLOWED_DOC_FILES = new Set(["README.md", "CONTRIBUTING.md"]);

const SYSTEM_PROMPT = `You are a documentation accuracy reviewer for CodeScope.

You have 4 CodeScope MCP tools. Use them efficiently:
1. cs_search — YOUR PRIMARY TOOL. Use this FIRST for any discovery. Combines semantic + keyword search automatically.
2. cs_read — Read specific files. Use mode=stubs for structural overviews without full file reads.
3. cs_grep — Exact pattern matching. Use for counting items or finding specific strings.
4. cs_status — Check what's indexed.

WORKFLOW: cs_search to discover → cs_read to verify → cs_grep to count.

RULES:
- Do NOT rewrite docs stylistically — only fix factual inaccuracies
- Do NOT add new sections or features that aren't already documented
- Preserve existing markdown structure, tone, and formatting
- Be fast — verify facts with minimal tool calls`;

/**
 * Build the prompt for the doc sync agent.
 * Verifies the FULL documentation against current source code state.
 */
function buildPrompt(version) {
  return `Verify ALL documentation files for factual accuracy against the current source code.

CURRENT VERSION: ${version}

## Verification Checklist

Read README.md and CONTRIBUTING.md first, then verify each fact:

### README.md
1. **MCP tool count**: cs_grep for tool handler registrations in server/src/mcp.rs
2. **Language support count**: cs_read server/src/stubs.rs (mode=stubs) — count languages
3. **CLI flags**: cs_read server/src/main.rs (mode=stubs) — compare against CLI Reference section
4. **Architecture table**: cs_search "rust source files architecture" — verify file list and descriptions
5. **Web UI panels**: cs_search "React UI components panels" — verify panel list
6. **Install commands**: cs_read server/setup.sh and server/setup.ps1 — verify paths and flags
7. **MCP tools table**: cs_grep for tool names in server/src/mcp.rs — verify names and descriptions
8. **Config options**: cs_search "codescope.toml config parsing" — verify documented options
9. **CI pipeline**: cs_search "CI workflow jobs pipeline" — verify job names and flow
10. **Dependency scanning**: cs_read server/src/scan.rs (mode=stubs) — verify formats

### CONTRIBUTING.md
1. **Architecture table**: Must list ALL .rs files in server/src/ with accurate descriptions
2. **Quality gate commands**: Must match CI workflow in .github/workflows/ci.yml
3. **Prerequisites**: Rust version, Node version
4. **Clone URL**: Must match actual repo

## Output Format

After verification, your FINAL line must be EXACTLY one JSON object (no markdown fencing):
{"updates":[{"file":"README.md","content":"full file content","reason":"one-line summary"}],"noChanges":["CONTRIBUTING.md"],"summary":"one-line summary"}

If all docs are accurate:
{"updates":[],"noChanges":["README.md","CONTRIBUTING.md"],"summary":"All docs are accurate"}`;
}

async function main() {
  const { lastTag, commits, diffStat } = gatherContext();

  // Check if any doc-relevant files changed
  if (!hasDocRelevantChanges(diffStat)) {
    console.error("[docs] No doc-relevant files changed — skipping sync");
    writeDocSyncOutput(OUTPUT_FILE, {
      updates: [],
      noChanges: ["README.md", "CONTRIBUTING.md"],
      summary: "Skipped — no doc-relevant files changed",
    });
    return;
  }

  // Get current version from env (set by CI) or from last tag
  const version =
    process.env.NEW_VERSION || lastTag.replace(/^v/, "") || "unknown";

  console.error(`[docs] Running doc sync for v${version}`);
  console.error(`[docs] Changes since ${lastTag}:\n${commits}\n---`);

  let agentResult;
  try {
    agentResult = await runAgent({
      prompt: buildPrompt(version),
      systemPrompt: SYSTEM_PROMPT,
      maxTurns: 8,
      maxBudgetUsd: 2.0,
      codeScopeOnly: true,
      logLabel: "docs-sync",
    });
  } catch (err) {
    console.error(`[docs] Agent SDK error: ${err.message}`);
    writeDocSyncOutput(OUTPUT_FILE, {
      updates: [],
      noChanges: [],
      summary: `Agent error: ${err.message}`,
    });
    writeStepSummary(`## AI Doc Sync\n\n**Status:** Failed — ${err.message}\n`);
    return;
  }

  const { text, usage } = agentResult;

  // Write step summary
  writeStepSummary([
    `## AI Doc Sync`,
    ``,
    `| Metric | Value |`,
    `|--------|-------|`,
    `| Turns | ${usage.turns} |`,
    `| Input tokens | ${usage.inputTokens.toLocaleString()} |`,
    `| Output tokens | ${usage.outputTokens.toLocaleString()} |`,
    `| Estimated cost | $${usage.totalCostUsd.toFixed(2)} |`,
  ].join("\n"));

  // Parse structured output
  const result = parseAgentJson(text, ["updates"]);

  if (!result) {
    console.error("[docs] Failed to parse agent JSON output");
    writeDocSyncOutput(OUTPUT_FILE, {
      updates: [],
      noChanges: [],
      summary: "Failed to parse agent output",
    });
    writeStepSummary(`\n**Result:** Failed to parse agent output\n`);
    return;
  }

  // Validate updates — ensure each has file and content, and only touches allowed files
  const validUpdates = (result.updates || []).filter((u) => {
    if (!u.file || !u.content || typeof u.content !== "string") return false;
    if (!ALLOWED_DOC_FILES.has(u.file)) {
      console.error(`[docs] BLOCKED: Agent tried to update disallowed file: ${u.file}`);
      return false;
    }
    return true;
  });

  const output = {
    updates: validUpdates,
    noChanges: result.noChanges || [],
    summary:
      result.summary ||
      (validUpdates.length
        ? `Updated ${validUpdates.length} doc(s)`
        : "All docs are accurate"),
  };

  writeDocSyncOutput(OUTPUT_FILE, output);

  if (validUpdates.length) {
    console.error(`[docs] ${output.summary}`);
    for (const u of validUpdates) {
      console.error(`[docs]   ${u.file}: ${u.reason}`);
    }
    writeStepSummary(`\n**Result:** Updated ${validUpdates.map((u) => u.file).join(", ")}\n`);
  } else {
    console.error("[docs] All docs are accurate — no updates needed");
    writeStepSummary(`\n**Result:** All docs accurate — no changes\n`);
  }
}

main().catch((err) => {
  console.error(`[docs] Fatal: ${err.message}`);
  // Don't fail the workflow — write empty output
  writeDocSyncOutput(OUTPUT_FILE, {
    updates: [],
    noChanges: [],
    summary: `Fatal error: ${err.message}`,
  });
  writeStepSummary(`## AI Doc Sync\n\n**Status:** Fatal error — ${err.message}\n`);
});
