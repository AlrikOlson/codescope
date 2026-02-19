// Canonical extension colors (hex strings). All other formats derived from these.
export const EXT_COLORS: Record<string, string> = {
  '.h':    '#89b4fa',
  '.hpp':  '#89b4fa',
  '.hxx':  '#89b4fa',
  '.cpp':  '#a6e3a1',
  '.c':    '#a6e3a1',
  '.cc':   '#a6e3a1',
  '.cxx':  '#a6e3a1',
  '.rs':   '#fab387',
  '.go':   '#89dceb',
  '.py':   '#f9e2af',
  '.js':   '#f9e2af',
  '.mjs':  '#f9e2af',
  '.cjs':  '#f9e2af',
  '.ts':   '#89b4fa',
  '.tsx':  '#89b4fa',
  '.jsx':  '#f9e2af',
  '.java': '#cba6f7',
  '.kt':   '#cba6f7',
  '.scala':'#cba6f7',
  '.cs':   '#cba6f7',
  '.swift':'#fab387',
  '.css':  '#f38ba8',
  '.scss': '#f38ba8',
  '.less': '#f38ba8',
  '.html': '#fab387',
  '.htm':  '#fab387',
  '.vue':  '#a6e3a1',
  '.svelte':'#fab387',
  '.json': '#7f849c',
  '.yaml': '#7f849c',
  '.yml':  '#7f849c',
  '.toml': '#7f849c',
  '.xml':  '#7f849c',
  '.ini':  '#7f849c',
  '.cfg':  '#7f849c',
  '.md':   '#cdd6f4',
  '.txt':  '#cdd6f4',
  '.rst':  '#cdd6f4',
  '.sh':   '#a6e3a1',
  '.bash': '#a6e3a1',
  '.zsh':  '#a6e3a1',
  '.usf':  '#fab387',
  '.ush':  '#fab387',
  '.hlsl': '#fab387',
  '.glsl': '#fab387',
  '.sql':  '#89dceb',
  '.rb':   '#f38ba8',
  '.lua':  '#89dceb',
  '.dockerfile': '#89b4fa',
  '.proto':'#cba6f7',
};

const HASH_PALETTE = ['#89b4fa', '#a6e3a1', '#f9e2af', '#fab387', '#cba6f7', '#f38ba8', '#89dceb', '#94e2d5'];

function hashColor(ext: string): string {
  let hash = 0;
  for (let i = 0; i < ext.length; i++) hash = ((hash << 5) - hash + ext.charCodeAt(i)) | 0;
  return HASH_PALETTE[Math.abs(hash) % HASH_PALETTE.length];
}

export function getExtColor(ext: string): string {
  return EXT_COLORS[ext] || hashColor(ext);
}

function hexToRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

export function getExtRgb(ext: string): [number, number, number] | null {
  const hex = EXT_COLORS[ext];
  return hex ? hexToRgb(hex) : null;
}

// Category/group colors for dependency graph
export const CATEGORY_COLORS: Record<string, string> = {
  'Source':     '#a6e3a1',
  'Runtime':   '#a6e3a1',
  'Editor':    '#f9e2af',
  'Developer': '#fab387',
  'Programs':  '#cba6f7',
  'Other':     '#7f849c',
};

const DEFAULT_CATEGORY_COLOR = '#7f849c';

export function getCategoryColor(group: string): string {
  return CATEGORY_COLORS[group] ?? DEFAULT_CATEGORY_COLOR;
}

export function getCategoryColorHex(group: string): number {
  return parseInt((CATEGORY_COLORS[group] ?? DEFAULT_CATEGORY_COLOR).slice(1), 16);
}
