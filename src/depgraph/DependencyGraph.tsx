import { useRef, useMemo, useEffect, useState } from 'react';
import type { DepGraph, Manifest } from '../types';
import type { DepGraphTooltip } from './types';
import { buildGraphData, findModuleByCategory, buildDepTree, buildMultiInspect } from './analysis';
import { createScene, disposeScene } from './sceneSetup';
import { createNodeSystem } from './nodeRenderer';
import { createEdgeSystem } from './edgeRenderer';
import { createNebulaSystem } from './nebulaEffects';
import { createInteraction } from './interaction';
import { computeHighlightState, computeEdgeAlphaTargets, lerpEdgeAlphas, applyNodeHighlights } from './highlights';
import '../styles/depgraph.css';
import { initPositions, tick } from './simulation';
import InspectPanel from './InspectPanel';
import MultiInspectPanel from './MultiInspectPanel';

interface Props {
  deps: DepGraph;
  activeCategory: string | null;
  searchTerm: string;
  globalSearch: string | null;
  selected: Set<string>;
  manifest: Manifest;
  onNavigateModule: (id: string) => void;
  onSelectModule: (moduleId: string) => void;
}

export default function DependencyGraph({ deps, activeCategory, searchTerm, globalSearch, selected, manifest, onNavigateModule, onSelectModule }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [tooltip, setTooltip] = useState<DepGraphTooltip | null>(null);
  const [selectedNode, setSelectedNode] = useState<string | null>(null);
  const [inspectDepth, setInspectDepth] = useState(2);

  // Build a reverse lookup: categoryPath → dep module name(s)
  const catToMods = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const [mod, entry] of Object.entries(deps)) {
      const cp = entry.categoryPath;
      if (!cp) continue;
      let arr = map.get(cp);
      if (!arr) { arr = []; map.set(cp, arr); }
      arr.push(mod);
    }
    return map;
  }, [deps]);

  const selectedModules = useMemo(() => {
    if (selected.size === 0) return new Set<string>();
    const mods = new Set<string>();
    for (const [catPath, files] of Object.entries(manifest)) {
      const hasSelected = files.some(f => selected.has(f.path));
      if (!hasSelected) continue;
      // Use reverse lookup (fast) instead of scanning all deps
      const mapped = catToMods.get(catPath);
      if (mapped) {
        for (const m of mapped) mods.add(m);
      }
      // Also try last segment as direct dep key
      const lastSeg = catPath.split(' > ').pop() || '';
      if (lastSeg && deps[lastSeg]) mods.add(lastSeg);
    }
    return mods;
  }, [selected, manifest, deps, catToMods]);

  const propsRef = useRef({ activeCategory, deps, onNavigateModule, onSelectModule, searchTerm, globalSearch, selectedModules });
  propsRef.current = { activeCategory, deps, onNavigateModule, onSelectModule, searchTerm, globalSearch, selectedModules };
  const resetCameraRef = useRef<() => void>(() => {});
  const flyToNodeRef = useRef<(nodeId: string) => void>(() => {});

  const graphData = useMemo(() => buildGraphData(deps), [deps]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const { nodes, edges, nodeMap, adjacency } = graphData;
    let cancelled = false;
    let cleanupFn: (() => void) | null = null;

    // Defer init to next frame so the browser resolves the grid layout first
    const raf = requestAnimationFrame(() => {
      if (cancelled) return;

      const rect = container.getBoundingClientRect();
      const simSize = { width: rect.width, height: rect.height };

      const { context, resizeObserver } = createScene(container, simSize);
      const { renderer, scene, camera, composer, controls, labelRenderer } = context;

      // Simulation warm-up — scale with graph size to avoid UI blocking
      initPositions(nodes, simSize.width, simSize.height);
      const n = nodes.length;
      const maxTicks = n > 500 ? 300 : n > 200 ? 500 : 800;
      const warmupTicks = n > 500 ? 60 : n > 200 ? 100 : 200;
      for (let i = 0; i < warmupTicks; i++) {
        tick(nodes, edges, nodeMap, simSize.width, simSize.height, 1 - i / maxTicks);
      }
      let tickCount = warmupTicks;
      let simRunning = true;

      // Subsystems
      const nodeSys = createNodeSystem(scene, nodes);
      const edgeSys = createEdgeSystem(scene, edges, nodes, nodeMap);
      const nebulaSys = createNebulaSystem(scene, nodes, nodeSys.groups);
      const interaction = createInteraction(
        renderer, camera, controls,
        nodeSys.instancedMeshes, nodes, nodeMap,
        { onTooltip: setTooltip, onSelectNode: setSelectedNode, getProps: () => propsRef.current },
      );
      resetCameraRef.current = interaction.resetCamera;
      flyToNodeRef.current = interaction.flyToNode;

      // Animation loop — dirty-flag highlights to avoid per-frame O(n) scans
      let nebulaUpdateTimer = 0;
      let prevHlKey = '';
      let cachedHlState: ReturnType<typeof computeHighlightState> | null = null;

      renderer.setAnimationLoop(() => {
        if (cancelled) return;
        const time = performance.now() * 0.001;

        if (simRunning) {
          tick(nodes, edges, nodeMap, simSize.width, simSize.height, Math.max(0.01, 1 - tickCount / maxTicks));
          tickCount++;
          nodeSys.updateInstances();
          edgeSys.updateEdgePositions();
          nebulaUpdateTimer++;
          if (nebulaUpdateTimer % 30 === 0) nebulaSys.updateNebulas();
          if (tickCount > maxTicks) simRunning = false;
        }

        // Camera fly-to
        const ft = interaction.state.flyTo;
        if (ft?.active) {
          ft.t = Math.min(1, ft.t + 0.025);
          const ease = 1 - Math.pow(1 - ft.t, 3);
          camera.position.lerpVectors(ft.from, ft.to, ease);
          controls.target.lerpVectors(ft.lookFrom, ft.lookTo, ease);
          if (ft.t >= 1) ft.active = false;
        }

        // Highlights — only recompute when inputs change
        const p = propsRef.current;
        const hlKey = `${p.activeCategory}|${p.searchTerm}|${p.globalSearch}|${[...p.selectedModules].sort().join(',')}|${interaction.state.hoveredNodeIdx}`;
        if (hlKey !== prevHlKey || !cachedHlState) {
          prevHlKey = hlKey;
          cachedHlState = computeHighlightState({
            activeCategory: p.activeCategory,
            deps: p.deps,
            searchTerm: p.searchTerm,
            globalSearch: p.globalSearch,
            selectedModules: p.selectedModules,
            hoveredNodeIdx: interaction.state.hoveredNodeIdx,
          }, nodes, nodeMap, adjacency);
          computeEdgeAlphaTargets(cachedHlState, edges, nodeMap, adjacency, edgeSys.edgeAlphaTargets);
        }
        lerpEdgeAlphas(edgeSys.edgeAlphas, edgeSys.edgeAlphaTargets);
        edgeSys.edgeGeo.attributes.alpha.needsUpdate = true;
        applyNodeHighlights(cachedHlState, nodeSys.instancedMeshes, nodes, nodeSys.dummy, interaction.state.clickPulseNodeIdx, interaction.state.clickPulseT, simRunning, time);

        if (interaction.state.clickPulseT < 1) {
          interaction.state.clickPulseT += 0.04;
        }

        if (!simRunning) nebulaSys.driftNebulas(time);

        controls.update();
        composer.render();
        labelRenderer.render(scene, camera);
      });

      cleanupFn = () => {
        interaction.dispose();
        nodeSys.dispose();
        edgeSys.dispose();
        nebulaSys.dispose();
        disposeScene(context, container, resizeObserver);
      };
    });

    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
      cleanupFn?.();
    };
  }, [graphData]);

  const activeModName = activeCategory ? findModuleByCategory(activeCategory, deps) : null;
  const connectedCount = activeModName ? (graphData.adjacency.get(activeModName)?.size || 0) : 0;
  const filterTerm = searchTerm || globalSearch || '';
  const searchMatchCount = filterTerm ? graphData.nodes.filter(n => n.id.toLowerCase().includes(filterTerm.toLowerCase()) || n.categoryPath.toLowerCase().includes(filterTerm.toLowerCase())).length : 0;

  const effectiveSelectedNode = selectedModules.size === 1 && !selectedNode
    ? [...selectedModules][0]
    : selectedNode;

  const depTree = useMemo(() => {
    if (!effectiveSelectedNode) return null;
    return buildDepTree(effectiveSelectedNode, deps, graphData.adjacency, graphData.nodes, graphData.nodeMap, inspectDepth);
  }, [effectiveSelectedNode, deps, graphData, inspectDepth]);

  const multiInspect = useMemo(() => {
    const modIds = [...selectedModules];
    if (modIds.length < 2) return null;
    return buildMultiInspect(modIds, deps, graphData.nodes, graphData.nodeMap, manifest);
  }, [selectedModules, deps, graphData, manifest]);

  const selectedEntry = effectiveSelectedNode ? deps[effectiveSelectedNode] : null;
  const selectedNodeData = effectiveSelectedNode ? graphData.nodes[graphData.nodeMap.get(effectiveSelectedNode)!] : null;

  const handleInspectNode = (nodeId: string) => {
    flyToNodeRef.current(nodeId);
    const entry = deps[nodeId];
    if (entry?.categoryPath) {
      onNavigateModule(entry.categoryPath);
    }
  };

  return (
    <div className="depgraph">
      <div className="depgraph-toolbar">
        <span className="depgraph-toolbar-title">3D Dependency Graph</span>
        {activeModName && (
          <span className="depgraph-active">
            <span className="depgraph-active-dot" />
            {activeModName}
            <span className="depgraph-active-count">
              {connectedCount} deps
            </span>
          </span>
        )}
        {filterTerm && !activeModName && (
          <span className="depgraph-search-info">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
            </svg>
            {searchMatchCount} match{searchMatchCount !== 1 ? 'es' : ''} for "{filterTerm}"
          </span>
        )}
        <button className="depgraph-reset-btn" onClick={() => resetCameraRef.current()} title="Reset view">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/>
          </svg>
        </button>
        <div className="depgraph-legend">
          <span className="depgraph-legend-item">
            <span className="depgraph-legend-line solid" /> public
          </span>
          <span className="depgraph-legend-item">
            <span className="depgraph-legend-line dashed" /> private
          </span>
        </div>
      </div>
      <div className="codemap-canvas-wrap" ref={containerRef} />

      {tooltip && !selectedNode && (
        <div
          className="codemap-tooltip"
          style={{
            left: Math.min(tooltip.x + 12, window.innerWidth - 220),
            top: tooltip.y + 16,
          }}
        >
          <div className="codemap-tooltip-name">{tooltip.name}</div>
          <div className="codemap-tooltip-id">{tooltip.category}</div>
          <div className="codemap-tooltip-count">{tooltip.depCount} dependencies</div>
        </div>
      )}

      {multiInspect && (
        <MultiInspectPanel
          data={multiInspect}
          onInspectNode={handleInspectNode}
          onClose={() => setSelectedNode(null)}
        />
      )}

      {!multiInspect && effectiveSelectedNode && selectedNodeData && depTree && (
        <InspectPanel
          selectedNode={effectiveSelectedNode}
          nodeData={selectedNodeData}
          selectedEntry={selectedEntry}
          depTree={depTree}
          inspectDepth={inspectDepth}
          onSetDepth={setInspectDepth}
          onInspectNode={handleInspectNode}
          onClose={() => setSelectedNode(null)}
        />
      )}
    </div>
  );
}
