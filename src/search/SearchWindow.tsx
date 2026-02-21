import { useState, useRef, useCallback, useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useSearch } from '../hooks/useSearch';
import { SearchInput } from './components/SearchInput';
import { MetadataStrip } from './components/MetadataStrip';
import { ResultsList } from './components/ResultsList';
import { CodePreview } from './components/CodePreview';
import { ShortcutsBar } from './components/ShortcutsBar';

export function SearchWindow() {
  const {
    query, setQuery, findResults, results, loading, extFilter, setExtFilter, topExts,
  } = useSearch();

  const [activeIdx, setActiveIdx] = useState(0);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Reset active index on new results
  useEffect(() => {
    setActiveIdx(0);
  }, [query, extFilter]);

  // Auto-preview first result
  useEffect(() => {
    if (results.length > 0) {
      setSelectedPath(results[0].path);
    } else {
      setSelectedPath(null);
    }
  }, [results]);

  const handleSelect = useCallback((path: string) => {
    setSelectedPath(path);
  }, []);

  const handleClear = useCallback(() => {
    setQuery('');
    setExtFilter(null);
    setSelectedPath(null);
    inputRef.current?.focus();
  }, [setQuery, setExtFilter]);

  // Global keyboard navigation
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActiveIdx(prev => {
          const next = prev < results.length - 1 ? prev + 1 : 0;
          return next;
        });
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActiveIdx(prev => {
          const next = prev > 0 ? prev - 1 : results.length - 1;
          return next;
        });
      }
      if (e.key === 'Enter' && results.length > 0) {
        const item = results[activeIdx];
        if (item) setSelectedPath(item.path);
      }
      if (e.key === 'Escape') {
        if (extFilter) {
          setExtFilter(null);
        } else if (query) {
          handleClear();
        } else {
          getCurrentWindow().close();
        }
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [activeIdx, results, query, extFilter, setExtFilter, handleClear]);

  // Sync activeIdx → selectedPath
  useEffect(() => {
    if (results.length > 0 && results[activeIdx]) {
      setSelectedPath(results[activeIdx].path);
    }
  }, [activeIdx, results]);

  const hasQuery = query.trim().length > 0;

  return (
    <div className="sw-root">
      {/* Custom titlebar */}
      <div className="sw-titlebar" data-tauri-drag-region>
        <span className="sw-titlebar-label" data-tauri-drag-region>CodeScope Search</span>
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

      {/* Metadata strip */}
      {hasQuery && results.length > 0 && (
        <MetadataStrip
          count={results.length}
          queryTime={findResults.queryTime}
          topExts={topExts}
          extFilter={extFilter}
          onFilterExt={setExtFilter}
        />
      )}

      {/* Split panel */}
      <div className="sw-split">
        <ResultsList
          results={results}
          activeIdx={activeIdx}
          onSetActive={setActiveIdx}
          onSelect={handleSelect}
        />
        <CodePreview path={selectedPath} />
      </div>

      {/* Shortcuts bar */}
      <ShortcutsBar />
    </div>
  );
}
