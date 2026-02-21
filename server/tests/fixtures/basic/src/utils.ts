/**
 * Format a name by trimming and lowercasing.
 */
export function formatName(name: string): string {
  return name.trim().toLowerCase();
}

/**
 * Capitalize the first letter of a string.
 */
export function capitalize(str: string): string {
  if (str.length === 0) return str;
  return str.charAt(0).toUpperCase() + str.slice(1);
}

/**
 * Check if a string is a valid identifier.
 */
export function isValidId(id: string): boolean {
  return /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(id);
}
