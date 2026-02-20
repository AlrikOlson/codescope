#!/usr/bin/env node
// AI-powered release analysis using Claude Agent SDK + CodeScope MCP.
// Produces: semver bump, release commit message, and GitHub release body.
// All outputs written to GITHUB_OUTPUT and /tmp/ai-release-output.json.

import { gatherContext } from "./lib/git.mjs";
import { runAgent, parseAgentJson } from "./lib/agent.mjs";
import { parseTag, applyBump, validateBump } from "./lib/version.mjs";
import { setOutput, writeReleaseOutput } from "./lib/output.mjs";

const OUTPUT_FILE = "/tmp/ai-release-output.json";

const SYSTEM_PROMPT = `You are a precise release engineer for CodeScope, a Rust MCP server + TypeScript web UI for codebase search and navigation.

You have 4 CodeScope MCP tools. Use them efficiently:
1. cs_search — YOUR PRIMARY TOOL. Use this FIRST for any discovery. Combines semantic + keyword search automatically.
2. cs_read — Read specific files. Use mode=stubs for structural overviews without reading entire files.
3. cs_grep — Exact pattern matching. Use for counting specific items or finding exact strings.
4. cs_status — Check what's indexed.

WORKFLOW: cs_search to discover → cs_read to verify → cs_grep to count. Be concise.`;

/**
 * Build the prompt for the AI agent.
 * @param {string} lastTag
 * @param {string} commits
 * @param {string} diffStat
 * @returns {string}
 */
function buildPrompt(lastTag, commits, diffStat) {
  return `Analyze the changes to CodeScope since the last release and produce release metadata.

LAST TAG: ${lastTag}

COMMITS SINCE LAST TAG:
${commits}

FILES CHANGED:
${diffStat}

## Instructions

1. Use CodeScope tools (cs_search, cs_read, cs_grep) to read the changed files and understand what was modified.
2. CRITICAL: The commit messages are your PRIMARY source of truth for what changed. CodeScope shows the CURRENT state of the code, NOT what was added in this release. If a file like mcp.rs was MODIFIED (not created), the features in it ALREADY EXISTED — they were changed, not added. Only classify something as "new" if:
   - The commit message explicitly says "add", "new", "introduce", or "implement"
   - The file itself is newly created (check the diffstat for new files vs modified files)
   - The diffstat shows the file went from 0 lines to N lines
3. Check if any public APIs, MCP tool interfaces, CLI flags, or config formats changed in breaking ways.
4. Determine the correct semver bump:
   - **MAJOR**: breaking changes to MCP protocol, CLI interface, API endpoints, or config format that would break existing users/integrations
   - **MINOR**: new features, new MCP tools, new CLI flags, new API endpoints, meaningful new capabilities
   - **PATCH**: bug fixes, performance improvements, refactoring, documentation, CI/CD changes, dependency updates
   When in doubt between minor and patch, prefer patch. Only use major for genuine breaking changes.
5. Write a conventional-commit release commit message (e.g. "release: v1.2.4 \u2014 fix module resolution edge case").
6. Write a GitHub release body in markdown with:
   - A "What's Changed" section grouping changes by category (Features, Fixes, Improvements, Internal)
   - Specific file/module references from your CodeScope analysis
   - Keep it concise but informative
7. Write a CHANGELOG.md entry following Keep a Changelog format. The entry should:
   - Start with "## [X.Y.Z] - YYYY-MM-DD" using today's date
   - Group changes under ### Added, ### Changed, ### Fixed, ### Removed as appropriate
   - Be concise bullet points (one line per change)
   - Only include sections that have changes

After your analysis, your FINAL line of output must be EXACTLY one JSON object (no markdown fencing, no text after):
{"bump":"patch","reason":"one sentence explanation","commitMessage":"release: vX.Y.Z \u2014 summary","releaseBody":"## What's Changed\\n\\n### Fixes\\n- ...","changelogEntry":"## [X.Y.Z] - YYYY-MM-DD\\n\\n### Fixed\\n- ..."}`;
}

/**
 * Defaults for when AI output is incomplete.
 * @param {string} newTag
 * @returns {{ commitMessage: string, releaseBody: string, reason: string }}
 */
function defaults(newTag) {
  const version = newTag.replace(/^v/, "");
  const date = new Date().toISOString().slice(0, 10);
  return {
    commitMessage: `release: ${newTag}`,
    releaseBody: "",
    changelogEntry: `## [${version}] - ${date}\n\n### Changed\n- Release ${newTag}`,
    reason: "AI output incomplete, used defaults",
  };
}

async function main() {
  const { lastTag, headTag, commits, diffStat } = gatherContext();

  // Early exit: HEAD already tagged
  if (headTag) {
    console.error(`HEAD already tagged (${headTag}) — skipping`);
    setOutput("skip", "true");
    setOutput("new_tag", headTag);
    return;
  }

  // Early exit: no new commits
  if (!commits) {
    console.error(`No new commits since ${lastTag} — skipping`);
    setOutput("skip", "true");
    setOutput("new_tag", lastTag);
    return;
  }

  console.error(`Last tag: ${lastTag}`);
  console.error(`Commits since ${lastTag}:\n${commits}\n---`);

  // Run AI analysis
  let text;
  try {
    text = await runAgent({
      prompt: buildPrompt(lastTag, commits, diffStat),
      systemPrompt: SYSTEM_PROMPT,
      codeScopeOnly: true,
    });
  } catch (err) {
    console.error(`Agent SDK error: ${err.message}`);
    const version = parseTag(lastTag);
    const newTag = applyBump(version, "patch");
    const fallback = defaults(newTag);
    finalize("patch", newTag, fallback.commitMessage, fallback.releaseBody, fallback.reason);
    return;
  }

  // Parse structured output
  const result = parseAgentJson(text, ["bump"]);
  const bump = validateBump(result?.bump);
  const version = parseTag(lastTag);
  const newTag = applyBump(version, bump);
  const fallback = defaults(newTag);

  const commitMessage = result?.commitMessage || fallback.commitMessage;
  const releaseBody = result?.releaseBody || fallback.releaseBody;
  const changelogEntry = result?.changelogEntry || fallback.changelogEntry;
  const reason = result?.reason || fallback.reason;

  finalize(bump, newTag, commitMessage, releaseBody, changelogEntry, reason);
}

/**
 * Write all outputs and log the result.
 */
function finalize(bump, newTag, commitMessage, releaseBody, changelogEntry, reason) {
  setOutput("new_tag", newTag);
  setOutput("skip", "false");
  setOutput("bump", bump);

  writeReleaseOutput(OUTPUT_FILE, {
    bump,
    newTag,
    commitMessage,
    releaseBody,
    changelogEntry,
    reason,
  });

  console.error(`AI Release: ${bump} — ${reason}`);
}

main().catch((err) => {
  console.error(`Fatal: ${err.message}`);
  // Don't fail the workflow — write patch defaults
  const version = parseTag(process.argv[2] || "v0.0.0");
  const newTag = applyBump(version, "patch");
  const fallback = defaults(newTag);
  setOutput("new_tag", newTag);
  setOutput("skip", "false");
  setOutput("bump", "patch");
  writeReleaseOutput(OUTPUT_FILE, {
    bump: "patch",
    newTag,
    commitMessage: fallback.commitMessage,
    releaseBody: "",
    changelogEntry: fallback.changelogEntry,
    reason: `Fatal error: ${err.message}`,
  });
});
