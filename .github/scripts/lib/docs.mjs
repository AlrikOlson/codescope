import { readFileSync, writeFileSync, existsSync } from "node:fs";

/**
 * File patterns that, when changed, may affect documentation accuracy.
 * If only files outside these patterns change, doc sync is skipped.
 */
const DOC_RELEVANT_PATTERNS = [
  /^server\/src\//,
  /^server\/setup\.sh$/,
  /^server\/Cargo\.toml$/,
  /^server\/codescope-/,
  /^package\.json$/,
  /^src\//,
  /^\.github\/workflows\/ci\.yml$/,
  /^vite\.config/,
  /^tsconfig/,
];

/**
 * Check whether any changed files could affect documentation.
 * Parses a git diffstat string for file paths and matches against known patterns.
 *
 * @param {string} diffStat - output of `git diff --stat`
 * @returns {boolean}
 */
export function hasDocRelevantChanges(diffStat) {
  if (!diffStat || diffStat === "(unavailable)") return false;

  const lines = diffStat.split("\n");
  for (const line of lines) {
    // diffstat lines look like: " server/src/mcp.rs | 42 ++--"
    const match = line.match(/^\s*(.+?)\s*\|/);
    if (!match) continue;
    const filePath = match[1].trim();
    if (DOC_RELEVANT_PATTERNS.some((p) => p.test(filePath))) {
      return true;
    }
  }

  return false;
}

/**
 * Write doc sync results to a JSON file.
 * @param {string} filePath
 * @param {{ updates: Array<{ file: string, content: string, reason: string }>, noChanges: string[], summary: string }} data
 */
export function writeDocSyncOutput(filePath, data) {
  writeFileSync(filePath, JSON.stringify(data, null, 2));
  console.error(`[docs] Wrote doc sync data to ${filePath}`);
}

/**
 * Apply doc updates from the sync output file.
 * Reads the JSON, writes each updated file to disk.
 *
 * @param {string} outputFile - path to ai-docs-sync-output.json
 * @returns {{ applied: string[], summary: string }}
 */
export function applyDocUpdates(outputFile) {
  if (!existsSync(outputFile)) {
    return { applied: [], summary: "No doc sync output found" };
  }

  const data = JSON.parse(readFileSync(outputFile, "utf8"));
  const applied = [];

  for (const update of data.updates || []) {
    if (update.file && update.content) {
      writeFileSync(update.file, update.content);
      console.error(`[docs] Updated: ${update.file} — ${update.reason}`);
      applied.push(update.file);
    }
  }

  return {
    applied,
    summary: data.summary || `Updated ${applied.length} doc(s)`,
  };
}
