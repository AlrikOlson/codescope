import type { TreeNode } from '../types';
import type { TreemapNode } from './types';
import { blendExtColors } from './colorUtils';
import { getExt } from '../utils';

export function buildTreemapData(tree: TreeNode): TreemapNode {
  return buildNode(tree, '', 'Project Root', 0);
}

function buildNode(node: TreeNode, id: string, name: string, depth: number): TreemapNode {
  const extBreakdown: Record<string, number> = {};
  let value = 0;
  const children: TreemapNode[] = [];

  // Create leaf nodes for individual files
  const files = node._files as any[] | undefined;
  if (files) {
    for (const f of files) {
      const ext = getExt(f.path);
      extBreakdown[ext] = (extBreakdown[ext] || 0) + 1;
      value++;
      const fname = f.path.split('/').pop() || f.path;
      children.push({
        id: `file:${f.path}`,
        name: fname,
        value: 1,
        children: [],
        extBreakdown: { [ext]: 1 },
        depth: depth + 1,
        color: blendExtColors({ [ext]: 1 }),
        x: 0, y: 0, w: 0, h: 0,
      });
    }
  }

  // Recurse into child nodes
  for (const [key, child] of Object.entries(node)) {
    if (key === '_files') continue;
    if (typeof child !== 'object' || Array.isArray(child)) continue;

    const childId = id ? `${id} > ${key}` : key;
    const childNode = buildNode(child as TreeNode, childId, key, depth + 1);

    if (childNode.value > 0) {
      children.push(childNode);
      value += childNode.value;
      // Aggregate ext breakdown
      for (const [ext, count] of Object.entries(childNode.extBreakdown)) {
        extBreakdown[ext] = (extBreakdown[ext] || 0) + count;
      }
    }
  }

  // Sort children by value descending for layout
  children.sort((a, b) => b.value - a.value);

  return {
    id,
    name,
    value,
    children,
    extBreakdown,
    depth,
    color: blendExtColors(extBreakdown),
    x: 0, y: 0, w: 0, h: 0,
  };
}
