#!/usr/bin/env node
// AI-powered release analysis using Claude Agent SDK + CodeScope MCP.
// Produces: semver bump, release commit message, and GitHub release body.
// All outputs written to GITHUB_OUTPUT and /tmp/ai-release-output.json.

import { gatherContext } from "./lib/git.mjs";
import { runAgent, writeStepSummary } from "./lib/agent.mjs";
import { parseTag, applyBump, validateBump } from "./lib/version.mjs";
import { setOutput, writeReleaseOutput } from "./lib/output.mjs";

/**
 * Find the highest version from commit messages and the last tag.
 * Scans for "release: vX.Y.Z" patterns. Returns the highest version
 * found (as a parsed object), which may be higher than the last tag
 * if release commits exist that haven't been tagged yet.
 * @param {string} commits - commit log text
 * @param {string} lastTag - last git tag
 * @returns {{ major: number, minor: number, patch: number }}
 */
function highestVersionFromCommits(commits, lastTag) {
  const versionPattern = /release:?\s*v?(\d+\.\d+\.\d+)/gi;
  let highest = parseTag(lastTag);

  for (const match of commits.matchAll(versionPattern)) {
    const candidate = parseTag(`v${match[1]}`);
    if (
      candidate.major > highest.major ||
      (candidate.major === highest.major && candidate.minor > highest.minor) ||
      (candidate.major === highest.major && candidate.minor === highest.minor && candidate.patch > highest.patch)
    ) {
      highest = candidate;
    }
  }

  return highest;
}

const OUTPUT_FILE = "/tmp/ai-release-output.json";

const outputSchema = {
  type: "object",
  properties: {
    bump: { type: "string", enum: ["major", "minor", "patch"] },
    reason: { type: "string" },
    commitMessage: { type: "string" },
    releaseBody: { type: "string" },
    changelogEntry: { type: "string" },
  },
  required: ["bump", "reason", "commitMessage", "releaseBody", "changelogEntry"],
};

const SYSTEM_PROMPT = `You are a precise release engineer for CodeScope, a Rust MCP server + TypeScript web UI for codebase search and navigation.

You have 4 CodeScope MCP tools:
1. cs_search — combined semantic + keyword search
2. cs_read — read specific files (use mode=stubs for overviews)
3. cs_grep — exact pattern matching
4. cs_status — check index status

CRITICAL: You have a LIMITED number of tool calls. Budget them carefully:
- Use AT MOST 6-8 tool calls total, then produce your final structured output.
- The commit messages already tell you WHAT changed. Only use tools to verify specifics (e.g. whether an API broke).
- Do NOT exhaustively explore every changed file. Focus on the 2-3 most important changes.
- If the changes are clearly CI/tooling (no server/src changes), skip tool calls entirely and go straight to your structured output.
- Your LAST turn MUST be your structured output — never end on a tool call.`;

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

1. CRITICAL: The commit messages and diffstat above are your PRIMARY source of truth. You can ALREADY determine the bump type and write release notes from them alone. Only use CodeScope tools if you need to verify something specific (e.g. whether a change is breaking). For CI/tooling-only changes, skip tools entirely.
2. CodeScope shows the CURRENT state of the code, NOT what was added in this release. If a file like mcp.rs was MODIFIED (not created), the features in it ALREADY EXISTED — they were changed, not added. Only classify something as "new" if:
   - The commit message explicitly says "add", "new", "introduce", or "implement"
   - The file itself is newly created (check the diffstat for new files vs modified files)
   - The diffstat shows the file went from 0 lines to N lines
3. Check if any public APIs, MCP tool interfaces, CLI flags, or config formats changed in breaking ways.
4. Determine the correct semver bump:
   - **MAJOR**: breaking changes to MCP protocol, CLI interface, API endpoints, or config format that would break existing users/integrations
   - **MINOR**: new features, new MCP tools, new CLI flags, new API endpoints, meaningful new capabilities
   - **PATCH**: bug fixes, performance improvements, refactoring, documentation, CI/CD changes, dependency updates
   When in doubt between minor and patch, prefer patch. Only use major for genuine breaking changes.
5. Write a conventional-commit release commit message (e.g. "release: v1.2.4 — fix module resolution edge case").
6. Write a GitHub release body in markdown with:
   - A "What's Changed" section grouping changes by category (Features, Fixes, Improvements, Internal)
   - Specific file/module references from your CodeScope analysis
   - Keep it concise but informative
7. Write a CHANGELOG.md entry following Keep a Changelog format. The entry should:
   - Start with "## [X.Y.Z] - YYYY-MM-DD" using today's date
   - Group changes under ### Added, ### Changed, ### Fixed, ### Removed as appropriate
   - Be concise bullet points (one line per change)
   - Only include sections that have changes

After your analysis, report your findings in the structured output.`;
}

/**
 * Sanitize a string for use in git commit messages.
 * Strips non-printable chars, caps length.
 * @param {string} str
 * @param {number} [maxLen=500]
 * @returns {string}
 */
function sanitizeForGit(str, maxLen = 500) {
  if (!str) return "";
  return str
    .replace(/[^\x20-\x7E\n—–·•\-]/g, "") // ASCII printable + common Unicode punctuation
    .substring(0, maxLen)
    .trim();
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
  let agentResult;
  try {
    agentResult = await runAgent({
      prompt: buildPrompt(lastTag, commits, diffStat),
      systemPrompt: SYSTEM_PROMPT,
      maxTurns: 20,
      maxBudgetUsd: 3.0,
      codeScopeOnly: true,
      logLabel: "release",
      outputFormat: { type: "json_schema", schema: outputSchema },
    });
  } catch (err) {
    console.error(`Agent SDK error: ${err.message}`);
    const newTag = applyBump(highestVersionFromCommits(commits, lastTag), "patch");
    const fallback = defaults(newTag);
    writeStepSummary(`## AI Release Analysis\n\n**Status:** Failed — ${err.message}\n**Fallback:** patch bump to ${newTag}\n`);
    finalize("patch", newTag, fallback.commitMessage, fallback.releaseBody, fallback.changelogEntry, `Agent error: ${err.message}`);
    return;
  }

  const { usage, structured_output: result } = agentResult;

  // Write step summary with cost/usage info
  writeStepSummary([
    `## AI Release Analysis`,
    ``,
    `| Metric | Value |`,
    `|--------|-------|`,
    `| Turns | ${usage.turns} |`,
    `| Input tokens | ${usage.inputTokens.toLocaleString()} |`,
    `| Output tokens | ${usage.outputTokens.toLocaleString()} |`,
    `| Cached tokens | ${usage.cachedTokens.toLocaleString()} |`,
    `| Estimated cost | $${usage.totalCostUsd.toFixed(2)} |`,
  ].join("\n"));

  if (!result) {
    console.error("[release] WARNING: Agent returned no structured output (likely exhausted turns or budget).");
    console.error(`[release] Agent used ${usage.turns} turns, $${usage.totalCostUsd.toFixed(2)} budget.`);
  }

  const bump = validateBump(result?.bump);
  // Apply bump from the highest known version (including release commits in the log)
  // so we don't regress version numbers when last tag is behind
  const baseVersion = highestVersionFromCommits(commits, lastTag);
  const newTag = applyBump(baseVersion, bump);
  const fallback = defaults(newTag);

  const commitMessage = sanitizeForGit(result?.commitMessage) || fallback.commitMessage;
  const releaseBody = result?.releaseBody || fallback.releaseBody;
  const changelogEntry = result?.changelogEntry || fallback.changelogEntry;
  const reason = result?.reason || (result ? "AI returned empty reason" : `No structured output (${usage.turns} turns, $${usage.totalCostUsd.toFixed(2)})`);

  writeStepSummary(`\n**Bump:** ${bump} → ${newTag}\n**Reason:** ${reason}\n`);

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
  writeStepSummary(`## AI Release Analysis\n\n**Status:** Fatal error — ${err.message}\n**Fallback:** patch bump to ${newTag}\n`);
});
