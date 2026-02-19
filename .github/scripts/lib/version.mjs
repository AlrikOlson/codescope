/**
 * Parse a version tag into numeric components.
 * @param {string} tag - e.g. "v1.2.3"
 * @returns {{ major: number, minor: number, patch: number }}
 */
export function parseTag(tag) {
  const version = tag.replace(/^v/, "");
  const [major = 0, minor = 0, patch = 0] = version.split(".").map(Number);
  return { major, minor, patch };
}

/**
 * Apply a bump type to version components and return a new tag string.
 * @param {{ major: number, minor: number, patch: number }} version
 * @param {"major"|"minor"|"patch"} bumpType
 * @returns {string} - e.g. "v1.3.0"
 */
export function applyBump(version, bumpType) {
  let { major, minor, patch } = version;

  switch (bumpType) {
    case "major":
      major += 1;
      minor = 0;
      patch = 0;
      break;
    case "minor":
      minor += 1;
      patch = 0;
      break;
    case "patch":
      patch += 1;
      break;
  }

  return `v${major}.${minor}.${patch}`;
}

/**
 * Validate a bump type string. Returns "patch" if invalid.
 * @param {string} bump
 * @returns {"major"|"minor"|"patch"}
 */
export function validateBump(bump) {
  if (bump === "major" || bump === "minor" || bump === "patch") {
    return bump;
  }
  return "patch";
}
