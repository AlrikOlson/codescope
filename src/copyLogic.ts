import { getExt, extToMarkdownLang } from './utils';
import { estimateTokens, formatTokenCount, formatTierCounts, formatCharCount } from './tokenCount';
import type { Manifest, ContextResponse } from './types';

/**
 * Copy text (or a promise of text) to the clipboard.
 * Uses ClipboardItem API for async content so the browser doesn't block on fetch.
 */
export async function copyToClipboard(content: string | Promise<string>): Promise<void> {
  if (typeof content === 'string') {
    await navigator.clipboard.writeText(content);
    return;
  }
  const item = new ClipboardItem({
    'text/plain': content.then(text => new Blob([text], { type: 'text/plain' })),
  });
  await navigator.clipboard.write([item]);
}

/**
 * Build smart (budget-aware, tiered) context via /api/context.
 * Returns the formatted markdown string and a toast message.
 */
export async function buildSmartContext(
  selected: Set<string>,
  manifest: Manifest,
  searchQuery: string | null,
  budget: number,
): Promise<{ text: string; toast: string }> {
  const paths = [...selected];
  if (paths.length === 0) return { text: '', toast: 'No files to copy' };

  const resp = await fetch('/api/context', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ paths, unit: 'tokens', budget, query: searchQuery || undefined }),
  });
  if (!resp.ok) throw new Error('Context request failed');
  const data: ContextResponse = await resp.json();
  const { files, summary } = data;

  const tierLabel = (tier: number) => {
    if (tier === 2) return ' [pruned]';
    if (tier === 3) return ' [TOC]';
    if (tier === 4) return ' [manifest]';
    return '';
  };

  const sizeStr = summary.unit === 'chars'
    ? `~${formatCharCount(summary.totalChars)}, budget: ${formatCharCount(summary.budget)}`
    : `~${formatTokenCount(summary.totalTokens)}, budget: ${formatTokenCount(summary.budget)}`;
  const lines: string[] = [
    `## Source Context (${summary.totalFiles} files, ${sizeStr})`,
    '',
    `> **Context Tiers:** ${formatTierCounts(summary.tierCounts)}`,
    '> Full contents via MCP server `codescope`.',
    '',
  ];

  const sortedPaths = Object.keys(files).sort((a, b) => (files[a].order ?? 0) - (files[b].order ?? 0));
  for (const p of sortedPaths) {
    const entry = files[p];
    const imp = entry.importance > 0 ? ` (importance: ${entry.importance.toFixed(1)})` : '';
    if (entry.tier === 0) {
      lines.push(`#### \`${p}\`${imp}\n*${entry.content}*\n`);
    } else if (entry.tier === 4) {
      lines.push(`// ${p}${imp} — ${entry.content.replace(/^\/\/ .* — /, '')}`);
    } else {
      const ext = getExt(p);
      const lang = extToMarkdownLang(ext);
      lines.push(`#### \`${p}\`${tierLabel(entry.tier)}${imp}\n\`\`\`${lang}\n${entry.content}\`\`\`\n`);
    }
  }

  const toastSize = summary.unit === 'chars'
    ? `~${formatCharCount(summary.totalChars)}`
    : `~${formatTokenCount(summary.totalTokens)}`;
  const toast = `Copied ${summary.totalFiles} files (${toastSize}) — ${formatTierCounts(summary.tierCounts)}`;
  return { text: lines.join('\n'), toast };
}

/**
 * Build full file contents via /api/files (chunked).
 * Returns the formatted markdown string and a toast message.
 */
export async function buildFullContents(
  selected: Set<string>,
  manifest: Manifest,
): Promise<{ text: string; toast: string }> {
  const info: Record<string, { desc: string; category: string }> = {};
  for (const [cat, files] of Object.entries(manifest)) {
    for (const f of files) {
      if (selected.has(f.path) && !info[f.path]) {
        info[f.path] = { desc: f.desc, category: cat };
      }
    }
  }
  const paths = Object.keys(info);
  if (paths.length === 0) return { text: '', toast: 'No files to copy' };

  const CHUNK_SIZE = 200;
  const allFiles: Record<string, { content?: string; size?: number; error?: string }> = {};
  const chunks: string[][] = [];
  for (let i = 0; i < paths.length; i += CHUNK_SIZE) chunks.push(paths.slice(i, i + CHUNK_SIZE));

  const results = await Promise.all(
    chunks.map(async (chunk) => {
      const resp = await fetch('/api/files', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ paths: chunk }),
      });
      if (!resp.ok) {
        const errText = await resp.text().catch(() => 'Request failed');
        return { error: errText, chunk };
      }
      return resp.json();
    })
  );

  for (const result of results) {
    if (result.error && result.chunk) {
      for (const p of result.chunk as string[]) allFiles[p] = { error: result.error };
    } else if (result.files) Object.assign(allFiles, result.files);
  }

  const groups: Record<string, { path: string; desc: string; content?: string; error?: string }[]> = {};
  for (const p of paths) {
    const fileInfo = info[p]; if (!fileInfo) continue;
    if (!groups[fileInfo.category]) groups[fileInfo.category] = [];
    const fileData = allFiles[p];
    groups[fileInfo.category].push({ path: p, desc: fileInfo.desc, content: fileData?.content, error: fileData?.error });
  }

  let totalBytes = 0;
  for (const fileData of Object.values(allFiles)) { if (fileData.size) totalBytes += fileData.size; }
  const tokens = estimateTokens(totalBytes);

  const lines: string[] = [`## Relevant Source Files (${paths.length} files, ~${formatTokenCount(tokens)})`, ''];
  for (const cat of Object.keys(groups).sort()) {
    lines.push(`### ${cat}`);
    for (const f of groups[cat].sort((a, b) => a.path.localeCompare(b.path))) {
      const ext = getExt(f.path);
      const lang = extToMarkdownLang(ext);
      lines.push(`#### \`${f.path}\``);
      if (f.content) { lines.push('```' + lang); lines.push(f.content); lines.push('```'); }
      else lines.push(`*Error: ${f.error || 'Unknown error'}*`);
      lines.push('');
    }
  }

  const toast = `Copied ${paths.length} files (~${formatTokenCount(tokens)})`;
  return { text: lines.join('\n'), toast };
}

/**
 * Build a paths-only listing grouped by category with descriptions.
 */
export function buildPathsOnly(
  selected: Set<string>,
  manifest: Manifest,
): string {
  const info: Record<string, { desc: string; category: string }> = {};
  for (const [cat, files] of Object.entries(manifest)) {
    for (const f of files) {
      if (selected.has(f.path) && !info[f.path]) {
        info[f.path] = { desc: f.desc, category: cat };
      }
    }
  }
  const groups: Record<string, { path: string; desc: string }[]> = {};
  for (const [p, { desc, category }] of Object.entries(info)) {
    if (!groups[category]) groups[category] = [];
    groups[category].push({ path: p, desc });
  }
  const lines: string[] = [`## Relevant Source Files (${selected.size})`, ''];
  for (const cat of Object.keys(groups).sort()) {
    const files = groups[cat].sort((a, b) => a.path.localeCompare(b.path));
    lines.push(`### ${cat} (${files.length} file${files.length > 1 ? 's' : ''})`);
    for (const f of files) {
      lines.push(`- \`${f.path}\` — ${f.desc}`);
    }
    lines.push('');
  }
  return lines.join('\n');
}
