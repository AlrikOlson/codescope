import { useState, useEffect, useRef } from 'react';
import { useIsTauri } from '../shared/api';
import type { FileContentResponse } from '../types';

export function useFileContent(path: string | null) {
  const isTauri = useIsTauri();
  const [data, setData] = useState<FileContentResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cache = useRef(new Map<string, FileContentResponse>());

  // Debounce path changes — skip debounce for cache hits (instant display)
  const [debouncedPath, setDebouncedPath] = useState(path);
  useEffect(() => {
    if (!path || cache.current.has(path)) {
      setDebouncedPath(path);
      return;
    }
    const timer = setTimeout(() => setDebouncedPath(path), 150);
    return () => clearTimeout(timer);
  }, [path]);

  useEffect(() => {
    if (!debouncedPath) {
      setData(null);
      setError(null);
      return;
    }

    const cached = cache.current.get(debouncedPath);
    if (cached) {
      setData(cached);
      setLoading(false);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);

    (async () => {
      try {
        let resp: FileContentResponse;
        if (isTauri) {
          const { invoke } = await import('@tauri-apps/api/core');
          resp = await invoke('search_read_file', { path: debouncedPath });
        } else {
          const r = await fetch(`/api/file?path=${encodeURIComponent(debouncedPath)}`);
          if (!r.ok) throw new Error(`${r.status}`);
          resp = await r.json();
        }
        if (cancelled) return;
        cache.current.set(debouncedPath, resp);
        if (cache.current.size > 50) {
          const first = cache.current.keys().next().value;
          if (first) cache.current.delete(first);
        }
        setData(resp);
      } catch (err: any) {
        if (!cancelled) setError(err.message ?? String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => { cancelled = true; };
  }, [debouncedPath, isTauri]);

  // Return cached data for current path even before debounce settles
  const cachedCurrent = path ? cache.current.get(path) ?? null : null;

  return {
    data: cachedCurrent ?? data,
    loading: (path !== debouncedPath && !cachedCurrent) || loading,
    error,
  };
}
