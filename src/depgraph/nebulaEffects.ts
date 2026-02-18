import * as THREE from 'three';
import { CSS2DObject } from 'three/examples/jsm/renderers/CSS2DRenderer.js';
import { getCategoryColorHex } from '../colors';
import type { GraphNode } from './types';
import { computeClusterBounds } from './simulation';

interface NebulaEntry {
  sprites: THREE.Sprite[];
  group: string;
  basePositions: THREE.Vector3[];
}

export interface NebulaSystem {
  nebulaSprites: NebulaEntry[];
  updateNebulas(): void;
  driftNebulas(time: number): void;
  dispose(): void;
}

function createNebulaTexture(): THREE.Texture {
  const size = 128;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext('2d')!;
  const gradient = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  gradient.addColorStop(0, 'rgba(255,255,255,0.6)');
  gradient.addColorStop(0.3, 'rgba(255,255,255,0.15)');
  gradient.addColorStop(0.7, 'rgba(255,255,255,0.03)');
  gradient.addColorStop(1, 'rgba(255,255,255,0)');
  ctx.fillStyle = gradient;
  ctx.fillRect(0, 0, size, size);
  const tex = new THREE.CanvasTexture(canvas);
  tex.needsUpdate = true;
  return tex;
}

export function createNebulaSystem(
  scene: THREE.Scene,
  nodes: GraphNode[],
  groups: Map<string, number[]>,
): NebulaSystem {
  const nebulaTexture = createNebulaTexture();
  const nebulaSprites: NebulaEntry[] = [];

  for (const [group] of groups) {
    const color = getCategoryColorHex(group);
    const sprites: THREE.Sprite[] = [];
    const basePositions: THREE.Vector3[] = [];
    for (let i = 0; i < 4; i++) {
      const mat = new THREE.SpriteMaterial({
        map: nebulaTexture,
        color,
        transparent: true,
        opacity: 0.06 + Math.random() * 0.04,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
      });
      const sprite = new THREE.Sprite(mat);
      scene.add(sprite);
      sprites.push(sprite);
      basePositions.push(new THREE.Vector3());
    }
    nebulaSprites.push({ sprites, group, basePositions });
  }

  const clusterLabels = new Map<string, CSS2DObject>();
  for (const [group] of groups) {
    const div = document.createElement('div');
    div.textContent = group;
    div.className = 'cluster-label';
    const label = new CSS2DObject(div);
    scene.add(label);
    clusterLabels.set(group, label);
  }

  function updateNebulas() {
    const bounds = computeClusterBounds(nodes);
    for (const { sprites, group, basePositions } of nebulaSprites) {
      const b = bounds.get(group);
      if (!b) continue;
      const scale = Math.max(50, b.r * 1.8);
      for (let i = 0; i < sprites.length; i++) {
        const offset = i === 0 ? 0 : b.r * 0.25;
        const angle1 = Math.random() * Math.PI * 2;
        const angle2 = Math.random() * Math.PI * 2;
        const px = b.cx + (i === 0 ? 0 : Math.cos(angle1) * Math.sin(angle2) * offset);
        const py = b.cy + (i === 0 ? 0 : Math.sin(angle1) * Math.sin(angle2) * offset);
        const pz = b.cz + (i === 0 ? 0 : Math.cos(angle2) * offset);
        sprites[i].position.set(px, py, pz);
        basePositions[i].set(px, py, pz);
        const s = scale * (0.8 + Math.random() * 0.6);
        sprites[i].scale.set(s, s, 1);
      }
    }
    for (const [group, label] of clusterLabels) {
      const b = bounds.get(group);
      if (b) label.position.set(b.cx, b.cy + b.r * 0.6, b.cz);
    }
  }

  function driftNebulas(time: number) {
    for (const { sprites, basePositions } of nebulaSprites) {
      for (let i = 0; i < sprites.length; i++) {
        const bp = basePositions[i];
        sprites[i].position.set(
          bp.x + Math.sin(time * 0.3 + i * 1.7) * 3,
          bp.y + Math.cos(time * 0.2 + i * 2.3) * 3,
          bp.z + Math.sin(time * 0.25 + i * 0.9) * 3,
        );
        const mat = sprites[i].material as THREE.SpriteMaterial;
        mat.opacity = 0.06 + 0.02 * Math.sin(time * 0.5 + i);
      }
    }
  }

  function dispose() {
    nebulaTexture.dispose();
    for (const { sprites } of nebulaSprites) {
      for (const s of sprites) { (s.material as THREE.SpriteMaterial).dispose(); }
    }
    for (const [, label] of clusterLabels) {
      scene.remove(label);
      label.element.remove();
    }
  }

  updateNebulas();

  return { nebulaSprites, updateNebulas, driftNebulas, dispose };
}
