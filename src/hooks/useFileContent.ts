import { useState, useEffect, useRef } from 'react';
import { useIsTauri } from '../shared/api';
import type { FileContentResponse } from '../types';

export function useFileContent(path: string | null) {
  const isTauri = useIsTauri();
  const [data, setData] = useState<FileContentResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cache = useRef(new Map<string, FileContentResponse>());

  useEffect(() => {
    if (!path) {
      setData(null);
      setError(null);
      return;
    }

    const cached = cache.current.get(path);
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
          resp = await invoke('search_read_file', { path });
        } else {
          const r = await fetch(`/api/file?path=${encodeURIComponent(path)}`);
          if (!r.ok) throw new Error(`${r.status}`);
          resp = await r.json();
        }
        if (cancelled) return;
        cache.current.set(path, resp);
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
  }, [path, isTauri]);

  return { data, loading, error };
}
