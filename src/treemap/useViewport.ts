import { useRef, useCallback, useEffect } from 'react';
import type { Viewport, TreemapNode } from './types';

const MIN_SCALE = 0.5;
const MAX_SCALE = 500;
const ZOOM_SPEED = 0.002;

export interface ViewportAPI {
  viewportRef: React.MutableRefObject<Viewport>;
  resetView: () => void;
  zoomToNode: (node: TreemapNode) => void;
  bind: (canvas: HTMLCanvasElement) => () => void;
}

export function useViewport(
  onFrame: () => void,
  canvasSize: () => { w: number; h: number },
): ViewportAPI {
  const viewportRef = useRef<Viewport>({ x: 0, y: 0, scale: 1 });
  const animRef = useRef<number>(0);
  const dragRef = useRef<{ startX: number; startY: number; vpX: number; vpY: number; moved: boolean } | null>(null);
  const frameReq = useRef<number>(0);
  const needsDraw = useRef(true);

  const requestDraw = useCallback(() => {
    needsDraw.current = true;
  }, []);

  // Animation loop
  useEffect(() => {
    let running = true;
    const loop = () => {
      if (!running) return;
      if (needsDraw.current) {
        needsDraw.current = false;
        onFrame();
      }
      frameReq.current = requestAnimationFrame(loop);
    };
    frameReq.current = requestAnimationFrame(loop);
    return () => { running = false; cancelAnimationFrame(frameReq.current); };
  }, [onFrame]);

  const animateTo = useCallback((target: Viewport, duration = 300) => {
    cancelAnimationFrame(animRef.current);
    const start = { ...viewportRef.current };
    const t0 = performance.now();
    const step = () => {
      const elapsed = performance.now() - t0;
      const t = Math.min(1, elapsed / duration);
      const ease = 1 - (1 - t) * (1 - t) * (1 - t); // ease-out cubic
      viewportRef.current = {
        x: start.x + (target.x - start.x) * ease,
        y: start.y + (target.y - start.y) * ease,
        scale: start.scale + (target.scale - start.scale) * ease,
      };
      needsDraw.current = true;
      if (t < 1) animRef.current = requestAnimationFrame(step);
    };
    animRef.current = requestAnimationFrame(step);
  }, []);

  const resetView = useCallback(() => {
    animateTo({ x: 0, y: 0, scale: 1 });
  }, [animateTo]);

  const zoomToNode = useCallback((node: TreemapNode) => {
    const { w, h } = canvasSize();
    if (w === 0 || h === 0) return;
    const padding = 20;
    const scaleX = (w - padding * 2) / node.w;
    const scaleY = (h - padding * 2) / node.h;
    const scale = Math.min(scaleX, scaleY, MAX_SCALE);
    const x = -node.x * scale + (w - node.w * scale) / 2;
    const y = -node.y * scale + (h - node.h * scale) / 2;
    animateTo({ x, y, scale });
  }, [animateTo, canvasSize]);

  const bind = useCallback((canvas: HTMLCanvasElement) => {
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const vp = viewportRef.current;
      const delta = -e.deltaY * ZOOM_SPEED;
      const factor = Math.exp(delta);
      const newScale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, vp.scale * factor));
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      // Zoom toward cursor
      viewportRef.current = {
        scale: newScale,
        x: mx - (mx - vp.x) * (newScale / vp.scale),
        y: my - (my - vp.y) * (newScale / vp.scale),
      };
      needsDraw.current = true;
    };

    const onMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      dragRef.current = {
        startX: e.clientX, startY: e.clientY,
        vpX: viewportRef.current.x, vpY: viewportRef.current.y,
        moved: false,
      };
    };

    const onMouseMove = (e: MouseEvent) => {
      if (!dragRef.current) return;
      const dx = e.clientX - dragRef.current.startX;
      const dy = e.clientY - dragRef.current.startY;
      if (Math.abs(dx) > 3 || Math.abs(dy) > 3) dragRef.current.moved = true;
      viewportRef.current = {
        ...viewportRef.current,
        x: dragRef.current.vpX + dx,
        y: dragRef.current.vpY + dy,
      };
      needsDraw.current = true;
    };

    const onMouseUp = () => {
      dragRef.current = null;
    };

    canvas.addEventListener('wheel', onWheel, { passive: false });
    canvas.addEventListener('mousedown', onMouseDown);
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);

    return () => {
      canvas.removeEventListener('wheel', onWheel);
      canvas.removeEventListener('mousedown', onMouseDown);
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };
  }, []);

  return { viewportRef, resetView, zoomToNode, bind };
}

// Utility: check if a click was a drag
export function wasDrag(dragRef: React.MutableRefObject<{ moved: boolean } | null>): boolean {
  return dragRef.current?.moved ?? false;
}
