import { useRef, useMemo, useCallback } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { FileIcon } from './icons';
import { getExt, getFilename } from './utils';
import type { TreeNode, FlatTreeRow, Manifest } from './types';
import './styles/sidebar.css';

interface Props {
  tree: TreeNode;
  manifest: Manifest;
  expanded: Set<string>;
  activeCategory: string | null;
  selected: Set<string>;
  onToggle: (id: string) => void;
  onSelect: (id: string) => void;
  onToggleFile: (path: string) => void;
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

export default function TreeSidebar({ tree, manifest, expanded, activeCategory, selected, onToggle, onSelect, onToggleFile, onCollapseAll, onExpandAll }: Props) {
  const parentRef = useRef<HTMLDivElement>(null);

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

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 36,
    overscan: 20,
  });

  const handleCategoryClick = useCallback((row: FlatTreeRow) => {
    onToggle(row.id);
    onSelect(row.id);
  }, [onToggle, onSelect]);

  return (
    <div ref={parentRef} className="sidebar">
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
              </div>
            );
          }

          const row = item.row;
          const isActive = activeCategory === row.id;
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
  );
}
