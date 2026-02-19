import { getCategoryColor } from '../colors';
import type { DepGraph, Manifest } from '../types';
import type { GraphNode, GraphEdge, GraphData, DepNode, DepLevel, DepTree, MultiConnection, MultiInspectData } from './types';

/** Sqrt for small counts, transitions to log for large counts to prevent hub nodes
 *  from visually overwhelming everything else. Crossover at ~25 connections. */
function dampenRadius(count: number): number {
  if (count <= 25) return Math.sqrt(count) * 0.8;
  // log scaling above threshold, continuous at crossover point
  return Math.sqrt(25) * 0.8 + Math.log2(count / 25) * 1.5;
}

export function buildGraphData(deps: DepGraph): GraphData {
  const nodes: GraphNode[] = [];
  const edges: GraphEdge[] = [];
  const nodeMap = new Map<string, number>();
  const adjacency = new Map<string, Set<string>>();
  const depCounts = new Map<string, number>();

  for (const [mod, entry] of Object.entries(deps)) {
    const total = entry.public.length + entry.private.length;
    depCounts.set(mod, (depCounts.get(mod) || 0) + total);
    for (const dep of [...entry.public, ...entry.private]) {
      depCounts.set(dep, (depCounts.get(dep) || 0) + 1);
    }
  }

  const allModules = new Set<string>();
  for (const mod of Object.keys(deps)) allModules.add(mod);
  for (const entry of Object.values(deps)) {
    for (const d of [...entry.public, ...entry.private]) allModules.add(d);
  }

  for (const mod of allModules) {
    const entry = deps[mod];
    const count = depCounts.get(mod) || 0;
    nodeMap.set(mod, nodes.length);
    adjacency.set(mod, new Set());
    const catPath = entry?.categoryPath || '';
    const group = catPath.split(' > ')[0] || 'Other';
    nodes.push({
      id: mod,
      x: 0, y: 0, z: 0, vx: 0, vy: 0, vz: 0,
      radius: Math.max(1.5, dampenRadius(count)),
      color: getCategoryColor(group),
      depCount: count,
      categoryPath: catPath,
      group,
    });
  }

  for (const [mod, entry] of Object.entries(deps)) {
    for (const dep of entry.public) {
      if (allModules.has(dep)) {
        edges.push({ source: mod, target: dep, type: 'public' });
        adjacency.get(mod)!.add(dep);
        adjacency.get(dep)!.add(mod);
      }
    }
    for (const dep of entry.private) {
      if (allModules.has(dep)) {
        edges.push({ source: mod, target: dep, type: 'private' });
        adjacency.get(mod)!.add(dep);
        adjacency.get(dep)!.add(mod);
      }
    }
  }

  return { nodes, edges, nodeMap, adjacency };
}

export function findModuleByCategory(category: string, deps: DepGraph): string | null {
  const lastSeg = category.split(' > ').pop() || '';
  if (deps[lastSeg]) return lastSeg;
  for (const [mod, entry] of Object.entries(deps)) {
    if (entry.categoryPath === category) return mod;
  }
  return null;
}

export function buildDepTree(
  rootId: string,
  deps: DepGraph,
  adjacency: Map<string, Set<string>>,
  allNodes: GraphNode[],
  nodeMap: Map<string, number>,
  maxDepth: number,
): DepTree {
  const dependsOn: DepLevel[] = [];
  const dependedBy: DepLevel[] = [];

  // Forward: modules this one depends on
  const visitedFwd = new Set<string>([rootId]);
  let frontier = [rootId];
  for (let depth = 1; depth <= maxDepth && frontier.length > 0; depth++) {
    const level: DepNode[] = [];
    const nextFrontier: string[] = [];
    for (const mod of frontier) {
      const entry = deps[mod];
      if (!entry) continue;
      for (const dep of entry.public) {
        if (!visitedFwd.has(dep) && nodeMap.has(dep)) {
          visitedFwd.add(dep);
          const ni = nodeMap.get(dep)!;
          const n = allNodes[ni];
          level.push({ id: dep, group: n.group, categoryPath: n.categoryPath, depCount: n.depCount, type: 'public', direction: 'depends-on' });
          nextFrontier.push(dep);
        }
      }
      for (const dep of entry.private) {
        if (!visitedFwd.has(dep) && nodeMap.has(dep)) {
          visitedFwd.add(dep);
          const ni = nodeMap.get(dep)!;
          const n = allNodes[ni];
          level.push({ id: dep, group: n.group, categoryPath: n.categoryPath, depCount: n.depCount, type: 'private', direction: 'depends-on' });
          nextFrontier.push(dep);
        }
      }
    }
    if (level.length > 0) dependsOn.push({ depth, nodes: level });
    frontier = nextFrontier;
  }

  // Reverse: modules that depend on this one
  const visitedRev = new Set<string>([rootId]);
  frontier = [rootId];
  for (let depth = 1; depth <= maxDepth && frontier.length > 0; depth++) {
    const level: DepNode[] = [];
    const nextFrontier: string[] = [];
    for (const mod of frontier) {
      for (const [other, entry] of Object.entries(deps)) {
        if (visitedRev.has(other)) continue;
        let type: 'public' | 'private' | null = null;
        if (entry.public.includes(mod)) type = 'public';
        else if (entry.private.includes(mod)) type = 'private';
        if (type && nodeMap.has(other)) {
          visitedRev.add(other);
          const ni = nodeMap.get(other)!;
          const n = allNodes[ni];
          level.push({ id: other, group: n.group, categoryPath: n.categoryPath, depCount: n.depCount, type, direction: 'depended-by' });
          nextFrontier.push(other);
        }
      }
    }
    if (level.length > 0) dependedBy.push({ depth, nodes: level });
    frontier = nextFrontier;
  }

  return { dependsOn, dependedBy };
}

export function findShortestPath(
  from: string,
  to: string,
  deps: DepGraph,
  nodeMap: Map<string, number>,
): string[] | null {
  if (from === to) return [from];
  const visited = new Set<string>([from]);
  const parent = new Map<string, string>();
  const queue = [from];
  let head = 0;
  while (head < queue.length) {
    const cur = queue[head++];
    const entry = deps[cur];
    if (!entry) continue;
    for (const dep of [...entry.public, ...entry.private]) {
      if (!nodeMap.has(dep) || visited.has(dep)) continue;
      visited.add(dep);
      parent.set(dep, cur);
      if (dep === to) {
        const path: string[] = [to];
        let p = to;
        while (parent.has(p)) { p = parent.get(p)!; path.unshift(p); }
        return path;
      }
      queue.push(dep);
    }
  }
  return null;
}

export function buildMultiInspect(
  moduleIds: string[],
  deps: DepGraph,
  nodes: GraphNode[],
  nodeMap: Map<string, number>,
  manifest: Manifest,
): MultiInspectData {
  const modules = moduleIds.map(id => {
    const ni = nodeMap.get(id);
    const n = ni !== undefined ? nodes[ni] : null;
    const entry = deps[id];
    let fileCount = 0;
    if (entry?.categoryPath) {
      fileCount = manifest[entry.categoryPath]?.length || 0;
    }
    return {
      id,
      group: n?.group || 'Other',
      categoryPath: n?.categoryPath || '',
      depCount: n?.depCount || 0,
      fileCount,
    };
  });

  const connections: MultiConnection[] = [];
  const modSet = new Set(moduleIds);
  for (let i = 0; i < moduleIds.length; i++) {
    for (let j = i + 1; j < moduleIds.length; j++) {
      const a = moduleIds[i], b = moduleIds[j];
      const entryA = deps[a];
      if (entryA?.public.includes(b)) {
        connections.push({ from: a, to: b, type: 'public', path: [a, b] });
      } else if (entryA?.private.includes(b)) {
        connections.push({ from: a, to: b, type: 'private', path: [a, b] });
      } else {
        const entryB = deps[b];
        if (entryB?.public.includes(a)) {
          connections.push({ from: b, to: a, type: 'public', path: [b, a] });
        } else if (entryB?.private.includes(a)) {
          connections.push({ from: b, to: a, type: 'private', path: [b, a] });
        } else {
          const pathAB = findShortestPath(a, b, deps, nodeMap);
          const pathBA = findShortestPath(b, a, deps, nodeMap);
          const best = !pathAB ? pathBA : !pathBA ? pathAB : pathAB.length <= pathBA.length ? pathAB : pathBA;
          if (best && best.length <= 6) {
            connections.push({ from: best[0], to: best[best.length - 1], type: 'indirect', path: best });
          }
        }
      }
    }
  }

  const depCounts = new Map<string, number>();
  for (const id of moduleIds) {
    const entry = deps[id];
    if (!entry) continue;
    const allDeps = new Set([...entry.public, ...entry.private]);
    for (const d of allDeps) {
      if (modSet.has(d)) continue;
      depCounts.set(d, (depCounts.get(d) || 0) + 1);
    }
  }
  const sharedDeps = [...depCounts.entries()]
    .filter(([, count]) => count >= 2)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 15)
    .map(([id, count]) => {
      const ni = nodeMap.get(id);
      const n = ni !== undefined ? nodes[ni] : null;
      return { id, group: n?.group || 'Other', dependedByCount: count };
    });

  return { modules, connections, sharedDeps };
}
