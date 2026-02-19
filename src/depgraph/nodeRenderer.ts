import * as THREE from 'three';
import { getCategoryColorHex } from '../colors';
import type { GraphNode } from './types';

export interface InstancedMeshEntry {
  mesh: THREE.InstancedMesh;
  indices: number[];
  group: string;
  baseMat: THREE.MeshStandardMaterial;
  glowArray: Float32Array;
}

export interface NodeSystem {
  instancedMeshes: InstancedMeshEntry[];
  nodeToInstance: Map<number, { entry: InstancedMeshEntry; instanceIdx: number }>;
  groups: Map<string, number[]>;
  dummy: THREE.Object3D;
  updateInstances(): void;
  dispose(): void;
}

export function createNodeSystem(scene: THREE.Scene, nodes: GraphNode[]): NodeSystem {
  // Scale geometry detail with node count — 384 tris/node is too much for 1000+ nodes
  const segs = nodes.length > 500 ? 8 : nodes.length > 200 ? 12 : 24;
  const rings = nodes.length > 500 ? 6 : nodes.length > 200 ? 8 : 16;
  const sphereGeo = new THREE.SphereGeometry(1, segs, rings);
  const groups = new Map<string, number[]>();
  for (let i = 0; i < nodes.length; i++) {
    let arr = groups.get(nodes[i].group);
    if (!arr) { arr = []; groups.set(nodes[i].group, arr); }
    arr.push(i);
  }

  const instancedMeshes: InstancedMeshEntry[] = [];
  const dummy = new THREE.Object3D();

  for (const [group, indices] of groups) {
    const color = getCategoryColorHex(group);
    const mat = new THREE.MeshStandardMaterial({
      color,
      emissive: color,
      emissiveIntensity: 0.4,
      roughness: 0.5,
      metalness: 0.15,
    });
    mat.onBeforeCompile = (shader) => {
      shader.vertexShader = shader.vertexShader
        .replace('#include <common>', 'attribute float instanceGlow;\nvarying float vGlow;\n#include <common>')
        .replace('#include <begin_vertex>', '#include <begin_vertex>\nvGlow = instanceGlow;');
      shader.fragmentShader = shader.fragmentShader
        .replace('#include <common>', 'varying float vGlow;\n#include <common>')
        .replace('#include <emissivemap_fragment>', '#include <emissivemap_fragment>\ntotalEmissiveRadiance *= (1.0 + vGlow * 3.0);');
    };
    const mesh = new THREE.InstancedMesh(sphereGeo, mat, indices.length);
    const glowArray = new Float32Array(indices.length);
    mesh.geometry = sphereGeo.clone();
    mesh.geometry.setAttribute('instanceGlow', new THREE.InstancedBufferAttribute(glowArray, 1));
    mesh.userData = { group, indices };
    scene.add(mesh);
    instancedMeshes.push({ mesh, indices, group, baseMat: mat, glowArray });
  }

  const nodeToInstance = new Map<number, { entry: InstancedMeshEntry; instanceIdx: number }>();
  for (const entry of instancedMeshes) {
    for (let ii = 0; ii < entry.indices.length; ii++) {
      nodeToInstance.set(entry.indices[ii], { entry, instanceIdx: ii });
    }
  }

  function updateInstances() {
    for (const { mesh, indices } of instancedMeshes) {
      for (let ii = 0; ii < indices.length; ii++) {
        const node = nodes[indices[ii]];
        dummy.position.set(node.x, node.y, node.z);
        const s = node.radius;
        dummy.scale.set(s, s, s);
        dummy.updateMatrix();
        mesh.setMatrixAt(ii, dummy.matrix);
      }
      mesh.instanceMatrix.needsUpdate = true;
    }
  }

  function dispose() {
    sphereGeo.dispose();
    for (const { mesh, baseMat } of instancedMeshes) {
      mesh.geometry.dispose();
      mesh.dispose();
      baseMat.dispose();
    }
  }

  updateInstances();

  return { instancedMeshes, nodeToInstance, groups, dummy, updateInstances, dispose };
}
