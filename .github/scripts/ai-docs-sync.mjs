#!/usr/bin/env node
// AI-powered documentation sync using Claude Agent SDK + CodeScope MCP.
// Reads current docs and source code, detects stale facts, produces updates.
// All outputs written to /tmp/ai-docs-sync-output.json.

import { gatherContext } from "./lib/git.mjs";
import { runAgent, parseAgentJson } from "./lib/agent.mjs";
import { hasDocRelevantChanges, writeDocSyncOutput } from "./lib/docs.mjs";

const OUTPUT_FILE = "/tmp/ai-docs-sync-output.json";

const SYSTEM_PROMPT = `You are a documentation accuracy reviewer for CodeScope.

You have ONLY CodeScope MCP tools available. Use them efficiently:
- cs_semantic_search — find code by intent (PREFERRED for discovery)
- cs_read_file — read specific files (use mode=stubs for structural overview)
- cs_grep — search for exact patterns
- cs_find — find files by name/content
- cs_list_modules — enumerate module structure
- cs_status — check indexed repo info

RULES:
1. Start with cs_semantic_search and cs_read_file for efficient discovery
2. Use cs_read_file with mode=stubs to get structural overviews without reading full files
3. Do NOT rewrite docs stylistically — only fix factual inaccuracies
4. Do NOT add new sections or features that aren't already documented
5. Preserve existing markdown structure, tone, and formatting
6. Be fast — verify facts with minimal tool calls, use parallel reads when possible`;

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
2. **Language support count**: cs_read_file server/src/stubs.rs (mode=stubs) — count languages
3. **CLI flags**: cs_read_file server/src/main.rs (mode=stubs) — compare against CLI Reference section
4. **Architecture table**: cs_find all .rs files in server/src/ — verify file list and descriptions
5. **Web UI panels**: cs_find React components in src/ — verify panel list
6. **Install commands**: cs_read_file server/setup.sh and server/setup.ps1 — verify paths and flags
7. **MCP tools table**: cs_grep for tool names in server/src/mcp.rs — verify names and descriptions
8. **Config options**: cs_semantic_search "codescope.toml config parsing" — verify documented options
9. **CI pipeline**: cs_find workflow files in .github/workflows/ — verify job names and flow
10. **Dependency scanning**: cs_read_file server/src/scan.rs (mode=stubs) — verify formats

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

  let text;
  try {
    text = await runAgent({
      prompt: buildPrompt(version),
      systemPrompt: SYSTEM_PROMPT,
      maxTurns: 8,
      codeScopeOnly: true,
    });
  } catch (err) {
    console.error(`[docs] Agent SDK error: ${err.message}`);
    writeDocSyncOutput(OUTPUT_FILE, {
      updates: [],
      noChanges: [],
      summary: `Agent error: ${err.message}`,
    });
    return;
  }

  // Parse structured output
  const result = parseAgentJson(text, ["updates"]);

  if (!result) {
    console.error("[docs] Failed to parse agent JSON output");
    writeDocSyncOutput(OUTPUT_FILE, {
      updates: [],
      noChanges: [],
      summary: "Failed to parse agent output",
    });
    return;
  }

  // Validate updates — ensure each has file and content
  const validUpdates = (result.updates || []).filter(
    (u) => u.file && u.content && typeof u.content === "string"
  );

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
  } else {
    console.error("[docs] All docs are accurate — no updates needed");
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
});
