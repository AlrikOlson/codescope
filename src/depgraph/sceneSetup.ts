import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js';
import { CSS2DRenderer } from 'three/examples/jsm/renderers/CSS2DRenderer.js';

export interface SceneContext {
  renderer: THREE.WebGLRenderer;
  labelRenderer: CSS2DRenderer;
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  composer: EffectComposer;
  controls: OrbitControls;
}

export function createScene(container: HTMLDivElement): { context: SceneContext; resizeObserver: ResizeObserver } {
  const rect = container.getBoundingClientRect();

  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
  renderer.setClearColor(0x1e1e2e);
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.2;
  renderer.setSize(rect.width, rect.height);
  container.appendChild(renderer.domElement);
  renderer.domElement.style.display = 'block';
  renderer.domElement.style.cursor = 'grab';

  const labelRenderer = new CSS2DRenderer();
  labelRenderer.setSize(rect.width, rect.height);
  labelRenderer.domElement.style.position = 'absolute';
  labelRenderer.domElement.style.top = '0';
  labelRenderer.domElement.style.left = '0';
  labelRenderer.domElement.style.pointerEvents = 'none';
  container.appendChild(labelRenderer.domElement);

  const scene = new THREE.Scene();
  scene.fog = new THREE.FogExp2(0x1e1e2e, 0.0008);

  const camera = new THREE.PerspectiveCamera(60, rect.width / rect.height, 1, 5000);
  camera.position.set(0, 0, 800);

  const composer = new EffectComposer(renderer);
  composer.addPass(new RenderPass(scene, camera));
  composer.addPass(new UnrealBloomPass(
    new THREE.Vector2(rect.width, rect.height),
    0.7, 0.4, 0.85,
  ));

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.dampingFactor = 0.1;
  controls.minDistance = 50;
  controls.maxDistance = 3000;

  scene.add(new THREE.AmbientLight(0x404060, 2));
  const dirLight = new THREE.DirectionalLight(0xffffff, 1);
  dirLight.position.set(200, 300, 400);
  scene.add(dirLight);

  const resizeObserver = new ResizeObserver((entries) => {
    const cr = entries[0]?.contentRect;
    if (!cr || cr.width === 0 || cr.height === 0) return;
    camera.aspect = cr.width / cr.height;
    camera.updateProjectionMatrix();
    renderer.setSize(cr.width, cr.height);
    composer.setSize(cr.width, cr.height);
    labelRenderer.setSize(cr.width, cr.height);
  });
  resizeObserver.observe(container);

  return {
    context: { renderer, labelRenderer, scene, camera, composer, controls },
    resizeObserver,
  };
}

export function disposeScene(ctx: SceneContext, container: HTMLDivElement, ro: ResizeObserver): void {
  ro.disconnect();
  ctx.renderer.setAnimationLoop(null);
  ctx.renderer.dispose();
  ctx.composer.dispose();
  if (container.contains(ctx.renderer.domElement)) container.removeChild(ctx.renderer.domElement);
  if (container.contains(ctx.labelRenderer.domElement)) container.removeChild(ctx.labelRenderer.domElement);
}
