import { useState, useCallback, useEffect, useRef } from 'react';

export function usePreviewResize(initialWidth = 480) {
  const [previewWidth, setPreviewWidth] = useState(initialWidth);
  const [isDragging, setIsDragging] = useState(false);
  const startXRef = useRef(0);
  const startWidthRef = useRef(initialWidth);

  const startResize = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    startXRef.current = e.clientX;
    startWidthRef.current = previewWidth;
    setIsDragging(true);
  }, [previewWidth]);

  useEffect(() => {
    if (!isDragging) return;

    const onMove = (e: MouseEvent) => {
      // Moving left increases width (resize handle is on left side of preview)
      const delta = startXRef.current - e.clientX;
      const newWidth = Math.min(800, Math.max(320, startWidthRef.current + delta));
      setPreviewWidth(newWidth);
    };

    const onUp = () => setIsDragging(false);

    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    return () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
    };
  }, [isDragging]);

  return { previewWidth, startResize, isDragging };
}
