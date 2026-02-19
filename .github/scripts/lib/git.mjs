import { execSync } from "node:child_process";

const DEFAULT_TIMEOUT = 30_000;

/**
 * Run a git command and return trimmed stdout. Returns "" on error.
 * @param {string} cmd - git subcommand and args
 * @param {{ timeout?: number }} [options]
 * @returns {string}
 */
export function git(cmd, options = {}) {
  try {
    return execSync(`git ${cmd}`, {
      encoding: "utf-8",
      timeout: options.timeout ?? DEFAULT_TIMEOUT,
    }).trim();
  } catch {
    return "";
  }
}

/**
 * Get the last semver tag matching v*. Returns "v0.0.0" if none found.
 * @returns {string}
 */
export function getLastTag() {
  return git("describe --tags --abbrev=0 --match 'v*'") || "v0.0.0";
}

/**
 * Check if HEAD already has a v* tag.
 * @returns {string|null}
 */
export function getHeadTag() {
  const tags = git("tag --points-at HEAD");
  const match = tags.split("\n").find((t) => t.startsWith("v"));
  return match || null;
}

/**
 * Get commit log between a tag and HEAD.
 * Falls back to last 20 commits if tag is v0.0.0.
 * @param {string} lastTag
 * @returns {string}
 */
export function getCommitsSince(lastTag) {
  if (lastTag === "v0.0.0") {
    return git('log --pretty=format:"%h %s" -20');
  }
  return git(`log ${lastTag}..HEAD --pretty=format:"%h %s"`);
}

/**
 * Get diffstat between a tag and HEAD, truncated to maxLen chars.
 * @param {string} lastTag
 * @param {number} [maxLen=2000]
 * @returns {string}
 */
export function getDiffStat(lastTag, maxLen = 2000) {
  const range = lastTag !== "v0.0.0" ? `${lastTag}..HEAD` : "HEAD~20..HEAD";
  const stat = git(`diff --stat ${range}`);
  return stat.slice(-maxLen) || "(unavailable)";
}

/**
 * Gather all git context needed for AI analysis.
 * @returns {{ lastTag: string, headTag: string|null, commits: string, diffStat: string }}
 */
export function gatherContext() {
  const lastTag = getLastTag();
  const headTag = getHeadTag();
  const commits = getCommitsSince(lastTag);
  const diffStat = getDiffStat(lastTag);
  return { lastTag, headTag, commits, diffStat };
}
