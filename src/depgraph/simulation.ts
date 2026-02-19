import type { GraphNode, GraphEdge } from './types';
import { CLUSTER_POSITIONS, getGroup } from './constants';

const REPULSION = 120;
const ATTRACTION = 0.004;
const DAMPING = 0.5;
const MAX_VELOCITY = 8;
const MAX_FORCE = 3;
const CELL_SIZE = 80;
const CLUSTER_GRAVITY = 0.015;
const GLOBAL_GRAVITY = 0.003;

export function initPositions(nodes: GraphNode[], _w: number, _h: number): void {
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    node.group = getGroup(node.categoryPath);
    const center = CLUSTER_POSITIONS[node.group] || [0, 0, 0];
    // Scatter around cluster center
    node.x = center[0] + (Math.random() - 0.5) * 200;
    node.y = center[1] + (Math.random() - 0.5) * 200;
    node.z = center[2] + (Math.random() - 0.5) * 200;
    node.vx = 0;
    node.vy = 0;
    node.vz = 0;
  }
}

// 3D spatial hash — integer keys avoid string allocation overhead
function hashKey(x: number, y: number, z: number): number {
  // Bijective pairing with signed coords: offset to positive, then combine
  const a = (x >= 0 ? x * 2 : -x * 2 - 1) | 0;
  const b = (y >= 0 ? y * 2 : -y * 2 - 1) | 0;
  const c = (z >= 0 ? z * 2 : -z * 2 - 1) | 0;
  // Szudzik pairing extended to 3D — unique for reasonable coordinate ranges
  const ab = a >= b ? a * a + a + b : b * b + a;
  return ab >= c ? ab * ab + ab + c : c * c + ab;
}

function buildGrid(nodes: GraphNode[], cellSize: number) {
  const grid = new Map<number, number[]>();
  for (let i = 0; i < nodes.length; i++) {
    const gx = Math.floor(nodes[i].x / cellSize);
    const gy = Math.floor(nodes[i].y / cellSize);
    const gz = Math.floor(nodes[i].z / cellSize);
    const key = hashKey(gx, gy, gz);
    let cell = grid.get(key);
    if (!cell) { cell = []; grid.set(key, cell); }
    cell.push(i);
  }
  return grid;
}

export function tick(
  nodes: GraphNode[],
  edges: GraphEdge[],
  nodeMap: Map<string, number>,
  _w: number,
  _h: number,
  alpha: number,
): void {
  const n = nodes.length;

  // Dampen
  for (const node of nodes) {
    node.vx *= DAMPING;
    node.vy *= DAMPING;
    node.vz *= DAMPING;
  }

  // Per-cluster gravity + global gravity toward origin
  for (let i = 0; i < n; i++) {
    const a = nodes[i];
    const center = CLUSTER_POSITIONS[a.group] || [0, 0, 0];
    // Cluster gravity
    a.vx += (center[0] - a.x) * CLUSTER_GRAVITY * alpha;
    a.vy += (center[1] - a.y) * CLUSTER_GRAVITY * alpha;
    a.vz += (center[2] - a.z) * CLUSTER_GRAVITY * alpha;
    // Global gravity
    a.vx += -a.x * GLOBAL_GRAVITY * alpha;
    a.vy += -a.y * GLOBAL_GRAVITY * alpha;
    a.vz += -a.z * GLOBAL_GRAVITY * alpha;
  }

  // Repulsion via 3D spatial grid — cell size scales with node count
  const cellSize = n > 500 ? 120 : CELL_SIZE;
  const cutoffSq = cellSize * cellSize * 4;
  const grid = buildGrid(nodes, cellSize);

  // Build a coord lookup so we can iterate neighbors by integer key
  const cellCoords: { key: number; gx: number; gy: number; gz: number }[] = [];
  for (let i = 0; i < n; i++) {
    const gx = Math.floor(nodes[i].x / cellSize);
    const gy = Math.floor(nodes[i].y / cellSize);
    const gz = Math.floor(nodes[i].z / cellSize);
    cellCoords.push({ key: hashKey(gx, gy, gz), gx, gy, gz });
  }

  // Deduplicate cells we've already visited
  const visited = new Set<number>();
  for (const { key, gx, gy, gz } of cellCoords) {
    if (visited.has(key)) continue;
    visited.add(key);
    const cell = grid.get(key);
    if (!cell) continue;

    for (let dx = -1; dx <= 1; dx++) {
      for (let dy = -1; dy <= 1; dy++) {
        for (let dz = -1; dz <= 1; dz++) {
          const nk = hashKey(gx + dx, gy + dy, gz + dz);
          const neighbor = grid.get(nk);
          if (!neighbor) continue;
          const isSelf = dx === 0 && dy === 0 && dz === 0;
          for (let ci = 0; ci < cell.length; ci++) {
            const i = cell[ci];
            const a = nodes[i];
            const list = isSelf ? cell : neighbor;
            const startJ = isSelf ? ci + 1 : 0;
            for (let ji = startJ; ji < list.length; ji++) {
              const j = list[ji];
              const b = nodes[j];
              const ddx = b.x - a.x;
              const ddy = b.y - a.y;
              const ddz = b.z - a.z;
              const distSq = ddx * ddx + ddy * ddy + ddz * ddz;
              if (distSq > cutoffSq) continue;
              const dist = Math.max(1, Math.sqrt(distSq));
              const force = Math.min(MAX_FORCE, REPULSION / (dist * dist)) * alpha;
              const fx = (ddx / dist) * force;
              const fy = (ddy / dist) * force;
              const fz = (ddz / dist) * force;
              a.vx -= fx; a.vy -= fy; a.vz -= fz;
              b.vx += fx; b.vy += fy; b.vz += fz;
            }
          }
        }
      }
    }
  }

  // Attraction along edges
  for (const edge of edges) {
    const si = nodeMap.get(edge.source);
    const ti = nodeMap.get(edge.target);
    if (si === undefined || ti === undefined) continue;
    const a = nodes[si];
    const b = nodes[ti];
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const dz = b.z - a.z;
    const dist = Math.max(1, Math.sqrt(dx * dx + dy * dy + dz * dz));
    const force = Math.min(MAX_FORCE, ATTRACTION * dist) * alpha;
    const fx = (dx / dist) * force;
    const fy = (dy / dist) * force;
    const fz = (dz / dist) * force;
    a.vx += fx; a.vy += fy; a.vz += fz;
    b.vx -= fx; b.vy -= fy; b.vz -= fz;
  }

  // Apply velocities with clamping
  const maxV = MAX_VELOCITY * alpha;
  for (const node of nodes) {
    const speed = Math.sqrt(node.vx * node.vx + node.vy * node.vy + node.vz * node.vz);
    if (speed > maxV) {
      const s = maxV / speed;
      node.vx *= s; node.vy *= s; node.vz *= s;
    }
    node.x += node.vx;
    node.y += node.vy;
    node.z += node.vz;
  }
}

// Compute cluster bounding spheres
export function computeClusterBounds(nodes: GraphNode[]): Map<string, { cx: number; cy: number; cz: number; r: number; count: number }> {
  const groups = new Map<string, GraphNode[]>();
  for (const n of nodes) {
    let arr = groups.get(n.group);
    if (!arr) { arr = []; groups.set(n.group, arr); }
    arr.push(n);
  }
  const result = new Map<string, { cx: number; cy: number; cz: number; r: number; count: number }>();
  for (const [group, gnodes] of groups) {
    let cx = 0, cy = 0, cz = 0;
    for (const n of gnodes) { cx += n.x; cy += n.y; cz += n.z; }
    cx /= gnodes.length; cy /= gnodes.length; cz /= gnodes.length;
    let maxR = 0;
    for (const n of gnodes) {
      const d = Math.sqrt((n.x - cx) ** 2 + (n.y - cy) ** 2 + (n.z - cz) ** 2);
      if (d > maxR) maxR = d;
    }
    result.set(group, { cx, cy, cz, r: maxR + 20, count: gnodes.length });
  }
  return result;
}
