import type { TreeNode } from '../types';
import type { TreemapNode } from './types';
import { blendExtColors } from './colorUtils';
import { getExt } from '../utils';

export function buildTreemapData(tree: TreeNode): TreemapNode {
  let root = buildNode(tree, '', 'Project Root', 0);

  // If the root has a single directory child (no root-level files), unwrap it
  // so the treemap starts at the first meaningful level — matches sidebar behavior.
  while (
    root.children.length === 1 &&
    !root.children[0].id.startsWith('file:')
  ) {
    root = root.children[0];
    renumberDepth(root, 0);
  }

  // Dampen layout values so huge modules don't swallow small ones.
  // Applies log scaling to directory siblings at each level independently,
  // preventing one massive folder from dominating all visible space.
  dampenValues(root);

  return root;
}

/** Dampen directory children at each level to compress extreme size ratios.
 *  Only directory nodes get dampened (not individual files).
 *  Applied per-level so dampening doesn't compound across depths. */
function dampenValues(node: TreemapNode): void {
  if (node.children.length === 0) return;

  // Recurse first so inner levels get their own independent dampening
  for (const child of node.children) {
    dampenValues(child);
  }

  // Separate directory children from file leaves
  const dirs = node.children.filter(c => c.children.length > 0);
  if (dirs.length >= 2) {
    // Cube root dampening: compresses extreme ratios while preserving
    // relative ordering. 100K vs 100 files → ~10:1 area instead of 1000:1.
    for (const dir of dirs) {
      dir.value = Math.cbrt(dir.fileCount);
    }
  }

  // Recompute parent value from (dampened dirs + unchanged files)
  node.value = node.children.reduce((sum, c) => sum + c.value, 0);
  node.children.sort((a, b) => b.value - a.value);
}

function renumberDepth(node: TreemapNode, depth: number) {
  node.depth = depth;
  for (const child of node.children) {
    renumberDepth(child, depth + 1);
  }
}

function buildNode(node: TreeNode, id: string, name: string, depth: number): TreemapNode {
  const extBreakdown: Record<string, number> = {};
  let fileCount = 0;
  const children: TreemapNode[] = [];

  // Create leaf nodes for individual files
  const files = node._files as any[] | undefined;
  if (files) {
    for (const f of files) {
      const ext = getExt(f.path);
      extBreakdown[ext] = (extBreakdown[ext] || 0) + 1;
      fileCount++;
      const fname = f.path.split('/').pop() || f.path;
      children.push({
        id: `file:${f.path}`,
        name: fname,
        value: 1,
        fileCount: 1,
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

    if (childNode.fileCount > 0) {
      children.push(childNode);
      fileCount += childNode.fileCount;
      for (const [ext, count] of Object.entries(childNode.extBreakdown)) {
        extBreakdown[ext] = (extBreakdown[ext] || 0) + count;
      }
    }
  }

  // Sort children by file count descending for layout
  children.sort((a, b) => b.fileCount - a.fileCount);

  return {
    id,
    name,
    value: fileCount,  // will be overwritten by dampenValues()
    fileCount,
    children,
    extBreakdown,
    depth,
    color: blendExtColors(extBreakdown),
    x: 0, y: 0, w: 0, h: 0,
  };
}
