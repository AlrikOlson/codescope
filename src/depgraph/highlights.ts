import * as THREE from 'three';
import type { DepGraph } from '../types';
import type { GraphNode, GraphEdge } from './types';
import type { InstancedMeshEntry } from './nodeRenderer';
import { findModuleByCategory } from './analysis';

export interface HighlightInput {
  activeCategory: string | null;
  deps: DepGraph;
  searchTerm: string;
  globalSearch: string | null;
  selectedModules: Set<string>;
  hoveredNodeIdx: number | null;
}

export interface HighlightState {
  effectiveHighlight: boolean;
  highlightSet: Set<number>;
  hasFocus: boolean;
  hasSelection: boolean;
  hasSearch: boolean;
  focusNodeId: string | null;
}

export function computeHighlightState(
  input: HighlightInput,
  nodes: GraphNode[],
  nodeMap: Map<string, number>,
  adjacency: Map<string, Set<string>>,
): HighlightState {
  const activeModName = input.activeCategory ? findModuleByCategory(input.activeCategory, input.deps) : null;
  const activeNodeIdx = activeModName ? nodeMap.get(activeModName) : undefined;
  const connectedToActive = activeModName ? adjacency.get(activeModName) : undefined;
  const filterTerm = input.searchTerm || input.globalSearch || '';

  const searchMatchSet = new Set<number>();
  if (filterTerm) {
    const lower = filterTerm.toLowerCase();
    for (let i = 0; i < nodes.length; i++) {
      if (nodes[i].id.toLowerCase().includes(lower) || nodes[i].categoryPath.toLowerCase().includes(lower)) {
        searchMatchSet.add(i);
      }
    }
  }
  const hasSearch = filterTerm.length > 0;

  const hasFocus = activeNodeIdx !== undefined || input.hoveredNodeIdx !== null;
  const focusSet = new Set<number>();
  let focusNodeId: string | null = null;
  if (input.hoveredNodeIdx !== null) {
    focusSet.add(input.hoveredNodeIdx);
    focusNodeId = nodes[input.hoveredNodeIdx].id;
    const hConn = adjacency.get(focusNodeId);
    if (hConn) for (const c of hConn) { const ci = nodeMap.get(c); if (ci !== undefined) focusSet.add(ci); }
  } else if (activeNodeIdx !== undefined && activeModName) {
    focusSet.add(activeNodeIdx);
    focusNodeId = activeModName;
    if (connectedToActive) for (const c of connectedToActive) { const ci = nodeMap.get(c); if (ci !== undefined) focusSet.add(ci); }
  }

  const selModSet = new Set<number>();
  if (input.selectedModules.size > 0) {
    for (const mod of input.selectedModules) {
      const ni = nodeMap.get(mod);
      if (ni !== undefined) selModSet.add(ni);
    }
  }
  const hasSelection = selModSet.size > 0;

  const effectiveHighlight = hasFocus || hasSelection || hasSearch;
  const highlightSet = hasFocus ? focusSet : hasSelection ? selModSet : searchMatchSet;

  return { effectiveHighlight, highlightSet, hasFocus, hasSelection, hasSearch, focusNodeId };
}

export function computeEdgeAlphaTargets(
  state: HighlightState,
  edges: GraphEdge[],
  nodeMap: Map<string, number>,
  adjacency: Map<string, Set<string>>,
  targets: Float32Array,
): void {
  const focusConnected = state.focusNodeId ? adjacency.get(state.focusNodeId) : undefined;

  for (let i = 0; i < edges.length; i++) {
    let target: number;
    if (!state.effectiveHighlight) {
      target = 0.12;
    } else if (state.hasFocus && state.focusNodeId && (edges[i].source === state.focusNodeId || edges[i].target === state.focusNodeId)) {
      target = 0.5;
    } else if (state.hasFocus && focusConnected && (focusConnected.has(edges[i].source) || focusConnected.has(edges[i].target))) {
      target = 0.08;
    } else if (state.hasSelection && !state.hasFocus) {
      const si = nodeMap.get(edges[i].source);
      const ti = nodeMap.get(edges[i].target);
      if (si !== undefined && ti !== undefined && state.highlightSet.has(si) && state.highlightSet.has(ti)) {
        target = 0.55;
      } else if (si !== undefined && ti !== undefined && (state.highlightSet.has(si) || state.highlightSet.has(ti))) {
        target = 0.15;
      } else {
        target = 0.02;
      }
    } else if (state.hasSearch && !state.hasFocus && !state.hasSelection) {
      const si = nodeMap.get(edges[i].source);
      const ti = nodeMap.get(edges[i].target);
      if (si !== undefined && ti !== undefined && state.highlightSet.has(si) && state.highlightSet.has(ti)) {
        target = 0.35;
      } else {
        target = 0.02;
      }
    } else {
      target = 0.02;
    }
    targets[i * 2] = target;
    targets[i * 2 + 1] = target;
  }
}

export function lerpEdgeAlphas(alphas: Float32Array, targets: Float32Array): void {
  for (let i = 0; i < alphas.length; i++) {
    alphas[i] += (targets[i] - alphas[i]) * 0.12;
  }
}

export function applyNodeHighlights(
  state: HighlightState,
  instancedMeshes: InstancedMeshEntry[],
  nodes: GraphNode[],
  dummy: THREE.Object3D,
  clickPulseNodeIdx: number | null,
  clickPulseT: number,
  simRunning: boolean,
  time: number,
): void {
  for (const entry of instancedMeshes) {
    let instancesChanged = false;

    if (state.effectiveHighlight) {
      const shouldDim = !entry.indices.some(i => state.highlightSet.has(i));
      entry.baseMat.emissiveIntensity = shouldDim ? 0.05 : 0.5;
      entry.baseMat.opacity = shouldDim ? 0.3 : 1;
      entry.baseMat.transparent = shouldDim;

      for (let ii = 0; ii < entry.indices.length; ii++) {
        const ni = entry.indices[ii];
        const node = nodes[ni];
        const isHighlighted = state.highlightSet.has(ni);
        let s = node.radius * (isHighlighted ? 1.4 : 0.7);
        if (ni === clickPulseNodeIdx && clickPulseT < 1) {
          s *= 1 + 0.3 * Math.sin(clickPulseT * Math.PI);
        }
        entry.glowArray[ii] = isHighlighted ? 1.0 : 0.0;
        dummy.position.set(node.x, node.y, node.z);
        dummy.scale.set(s, s, s);
        dummy.updateMatrix();
        entry.mesh.setMatrixAt(ii, dummy.matrix);
      }
      instancesChanged = true;
    } else {
      entry.baseMat.emissiveIntensity = 0.4;
      entry.baseMat.opacity = 1;
      entry.baseMat.transparent = false;

      if (!simRunning) {
        for (let ii = 0; ii < entry.indices.length; ii++) {
          const ni = entry.indices[ii];
          const node = nodes[ni];
          const breath = 1 + 0.03 * Math.sin(time * 0.8 + ni * 0.5);
          let s = node.radius * breath;
          if (ni === clickPulseNodeIdx && clickPulseT < 1) {
            s *= 1 + 0.3 * Math.sin(clickPulseT * Math.PI);
          }
          entry.glowArray[ii] = 0.0;
          dummy.position.set(node.x, node.y, node.z);
          dummy.scale.set(s, s, s);
          dummy.updateMatrix();
          entry.mesh.setMatrixAt(ii, dummy.matrix);
        }
        instancesChanged = true;
      } else {
        entry.glowArray.fill(0);
      }
    }

    if (instancesChanged) {
      entry.mesh.instanceMatrix.needsUpdate = true;
      (entry.mesh.geometry.attributes.instanceGlow as THREE.InstancedBufferAttribute).needsUpdate = true;
    }
  }
}
