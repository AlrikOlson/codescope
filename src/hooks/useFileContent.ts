import { useState, useEffect, useRef } from 'react';
import { useApiBase, apiUrl } from '../shared/api';
import type { FileContentResponse } from '../types';

export function useFileContent(path: string | null) {
  const baseUrl = useApiBase();
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

    fetch(apiUrl(baseUrl, `/api/file?path=${encodeURIComponent(path)}`))
      .then(r => {
        if (!r.ok) throw new Error(`${r.status}`);
        return r.json();
      })
      .then((resp: FileContentResponse) => {
        if (cancelled) return;
        cache.current.set(path, resp);
        // LRU: keep cache under 50 entries
        if (cache.current.size > 50) {
          const first = cache.current.keys().next().value;
          if (first) cache.current.delete(first);
        }
        setData(resp);
      })
      .catch(err => {
        if (!cancelled) setError(err.message);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => { cancelled = true; };
  }, [path, baseUrl]);

  return { data, loading, error };
}
