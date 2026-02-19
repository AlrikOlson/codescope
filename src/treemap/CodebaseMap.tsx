import { useRef, useMemo, useCallback, useEffect, useState } from 'react';
import type { TreeNode, Manifest } from '../types';
import type { TreemapNode } from './types';
import { buildTreemapData } from './buildTreemapData';
import { layoutTreemap } from './layout';
import { renderTreemap, hitTest, getMaxDepth } from './render';
import { useViewport } from './useViewport';
import '../styles/codemap.css';

interface Props {
  tree: TreeNode;
  manifest: Manifest;
  activeCategory: string | null;
  selected: Set<string>;
  onNavigateModule: (id: string) => void;
  onToggleFile: (path: string) => void;
  onSelectModule: (moduleId: string) => void;
}

export default function CodebaseMap({ tree, manifest, activeCategory, selected, onNavigateModule, onToggleFile, onSelectModule }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rootRef = useRef<TreemapNode | null>(null);
  const sizeRef = useRef({ w: 0, h: 0 });
  const [tooltip, setTooltip] = useState<{ x: number; y: number; node: TreemapNode } | null>(null);
  const hoveredRef = useRef<TreemapNode | null>(null);
  const dragMovedRef = useRef(false);

  // Build treemap data
  const rootNode = useMemo(() => buildTreemapData(tree), [tree]);

  // Which modules/files are selected — includes file: prefixed ids AND ancestor paths
  // so parent treemap nodes show selection indicators when any child is selected
  const selectedModules = useMemo(() => {
    const mods = new Set<string>();
    for (const [cat, files] of Object.entries(manifest)) {
      for (const f of files) {
        if (selected.has(f.path)) {
          mods.add(cat);
          mods.add(`file:${f.path}`);
          // Add all ancestor paths so parent nodes highlight too
          const parts = cat.split(' > ');
          for (let i = 1; i < parts.length; i++) {
            mods.add(parts.slice(0, i).join(' > '));
          }
        }
      }
    }
    return mods;
  }, [manifest, selected]);

  const canvasSize = useCallback(() => sizeRef.current, []);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    const root = rootRef.current;
    if (!canvas || !root) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const vp = vpAPI.viewportRef.current;
    renderTreemap(ctx, root, vp, canvas.width, canvas.height, window.devicePixelRatio || 1, activeCategory, selectedModules, hoveredRef.current);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeCategory, selectedModules]);

  const vpAPI = useViewport(draw, canvasSize);

  // Layout on mount + resize
  useEffect(() => {
    const container = containerRef.current;
    const canvas = canvasRef.current;
    if (!container || !canvas) return;

    const doLayout = () => {
      const rect = container.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      const w = rect.width;
      const h = rect.height;
      sizeRef.current = { w, h };
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;

      // Layout treemap into available space
      layoutTreemap(rootNode, { x: 0, y: 0, w, h });
      rootRef.current = rootNode;

      // Trigger redraw
      vpAPI.viewportRef.current = { ...vpAPI.viewportRef.current };
      draw();
    };

    const raf = requestAnimationFrame(() => doLayout());

    const ro = new ResizeObserver(() => {
      doLayout();
    });
    ro.observe(container);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, [rootNode, draw, vpAPI.viewportRef]);

  // Bind canvas events
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    return vpAPI.bind(canvas);
  }, [vpAPI]);

  // Mouse tracking for tooltip + click
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const onMove = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      const root = rootRef.current;
      if (!root) return;
      const vp = vpAPI.viewportRef.current;
      const maxDepth = getMaxDepth(vp.scale);
      const hit = hitTest(root, mx, my, vp, maxDepth);
      hoveredRef.current = hit;
      if (hit) {
        setTooltip({ x: e.clientX, y: e.clientY, node: hit });
      } else {
        setTooltip(null);
      }
    };

    const onDown = () => { dragMovedRef.current = false; };
    const onMoveTrack = (e: MouseEvent) => {
      if (e.buttons === 1) dragMovedRef.current = true;
    };

    const onClick = (e: MouseEvent) => {
      if (dragMovedRef.current) return;
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      const root = rootRef.current;
      if (!root) return;
      const vp = vpAPI.viewportRef.current;
      const maxDepth = getMaxDepth(vp.scale);
      const hit = hitTest(root, mx, my, vp, maxDepth);
      if (hit && hit.id) {
        if (hit.id.startsWith('file:')) {
          onToggleFile(hit.id.slice(5));
        } else {
          onSelectModule(hit.id);
        }
      }
    };

    const onDblClick = (e: MouseEvent) => {
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      const root = rootRef.current;
      if (!root) return;
      const vp = vpAPI.viewportRef.current;
      const maxDepth = getMaxDepth(vp.scale);
      const hit = hitTest(root, mx, my, vp, maxDepth);
      if (hit) {
        vpAPI.zoomToNode(hit);
      }
    };

    canvas.addEventListener('mousemove', onMove);
    canvas.addEventListener('mousedown', onDown);
    window.addEventListener('mousemove', onMoveTrack);
    canvas.addEventListener('click', onClick);
    canvas.addEventListener('dblclick', onDblClick);

    return () => {
      canvas.removeEventListener('mousemove', onMove);
      canvas.removeEventListener('mousedown', onDown);
      window.removeEventListener('mousemove', onMoveTrack);
      canvas.removeEventListener('click', onClick);
      canvas.removeEventListener('dblclick', onDblClick);
    };
  }, [vpAPI, onNavigateModule, onToggleFile, onSelectModule]);

  // Redraw when active/selected changes
  useEffect(() => { draw(); }, [draw, activeCategory, selectedModules]);

  // Breadcrumb
  const breadcrumb = activeCategory ? activeCategory.split(' > ') : [];

  return (
    <div className="codemap">
      <div className="codemap-toolbar">
        <button className="codemap-btn" onClick={() => vpAPI.resetView()}>Fit All</button>
        {activeCategory && (
          <button className="codemap-btn" onClick={() => {
            const root = rootRef.current;
            if (!root) return;
            const node = findNode(root, activeCategory);
            if (node) vpAPI.zoomToNode(node);
          }}>Focus Active</button>
        )}
        {breadcrumb.length > 0 && (
          <span className="codemap-breadcrumb">
            {breadcrumb.map((seg, i) => (
              <span key={i}>
                {i > 0 && <span className="codemap-sep"> › </span>}
                <span className="codemap-seg">{seg}</span>
              </span>
            ))}
          </span>
        )}
      </div>
      <div className="codemap-canvas-wrap" ref={containerRef}>
        <canvas ref={canvasRef} />
      </div>
      {tooltip && (
        <div
          className="codemap-tooltip"
          style={{
            left: Math.min(tooltip.x + 12, window.innerWidth - 220),
            top: tooltip.y + 16,
          }}
        >
          <div className="codemap-tooltip-name">{tooltip.node.name}</div>
          <div className="codemap-tooltip-id">{tooltip.node.id.startsWith('file:') ? tooltip.node.id.slice(5) : tooltip.node.id}</div>
          {tooltip.node.id.startsWith('file:') ? null : (
            <div className="codemap-tooltip-count">{tooltip.node.fileCount.toLocaleString()} files</div>
          )}
          <div className="codemap-tooltip-bar">
            {Object.entries(tooltip.node.extBreakdown)
              .sort((a, b) => b[1] - a[1])
              .slice(0, 5)
              .map(([ext, count]) => (
                <span key={ext} className="codemap-tooltip-ext">
                  {ext} {count}
                </span>
              ))}
          </div>
        </div>
      )}
    </div>
  );
}

function findNode(root: TreemapNode, id: string): TreemapNode | null {
  if (root.id === id) return root;
  for (const child of root.children) {
    const found = findNode(child, id);
    if (found) return found;
  }
  return null;
}
