import type { SearchResponse, GrepResponse, FileSearchResult, ModuleSearchResult, GrepFileResult } from './types';

export type UnifiedResult =
  | { type: 'module-header' }
  | { type: 'file-header' }
  | { type: 'content-header' }
  | { type: 'module'; data: ModuleSearchResult }
  | { type: 'file'; data: FileSearchResult }
  | { type: 'grep-file'; data: GrepFileResult }
  | { type: 'grep-match'; line: string; lineNum: number; filePath: string };

export const EMPTY_SEARCH: SearchResponse = { files: [], modules: [], queryTime: 0, totalFiles: 0, totalModules: 0 };

export function HighlightedText({ text, indices }: { text: string; indices: number[] }) {
  if (indices.length === 0) return <>{text}</>;
  const set = new Set(indices);
  const parts: JSX.Element[] = [];
  let run = '';
  let inMatch = false;

  for (let i = 0; i < text.length; i++) {
    const isMatch = set.has(i);
    if (isMatch !== inMatch) {
      if (run) {
        parts.push(inMatch ? <mark key={i}>{run}</mark> : <span key={i}>{run}</span>);
      }
      run = '';
      inMatch = isMatch;
    }
    run += text[i];
  }
  if (run) {
    parts.push(inMatch ? <mark key="end">{run}</mark> : <span key="end">{run}</span>);
  }
  return <>{parts}</>;
}

export function buildFlatResults(
  results: SearchResponse,
  grepResults: GrepResponse | null,
): UnifiedResult[] {
  const items: UnifiedResult[] = [];

  if (results.modules.length > 0) {
    items.push({ type: 'module-header' });
    for (const m of results.modules) items.push({ type: 'module', data: m });
  }

  if (results.files.length > 0) {
    items.push({ type: 'file-header' });
    for (const f of results.files) items.push({ type: 'file', data: f });
  }

  if (grepResults && grepResults.results.length > 0) {
    items.push({ type: 'content-header' });
    for (const file of grepResults.results) {
      items.push({ type: 'grep-file', data: file });
      for (const match of file.matches) {
        items.push({ type: 'grep-match', line: match.line, lineNum: match.lineNum, filePath: file.path });
      }
    }
  }

  return items;
}
