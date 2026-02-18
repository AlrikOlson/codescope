import type { TreemapNode, Rect } from './types';

// Squarified treemap layout (Bruls, Huizing, van Wijk)
export function layoutTreemap(root: TreemapNode, rect: Rect): void {
  root.x = rect.x;
  root.y = rect.y;
  root.w = rect.w;
  root.h = rect.h;
  if (root.children.length > 0) {
    squarify(root.children, root.value, rect);
  }
}

function squarify(children: TreemapNode[], parentValue: number, rect: Rect): void {
  if (children.length === 0 || rect.w <= 0 || rect.h <= 0) return;

  const scale = (rect.w * rect.h) / parentValue;
  let remaining = { ...rect };
  let i = 0;

  while (i < children.length) {
    const row: TreemapNode[] = [];
    let rowSum = 0;
    const isWide = remaining.w >= remaining.h;
    const side = isWide ? remaining.h : remaining.w;

    if (side <= 0) break;

    // Add first item
    row.push(children[i]);
    rowSum = children[i].value;
    let bestRatio = worstRatio(row, rowSum, scale, side);
    i++;

    // Greedily add more while aspect ratio improves
    while (i < children.length) {
      const candidate = children[i];
      const newSum = rowSum + candidate.value;
      row.push(candidate);
      const newRatio = worstRatio(row, newSum, scale, side);
      if (newRatio > bestRatio) {
        row.pop();
        break;
      }
      rowSum = newSum;
      bestRatio = newRatio;
      i++;
    }

    // Layout this row
    const rowPixels = (rowSum * scale) / side;
    let offset = 0;

    for (const node of row) {
      const nodePixels = (node.value * scale) / rowPixels;
      if (isWide) {
        node.x = remaining.x;
        node.y = remaining.y + offset;
        node.w = rowPixels;
        node.h = nodePixels;
      } else {
        node.x = remaining.x + offset;
        node.y = remaining.y;
        node.w = nodePixels;
        node.h = rowPixels;
      }
      offset += nodePixels;

      // Recurse
      if (node.children.length > 0) {
        const pad = Math.min(2, node.w * 0.02, node.h * 0.02);
        const inner: Rect = {
          x: node.x + pad,
          y: node.y + pad,
          w: Math.max(0, node.w - pad * 2),
          h: Math.max(0, node.h - pad * 2),
        };
        if (inner.w > 0 && inner.h > 0) {
          squarify(node.children, node.value, inner);
        }
      }
    }

    // Shrink remaining area
    if (isWide) {
      remaining = {
        x: remaining.x + rowPixels,
        y: remaining.y,
        w: remaining.w - rowPixels,
        h: remaining.h,
      };
    } else {
      remaining = {
        x: remaining.x,
        y: remaining.y + rowPixels,
        w: remaining.w,
        h: remaining.h - rowPixels,
      };
    }
  }
}

function worstRatio(row: TreemapNode[], sum: number, scale: number, side: number): number {
  if (sum === 0 || side === 0) return Infinity;
  const rowArea = sum * scale;
  const rowWidth = rowArea / side;
  if (rowWidth === 0) return Infinity;

  let worst = 0;
  for (const node of row) {
    const nodeArea = node.value * scale;
    const nodeLen = nodeArea / rowWidth;
    const ratio = rowWidth > nodeLen ? rowWidth / nodeLen : nodeLen / rowWidth;
    if (ratio > worst) worst = ratio;
  }
  return worst;
}
