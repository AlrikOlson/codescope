import { useRef, useMemo, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { FileIcon } from './icons';
import { getExtColor } from './colors';
import { getExt, getFilename, getDir } from './utils';
import type { Manifest, DepGraph } from './types';
import './styles/filelist.css';

const TOKEN_CAP = 1_000_000; // ~1M tokens max across all listed files

interface FileWithCategory {
  path: string;
  desc: string;
  category: string;
  ext: string;
  filename: string;
  dir: string;
  size: number;
  score: number;
  rank?: number;     // 1-based rank within direct matches (by score)
  related?: boolean; // true if added via dependency expansion (not a direct match)
}

type SortMode = 'relevance' | 'category';

interface Props {
  manifest: Manifest;
  deps: DepGraph | null;
  activeCategory: string | null;
  globalSearch: string | null;
  globalSearchPaths: Map<string, number> | null;
  selected: Set<string>;
  onToggleFile: (path: string) => void;
  onSelectAll: (paths: string[]) => void;
  onDeselectAll: (paths: string[]) => void;
  onPreview: (path: string) => void;
}

type FlatRow =
  | { type: 'group'; name: string; count: number }
  | { type: 'related-header'; count: number }
  | { type: 'file'; file: FileWithCategory };

// Pre-count groups in O(n) instead of O(n²)
function countGroups(files: FileWithCategory[], getGroup: (f: FileWithCategory) => string): Map<string, number> {
  const counts = new Map<string, number>();
  for (const f of files) {
    const g = getGroup(f);
    if (g) counts.set(g, (counts.get(g) || 0) + 1);
  }
  return counts;
}

// Find which dep module a manifest category belongs to (longest matching categoryPath prefix)
function categoryToModule(category: string, deps: DepGraph): string | null {
  let best: string | null = null;
  let bestLen = 0;
  for (const [mod_name, entry] of Object.entries(deps)) {
    const cp = entry.categoryPath;
    if ((category === cp || category.startsWith(cp + ' > ')) && cp.length > bestLen) {
      best = mod_name;
      bestLen = cp.length;
    }
  }
  return best;
}

export default function FileList({ manifest, deps, activeCategory, globalSearch, globalSearchPaths, selected, onToggleFile, onSelectAll, onDeselectAll, onPreview }: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [sortMode, setSortMode] = useState<SortMode>('relevance');

  const { rows, files, extCounts } = useMemo(() => {
    // Global search mode: search ALL modules
    if (globalSearch) {
      const directFiles: FileWithCategory[] = [];
      const seen = new Set<string>();

      if (globalSearchPaths) {
        // Path-based search (from grep results): show exact files with scores
        for (const [cat, catFiles] of Object.entries(manifest)) {
          for (const f of catFiles) {
            if (seen.has(f.path)) continue;
            if (!globalSearchPaths.has(f.path)) continue;
            seen.add(f.path);
            directFiles.push({
              path: f.path, desc: f.desc, category: cat, size: f.size,
              ext: getExt(f.path), filename: getFilename(f.path), dir: getDir(f.path),
              score: globalSearchPaths.get(f.path) ?? 0,
            });
          }
        }
      } else {
        // Keyword-based search: fuzzy match across path/desc/category
        const term = globalSearch.toLowerCase();
        const tokens = term.split(/\s+/).filter(Boolean);
        for (const [cat, catFiles] of Object.entries(manifest)) {
          for (const f of catFiles) {
            if (seen.has(f.path)) continue;
            const haystack = (f.path + ' ' + f.desc + ' ' + cat).toLowerCase();
            if (!tokens.every(t => haystack.includes(t))) continue;
            seen.add(f.path);
            directFiles.push({
              path: f.path, desc: f.desc, category: cat, size: f.size,
              ext: getExt(f.path), filename: getFilename(f.path), dir: getDir(f.path),
              score: 0,
            });
          }
        }
      }

      // Expand with dependency-connected files
      const relatedFiles: FileWithCategory[] = [];
      if (deps && directFiles.length > 0) {
        // Find modules that contain direct-match files
        const matchedModules = new Set<string>();
        for (const f of directFiles) {
          const m = categoryToModule(f.category, deps);
          if (m) matchedModules.add(m);
        }

        // Collect connected modules (1-degree: deps + reverse deps)
        const connectedModules = new Set<string>();
        // Build reverse dep index
        const reverseDeps = new Map<string, Set<string>>();
        for (const [mod_name, entry] of Object.entries(deps)) {
          for (const dep of [...entry.public, ...entry.private]) {
            if (!reverseDeps.has(dep)) reverseDeps.set(dep, new Set());
            reverseDeps.get(dep)!.add(mod_name);
          }
        }

        for (const m of matchedModules) {
          // Modules that m depends on
          const entry = deps[m];
          if (entry) {
            for (const dep of [...entry.public, ...entry.private]) {
              if (!matchedModules.has(dep)) connectedModules.add(dep);
            }
          }
          // Modules that depend on m
          const dependents = reverseDeps.get(m);
          if (dependents) {
            for (const dep of dependents) {
              if (!matchedModules.has(dep)) connectedModules.add(dep);
            }
          }
        }

        // Collect connected module category prefixes for matching
        const connectedPrefixes: string[] = [];
        for (const m of connectedModules) {
          const entry = deps[m];
          if (entry) connectedPrefixes.push(entry.categoryPath);
        }

        // Add files from connected modules (only .h headers to keep it focused)
        const bestScore = directFiles.reduce((max, f) => Math.max(max, f.score), 0);
        const relatedScore = bestScore * 0.3; // 30% of best direct match score

        for (const [cat, catFiles] of Object.entries(manifest)) {
          // Check if this category belongs to a connected module
          const isConnected = connectedPrefixes.some(
            prefix => cat === prefix || cat.startsWith(prefix + ' > ')
          );
          if (!isConnected) continue;

          for (const f of catFiles) {
            if (seen.has(f.path)) continue;
            // Only include headers from related modules (most structurally useful)
            if (getExt(f.path) !== '.h') continue;
            seen.add(f.path);
            relatedFiles.push({
              path: f.path, desc: f.desc, category: cat, size: f.size,
              ext: getExt(f.path), filename: getFilename(f.path), dir: getDir(f.path),
              score: relatedScore,
              related: true,
            });
          }
        }
      }

      // Cap total to ~1M tokens: directs first (by score), then related
      directFiles.sort((a, b) => b.score - a.score || a.size - b.size);
      const cappedDirects: FileWithCategory[] = [];
      let tokenBudget = TOKEN_CAP;
      for (const f of directFiles) {
        const t = Math.ceil(f.size / 3);
        if (tokenBudget - t < 0 && cappedDirects.length > 0) continue;
        tokenBudget -= t;
        cappedDirects.push(f);
      }
      if (tokenBudget < 0) tokenBudget = 0;

      // Fill remaining budget with related files
      relatedFiles.sort((a, b) => b.score - a.score || a.size - b.size);
      const cappedRelated: FileWithCategory[] = [];
      for (const f of relatedFiles) {
        const t = Math.ceil(f.size / 3);
        if (tokenBudget - t < 0) continue;
        tokenBudget -= t;
        cappedRelated.push(f);
      }

      const allFiles = [...cappedDirects, ...cappedRelated];

      const counts: Record<string, number> = {};
      for (const f of allFiles) counts[f.ext] = (counts[f.ext] || 0) + 1;

      const rows: FlatRow[] = [];

      if (sortMode === 'relevance') {
        // Direct matches first (already sorted by score) with rank numbers
        for (let i = 0; i < cappedDirects.length; i++) {
          cappedDirects[i].rank = i + 1;
          rows.push({ type: 'file', file: cappedDirects[i] });
        }
        if (cappedRelated.length > 0) {
          // Sort related files by score (importance) instead of alphabetically
          cappedRelated.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
          rows.push({ type: 'related-header', count: cappedRelated.length });
          for (const f of cappedRelated) {
            rows.push({ type: 'file', file: f });
          }
        }
      } else {
        // Group by category, categories sorted by best score, files within by score
        const catScores = new Map<string, number>();
        for (const f of allFiles) {
          catScores.set(f.category, Math.max(catScores.get(f.category) ?? 0, f.score));
        }
        allFiles.sort((a, b) => {
          const catCmp = (catScores.get(b.category) ?? 0) - (catScores.get(a.category) ?? 0)
            || a.category.localeCompare(b.category);
          if (catCmp !== 0) return catCmp;
          return b.score - a.score || a.path.localeCompare(b.path);
        });

        const groupCounts = countGroups(allFiles, f => f.category);
        let lastCat = '';
        for (const f of allFiles) {
          if (f.category !== lastCat) {
            rows.push({ type: 'group', name: f.category, count: groupCounts.get(f.category) || 0 });
            lastCat = f.category;
          }
          rows.push({ type: 'file', file: f });
        }
      }

      return { rows, files: allFiles, extCounts: counts };
    }

    // Category mode
    if (!activeCategory) return { rows: [] as FlatRow[], files: [] as FileWithCategory[], extCounts: {} as Record<string, number> };

    const allFiles: FileWithCategory[] = [];
    const seen = new Set<string>();
    const prefix = activeCategory;
    const prefixDot = prefix + ' > ';
    for (const [cat, catFiles] of Object.entries(manifest)) {
      if (cat !== prefix && !cat.startsWith(prefixDot)) continue;
      for (const f of catFiles) {
        if (seen.has(f.path)) continue;
        seen.add(f.path);
        allFiles.push({
          path: f.path,
          desc: f.desc,
          category: cat,
          size: f.size,
          ext: getExt(f.path),
          filename: getFilename(f.path),
          dir: getDir(f.path),
          score: 0,
        });
      }
    }
    allFiles.sort((a, b) => a.category.localeCompare(b.category) || a.path.localeCompare(b.path));

    const counts: Record<string, number> = {};
    for (const f of allFiles) counts[f.ext] = (counts[f.ext] || 0) + 1;

    // Pre-count groups in O(n)
    const prefixLen = activeCategory.length + 3;
    const getGroup = (f: FileWithCategory) => f.category.startsWith(prefixDot) ? f.category.slice(prefixLen) : '';
    const groupCounts = countGroups(allFiles, getGroup);

    const rows: FlatRow[] = [];
    let lastGroup = '';
    for (const f of allFiles) {
      const group = getGroup(f);
      if (group && group !== lastGroup) {
        rows.push({ type: 'group', name: group, count: groupCounts.get(group) || 0 });
        lastGroup = group;
      }
      rows.push({ type: 'file', file: f });
    }

    return { rows, files: allFiles, extCounts: counts };
  }, [manifest, deps, activeCategory, globalSearch, globalSearchPaths, sortMode]);

  const directCount = useMemo(() => files.filter(f => !f.related).length, [files]);
  const relatedCount = useMemo(() => files.filter(f => f.related).length, [files]);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (i) => {
      const r = rows[i];
      if (!r) return 44;
      if (r.type === 'group' || r.type === 'related-header') return 36;
      return 44;
    },
    overscan: 25,
  });

  const allPaths = useMemo(() => files.map(f => f.path), [files]);

  if (!activeCategory && !globalSearch) {
    return (
      <div className="main-panel">
        <div className="empty-state">
          <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1" opacity="0.15">
            <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
          </svg>
          <div className="empty-state-title">Explore the Codebase</div>
          <div className="empty-state-desc">
            {Object.keys(manifest).length.toLocaleString()} modules ready to browse
          </div>
          <div className="empty-state-actions">
            <button className="empty-action-btn" onClick={() => {
              const event = new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true });
              window.dispatchEvent(event);
            }}>
              <kbd>Ctrl+K</kbd>
              <span>Search files & modules</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  const totalFiles = files.length;

  const categoryCount = globalSearch ? new Set(files.map(f => f.category)).size : 0;

  const title = globalSearch
    ? `"${globalSearch}"`
    : activeCategory!.split(' > ').pop()!;

  const subtitle = globalSearch
    ? relatedCount > 0
      ? `${directCount.toLocaleString()} matches + ${relatedCount.toLocaleString()} related across ${categoryCount} modules`
      : `${totalFiles.toLocaleString()} files across ${categoryCount} modules`
    : activeCategory!.split(' > ').join(' › ');

  return (
    <div className="main-panel">
      {/* Summary header */}
      <div className="module-summary">
        <div className="module-summary-top">
          <h2 className="module-title">{title}</h2>
          <span className="breadcrumb">{subtitle}</span>
        </div>
        <div className="module-summary-actions">
          <span className="file-count">{totalFiles.toLocaleString()} files</span>
          {Object.entries(extCounts).sort((a, b) => b[1] - a[1]).slice(0, 5).map(([ext, count]) => (
            <span key={ext} className="ext-chip" style={{ color: getExtColor(ext) }}>
              {ext} {count}
            </span>
          ))}
          <div className="module-actions-spacer" />
          {globalSearch && (
            <div className="sort-toggle">
              <button className={`sort-btn${sortMode === 'relevance' ? ' active' : ''}`} onClick={() => setSortMode('relevance')}>
                Relevance
              </button>
              <button className={`sort-btn${sortMode === 'category' ? ' active' : ''}`} onClick={() => setSortMode('category')}>
                Category
              </button>
            </div>
          )}
          <div className="module-actions">
            <button className="chip-btn" onClick={() => onSelectAll(allPaths)}>Select All</button>
            <button className="chip-btn" onClick={() => onDeselectAll(allPaths)}>Deselect</button>
          </div>
        </div>
      </div>

      {/* Virtualized file list */}
      <div ref={parentRef} className="file-scroll">
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map(vi => {
            const item = rows[vi.index];

            if (item.type === 'group') {
              return (
                <div
                  key={`g-${item.name}`}
                  className="file-group-header"
                  style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}
                >
                  <span className="file-group-name">{item.name}</span>
                  <span className="file-group-count">{item.count}</span>
                </div>
              );
            }

            if (item.type === 'related-header') {
              return (
                <div
                  key="related-hdr"
                  className="file-group-header related-header"
                  style={{ position: 'absolute', top: vi.start, height: vi.size, width: '100%' }}
                >
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ opacity: 0.5 }}>
                    <circle cx="6" cy="6" r="3"/><circle cx="18" cy="18" r="3"/><path d="M9 6h6M6 9v6M18 9v6"/>
                  </svg>
                  <span className="file-group-name">Related via dependencies</span>
                  <span className="file-group-count">{item.count} headers</span>
                </div>
              );
            }

            const f = item.file;
            const isSelected = selected.has(f.path);
            const borderColor = getExtColor(f.ext);
            const isSmartPicked = isSelected && !f.related && globalSearch !== null;

            return (
              <div
                key={f.path}
                className={`file-row${isSelected ? ' selected' : ''}${f.related ? ' related' : ''}${isSmartPicked ? ' smart-picked' : ''}`}
                style={{
                  position: 'absolute',
                  top: vi.start,
                  height: vi.size,
                  width: '100%',
                  borderLeftColor: borderColor,
                  '--row-idx': Math.min(vi.index, 15),
                } as React.CSSProperties}
                onClick={() => onToggleFile(f.path)}
              >
                <label className="checkbox" onClick={e => e.stopPropagation()}>
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => onToggleFile(f.path)}
                  />
                  <span className="checkbox-box" />
                </label>
                <FileIcon ext={f.ext} />
                <span className="file-name">{f.filename}</span>
                <span className="file-dir">{f.dir}/</span>
                <button
                  className="file-preview-btn"
                  onClick={e => { e.stopPropagation(); onPreview(f.path); }}
                  title="Preview"
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>
                  </svg>
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
