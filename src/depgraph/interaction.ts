import * as THREE from 'three';
import type { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import type { DepGraph } from '../types';
import type { GraphNode } from './types';
import type { InstancedMeshEntry } from './nodeRenderer';

export interface FlyToState {
  from: THREE.Vector3;
  to: THREE.Vector3;
  lookFrom: THREE.Vector3;
  lookTo: THREE.Vector3;
  t: number;
  active: boolean;
}

export interface InteractionState {
  hoveredNodeIdx: number | null;
  flyTo: FlyToState | null;
  clickPulseNodeIdx: number | null;
  clickPulseT: number;
}

interface InteractionCallbacks {
  onTooltip: (tooltip: { x: number; y: number; name: string; category: string; depCount: number } | null) => void;
  onSelectNode: (nodeId: string | null) => void;
  getProps: () => { deps: DepGraph; onNavigateModule: (id: string) => void; onSelectModule: (moduleId: string) => void };
}

export function createInteraction(
  renderer: THREE.WebGLRenderer,
  camera: THREE.PerspectiveCamera,
  controls: OrbitControls,
  instancedMeshes: InstancedMeshEntry[],
  nodes: GraphNode[],
  nodeMap: Map<string, number>,
  callbacks: InteractionCallbacks,
): {
  state: InteractionState;
  flyToNode: (nodeId: string) => void;
  resetCamera: () => void;
  dispose: () => void;
} {
  const state: InteractionState = {
    hoveredNodeIdx: null,
    flyTo: null,
    clickPulseNodeIdx: null,
    clickPulseT: 1,
  };

  const raycaster = new THREE.Raycaster();
  const mouse = new THREE.Vector2();
  let dragMoved = false;
  let mouseDown = false;

  function findClosestNode(): number | null {
    let closest: { nodeIdx: number; dist: number } | null = null;
    for (const { mesh, indices } of instancedMeshes) {
      const intersects = raycaster.intersectObject(mesh);
      if (intersects.length > 0 && intersects[0].instanceId !== undefined) {
        const nodeIdx = indices[intersects[0].instanceId];
        const dist = intersects[0].distance;
        if (!closest || dist < closest.dist) {
          closest = { nodeIdx, dist };
        }
      }
    }
    return closest?.nodeIdx ?? null;
  }

  function startFlyTo(nodePos: THREE.Vector3, radius: number) {
    const dir = new THREE.Vector3().subVectors(camera.position, controls.target).normalize();
    const targetCamPos = nodePos.clone().add(dir.multiplyScalar(Math.max(100, radius * 30)));
    state.flyTo = {
      from: camera.position.clone(),
      to: targetCamPos,
      lookFrom: controls.target.clone(),
      lookTo: nodePos.clone(),
      t: 0,
      active: true,
    };
  }

  const onMouseMove = (e: MouseEvent) => {
    const r = renderer.domElement.getBoundingClientRect();
    mouse.x = ((e.clientX - r.left) / r.width) * 2 - 1;
    mouse.y = -((e.clientY - r.top) / r.height) * 2 + 1;

    if (mouseDown) {
      dragMoved = true;
      callbacks.onTooltip(null);
      return;
    }

    raycaster.setFromCamera(mouse, camera);
    const found = findClosestNode();

    if (found !== null) {
      state.hoveredNodeIdx = found;
      renderer.domElement.style.cursor = 'pointer';
      const node = nodes[found];
      callbacks.onTooltip({
        x: e.clientX,
        y: e.clientY,
        name: node.id,
        category: node.categoryPath,
        depCount: node.depCount,
      });
    } else {
      state.hoveredNodeIdx = null;
      renderer.domElement.style.cursor = 'grab';
      callbacks.onTooltip(null);
    }
  };

  const onMouseDown = () => { mouseDown = true; dragMoved = false; renderer.domElement.style.cursor = 'grabbing'; };
  const onMouseUp = () => { mouseDown = false; };

  const onClick = (e: MouseEvent) => {
    if (dragMoved) return;
    const r = renderer.domElement.getBoundingClientRect();
    mouse.x = ((e.clientX - r.left) / r.width) * 2 - 1;
    mouse.y = -((e.clientY - r.top) / r.height) * 2 + 1;
    raycaster.setFromCamera(mouse, camera);

    const found = findClosestNode();
    if (found !== null) {
      const node = nodes[found];
      state.clickPulseNodeIdx = found;
      state.clickPulseT = 0;
      startFlyTo(new THREE.Vector3(node.x, node.y, node.z), node.radius);
      callbacks.onSelectNode(node.id);

      // Select this module + its dependency neighborhood
      const props = callbacks.getProps();
      props.onSelectModule(node.id);
    } else {
      callbacks.onSelectNode(null);
    }
  };

  renderer.domElement.addEventListener('mousemove', onMouseMove);
  renderer.domElement.addEventListener('mousedown', onMouseDown);
  window.addEventListener('mouseup', onMouseUp);
  renderer.domElement.addEventListener('click', onClick);

  function flyToNode(nodeId: string) {
    const ni = nodeMap.get(nodeId);
    if (ni === undefined) return;
    const node = nodes[ni];
    state.clickPulseNodeIdx = ni;
    state.clickPulseT = 0;
    startFlyTo(new THREE.Vector3(node.x, node.y, node.z), node.radius);
    callbacks.onSelectNode(nodeId);
  }

  function resetCamera() {
    const box = new THREE.Box3();
    for (const n of nodes) box.expandByPoint(new THREE.Vector3(n.x, n.y, n.z));
    const sphere = new THREE.Sphere();
    box.getBoundingSphere(sphere);
    const dist = sphere.radius / Math.tan((camera.fov * Math.PI / 180) / 2) * 1.2;
    state.flyTo = {
      from: camera.position.clone(),
      to: new THREE.Vector3(0, 0, dist),
      lookFrom: controls.target.clone(),
      lookTo: sphere.center.clone(),
      t: 0,
      active: true,
    };
  }

  function dispose() {
    renderer.domElement.removeEventListener('mousemove', onMouseMove);
    renderer.domElement.removeEventListener('mousedown', onMouseDown);
    window.removeEventListener('mouseup', onMouseUp);
    renderer.domElement.removeEventListener('click', onClick);
  }

  return { state, flyToNode, resetCamera, dispose };
}
