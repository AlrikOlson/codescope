import type { TreemapNode, Viewport } from './types';

const BORDER_COLOR = 'rgba(62, 62, 85, 0.6)';
const ACTIVE_COLOR = '#89b4fa';
const SELECTED_DOT = '#a6e3a1';
const LABEL_COLOR = '#cdd6f4';
const LABEL_SHADOW = 'rgba(0,0,0,0.7)';

export function getMaxDepth(scale: number): number {
  return Math.max(2, Math.min(10, Math.floor(Math.log2(Math.max(1, scale))) + 3));
}

export function renderTreemap(
  ctx: CanvasRenderingContext2D,
  root: TreemapNode,
  viewport: Viewport,
  canvasW: number,
  canvasH: number,
  dpr: number,
  activeCategory: string | null,
  selectedModules: Set<string>, // module ids that contain selected files
  hoveredNode: TreemapNode | null,
): void {
  ctx.save();
  ctx.clearRect(0, 0, canvasW, canvasH);
  ctx.scale(dpr, dpr);

  const w = canvasW / dpr;
  const h = canvasH / dpr;
  const { x: vx, y: vy, scale } = viewport;

  const maxDepth = getMaxDepth(scale);

  drawNode(ctx, root, vx, vy, scale, w, h, maxDepth, activeCategory, selectedModules, hoveredNode);
  ctx.restore();
}

function drawNode(
  ctx: CanvasRenderingContext2D,
  node: TreemapNode,
  vx: number, vy: number, scale: number,
  canvasW: number, canvasH: number,
  maxDepth: number,
  activeCategory: string | null,
  selectedModules: Set<string>,
  hoveredNode: TreemapNode | null,
): void {
  // Transform to screen space
  const sx = node.x * scale + vx;
  const sy = node.y * scale + vy;
  const sw = node.w * scale;
  const sh = node.h * scale;

  // Viewport culling
  if (sx + sw < 0 || sy + sh < 0 || sx > canvasW || sy > canvasH) return;

  // Skip tiny nodes
  if (sw < 1 || sh < 1) return;

  const isLeafLevel = node.children.length === 0 || node.depth >= maxDepth;

  if (isLeafLevel || sw < 4 || sh < 4) {
    // Draw filled rect
    ctx.fillStyle = node.color;
    ctx.fillRect(sx, sy, sw, sh);

    // Border
    ctx.strokeStyle = BORDER_COLOR;
    ctx.lineWidth = node.depth <= 1 ? 1.5 : 0.5;
    ctx.strokeRect(sx, sy, sw, sh);

    // Hovered highlight
    if (hoveredNode === node) {
      ctx.fillStyle = 'rgba(255,255,255,0.08)';
      ctx.fillRect(sx, sy, sw, sh);
    }

    // Active category highlight
    if (node.id === activeCategory) {
      ctx.strokeStyle = ACTIVE_COLOR;
      ctx.lineWidth = 2;
      ctx.strokeRect(sx + 1, sy + 1, sw - 2, sh - 2);
    }

    // Selected badge
    if (selectedModules.has(node.id)) {
      const r = Math.min(5, sw * 0.08, sh * 0.08);
      if (r >= 2) {
        ctx.fillStyle = SELECTED_DOT;
        ctx.beginPath();
        ctx.arc(sx + sw - r - 3, sy + r + 3, r, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    // Label
    if (sw > 50 && sh > 20) {
      const fontSize = Math.max(8, Math.min(14, sw * 0.07, sh * 0.35));
      ctx.font = `600 ${fontSize}px -apple-system, 'Segoe UI', system-ui, sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';

      const label = truncateLabel(node.name, ctx, sw - 8);
      ctx.fillStyle = LABEL_SHADOW;
      ctx.fillText(label, sx + sw / 2 + 0.5, sy + sh / 2 + 0.5);
      ctx.fillStyle = LABEL_COLOR;
      ctx.fillText(label, sx + sw / 2, sy + sh / 2);

      // File count below name if space allows
      if (sh > 36 && fontSize >= 10) {
        const countSize = Math.max(7, fontSize * 0.7);
        ctx.font = `400 ${countSize}px -apple-system, 'Segoe UI', system-ui, sans-serif`;
        ctx.fillStyle = 'rgba(205,214,244,0.5)';
        ctx.fillText(`${node.fileCount.toLocaleString()}`, sx + sw / 2, sy + sh / 2 + fontSize * 0.8);
      }
    }
  } else {
    // Parent: draw background, then recurse into children
    ctx.fillStyle = darken(node.color, 0.3);
    ctx.fillRect(sx, sy, sw, sh);

    // Parent border
    ctx.strokeStyle = BORDER_COLOR;
    ctx.lineWidth = node.depth === 0 ? 2 : 1;
    ctx.strokeRect(sx, sy, sw, sh);

    // Parent label at top if enough space
    if (sw > 80 && sh > 30) {
      const fontSize = Math.max(8, Math.min(12, sw * 0.04));
      ctx.font = `600 ${fontSize}px -apple-system, 'Segoe UI', system-ui, sans-serif`;
      ctx.textAlign = 'left';
      ctx.textBaseline = 'top';
      ctx.fillStyle = 'rgba(205,214,244,0.4)';
      const label = truncateLabel(node.name, ctx, sw - 8);
      ctx.fillText(label, sx + 4, sy + 2);
    }

    // Active highlight on parent
    if (node.id === activeCategory) {
      ctx.strokeStyle = ACTIVE_COLOR;
      ctx.lineWidth = 2;
      ctx.strokeRect(sx + 1, sy + 1, sw - 2, sh - 2);
    }

    for (const child of node.children) {
      drawNode(ctx, child, vx, vy, scale, canvasW, canvasH, maxDepth, activeCategory, selectedModules, hoveredNode);
    }
  }
}

function truncateLabel(text: string, ctx: CanvasRenderingContext2D, maxWidth: number): string {
  if (ctx.measureText(text).width <= maxWidth) return text;
  let t = text;
  while (t.length > 1 && ctx.measureText(t + '…').width > maxWidth) {
    t = t.slice(0, -1);
  }
  return t + '…';
}

function darken(rgb: string, amount: number): string {
  const m = rgb.match(/rgb\((\d+),(\d+),(\d+)\)/);
  if (!m) return rgb;
  const r = Math.round(+m[1] * (1 - amount));
  const g = Math.round(+m[2] * (1 - amount));
  const b = Math.round(+m[3] * (1 - amount));
  return `rgb(${r},${g},${b})`;
}

// Hit-test: find deepest node at screen coordinate
export function hitTest(
  root: TreemapNode,
  sx: number, sy: number,
  viewport: Viewport,
  maxDepth: number,
): TreemapNode | null {
  return hitTestNode(root, sx, sy, viewport, maxDepth);
}

function hitTestNode(
  node: TreemapNode,
  sx: number, sy: number,
  vp: Viewport,
  maxDepth: number,
): TreemapNode | null {
  const nx = node.x * vp.scale + vp.x;
  const ny = node.y * vp.scale + vp.y;
  const nw = node.w * vp.scale;
  const nh = node.h * vp.scale;

  if (sx < nx || sy < ny || sx > nx + nw || sy > ny + nh) return null;

  // Check children first (deepest match)
  if (node.depth < maxDepth) {
    for (const child of node.children) {
      const found = hitTestNode(child, sx, sy, vp, maxDepth);
      if (found) return found;
    }
  }

  return node.id ? node : null; // skip root (empty id)
}
