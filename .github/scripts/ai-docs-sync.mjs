#!/usr/bin/env node
// AI-powered documentation sync using Claude Agent SDK + CodeScope MCP.
// Reads current docs and source code, detects stale facts, produces updates.
// All outputs written to /tmp/ai-docs-sync-output.json.

import { gatherContext } from "./lib/git.mjs";
import { runAgent, parseAgentJson } from "./lib/agent.mjs";
import { hasDocRelevantChanges, writeDocSyncOutput } from "./lib/docs.mjs";

const OUTPUT_FILE = "/tmp/ai-docs-sync-output.json";

const SYSTEM_PROMPT = `You are a documentation accuracy reviewer for CodeScope, a Rust MCP server + TypeScript web UI for codebase search and navigation.

You have access to CodeScope MCP tools. Use them to read documentation files and source code, then compare them for accuracy. You also have cs_semantic_search for intent-based code discovery.

Your job is to find and fix factual inaccuracies in docs — wrong counts, missing features, outdated architecture, stale CLI flags, etc. Do NOT rewrite docs stylistically. Only change facts that are wrong.

Preserve the existing markdown structure, tone, and formatting of each doc. Make minimal, surgical changes.`;

/**
 * Build the prompt for the doc sync agent.
 * @param {string} lastTag
 * @param {string} commits
 * @param {string} diffStat
 * @param {string} version
 * @returns {string}
 */
function buildPrompt(lastTag, commits, diffStat, version) {
  return `Review all documentation files for factual accuracy against the current source code.

CURRENT VERSION: ${version}
LAST TAG: ${lastTag}

COMMITS SINCE LAST TAG:
${commits}

FILES CHANGED:
${diffStat}

## Documentation Files to Verify

### 1. README.md
Verify these facts against source code:
- **MCP tool count**: Count the actual tools registered in \`server/src/mcp.rs\` (look for tool definitions/handlers)
- **Language support count**: Count languages in \`server/src/stubs.rs\` (stub extraction) and \`server/src/scan.rs\` (import tracing)
- **CLI flags/options**: Compare the CLI Reference section against actual clap args in \`server/src/main.rs\`
- **Architecture section**: Verify all listed source files exist and descriptions are accurate
- **Web UI panels/views**: Verify against actual React components in \`src/\`
- **Prerequisites**: Check Rust version in \`server/Cargo.toml\` (rust-version or edition), Node version
- **Install commands**: Verify setup.sh paths and flags
- **MCP tools table**: Verify tool names and descriptions match \`server/src/mcp.rs\`
- **Configuration options**: Verify .codescope.toml fields match what the server actually parses
- **CI pipeline diagram**: Verify job names and flow match \`.github/workflows/ci.yml\`
- **Dependency scanning formats**: Verify against \`server/src/scan.rs\`

### 2. CONTRIBUTING.md
Verify these facts against source code:
- **Architecture table**: Must list all .rs files in \`server/src/\` with accurate descriptions (should match README)
- **Quality gate commands**: Must match CI workflow commands in \`.github/workflows/ci.yml\`
- **Prerequisites**: Rust version and Node version must match actual requirements
- **Clone URL**: Must match actual GitHub repo

## Instructions

1. Use \`cs_read_file\` to read README.md and CONTRIBUTING.md
2. Use \`cs_find\`, \`cs_grep\`, and \`cs_read_file\` to verify each fact listed above against source code
3. Be thorough — check every number, every file path, every feature claim
4. For each doc that needs changes, output the COMPLETE updated file content
5. Do NOT change writing style, tone, or structure — only fix factual inaccuracies
6. Do NOT add new sections or features that aren't already documented
7. If a doc is accurate, include it in noChanges

After your analysis, your FINAL line of output must be EXACTLY one JSON object (no markdown fencing, no text after):
{"updates":[{"file":"README.md","content":"full file content here","reason":"one-line summary of what changed"}],"noChanges":["CONTRIBUTING.md"],"summary":"one-line summary of all changes"}

If NO docs need updating, output:
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
      prompt: buildPrompt(lastTag, commits, diffStat, version),
      systemPrompt: SYSTEM_PROMPT,
      maxTurns: 15,
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
