import * as THREE from 'three';
import type { GraphNode, GraphEdge } from './types';

export interface EdgeSystem {
  edgeGeo: THREE.BufferGeometry;
  edgeMat: THREE.ShaderMaterial;
  edgeAlphas: Float32Array;
  edgeAlphaTargets: Float32Array;
  updateEdgePositions(): void;
  dispose(): void;
}

export function createEdgeSystem(
  scene: THREE.Scene,
  edges: GraphEdge[],
  nodes: GraphNode[],
  nodeMap: Map<string, number>,
): EdgeSystem {
  const edgePositions = new Float32Array(edges.length * 6);
  const edgeColors = new Float32Array(edges.length * 6);
  const edgeAlphas = new Float32Array(edges.length * 2);
  const edgeAlphaTargets = new Float32Array(edges.length * 2);
  edgeAlphas.fill(0.12);
  edgeAlphaTargets.fill(0.12);

  const edgeGeo = new THREE.BufferGeometry();
  edgeGeo.setAttribute('position', new THREE.BufferAttribute(edgePositions, 3));
  edgeGeo.setAttribute('color', new THREE.BufferAttribute(edgeColors, 3));
  edgeGeo.setAttribute('alpha', new THREE.BufferAttribute(edgeAlphas, 1));

  const publicColor = new THREE.Color(0x89b4fa);
  const privateColor = new THREE.Color(0xcba6f7);
  for (let i = 0; i < edges.length; i++) {
    const c = edges[i].type === 'public' ? publicColor : privateColor;
    edgeColors[i * 6] = c.r; edgeColors[i * 6 + 1] = c.g; edgeColors[i * 6 + 2] = c.b;
    edgeColors[i * 6 + 3] = c.r; edgeColors[i * 6 + 4] = c.g; edgeColors[i * 6 + 5] = c.b;
  }
  edgeGeo.attributes.color.needsUpdate = true;

  const edgeMat = new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    blending: THREE.AdditiveBlending,
    vertexShader: `
      attribute vec3 color;
      attribute float alpha;
      varying vec3 vColor;
      varying float vAlpha;
      void main() {
        vColor = color;
        vAlpha = alpha;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: `
      varying vec3 vColor;
      varying float vAlpha;
      void main() {
        gl_FragColor = vec4(vColor, vAlpha);
      }
    `,
  });

  const edgeLines = new THREE.LineSegments(edgeGeo, edgeMat);
  scene.add(edgeLines);

  function updateEdgePositions() {
    for (let i = 0; i < edges.length; i++) {
      const si = nodeMap.get(edges[i].source);
      const ti = nodeMap.get(edges[i].target);
      if (si === undefined || ti === undefined) continue;
      const a = nodes[si], b = nodes[ti];
      edgePositions[i * 6] = a.x; edgePositions[i * 6 + 1] = a.y; edgePositions[i * 6 + 2] = a.z;
      edgePositions[i * 6 + 3] = b.x; edgePositions[i * 6 + 4] = b.y; edgePositions[i * 6 + 5] = b.z;
    }
    edgeGeo.attributes.position.needsUpdate = true;
  }

  function dispose() {
    edgeGeo.dispose();
    edgeMat.dispose();
  }

  updateEdgePositions();

  return { edgeGeo, edgeMat, edgeAlphas, edgeAlphaTargets, updateEdgePositions, dispose };
}
