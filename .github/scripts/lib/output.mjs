import { appendFileSync, writeFileSync } from "node:fs";

/**
 * Write a key=value pair to GITHUB_OUTPUT.
 * No-op if GITHUB_OUTPUT env var is not set (local development).
 * @param {string} key
 * @param {string} value
 */
export function setOutput(key, value) {
  const outputFile = process.env.GITHUB_OUTPUT;
  if (outputFile) {
    appendFileSync(outputFile, `${key}=${value}\n`);
  }
  console.error(`[output] ${key}=${value}`);
}

/**
 * Write the full AI release output to a JSON file.
 * @param {string} filePath
 * @param {{ bump: string, newTag: string, commitMessage: string, releaseBody: string, reason: string }} data
 */
export function writeReleaseOutput(filePath, data) {
  writeFileSync(filePath, JSON.stringify(data, null, 2));
  console.error(`[output] Wrote release data to ${filePath}`);
}
