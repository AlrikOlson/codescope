import { useState, useRef, useCallback, useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useSearch } from '../hooks/useSearch';
import { SearchInput } from './components/SearchInput';
import { MetadataStrip } from './components/MetadataStrip';
import { ResultsList } from './components/ResultsList';
import { CodePreview } from './components/CodePreview';
import { ShortcutsBar } from './components/ShortcutsBar';
import { IndexOverview } from './components/IndexOverview';
import type { FindResult } from '../types';

export function SearchWindow() {
  const {
    query, setQuery, findResults, results, loading, extFilter, setExtFilter, topExts,
  } = useSearch();

  const [activeIdx, setActiveIdx] = useState(0);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  // Refs for stable keyboard handler — avoids re-registering on every state change
  const stateRef = useRef<{
    activeIdx: number;
    results: FindResult[];
    query: string;
    extFilter: string | null;
  }>({ activeIdx: 0, results: [], query: '', extFilter: null });
  stateRef.current = { activeIdx, results, query, extFilter };

  const handleClearRef = useRef<() => void>(() => {});

  // Reset active index on new query/filter
  useEffect(() => {
    setActiveIdx(0);
  }, [query, extFilter]);

  // Sync activeIdx/results → selectedPath (single effect replaces two separate ones)
  useEffect(() => {
    if (results.length > 0 && results[activeIdx]) {
      setSelectedPath(results[activeIdx].path);
    } else {
      setSelectedPath(null);
    }
  }, [activeIdx, results]);

  const handleSelect = useCallback((path: string) => {
    setSelectedPath(path);
  }, []);

  const handleClear = useCallback(() => {
    setQuery('');
    setExtFilter(null);
    setSelectedPath(null);
    inputRef.current?.focus();
  }, [setQuery, setExtFilter]);

  handleClearRef.current = handleClear;

  // Lock root scroll — WebView2 may auto-scroll to keep the focused input visible,
  // bypassing overflow: hidden. Force scrollTop = 0 on root + body at all times.
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const lock = () => {
      if (root.scrollTop !== 0) root.scrollTop = 0;
      if (document.documentElement.scrollTop !== 0) document.documentElement.scrollTop = 0;
    };
    root.addEventListener('scroll', lock);
    document.addEventListener('scroll', lock);
    return () => {
      root.removeEventListener('scroll', lock);
      document.removeEventListener('scroll', lock);
    };
  }, []);

  // Global keyboard handler — registered ONCE, reads mutable refs for current state
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const { results, query, extFilter, activeIdx } = stateRef.current;
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActiveIdx(prev => (prev < results.length - 1 ? prev + 1 : 0));
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActiveIdx(prev => (prev > 0 ? prev - 1 : results.length - 1));
      }
      if (e.key === 'Enter' && results.length > 0) {
        const item = results[activeIdx];
        if (item) setSelectedPath(item.path);
      }
      if (e.key === 'Escape') {
        if (extFilter) {
          setExtFilter(null);
        } else if (query) {
          handleClearRef.current();
        } else {
          getCurrentWindow().close();
        }
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const hasQuery = query.trim().length > 0;

  return (
    <div ref={rootRef} className="sw-root">
      {/* Custom titlebar (drag region only, no label — label is inside hero field) */}
      <div className="sw-titlebar" data-tauri-drag-region>
        <div className="sw-titlebar-spacer" data-tauri-drag-region />
        <button className="sw-titlebar-close" onClick={() => getCurrentWindow().close()}>&times;</button>
      </div>

      {/* Hero search input */}
      <SearchInput
        ref={inputRef}
        value={query}
        onChange={setQuery}
        loading={loading}
        onClear={handleClear}
      />

      {/* Metadata strip — always rendered when querying to reserve vertical space */}
      {hasQuery && (
        <MetadataStrip
          count={results.length}
          queryTime={findResults.queryTime}
          topExts={topExts}
          extFilter={extFilter}
          onFilterExt={setExtFilter}
        />
      )}

      {/* Split panel or overview */}
      {hasQuery ? (
        <div className="sw-split">
          <ResultsList
            results={results}
            activeIdx={activeIdx}
            onSetActive={setActiveIdx}
            onSelect={handleSelect}
          />
          <CodePreview path={selectedPath} />
        </div>
      ) : (
        <IndexOverview />
      )}

      {/* Shortcuts bar */}
      <ShortcutsBar />
    </div>
  );
}
