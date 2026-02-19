import { useRef, useMemo, useCallback, useState, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { FileIcon } from './icons';
import { getExt, getFilename } from './utils';
import { estimateTokens, formatTokenCount } from './tokenCount';
import { copyToClipboard, buildSmartContext, buildFullContents, buildPathsOnly } from './copyLogic';
import type { TreeNode, FlatTreeRow, Manifest } from './types';
import './styles/sidebar.css';

const MODEL_LIMITS: Record<string, { name: string; tokens: number }> = {
  'claude-200k': { name: 'Claude 200K', tokens: 200_000 },
  'gpt4-128k': { name: 'GPT-4 128K', tokens: 128_000 },
  'claude-100k': { name: 'Claude 100K', tokens: 100_000 },
  'custom-50k': { name: 'Custom 50K', tokens: 50_000 },
};

const MODEL_KEYS = Object.keys(MODEL_LIMITS);

interface Props {
  tree: TreeNode;
  manifest: Manifest;
  expanded: Set<string>;
  activeCategory: string | null;
  selected: Set<string>;
  globalSearch: string | null;
  onToggle: (id: string) => void;
  onSelect: (id: string) => void;
  onToggleFile: (path: string) => void;
  onToggleModule: (moduleId: string) => void;
  onClear: () => void;
  onCollapseAll: () => void;
  onExpandAll: () => void;
}

interface NodeMeta {
  children: { name: string; id: string; node: TreeNode; hasChildren: boolean }[];
  fileCount: number;
}

function buildMetaCache(node: TreeNode, prefix: string, cache: Map<string, NodeMeta>): number {
  let fileCount = (node._files as any[] || []).length;
  const children: NodeMeta['children'] = [];

  const entries = Object.entries(node)
    .filter(([k, v]) => k !== '_files' && typeof v === 'object' && !Array.isArray(v))
    .sort(([a], [b]) => a.localeCompare(b));

  for (const [name, child] of entries) {
    const childNode = child as TreeNode;
    const id = prefix ? `${prefix} > ${name}` : name;
    const childCount = buildMetaCache(childNode, id, cache);
    fileCount += childCount;
    const hasChildren = Object.keys(childNode).some(k => k !== '_files' && typeof childNode[k] === 'object' && !Array.isArray(childNode[k]));
    children.push({ name, id, node: childNode, hasChildren });
  }

  cache.set(prefix, { children, fileCount });
  return fileCount;
}

type FlatItem =
  | { type: 'category'; row: FlatTreeRow }
  | { type: 'file'; path: string; name: string; ext: string; depth: number };

function flattenTree(
  rootMeta: NodeMeta,
  expanded: Set<string>,
  metaCache: Map<string, NodeMeta>,
  manifest: Manifest,
): FlatItem[] {
  const items: FlatItem[] = [];
  const stack: { children: NodeMeta['children']; idx: number; depth: number }[] = [];
  stack.push({ children: rootMeta.children, idx: 0, depth: 0 });

  while (stack.length > 0) {
    const frame = stack[stack.length - 1];
    if (frame.idx >= frame.children.length) {
      stack.pop();
      continue;
    }

    const child = frame.children[frame.idx++];
    const { name, id, hasChildren } = child;

    const meta = metaCache.get(id);
    const isExpanded = expanded.has(id);
    const hasFiles = !!manifest[id]?.length;

    items.push({
      type: 'category',
      row: {
        id,
        name,
        depth: frame.depth,
        fileCount: meta?.fileCount ?? 0,
        hasChildren: hasChildren || hasFiles,
        isExpanded,
      },
    });

    if (isExpanded) {
      // Show individual files under this category
      const files = manifest[id];
      if (files?.length) {
        for (const f of files) {
          const fname = getFilename(f.path);
          items.push({
            type: 'file',
            path: f.path,
            name: fname,
            ext: getExt(fname),
            depth: frame.depth + 1,
          });
        }
      }
      // Then push child categories
      if (hasChildren && meta) {
        stack.push({ children: meta.children, idx: 0, depth: frame.depth + 1 });
      }
    }
  }

  return items;
}

/** Collect all file paths under a module ID (direct + nested children). */
function getModuleFilePaths(moduleId: string, manifest: Manifest): string[] {
  const prefix = moduleId + ' > ';
  const result: string[] = [];
  for (const [cat, files] of Object.entries(manifest)) {
    if (cat === moduleId || cat.startsWith(prefix)) {
      for (const f of files) result.push(f.path);
    }
  }
  return result;
}

type SelectionState = 'none' | 'partial' | 'all';

export default function TreeSidebar({
  tree, manifest, expanded, activeCategory, selected, globalSearch,
  onToggle, onSelect, onToggleFile, onToggleModule, onClear, onCollapseAll, onExpandAll,
}: Props) {
  const parentRef = useRef<HTMLDivElement>(null);
  const [modelKey, setModelKey] = useState<string>(MODEL_KEYS[0]);
  const [toast, setToast] = useState<string | null>(null);
  const [copying, setCopying] = useState(false);

  const limit = MODEL_LIMITS[modelKey]?.tokens ?? 200_000;

  const metaCache = useMemo(() => {
    const cache = new Map<string, NodeMeta>();
    buildMetaCache(tree, '', cache);
    return cache;
  }, [tree]);

  const items = useMemo(() => {
    const rootMeta = metaCache.get('');
    if (!rootMeta) return [];
    return flattenTree(rootMeta, expanded, metaCache, manifest);
  }, [expanded, metaCache, manifest]);

  // Compute tri-state selection for each module
  const moduleSelectionState = useMemo(() => {
    const states = new Map<string, SelectionState>();
    for (const [cat, files] of Object.entries(manifest)) {
      if (files.length === 0) { states.set(cat, 'none'); continue; }
      const selectedCount = files.filter(f => selected.has(f.path)).length;
      states.set(cat, selectedCount === 0 ? 'none' : selectedCount === files.length ? 'all' : 'partial');
    }
    return states;
  }, [manifest, selected]);

  // Token budget
  const sizeMap = useMemo(() => {
    const map = new Map<string, number>();
    for (const files of Object.values(manifest)) {
      for (const f of files) {
        if (!map.has(f.path)) map.set(f.path, f.size || 0);
      }
    }
    return map;
  }, [manifest]);

  const totalTokens = useMemo(() => {
    let bytes = 0;
    for (const p of selected) bytes += sizeMap.get(p) || 0;
    return estimateTokens(bytes);
  }, [selected, sizeMap]);

  const budgetRatio = limit > 0 ? totalTokens / limit : 0;
  const budgetLevel = budgetRatio > 0.9 ? 'danger' : budgetRatio > 0.6 ? 'warn' : 'ok';
  const budgetPercent = Math.min(100, budgetRatio * 100);

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 36,
    overscan: 20,
  });

  // Auto-scroll to activeCategory when it changes (e.g. from graph/treemap clicks)
  useEffect(() => {
    if (!activeCategory) return;
    const idx = items.findIndex(
      item => item.type === 'category' && item.row.id === activeCategory
    );
    if (idx >= 0) {
      virtualizer.scrollToIndex(idx, { align: 'center', behavior: 'smooth' });
    }
  }, [activeCategory, items, virtualizer]);

  const handleCategoryClick = useCallback((row: FlatTreeRow) => {
    onToggle(row.id);
    onSelect(row.id);
  }, [onToggle, onSelect]);

  const handleCheckboxClick = useCallback((e: React.MouseEvent, moduleId: string) => {
    e.stopPropagation();
    onToggleModule(moduleId);
  }, [onToggleModule]);

  // Copy handlers
  function showToast(msg: string) {
    setToast(msg);
    setTimeout(() => setToast(null), 2000);
  }

  const handleCopySmartContext = useCallback(async () => {
    if (selected.size === 0) return;
    setCopying(true);
    try {
      const result = await buildSmartContext(selected, manifest, globalSearch, limit);
      await copyToClipboard(result.text);
      showToast(result.toast);
    } catch (e) {
      showToast(`Copy failed: ${e instanceof Error ? e.message : 'unknown error'}`);
    } finally {
      setCopying(false);
    }
  }, [selected, manifest, globalSearch, limit]);

  const handleCopyFullContents = useCallback(async () => {
    if (selected.size === 0) return;
    setCopying(true);
    try {
      const result = await buildFullContents(selected, manifest);
      await copyToClipboard(result.text);
      showToast(result.toast);
    } catch (e) {
      showToast(`Copy failed: ${e instanceof Error ? e.message : 'unknown error'}`);
    } finally {
      setCopying(false);
    }
  }, [selected, manifest]);

  const handleCopyPathsOnly = useCallback(() => {
    if (selected.size === 0) return;
    const text = buildPathsOnly(selected, manifest);
    copyToClipboard(text).then(() => showToast(`Copied ${selected.size} file paths`));
  }, [selected, manifest]);

  return (
    <div className="sidebar tree-sidebar">
      <div className="sidebar-header">
        <span className="sidebar-title">Modules</span>
        <div className="sidebar-controls">
          <button className="sidebar-ctrl-btn" onClick={onCollapseAll} title="Collapse all">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M4 14h16M4 10h16"/>
            </svg>
          </button>
          <button className="sidebar-ctrl-btn" onClick={onExpandAll} title="Expand top level">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M4 6h16M4 12h10M4 18h16M14 12v6"/>
            </svg>
          </button>
        </div>
      </div>
      <div ref={parentRef} className="tree-scroll-area">
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map(vi => {
            const item = items[vi.index];

            if (item.type === 'file') {
              const isSelected = selected.has(item.path);
              return (
                <div
                  key={item.path}
                  className={`tree-file-row${isSelected ? ' selected' : ''}`}
                  style={{
                    position: 'absolute',
                    top: vi.start,
                    height: vi.size,
                    width: '100%',
                    paddingLeft: item.depth * 16 + 12,
                  }}
                  onClick={() => onToggleFile(item.path)}
                >
                  {item.depth > 0 && (
                    <span className="tree-indent-guide" style={{ left: item.depth * 16 - 2 }} />
                  )}
                  <FileIcon ext={item.ext} size={13} />
                  <span className="tree-file-name">{item.name}</span>
                  {isSelected && <span className="tree-file-check">&#10003;</span>}
                </div>
              );
            }

            const row = item.row;
            const isActive = activeCategory === row.id;
            const selState = moduleSelectionState.get(row.id) ?? 'none';
            // Also check nested children for partial state
            const effectiveState = selState === 'none' ? computeNestedState(row.id, moduleSelectionState) : selState;

            return (
              <div
                key={row.id}
                className={`tree-row${isActive ? ' active' : ''}`}
                style={{
                  position: 'absolute',
                  top: vi.start,
                  height: vi.size,
                  width: '100%',
                  paddingLeft: row.depth * 16 + 12,
                }}
                onClick={() => handleCategoryClick(row)}
              >
                {row.depth > 0 && (
                  <span className="tree-indent-guide" style={{ left: row.depth * 16 - 2 }} />
                )}
                <span
                  className={`tree-module-checkbox state-${effectiveState}`}
                  onClick={(e) => handleCheckboxClick(e, row.id)}
                  role="checkbox"
                  aria-checked={effectiveState === 'all' ? true : effectiveState === 'partial' ? 'mixed' : false}
                >
                  {effectiveState === 'all' ? '\u2611' : effectiveState === 'partial' ? '\u2612' : '\u2610'}
                </span>
                <span className={`tree-arrow${row.hasChildren ? (row.isExpanded ? ' open' : '') : ' leaf'}`}>
                  &#9654;
                </span>
                <span className="tree-name">{row.name}</span>
                <span className="tree-count">{row.fileCount}</span>
              </div>
            );
          })}
        </div>
      </div>

      {/* Sticky context footer */}
      {selected.size > 0 && (
        <div className="tree-context-footer">
          <div className="tree-context-summary">
            <span className="tree-context-count">{selected.size} files</span>
            <span className="tree-context-tokens">{formatTokenCount(totalTokens)}</span>
            <select
              className="tree-context-model"
              value={modelKey}
              onChange={e => setModelKey(e.target.value)}
            >
              {MODEL_KEYS.map(k => (
                <option key={k} value={k}>{MODEL_LIMITS[k].name}</option>
              ))}
            </select>
          </div>
          <div className="tree-context-budget-bar">
            <div
              className={`tree-context-budget-fill level-${budgetLevel}`}
              style={{ width: `${budgetPercent}%` }}
            />
          </div>
          <div className="tree-context-budget-label">
            <span>{formatTokenCount(totalTokens)}</span>
            <span>{formatTokenCount(limit)}</span>
          </div>
          <div className="tree-context-actions">
            <button onClick={handleCopySmartContext} disabled={copying}>Smart</button>
            <button onClick={handleCopyFullContents} disabled={copying}>Full</button>
            <button onClick={handleCopyPathsOnly} disabled={copying}>Paths</button>
            <button className="tree-context-clear" onClick={onClear}>Clear</button>
          </div>
        </div>
      )}

      {/* Toast */}
      <div className={`context-toast${toast ? ' show' : ''}`}>{toast}</div>
    </div>
  );
}

/** Check if any nested child modules have selected files. */
function computeNestedState(moduleId: string, states: Map<string, SelectionState>): SelectionState {
  const prefix = moduleId + ' > ';
  let hasAny = false;
  let allFull = true;
  let hasModules = false;
  for (const [key, state] of states) {
    if (key === moduleId || key.startsWith(prefix)) {
      hasModules = true;
      if (state !== 'none') hasAny = true;
      if (state !== 'all') allFull = false;
    }
  }
  if (!hasModules) return 'none';
  if (!hasAny) return 'none';
  if (allFull) return 'all';
  return 'partial';
}
