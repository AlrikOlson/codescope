import { useState, useEffect, useRef } from 'react';

export function useSidebarResize(initialWidth = 320) {
  const [sidebarWidth, setSidebarWidth] = useState(initialWidth);
  const [isDragging, setIsDragging] = useState(false);
  const resizing = useRef(false);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!resizing.current) return;
      setSidebarWidth(Math.max(200, Math.min(600, e.clientX)));
    };
    const onUp = () => {
      resizing.current = false;
      setIsDragging(false);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, []);

  const startResize = () => {
    resizing.current = true;
    setIsDragging(true);
  };

  return { sidebarWidth, startResize, isDragging };
}
