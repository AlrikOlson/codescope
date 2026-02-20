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

const SYSTEM_PROMPT = `You are a release engineer for CodeScope (https://github.com/AlrikOlson/codescope).

TOOLS:
- cs_search — semantic + keyword search (best for discovery)
- cs_read — read files (mode=stubs for overviews, mode=full for details)
- cs_grep — exact pattern matching
- cs_status — index status

RULES:
- Commit messages + diffstat are provided in the prompt. Use CodeScope tools to verify and refine your analysis.
- Do NOT invent URLs, paths, or features not in the commit log.
- Do NOT include comparison/changelog URLs — the workflow generates those.
- Your LAST turn MUST be your structured output — never end on a tool call.`;

/**
 * Build the prompt for the AI agent.
 * @param {string} lastTag
 * @param {string} commits
 * @param {string} diffStat
 * @returns {string}
 */
function buildPrompt(lastTag, commits, diffStat) {
  const today = new Date().toISOString().slice(0, 10);
  return `Analyze CodeScope (AlrikOlson/codescope) changes since the last release and produce release metadata.

LAST TAG: ${lastTag}

COMMITS SINCE LAST TAG:
${commits}

FILES CHANGED (diffstat):
${diffStat}

STEP 1 — FORM INITIAL BUMP HYPOTHESIS from commits + diffstat:
- ALL commits are fix/docs/ci/chore with no server/src/ in diffstat → likely PATCH
- Any commit adds user-facing capability (new tool, new flag, new endpoint) → likely MINOR
- Any commit breaks existing API/CLI/protocol → likely MAJOR
When in doubt: lean PATCH.

STEP 2 — VERIFY WITH CODESCOPE. Use tools to refine your hypothesis:
- cs_search/cs_grep to check if changed files affect public APIs or user-facing behavior
- cs_read (mode=stubs) to inspect signatures of modified modules
- Confirm whether changes are internal-only or user-visible
- Use as many tool calls as needed, but stay within your turn budget

STEP 3 — PRODUCE STRUCTURED OUTPUT with:
- bump: finalized semver bump based on steps 1-2
- reason: one-line justification
- commitMessage: conventional-commit format, e.g. "release: v1.2.4 — fix module resolution"
- releaseBody: markdown with "## What's Changed" grouping by category (Features, Fixes, Improvements, Internal)
- changelogEntry: Keep a Changelog format, "## [X.Y.Z] - ${today}", sections: Added/Changed/Fixed/Removed

IMPORTANT:
- CodeScope shows CURRENT code state, NOT what was added. Only classify as "new" if commit says "add"/"new"/"introduce" or diffstat shows a new file.
- Do NOT include comparison URLs or full changelog URLs — the workflow generates those.
- Do NOT invent features, URLs, or paths not in the commit log above.

EXAMPLE OUTPUT:
{
  "bump": "patch",
  "reason": "CI/CD improvements: binary caching, workflow fixes, CUDA build fixes",
  "commitMessage": "release: v0.8.2 — CI reliability and build caching",
  "releaseBody": "## What's Changed\\n\\n### Internal\\n- Cache codescope-server binary across CI jobs\\n- Fix CUDA build linking\\n- Fix workflow concurrency",
  "changelogEntry": "## [0.8.2] - ${today}\\n\\n### Fixed\\n- CUDA build linking\\n\\n### Changed\\n- Cache server binary across workflows"
}

Now analyze the commits and diffstat above, verify with CodeScope, and produce your structured output.`;
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
