import { useState, useEffect, useMemo, useRef } from 'react';
import { useIsTauri } from '../shared/api';
import { EMPTY_FIND } from '../search-utils';
import type { FindResponse } from '../types';

function useDebouncedValue<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return debounced;
}

export function useSearch() {
  const isTauri = useIsTauri();
  const [query, setQuery] = useState('');
  const [findResults, setFindResults] = useState<FindResponse>(EMPTY_FIND);
  const [loading, setLoading] = useState(false);
  const [extFilter, setExtFilter] = useState<string | null>(null);
  const requestId = useRef(0);

  const debouncedQuery = useDebouncedValue(query, 200);

  useEffect(() => {
    const q = debouncedQuery.trim();
    if (!q) {
      setFindResults(EMPTY_FIND);
      setLoading(false);
      return;
    }

    const id = ++requestId.current;
    setLoading(true);
    const controller = new AbortController();

    (async () => {
      try {
        let data: FindResponse;
        if (isTauri) {
          const { invoke } = await import('@tauri-apps/api/core');
          data = await invoke('search_find', {
            q,
            ext: extFilter ?? undefined,
            limit: 50,
          });
        } else {
          const params = new URLSearchParams({ q, limit: '50' });
          if (extFilter) params.set('ext', extFilter);
          const resp = await fetch(`/api/find?${params}`, { signal: controller.signal });
          data = await resp.json() as FindResponse;
        }
        if (id === requestId.current) {
          setFindResults(data);
          setLoading(false);
        }
      } catch {
        if (id === requestId.current) {
          setLoading(false);
        }
      }
    })();

    return () => { controller.abort(); };
  }, [debouncedQuery, extFilter, isTauri]);

  // Derive "typing" state from query vs debounced — no extra render needed
  const isTyping = query.trim() !== '' && query !== debouncedQuery;

  const results = findResults.results;

  const topExts = useMemo(() => {
    return Object.entries(findResults.extCounts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 6);
  }, [findResults.extCounts]);

  return {
    query,
    setQuery,
    findResults,
    results,
    loading: loading || isTyping,
    extFilter,
    setExtFilter,
    topExts,
  };
}
