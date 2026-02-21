import { useState, useEffect, useMemo } from 'react';
import { useApiBase, apiUrl } from '../shared/api';
import { EMPTY_FIND } from '../search-utils';
import type { FindResponse } from '../types';

export function useSearch() {
  const baseUrl = useApiBase();
  const [query, setQuery] = useState('');
  const [findResults, setFindResults] = useState<FindResponse>(EMPTY_FIND);
  const [loading, setLoading] = useState(false);
  const [extFilter, setExtFilter] = useState<string | null>(null);

  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setFindResults(EMPTY_FIND);
      return;
    }

    setLoading(true);
    const controller = new AbortController();

    const timer = setTimeout(() => {
      const params = new URLSearchParams({ q, limit: '50' });
      if (extFilter) params.set('ext', extFilter);

      fetch(apiUrl(baseUrl, `/api/find?${params}`), { signal: controller.signal })
        .then(r => r.json() as Promise<FindResponse>)
        .then(data => {
          setFindResults(data);
          setLoading(false);
        })
        .catch(() => {
          // Aborted or error — don't update state
        });
    }, 150);

    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [query, extFilter, baseUrl]);

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
    loading,
    extFilter,
    setExtFilter,
    topExts,
  };
}
